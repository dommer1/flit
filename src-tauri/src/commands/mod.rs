use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppError;
use crate::models::{
    Account, Alias, Mailbox, MessageBody, MessageHeader, NewAccount, NotificationSettings,
    OutgoingMessage, RemoteImagePolicy, Signature, SwipeActions, ThreadOrder,
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
pub async fn sync_account(app: AppHandle, account_id: i64) -> Result<(), AppError> {
    run_sync(&app, account_id).await
}

/// The sync pass behind the command, callable from background tasks (the
/// poller) that have an AppHandle but no `State` extractor.
pub(crate) async fn run_sync(app: &AppHandle, account_id: i64) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    // why: held (RAII) until this function returns — a manual refresh racing
    // the background poll must not open a second IMAP session for the same
    // account or double-fire notifications. The loser skips silently.
    let Some(_slot) = state.try_begin_sync(account_id) else {
        return Ok(());
    };
    let account = storage::accounts::get(&state.pool, account_id).await?;
    // why: the password comes from the session cache (one keychain read per
    // account per run) and is handed on to the prefetch task below — never
    // written to state beyond the cache, events, or logs.
    let result = async {
        let password = state.password(account_id).await?;
        let new_mail = mail::sync::sync_account(&state.pool, &account, &password).await?;
        Ok::<(String, Vec<mail::sync::NewMail>), AppError>((password, new_mail))
    }
    .await;

    // why: every sync doubles as a health check — the recorded outcome is
    // what settings shows when credentials go stale server-side.
    let error_text = result.as_ref().err().map(ToString::to_string);
    storage::accounts::set_status(&state.pool, account_id, error_text.as_deref(), now_epoch())
        .await?;
    app.emit("accounts-changed", ())?;

    let (password, new_mail) = result?;
    app.emit("messages-changed", account_id)?;

    // why: after the emit — banners are cosmetic, the fresh list is not.
    // Settings are re-read per sync so a toggle applies to the next pass.
    let defaults = storage::settings::notification_settings(&state.pool).await?;
    crate::notify::show(app, &crate::notify::plan(&account, &defaults, &new_mail));

    // why: bodies download in the background AFTER the command returns — the
    // header list is already usable, and each cached body feeds the FTS index
    // so search covers unopened mail.
    let pool = state.pool.clone();
    let app = app.clone();
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
    let rows = vec![(message_id, loc.uid)];
    move_rows_to_mailbox(&app, &state, loc.account_id, &loc.mailbox, &rows, &mailbox).await
}

