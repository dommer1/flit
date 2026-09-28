use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::error::AppError;

pub mod oauth;
pub mod redirect;

/// What an account proves its identity with, handed down to the IMAP and
/// SMTP layers.
///
/// SECURITY: deliberately not `Debug` — the secret must never end up in a
/// log line or an error message.
pub enum Credential {
    /// An account password, sent with IMAP LOGIN / SMTP AUTH.
    Password(String),
    /// A short-lived OAuth access token (Gmail), sent with SASL XOAUTH2.
    AccessToken(String),
}

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

/// Refresh an access token once less than this is left of its life.
/// why so little: the token only has to outlive the IMAP/SMTP login it is
/// fetched for — a session, once authenticated, stays so after it expires.
const EXPIRY_MARGIN: Duration = Duration::from_secs(120);

/// Session-only cache of OAuth access tokens and when each expires.
///
/// SECURITY: like PasswordCache, memory only — an access token is never
/// written to disk, a log or the database.
#[derive(Default)]
pub struct AccessTokens {
    tokens: Mutex<HashMap<i64, (String, Instant)>>,
    refreshing: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
}

impl AccessTokens {
    /// A token with life left in it, or run `refresh` (a call to the token
    /// endpoint, returning the token and its lifetime) and remember it.
    pub async fn get_or_refresh<F, Fut>(
        &self,
        account_id: i64,
        refresh: F,
    ) -> Result<String, AppError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(String, Duration), AppError>>,
    {
        if let Some(token) = self.fresh(account_id) {
            return Ok(token);
        }
        // why a per-account lock: a sync pass, the IDLE listener and a body
        // fetch can all need a token at the same moment, and one refresh
        // should serve them all — a provider that rotates refresh tokens
        // must not see the same one spent twice. A tokio Mutex, because it
        // is held across the refresh's await.
        let lock = self
            .lock_refreshing()
            .entry(account_id)
            .or_default()
            .clone();
        let _guard = lock.lock().await;
        if let Some(token) = self.fresh(account_id) {
            return Ok(token);
        }
        let (token, lifetime) = refresh().await?;
        self.insert(account_id, token.clone(), lifetime);
        Ok(token)
    }

    /// Remember a token obtained elsewhere — a sign-in that just finished.
    pub fn insert(&self, account_id: i64, token: String, lifetime: Duration) {
        self.lock_tokens()
            .insert(account_id, (token, Instant::now() + lifetime));
    }

    /// Drop an account's token (account deleted, or signed in anew).
    pub fn forget(&self, account_id: i64) {
        self.lock_tokens().remove(&account_id);
    }

    fn fresh(&self, account_id: i64) -> Option<String> {
        let tokens = self.lock_tokens();
        let (token, expires_at) = tokens.get(&account_id)?;
        let left = expires_at.saturating_duration_since(Instant::now());
        (left > EXPIRY_MARGIN).then(|| token.clone())
    }

    // why unwrap_or_else(into_inner): as in PasswordCache — a poisoned lock
    // only means a panic elsewhere; the maps themselves stay coherent.
    fn lock_tokens(&self) -> MutexGuard<'_, HashMap<i64, (String, Instant)>> {
        self.tokens
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_refreshing(&self) -> MutexGuard<'_, HashMap<i64, Arc<tokio::sync::Mutex<()>>>> {
        self.refreshing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Install keyring's platform store, once, before anything reads a secret.
///
/// why: keyring 4's `Entry::new` installs the store lazily on its first call
/// and flips its "done" flag *before* the store exists, so a second caller in
/// that window gets `NoDefaultStore`. Flit reads several accounts' passwords
/// at once on launch (sync passes, IDLE listeners), and a listener or pass
/// failed with "No default store has been set" — reproduced on two launches
/// in a row. Making the first call here, on the setup thread before any task
/// is spawned, closes that window. Creating an entry touches no keychain item.
pub fn init_keychain() -> Result<(), AppError> {
    keyring::Entry::new(SERVICE, "flit-startup")?;
    Ok(())
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

    #[test]
    fn the_keychain_store_is_ready_once_initialised() {
        init_keychain().expect("the platform store installs");
        assert!(keyring::Entry::new(SERVICE, "flit-test-probe").is_ok());
    }
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

    const HOUR: Duration = Duration::from_secs(3600);

    #[tokio::test]
    async fn an_access_token_is_reused_until_it_nears_expiry() {
        let tokens = AccessTokens::default();
        let refreshes = AtomicUsize::new(0);
        let refresh = || async {
            let n = refreshes.fetch_add(1, Ordering::SeqCst);
            Ok((format!("token-{n}"), HOUR))
        };

        assert_eq!(tokens.get_or_refresh(1, refresh).await.unwrap(), "token-0");
        assert_eq!(tokens.get_or_refresh(1, refresh).await.unwrap(), "token-0");
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_token_about_to_expire_is_refreshed_first() {
        let tokens = AccessTokens::default();
        let short = || async { Ok(("old".to_string(), Duration::from_secs(30))) };
        tokens.get_or_refresh(1, short).await.unwrap();

        let fresh = || async { Ok(("new".to_string(), HOUR)) };
        assert_eq!(tokens.get_or_refresh(1, fresh).await.unwrap(), "new");
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_refresh() {
        let tokens = std::sync::Arc::new(AccessTokens::default());
        let refreshes = std::sync::Arc::new(AtomicUsize::new(0));
        let call = || {
            let (tokens, refreshes) = (tokens.clone(), refreshes.clone());
            tokio::spawn(async move {
                tokens
                    .get_or_refresh(1, || async move {
                        refreshes.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok(("t".to_string(), HOUR))
                    })
                    .await
            })
        };

        let (a, b) = (call(), call());
        assert_eq!(a.await.unwrap().unwrap(), "t");
        assert_eq!(b.await.unwrap().unwrap(), "t");
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_token_from_a_sign_in_is_used_without_a_refresh() {
        let tokens = AccessTokens::default();
        tokens.insert(1, "signed-in".to_string(), HOUR);

        let untouched = || async { panic!("a fresh token must not refresh") };
        assert_eq!(
            tokens.get_or_refresh(1, untouched).await.unwrap(),
            "signed-in"
        );
    }

    #[tokio::test]
    async fn a_failed_refresh_caches_nothing_and_forget_drops_a_token() {
        let tokens = AccessTokens::default();
        let failing = || async { Err(AppError::SignInExpired) };
        assert!(tokens.get_or_refresh(1, failing).await.is_err());

        let first = || async { Ok(("a".to_string(), HOUR)) };
        assert_eq!(tokens.get_or_refresh(1, first).await.unwrap(), "a");
        tokens.forget(1);
        let second = || async { Ok(("b".to_string(), HOUR)) };
        assert_eq!(tokens.get_or_refresh(1, second).await.unwrap(), "b");
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
