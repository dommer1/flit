//! New-mail notifications: a pure planning step (which banners, what sound —
//! unit-tested) and a thin show step over tauri-plugin-notification.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::mail::sync::NewMail;
use crate::models::{Account, NotificationSettings};

/// Above this many new messages, one summary banner per account replaces the
/// per-message ones — a night's worth of mail must not carpet the screen.
const MAX_INDIVIDUAL: usize = 3;

/// The name macOS gives its built-in notification sound; our stored
/// "default" resolves to this (see mac-notification-sys).
const MACOS_DEFAULT_SOUND: &str = "NSUserNotificationDefaultSoundName";

/// One notification ready to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub title: String,
    pub body: String,
    /// macOS sound name; `None` = silent banner.
    pub sound: Option<String>,
}

/// Decide the banners for one account's newly arrived mail: the account's
/// overrides (NULL = inherit) applied on top of the global defaults.
pub fn plan(
    account: &Account,
    defaults: &NotificationSettings,
    new_mail: &[NewMail],
) -> Vec<Banner> {
    let enabled = account.notify_enabled.unwrap_or(defaults.enabled);
    if !enabled || new_mail.is_empty() {
        return Vec::new();
    }

    let sound = match account.notify_sound.as_deref().unwrap_or(&defaults.sound) {
        "none" => None,
        "default" => Some(MACOS_DEFAULT_SOUND.to_string()),
        name => Some(name.to_string()),
    };

    if new_mail.len() > MAX_INDIVIDUAL {
        return vec![Banner {
            title: account.name.clone(),
            body: format!("{} new messages", new_mail.len()),
            sound,
        }];
    }

    new_mail
        .iter()
        .map(|mail| Banner {
            title: sender_name(&mail.from),
            body: if mail.subject.trim().is_empty() {
                "(No subject)".to_string()
            } else {
                mail.subject.clone()
            },
            sound: sound.clone(),
        })
        .collect()
}

/// Show the banners. Failures are logged, never propagated — a notification
/// is not worth failing a finished sync over.
pub fn show(app: &AppHandle, banners: &[Banner]) {
    for banner in banners {
        let mut builder = app
            .notification()
            .builder()
            .title(&banner.title)
            .body(&banner.body);
        if let Some(sound) = &banner.sound {
            builder = builder.sound(sound);
        }
        if let Err(err) = builder.show() {
            eprintln!("new-mail notification failed: {err}");
        }
    }
}

/// The macOS system alert sounds the settings pane offers. Doubles as the
/// preview whitelist — nothing outside this list ever reaches afplay.
/// Mirrors SOUNDS in src/lib/NotificationsPane.svelte.
const SYSTEM_SOUNDS: [&str; 14] = [
    "Basso",
    "Blow",
    "Bottle",
    "Frog",
    "Funk",
    "Glass",
    "Hero",
    "Morse",
    "Ping",
    "Pop",
    "Purr",
    "Sosumi",
    "Submarine",
    "Tink",
];

/// Absolute path of a previewable system sound. `None` for everything else:
/// "none" is silence and "default" is not a file (it is a notification-API
/// sound name) — neither has anything to play.
fn sound_file(sound: &str) -> Option<String> {
    SYSTEM_SOUNDS
        .contains(&sound)
        .then(|| format!("/System/Library/Sounds/{sound}.aiff"))
}

/// Play a short preview of a sound picked in settings. Unknown names are a
/// silent no-op, not an error.
pub fn preview(sound: &str) {
    let Some(path) = sound_file(sound) else {
        return;
    };
    // why afplay: macOS ships it, it plays one file and exits — no audio
    // crate dependency for a settings-pane nicety.
    match tokio::process::Command::new("afplay").arg(path).spawn() {
        // why wait on a task: an unwaited unix child would linger as a
        // zombie process until the app quits.
        Ok(mut child) => {
            tauri::async_runtime::spawn(async move {
                let _ = child.wait().await;
            });
        }
        Err(err) => eprintln!("sound preview failed: {err}"),
    }
}

