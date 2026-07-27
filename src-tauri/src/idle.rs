//! IMAP IDLE listeners: while push is on, one open connection per account
//! that the server can announce new inbox mail on, instead of waiting for
//! the next poll.
//!
//! The poller is untouched and still runs at the configured interval — with
//! push on, that interval stops being the inbox's cadence and becomes
//! everything else's: the other folders, and flag changes made elsewhere.

use std::collections::HashMap;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::time::MissedTickBehavior;

use crate::error::AppError;
use crate::mail::imap::{self, Idled};
use crate::state::AppState;
use crate::{commands, storage};

/// How often the supervisor compares running listeners against the settings.
/// A toggle in the settings window therefore takes effect within this long —
/// cheap, and far simpler than plumbing an event through.
const RECONCILE_TICK: Duration = Duration::from_secs(30);

/// How long one IDLE waits before re-issuing it. RFC 2177 tells clients to
/// re-IDLE at least every 29 minutes; left longer, NAT and server timeouts
/// drop the connection without telling anyone.
const IDLE_LIMIT: Duration = Duration::from_secs(29 * 60);

/// First retry delay after a failed connection; each further consecutive
/// failure doubles it, up to BACKOFF_CAP.
const BACKOFF_BASE: Duration = Duration::from_secs(5);
const BACKOFF_CAP: Duration = Duration::from_secs(5 * 60);

/// How long to wait before looking again at a server that has no IDLE. Much
/// longer than any backoff: there is nothing to retry, only the small chance
/// the server was upgraded under us.
const UNSUPPORTED_RETRY: Duration = Duration::from_secs(60 * 60);

/// How long a connection must stand before a later break counts as a fresh
/// problem rather than a continuing one.
const STABLE_AFTER: Duration = Duration::from_secs(60);

/// Why one listener's connection ended. A listener never ends any other way
/// — a healthy one loops on IDLE forever.
enum Ended {
    /// The server does not offer IDLE, so there is nothing to listen on.
    Unsupported,
    /// The connection, or a command on it, failed.
    Failed(AppError),
}

/// Delay before reconnecting after `failures` consecutive failures.
fn backoff(failures: u32) -> Duration {
    // why saturating/min: a listener that has been failing for hours must
    // settle at the cap, not overflow the shift into a panic or a zero.
    let doublings = failures.saturating_sub(1).min(16);
    let secs = BACKOFF_BASE
        .as_secs()
        .saturating_mul(1u64 << doublings)
        .min(BACKOFF_CAP.as_secs());
    Duration::from_secs(secs)
}

/// Which listeners to start and which to stop, from the accounts that should
/// have one and the accounts that already do.
fn reconcile(desired: &[i64], running: &[i64]) -> (Vec<i64>, Vec<i64>) {
    let start = desired
        .iter()
        .copied()
        .filter(|id| !running.contains(id))
        .collect();
    let stop = running
        .iter()
        .copied()
        .filter(|id| !desired.contains(id))
        .collect();
    (start, stop)
}

/// Spawned once at startup; runs for the life of the app.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut running: HashMap<i64, tauri::async_runtime::JoinHandle<()>> = HashMap::new();
        let mut interval = tokio::time::interval(RECONCILE_TICK);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            interval.tick().await;

            // A listener only ends by panicking — drop it so it restarts.
            running.retain(|_, handle| !handle.inner().is_finished());

            let desired = match wanted_accounts(&app).await {
                Ok(ids) => ids,
                Err(err) => {
                    eprintln!("idle supervisor could not read settings: {err}");
                    continue;
                }
            };
            let live: Vec<i64> = running.keys().copied().collect();
            let (start, stop) = reconcile(&desired, &live);

            for id in stop {
                // Aborting drops the task's session, which closes the socket.
                if let Some(handle) = running.remove(&id) {
                    handle.abort();
                }
            }
            for id in start {
                let app = app.clone();
                running.insert(id, tauri::async_runtime::spawn(listen(app, id)));
            }
        }
    });
}

/// The accounts that should have a listener: all of them while push is on,
/// none otherwise.
async fn wanted_accounts(app: &AppHandle) -> Result<Vec<i64>, AppError> {
    let state = app.state::<AppState>();
    if !storage::settings::notification_settings(&state.pool)
        .await?
        .push_enabled
    {
        return Ok(Vec::new());
    }
    Ok(storage::accounts::list(&state.pool)
        .await?
        .iter()
        .map(|account| account.id)
        .collect())
}

