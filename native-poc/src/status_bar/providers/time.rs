//! Time provider for `{time}`.
//!
//! Formats the current local time using a token-based template
//! (e.g. `HH:mm:ss`). The format string lives on
//! `Settings::statusbar.time_format` and is supplied at construction.
//!
//! Unix uses `libc::localtime_r` so the result respects `TZ`. On
//! Windows we fall back to UTC arithmetic for now; a follow-up can
//! introduce `GetLocalTime` if test machines surface drift.
//!
//! ## Self-owned timer thread
//!
//! Each `TimeProvider` instance owns a dedicated timer thread that
//! periodically calls the injected [`WakeFn`] (see [`crate::wakeup`]).
//! Without this, the egui frame stalls on idle PTYs because
//! `Context::request_repaint_after` does not bridge to winit; the
//! clock simply stops ticking in release builds. The timer thread
//! sleeps on a `Condvar::wait_timeout` so [`Drop`] can wake it
//! immediately by flipping a stop flag and calling `notify_all`. The
//! `JoinHandle` is then joined to guarantee no thread leaks (TS-perf-3).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::html::CssColor;
use crate::status_bar::template_engine::VariableProvider;
use crate::wakeup::WakeFn;

/// Configuration for the [`TimeProvider`] timer thread.
///
/// `interval` is the period between consecutive wake calls. The
/// status-bar runtime sources this from `refresh_rates["time"]`
/// (default 1000 ms per SPEC FR3 / FR10).
#[derive(Debug, Clone, Copy)]
pub struct RefreshConfig {
    pub interval: Duration,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(1000),
        }
    }
}

/// Provider that exposes the local wall clock under `{time}`.
pub struct TimeProvider {
    /// User-supplied format spec. Shared with the cache so callers
    /// can mutate it (e.g. when the settings UI lands).
    format: Mutex<String>,
    /// Per-provider monotonic version counter. Phase F cache uses
    /// this to invalidate stale runs.
    version: AtomicU64,
    /// Last rendered second (since epoch). Used to bump `version`
    /// only when the visible value changes.
    last_second: AtomicU64,
    /// Timer-thread coordination. `stop` is flipped to `true` by the
    /// `Drop` impl; the timer wakes from `Condvar::wait_timeout` and
    /// exits the loop. `cv` is the matching `(Mutex<()>, Condvar)`
    /// pair shared with the worker.
    stop: Arc<AtomicBool>,
    cv: Arc<(Mutex<()>, Condvar)>,
    /// `JoinHandle` for the timer thread. `Drop` takes the handle out
    /// of the `Option` and joins it so the test runner can verify
    /// `TimeProvider` does not leak threads (TS-perf-3).
    join: Mutex<Option<JoinHandle<()>>>,
}

impl TimeProvider {
    /// Construct without a timer thread.
    ///
    /// Retained as a convenience for unit tests that exercise the
    /// pull-style `get_value` path without spawning a thread.
    /// Production code should call [`Self::with_wake`] instead so the
    /// clock keeps ticking on otherwise-idle PTYs.
    pub fn new(format: impl Into<String>) -> Self {
        Self {
            format: Mutex::new(format.into()),
            version: AtomicU64::new(0),
            last_second: AtomicU64::new(u64::MAX),
            stop: Arc::new(AtomicBool::new(false)),
            cv: Arc::new((Mutex::new(()), Condvar::new())),
            join: Mutex::new(None),
        }
    }

    /// Construct with a self-owned timer thread.
    ///
    /// The thread fires `wake` every `refresh.interval` so the egui
    /// frame schedules a redraw and `get_value` recomputes the wall
    /// clock. `Drop` stops + joins the thread.
    pub fn with_wake(format: impl Into<String>, wake: WakeFn, refresh: RefreshConfig) -> Self {
        let provider = Self::new(format);
        provider.spawn_timer(wake, refresh.interval);
        provider
    }