/// The display-name half of a stored `Name <addr>` sender, degrading to the
/// whole string for bare addresses. Mirrors senderName in src/lib/format.ts.
fn sender_name(from: &str) -> String {
    let trimmed = from.trim();
    let name = match trimmed.rfind('<') {
        Some(open) if trimmed.ends_with('>') => trimmed[..open].trim(),
        _ => "",
    };
    let name = name.trim_matches('"').trim();
    if name.is_empty() {
        trimmed.to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(notify_enabled: Option<bool>, notify_sound: Option<&str>) -> Account {
        Account {
            id: 1,
            name: "Work".to_string(),
            email: "me@example.com".to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            username: "me@example.com".to_string(),
            last_error: None,
            checked_at: None,
            color: None,
            signature_id: None,
            notify_enabled,
            notify_sound: notify_sound.map(str::to_string),
            default_alias_id: None,
        }
    }

    fn mail(from: &str, subject: &str) -> NewMail {
        NewMail {
            from: from.to_string(),
            subject: subject.to_string(),
        }
    }

    fn defaults() -> NotificationSettings {
        NotificationSettings::default()
    }

    #[test]
    fn no_new_mail_means_no_banners() {
        assert!(plan(&account(None, None), &defaults(), &[]).is_empty());
    }

    #[test]
    fn global_off_silences_inheriting_accounts() {
        let off = NotificationSettings {
            enabled: false,
            ..defaults()
        };
        let mail = [mail("Alice <a@example.com>", "Hi")];

        assert!(plan(&account(None, None), &off, &mail).is_empty());
        // ...but an explicit per-account override still wins.
        assert_eq!(plan(&account(Some(true), None), &off, &mail).len(), 1);
    }

    #[test]
    fn account_override_can_disable_despite_global_on() {
        let mail = [mail("Alice <a@example.com>", "Hi")];

        assert!(plan(&account(Some(false), None), &defaults(), &mail).is_empty());
    }

    #[test]
    fn few_messages_become_individual_banners() {
        let banners = plan(
            &account(None, None),
            &defaults(),
            &[
                mail("Alice <a@example.com>", "First"),
                mail("\"Bob B.\" <b@example.com>", "  "),
            ],
        );

        assert_eq!(
            banners,
            vec![
                Banner {
                    title: "Alice".to_string(),
                    body: "First".to_string(),
                    sound: Some(MACOS_DEFAULT_SOUND.to_string()),
                },
                Banner {
                    title: "Bob B.".to_string(),
                    body: "(No subject)".to_string(),
                    sound: Some(MACOS_DEFAULT_SOUND.to_string()),
                },
            ]
        );
    }

    #[test]
    fn many_messages_collapse_into_one_summary() {
        let stack: Vec<NewMail> = (0..5)
            .map(|i| mail("Alice <a@example.com>", &format!("Msg {i}")))
            .collect();

        let banners = plan(&account(None, None), &defaults(), &stack);

        assert_eq!(
            banners,
            vec![Banner {
                title: "Work".to_string(),
                body: "5 new messages".to_string(),
                sound: Some(MACOS_DEFAULT_SOUND.to_string()),
            }]
        );
    }

    #[test]
    fn sound_resolution_inherits_then_overrides() {
        let mail = [mail("Alice <a@example.com>", "Hi")];
        let named_default = NotificationSettings {
            sound: "Glass".to_string(),
            ..defaults()
        };

        // Inherit the global named sound.
        let inherited = plan(&account(None, None), &named_default, &mail);
        assert_eq!(inherited[0].sound.as_deref(), Some("Glass"));

        // Account override beats the global default.
        let overridden = plan(&account(None, Some("Ping")), &named_default, &mail);
        assert_eq!(overridden[0].sound.as_deref(), Some("Ping"));

        // "none" means a silent banner.
        let silent = plan(&account(None, Some("none")), &named_default, &mail);
        assert_eq!(silent[0].sound, None);
    }

    #[test]
    fn sound_file_resolves_only_whitelisted_names() {
        assert_eq!(
            sound_file("Ping").as_deref(),
            Some("/System/Library/Sounds/Ping.aiff")
        );

        // "default" and "none" have no file to play; arbitrary strings must
        // never reach the afplay process.
        assert_eq!(sound_file("default"), None);
        assert_eq!(sound_file("none"), None);
        assert_eq!(sound_file("../../etc/passwd"), None);
    }

    #[test]
    fn sender_name_extracts_the_display_half() {
        assert_eq!(sender_name("Alice <a@example.com>"), "Alice");
        assert_eq!(sender_name("\"Alice A.\" <a@example.com>"), "Alice A.");
        assert_eq!(sender_name("a@example.com"), "a@example.com");
        assert_eq!(sender_name("  <a@example.com>"), "<a@example.com>");
    }
}
