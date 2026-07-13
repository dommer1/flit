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
    for url in urls.iter().filter(|u| !trackers::is_tracker(u)) {
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
    let _ = sqlx::query("DELETE FROM remote_images WHERE fetched_at < ?")
        .bind(now_epoch() - CACHE_MAX_AGE_SECS)
        .execute(pool)
        .await;
}

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
        store(&pool, "https://a.example/x.png", "image/png", b"\x89PNG").await;

        // Everything already cached (or a tracker) — no fetch, no network.
        let urls = vec![
            "https://a.example/x.png".to_string(),
            "https://u1.ct.sendgrid.net/open.png".to_string(),
        ];
        let resolved = load_images(&pool, &urls).await;

        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved["https://a.example/x.png"],
            "data:image/png;base64,iVBORw=="
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
