//! Saving and discarding drafts: the local cache first, so typing and
//! closing never wait on the network, then the server's Drafts folder behind
//! a per-account FIFO lock. The MIME itself is built in `mail::draft`.

use tauri::{AppHandle, Emitter};

use crate::auth::Credential;
use crate::error::AppError;
use crate::models::{Account, MessagesChanged, OutgoingMessage};
use crate::state::AppState;
use crate::{mail, storage};

/// Save one compose draft into its account's Drafts folder on the server,
/// where webmail and other clients see it. Returns the Message-ID of the
/// saved version — the handle for replacing or deleting it later. IMAP has
/// no edit-in-place, so each save appends a fresh version and then deletes
/// the previous one (`previous_draft_id`).
pub async fn save_draft(
    app: &AppHandle,
    state: &AppState,
    message: OutgoingMessage,
    previous_draft_id: Option<String>,
) -> Result<String, AppError> {
    let account = storage::accounts::get(&state.pool, message.account_id).await?;
    let Some(drafts) =
        storage::mailboxes::name_for_role(&state.pool, message.account_id, "drafts").await?
    else {
        return Err(AppError::Imap(
            "no drafts folder discovered yet".to_string(),
        ));
    };
    let message_id = mail::draft::generate_message_id();
    let (from_name, from_email) = storage::aliases::sender(&state.pool, &message).await?;
    let raw = mail::draft::build_draft(&from_name, &from_email, &message, &message_id).await?;

    // LOCAL-FIRST: mirror the new version into the cache and notify the UI
    // before touching the network — typing, closing and the conversation
    // view must never wait on the server. The provisional row carries a
    // negative uid (invisible to sync bookkeeping); the background push
    // below replaces it with the mirrored server row.
    let mut header = mail::sync::to_provisional_header(
        &raw,
        -provisional_uid(),
        !message.attachments.is_empty(),
    );
    let parsed = mail::parse::parse_body(&raw);
    header.snippet = parsed.snippet.clone();
    storage::messages::upsert_headers(&state.pool, account.id, &drafts, &[header]).await?;
    // why the gate: a text-only draft must not cache mail-parser's
    // synthesized HTML — the reopen path would mistake the conversion
    // for authored content.
    let authored_html = message
        .body_html
        .is_some()
        .then_some(parsed.html.as_deref())
        .flatten();
    if let Some(row_id) =
        storage::messages::find_by_message_id(&state.pool, account.id, &drafts, &message_id).await?
    {
        storage::messages::set_body(
            &state.pool,
            row_id,
            parsed.text.as_deref(),
            authored_html,
            &parsed.snippet,
            &parsed.images,
            &parsed.attachments,
            parsed.auth.as_ref(),
        )
        .await?;
    }
    // The replaced version leaves the cache with the same immediacy.
    if let Some(old_id) = &previous_draft_id {
        storage::messages::delete_by_message_id(&state.pool, account.id, old_id).await?;
    }
    let _ = app.emit("messages-changed", MessagesChanged::reload(account.id));

    // Server push in the background, serialized per account (FIFO lock) so
    // an autosave burst appends and deletes in save order.
    let credential = state.credential(&account).await?;
    let lock = state.draft_push_lock(account.id);
    let pool = state.pool.clone();
    let app = app.clone();
    let pushed_id = message_id.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = lock.lock().await;
        if let Err(err) = push_draft(
            &app,
            &pool,
            &account,
            &credential,
            &drafts,
            &raw,
            &pushed_id,
            previous_draft_id.as_deref(),
        )
        .await
        {
            // The draft is safe in the local cache and reopens from it; the
            // next save (same window or after reopen) pushes again.
            eprintln!("draft push for account {} failed: {err}", account.id);
        }
    });
    Ok(message_id)
}

/// One provisional-row uid magnitude: epoch nanos, unique for any two saves
/// this side of a clock jump backwards within the same nanosecond.
fn provisional_uid() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(1)
}

