//! Sender-domain avatars: favicon lookup, cached per domain.
//!
//! Avatars are keyed on the sender's *domain*, never on an address. A domain
//! is looked up at most once per expiry window and then served from here, so
//! the icon host cannot learn which message was opened or when — only that
//! this client has, at some point, seen mail from that domain.
//!
//! SECURITY: the domain arrives from a From header, so a sender chooses it.
//! `is_fetchable_domain` is the guard — see its docs. What it cannot cover:
//! a public name whose DNS answer points at a private address, which would
//! aim one GET at the user's own network. The response is only ever cached
//! as image bytes, so nothing can be read back out of it; closing the hole
//! properly needs resolution-time IP filtering, which reqwest does not offer
//! out of the box. Worth revisiting if this path ever grows.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use base64::Engine as _;
use futures::StreamExt;
use sqlx::SqlitePool;

/// An icon is small; anything larger is not one, whatever it claims to be.
const MAX_ICON_BYTES: usize = 256 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const CONCURRENT_FETCHES: usize = 6;
/// One list load must never fan out to hundreds of hosts.
const MAX_DOMAINS: usize = 100;

/// Raster image types a favicon may legitimately arrive as. Deliberately no
/// image/svg+xml: SVG is an active format, and nothing here needs it.
const ICON_TYPES: [&str; 6] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/x-icon",
    "image/vnd.microsoft.icon",
];

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

/// Resolve each sender domain to a data: URI for its icon — cache first,
/// network second. Domains with no icon simply stay absent from the result,
/// and the caller falls back to the monogram.
///
/// Callers must check the user's setting first: reaching this function at all
/// means avatar lookups are switched on.
pub async fn load_avatars(pool: &SqlitePool, domains: &[String]) -> HashMap<String, String> {
    prune_expired(pool).await;

    let mut resolved = HashMap::new();
    let mut to_fetch = Vec::new();
    let mut seen = HashSet::new();
    for domain in domains {
        let domain = domain.to_ascii_lowercase();
        if !seen.insert(domain.clone()) || !is_fetchable_domain(&domain) {
            continue;
        }
        if resolved.len() + to_fetch.len() >= MAX_DOMAINS {
            break;
        }
        match lookup(pool, &domain).await {
            Some(Cached::Icon { content_type, data }) => {
                resolved.insert(domain, data_uri(&content_type, &data));
            }
            // Known to have nothing — the whole point of the negative cache.
            Some(Cached::Missing) => {}
            None => to_fetch.push(domain),
        }
    }
    if to_fetch.is_empty() {
        return resolved;
    }

    let Ok(client) = client() else {
        return resolved;
    };
    let fetched = futures::stream::iter(to_fetch.into_iter().map(|domain| {
        let client = client.clone();
        async move {
            let icon = fetch_icon(&client, &domain).await;
            (domain, icon)
        }
    }))
    .buffer_unordered(CONCURRENT_FETCHES)
    .collect::<Vec<_>>()
    .await;

    for (domain, icon) in fetched {
        match icon {
            Some((content_type, bytes)) => {
                store_icon(pool, &domain, &content_type, &bytes).await;
                resolved.insert(domain, data_uri(&content_type, &bytes));
            }
            // why store the failure: without it every sync re-asks the same
            // silent hosts, which is exactly the repeated contact this design
            // exists to avoid.
            None => store_missing(pool, &domain).await,
        }
    }
    resolved
}

/// Whether a string is a public hostname safe to splice into a fetch URL.
///
/// This is the guard on the whole network path: the domain comes from a From
/// header, which any sender controls. Rejecting anything with a path, port,
/// space or userinfo keeps it from reshaping the URL; rejecting bare labels,
/// IP literals and reserved suffixes keeps the app off the user's own LAN.
fn is_fetchable_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    // A name with no dot is a local machine, not a domain on the internet.
    if labels.len() < 2 {
        return false;
    }
    if labels.iter().any(|label| {
        label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }) {
        return false;
    }
    let tld = labels[labels.len() - 1].to_ascii_lowercase();
    // An all-digit final label means this is an IPv4 literal.
    if tld.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    !matches!(
        tld.as_str(),
        "local"
            | "localhost"
            | "internal"
            | "intranet"
            | "lan"
            | "home"
            | "corp"
            | "test"
            | "example"
            | "invalid"
            | "onion"
    )
}

/// One shared client: TLS-only (redirects included), bounded redirects, no
/// Referer, no cookie jar, and a bare browser UA rather than a string that
/// would identify this app across every host it touches.
fn client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .referer(false)
        .user_agent("Mozilla/5.0")
        .build()
}

/// GET a domain's favicon; `None` for anything that isn't a well-behaved
/// raster image response. A miss is normal, not an error.
async fn fetch_icon(client: &reqwest::Client, domain: &str) -> Option<(String, Vec<u8>)> {
    let mut response = client
        .get(format!("https://{domain}/favicon.ico"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)?
        .to_str()
        .ok()?
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase();
    // why an allowlist: a missing favicon is often answered with a 200 and an
    // HTML error page, and a served type is a claim, not a guarantee.
    if !ICON_TYPES.contains(&content_type.as_str()) {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_ICON_BYTES as u64)
    {
        return None;
    }
    // why chunked: Content-Length is optional and unverified, so the cap has
    // to hold against the actual stream.
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_ICON_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return None;
    }
    Some((content_type, bytes))
}

fn data_uri(content_type: &str, data: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        content_type,
        base64::engine::general_purpose::STANDARD.encode(data)
    )
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

    #[test]
    fn rejects_anything_that_is_not_a_public_hostname() {
        for domain in [
            "",
            "localhost",
            // Bare addresses and paths would splice into the fetch URL.
            "example.com/../admin",
            "example.com:8080",
            "user@example.com",
            "exa mple.com",
            // No dot: a bare label is a LAN name, not a public domain.
            "intranet",
            ".example.com",
            "example.com.",
            "example..com",
            "-example.com",
            // IP literals bypass the whole point of a domain key.
            "192.168.1.1",
            "127.0.0.1",
            // Reserved suffixes that only ever name a local machine.
            "router.local",
            "wiki.internal",
            "host.localhost",
        ] {
            assert!(!is_fetchable_domain(domain), "should reject {domain:?}");
        }
    }

    #[test]
    fn accepts_ordinary_public_domains() {
        for domain in [
            "example.com",
            "mail.google.com",
            "bbc.co.uk",
            "my-shop.example.com",
            "xn--80ak6aa92e.com",
        ] {
            assert!(is_fetchable_domain(domain), "should accept {domain:?}");
        }
    }

    #[tokio::test]
    async fn serves_cached_icons_without_touching_the_network() {
        let pool = test_pool().await;
        store_icon(&pool, "example.com", "image/png", b"\x89PNG").await;
        store_missing(&pool, "plain.example").await;

        // Every domain is already known, so nothing here can reach the network:
        // a hit, a known miss, and a name that is never fetchable.
        let resolved = load_avatars(
            &pool,
            &[
                "example.com".to_string(),
                "plain.example".to_string(),
                "localhost".to_string(),
            ],
        )
        .await;

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved["example.com"], "data:image/png;base64,iVBORw==");
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
