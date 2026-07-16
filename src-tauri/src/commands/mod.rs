use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppError;
use crate::models::{
    Account, Mailbox, MessageBody, MessageHeader, NewAccount, OutgoingMessage, RemoteImagePolicy,
};
use crate::state::AppState;
use crate::{auth, mail, storage};

// why: commands stay thin — validate/orchestrate, call a module, return
// Result. Business logic lives in storage/ and auth/, which are unit-tested.

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, AppError> {
    storage::accounts::list(&state.pool).await
}

#[tauri::command]
pub async fn add_account(
    app: AppHandle,
    state: State<'_, AppState>,
    account: NewAccount,
    password: String,
) -> Result<Account, AppError> {
    let inserted = storage::accounts::insert(&state.pool, &account).await?;
    // why: the keychain write can fail (locked keychain, denied prompt) — roll
    // the row back so no account can exist without a stored credential.
    if let Err(err) = auth::set_password(inserted.id, password.clone()).await {
        storage::accounts::delete(&state.pool, inserted.id).await?;
        return Err(err);
    }
    // why: seed the session cache — the first sync then needs no keychain
    // read (and no macOS prompt) at all.
    state.passwords.insert(inserted.id, password);
    // why: broadcast to every window — the settings window mutates accounts,
    // the main window listens and refetches its sidebar list.
    app.emit("accounts-changed", ())?;
    Ok(inserted)
}

/// Set (or clear) an account's accent color, then broadcast so the main
/// window re-tints its sidebar and message dots.
#[tauri::command]
pub async fn set_account_color(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    color: Option<String>,
) -> Result<(), AppError> {
    storage::accounts::set_color(&state.pool, id, color.as_deref()).await?;
    app.emit("accounts-changed", ())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), AppError> {
    // why: keychain first — if it fails the account stays intact; the reverse
    // order could strand a secret in the keychain with no owning account row.
    auth::delete_password(id).await?;
    state.passwords.remove(id);
    storage::accounts::delete(&state.pool, id).await?;
    app.emit("accounts-changed", ())?;
    Ok(())
}

/// Sync one account into the local cache: folder list + every folder's
/// headers.
#[tauri::command]
pub async fn sync_account(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: i64,
) -> Result<(), AppError> {
    let account = storage::accounts::get(&state.pool, account_id).await?;
    // why: the password comes from the session cache (one keychain read per
    // account per run) and is handed on to the prefetch task below — never
    // written to state beyond the cache, events, or logs.
    let result = async {
        let password = state.password(account_id).await?;
        mail::sync::sync_account(&state.pool, &account, &password).await?;
        Ok::<String, AppError>(password)
    }
    .await;

    // why: every sync doubles as a health check — the recorded outcome is
    // what settings shows when credentials go stale server-side.
    let error_text = result.as_ref().err().map(ToString::to_string);
    storage::accounts::set_status(&state.pool, account_id, error_text.as_deref(), now_epoch())
        .await?;
    app.emit("accounts-changed", ())?;

    let password = result?;
    app.emit("messages-changed", account_id)?;

    // why: bodies download in the background AFTER the command returns — the
    // header list is already usable, and each cached body feeds the FTS index
    // so search covers unopened mail.
    let pool = state.pool.clone();
    tauri::async_runtime::spawn(async move {
        match mail::sync::prefetch_bodies(&pool, &account, &password).await {
            // why: snippets just became real — lists and searches should see them.
            Ok(cached) if cached > 0 => {
                let _ = app.emit("messages-changed", account_id);
            }
            Ok(_) => {}
            Err(err) => eprintln!("body prefetch failed for account {account_id}: {err}"),
        }
    });
    Ok(())
}

/// Mark one message read/unread. Updates the local cache and notifies the UI
/// immediately, then pushes the `\Seen` flag to the server in the background —
/// opening a message never waits on the network for the dot to clear.
#[tauri::command]
pub async fn set_message_read(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
    read: bool,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    storage::messages::set_read(&state.pool, message_id, read).await?;
    app.emit("messages-changed", loc.account_id)?;

    // why: fetch account + password on the command path (cheap, from the
    // session cache) so the spawned task owns everything it needs.
    let account = storage::accounts::get(&state.pool, loc.account_id).await?;
    let password = state.password(loc.account_id).await?;
    tauri::async_runtime::spawn(async move {
        // why: best effort — if the server STORE fails, the next sync's
        // reconcile adopts the server's flag, so nothing drifts permanently.
        if let Err(err) = push_seen_flag(&account, &password, &loc.mailbox, loc.uid, read).await {
            eprintln!("failed to push read={read} for message {message_id}: {err}");
        }
    });
    Ok(())
}

