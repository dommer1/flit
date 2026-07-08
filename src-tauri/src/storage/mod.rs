use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

use crate::error::AppError;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// Open the app database (creating the file if missing) and bring the schema
/// up to date.
pub async fn init(db_path: &Path) -> Result<SqlitePool, AppError> {
    // why: filename() instead of a "sqlite://…" URL — the macOS app data dir
    // contains a space ("Application Support") and a plain path sidesteps URL
    // parsing entirely.
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        // why: WAL lets readers proceed alongside a writer; sqlx keeps
        // SQLite's default journal mode (DELETE) unless set explicitly.
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    // why: a modest pool — SQLite allows only one writer at a time anyway,
    // extra connections only help concurrent reads.
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// In-memory database with migrations applied, for tests.
#[cfg(test)]
pub(crate) async fn test_pool() -> SqlitePool {
    use std::str::FromStr;

    // why: every connection to :memory: opens its OWN empty database, so the
    // pool is capped at one connection to keep all queries on the same DB.
    let options = SqliteConnectOptions::from_str("sqlite::memory:").unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_create_accounts_table() {
        let pool = test_pool().await;

        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'accounts'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(count, 1);
    }
}
