//! Opt-in timing for the performance work.
//!
//! Every hot path we suspect (DB init, the list query, opening a message,
//! IMAP connect, the mutating commands) wraps itself in a [`Timer`]. Nothing
//! is printed unless `FLIT_TIMING` is set in the environment, so ordinary
//! runs stay silent and pay nothing but an atomic load.
//!
//! why an env var rather than `#[cfg(debug_assertions)]`: the numbers that
//! matter come from a release build against a real mailbox — a debug-only
//! switch would measure the wrong binary.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Is timing output switched on? Reads `FLIT_TIMING` once per process.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| switched_on(std::env::var("FLIT_TIMING").ok().as_deref()))
}

/// The env var's meaning, split out so it can be tested without touching
/// process-global state.
fn switched_on(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty() && v != "0")
}

/// One line of timing output. Milliseconds with one decimal: the paths we
/// care about run from a fraction of a millisecond to seconds, and a decimal
/// keeps the fast ones from all reading "0 ms".
fn format_line(label: &str, elapsed: Duration) -> String {
    format!("[timing] {label} {:.1} ms", elapsed.as_secs_f64() * 1000.0)
}

/// Times the scope it lives in and prints when dropped.
///
/// why Drop rather than an explicit stop: held across `.await` points it
/// covers the whole async call, including every await, and it still reports
/// when the path returns early through `?`.
pub struct Timer {
    label: &'static str,
    start: Instant,
}

/// Start timing `label`, or `None` when timing is off — dropping `None`
/// costs nothing, so call sites need no branch of their own.
///
/// why `&'static str`: every call site names a fixed path, so nothing has to
/// be formatted (and no allocation happens) just to be thrown away when
/// timing is off. Labels carrying runtime values can be added if we ever
/// need them.
pub fn start(label: &'static str) -> Option<Timer> {
    enabled().then(|| Timer {
        label,
        start: Instant::now(),
    })
}

impl Drop for Timer {
    fn drop(&mut self) {
        eprintln!("{}", format_line(self.label, self.start.elapsed()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switched_on_only_for_a_meaningful_value() {
        assert!(switched_on(Some("1")));
        assert!(switched_on(Some("yes")));
        // Unset, empty and an explicit zero all mean off — so `FLIT_TIMING=0`
        // in a shell profile reads the way anyone would expect.
        assert!(!switched_on(None));
        assert!(!switched_on(Some("")));
        assert!(!switched_on(Some("0")));
    }

    #[test]
    fn format_line_reports_milliseconds_with_one_decimal() {
        assert_eq!(
            format_line("list_threaded", Duration::from_micros(1500)),
            "[timing] list_threaded 1.5 ms"
        );
        assert_eq!(
            format_line("storage::init", Duration::from_millis(2340)),
            "[timing] storage::init 2340.0 ms"
        );
    }

    #[test]
    fn start_yields_nothing_while_timing_is_off() {
        // The test process has no FLIT_TIMING set, so call sites get None and
        // the whole mechanism compiles down to a dropped Option.
        assert!(start("unused").is_none());
    }
}
