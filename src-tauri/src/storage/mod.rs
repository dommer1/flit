pub mod accounts;
pub mod aliases;
pub mod contacts;
pub mod mailboxes;
pub mod messages;
pub mod scheduled;
pub mod search;
pub mod settings;
pub mod signatures;

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
        // why: SQLite ships with foreign keys OFF per connection — without
        // this, deleting an account would strand its cached messages.
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    // why: a modest pool — SQLite allows only one writer at a time anyway,
    // extra connections only help concurrent reads.
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    MIGRATOR.run(&pool).await?;

    // why: data fix, not a schema change — migrations are pure SQL but the
    // snippet rules live in Rust, so a one-time backfill corrects previews
    // cached by older builds (idempotent; see backfill_url_snippets).
    let fixed = messages::backfill_url_snippets(&pool).await?;
    if fixed > 0 {
        eprintln!("backfilled {fixed} message snippet(s) to strip leading URLs");
    }

    // why: same idea for snippets polluted by quoted reply history — the
    // quote splitter now stops before it, so cached previews catch up once.
    let fixed = messages::backfill_quoted_snippets(&pool).await?;
    if fixed > 0 {
        eprintln!("backfilled {fixed} message snippet(s) to stop before quoted history");
    }

    // why: same idea for snippets carrying literal "&zwnj;" entity padding —
    // the parser now strips it, so cached previews catch up once.
    let fixed = messages::backfill_entity_snippets(&pool).await?;
    if fixed > 0 {
        eprintln!("backfilled {fixed} message snippet(s) to drop entity padding");
    }

    // why: rows cached before the has_attachments column existed know their
    // attachments only through the metadata table — adopt that once
    // (idempotent; new rows are kept in sync by upsert/set_body).
    sqlx::query(
        "UPDATE messages SET has_attachments = 1
         WHERE has_attachments = 0
           AND id IN (SELECT DISTINCT message_id FROM message_attachments)",
    )
    .execute(&pool)
    .await?;

    // why: sync only harvests headers it newly fetches — messages cached
    // before the contacts table existed seed it here, once (no-op after).
    let seeded = contacts::backfill(&pool).await?;
    if seeded > 0 {
        eprintln!("seeded contacts from {seeded} cached message(s)");
    }

    Ok(pool)
}

/// In-memory database with migrations applied, for tests.
#[cfg(test)]
pub(crate) async fn test_pool() -> SqlitePool {
    use std::str::FromStr;

    // why: every connection to :memory: opens its OWN empty database, so the
    // pool is capped at one connection to keep all queries on the same DB.
    let options = SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
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

    #[tokio::test]
    async fn migrations_create_messages_table() {
        let pool = test_pool().await;

        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'messages'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(count, 1);
    }

    fn sample_account() -> crate::models::NewAccount {
        crate::models::NewAccount {
            name: "Personal".to_string(),
            email: "a@example.com".to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            username: "a@example.com".to_string(),
        }
    }

    #[tokio::test]
    async fn deleting_an_account_cascades_its_messages() {
        let pool = test_pool().await;
        let account = crate::storage::accounts::insert(&pool, &sample_account())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO messages (account_id, uid, uid_validity, date) VALUES (?, 1, 1, '2026-01-01T00:00:00Z')",
        )
        .bind(account.id)
        .execute(&pool)
        .await
        .unwrap();

        crate::storage::accounts::delete(&pool, account.id)
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    async fn fts_hits(pool: &SqlitePool, needle: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM messages_fts WHERE messages_fts MATCH ?")
            .bind(needle)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn fts_matches_subject_ignoring_diacritics() {
        let pool = test_pool().await;
        let account = crate::storage::accounts::insert(&pool, &sample_account())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO messages (account_id, uid, uid_validity, date, subject, to_addr)
             VALUES (?, 1, 1, '2026-07-01T00:00:00Z', 'Faktúra za jún', 'Ján <jan@example.com>')",
        )
        .bind(account.id)
        .execute(&pool)
        .await
        .unwrap();

        // Slovak reality: people type without diacritics; both must hit.
        assert_eq!(fts_hits(&pool, "faktura").await, 1);
        assert_eq!(fts_hits(&pool, "faktúra").await, 1);
        // The new to_addr column is indexed too.
        assert_eq!(fts_hits(&pool, "to_addr: jan").await, 1);
    }

    #[tokio::test]
    async fn fts_index_follows_body_updates_and_deletes() {
        let pool = test_pool().await;
        let account = crate::storage::accounts::insert(&pool, &sample_account())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO messages (account_id, uid, uid_validity, date, subject)
             VALUES (?, 1, 1, '2026-07-01T00:00:00Z', 'Hello')",
        )
        .bind(account.id)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(fts_hits(&pool, "ponuka").await, 0);

        sqlx::query("UPDATE messages SET body_text = 'cenová ponuka v prílohe'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(fts_hits(&pool, "ponuka").await, 1);

        sqlx::query("DELETE FROM messages")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(fts_hits(&pool, "ponuka").await, 0);
    }

    #[tokio::test]
    async fn deleting_an_account_keeps_fts_in_sync() {
        let pool = test_pool().await;
        let account = crate::storage::accounts::insert(&pool, &sample_account())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO messages (account_id, uid, uid_validity, date, subject)
             VALUES (?, 1, 1, '2026-07-01T00:00:00Z', 'zmluva o dielo')",
        )
        .bind(account.id)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(fts_hits(&pool, "zmluva").await, 1);

        crate::storage::accounts::delete(&pool, account.id)
            .await
            .unwrap();

        // The cascade delete must not leave stale index entries behind.
        assert_eq!(fts_hits(&pool, "zmluva").await, 0);
    }
}