    fn spawn_timer(&self, wake: WakeFn, interval: Duration) {
        let stop = self.stop.clone();
        let cv = self.cv.clone();
        let handle = std::thread::Builder::new()
            .name("time-provider-timer".into())
            .spawn(move || timer_loop(stop, cv, interval, wake))
            .expect("failed to spawn time-provider-timer");
        *self.join.lock().unwrap() = Some(handle);
    }

    /// Replace the format spec. The next `get_value` call uses the
    /// new spec.
    #[allow(dead_code)]
    pub fn set_format(&self, format: impl Into<String>) {
        *self.format.lock().unwrap() = format.into();
        // Bump version so cached run-lists invalidate.
        self.version.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for TimeProvider {
    fn drop(&mut self) {
        // Flip the stop flag and wake the timer out of
        // `wait_timeout`. Joining is best-effort: if the timer thread
        // panicked before reading the flag we still want `Drop` to
        // complete.
        self.stop.store(true, Ordering::Relaxed);
        let (_, cv) = &*self.cv;
        cv.notify_all();
        if let Some(handle) = self.join.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

fn timer_loop(
    stop: Arc<AtomicBool>,
    cv: Arc<(Mutex<()>, Condvar)>,
    interval: Duration,
    wake: WakeFn,
) {
    let (m, cond) = &*cv;
    while !stop.load(Ordering::Relaxed) {
        // Drop the guard immediately after `wait_timeout` returns so
        // the next iteration can re-acquire it without holding the
        // lock during `wake()`.
        let guard = m.lock().unwrap();
        let (_g, _res) = cond.wait_timeout(guard, interval).unwrap();
        if stop.load(Ordering::Relaxed) {
            break;
        }
        wake();
    }
}

impl VariableProvider for TimeProvider {
    fn name(&self) -> &str {
        "time"
    }

    fn get_value(&self, _argument: Option<&str>) -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Bump the version counter when the wall-clock second crosses
        // a boundary so Phase F's cache invalidates correctly.
        let prev = self.last_second.swap(secs, Ordering::Relaxed);
        if prev != secs {
            self.version.fetch_add(1, Ordering::Relaxed);
        }
        let fmt = self.format.lock().unwrap().clone();
        let (y, mo, d, h, mi, s) = local_components(secs as i64);
        format_with(&fmt, y, mo, d, h, mi, s)
    }

    fn get_color(&self, _argument: Option<&str>) -> Option<CssColor> {
        None
    }

    fn version(&self, _argument: Option<&str>) -> u64 {
        self.version.load(Ordering::Relaxed)
    }
}

#[cfg(unix)]
fn local_components(secs: i64) -> (u32, u32, u32, u32, u32, u32) {
    use std::mem::MaybeUninit;
    let t: libc::time_t = secs as libc::time_t;
    let mut tm_buf: MaybeUninit<libc::tm> = MaybeUninit::uninit();
    let res = unsafe { libc::localtime_r(&t, tm_buf.as_mut_ptr()) };
    if res.is_null() {
        return utc_components(secs);
    }
    let tm = unsafe { tm_buf.assume_init() };
    (
        (tm.tm_year + 1900) as u32,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    )
}

#[cfg(not(unix))]
fn local_components(secs: i64) -> (u32, u32, u32, u32, u32, u32) {
    utc_components(secs)
}

/// UTC date+time decomposition by integer arithmetic. Used as the
/// non-unix path and the fallback when `localtime_r` fails.
fn utc_components(secs: i64) -> (u32, u32, u32, u32, u32, u32) {
    let day = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400) as u32;
    let h = day_secs / 3600;
    let mi = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    let (y, mo, d) = days_to_ymd(day);
    (y, mo, d, h, mi, s)
}

/// Convert days-since-1970-01-01 to (year, month, day) using the
/// Howard Hinnant civil-from-days algorithm (public domain).
fn days_to_ymd(z: i64) -> (u32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

/// Apply a token-based format to the supplied components.
///
/// Supported tokens (longest-match first):
/// - `YYYY` / `YY` - year
/// - `MM` - month (zero-padded)
/// - `DD` - day-of-month (zero-padded)
/// - `HH` - 24h hour (zero-padded)
/// - `hh` - 12h hour (zero-padded; midnight/noon → 12)
/// - `mm` - minute
/// - `ss` - second
/// - `A` - "AM" or "PM"
/// - `a` - "am" or "pm"
///
/// Any other characters are emitted verbatim. Unknown letter runs are
/// passed through to keep punctuation (`:`, `-`, `/`, etc.) literal.
pub fn format_with(fmt: &str, y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> String {
    let bytes = fmt.as_bytes();
    let mut out = String::with_capacity(fmt.len() + 4);
    let mut i = 0;
    let len = bytes.len();
    while i < len {
        let rest = &fmt[i..];
        if rest.starts_with("YYYY") {
            out.push_str(&format!("{:04}", y));
            i += 4;
        } else if rest.starts_with("YY") {
            out.push_str(&format!("{:02}", y % 100));
            i += 2;
        } else if rest.starts_with("MM") {
            out.push_str(&format!("{:02}", mo));
            i += 2;
        } else if rest.starts_with("DD") {
            out.push_str(&format!("{:02}", d));
            i += 2;
        } else if rest.starts_with("HH") {
            out.push_str(&format!("{:02}", h));
            i += 2;
        } else if rest.starts_with("hh") {
            let h12 = match h % 12 {
                0 => 12,
                n => n,
            };
            out.push_str(&format!("{:02}", h12));
            i += 2;
        } else if rest.starts_with("mm") {
            out.push_str(&format!("{:02}", mi));
            i += 2;
        } else if rest.starts_with("ss") {
            out.push_str(&format!("{:02}", s));
            i += 2;
        } else if rest.starts_with('A') {
            out.push_str(if h < 12 { "AM" } else { "PM" });
            i += 1;
        } else if rest.starts_with('a') {
            out.push_str(if h < 12 { "am" } else { "pm" });
            i += 1;
        } else {
            // Copy one UTF-8 codepoint verbatim.
            let cp_len = utf8_len(bytes[i]);
            out.push_str(&fmt[i..(i + cp_len).min(len)]);
            i += cp_len;
        }
    }
    out
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_basic_hhmmss() {
        assert_eq!(format_with("HH:mm:ss", 2026, 5, 22, 13, 4, 9), "13:04:09");
    }

    #[test]
    fn format_year_month_day() {
        assert_eq!(
            format_with("YYYY-MM-DD", 2026, 5, 22, 0, 0, 0),
            "2026-05-22"
        );
    }

    #[test]
    fn format_two_digit_year() {
        assert_eq!(format_with("YY-MM-DD", 2026, 5, 22, 0, 0, 0), "26-05-22");
    }

    #[test]
    fn format_12h_with_am_pm_boundaries() {
        // Midnight → 12 AM
        assert_eq!(format_with("hh:mm A", 2026, 1, 1, 0, 0, 0), "12:00 AM");
        // Noon → 12 PM
        assert_eq!(format_with("hh:mm A", 2026, 1, 1, 12, 0, 0), "12:00 PM");
        // 1 AM
        assert_eq!(format_with("hh:mm a", 2026, 1, 1, 1, 0, 0), "01:00 am");
        // 1 PM
        assert_eq!(format_with("hh:mm a", 2026, 1, 1, 13, 0, 0), "01:00 pm");
        // 11 PM
        assert_eq!(format_with("hh:mm a", 2026, 1, 1, 23, 0, 0), "11:00 pm");
    }

    #[test]
    fn format_unknown_literal_chars_are_passed_through() {
        assert_eq!(format_with("[HH]", 2026, 1, 1, 9, 0, 0), "[09]");
    }

    #[test]
    fn format_preserves_utf8_separator() {
        assert_eq!(format_with("HH時mm分", 2026, 1, 1, 9, 5, 0), "09時05分");
    }

    #[test]
    fn provider_get_value_round_trip_shape() {
        let p = TimeProvider::new("HH:mm:ss");
        let v = p.get_value(None);
        assert_eq!(v.len(), 8);
        // Format guard: `HH:mm:ss` is 8 ASCII bytes.
        assert_eq!(v.as_bytes()[2], b':');
        assert_eq!(v.as_bytes()[5], b':');
    }

    #[test]
    fn provider_set_format_bumps_version() {
        let p = TimeProvider::new("HH");
        let _ = p.get_value(None);
        let v1 = p.version(None);
        p.set_format("HH:mm");
        // set_format always bumps regardless of clock tick.
        assert!(p.version(None) > v1);
    }

    #[test]
    fn days_to_ymd_known_dates() {
        // 1970-01-01 → day 0
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        // 2000-01-01 → day 10957
        assert_eq!(days_to_ymd(10_957), (2000, 1, 1));
        // 2024-02-29 (leap day) → day 19782
        assert_eq!(days_to_ymd(19_782), (2024, 2, 29));
    }

    #[test]
    fn utc_components_for_known_epoch() {
        // 2026-01-01T00:00:00Z = 1767225600
        let (y, mo, d, h, mi, s) = utc_components(1_767_225_600);
        assert_eq!((y, mo, d, h, mi, s), (2026, 1, 1, 0, 0, 0));
    }

    // ── TS-29 + TS-perf-3: timer thread + Drop join ─────────────

    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    fn counter_wake() -> (WakeFn, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let wake: WakeFn = Arc::new(move || {
            c2.fetch_add(1, Ordering::Relaxed);
        });
        (wake, count)
    }

    /// TS-29: TimeProvider's timer thread fires `WakeFn` on the
    /// configured interval. We use a short 25 ms interval and wait
    /// long enough for at least two ticks.
    #[test]
    fn time_provider_timer_thread_calls_wake_on_interval() {
        let (wake, count) = counter_wake();
        let p = TimeProvider::with_wake(
            "HH:mm:ss",
            wake,
            RefreshConfig {
                interval: Duration::from_millis(25),
            },
        );
        // Sleep long enough for ≥ 2 intervals to elapse.
        std::thread::sleep(Duration::from_millis(120));
        let observed = count.load(Ordering::Relaxed);
        assert!(
            observed >= 2,
            "expected ≥2 wake calls, got {observed} in 120ms with 25ms interval"
        );
        drop(p);
    }

    /// TS-perf-3: Drop signals the timer thread, joins it, and
    /// leaves no residual thread. We assert the call completes
    /// within a generous bound — the timer's wait_timeout cycle
    /// should observe `stop` immediately via `notify_all`.
    #[test]
    fn time_provider_drop_joins_timer_thread() {
        let (wake, _count) = counter_wake();
        let p = TimeProvider::with_wake(
            "HH:mm:ss",
            wake,
            RefreshConfig {
                // Long interval so the join must rely on `notify_all`,
                // not on the natural wait_timeout expiry.
                interval: Duration::from_secs(60),
            },
        );
        let start = std::time::Instant::now();
        drop(p);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "Drop must signal+join promptly via notify_all; took {elapsed:?}"
        );
    }

    /// Sanity: when constructed without a timer (legacy `new`), Drop
    /// is still safe and produces no spurious wake calls.
    #[test]
    fn time_provider_without_timer_does_not_spawn_thread() {
        let (wake, count) = counter_wake();
        let _wake_unused = wake; // not handed to provider
        let p = TimeProvider::new("HH:mm:ss");
        std::thread::sleep(Duration::from_millis(40));
        drop(p);
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }
}
