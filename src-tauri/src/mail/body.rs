//! Message bodies for the viewer: loaded from the cache (fetched from the
//! server on a miss) and rendered through the sanitizer.
//!
//! SECURITY: this is the email-body rendering path (CLAUDE.md hard rule) —
//! every body the frontend gets is built here, by `mail::sanitize`.

use tauri::{AppHandle, Emitter};

use crate::clock::now_epoch;
use crate::error::AppError;
use crate::models::{AuthResults, MessageBody, MessageQuote, MessagesChanged, RemoteImagePolicy};
use crate::state::AppState;
use crate::timing;
use crate::{llm, mail, storage};

/// Body for the viewer — served from cache, lazily fetched on first open.
/// `load_remote` is the per-message "Load images" click; it only has an
/// effect under the Ask policy (Block ignores it, Always needs no click).
pub async fn get_message_body(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    load_remote: Option<bool>,
) -> Result<MessageBody, AppError> {
    let _t = timing::start("cmd::get_message_body");
    let loaded = load_body(app, state, message_id).await?;
    // Shown now: its HTML's retention period starts over.
    storage::messages::touch_html(&state.pool, message_id, now_epoch()).await?;

    let redundant = quote_repeats_thread(&state.pool, message_id, loaded.text.as_deref()).await?;
    let mut body = sanitized_body(
        &state.pool,
        loaded.html,
        loaded.text,
        &loaded.images,
        load_remote,
        redundant,
        None,
    )
    .await?;
    // why from the DB, not the parse result: both branches above have already
    // written the metadata (set_body), so one read serves cached and fresh.
    body.attachments = storage::messages::attachments(&state.pool, message_id).await?;
    body.auth = loaded.auth;
    let own = storage::accounts::own_emails(&state.pool).await?;
    body.sender_anomaly =
        storage::contacts::sender_anomaly(&state.pool, &loaded.from, &own).await?;
    Ok(body)
}

/// Whether this message's quoted history just repeats an earlier cached
/// message of its conversation — such a quote renders nothing at all (the
/// content sits one card up in the thread view).
pub(crate) async fn quote_repeats_thread(
    pool: &sqlx::SqlitePool,
    message_id: i64,
    text: Option<&str>,
) -> Result<bool, AppError> {
    let _t = timing::start("cmd::quote_repeats_thread");
    let Some(text) = text else { return Ok(false) };
    let (_, Some(quoted)) = mail::quote::split_text_quote(text) else {
        return Ok(false);
    };
    let thread = storage::messages::thread_of(pool, message_id).await?;
    let mut earlier = Vec::new();
    for header in &thread {
        if header.id == message_id {
            break;
        }
        let row = storage::messages::get_body(pool, header.id).await?;
        if let Some(body_text) = row.body_text {
            earlier.push(body_text);
        }
    }
    Ok(mail::quote::quote_matches_history(&quoted, &earlier))
}

