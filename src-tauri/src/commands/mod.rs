use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::AppError;
use crate::models::{Account, MessageBody, MessageHeader, NewAccount, OutgoingMessage};
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
    if let Err(err) = auth::set_password(inserted.id, password).await {
        storage::accounts::delete(&state.pool, inserted.id).await?;
        return Err(err);
    }
    // why: broadcast to every window — the settings window mutates accounts,
    // the main window listens and refetches its sidebar list.
    app.emit("accounts-changed", ())?;
    Ok(inserted)
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
    storage::accounts::delete(&state.pool, id).await?;
    app.emit("accounts-changed", ())?;
    Ok(())
}

/// Sync one account's INBOX headers into the local cache.
#[tauri::command]
pub async fn sync_inbox(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: i64,
) -> Result<(), AppError> {
    let account = storage::accounts::get(&state.pool, account_id).await?;
    // why: the password is read from the keychain at call time and lives only
    // on this task's stack — never in state, events, or logs.
    let result = async {
        let password = auth::get_password(account_id).await?;
        mail::sync::sync_inbox(&state.pool, &account, &password).await
    }
    .await;

    // why: every sync doubles as a health check — the recorded outcome is
    // what settings shows when credentials go stale server-side.
    let error_text = result.as_ref().err().map(ToString::to_string);
    storage::accounts::set_status(&state.pool, account_id, error_text.as_deref(), now_epoch())
        .await?;
    app.emit("accounts-changed", ())?;

    result?;
    app.emit("messages-changed", account_id)?;
    Ok(())
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
) -> Result<Vec<MessageHeader>, AppError> {
    storage::messages::list(&state.pool, account_id).await
}

/// Send a composed message through the sending account's SMTP server.
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    message: OutgoingMessage,
) -> Result<(), AppError> {
    let account = storage::accounts::get(&state.pool, message.account_id).await?;
    let mime = mail::smtp::build_message(&account.email, &message)?;
    // why: the password is read from the keychain at call time and lives only
    // on this task's stack — never in state, events, or logs.
    let password = auth::get_password(message.account_id).await?;
    mail::smtp::send(
        &account.smtp_host,
        account.smtp_port,
        &account.username,
        &password,
        mime,
    )
    .await
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
#[tauri::command]
pub async fn get_message_body(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: i64,
) -> Result<MessageBody, AppError> {
    let row = storage::messages::get_body(&state.pool, message_id).await?;
    if row.body_text.is_some() || row.body_html.is_some() {
        return Ok(sanitized_body(row.body_html, row.body_text));
    }

    let account = storage::accounts::get(&state.pool, row.account_id).await?;
    let password = auth::get_password(account.id).await?;
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
    Ok(sanitized_body(parsed.html, parsed.text))
}

// SECURITY: the single place message HTML is prepared for the frontend —
// everything goes through mail::sanitize::build_srcdoc, cached or fresh.
fn sanitized_body(html: Option<String>, text: Option<String>) -> MessageBody {
    MessageBody {
        html: html.as_deref().map(mail::sanitize::build_srcdoc),
        text,
    }
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
