//! Fetching one pinned model file to disk.
//!
//! SECURITY/PRIVACY: this is the only place the app contacts a host that is
//! neither the user's mail server nor a sender's image host, and it runs
//! only when the user clicked Download on a specific model. TLS-only, no
//! cookies, no Referer, and every byte is hashed on the way down — a file
//! whose SHA-256 differs from the catalog pin is deleted, never used.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::AppError;
use crate::llm::catalog::ModelSpec;
use crate::llm::store;

/// Time to first byte and between bytes — a stalled CDN must not hang the
/// download forever. Not a total timeout: gigabytes take as long as they take.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Same posture as mail::remote's client: TLS-only, bounded redirects (the
/// CDN redirects once), no Referer, no cookie jar (feature not compiled).
pub fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .https_only(true)
        .read_timeout(READ_TIMEOUT)
        .connect_timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(5))
        .referer(false)
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Http(e.to_string()))
}

/// How a finished download ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The file is in place, size and hash verified.
    Done,
    /// The cancel flag was raised; the partial file is gone.
    Cancelled,
}

/// A raisable stop signal shared between the download task and the cancel
/// command. Plain atomic, not a channel: the loop only ever polls it.
pub type CancelFlag = Arc<AtomicBool>;

/// What Settings sees of a download in flight (or one that failed and has
/// not been retried or dismissed yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub received: u64,
    pub total: u64,
    /// Set once the task gave up; the entry then stays until the user
    /// retries or removes the model, so the failure is visible.
    pub error: Option<String>,
}

struct Entry {
    received: u64,
    total: u64,
    cancel: CancelFlag,
    error: Option<String>,
}

/// The downloads currently running or failed, keyed by model id. Lives in
/// AppState so the commands and the download tasks share one view.
#[derive(Default)]
pub struct Downloads {
    inner: Mutex<HashMap<String, Entry>>,
}

impl Downloads {
    /// Claim the model's download slot; `None` when a download of it is
    /// already running. A failed entry is replaced — that is a retry.
    pub fn begin(&self, id: &str, total: u64) -> Option<CancelFlag> {
        let mut map = self.lock();
        if map.get(id).is_some_and(|e| e.error.is_none()) {
            return None;
        }
        let cancel = CancelFlag::default();
        map.insert(
            id.to_string(),
            Entry {
                received: 0,
                total,
                cancel: cancel.clone(),
                error: None,
            },
        );
        Some(cancel)
    }

