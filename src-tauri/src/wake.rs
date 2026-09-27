//! Noticing that the Mac has just woken up.
//!
//! Background loops tick on tokio's clock, which is monotonic — and on
//! macOS the monotonic clock stands still while the machine sleeps. So the
//! first tick after a night's sleep looks like any other to the loop, while
//! the wall clock has jumped hours ahead. Comparing the two is how a loop
//! can tell, without any OS notification API.
//!
//! why a loop needs to know: measured 2026-09-27, the passes in flight when
//! the Mac went to sleep died silently on wake, push connections could sit
//! dead for up to their 29-minute re-IDLE, and nothing caught up on the mail
//! that arrived overnight until the user clicked something or the next poll.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// How much further the wall clock must have moved than the loop's own
/// clock before the gap counts as a sleep. Far above timer jitter and
/// ordinary clock corrections, far below any sleep worth catching up on.
const SLEPT_AFTER_SECS: i64 = 90;

/// Tracks the time between ticks of a background loop.
pub struct WakeDetector {
    last_mono: Instant,
    last_wall: i64,
}

impl WakeDetector {
    pub fn new(mono: Instant, wall: i64) -> Self {
        Self {
            last_mono: mono,
            last_wall: wall,
        }
    }

    /// Record a tick at `mono` / `wall` (Unix seconds); true when the time
    /// since the previous tick included a sleep.
    pub fn woke(&mut self, mono: Instant, wall: i64) -> bool {
        let mono_secs = mono.saturating_duration_since(self.last_mono).as_secs() as i64;
        let wall_secs = wall - self.last_wall;
        self.last_mono = mono;
        self.last_wall = wall;
        wall_secs - mono_secs > SLEPT_AFTER_SECS
    }
}

/// The wall clock in Unix seconds (0 if it reads before 1970).
pub fn wall_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_normal_tick_is_not_a_wake() {
        let start = Instant::now();
        let mut wake = WakeDetector::new(start, 1_000);

        assert!(!wake.woke(start + Duration::from_secs(60), 1_060));
        // A little clock jitter is still not a sleep.
        assert!(!wake.woke(start + Duration::from_secs(120), 1_125));
    }

    #[test]
    fn a_wall_clock_jump_past_the_tick_is_a_wake() {
        // Tokio's clock is monotonic, and on macOS that clock stands still
        // while the machine sleeps: the loop ticks 60 s after the previous
        // tick by its clock, while the wall clock has moved on 45 minutes.
        let start = Instant::now();
        let mut wake = WakeDetector::new(start, 1_000);

        assert!(wake.woke(start + Duration::from_secs(60), 1_000 + 45 * 60));
        // The tick after that is ordinary again.
        assert!(!wake.woke(start + Duration::from_secs(120), 1_000 + 46 * 60));
    }
}