/// One account's listener: keep a connection watching the inbox, reconnecting
/// with backoff whenever it breaks. Only ever ends by being aborted.
async fn listen(app: AppHandle, account_id: i64) {
    let mut failures: u32 = 0;
    loop {
        let started = std::time::Instant::now();
        let delay = match watch(&app, account_id).await {
            Ended::Unsupported => {
                eprintln!("account {account_id} has no IMAP IDLE; polling covers it");
                UNSUPPORTED_RETRY
            }
            Ended::Failed(err) => {
                // why the reset: a connection that stood for a while and then
                // broke is a fresh problem, not a continuing one. Without
                // this, a listener that has been up for days inherits an old
                // failure streak and waits the full cap to come back.
                if started.elapsed() >= STABLE_AFTER {
                    failures = 0;
                }
                failures += 1;
                eprintln!("idle listener for account {account_id} dropped: {err}");
                backoff(failures)
            }
        };
        tokio::time::sleep(delay).await;
    }
}

/// One connection's life: connect, watch the inbox, sync whenever the server
/// announces something. Returns only when the connection is finished with.
async fn watch(app: &AppHandle, account_id: i64) -> Ended {
    match watch_inner(app, account_id).await {
        Ok(ended) => ended,
        Err(err) => Ended::Failed(err),
    }
}

async fn watch_inner(app: &AppHandle, account_id: i64) -> Result<Ended, AppError> {
    let (account, password, inbox) = {
        let state = app.state::<AppState>();
        let account = storage::accounts::get(&state.pool, account_id).await?;
        let password = state.password(account_id).await?;
        // The inbox is whatever discovery labelled with the inbox role;
        // "INBOX" is the RFC-guaranteed fallback before a first sync.
        let inbox = storage::mailboxes::name_for_role(&state.pool, account_id, "inbox")
            .await?
            .unwrap_or_else(|| "INBOX".to_string());
        (account, password, inbox)
    };

    let mut session = imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &password,
    )
    .await?;
    if !imap::supports_idle(&mut session).await {
        return Ok(Ended::Unsupported);
    }
    session
        .select(&inbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {inbox}: {e}")))?;

    loop {
        let (returned, outcome) = imap::idle_once(session, IDLE_LIMIT).await?;
        session = returned;
        if outcome == Idled::Quiet {
            continue;
        }
        // why a full pass and not a targeted fetch of the new uid: run_sync
        // already owns the sync slot, deduplication and notifications, so
        // routing through it keeps push and polling on one code path.
        if let Err(err) = commands::run_sync(app, account_id).await {
            eprintln!("push-triggered sync failed for account {account_id}: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_the_base_delay() {
        assert_eq!(backoff(1), Duration::from_secs(5));
        assert_eq!(backoff(2), Duration::from_secs(10));
        assert_eq!(backoff(3), Duration::from_secs(20));
        assert_eq!(backoff(4), Duration::from_secs(40));
    }

    #[test]
    fn backoff_settles_at_the_cap() {
        assert_eq!(backoff(20), BACKOFF_CAP);
        // A listener failing for a very long time must not overflow into a
        // panic or wrap around to no delay at all.
        assert_eq!(backoff(u32::MAX), BACKOFF_CAP);
    }

    #[test]
    fn backoff_never_returns_zero() {
        assert_eq!(backoff(0), BACKOFF_BASE);
    }

    #[test]
    fn reconcile_starts_what_is_missing_and_stops_what_is_extra() {
        let (start, stop) = reconcile(&[1, 2, 3], &[2, 9]);

        assert_eq!(start, vec![1, 3]);
        assert_eq!(stop, vec![9]);
    }

    #[test]
    fn reconcile_is_quiet_when_everything_already_matches() {
        let (start, stop) = reconcile(&[1, 2], &[2, 1]);

        assert!(start.is_empty());
        assert!(stop.is_empty());
    }

    #[test]
    fn reconcile_stops_everything_when_push_goes_off() {
        let (start, stop) = reconcile(&[], &[1, 2]);

        assert!(start.is_empty());
        assert_eq!(stop, vec![1, 2]);
    }
}