/// Quote material for a reply to this message — served from the same body
/// cache as the viewer; a miss fetches the body first. Thin wrapper over
/// mail::quote::quote_material.
pub async fn get_message_quote(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<MessageQuote, AppError> {
    let loaded = load_body(app, state, message_id).await?;
    Ok(mail::quote::quote_material(
        loaded.html.as_deref(),
        loaded.text.as_deref(),
        &loaded.images,
    ))
}

/// One message's body as load_body hands it out: the cached (or freshly
/// fetched) content plus the metadata that rides along with it.
pub(crate) struct LoadedBody {
    pub(crate) html: Option<String>,
    pub(crate) text: Option<String>,
    pub(crate) images: Vec<mail::parse::InlineImage>,
    /// SPF/DKIM/DMARC verdicts; None = unknown.
    auth: Option<AuthResults>,
    /// The stored From line ("Name <addr>"), for the sender check.
    from: String,
}

/// Cached AuthResults JSON back into the struct; unreadable or absent
/// means "unknown", never an error.
fn parse_auth(json: Option<&str>) -> Option<AuthResults> {
    json.and_then(|j| serde_json::from_str(j).ok())
}

/// A message's raw cached body — HTML, text and inline images — fetched
/// from the server on a cache miss, refetched once for cache rows written
/// before message_images/message_attachments existed (they'd otherwise
/// render blanks/nothing forever), and refetched when retention dropped the
/// HTML. When a refetch fails but text is cached (offline, say), the text
/// is served instead of an error.
pub(crate) async fn load_body(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<LoadedBody, AppError> {
    let row = storage::messages::get_body(&state.pool, message_id).await?;

    let cached = row.body_text.is_some() || row.body_html.is_some();
    let backfill = cached
        && (!row.attachments_scanned
            || (row.body_html.as_deref().is_some_and(|h| h.contains("cid:"))
                && storage::messages::images(&state.pool, message_id)
                    .await?
                    .is_empty()));

    if cached && !backfill && !row.html_dropped {
        let images = storage::messages::images(&state.pool, message_id).await?;
        return Ok(LoadedBody {
            html: row.body_html,
            text: row.body_text,
            images,
            auth: parse_auth(row.auth_results.as_deref()),
            from: row.from_addr,
        });
    }
    let fetched = async {
        let account = storage::accounts::get(&state.pool, row.account_id).await?;
        let credential = state.credential(&account).await?;
        mail::sync::fetch_body_into_cache(
            &state.pool,
            &account,
            &credential,
            message_id,
            &row.mailbox,
            row.uid,
        )
        .await
    }
    .await;
    let parsed = match fetched {
        Ok(parsed) => parsed,
        // why fall back only when something is cached: with nothing to show
        // the error is the answer; with the text still here (HTML dropped by
        // retention, or a legacy row), a read that works beats a failure.
        Err(err) if cached => {
            eprintln!("could not refetch the body of message {message_id}: {err}");
            let images = storage::messages::images(&state.pool, message_id).await?;
            return Ok(LoadedBody {
                html: row.body_html,
                text: row.body_text,
                images,
                auth: parse_auth(row.auth_results.as_deref()),
                from: row.from_addr,
            });
        }
        Err(err) => return Err(err),
    };
    // why: the snippet just became real — lists should refresh.
    app.emit("messages-changed", MessagesChanged::reload(row.account_id))?;
    Ok(LoadedBody {
        html: parsed.html,
        text: parsed.text,
        images: parsed.images,
        auth: parsed.auth,
        from: row.from_addr,
    })
}

/// The message's whole text, plain — quoted history included — fetching
/// the body if it is not cached yet. What the summaries read.
pub(crate) async fn plain_text(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<String, AppError> {
    let body = load_body(app, state, message_id).await?;
    // why the html fallback: mail-parser fills body_text for html-only mail
    // at parse time, so this is for the odd cached row that has only html.
    Ok(body
        .text
        .or_else(|| {
            body.html
                .as_deref()
                .map(mail_parser::decoders::html::html_to_text)
        })
        .unwrap_or_default())
}

/// Bodies for a whole conversation in one call — the conversation view's
/// bulk load. Cached bodies come straight from SQLite; everything missing
/// is fetched over a SINGLE IMAP session, grouped by folder, instead of one
/// TLS connect per message (which made long threads crawl open).
pub async fn thread_bodies(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<std::collections::HashMap<i64, MessageBody>, AppError> {
    let _t = timing::start("cmd::thread_bodies");
    let thread = storage::messages::thread_of(&state.pool, message_id).await?;

    // Members to download, grouped by the folder to select: those with no
    // cached body yet, and those whose HTML retention dropped.
    let mut missing: std::collections::BTreeMap<String, Vec<(i64, i64)>> = Default::default();
    let mut fetch_account = None;
    // True when some member has nothing cached at all — then a failed
    // download is an error. Otherwise every member still has its text.
    let mut must_fetch = false;
    for header in &thread {
        let row = storage::messages::get_body(&state.pool, header.id).await?;
        let empty = row.body_text.is_none() && row.body_html.is_none();
        if empty || row.html_dropped {
            must_fetch |= empty;
            fetch_account = Some(row.account_id);
            missing
                .entry(row.mailbox)
                .or_default()
                .push((header.id, row.uid));
        }
    }

    if let Some(account_id) = fetch_account {
        let downloaded = async {
            let account = storage::accounts::get(&state.pool, account_id).await?;
            let credential = state.credential(&account).await?;
            let mut session = mail::imap::connect(
                &account.imap_host,
                account.imap_port,
                &account.username,
                &credential,
            )
            .await?;
            let fetched = download_thread_bodies(&state.pool, &mut session, &missing).await;
            let _ = session.logout().await;
            fetched
        }
        .await;
        match downloaded {
            // why: snippets just became real — lists should refresh.
            Ok(()) => app.emit("messages-changed", MessagesChanged::reload(account_id))?,
            // Offline, say: every member still has its text to show.
            Err(err) if !must_fetch => {
                eprintln!("could not refetch conversation bodies: {err}");
            }
            Err(err) => return Err(err),
        }
    }

    let own = storage::accounts::own_emails(&state.pool).await?;
    let mut bodies = std::collections::HashMap::new();
    // Oldest first (thread_of order): each message's quote is compared to
    // the bodies before it — a quote that repeats one renders nothing.
    let mut earlier_texts: Vec<String> = Vec::new();
    for header in &thread {
        let row = storage::messages::get_body(&state.pool, header.id).await?;
        let images = storage::messages::images(&state.pool, header.id).await?;
        let redundant = row
            .body_text
            .as_deref()
            .and_then(|t| mail::quote::split_text_quote(t).1)
            .is_some_and(|q| mail::quote::quote_matches_history(&q, &earlier_texts));
        if let Some(body_text) = &row.body_text {
            earlier_texts.push(body_text.clone());
        }
        let auth = parse_auth(row.auth_results.as_deref());
        let mut body = sanitized_body(
            &state.pool,
            row.body_html,
            row.body_text,
            &images,
            None,
            redundant,
            None,
        )
        .await?;
        body.attachments = storage::messages::attachments(&state.pool, header.id).await?;
        body.auth = auth;
        body.sender_anomaly =
            storage::contacts::sender_anomaly(&state.pool, &header.from, &own).await?;
        bodies.insert(header.id, body);
    }
    // Shown now: every member's HTML retention period starts over.
    let now = now_epoch();
    for header in &thread {
        storage::messages::touch_html(&state.pool, header.id, now).await?;
    }
    Ok(bodies)
}

/// Download and cache the bodies listed in `missing` (folder → (row id,
/// uid)) over one session, one SELECT per folder.
async fn download_thread_bodies(
    pool: &sqlx::SqlitePool,
    session: &mut mail::imap::ImapSession,
    missing: &std::collections::BTreeMap<String, Vec<(i64, i64)>>,
) -> Result<(), AppError> {
    for (mailbox, entries) in missing {
        session
            .select(mailbox)
            .await
            .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
        for (id, uid) in entries {
            // why skip: a uid can vanish mid-run (deleted elsewhere) —
            // the rest of the conversation must still load.
            let Some(raw) = mail::imap::fetch_body(session, *uid).await? else {
                continue;
            };
            let parsed = mail::parse::parse_body(&raw);
            storage::messages::set_body(
                pool,
                *id,
                parsed.text.as_deref(),
                parsed.html.as_deref(),
                &parsed.snippet,
                &parsed.images,
                &parsed.attachments,
                parsed.auth.as_ref(),
            )
            .await?;
        }
    }
    Ok(())
}

// SECURITY: the single place message HTML is prepared for the frontend —
// everything goes through mail::sanitize::build_srcdoc, cached or fresh.
// Remote images load only under policy Always or an explicit Ask-click;
// under Block a stray load_remote from the frontend changes nothing. The
// policy is read here, next to where it is enforced.
async fn sanitized_body(
    pool: &sqlx::SqlitePool,
    html: Option<String>,
    text: Option<String>,
    images: &[mail::parse::InlineImage],
    load_remote: Option<bool>,
    redundant_quote: bool,
    translations: Option<&std::collections::HashMap<String, String>>,
) -> Result<MessageBody, AppError> {
    let inputs = render_inputs(pool, html.as_deref(), load_remote).await?;
    Ok(render_body(
        html.as_deref(),
        text.as_deref(),
        images,
        &inputs,
        redundant_quote,
        translations,
    ))
}

/// What rendering a body needs besides the message: the remote-image
/// decision and the images fetched for it. Settled once — it awaits the
/// settings and maybe the network — so the body can then be rendered again
/// without waiting, as a translation does after every batch.
pub(crate) struct RenderInputs {
    policy: RemoteImagePolicy,
    load: bool,
    remote: std::collections::HashMap<String, String>,
}

pub(crate) async fn render_inputs(
    pool: &sqlx::SqlitePool,
    html: Option<&str>,
    load_remote: Option<bool>,
) -> Result<RenderInputs, AppError> {
    let policy = storage::settings::remote_image_policy(pool).await?;
    let load = policy == RemoteImagePolicy::Always
        || (policy == RemoteImagePolicy::Ask && load_remote.unwrap_or(false));
    let remote = match html {
        Some(h) if load => {
            let urls = mail::sanitize::remote_image_urls(h);
            mail::remote::load_images(pool, &urls).await
        }
        _ => std::collections::HashMap::new(),
    };
    Ok(RenderInputs {
        policy,
        load,
        remote,
    })
}

/// The body as the viewer gets it, from settled `inputs` — see
/// sanitized_body for the rules it keeps.
pub(crate) fn render_body(
    html: Option<&str>,
    text: Option<&str>,
    images: &[mail::parse::InlineImage],
    inputs: &RenderInputs,
    redundant_quote: bool,
    translations: Option<&std::collections::HashMap<String, String>>,
) -> MessageBody {
    let mut blocked_images = 0;
    let rendered = html.map(|h| {
        let body =
            mail::sanitize::build_srcdoc(h, images, &inputs.remote, redundant_quote, translations);
        blocked_images = body.blocked_remote;
        body.html
    });

    // Text bodies fold their quoted history on the frontend — split here so
    // the detection lives in one place (mail::quote). A quote the caller
    // verified as repeating an earlier thread message renders nothing.
    let mut language = None;
    let (text, quoted_text) = match text {
        Some(t) => {
            let (own, quoted) = mail::quote::split_text_quote(t);
            language = llm::summarize::detect_language(&own);
            let own = match translations {
                Some(translations) => mail::translate::apply_text(&own, translations),
                None => own,
            };
            (Some(own), if redundant_quote { None } else { quoted })
        }
        None => (None, None),
    };

    MessageBody {
        html: rendered,
        text,
        quoted_text,
        blocked_images,
        // why: !load, not just Ask — after the click the banner disappears
        // even when some images failed to fetch (no endless "load" loop).
        can_load_remote: inputs.policy == RemoteImagePolicy::Ask
            && !inputs.load
            && blocked_images > 0,
        // Filled by the callers from the DB — sanitization has no say.
        attachments: Vec::new(),
        auth: None,
        sender_anomaly: None,
        language,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rendered_body_names_the_language_of_its_own_text() {
        let inputs = RenderInputs {
            policy: RemoteImagePolicy::Block,
            load: false,
            remote: std::collections::HashMap::new(),
        };
        let text = "Hallo Anna,\n\nich schicke dir die Rechnung für August. Bitte überweise \
                    den Betrag bis Ende des Monats.\n\nOn Mon, Bob wrote:\n> Hi, any news?";

        let body = render_body(None, Some(text), &[], &inputs, false, None);
        assert_eq!(body.language.as_deref(), Some("German"));

        let short = render_body(None, Some("ok"), &[], &inputs, false, None);
        assert_eq!(short.language, None);
    }
}