/// Server side of a draft save: APPEND the new version, drop the previous
/// one, mirror the folder, then retire the provisional local row. Runs
/// under the account's draft-push lock.
#[allow(clippy::too_many_arguments)]
async fn push_draft(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    account: &Account,
    credential: &Credential,
    drafts: &str,
    raw: &[u8],
    message_id: &str,
    previous_draft_id: Option<&str>,
) -> Result<(), AppError> {
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        credential,
    )
    .await?;
    // why \Seen: the user wrote this text — it must not light up unread
    // badges here or in other clients.
    let appended = mail::imap::append(&mut session, drafts, "(\\Draft \\Seen)", raw).await;
    if appended.is_ok() {
        // why best effort: the new version is safely on the server; a
        // leftover old version is a cosmetic duplicate the user can delete,
        // never lost text. The next save also retries it via its own id.
        if let Some(old_id) = previous_draft_id {
            if let Err(err) = delete_draft_version(&mut session, drafts, old_id).await {
                eprintln!("could not delete draft version {old_id}: {err}");
            }
        }
        match mail::sync::sync_mailbox(pool, account.id, &mut session, drafts).await {
            Ok(_) => {
                // The server copy is mirrored — swap it in for the
                // provisional row, keeping the cached body.
                if let Err(err) =
                    storage::messages::adopt_provisional_draft(pool, account.id, drafts, message_id)
                        .await
                {
                    eprintln!("could not adopt provisional draft {message_id}: {err}");
                }
                let _ = app.emit("messages-changed", MessagesChanged::reload(account.id));
            }
            Err(err) => eprintln!("drafts sync after save failed: {err}"),
        }
    }
    let _ = session.logout().await;
    appended
}

/// "Don't Save" on compose close, and the draft card's Delete: remove the
/// draft locally right away, then from the server in the background. A
/// missing folder or version is fine — the state the user asked for (no
/// draft) already holds.
pub async fn discard_draft(
    app: &AppHandle,
    state: &AppState,
    account_id: i64,
    draft_message_id: String,
) -> Result<(), AppError> {
    let account = storage::accounts::get(&state.pool, account_id).await?;
    let Some(drafts) = storage::mailboxes::name_for_role(&state.pool, account_id, "drafts").await?
    else {
        return Ok(()); // no drafts folder, nothing to clean up
    };
    // LOCAL-FIRST: the card leaves the conversation and the list now.
    storage::messages::delete_by_message_id(&state.pool, account_id, &draft_message_id).await?;
    let _ = app.emit("messages-changed", MessagesChanged::reload(account_id));

    // Behind the same FIFO lock as saves — a discard must never overtake
    // the push that is still appending the version it deletes.
    let credential = state.credential(&account).await?;
    let lock = state.draft_push_lock(account_id);
    let pool = state.pool.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = lock.lock().await;
        if let Err(err) = delete_draft_on_server(
            &app,
            &pool,
            &account,
            &credential,
            &drafts,
            &draft_message_id,
        )
        .await
        {
            eprintln!("draft delete for account {} failed: {err}", account.id);
        }
    });
    Ok(())
}

/// Remove one autosaved draft version after a send, so it stops looking
/// like unfinished work: local cache immediately, then the server copy
/// (behind the account's draft-push FIFO lock — the version might still be
/// mid-append).
pub(crate) async fn delete_sent_draft(
    app: &AppHandle,
    state: &AppState,
    account: &Account,
    credential: &Credential,
    draft_id: &str,
) -> Result<(), AppError> {
    let Some(drafts) = storage::mailboxes::name_for_role(&state.pool, account.id, "drafts").await?
    else {
        return Ok(()); // no drafts folder, nothing to clean up
    };
    storage::messages::delete_by_message_id(&state.pool, account.id, draft_id).await?;
    let _ = app.emit("messages-changed", MessagesChanged::reload(account.id));
    let lock = state.draft_push_lock(account.id);
    let _guard = lock.lock().await;
    delete_draft_on_server(app, &state.pool, account, credential, &drafts, draft_id).await
}

/// Connect, delete one draft version, and reconcile the folder mirror.
/// Callers hold the account's draft-push lock.
async fn delete_draft_on_server(
    app: &AppHandle,
    pool: &sqlx::SqlitePool,
    account: &Account,
    credential: &Credential,
    drafts: &str,
    draft_id: &str,
) -> Result<(), AppError> {
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        credential,
    )
    .await?;
    let result = delete_draft_version(&mut session, drafts, draft_id).await;
    if result.is_ok() {
        match mail::sync::sync_mailbox(pool, account.id, &mut session, drafts).await {
            Ok(_) => {
                let _ = app.emit("messages-changed", MessagesChanged::reload(account.id));
            }
            Err(err) => eprintln!("drafts sync after delete failed: {err}"),
        }
    }
    let _ = session.logout().await;
    result
}

/// Delete one draft version by Message-ID from `drafts`, if it still exists.
async fn delete_draft_version(
    session: &mut mail::imap::ImapSession,
    drafts: &str,
    message_id: &str,
) -> Result<(), AppError> {
    session
        .select(drafts)
        .await
        .map_err(|e| AppError::Imap(format!("select {drafts}: {e}")))?;
    if let Some(uid) = mail::imap::find_by_message_id(session, message_id).await? {
        mail::imap::delete_message(session, uid).await?;
    }
    Ok(())
}
