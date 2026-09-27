//! Backend fetching of remote (https) message images, on the user's explicit
//! terms (policy "always", or a per-message click under "ask").
//!
//! SECURITY: this is the ONLY network path a message can trigger, and never
//! without user consent. The webview stays offline — images come back as
//! data: URIs for the sanitizer, requests are TLS-only, carry no cookies and
//! no Referer, and known trackers (mail::trackers) are never fetched. What
//! this cannot hide: the sender's server still sees the client IP and the
//! time of the request — that is inherent to loading without a relay proxy.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine as _;
use futures::StreamExt;
use sqlx::SqlitePool;

use crate::mail::trackers;

/// Ceilings so one mail can't stall the app or balloon the database.
const MAX_IMAGES: usize = 50;
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const CONCURRENT_FETCHES: usize = 6;
/// Cache rows older than this are dropped — convenience cache, not archive.
const CACHE_MAX_AGE_SECS: i64 = 30 * 24 * 60 * 60;

/// Resolve every loadable URL to a data: URI — cache first, network second.
/// Known trackers are skipped before anything else. Failures are silently
/// dropped: their images simply stay blocked in the rendered document.
pub async fn load_images(pool: &SqlitePool, urls: &[String]) -> HashMap<String, String> {
    prune_expired(pool).await;

    let mut resolved = HashMap::new();
    let mut to_fetch = Vec::new();
    for url in urls
        .iter()
        .filter(|u| is_loadable_url(u) && !trackers::is_tracker(u))
    {
        if resolved.len() + to_fetch.len() >= MAX_IMAGES {
            break;
        }
        match cached(pool, url).await {
            Some(uri) => {
                resolved.insert(url.clone(), uri);
            }
            None => to_fetch.push(url.clone()),
        }
    }
    if to_fetch.is_empty() {
        return resolved;
    }

    let Ok(client) = client() else {
        return resolved;
    };
    let fetched = futures::stream::iter(to_fetch.into_iter().map(|url| {
        let client = client.clone();
        async move {
            let body = fetch_one(&client, &url).await;
            (url, body)
        }
    }))
    .buffer_unordered(CONCURRENT_FETCHES)
    .collect::<Vec<_>>()
    .await;

    for (url, body) in fetched {
        if let Some((content_type, bytes)) = body {
            // why: best effort — a full disk must not turn into "no images".
            store(pool, &url, &content_type, &bytes).await;
            resolved.insert(url, data_uri(&content_type, &bytes));
        }
    }
    resolved
}

/// Whether an image URL may be fetched at all: https, to a host that is a
/// public name — never an IP literal, a single-label name or a reserved
/// suffix (`localhost`, `.local`, `.internal`, …).
///
/// why: the URL is the sender's choice, so without this a "Load images"
/// click (or the "always" policy) aimed a blind GET wherever the mail
/// pointed — this Mac, the router, an intranet host whose certificate the
/// machine trusts. Same guard, and the same limit (a public name resolving
/// to a private address), as the avatar lookup's.
fn is_loadable_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    // why `domain()`: it is None for IP-literal hosts, which is the point.
    parsed.scheme() == "https"
        && parsed
            .domain()
            .is_some_and(super::avatars::is_fetchable_domain)
}

/// One shared client per load: TLS-only (redirects included), bounded
/// redirects, no Referer. reqwest without the cookies feature keeps no
/// cookie jar, and the UA is a bare browser string instead of a client
/// fingerprint ("flit/0.1" would identify the user across mails).
fn client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .referer(false)
        .user_agent("Mozilla/5.0")
        .build()
}

/// GET one image; `None` for anything that isn't a well-behaved image
/// response (bad status, non-image type, over the size cap).
async fn fetch_one(client: &reqwest::Client, url: &str) -> Option<(String, Vec<u8>)> {
    let mut response = client.get(url).send().await.ok()?;
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
    if !content_type.starts_with("image/") {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_IMAGE_BYTES as u64)
    {
        return None;
    }
    // why: chunked read — Content-Length is optional and unverified, so the
    // cap must hold against the actual stream.
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return None;
    }
    Some((content_type, bytes))
}

async fn cached(pool: &SqlitePool, url: &str) -> Option<String> {
    let row: Option<(String, Vec<u8>)> =
        sqlx::query_as("SELECT content_type, data FROM remote_images WHERE url = ?")
            .bind(url)
            .fetch_optional(pool)
            .await
            .ok()?;
    row.map(|(content_type, data)| data_uri(&content_type, &data))
}

