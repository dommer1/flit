//! Background new-mail poll: re-reads the configured interval every minute
//! and, when it elapses, syncs all accounts in parallel. This is what makes
//! notifications arrive while the app sits idle — without it, mail would
//! only ever sync when the user clicks something.

use tauri::{AppHandle, Manager};
use tokio::time::MissedTickBehavior;

use crate::state::AppState;
use crate::wake::{self, WakeDetector};
use crate::{commands, storage};

/// One-minute ticks, like the send-later scheduler: the poll interval is
/// minute-grained, and re-reading the setting each tick means a change in
/// the settings window takes effect within a minute, no restart needed.
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

/// How long a message's HTML is kept after it was last shown (or fetched,
/// if it never was). After that only its text stays; opening it fetches the
/// HTML again. Asked for 2026-09-27: HTML is ~90% of the cached body bytes.
const HTML_RETENTION_DAYS: i64 = 60;
const DAY_SECS: i64 = 24 * 60 * 60;

/// Whether the daily HTML retention should run, given when it last did.
fn retention_due(last_run: Option<i64>, now: i64) -> bool {
    last_run.is_none_or(|last| now - last >= DAY_SECS)
}

/// HTML last touched before this (Unix seconds) is dropped.
fn html_cutoff(now: i64) -> i64 {
    now - HTML_RETENTION_DAYS * DAY_SECS
}

/// Spawned once at startup.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        // why Delay: after a long system sleep we want one late tick, not a
        // burst of catch-up ticks all deciding to poll at once.
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // why: the first interval.tick() resolves immediately; consuming it
        // here means "minutes since the last poll" starts counting from
        // startup (the frontend already syncs everything on launch).
        interval.tick().await;

        let mut elapsed_minutes: i64 = 0;
        let mut wake = WakeDetector::new(std::time::Instant::now(), wake::wall_now());
        let mut last_retention = None;
        loop {
            interval.tick().await;
            elapsed_minutes += 1;
            let woke = wake.woke(std::time::Instant::now(), wake::wall_now());

            // why here, before the poll settings: retention is housekeeping
            // of what is already cached, and must run even with background
            // checks switched off.
            let now = wake::wall_now();
            if retention_due(last_retention, now) {
                last_retention = Some(now);
                let state = app.state::<AppState>();
                match storage::messages::drop_stale_html(&state.pool, html_cutoff(now)).await {
                    Ok(0) => {}
                    Ok(dropped) => eprintln!("dropped the HTML of {dropped} stale messages"),
                    Err(err) => eprintln!("HTML retention failed: {err}"),
                }
            }

            let configured = {
                let state = app.state::<AppState>();
                match storage::settings::notification_settings(&state.pool).await {
                    Ok(settings) => settings.sync_interval_minutes,
                    Err(err) => {
                        eprintln!("background poll could not read settings: {err}");
                        continue;
                    }
                }
            };
            // 0 = background checks off; keep the counter parked at zero so
            // re-enabling starts a fresh interval instead of firing at once.
            if configured == 0 {
                elapsed_minutes = 0;
                continue;
            }
            // why poll right after a wake: the passes in flight when the Mac
            // went to sleep died with their connections, and whatever arrived
            // overnight would otherwise wait out the rest of the interval.
            if elapsed_minutes < configured && !woke {
                continue;
            }
            elapsed_minutes = 0;
            poll(&app).await;
        }
    });
}

/// One poll pass: every account syncs in parallel (design goal — never
/// sequentially). Failures are logged and recorded per account by run_sync
/// itself; one broken account never blocks the others.
async fn poll(app: &AppHandle) {
    let accounts = {
        let state = app.state::<AppState>();
        match storage::accounts::list(&state.pool).await {
            Ok(accounts) => accounts,
            Err(err) => {
                eprintln!("background poll could not list accounts: {err}");
                return;
            }
        }
    };
    let ids: Vec<i64> = accounts.iter().map(|account| account.id).collect();
    let jobs = ids
        .iter()
        .map(|id| commands::run_sync(app, *id, commands::SyncScope::Everything));
    for (id, outcome) in ids.iter().zip(futures::future::join_all(jobs).await) {
        if let Err(err) = outcome {
            eprintln!("background sync failed for account {id}: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_retention_runs_on_the_first_tick_and_then_daily() {
        assert!(retention_due(None, 1_000));
        assert!(!retention_due(Some(1_000), 1_000 + 86_399));
        assert!(retention_due(Some(1_000), 1_000 + 86_400));
    }

    #[test]
    fn html_older_than_sixty_days_is_past_the_cutoff() {
        assert_eq!(html_cutoff(100 * 86_400), 40 * 86_400);
    }
}
