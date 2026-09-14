use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use sqlx::SqlitePool;

use crate::auth::{self, PasswordCache};
use crate::error::AppError;
use crate::llm;
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
    /// Accounts with a header backfill currently running. Separate from
    /// `syncing`: a backfill runs for minutes and must never block the
    /// regular sync (or vice versa).
    backfilling: Mutex<HashSet<i64>>,
    /// Per-account queue for background draft pushes (append / delete /
    /// folder sync). tokio's Mutex hands the lock out in FIFO order, so
    /// pushes run in save order — a later save can never reach the server
    /// before the version it is meant to replace.
    draft_pushes: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
    /// Model downloads in flight or failed (see llm::download::Downloads).
    pub llm_downloads: llm::download::Downloads,
    /// The summaries worker (see llm::engine). Idle until the first request.
    pub llm_engine: llm::engine::Engine,
    /// Summaries being written, so a cancel can reach them.
    pub llm_summaries: llm::Summaries,
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

/// Proof of holding an account's backfill slot — same RAII contract as
/// `SyncSlot`.
pub struct BackfillSlot<'a> {
    state: &'a AppState,
    account_id: i64,
}

impl Drop for BackfillSlot<'_> {
    fn drop(&mut self) {
        self.state.lock_backfilling().remove(&self.account_id);
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
            backfilling: Mutex::new(HashSet::new()),
            draft_pushes: Mutex::new(HashMap::new()),
            llm_downloads: llm::download::Downloads::default(),
            llm_engine: llm::engine::Engine::default(),
            llm_summaries: llm::Summaries::default(),
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

    /// Claim the account's backfill slot; `None` = a backfill of this
    /// account is already running (every sync pass tries to start one — the
    /// running loop already covers the work).
    pub fn try_begin_backfill(&self, account_id: i64) -> Option<BackfillSlot<'_>> {
        if self.lock_backfilling().insert(account_id) {
            Some(BackfillSlot {
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

    /// The account's draft-push queue lock — hold it across the whole
    /// server conversation of one push or discard.
    pub fn draft_push_lock(&self, account_id: i64) -> Arc<tokio::sync::Mutex<()>> {
        self.draft_pushes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(account_id)
            .or_default()
            .clone()
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

    fn lock_backfilling(&self) -> MutexGuard<'_, HashSet<i64>> {
        self.backfilling
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
            alias_id: None,
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
            quote: None,
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
    async fn backfill_slot_is_independent_of_the_sync_slot() {
        let state = AppState::new(test_pool().await);

        let backfill = state.try_begin_backfill(1);
        assert!(backfill.is_some());
        // A second backfill of the same account must not start...
        assert!(state.try_begin_backfill(1).is_none());
        // ...but a regular sync of the same account still can.
        assert!(state.try_begin_sync(1).is_some());
        assert!(state.try_begin_backfill(2).is_some());

        drop(backfill);
        assert!(state.try_begin_backfill(1).is_some());
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
