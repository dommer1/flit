use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use sqlx::SqlitePool;

use crate::auth::{self, PasswordCache};
use crate::error::AppError;
use crate::models::OutgoingMessage;

/// Shared app state managed by Tauri; commands receive it via `tauri::State`.
///
/// why: no Mutex around the pool — SqlitePool is internally synchronized and
/// Clone, so concurrent commands can use it directly.
pub struct AppState {
    pub pool: SqlitePool,
    /// Drafts parked for compose windows that are still opening, keyed by
    /// window label. One-shot: taking a draft removes the entry.
    ///
    /// why std Mutex, not tokio: it is only held for a map insert/remove,
    /// never across an await.
    pending_drafts: Mutex<HashMap<String, OutgoingMessage>>,
    /// Messages sitting out their undo window before actually sending.
    /// One-shot by design: whoever takes the entry owns the outcome — the
    /// timer task sends it, or undo hands it back to a compose window.
    pending_sends: Mutex<HashMap<u64, OutgoingMessage>>,
    /// Session cache of account passwords (see auth::PasswordCache).
    pub passwords: PasswordCache,
    /// Accounts with a sync pass currently running — see try_begin_sync.
    syncing: Mutex<HashSet<i64>>,
}

/// Proof of holding an account's sync slot. Dropping it releases the slot —
/// RAII, like MutexGuard — so an early return or `?` in the sync path can
/// never leave an account stuck "already syncing" forever.
pub struct SyncSlot<'a> {
    state: &'a AppState,
    account_id: i64,
}

impl Drop for SyncSlot<'_> {
    fn drop(&mut self) {
        self.state.lock_syncing().remove(&self.account_id);
    }
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            pending_drafts: Mutex::new(HashMap::new()),
            pending_sends: Mutex::new(HashMap::new()),
            passwords: PasswordCache::default(),
            syncing: Mutex::new(HashSet::new()),
        }
    }

    /// Claim the account's sync slot; `None` = a sync of this account is
    /// already running (a manual refresh racing the background poll). The
    /// loser simply skips — the running pass already covers the work, and
    /// two parallel passes would double-fire new-mail notifications.
    pub fn try_begin_sync(&self, account_id: i64) -> Option<SyncSlot<'_>> {
        if self.lock_syncing().insert(account_id) {
            Some(SyncSlot {
                state: self,
                account_id,
            })
        } else {
            None
        }
    }

    /// Account password via the session cache: the keychain — and with it a
    /// possible macOS ACL prompt — is consulted at most once per account
    /// per app run.
    pub async fn password(&self, account_id: i64) -> Result<String, AppError> {
        self.passwords
            .get_or_fetch(account_id, || auth::get_password(account_id))
            .await
    }

    /// Park a draft for the compose window `label` to pick up after it loads.
    pub fn park_draft(&self, label: &str, draft: OutgoingMessage) {
        self.lock_drafts().insert(label.to_string(), draft);
    }

    /// One-shot pickup: `None` for unknown labels (e.g. a reloaded window
    /// whose draft was already taken).
    pub fn take_draft(&self, label: &str) -> Option<OutgoingMessage> {
        self.lock_drafts().remove(label)
    }

    /// Park a message for the duration of its undo window.
    pub fn park_send(&self, id: u64, message: OutgoingMessage) {
        self.lock_sends().insert(id, message);
    }

    /// One-shot pickup: `None` means the other side already took it — for
    /// the timer that's "undone", for undo that's "too late, already sending".
    pub fn take_send(&self, id: u64) -> Option<OutgoingMessage> {
        self.lock_sends().remove(&id)
    }

    // why unwrap_or_else(into_inner): a poisoned lock only means some thread
    // panicked while holding it — the map itself is still coherent, and a
    // compose draft is not worth failing commands over.
    fn lock_drafts(&self) -> MutexGuard<'_, HashMap<String, OutgoingMessage>> {
        self.pending_drafts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_sends(&self) -> MutexGuard<'_, HashMap<u64, OutgoingMessage>> {
        self.pending_sends
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_syncing(&self) -> MutexGuard<'_, HashSet<i64>> {
        self.syncing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_pool;

    fn draft(subject: &str) -> OutgoingMessage {
        OutgoingMessage {
            account_id: 1,
            to: "alice@example.com".to_string(),
            cc: String::new(),
            bcc: String::new(),
            subject: subject.to_string(),
            body: "hi".to_string(),
            body_html: None,
            attachments: Vec::new(),
            draft_message_id: None,
            in_reply_to: None,
            references: None,
        }
    }

    #[tokio::test]
    async fn taking_a_draft_is_one_shot() {
        let state = AppState::new(test_pool().await);
        state.park_draft("compose-0", draft("Hello"));

        let taken = state.take_draft("compose-0");

        assert_eq!(taken.map(|d| d.subject), Some("Hello".to_string()));
        assert!(state.take_draft("compose-0").is_none());
    }

    #[tokio::test]
    async fn taking_a_pending_send_is_one_shot() {
        let state = AppState::new(test_pool().await);
        state.park_send(7, draft("Hello"));

        let taken = state.take_send(7);

        assert_eq!(taken.map(|d| d.subject), Some("Hello".to_string()));
        // the second taker loses — that is the whole undo-vs-timer contract
        assert!(state.take_send(7).is_none());
        assert!(state.take_send(8).is_none());
    }

    #[tokio::test]
    async fn sync_slot_blocks_a_second_claim_until_dropped() {
        let state = AppState::new(test_pool().await);

        let slot = state.try_begin_sync(1);
        assert!(slot.is_some());
        // The same account cannot be claimed twice...
        assert!(state.try_begin_sync(1).is_none());
        // ...but another account syncs independently.
        assert!(state.try_begin_sync(2).is_some());

        drop(slot);
        assert!(state.try_begin_sync(1).is_some());
    }

    #[tokio::test]
    async fn drafts_are_keyed_by_window_label() {
        let state = AppState::new(test_pool().await);
        state.park_draft("compose-0", draft("First"));
        state.park_draft("compose-1", draft("Second"));

        assert!(state.take_draft("compose-9").is_none());
        assert_eq!(
            state.take_draft("compose-1").map(|d| d.subject),
            Some("Second".to_string())
        );
        assert_eq!(
            state.take_draft("compose-0").map(|d| d.subject),
            Some("First".to_string())
        );
    }
}