/// Move one message to the account's Trash folder, then drop it from the
/// local cache.
#[tauri::command]
pub async fn move_to_trash(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<(), AppError> {
    move_to_special_folder(&app, &state, message_id, &["trash"], "trash").await
}

/// Archive one message: move it to the account's Archive folder, then drop it
/// from the local cache. Same server-confirmed path as trashing.
///
/// why the "all" fallback: Gmail has no \Archive folder — archiving means
/// removing the Inbox label, which over IMAP is a move into "All Mail" (\All).
#[tauri::command]
pub async fn archive_message(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<(), AppError> {
    move_to_special_folder(&app, &state, message_id, &["archive", "all"], "archive").await
}

/// Move one message to a user-chosen folder of its account, then drop it
/// from the local cache. The destination must be a folder discovery has
/// mirrored — an unknown name fails before any IMAP command runs.
#[tauri::command]
pub async fn move_message(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
    mailbox: String,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    if !storage::mailboxes::exists(&state.pool, loc.account_id, &mailbox).await? {
        return Err(AppError::Imap(format!("no folder named {mailbox}")));
    }
    move_to_mailbox(&app, &state, message_id, loc, &mailbox).await
}

/// Move a message to the account's folder for the first matching special-use
/// `role`, then drop it from the local cache. Unlike the read flag this waits
/// on the server — the row must not vanish from the list if the move failed.
async fn move_to_special_folder(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    roles: &[&str],
    label: &str,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    let mut dest = None;
    for role in roles {
        if let Some(name) =
            storage::mailboxes::name_for_role(&state.pool, loc.account_id, role).await?
        {
            dest = Some(name);
            break;
        }
    }
    let Some(dest) = dest else {
        return Err(AppError::Imap(format!("no {label} folder discovered yet")));
    };
    move_to_mailbox(app, state, message_id, loc, &dest).await
}

/// The one server-confirmed move path: select the source folder, UID MOVE
/// into `dest`, and only then drop the cached row — it must not vanish from
/// the list if the move failed.
async fn move_to_mailbox(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    loc: storage::messages::MessageLocation,
    dest: &str,
) -> Result<(), AppError> {
    // why: moving a message into the folder it already lives in is a no-op
    // (and some servers error on it) — just leave it be.
    if loc.mailbox == dest {
        return Ok(());
    }

    let account = storage::accounts::get(&state.pool, loc.account_id).await?;
    let password = state.password(loc.account_id).await?;
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &password,
    )
    .await?;
    session
        .select(&loc.mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {}: {e}", loc.mailbox)))?;
    let moved = mail::imap::move_message(&mut session, loc.uid, dest).await;
    let _ = session.logout().await;
    moved?;

    // why: only after the server confirms — the message left the source folder,
    // so the cached row (and its images, via cascade) go too.
    storage::messages::delete_by_id(&state.pool, message_id).await?;
    app.emit("messages-changed", loc.account_id)?;
    Ok(())
}

/// Connect, select the folder, flip the `\Seen` flag, log out.
async fn push_seen_flag(
    account: &Account,
    password: &str,
    mailbox: &str,
    uid: i64,
    seen: bool,
) -> Result<(), AppError> {
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        password,
    )
    .await?;
    session
        .select(mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
    let result = mail::imap::set_seen(&mut session, uid, seen).await;
    let _ = session.logout().await;
    result
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub async fn list_messages(
    state: State<'_, AppState>,
    account_id: Option<i64>,
    mailbox: Option<String>,
) -> Result<Vec<MessageHeader>, AppError> {
    let mailbox = mailbox.as_deref().unwrap_or("INBOX");
    storage::messages::list(&state.pool, account_id, mailbox).await
}

/// Folders of one account in sidebar order, from the local mirror.
#[tauri::command]
pub async fn list_mailboxes(
    state: State<'_, AppState>,
    account_id: i64,
) -> Result<Vec<Mailbox>, AppError> {
    storage::mailboxes::list(&state.pool, account_id).await
}

/// Search the local cache with a gmail-style query ("from:x is:unread text").
/// Purely local — never talks to the server.
#[tauri::command]
pub async fn search_messages(
    state: State<'_, AppState>,
    account_id: Option<i64>,
    query: String,
) -> Result<Vec<MessageHeader>, AppError> {
    let parsed = storage::search::parse_query(&query);
    storage::search::search(&state.pool, account_id, &parsed).await
}

/// Stat files dropped on the compose window into chip metadata (name +
/// size); duplicates and directories drop out, missing files error now
/// rather than at send time.
#[tauri::command]
pub async fn inspect_attachments(
    paths: Vec<String>,
) -> Result<Vec<crate::models::AttachmentInfo>, AppError> {
    mail::attachments::inspect(paths).await
}

// note: there is deliberately no direct send command — every outgoing
// message goes through the undoable queue below.

/// How long a queued message can still be undone before it really sends.
const UNDO_WINDOW: std::time::Duration = std::time::Duration::from_secs(8);

/// Payload of the send-queued / send-finished / send-undone events the main
/// window renders as outbox badges.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SendEvent {
    id: u64,
    subject: String,
    error: Option<String>,
    /// Length of the undo window in milliseconds — drives the countdown
    /// donut in the badge. Only meaningful on send-queued; 0 elsewhere.
    undo_ms: u64,
}

/// Park a composed message for its undo window, then send it. Returns as
/// soon as the message is queued; progress is broadcast as events.
#[tauri::command]
pub async fn queue_send(
    app: AppHandle,
    state: State<'_, AppState>,
    message: OutgoingMessage,
) -> Result<(), AppError> {
    // why: build the MIME now even though it is rebuilt at send time — an
    // invalid address must surface in the compose window immediately, not
    // as a failure badge eight seconds after the window closed.
    let account = storage::accounts::get(&state.pool, message.account_id).await?;
    mail::smtp::build_message(&account.email, &message).await?;

    static SEND_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = SEND_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let subject = message.subject.clone();
    state.park_send(id, message);
    app.emit(
        "send-queued",
        SendEvent {
            id,
            subject: subject.clone(),
            error: None,
            undo_ms: UNDO_WINDOW.as_millis() as u64,
        },
    )?;

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(UNDO_WINDOW).await;
        // why: take_send is the ownership handshake — None means undo won
        // the race and this message must never leave the machine.
        let Some(message) = app.state::<AppState>().take_send(id) else {
            return;
        };
        let error = deliver(&app.state::<AppState>(), &message).await.err();
        if let Some(err) = &error {
            eprintln!("queued send {id} failed: {err}");
            // why: a failed send must never destroy mail — the draft comes
            // back as a fresh compose window while the badge shows the error.
            if let Err(reopen) = open_compose_window(&app, message).await {
                eprintln!("failed to reopen draft for send {id}: {reopen}");
            }
        }
        let _ = app.emit(
            "send-finished",
            SendEvent {
                id,
                subject,
                error: error.map(|e| e.to_string()),
                undo_ms: 0,
            },
        );
    });
    Ok(())
}

