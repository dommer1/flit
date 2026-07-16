use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{NotificationSettings, RemoteImagePolicy};

const REMOTE_IMAGES_KEY: &str = "remote_images";
const NOTIFICATIONS_ENABLED_KEY: &str = "notifications_enabled";
const NOTIFICATION_SOUND_KEY: &str = "notification_sound";
const SYNC_INTERVAL_KEY: &str = "sync_interval_minutes";

async fn value(pool: &SqlitePool, key: &str) -> Result<Option<String>, AppError> {
    Ok(
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

async fn upsert(pool: &SqlitePool, key: &str, value: &str) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// Stored notification settings. Each field that is missing or unparsable
/// falls back to its default independently — one corrupt row never drags the
/// other settings down with it.
pub async fn notification_settings(pool: &SqlitePool) -> Result<NotificationSettings, AppError> {
    let defaults = NotificationSettings::default();
    let enabled = match value(pool, NOTIFICATIONS_ENABLED_KEY).await?.as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => defaults.enabled,
    };
    let sound = value(pool, NOTIFICATION_SOUND_KEY)
        .await?
        .filter(|sound| !sound.is_empty())
        .unwrap_or(defaults.sound);
    let sync_interval_minutes = value(pool, SYNC_INTERVAL_KEY)
        .await?
        .and_then(|minutes| minutes.parse::<i64>().ok())
        .filter(|minutes| *minutes >= 0)
        .unwrap_or(defaults.sync_interval_minutes);
    Ok(NotificationSettings {
        enabled,
        sound,
        sync_interval_minutes,
    })
}

pub async fn set_notification_settings(
    pool: &SqlitePool,
    settings: &NotificationSettings,
) -> Result<(), AppError> {
    let enabled = if settings.enabled { "true" } else { "false" };
    upsert(pool, NOTIFICATIONS_ENABLED_KEY, enabled).await?;
    upsert(pool, NOTIFICATION_SOUND_KEY, &settings.sound).await?;
    upsert(
        pool,
        SYNC_INTERVAL_KEY,
        &settings.sync_interval_minutes.to_string(),
    )
    .await?;
    Ok(())
}

/// The stored remote-image policy, falling back to the default (Ask) when
/// nothing was saved yet.
pub async fn remote_image_policy(pool: &SqlitePool) -> Result<RemoteImagePolicy, AppError> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(REMOTE_IMAGES_KEY)
        .fetch_optional(pool)
        .await?;
    Ok(value
        .as_deref()
        .map(RemoteImagePolicy::parse)
        .unwrap_or_default())
}

pub async fn set_remote_image_policy(
    pool: &SqlitePool,
    policy: RemoteImagePolicy,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(REMOTE_IMAGES_KEY)
    .bind(policy.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    #[tokio::test]
    async fn defaults_to_ask_and_roundtrips() {
        let pool = test_pool().await;

        assert_eq!(
            remote_image_policy(&pool).await.unwrap(),
            RemoteImagePolicy::Ask
        );

        set_remote_image_policy(&pool, RemoteImagePolicy::Always)
            .await
            .unwrap();
        assert_eq!(
            remote_image_policy(&pool).await.unwrap(),
            RemoteImagePolicy::Always
        );

        // Changing it again overwrites instead of duplicating the key.
        set_remote_image_policy(&pool, RemoteImagePolicy::Block)
            .await
            .unwrap();
        assert_eq!(
            remote_image_policy(&pool).await.unwrap(),
            RemoteImagePolicy::Block
        );
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    async fn unknown_stored_value_falls_back_to_ask() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('remote_images', 'yolo')")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            remote_image_policy(&pool).await.unwrap(),
            RemoteImagePolicy::Ask
        );
    }

    #[tokio::test]
    async fn notification_settings_default_to_on_with_system_sound() {
        let pool = test_pool().await;

        let settings = notification_settings(&pool).await.unwrap();

        assert!(settings.enabled);
        assert_eq!(settings.sound, "default");
        assert_eq!(settings.sync_interval_minutes, 3);
    }

    #[tokio::test]
    async fn notification_settings_roundtrip_without_duplicating_keys() {
        let pool = test_pool().await;
        let wanted = NotificationSettings {
            enabled: false,
            sound: "Ping".to_string(),
            sync_interval_minutes: 15,
        };

        set_notification_settings(&pool, &wanted).await.unwrap();
        assert_eq!(notification_settings(&pool).await.unwrap(), wanted);

        // Saving again overwrites the same three rows instead of adding more.
        set_notification_settings(&pool, &wanted).await.unwrap();
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 3);
    }

    #[tokio::test]
    async fn corrupt_notification_values_fall_back_to_defaults() {
        let pool = test_pool().await;
        for (key, value) in [
            ("notifications_enabled", "yolo"),
            ("notification_sound", ""),
            ("sync_interval_minutes", "-5"),
        ] {
            sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
                .bind(key)
                .bind(value)
                .execute(&pool)
                .await
                .unwrap();
        }

        let settings = notification_settings(&pool).await.unwrap();

        assert_eq!(settings, NotificationSettings::default());
    }
}
