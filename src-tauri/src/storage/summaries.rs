//! The on-device summary cache — a convenience cache, not an archive: rows
//! older than a month are dropped on the next write.

use sqlx::SqlitePool;

use crate::error::AppError;

const MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;

pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>, AppError> {
    Ok(
        sqlx::query_scalar("SELECT text FROM summaries WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn put(pool: &SqlitePool, key: &str, text: &str) -> Result<(), AppError> {
    prune_expired(pool).await?;
    sqlx::query(
        "INSERT INTO summaries (key, text, created_at) VALUES (?, ?, unixepoch())
         ON CONFLICT (key) DO UPDATE SET text = excluded.text, created_at = excluded.created_at",
    )
    .bind(key)
    .bind(text)
    .execute(pool)
    .await?;
    Ok(())
}

async fn prune_expired(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query("DELETE FROM summaries WHERE created_at < unixepoch() - ?")
        .bind(MAX_AGE_SECS)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    #[tokio::test]
    async fn stores_and_replaces_by_key() {
        let pool = test_pool().await;

        assert_eq!(get(&pool, "k").await.unwrap(), None);

        put(&pool, "k", "first").await.unwrap();
        assert_eq!(get(&pool, "k").await.unwrap().as_deref(), Some("first"));

        put(&pool, "k", "second").await.unwrap();
        assert_eq!(get(&pool, "k").await.unwrap().as_deref(), Some("second"));
        assert_eq!(get(&pool, "other").await.unwrap(), None);
    }

    #[tokio::test]
    async fn old_rows_are_dropped_on_the_next_write() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO summaries (key, text, created_at) VALUES ('old', 'x', 0)")
            .execute(&pool)
            .await
            .unwrap();

        put(&pool, "new", "y").await.unwrap();

        assert_eq!(get(&pool, "old").await.unwrap(), None);
        assert_eq!(get(&pool, "new").await.unwrap().as_deref(), Some("y"));
    }
}