/// Trash a whole conversation as shown in its folder — every thread member
/// sharing the anchor's mailbox. Flat views (trash/junk/drafts) keep using
/// the single-message commands; the frontend picks per row.
#[tauri::command]
pub async fn trash_thread(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<(), AppError> {
    move_thread_to_special_folder(&app, &state, message_id, &["trash"], "trash").await
}

/// Archive a whole conversation as shown in its folder (Gmail: All Mail).
#[tauri::command]
pub async fn archive_thread(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<(), AppError> {
    move_thread_to_special_folder(&app, &state, message_id, &["archive", "all"], "archive").await
}

/// Move a whole conversation (as shown in the anchor's folder) to a
/// user-chosen folder of its account.
#[tauri::command]
pub async fn move_thread(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
    mailbox: String,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    if !storage::mailboxes::exists(&state.pool, loc.account_id, &mailbox).await? {
        return Err(AppError::Imap(format!("no folder named {mailbox}")));
    }
    let rows = storage::messages::thread_rows_in_mailbox(&state.pool, message_id).await?;
    move_rows_to_mailbox(&app, &state, loc.account_id, &loc.mailbox, &rows, &mailbox).await
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
    let dest = special_folder_dest(state, loc.account_id, roles, label).await?;
    let rows = vec![(message_id, loc.uid)];
    move_rows_to_mailbox(app, state, loc.account_id, &loc.mailbox, &rows, &dest).await
}

/// Like `move_to_special_folder`, but for the anchor's whole conversation
/// as shown in its folder — every thread member sharing the mailbox moves
/// in one server session.
async fn move_thread_to_special_folder(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    roles: &[&str],
    label: &str,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    let dest = special_folder_dest(state, loc.account_id, roles, label).await?;
    let rows = storage::messages::thread_rows_in_mailbox(&state.pool, message_id).await?;
    move_rows_to_mailbox(app, state, loc.account_id, &loc.mailbox, &rows, &dest).await
}

/// The account's folder name for the first matching special-use role.
async fn special_folder_dest(
    state: &AppState,
    account_id: i64,
    roles: &[&str],
    label: &str,
) -> Result<String, AppError> {
    for role in roles {
        if let Some(name) = storage::mailboxes::name_for_role(&state.pool, account_id, role).await?
        {
            return Ok(name);
        }
    }
    Err(AppError::Imap(format!("no {label} folder discovered yet")))
}

/// The one server-confirmed move path: select the source folder, UID MOVE
/// each `(row id, uid)` into `dest`, and drop cached rows only for confirmed
/// moves — a row must not vanish from the list if its move failed. A failure
/// mid-batch keeps the earlier moves (they already happened server-side) and
/// surfaces the error.
async fn move_rows_to_mailbox(
    app: &AppHandle,
    state: &AppState,
    account_id: i64,
    source: &str,
    rows: &[(i64, i64)],
    dest: &str,
) -> Result<(), AppError> {
    // why: moving a message into the folder it already lives in is a no-op
    // (and some servers error on it) — just leave it be.
    if source == dest || rows.is_empty() {
        return Ok(());
    }

    let account = storage::accounts::get(&state.pool, account_id).await?;
    let password = state.password(account_id).await?;
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &password,
    )
    .await?;
    session
        .select(source)
        .await
        .map_err(|e| AppError::Imap(format!("select {source}: {e}")))?;
    let mut outcome = Ok(());
    let mut moved: Vec<i64> = Vec::new();
    for (row_id, uid) in rows {
        match mail::imap::move_message(&mut session, *uid, dest).await {
            Ok(()) => moved.push(*row_id),
            Err(err) => {
                outcome = Err(err);
                break;
            }
        }
    }
    let _ = session.logout().await;

    // why: only after the server confirms — each confirmed message left the
    // source folder, so its cached row (and images, via cascade) go too.
    let emit = !moved.is_empty();
    for row_id in moved {
        storage::messages::delete_by_id(&state.pool, row_id).await?;
    }
    if emit {
        app.emit("messages-changed", account_id)?;
    }
    outcome
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

pub(crate) fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Message list for one folder view: threaded (one row per conversation)
/// everywhere except trash/junk/drafts, which stay flat — an action there
/// must touch exactly one message, never its whole conversation.
#[tauri::command]
pub async fn list_messages(
    state: State<'_, AppState>,
    account_id: Option<i64>,
    mailbox: Option<String>,
) -> Result<Vec<MessageHeader>, AppError> {
    let mailbox = mailbox.as_deref().unwrap_or("INBOX");
    let flat = match account_id {
        Some(id) => matches!(
            storage::mailboxes::role_of(&state.pool, id, mailbox)
                .await?
                .as_deref(),
            Some("trash" | "junk" | "drafts")
        ),
        // why: the unified view only ever shows INBOX folders.
        None => false,
    };
    if flat {
        storage::messages::list(&state.pool, account_id, mailbox).await
    } else {
        storage::messages::list_threaded(&state.pool, account_id, mailbox).await
    }
}

/// The full conversation of one message (all folders except
/// trash/junk/drafts), oldest first — what the conversation view renders.
#[tauri::command]
pub async fn list_thread(
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<Vec<MessageHeader>, AppError> {
    storage::messages::thread_of(&state.pool, message_id).await
}

/// Autocomplete for compose recipient fields: locally harvested contacts
/// matching `query`, best first. Never touches the network.
#[tauri::command]
pub async fn list_contacts(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<crate::models::Contact>, AppError> {
    storage::contacts::suggest(&state.pool, &query).await
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

/// Best-effort thumbnail for one attachment as a data: URI; None means
/// "show the generic file card". Never fails — previews are decoration.
#[tauri::command]
pub async fn attachment_preview(path: String) -> Result<Option<String>, AppError> {
    Ok(mail::attachments::preview(path).await)
}

// note: there is deliberately no direct send command — every outgoing
// message goes through the undoable queue below.

/// How long a queued message can still be undone before it really sends.
const UNDO_WINDOW: std::time::Duration = std::time::Duration::from_secs(8);

/// One id sequence for every send-* badge event, whichever path emits it
/// (undo queue, scheduler) — the outbox keys badges by this id.
static SEND_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    let (from_name, from_email) = storage::aliases::sender(&state.pool, &message).await?;
    mail::smtp::build_message(&from_name, &from_email, &message).await?;

    // why best effort: the contacts book is a convenience — a failed write
    // must never block or fail a send that already validated.
    if let Err(err) =
        storage::contacts::harvest_sent(&state.pool, &[&message.to, &message.cc, &message.bcc])
            .await
    {
        eprintln!("failed to record recipients as contacts: {err}");
    }

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

/// Park a composed message in SQLite until its delivery time ("Send Later").
/// Validation mirrors queue_send: problems must surface in the compose
/// window now, not as a failure badge when the time comes.
#[tauri::command]
pub async fn schedule_send(
    app: AppHandle,
    state: State<'_, AppState>,
    message: OutgoingMessage,
    scheduled_at: i64,
) -> Result<(), AppError> {
    let (from_name, from_email) = storage::aliases::sender(&state.pool, &message).await?;
    mail::smtp::build_message(&from_name, &from_email, &message).await?;
    validate_scheduled_at(scheduled_at, now_epoch())?;
    storage::scheduled::insert(&state.pool, &message, scheduled_at).await?;
    // why: no payload — listeners (scheduled list in the sidebar) re-query
    // the DB, which is the single source of truth for parked messages.
    app.emit("scheduled-changed", ())?;
    Ok(())
}

/// Every parked send-later message, soonest first — pending and missed
/// alike; the frontend splits them by status.
#[tauri::command]
pub async fn list_scheduled(
    state: State<'_, AppState>,
) -> Result<Vec<crate::models::ScheduledMessage>, AppError> {
    storage::scheduled::list(&state.pool).await
}

/// "Send now" for one parked message (the catch-up dialog's confirm). The
/// take() is the claim — if the scheduler beat us to it, this is a no-op.
#[tauri::command]
pub async fn send_scheduled_now(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), AppError> {
    let Some(message) = storage::scheduled::take(&state.pool, id).await? else {
        return Ok(());
    };
    app.emit("scheduled-changed", ())?;
    // why spawn: delivery talks to the SMTP server — the dialog gets its
    // answer now, progress arrives as the usual outbox badge events.
    tauri::async_runtime::spawn(async move {
        deliver_scheduled(&app, message.outgoing()).await;
    });
    Ok(())
}

/// Cancel one parked message: it leaves the schedule and reopens as a
/// compose window, so nothing the user wrote is ever destroyed.
#[tauri::command]
pub async fn cancel_scheduled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), AppError> {
    let Some(message) = storage::scheduled::take(&state.pool, id).await? else {
        return Ok(());
    };
    app.emit("scheduled-changed", ())?;
    open_compose_window(&app, message.outgoing()).await?;
    Ok(())
}

/// A schedule time must be strictly in the future — "now" belongs to the
/// ordinary send button.
fn validate_scheduled_at(scheduled_at: i64, now: i64) -> Result<(), AppError> {
    if scheduled_at <= now {
        return Err(AppError::Invalid(
            "scheduled time is in the past".to_string(),
        ));
    }
    Ok(())
}

/// Deliver a message the scheduler claimed, with the same badge events and
/// failure recovery (reopen as a draft window) as the undo queue — minus the
/// undo window, which already passed when the user picked a time.
pub(crate) async fn deliver_scheduled(app: &AppHandle, message: OutgoingMessage) {
    let id = SEND_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let subject = message.subject.clone();
    let _ = app.emit(
        "send-queued",
        SendEvent {
            id,
            subject: subject.clone(),
            error: None,
            undo_ms: 0,
        },
    );
    let error = deliver(&app.state::<AppState>(), &message).await.err();
    if let Some(err) = &error {
        eprintln!("scheduled send {id} failed: {err}");
        // why: a failed send must never destroy mail — same contract as the
        // undo queue's failure path.
        if let Err(reopen) = open_compose_window(app, message).await {
            eprintln!("failed to reopen draft for scheduled send {id}: {reopen}");
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
}

/// The one SMTP delivery path: account row → MIME → session cache → send.
async fn deliver(state: &AppState, message: &OutgoingMessage) -> Result<(), AppError> {
    let account = storage::accounts::get(&state.pool, message.account_id).await?;
    let (from_name, from_email) = storage::aliases::sender(&state.pool, message).await?;
    let mime = mail::smtp::build_message(&from_name, &from_email, message).await?;
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
    // Same rule for the draft cleanup: a leftover draft is cosmetic, the
    // next save/sync can deal with it.
    if let Some(draft_id) = &message.draft_message_id {
        if let Err(err) = delete_sent_draft(state, &account, &password, draft_id).await {
            eprintln!("could not delete draft {draft_id} after send: {err}");
        }
    }
    Ok(())
}

/// Remove the sent message's autosaved version from the Drafts folder, so
/// it stops looking like unfinished work here and in webmail.
async fn delete_sent_draft(
    state: &AppState,
    account: &Account,
    password: &str,
    draft_id: &str,
) -> Result<(), AppError> {
    let Some(drafts) = storage::mailboxes::name_for_role(&state.pool, account.id, "drafts").await?
    else {
        return Ok(()); // no drafts folder, nothing to clean up
    };
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        password,
    )
    .await?;
    let result = delete_draft_version(&mut session, &drafts, draft_id).await;
    let _ = session.logout().await;
    result
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
    let result = mail::imap::append(&mut session, &sent, "(\\Seen)", raw).await;
    let _ = session.logout().await;
    result
}

/// Save one compose draft into its account's Drafts folder on the server,
/// where webmail and other clients see it. Returns the Message-ID of the
/// saved version — the handle for replacing or deleting it later. IMAP has
/// no edit-in-place, so each save appends a fresh version and then deletes
/// the previous one (`previous_draft_id`).
#[tauri::command]
pub async fn save_draft(
    state: State<'_, AppState>,
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

    let password = state.password(message.account_id).await?;
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &password,
    )
    .await?;
    // why \Seen: the user wrote this text — it must not light up unread
    // badges here or in other clients.
    let appended = mail::imap::append(&mut session, &drafts, "(\\Draft \\Seen)", &raw).await;
    if appended.is_ok() {
        // why best effort: the new version is safely on the server; a
        // leftover old version is a cosmetic duplicate the user can delete,
        // never lost text. The next save also retries it via its own id.
        if let Some(old_id) = &previous_draft_id {
            if let Err(err) = delete_draft_version(&mut session, &drafts, old_id).await {
                eprintln!("could not delete draft version {old_id}: {err}");
            }
        }
    }
    let _ = session.logout().await;
    appended?;
    Ok(message_id)
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
    // but no stored images, and bodies cached before message_attachments
    // existed were never scanned for attachments — a one-off refetch
    // backfills either instead of rendering blanks/nothing forever.
    let backfill = cached
        && (!row.attachments_scanned
            || (row.body_html.as_deref().is_some_and(|h| h.contains("cid:"))
                && storage::messages::images(&state.pool, message_id)
                    .await?
                    .is_empty()));

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
    let mut body = sanitized_body(&state.pool, html, text, &images, policy, load_remote).await?;
    // why from the DB, not the parse result: both branches above have already
    // written the metadata (set_body), so one read serves cached and fresh.
    body.attachments = storage::messages::attachments(&state.pool, message_id).await?;
    Ok(body)
}

/// Bodies for a whole conversation in one call — the conversation view's
/// bulk load. Cached bodies come straight from SQLite; everything missing
/// is fetched over a SINGLE IMAP session, grouped by folder, instead of one
/// TLS connect per message (which made long threads crawl open).
#[tauri::command]
pub async fn thread_bodies(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<std::collections::HashMap<i64, MessageBody>, AppError> {
    let thread = storage::messages::thread_of(&state.pool, message_id).await?;

    // Members whose body is not cached yet, grouped by the folder to select.
    let mut missing: std::collections::BTreeMap<String, Vec<(i64, i64)>> = Default::default();
    let mut fetch_account = None;
    for header in &thread {
        let row = storage::messages::get_body(&state.pool, header.id).await?;
        if row.body_text.is_none() && row.body_html.is_none() {
            fetch_account = Some(row.account_id);
            missing
                .entry(row.mailbox)
                .or_default()
                .push((header.id, row.uid));
        }
    }

    if let Some(account_id) = fetch_account {
        let account = storage::accounts::get(&state.pool, account_id).await?;
        let password = state.password(account_id).await?;
        let mut session = mail::imap::connect(
            &account.imap_host,
            account.imap_port,
            &account.username,
            &password,
        )
        .await?;
        let fetched = async {
            for (mailbox, entries) in &missing {
                session
                    .select(mailbox)
                    .await
                    .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
                for (id, uid) in entries {
                    // why skip: a uid can vanish mid-run (deleted elsewhere) —
                    // the rest of the conversation must still load.
                    let Some(raw) = mail::imap::fetch_body(&mut session, *uid).await? else {
                        continue;
                    };
                    let parsed = mail::parse::parse_body(&raw);
                    storage::messages::set_body(
                        &state.pool,
                        *id,
                        parsed.text.as_deref(),
                        parsed.html.as_deref(),
                        &parsed.snippet,
                        &parsed.images,
                        &parsed.attachments,
                    )
                    .await?;
                }
            }
            Ok::<(), AppError>(())
        }
        .await;
        let _ = session.logout().await;
        fetched?;
        // why: snippets just became real — lists should refresh.
        app.emit("messages-changed", account_id)?;
    }

    let policy = storage::settings::remote_image_policy(&state.pool).await?;
    let mut bodies = std::collections::HashMap::new();
    for header in &thread {
        let row = storage::messages::get_body(&state.pool, header.id).await?;
        let images = storage::messages::images(&state.pool, header.id).await?;
        let mut body = sanitized_body(
            &state.pool,
            row.body_html,
            row.body_text,
            &images,
            policy,
            None,
        )
        .await?;
        body.attachments = storage::messages::attachments(&state.pool, header.id).await?;
        bodies.insert(header.id, body);
    }
    Ok(bodies)
}

/// Download the raw RFC822 bytes of one cached message from its server.
async fn fetch_raw_message(
    state: &AppState,
    loc: &storage::messages::MessageLocation,
) -> Result<Vec<u8>, AppError> {
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
    let raw = mail::imap::fetch_body(&mut session, loc.uid).await;
    let _ = session.logout().await;
    raw?.ok_or_else(|| AppError::Imap("message no longer on the server".to_string()))
}

/// Save one attachment to `path` (a native save-dialog pick, which already
/// confirmed any overwrite). The bytes are re-fetched from the server and
/// re-extracted by part index — they never touch the local DB.
#[tauri::command]
pub async fn save_attachment(
    state: State<'_, AppState>,
    attachment_id: i64,
    path: String,
) -> Result<(), AppError> {
    let meta = storage::messages::attachment(&state.pool, attachment_id).await?;
    let loc = storage::messages::location(&state.pool, meta.message_id).await?;
    let raw = fetch_raw_message(&state, &loc).await?;
    let data = mail::parse::attachment_data(&raw, meta.part_index).ok_or_else(|| {
        AppError::Imap(format!(
            "attachment {} not found in the message",
            meta.filename
        ))
    })?;
    tokio::fs::write(&path, data).await?;
    Ok(())
}

/// Save every attachment of a message into `dir` (a native folder pick):
/// one server fetch, each part written under its sanitized filename.
#[tauri::command]
pub async fn save_all_attachments(
    state: State<'_, AppState>,
    message_id: i64,
    dir: String,
) -> Result<(), AppError> {
    let attachments = storage::messages::attachments(&state.pool, message_id).await?;
    if attachments.is_empty() {
        return Ok(());
    }
    let loc = storage::messages::location(&state.pool, message_id).await?;
    let raw = fetch_raw_message(&state, &loc).await?;
    for meta in attachments {
        let data = mail::parse::attachment_data(&raw, meta.part_index).ok_or_else(|| {
            AppError::Imap(format!(
                "attachment {} not found in the message",
                meta.filename
            ))
        })?;
        let path = unique_path(
            std::path::Path::new(&dir),
            &mail::parse::safe_filename(&meta.filename),
        );
        tokio::fs::write(&path, data).await?;
    }
    Ok(())
}

/// `dir/name`, with " (n)" inserted before the extension while taken —
/// Save All picks only a directory, so it must not silently overwrite.
fn unique_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("unbounded counter always finds a free name")
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

    // Text bodies fold their quoted history on the frontend — split here so
    // the detection lives in one place (mail::quote).
    let (text, quoted_text) = match text {
        Some(t) => {
            let (own, quoted) = mail::quote::split_text_quote(&t);
            (Some(own), quoted)
        }
        None => (None, None),
    };

    Ok(MessageBody {
        html: rendered,
        text,
        quoted_text,
        blocked_images,
        // why: !load, not just Ask — after the click the banner disappears
        // even when some images failed to fetch (no endless "load" loop).
        can_load_remote: policy == RemoteImagePolicy::Ask && !load && blocked_images > 0,
        // Filled by get_message_body from the DB — sanitization has no say.
        attachments: Vec::new(),
    })
}

/// Reopen a message from a Drafts folder for editing: fetch the raw draft,
/// parse it back into compose fields, and open a compose window that keeps
/// replacing this server version (via its Message-ID) on every save.
#[tauri::command]
pub async fn open_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    let account = storage::accounts::get(&state.pool, loc.account_id).await?;
    let password = state.password(loc.account_id).await?;

    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &password,
    )
    .await?;
    let fetched = async {
        session
            .select(&loc.mailbox)
            .await
            .map_err(|e| AppError::Imap(format!("select {}: {e}", loc.mailbox)))?;
        mail::imap::fetch_body(&mut session, loc.uid).await
    }
    .await;
    let _ = session.logout().await;
    let raw =
        fetched?.ok_or_else(|| AppError::Imap("draft no longer on the server".to_string()))?;

    let parsed = mail::parse::parse_draft(&raw);
    // why fall back to None: a draft From that matches no alias (foreign
    // client, deleted alias) reopens from the account's own address.
    let alias = match parsed.from_addr.as_deref() {
        Some(addr) => storage::aliases::find_by_email(&state.pool, loc.account_id, addr).await?,
        None => None,
    };
    let attachments = stash_draft_attachments(message_id, parsed.attachments).await?;
    open_compose_window(
        &app,
        OutgoingMessage {
            account_id: loc.account_id,
            alias_id: alias.map(|a| a.id),
            to: parsed.to,
            cc: parsed.cc,
            bcc: parsed.bcc,
            subject: parsed.subject,
            body: parsed.body,
            // The compose editor takes plain text; a draft's HTML part is
            // regenerated from it on the next save.
            body_html: None,
            attachments,
            draft_message_id: parsed.message_id,
            in_reply_to: parsed.in_reply_to,
            references: parsed.references,
        },
    )
    .await
}

/// Write a reopened draft's attachment bytes into per-message temp files,
/// so the compose window can treat them exactly like freshly dropped files
/// (only paths travel through the app). Names were sanitized by parse_draft,
/// so the files always land inside the temp directory.
async fn stash_draft_attachments(
    message_id: i64,
    parts: Vec<mail::parse::DraftAttachment>,
) -> Result<Vec<crate::models::AttachmentRef>, AppError> {
    let mut refs = Vec::with_capacity(parts.len());
    for (index, part) in parts.into_iter().enumerate() {
        let dir = std::env::temp_dir().join(format!("flit-draft-{message_id}-{index}"));
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(&part.name);
        tokio::fs::write(&path, &part.data).await?;
        refs.push(crate::models::AttachmentRef {
            path: path.to_string_lossy().into_owned(),
            name: part.name,
        });
    }
    Ok(refs)
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

#[tauri::command]
pub async fn get_swipe_actions(state: State<'_, AppState>) -> Result<SwipeActions, AppError> {
    storage::settings::swipe_actions(&state.pool).await
}

#[tauri::command]
pub async fn set_swipe_actions(
    app: AppHandle,
    state: State<'_, AppState>,
    actions: SwipeActions,
) -> Result<(), AppError> {
    storage::settings::set_swipe_actions(&state.pool, actions).await?;
    // why: the settings window mutates, the main window's message list
    // listens and re-reads the gesture config.
    app.emit("settings-changed", ())?;
    Ok(())
}

#[tauri::command]
pub async fn get_thread_order(state: State<'_, AppState>) -> Result<ThreadOrder, AppError> {
    storage::settings::thread_order(&state.pool).await
}

#[tauri::command]
pub async fn set_thread_order(
    app: AppHandle,
    state: State<'_, AppState>,
    order: ThreadOrder,
) -> Result<(), AppError> {
    storage::settings::set_thread_order(&state.pool, order).await?;
    // why: the settings window mutates, the main window's conversation view
    // listens and re-reads the order.
    app.emit("settings-changed", ())?;
    Ok(())
}

#[tauri::command]
pub async fn get_notification_settings(
    state: State<'_, AppState>,
) -> Result<NotificationSettings, AppError> {
    storage::settings::notification_settings(&state.pool).await
}

#[tauri::command]
pub async fn set_notification_settings(
    state: State<'_, AppState>,
    settings: NotificationSettings,
) -> Result<(), AppError> {
    storage::settings::set_notification_settings(&state.pool, &settings).await
}

/// Set (or clear, with nulls) one account's notification overrides, then
/// broadcast so every window sees the fresh account list.
#[tauri::command]
pub async fn set_account_notifications(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    enabled: Option<bool>,
    sound: Option<String>,
) -> Result<(), AppError> {
    storage::accounts::set_notify(&state.pool, id, enabled, sound.as_deref()).await?;
    app.emit("accounts-changed", ())?;
    Ok(())
}

/// Play a short preview of a notification sound (settings pane UX);
/// "default"/"none" have nothing to play and are a no-op.
#[tauri::command]
pub async fn preview_notification_sound(sound: String) -> Result<(), AppError> {
    crate::notify::preview(&sound);
    Ok(())
}

#[tauri::command]
pub async fn list_signatures(state: State<'_, AppState>) -> Result<Vec<Signature>, AppError> {
    storage::signatures::list(&state.pool).await
}

#[tauri::command]
pub async fn create_signature(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<Signature, AppError> {
    let created = storage::signatures::create(&state.pool, &name).await?;
    app.emit("signatures-changed", ())?;
    Ok(created)
}

#[tauri::command]
pub async fn update_signature(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    name: String,
    body: String,
) -> Result<(), AppError> {
    storage::signatures::update(&state.pool, id, &name, &body).await?;
    app.emit("signatures-changed", ())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_signature(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), AppError> {
    storage::signatures::delete(&state.pool, id).await?;
    app.emit("signatures-changed", ())?;
    // why: ON DELETE SET NULL may have cleared account defaults too.
    app.emit("accounts-changed", ())?;
    Ok(())
}

/// Make one signature the default for exactly the given accounts.
#[tauri::command]
pub async fn set_signature_accounts(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    account_ids: Vec<i64>,
) -> Result<(), AppError> {
    storage::signatures::set_default_for_accounts(&state.pool, id, &account_ids).await?;
    // why: defaults live on the account rows, so account listeners refetch.
    app.emit("accounts-changed", ())?;
    Ok(())
}

#[tauri::command]
pub async fn list_aliases(state: State<'_, AppState>) -> Result<Vec<Alias>, AppError> {
    storage::aliases::list(&state.pool).await
}

#[tauri::command]
pub async fn add_alias(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: i64,
    name: String,
    email: String,
) -> Result<Alias, AppError> {
    let added = storage::aliases::add(&state.pool, account_id, &name, &email).await?;
    // why: aliases are part of an account's send identity — the settings
    // pane and the compose From picker listen on the same account event.
    app.emit("accounts-changed", ())?;
    Ok(added)
}

#[tauri::command]
pub async fn update_alias(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    name: String,
    email: String,
) -> Result<(), AppError> {
    storage::aliases::update(&state.pool, id, &name, &email).await?;
    app.emit("accounts-changed", ())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_alias(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), AppError> {
    storage::aliases::delete(&state.pool, id).await?;
    app.emit("accounts-changed", ())?;
    Ok(())
}

/// Pick the identity new mail from this account starts with;
/// `None` = the account's own address.
#[tauri::command]
pub async fn set_default_alias(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: i64,
    alias_id: Option<i64>,
) -> Result<(), AppError> {
    storage::aliases::set_default(&state.pool, account_id, alias_id).await?;
    app.emit("accounts-changed", ())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_times_must_be_in_the_future() {
        assert!(validate_scheduled_at(1_001, 1_000).is_ok());
        // "now" and the past both belong to the ordinary send button
        assert!(validate_scheduled_at(1_000, 1_000).is_err());
        assert!(validate_scheduled_at(999, 1_000).is_err());
    }
}
