pub mod auth;
mod commands;
pub mod error;
pub mod mail;
mod models;
pub mod state;
pub mod storage;

use tauri::Manager;

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // why: .setup() is synchronous, so block_on finishes the async DB
            // init (open + migrate) before any command can fire — commands may
            // then assume the pool always exists in state.
            let pool = tauri::async_runtime::block_on(storage::init(&data_dir.join("flit.db")))?;
            app.manage(AppState { pool });

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
            commands::list_messages,
            commands::sync_inbox,
            commands::get_message_body,
            commands::test_connection,
            commands::open_settings,
            commands::close_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