/// Cancel a queued send inside its undo window: the draft reopens in a new
/// compose window. A no-op when the timer already fired — the badge will
/// resolve to sent/failed on its own.
#[tauri::command]
pub async fn undo_send(
    app: AppHandle,
    state: State<'_, AppState>,
    id: u64,
) -> Result<(), AppError> {
    if let Some(draft) = state.take_send(id) {
        let subject = draft.subject.clone();
        open_compose_window(&app, draft).await?;
        app.emit(
            "send-undone",
            SendEvent {
                id,
                subject,
                error: None,
                undo_ms: 0,
            },
        )?;
    }
    Ok(())
}

/// The one SMTP delivery path: account row → MIME → session cache → send.
async fn deliver(state: &AppState, message: &OutgoingMessage) -> Result<(), AppError> {
    let account = storage::accounts::get(&state.pool, message.account_id).await?;
    let mime = mail::smtp::build_message(&account.email, message).await?;
    // why: the session cache reads the keychain at most once per account per
    // run; the password never reaches events or logs.
    let password = state.password(message.account_id).await?;
    // why: formatted() consumes nothing but we need the raw bytes twice —
    // once for SMTP, once for the Sent-folder copy below.
    let raw = mime.formatted();
    mail::smtp::send(
        &account.smtp_host,
        account.smtp_port,
        &account.username,
        &password,
        mime,
    )
    .await?;
    // why: best effort — the mail already left the machine, so a failed
    // Sent copy must never surface as a failed send (or reopen the draft).
    if let Err(err) = save_sent_copy(state, &account, &password, &raw).await {
        eprintln!("sent copy for account {} failed: {err}", account.id);
    }
    Ok(())
}

