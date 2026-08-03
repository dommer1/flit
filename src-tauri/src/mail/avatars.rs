//! Sender-domain avatars: the cache layer.
//!
//! Avatars are keyed on the sender's *domain*, never on an address. A domain
//! is looked up at most once per expiry window and then served from here, so
//! the icon host cannot learn which message was opened or when — only that
//! this client has, at some point, seen mail from that domain.

use sqlx::SqlitePool;

/// How long a found icon stays usable before it is looked up again.
const ICON_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;
/// Negative results expire sooner — a domain may add an icon later.
const MISSING_MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// What the cache knows about one domain.
#[derive(Debug, PartialEq)]
pub enum Cached {
    /// A usable icon: content type and bytes.
    Icon { content_type: String, data: Vec<u8> },
    /// Looked up before and found nothing — don't ask the network again.
    Missing,
}

/// The cached state of a domain, or `None` when it has never been looked up
/// (or its row has aged out and should be refreshed).
pub async fn lookup(pool: &SqlitePool, domain: &str) -> Option<Cached> {
    let row: Option<(Option<String>, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT content_type, data FROM domain_avatars WHERE domain = ?")
            .bind(domain)
            .fetch_optional(pool)
            .await
            .ok()?;
    // why: NULL data is the negative result, and it must stay distinguishable
    // from "no row at all" — one means don't ask again, the other means ask.
    match row? {
        (Some(content_type), Some(data)) => Some(Cached::Icon { content_type, data }),
        _ => Some(Cached::Missing),
    }
}

/// Remember an icon for a domain, replacing any earlier row.
pub async fn store_icon(pool: &SqlitePool, domain: &str, content_type: &str, data: &[u8]) {
    // why ignore the error: a failed cache write must degrade to "look it up
    // again next time", never to a failed avatar load.
    let _ = sqlx::query(
        "INSERT INTO domain_avatars (domain, content_type, data, fetched_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (domain) DO UPDATE SET
           content_type = excluded.content_type,
           data = excluded.data,
           fetched_at = excluded.fetched_at",
    )
    .bind(domain)
    .bind(content_type)
    .bind(data)
    .bind(now_epoch())
    .execute(pool)
    .await;
}

/// Remember that a domain has no usable icon.
pub async fn store_missing(pool: &SqlitePool, domain: &str) {
    let _ = sqlx::query(
        "INSERT INTO domain_avatars (domain, content_type, data, fetched_at)
         VALUES (?, NULL, NULL, ?)
         ON CONFLICT (domain) DO UPDATE SET
           content_type = NULL,
           data = NULL,
           fetched_at = excluded.fetched_at",
    )
    .bind(domain)
    .bind(now_epoch())
    .execute(pool)
    .await;
}

/// Drop rows past their age limit; icons and negative results expire on
/// separate clocks.
pub async fn prune_expired(pool: &SqlitePool) {
    let now = now_epoch();
    let _ = sqlx::query(
        "DELETE FROM domain_avatars
         WHERE (data IS NOT NULL AND fetched_at < ?)
            OR (data IS NULL AND fetched_at < ?)",
    )
    .bind(now - ICON_MAX_AGE_SECS)
    .bind(now - MISSING_MAX_AGE_SECS)
    .execute(pool)
    .await;
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    async fn age(pool: &SqlitePool, domain: &str, secs: i64) {
        sqlx::query("UPDATE domain_avatars SET fetched_at = ? WHERE domain = ?")
            .bind(now_epoch() - secs)
            .bind(domain)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn unknown_domain_is_not_cached() {
        let pool = test_pool().await;
        assert_eq!(lookup(&pool, "example.com").await, None);
    }

    #[tokio::test]
    async fn icon_roundtrips() {
        let pool = test_pool().await;
        store_icon(&pool, "example.com", "image/png", b"\x89PNG").await;

        assert_eq!(
            lookup(&pool, "example.com").await,
            Some(Cached::Icon {
                content_type: "image/png".to_string(),
                data: b"\x89PNG".to_vec(),
            })
        );
    }

    #[tokio::test]
    async fn storing_twice_replaces_the_row() {
        let pool = test_pool().await;
        store_icon(&pool, "example.com", "image/png", b"old").await;
        store_icon(&pool, "example.com", "image/gif", b"new").await;

        assert_eq!(
            lookup(&pool, "example.com").await,
            Some(Cached::Icon {
                content_type: "image/gif".to_string(),
                data: b"new".to_vec(),
            })
        );
    }

    #[tokio::test]
    async fn a_domain_without_an_icon_is_remembered_as_missing() {
        let pool = test_pool().await;
        store_missing(&pool, "example.com").await;

        assert_eq!(lookup(&pool, "example.com").await, Some(Cached::Missing));
    }

    #[tokio::test]
    async fn a_missing_result_can_be_replaced_by_an_icon_found_later() {
        let pool = test_pool().await;
        store_missing(&pool, "example.com").await;
        store_icon(&pool, "example.com", "image/png", b"\x89PNG").await;

        assert!(matches!(
            lookup(&pool, "example.com").await,
            Some(Cached::Icon { .. })
        ));
    }

    #[tokio::test]
    async fn negative_results_expire_sooner_than_icons() {
        let pool = test_pool().await;
        store_icon(&pool, "icon.example", "image/png", b"\x89PNG").await;
        store_missing(&pool, "plain.example").await;

        // Past the negative limit, still inside the icon limit.
        age(&pool, "icon.example", MISSING_MAX_AGE_SECS + 1).await;
        age(&pool, "plain.example", MISSING_MAX_AGE_SECS + 1).await;
        prune_expired(&pool).await;

        assert!(lookup(&pool, "icon.example").await.is_some());
        assert_eq!(lookup(&pool, "plain.example").await, None);
    }

    #[tokio::test]
    async fn icons_expire_once_past_their_own_limit() {
        let pool = test_pool().await;
        store_icon(&pool, "icon.example", "image/png", b"\x89PNG").await;

        age(&pool, "icon.example", ICON_MAX_AGE_SECS + 1).await;
        prune_expired(&pool).await;

        assert_eq!(lookup(&pool, "icon.example").await, None);
    }
}
