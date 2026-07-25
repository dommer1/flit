use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::{
    DateFormat, DateTimeFormat, NotificationSettings, RemoteImagePolicy, SwipeAction, SwipeActions,
    ThreadOrder, TimeFormat,
};

const REMOTE_IMAGES_KEY: &str = "remote_images";
const THREAD_ORDER_KEY: &str = "thread_order";
const DATE_FORMAT_KEY: &str = "date_format";
const TIME_FORMAT_KEY: &str = "time_format";
const NOTIFICATIONS_ENABLED_KEY: &str = "notifications_enabled";
const NOTIFICATION_SOUND_KEY: &str = "notification_sound";
const SYNC_INTERVAL_KEY: &str = "sync_interval_minutes";
const SWIPE_LEFT_KEY: &str = "swipe_left";
const SWIPE_RIGHT_KEY: &str = "swipe_right";

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

/// The stored conversation-view order; missing or corrupt values fall back
/// to the default (newest message at the bottom).
pub async fn thread_order(pool: &SqlitePool) -> Result<ThreadOrder, AppError> {
    Ok(value(pool, THREAD_ORDER_KEY)
        .await?
        .as_deref()
        .map(ThreadOrder::parse)
        .unwrap_or_default())
}

pub async fn set_thread_order(pool: &SqlitePool, order: ThreadOrder) -> Result<(), AppError> {
    upsert(pool, THREAD_ORDER_KEY, order.as_str()).await
}

/// The stored date/time display format. The halves are read independently,
/// so an unreadable date pattern still leaves the clock preference intact.
pub async fn date_time_format(pool: &SqlitePool) -> Result<DateTimeFormat, AppError> {
    Ok(DateTimeFormat {
        date: value(pool, DATE_FORMAT_KEY)
            .await?
            .as_deref()
            .map(DateFormat::parse)
            .unwrap_or_default(),
        time: value(pool, TIME_FORMAT_KEY)
            .await?
            .as_deref()
            .map(TimeFormat::parse)
            .unwrap_or_default(),
    })
}

pub async fn set_date_time_format(
    pool: &SqlitePool,
    format: DateTimeFormat,
) -> Result<(), AppError> {
    upsert(pool, DATE_FORMAT_KEY, format.date.as_str()).await?;
    upsert(pool, TIME_FORMAT_KEY, format.time.as_str()).await?;
    Ok(())
}

/// The configured swipe actions; a missing or corrupt side falls back to
/// that side's shipped default (left = Archive, right = ToggleRead).
pub async fn swipe_actions(pool: &SqlitePool) -> Result<SwipeActions, AppError> {
    let defaults = SwipeActions::default();
    Ok(SwipeActions {
        left: swipe_side(pool, SWIPE_LEFT_KEY)
            .await?
            .unwrap_or(defaults.left),
        right: swipe_side(pool, SWIPE_RIGHT_KEY)
            .await?
            .unwrap_or(defaults.right),
    })
}

async fn swipe_side(pool: &SqlitePool, key: &str) -> Result<Option<SwipeAction>, AppError> {
    Ok(value(pool, key)
        .await?
        .as_deref()
        .and_then(SwipeAction::parse))
}

pub async fn set_swipe_actions(pool: &SqlitePool, actions: SwipeActions) -> Result<(), AppError> {
    upsert(pool, SWIPE_LEFT_KEY, actions.left.as_str()).await?;
    upsert(pool, SWIPE_RIGHT_KEY, actions.right.as_str()).await?;
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
    async fn swipe_actions_default_and_roundtrip() {
        let pool = test_pool().await;

        // Nothing stored yet — the shipped defaults (today's hardcoded gesture).
        assert_eq!(
            swipe_actions(&pool).await.unwrap(),
            SwipeActions {
                left: SwipeAction::Archive,
                right: SwipeAction::ToggleRead,
            }
        );

        let chosen = SwipeActions {
            left: SwipeAction::Trash,
            right: SwipeAction::Reply,
        };
        set_swipe_actions(&pool, chosen).await.unwrap();
        assert_eq!(swipe_actions(&pool).await.unwrap(), chosen);

        // Saving again overwrites the two keys instead of duplicating them.
        let again = SwipeActions {
            left: SwipeAction::None,
            right: SwipeAction::Archive,
        };
        set_swipe_actions(&pool, again).await.unwrap();
        assert_eq!(swipe_actions(&pool).await.unwrap(), again);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 2);
    }

    #[tokio::test]
    async fn unknown_swipe_value_falls_back_per_side() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('swipe_left', 'yolo')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('swipe_right', 'reply')")
            .execute(&pool)
            .await
            .unwrap();

        // The corrupt side falls back to its own default; the valid side sticks.
        assert_eq!(
            swipe_actions(&pool).await.unwrap(),
            SwipeActions {
                left: SwipeAction::Archive,
                right: SwipeAction::Reply,
            }
        );
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
    async fn thread_order_defaults_to_newest_last_and_roundtrips() {
        let pool = test_pool().await;

        assert_eq!(thread_order(&pool).await.unwrap(), ThreadOrder::NewestLast);

        set_thread_order(&pool, ThreadOrder::NewestFirst)
            .await
            .unwrap();
        assert_eq!(thread_order(&pool).await.unwrap(), ThreadOrder::NewestFirst);

        // Changing it again overwrites instead of duplicating the key.
        set_thread_order(&pool, ThreadOrder::NewestLast)
            .await
            .unwrap();
        assert_eq!(thread_order(&pool).await.unwrap(), ThreadOrder::NewestLast);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    async fn corrupt_thread_order_falls_back_to_newest_last() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('thread_order', 'yolo')")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(thread_order(&pool).await.unwrap(), ThreadOrder::NewestLast);
    }

    #[tokio::test]
    async fn date_time_format_defaults_to_the_system_locale_and_roundtrips() {
        let pool = test_pool().await;

        assert_eq!(
            date_time_format(&pool).await.unwrap(),
            DateTimeFormat::default()
        );

        let wanted = DateTimeFormat {
            date: DateFormat::DayMonthYearDot,
            time: TimeFormat::Hour24,
        };
        set_date_time_format(&pool, wanted).await.unwrap();
        assert_eq!(date_time_format(&pool).await.unwrap(), wanted);

        // Saving again overwrites the same two rows instead of adding more.
        set_date_time_format(&pool, wanted).await.unwrap();
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 2);
    }

    #[tokio::test]
    async fn each_corrupt_format_half_falls_back_on_its_own() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('date_format', 'yolo')")
            .execute(&pool)
            .await
            .unwrap();
        set_date_time_format(
            &pool,
            DateTimeFormat {
                date: DateFormat::System,
                time: TimeFormat::Hour12,
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE settings SET value = 'yolo' WHERE key = 'date_format'")
            .execute(&pool)
            .await
            .unwrap();

        let format = date_time_format(&pool).await.unwrap();

        // The unreadable half reverts, the readable one is kept.
        assert_eq!(format.date, DateFormat::System);
        assert_eq!(format.time, TimeFormat::Hour12);
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