/// Mirror a delivered message into the account's IMAP Sent folder so other
/// clients (webmail, phone) see it. Skipped for servers that store their own
/// copy (Gmail), where appending would duplicate the message.
async fn save_sent_copy(
    state: &AppState,
    account: &Account,
    password: &str,
    raw: &[u8],
) -> Result<(), AppError> {
    if mail::smtp::server_saves_sent_copy(&account.smtp_host) {
        return Ok(());
    }
    let Some(sent) = storage::mailboxes::sent_name(&state.pool, account.id).await? else {
        return Err(AppError::Imap("no sent folder discovered yet".to_string()));
    };
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        password,
    )
    .await?;
    let result = mail::imap::append(&mut session, &sent, raw).await;
    let _ = session.logout().await;
    result
}

/// Verify & Save: prove the submitted credentials against both servers
/// before the account is stored anywhere. The AppError Display strings
/// already name the failing leg ("imap error: …" / "smtp error: …").
#[tauri::command]
pub async fn test_connection(account: NewAccount, password: String) -> Result<(), AppError> {
    let (imap, smtp) = tokio::join!(
        mail::imap::verify(
            &account.imap_host,
            account.imap_port,
            &account.username,
            &password,
        ),
        mail::smtp::verify(
            &account.smtp_host,
            account.smtp_port,
            &account.username,
            &password,
        ),
    );
    imap?;
    smtp
}

/// Body for the viewer — served from cache, lazily fetched on first open.
/// `load_remote` is the per-message "Load images" click; it only has an
/// effect under the Ask policy (Block ignores it, Always needs no click).
#[tauri::command]
pub async fn get_message_body(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
    load_remote: Option<bool>,
) -> Result<MessageBody, AppError> {
    let row = storage::messages::get_body(&state.pool, message_id).await?;

    let cached = row.body_text.is_some() || row.body_html.is_some();
    // why: bodies cached before message_images existed have cid: references
    // but no stored images — a one-off refetch backfills them instead of
    // rendering blanks forever.
    let backfill = cached
        && row.body_html.as_deref().is_some_and(|h| h.contains("cid:"))
        && storage::messages::images(&state.pool, message_id)
            .await?
            .is_empty();

    let (html, text, images) = if cached && !backfill {
        let images = storage::messages::images(&state.pool, message_id).await?;
        (row.body_html, row.body_text, images)
    } else {
        let account = storage::accounts::get(&state.pool, row.account_id).await?;
        let password = state.password(account.id).await?;
        let parsed = mail::sync::fetch_body_into_cache(
            &state.pool,
            &account,
            &password,
            message_id,
            &row.mailbox,
            row.uid,
        )
        .await?;
        // why: the snippet just became real — lists should refresh.
        app.emit("messages-changed", row.account_id)?;
        (parsed.html, parsed.text, parsed.images)
    };

    let policy = storage::settings::remote_image_policy(&state.pool).await?;
    sanitized_body(&state.pool, html, text, &images, policy, load_remote).await
}