async fn store(pool: &SqlitePool, url: &str, content_type: &str, data: &[u8]) {
    let _ = sqlx::query(
        "INSERT INTO remote_images (url, content_type, data, fetched_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (url) DO UPDATE SET
           content_type = excluded.content_type,
           data = excluded.data,
           fetched_at = excluded.fetched_at",
    )
    .bind(url)
    .bind(content_type)
    .bind(data)
    .bind(now_epoch())
    .execute(pool)
    .await;
}

async fn prune_expired(pool: &SqlitePool) {
    let _ = sqlx::query(PRUNE_SQL)
        .bind(now_epoch() - CACHE_MAX_AGE_SECS)
        .execute(pool)
        .await;
}

const PRUNE_SQL: &str = "DELETE FROM remote_images WHERE fetched_at < ?";

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn data_uri(content_type: &str, data: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        content_type,
        base64::engine::general_purpose::STANDARD.encode(data)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    #[tokio::test]
    async fn cache_roundtrips_and_serves_without_network() {
        let pool = test_pool().await;
        // A public-looking host (.example is reserved and refused as a
        // fetch target); "zz" is no real TLD, so nothing could leave anyway.
        store(&pool, "https://img.shop.zz/x.png", "image/png", b"\x89PNG").await;

        // Everything already cached (or a tracker) — no fetch, no network.
        let urls = vec![
            "https://img.shop.zz/x.png".to_string(),
            "https://u1.ct.sendgrid.net/open.png".to_string(),
        ];
        let resolved = load_images(&pool, &urls).await;

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved["https://img.shop.zz/x.png"],
            "data:image/png;base64,iVBORw=="
        );
    }

    #[test]
    fn only_public_https_hosts_are_loadable() {
        // The URL is the sender's choice. Without a host check a "Load
        // images" click aimed a blind GET wherever the mail pointed —
        // this Mac, the router, an intranet host with a trusted certificate.
        for url in [
            "https://localhost/x.png",
            "https://127.0.0.1/x.png",
            "https://[::1]/x.png",
            "https://192.168.1.1/x.png",
            "https://router/x.png",
            "https://printer.local/x.png",
            "https://jenkins.corp.internal/x.png",
            "http://images.example.com/x.png",
            "not a url",
        ] {
            assert!(!is_loadable_url(url), "should refuse {url}");
        }
        assert!(is_loadable_url("https://images.shop.com/logo.png"));
        assert!(is_loadable_url("https://cdn.example.co.uk:8443/a.gif?x=1"));
    }

    #[tokio::test]
    async fn refused_hosts_are_not_served_even_from_the_cache() {
        let pool = test_pool().await;
        // A row fetched before hosts were checked.
        store(&pool, "https://localhost/x.png", "image/png", b"\x89PNG").await;

        let resolved = load_images(&pool, &["https://localhost/x.png".to_string()]).await;

        assert!(resolved.is_empty());
    }

    #[tokio::test]
    async fn prune_finds_expired_rows_through_an_index() {
        // Prune runs before every HTML body is shown. Without an index it
        // read every cached image to reach fetched_at, stored after the
        // blob: 52 ms warm (1.3 s cold) over a real 320 MB cache, once per
        // message of an opened conversation.
        let pool = test_pool().await;
        let plan: Vec<String> = sqlx::query(sqlx::AssertSqlSafe(format!(
            "EXPLAIN QUERY PLAN {PRUNE_SQL}"
        )))
        .bind(0_i64)
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|row| sqlx::Row::get::<String, _>(row, "detail"))
        .collect();

        assert!(
            plan.iter().any(|step| step.contains("USING INDEX")),
            "{plan:#?}"
        );
    }

    #[tokio::test]
    async fn store_overwrites_and_prune_drops_expired_rows() {
        let pool = test_pool().await;
        store(&pool, "https://a.example/x.png", "image/png", b"old").await;
        store(&pool, "https://a.example/x.png", "image/gif", b"new").await;

        let uri = cached(&pool, "https://a.example/x.png").await.unwrap();
        assert!(uri.starts_with("data:image/gif;base64,"));

        // Age the row past the cutoff, then prune.
        sqlx::query("UPDATE remote_images SET fetched_at = ?")
            .bind(now_epoch() - CACHE_MAX_AGE_SECS - 1)
            .execute(&pool)
            .await
            .unwrap();
        prune_expired(&pool).await;
        assert!(cached(&pool, "https://a.example/x.png").await.is_none());
    }
}
