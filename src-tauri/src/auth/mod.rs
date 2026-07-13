use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard};

use crate::error::AppError;

// why: the Keychain "service" is the bundle identifier, so Flit's entries
// group predictably under one name in Keychain Access.app.
const SERVICE: &str = "sk.vocalio.flit";

/// Session-only, in-memory cache of account passwords.
///
/// SECURITY: passwords stay in this process's memory for the app's lifetime
/// and never touch disk (hard rule). The point is UX — every keychain read
/// can raise a macOS ACL prompt (always, for unsigned dev builds, whose
/// signature changes each rebuild), so the keychain is asked at most once
/// per account per run instead of once per operation.
#[derive(Default)]
pub struct PasswordCache(Mutex<HashMap<i64, String>>);

impl PasswordCache {
    /// The cached password, or run `fetch` (a keychain read) and remember it.
    pub async fn get_or_fetch<F, Fut>(&self, account_id: i64, fetch: F) -> Result<String, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, AppError>>,
    {
        if let Some(hit) = self.lock().get(&account_id).cloned() {
            return Ok(hit);
        }
        // why: the lock is released before awaiting — two concurrent first
        // calls may both fetch, which is harmless (same value, one extra
        // keychain read); holding a std Mutex across an await is not.
        let password = fetch().await?;
        self.lock().insert(account_id, password.clone());
        Ok(password)
    }

    /// Seed the cache (e.g. right after add_account stored the secret).
    pub fn insert(&self, account_id: i64, password: String) {
        self.lock().insert(account_id, password);
    }

    /// Drop an entry (account deleted, or its credential went stale).
    pub fn remove(&self, account_id: i64) {
        self.lock().remove(&account_id);
    }

    // why unwrap_or_else(into_inner): a poisoned lock only means some thread
    // panicked while holding it — the map itself is still coherent.
    fn lock(&self) -> MutexGuard<'_, HashMap<i64, String>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Keychain user name for an account row id.
///
/// why: keyed by id, not email — emails are user-editable and two accounts
/// may share one address; the row id is stable and unique.
fn keychain_user(account_id: i64) -> String {
    format!("account-{account_id}")
}

/// True when the error just means "no such credential stored".
fn is_missing_credential(err: &keyring::Error) -> bool {
    matches!(err, keyring::Error::NoEntry)
}

// why: the keyring API is synchronous — spawn_blocking moves each call off
// the async executor threads, so a slow Keychain access (or a user prompt)
// can't stall unrelated tasks. The outer `?` maps a panicked/cancelled task
// (JoinError); the inner Result is the keychain outcome.

pub async fn set_password(account_id: i64, password: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let entry = keyring::Entry::new(SERVICE, &keychain_user(account_id))?;
        entry.set_password(&password)?;
        Ok(())
    })
    .await?
}

pub async fn get_password(account_id: i64) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let entry = keyring::Entry::new(SERVICE, &keychain_user(account_id))?;
        Ok(entry.get_password()?)
    })
    .await?
}

/// Delete the stored password. Idempotent — a missing credential is already
/// the desired end state.
pub async fn delete_password(account_id: i64) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let entry = keyring::Entry::new(SERVICE, &keychain_user(account_id))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(err) if is_missing_credential(&err) => Ok(()),
            Err(err) => Err(err.into()),
        }
    })
    .await?
}

// why: only the pure parts are unit-tested. The real Keychain can't run in CI
// (it would prompt and mutate the developer's keychain), and keyring's v1
// facade pins the platform store on first use, so its mock store can't be
// injected here. The thin wrappers above get exercised manually via the app.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn cache_fetches_once_per_account() {
        let cache = PasswordCache::default();
        let fetches = AtomicUsize::new(0);
        let fetch = || async {
            fetches.fetch_add(1, Ordering::SeqCst);
            Ok("tajné".to_string())
        };

        assert_eq!(cache.get_or_fetch(1, fetch).await.unwrap(), "tajné");
        assert_eq!(cache.get_or_fetch(1, fetch).await.unwrap(), "tajné");
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cache_entries_are_per_account_and_removable() {
        let cache = PasswordCache::default();
        cache.insert(1, "prvé".to_string());
        cache.insert(2, "druhé".to_string());

        let untouched = || async { panic!("cached entry must not fetch") };
        assert_eq!(cache.get_or_fetch(1, untouched).await.unwrap(), "prvé");
        assert_eq!(cache.get_or_fetch(2, untouched).await.unwrap(), "druhé");

        cache.remove(1);
        let refetched = || async { Ok("nové".to_string()) };
        assert_eq!(cache.get_or_fetch(1, refetched).await.unwrap(), "nové");
    }

    #[test]
    fn keychain_user_embeds_account_id() {
        assert_eq!(keychain_user(42), "account-42");
    }

    #[test]
    fn no_entry_counts_as_missing_credential() {
        assert!(is_missing_credential(&keyring::Error::NoEntry));
    }

    #[test]
    fn other_errors_are_not_missing_credential() {
        let err = keyring::Error::BadStoreFormat("broken".to_string());
        assert!(!is_missing_credential(&err));
    }
}
