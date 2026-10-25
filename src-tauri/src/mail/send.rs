//! Sending: the undo queue, Send Later, and the one SMTP delivery path both
//! end in. Progress goes out as the send-* events the main window renders
//! as outbox badges; a failed send reopens its draft, never loses it.

use tauri::{AppHandle, Emitter, Manager};

use crate::auth::Credential;
use crate::clock::now_epoch;
use crate::error::AppError;
use crate::models::{Account, OutgoingMessage};
use crate::state::AppState;
use crate::windows::open_compose_window;
use crate::{mail, storage};

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
pub async fn queue_send(
    app: &AppHandle,
    state: &AppState,
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

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(UNDO_WINDOW).await;
        // why: take_send is the ownership handshake — None means undo won
        // the race and this message must never leave the machine.
        let Some(message) = app.state::<AppState>().take_send(id) else {
            return;
        };
        let error = deliver(&app, &message).await.err();
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
pub async fn undo_send(app: &AppHandle, state: &AppState, id: u64) -> Result<(), AppError> {
    if let Some(draft) = state.take_send(id) {
        let subject = draft.subject.clone();
        open_compose_window(app, draft).await?;
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
pub async fn schedule_send(
    app: &AppHandle,
    state: &AppState,
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

/// "Send now" for one parked message (the catch-up dialog's confirm). The
/// take() is the claim — if the scheduler beat us to it, this is a no-op.
pub async fn send_scheduled_now(
    app: &AppHandle,
    state: &AppState,
    id: i64,
) -> Result<(), AppError> {
    let Some(message) = storage::scheduled::take(&state.pool, id).await? else {
        return Ok(());
    };
    app.emit("scheduled-changed", ())?;
    // why spawn: delivery talks to the SMTP server — the dialog gets its
    // answer now, progress arrives as the usual outbox badge events.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        deliver_scheduled(&app, message.outgoing()).await;
    });
    Ok(())
}

/// Cancel one parked message: it leaves the schedule and reopens as a
/// compose window, so nothing the user wrote is ever destroyed.
pub async fn cancel_scheduled(app: &AppHandle, state: &AppState, id: i64) -> Result<(), AppError> {
    let Some(message) = storage::scheduled::take(&state.pool, id).await? else {
        return Ok(());
    };
    app.emit("scheduled-changed", ())?;
    open_compose_window(app, message.outgoing()).await?;
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
    let error = deliver(app, &message).await.err();
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
async fn deliver(app: &AppHandle, message: &OutgoingMessage) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let state = &*state;
    let account = storage::accounts::get(&state.pool, message.account_id).await?;
    let (from_name, from_email) = storage::aliases::sender(&state.pool, message).await?;
    let mime = mail::smtp::build_message(&from_name, &from_email, message).await?;
    // why: the session cache reads the keychain at most once per account per
    // run; the credential never reaches events or logs.
    let credential = state.credential(&account).await?;
    // why: formatted() consumes nothing but we need the raw bytes twice —
    // once for SMTP, once for the Sent-folder copy below.
    let raw = mime.formatted();
    mail::smtp::send(
        &account.smtp_host,
        account.smtp_port,
        &account.username,
        &credential,
        mime,
    )
    .await?;
    // why: best effort — the mail already left the machine, so a failed
    // Sent copy must never surface as a failed send (or reopen the draft).
    if let Err(err) = save_sent_copy(state, &account, &credential, &raw).await {
        eprintln!("sent copy for account {} failed: {err}", account.id);
    }
    // Same rule for the draft cleanup: a leftover draft is cosmetic, the
    // next save/sync can deal with it.
    if let Some(draft_id) = &message.draft_message_id {
        if let Err(err) =
            mail::drafts::delete_sent_draft(app, state, &account, &credential, draft_id).await
        {
            eprintln!("could not delete draft {draft_id} after send: {err}");
        }
    }
    Ok(())
}

/// Mirror a delivered message into the account's IMAP Sent folder so other
/// clients (webmail, phone) see it. Skipped for servers that store their own
/// copy (Gmail), where appending would duplicate the message.
async fn save_sent_copy(
    state: &AppState,
    account: &Account,
    credential: &Credential,
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
        credential,
    )
    .await?;
    let result = mail::imap::append(&mut session, &sent, "(\\Seen)", raw).await;
    let _ = session.logout().await;
    result
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