    pub fn progress(&self, id: &str, received: u64) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.received = received;
        }
    }

    pub fn fail(&self, id: &str, error: String) {
        if let Some(entry) = self.lock().get_mut(id) {
            entry.error = Some(error);
        }
    }

    /// Forget the entry: the download finished, was cancelled, or its
    /// failure was dismissed.
    pub fn finish(&self, id: &str) {
        self.lock().remove(id);
    }

    /// Raise the running download's stop flag; false when nothing is
    /// running for this id.
    pub fn cancel(&self, id: &str) -> bool {
        match self.lock().get(id) {
            Some(entry) if entry.error.is_none() => {
                entry.cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.lock().get(id).is_some_and(|e| e.error.is_none())
    }

    pub fn snapshot(&self) -> HashMap<String, Snapshot> {
        self.lock()
            .iter()
            .map(|(id, e)| {
                (
                    id.clone(),
                    Snapshot {
                        received: e.received,
                        total: e.total,
                        error: e.error.clone(),
                    },
                )
            })
            .collect()
    }

    // why unwrap_or_else(into_inner): a poisoned lock only means a task
    // panicked mid-update; the map is still coherent.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Stream `spec.url` into `<dir>/<file>.part`, verify, then rename into
/// place. `on_progress` gets the running byte count after every chunk.
///
/// why a .part file: the final name means "complete" everywhere else
/// (store::is_ready), so a crash mid-download can never leave a half file
/// under the name the engine would load.
pub async fn fetch(
    client: &reqwest::Client,
    spec: &ModelSpec,
    dir: &Path,
    cancel: &CancelFlag,
    mut on_progress: impl FnMut(u64),
) -> Result<Outcome, AppError> {
    let final_path = store::model_path(dir, spec);
    let part_path = part_path(&final_path);

    let result = stream_to(client, spec, &part_path, cancel, &mut on_progress).await;
    match result {
        Ok(Outcome::Done) => {
            tokio::fs::rename(&part_path, &final_path).await?;
            Ok(Outcome::Done)
        }
        other => {
            // Cancelled or failed: never leave a partial file behind.
            let _ = tokio::fs::remove_file(&part_path).await;
            other
        }
    }
}

fn part_path(final_path: &Path) -> PathBuf {
    let mut name = final_path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    final_path.with_file_name(name)
}

async fn stream_to(
    client: &reqwest::Client,
    spec: &ModelSpec,
    part_path: &Path,
    cancel: &CancelFlag,
    on_progress: &mut impl FnMut(u64),
) -> Result<Outcome, AppError> {
    let mut response = client
        .get(spec.url)
        .send()
        .await
        .map_err(|e| AppError::Http(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Http(format!(
            "server answered {}",
            response.status()
        )));
    }

    let mut file = tokio::fs::File::create(part_path).await?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled);
        }
        let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| AppError::Http(e.to_string()))?
        else {
            break;
        };
        received += chunk.len() as u64;
        // why check as we go: a wrong Content-Length or a runaway response
        // must stop at the pinned size, not fill the disk.
        if received > spec.size {
            return Err(AppError::Http("file is larger than expected".to_string()));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        on_progress(received);
    }
    file.flush().await?;
    drop(file);

    if received != spec.size {
        return Err(AppError::Http(format!(
            "file is {received} bytes, expected {}",
            spec.size
        )));
    }
    let digest = format!("{:x}", hasher.finalize());
    if digest != spec.sha256 {
        return Err(AppError::Http("file failed verification".to_string()));
    }
    Ok(Outcome::Done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A one-shot HTTP server on localhost that answers any request with
    /// `body` (and the given Content-Length). Returns the URL to hit.
    async fn serve(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        format!("http://{addr}/model.gguf")
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("flit-llm-download-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A spec pinned to `body`, served from `url`. Leaked so the test can hand
    /// out the `'static` strs the catalog type carries.
    fn spec_for(url: String, body: &[u8], sha256: Option<String>) -> ModelSpec {
        let digest = sha256.unwrap_or_else(|| format!("{:x}", Sha256::digest(body)));
        ModelSpec {
            id: "test",
            name: "Test",
            description: "",
            file_name: "model.gguf",
            size: body.len() as u64,
            sha256: Box::leak(digest.into_boxed_str()),
            url: Box::leak(url.into_boxed_str()),
            recommended: false,
        }
    }

    /// Plain http for the local test server — the production client is
    /// TLS-only and cannot talk to it.
    fn test_client() -> reqwest::Client {
        reqwest::Client::builder().build().unwrap()
    }

    #[test]
    fn a_running_download_blocks_a_second_claim_until_finished() {
        let downloads = Downloads::default();

        assert!(downloads.begin("m", 10).is_some());
        assert!(downloads.begin("m", 10).is_none());
        assert!(downloads.is_running("m"));
        assert!(downloads.begin("other", 10).is_some());

        downloads.finish("m");
        assert!(!downloads.is_running("m"));
        assert!(downloads.begin("m", 10).is_some());
    }

    #[test]
    fn a_failed_download_stays_visible_until_retried() {
        let downloads = Downloads::default();
        downloads.begin("m", 10);
        downloads.progress("m", 4);
        downloads.fail("m", "boom".to_string());

        let snap = downloads.snapshot().remove("m").unwrap();
        assert_eq!(snap.received, 4);
        assert_eq!(snap.total, 10);
        assert_eq!(snap.error.as_deref(), Some("boom"));
        assert!(!downloads.is_running("m"));
        // Cancel has nothing to stop...
        assert!(!downloads.cancel("m"));
        // ...but a retry replaces the failed entry.
        assert!(downloads.begin("m", 10).is_some());
        assert_eq!(downloads.snapshot()["m"].error, None);
    }

    #[test]
    fn cancel_raises_the_running_downloads_flag() {
        let downloads = Downloads::default();
        let flag = downloads.begin("m", 10).unwrap();

        assert!(!flag.load(Ordering::Relaxed));
        assert!(downloads.cancel("m"));
        assert!(flag.load(Ordering::Relaxed));
        assert!(!downloads.cancel("unknown"));
    }

    #[tokio::test]
    async fn a_verified_download_lands_under_the_final_name() {
        let body = b"hello model".to_vec();
        let url = serve(body.clone()).await;
        let spec = spec_for(url, &body, None);
        let dir = scratch_dir("ok");
        let mut seen = Vec::new();

        let outcome = fetch(&test_client(), &spec, &dir, &CancelFlag::default(), |n| {
            seen.push(n)
        })
        .await
        .unwrap();

        assert_eq!(outcome, Outcome::Done);
        assert_eq!(std::fs::read(store::model_path(&dir, &spec)).unwrap(), body);
        assert!(store::is_ready(&dir, &spec));
        assert_eq!(seen.last(), Some(&(body.len() as u64)));
        assert!(!part_path(&store::model_path(&dir, &spec)).exists());
    }

    #[tokio::test]
    async fn a_hash_mismatch_is_rejected_and_leaves_no_file() {
        let body = b"tampered".to_vec();
        let url = serve(body.clone()).await;
        let spec = spec_for(url, &body, Some("0".repeat(64)));
        let dir = scratch_dir("hash");

        let err = fetch(&test_client(), &spec, &dir, &CancelFlag::default(), |_| {})
            .await
            .unwrap_err();

        assert!(err.to_string().contains("verification"), "{err}");
        assert!(!store::model_path(&dir, &spec).exists());
        assert!(!part_path(&store::model_path(&dir, &spec)).exists());
    }

    #[tokio::test]
    async fn a_short_or_long_body_is_rejected() {
        let body = b"0123456789".to_vec();
        let url = serve(body.clone()).await;
        let mut spec = spec_for(url, &body, None);
        spec.size = 4; // the pin says 4 bytes; the server sends 10
        let dir = scratch_dir("size");

        let err = fetch(&test_client(), &spec, &dir, &CancelFlag::default(), |_| {})
            .await
            .unwrap_err();

        assert!(err.to_string().contains("larger"), "{err}");
        assert!(!store::model_path(&dir, &spec).exists());
    }

    #[tokio::test]
    async fn cancelling_removes_the_partial_file() {
        let body = vec![7u8; 64 * 1024];
        let url = serve(body.clone()).await;
        let spec = spec_for(url, &body, None);
        let dir = scratch_dir("cancel");
        let cancel = CancelFlag::default();
        // Raise the flag from inside the first progress callback, so the
        // loop sees it on its next turn.
        let flag = cancel.clone();
        let outcome = fetch(&test_client(), &spec, &dir, &cancel, move |_| {
            flag.store(true, Ordering::Relaxed)
        })
        .await
        .unwrap();

        assert_eq!(outcome, Outcome::Cancelled);
        assert!(!store::model_path(&dir, &spec).exists());
        assert!(!part_path(&store::model_path(&dir, &spec)).exists());
    }
}
