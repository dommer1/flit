use sqlx::SqlitePool;

use crate::error::AppError;
use crate::models::RemoteImagePolicy;

const REMOTE_IMAGES_KEY: &str = "remote_images";

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
}
