//! The "Send Later" scheduler: one 60-second tick that flags slept-through
//! rows as missed (the user decides those in the catch-up dialog) and
//! delivers everything else that has come due.

use tauri::{AppHandle, Emitter, Manager};
use tokio::time::MissedTickBehavior;

use crate::state::AppState;
use crate::{commands, storage};

/// How often due messages are looked for. Scheduling is minute-grained
/// (datetime-local has no seconds), so a finer tick would buy nothing.
const TICK: std::time::Duration = std::time::Duration::from_secs(60);

/// A pending row overdue by more than this when a tick finds it did not come
/// due while the app was awake — the Mac slept past its time. It turns into
/// a missed row instead of sending, so waking the lid never fires off stale
/// mail unasked.
const GRACE_SECS: i64 = 15 * 60;

/// Spawned once at startup.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Anything already due right now was missed while the app was
        // closed — flag it before the first tick can claim it.
        sweep_missed(&app, commands::now_epoch()).await;

        let mut interval = tokio::time::interval(TICK);
        // why Delay: after a long system sleep we want ONE tick that claims
        // everything due at once, not a burst of catch-up ticks.
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            tick(&app).await;
        }
    });
}

/// One scheduler pass: stale rows become missed first, then every row still
/// pending and due is claimed and delivered. Errors are logged, never fatal —
/// the next tick simply tries again.
async fn tick(app: &AppHandle) {
    let now = commands::now_epoch();
    sweep_missed(app, now - GRACE_SECS).await;

    let due = {
        let state = app.state::<AppState>();
        match storage::scheduled::claim_due(&state.pool, now).await {
            Ok(due) => due,
            Err(err) => {
                eprintln!("send-later claim failed: {err}");
                return;
            }
        }
    };
    if due.is_empty() {
        return;
    }
    for message in &due {
        commands::deliver_scheduled(app, message.outgoing()).await;
    }
    let _ = app.emit("scheduled-changed", ());
}

/// Flag pending rows due at or before `cutoff` as missed and tell the main
/// window when there is something new to ask about.
async fn sweep_missed(app: &AppHandle, cutoff: i64) {
    let state = app.state::<AppState>();
    match storage::scheduled::mark_missed(&state.pool, cutoff).await {
        Ok(missed) if !missed.is_empty() => {
            let _ = app.emit("scheduled-missed", ());
        }
        Ok(_) => {}
        Err(err) => eprintln!("send-later missed sweep failed: {err}"),
    }
}
