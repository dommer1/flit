pub mod auth;
mod commands;
pub mod error;
mod idle;
pub mod mail;
mod models;
mod notify;
mod poller;
mod scheduler;
pub mod state;
pub mod storage;
pub mod timing;

use tauri::{Manager, Url};

use crate::state::AppState;

/// Should a new-window request from the webview be handed to the user's
/// default browser? Only web links are: the URL comes from a message, so its
/// scheme is untrusted input, and handing an arbitrary one to the OS would let
/// a sender choose which application launches.
fn opens_in_browser(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // why: .setup() is synchronous, so block_on finishes the async DB
            // init (open + migrate) before any command can fire — commands may
            // then assume the pool always exists in state.
            let pool = tauri::async_runtime::block_on(storage::init(&data_dir.join("flit.db")))?;
            app.manage(AppState::new(pool));

            // SECURITY (message-body rendering): the main window is built here
            // instead of by tauri.conf.json ("create": false) because only the
            // builder can carry a new-window handler — and that handler is how
            // message-body links reach the browser. Bodies render with
            // `<base target="_blank">` in a frame whose sandbox blocks both
            // navigation and script (WebKit runs no listener there at all, see
            // mail::sanitize), so a link click surfaces as a new-window
            // request. Every one of them is DENIED — no second webview is ever
            // created, nothing remote loads inside the app — and a plain
            // http(s) link is handed to the user's default browser instead.
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .cloned()
                .ok_or("no main window in tauri.conf.json")?;
            tauri::WebviewWindowBuilder::from_config(app.handle(), &window_config)?
                .on_new_window(|url, _features| {
                    if opens_in_browser(&url) {
                        if let Err(err) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
                        {
                            eprintln!("failed to open {url} in the default browser: {err}");
                        }
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()?;

            // The send-later scheduler: delivers parked messages when their
            // time comes; runs for the whole life of the app.
            scheduler::spawn(app.handle().clone());

            // The background new-mail poll — syncs accounts on the interval
            // configured in Settings › Notifications so notifications work
            // while the app idles.
            poller::spawn(app.handle().clone());

            // IMAP IDLE listeners, when push is on: an open connection per
            // account so new inbox mail arrives as the server announces it.
            // The poll above keeps running — under push its interval covers
            // the folders IDLE cannot watch.
            idle::spawn(app.handle().clone());

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
            commands::view_status,
            commands::list_thread,
            commands::search_messages,
            commands::sync_account,
            commands::refresh_account,
            commands::set_message_read,
            commands::move_to_trash,
            commands::archive_message,
            commands::move_message,
            commands::move_messages,
            commands::trash_messages,
            commands::archive_messages,
            commands::set_messages_read,
            commands::trash_thread,
            commands::archive_thread,
            commands::move_thread,
            commands::get_message_body,
            commands::get_message_quote,
            commands::thread_bodies,
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
            commands::discard_draft,
            commands::open_draft,
            commands::test_connection,
            commands::open_compose,
            commands::take_compose_draft,
            commands::close_compose,
            commands::open_settings,
            commands::close_settings,
            commands::get_remote_image_policy,
            commands::set_remote_image_policy,
            commands::get_avatar_lookup_enabled,
            commands::set_avatar_lookup_enabled,
            commands::load_domain_avatars,
            commands::get_notification_settings,
            commands::set_notification_settings,
            commands::set_account_notifications,
            commands::preview_notification_sound,
            commands::get_swipe_actions,
            commands::set_swipe_actions,
            commands::timing_enabled,
            commands::get_thread_order,
            commands::set_thread_order,
            commands::get_date_time_format,
            commands::set_date_time_format,
            commands::list_signatures,
            commands::create_signature,
            commands::update_signature,
            commands::delete_signature,
            commands::set_signature_accounts,
            commands::list_aliases,
            commands::add_alias,
            commands::update_alias,
            commands::delete_alias,
            commands::set_default_alias
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::opens_in_browser;
    use tauri::Url;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("test url parses")
    }

    #[test]
    fn hands_web_links_to_the_browser() {
        assert!(opens_in_browser(&url("https://example.com/offer")));
        assert!(opens_in_browser(&url("http://example.com")));
    }

    #[test]
    fn keeps_every_other_scheme_inside_the_app() {
        // A message names the URL, so the scheme is untrusted input: handing
        // an arbitrary one to the OS would let a sender pick which app
        // launches. Only web links leave, everything else dies here.
        assert!(!opens_in_browser(&url("file:///etc/passwd")));
        assert!(!opens_in_browser(&url("javascript:alert(1)")));
        assert!(!opens_in_browser(&url("mailto:x@example.com")));
        assert!(!opens_in_browser(&url("data:text/html,<b>x")));
    }
}
