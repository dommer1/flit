//! Wall-clock time in the form the database stores it.

/// Seconds since the Unix epoch; 0 if the clock reads before 1970.
pub fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
