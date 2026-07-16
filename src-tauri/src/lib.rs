pub mod auth;
mod commands;
pub mod error;
pub mod mail;
mod models;
mod scheduler;
pub mod state;
pub mod storage;

use tauri::Manager;

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // why: .setup() is synchronous, so block_on finishes the async DB
            // init (open + migrate) before any command can fire — commands may
            // then assume the pool always exists in state.
            let pool = tauri::async_runtime::block_on(storage::init(&data_dir.join("flit.db")))?;
            app.manage(AppState::new(pool));

            // The send-later scheduler: delivers parked messages when their
            // time comes; runs for the whole life of the app.
            scheduler::spawn(app.handle().clone());

            // why: the main window is transparent (tauri.conf.json) and this
            // NSVisualEffectView provides the actual backdrop — the frontend
            // leaves the sidebar area transparent so the blur shows through.
            #[cfg(target_os = "macos")]
            {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

                let window = app
                    .get_webview_window("main")
                    .ok_or("main window not found")?;
                apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None)?;
            }

            // why: macOS convention puts "Settings…" (⌘,) in the app menu; we
            // extend Tauri's default menu instead of rebuilding it from
            // scratch so all standard items (Edit, Window, …) stay intact.
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};

                let menu = Menu::default(app.handle())?;
                if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.first() {
                    // why: position 2 = right after "About" and its
                    // separator, where the HIG places Settings.
                    app_menu.insert_items(
                        &[
                            &MenuItem::with_id(app, "settings", "Settings…", true, Some("Cmd+,"))?,
                            &PredefinedMenuItem::separator(app)?,
                        ],
                        2,
                    )?;
                }
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    if event.id() == "settings" {
                        let app = app.clone();
                        // why: menu handlers are sync — spawn the async
                        // window-opening command instead of blocking here.
                        tauri::async_runtime::spawn(async move {
                            if let Err(err) = commands::open_settings(app).await {
                                eprintln!("failed to open settings window: {err}");
                            }
                        });
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::add_account,
            commands::delete_account,
            commands::set_account_color,
            commands::list_contacts,
            commands::list_mailboxes,
            commands::list_messages,
            commands::search_messages,
            commands::sync_account,
            commands::set_message_read,
            commands::move_to_trash,
            commands::archive_message,
            commands::move_message,
            commands::get_message_body,
            commands::inspect_attachments,
            commands::save_attachment,
            commands::save_all_attachments,
            commands::attachment_preview,
            commands::queue_send,
            commands::undo_send,
            commands::schedule_send,
            commands::list_scheduled,
            commands::send_scheduled_now,
            commands::cancel_scheduled,
            commands::save_draft,
            commands::open_draft,
            commands::test_connection,
            commands::open_compose,
            commands::take_compose_draft,
            commands::close_compose,
            commands::open_settings,
            commands::close_settings,
            commands::get_remote_image_policy,
            commands::set_remote_image_policy,
            commands::get_notification_settings,
            commands::set_notification_settings,
            commands::set_account_notifications,
            commands::list_signatures,
            commands::create_signature,
            commands::update_signature,
            commands::delete_signature,
            commands::set_signature_accounts
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
