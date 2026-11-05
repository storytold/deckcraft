/// Seconds since this process's monotonic origin.
///
/// `web_time::Instant` is `std::time::Instant` natively and `performance.now()` on wasm; it never
/// jumps backwards and is not `Date.now()` (which is wall time and may be zero/frozen in a
/// browser). Use this for deadlines, show/media clocks and duration displays.
static CLOCK_ORIGIN: std::sync::OnceLock<web_time::Instant> = std::sync::OnceLock::new();

/// Monotonic seconds since the first call.
pub fn monotonic_secs() -> f64 {
    CLOCK_ORIGIN.get_or_init(web_time::Instant::now).elapsed().as_secs_f64()
}

/// Monotonic milliseconds. Kept for status/toast durations and frame timings; never wall time.
pub fn now_ms() -> f64 {
    monotonic_secs() * 1000.0
}
