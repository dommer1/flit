use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use sqlx::SqlitePool;

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
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            pending_drafts: Mutex::new(HashMap::new()),
        }
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

    // why unwrap_or_else(into_inner): a poisoned lock only means some thread
    // panicked while holding it — the map itself is still coherent, and a
    // compose draft is not worth failing commands over.
    fn lock_drafts(&self) -> MutexGuard<'_, HashMap<String, OutgoingMessage>> {
        self.pending_drafts
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
            subject: subject.to_string(),
            body: "hi".to_string(),
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