// SECURITY: the single place message HTML is prepared for the frontend —
// everything goes through mail::sanitize::build_srcdoc, cached or fresh.
// Remote images load only under policy Always or an explicit Ask-click;
// under Block a stray load_remote from the frontend changes nothing.
async fn sanitized_body(
    pool: &sqlx::SqlitePool,
    html: Option<String>,
    text: Option<String>,
    images: &[mail::parse::InlineImage],
    policy: RemoteImagePolicy,
    load_remote: Option<bool>,
) -> Result<MessageBody, AppError> {
    let load = policy == RemoteImagePolicy::Always
        || (policy == RemoteImagePolicy::Ask && load_remote.unwrap_or(false));

    let mut blocked_images = 0;
    let rendered = match html.as_deref() {
        Some(h) => {
            let remote = if load {
                let urls = mail::sanitize::remote_image_urls(h);
                mail::remote::load_images(pool, &urls).await
            } else {
                std::collections::HashMap::new()
            };
            let body = mail::sanitize::build_srcdoc(h, images, &remote);
            blocked_images = body.blocked_remote;
            Some(body.html)
        }
        None => None,
    };

    Ok(MessageBody {
        html: rendered,
        text,
        blocked_images,
        // why: !load, not just Ask — after the click the banner disappears
        // even when some images failed to fetch (no endless "load" loop).
        can_load_remote: policy == RemoteImagePolicy::Ask && !load && blocked_images > 0,
    })
}

/// Open a native compose window seeded with `draft`. Every call opens its
/// own window (unique label), so several drafts can be in flight at once.
#[tauri::command]
pub async fn open_compose(app: AppHandle, draft: OutgoingMessage) -> Result<(), AppError> {
    open_compose_window(&app, draft).await
}

/// Shared with the send queue, which reopens a draft on undo or failure.
pub(crate) async fn open_compose_window(
    app: &AppHandle,
    draft: OutgoingMessage,
) -> Result<(), AppError> {
    // why: a process-wide counter, not "count of open windows" — labels of
    // closed windows must never be reused while their drafts might linger.
    static COMPOSE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let label = format!(
        "compose-{}",
        COMPOSE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );

    let title = if draft.subject.trim().is_empty() {
        "New Message".to_string()
    } else {
        draft.subject.clone()
    };
    app.state::<AppState>().park_draft(&label, draft);

    let builder = tauri::WebviewWindowBuilder::new(
        app,
        &label,
        // why: all windows serve the same bundle — main.ts picks the root
        // component from the window label.
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(title)
    .inner_size(640.0, 680.0)
    .min_inner_size(480.0, 400.0);
    // why: overlay puts the traffic lights on the compose toolbar strip,
    // matching the main window's chrome.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build()?;
    Ok(())
}

/// One-shot draft pickup for a freshly opened compose window; `None` when
/// the draft was already taken (e.g. after a webview reload).
#[tauri::command]
pub fn take_compose_draft(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<OutgoingMessage>, AppError> {
    Ok(state.take_draft(window.label()))
}

/// Close the invoking compose window — windows lack the core close
/// permission, mirroring how settings closes through a command.
#[tauri::command]
pub async fn close_compose(window: tauri::WebviewWindow) -> Result<(), AppError> {
    window.close()?;
    Ok(())
}

#[tauri::command]
pub async fn get_remote_image_policy(
    state: State<'_, AppState>,
) -> Result<RemoteImagePolicy, AppError> {
    storage::settings::remote_image_policy(&state.pool).await
}

#[tauri::command]
pub async fn set_remote_image_policy(
    app: AppHandle,
    state: State<'_, AppState>,
    policy: RemoteImagePolicy,
) -> Result<(), AppError> {
    storage::settings::set_remote_image_policy(&state.pool, policy).await?;
    // why: the settings window mutates, the main window's open message view
    // listens and re-renders with the new policy.
    app.emit("settings-changed", ())?;
    Ok(())
}

// why: async on purpose — Tauri docs warn that creating windows from a sync
// command can deadlock on some platforms (the command runs on the main
// thread there, and window creation needs it too).
#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Result<(), AppError> {
    // why: a closed window is destroyed, so reopen = rebuild; if it's only
    // hidden/behind, just focus it instead of spawning a duplicate.
    if let Some(window) = app.get_webview_window("settings") {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        // why: both windows serve the same bundle — main.ts picks the root
        // component from the window label.
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("Settings")
    .inner_size(720.0, 480.0)
    .min_inner_size(560.0, 360.0)
    .build()?;
    Ok(())
}

#[tauri::command]
pub async fn close_settings(app: AppHandle) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window("settings") {
        window.close()?;
    }
    Ok(())
}
