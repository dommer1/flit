//! Opening app windows from backend code — the send queue reopens a draft on
//! undo or failure, not only the frontend's compose button.

use tauri::{AppHandle, Manager};

use crate::error::AppError;
use crate::models::OutgoingMessage;
use crate::state::AppState;

/// Open a native compose window seeded with `draft`. Every call opens its
/// own window (unique label), so several drafts can be in flight at once.
pub async fn open_compose_window(app: &AppHandle, draft: OutgoingMessage) -> Result<(), AppError> {
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
