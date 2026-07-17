//! Background new-mail poll: re-reads the configured interval every minute
//! and, when it elapses, syncs all accounts in parallel. This is what makes
//! notifications arrive while the app sits idle — without it, mail would
//! only ever sync when the user clicks something.

use tauri::{AppHandle, Manager};
use tokio::time::MissedTickBehavior;

use crate::state::AppState;
use crate::{commands, storage};

/// One-minute ticks, like the send-later scheduler: the poll interval is
/// minute-grained, and re-reading the setting each tick means a change in
/// the settings window takes effect within a minute, no restart needed.
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

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
        loop {
            interval.tick().await;
            elapsed_minutes += 1;

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
            if elapsed_minutes < configured {
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
    let jobs = ids.iter().map(|id| commands::run_sync(app, *id));
    for (id, outcome) in ids.iter().zip(futures::future::join_all(jobs).await) {
        if let Err(err) = outcome {
            eprintln!("background sync failed for account {id}: {err}");
        }
    }
}
