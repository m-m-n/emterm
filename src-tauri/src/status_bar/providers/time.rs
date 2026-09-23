//! Time provider for `{time}`.
//!
//! Formats the current local time using a token-based template
//! (e.g. `HH:mm:ss`). The format string lives on
//! `Settings::statusbar.time_format` and is supplied at construction.
//!
//! Unix uses `libc::localtime_r` so the result respects `TZ`; Windows
//! uses `libc::localtime_s` so the result respects the system zone
//! (registered via the Control Panel / Time & Language settings).
//! Both paths live in [`crate::localtime`].
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
//!
//! The timer thread is also the **sole owner of the version bump**.
//! Each tick `fetch_add(1)` on `version` before calling `wake()` so
//! the run-cache invalidates exactly once per interval. This keeps
//! `VariableProvider::version()` a pure atomic load (matching the
//! trait contract honored by every other provider) and removes the
//! `SystemTime::now()` syscall from `version()`'s hot path — both
//! `version()` and `get_value()` stay side-effect-free reads.
//!
//! ## Tick source seam (task0001)
//!
//! Where the next tick comes from is decoupled from the loop body
//! (stop check → version bump → wake → completion notice) behind the
//! private [`TickSource`] contract. [`with_wake`](TimeProvider::with_wake)
//! uses [`ProductionTickSource`], an exact reproduction of the timed
//! `Condvar::wait_timeout` above. Test code (`cfg(test)`) can instead
//! drive the loop with a manual tick source so the timer tests assert
//! exact counts without depending on real time.

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

/// Outcome of asking a [`TickSource`] for the next tick.
#[derive(Debug, PartialEq, Eq)]
enum TickOutcome {
    Tick,
    Stop,
}

/// Decouples "where does the next tick come from" from the timer loop
/// body (stop check → version bump → wake → completion notice). The
/// loop body itself is the single copy in [`timer_loop`], shared by
/// every source, so production and manually-driven tests exercise the
/// exact same bump/wake ordering.
///
/// `next_tick` and `tick_done` are called only by the timer thread
/// that owns this source.
trait TickSource: Send {
    /// Returns `Stop` whenever the provider's stop flag is set at
    /// decision time, even if a tick is available.
    fn next_tick(&self) -> TickOutcome;
    /// Called by the loop after `wake()` returns for the tick just
    /// obtained.
    fn tick_done(&self);
}

/// Lets `Drop` unblock a timer thread parked inside a `TickSource`
/// implementation. Held by the provider (not by the timer thread), so
/// it must be independently shareable with whichever source is
/// active.
trait TickInterrupt: Send + Sync {
    /// Called after the stop flag is set. A blocked `next_tick`
    /// re-evaluates the stop flag and returns `Stop` promptly.
    fn interrupt(&self);
}

/// Reproduces today's timing behaviour exactly (one timed wait per
/// tick on the provider's existing `(Mutex<()>, Condvar)` pair).
struct ProductionTickSource {
    stop: Arc<AtomicBool>,
    cv: Arc<(Mutex<()>, Condvar)>,
    interval: Duration,
}

impl TickSource for ProductionTickSource {
    fn next_tick(&self) -> TickOutcome {
        let (m, cond) = &*self.cv;
        // Drop the guard immediately after `wait_timeout` returns so
        // the next iteration can re-acquire it without holding the
        // lock during `wake()`.
        let guard = m.lock().unwrap();
        let (_g, _res) = cond.wait_timeout(guard, self.interval).unwrap();
        if self.stop.load(Ordering::Relaxed) {
            TickOutcome::Stop
        } else {
            // An early or spurious `wait_timeout` return counts as a
            // tick whenever stop is not set -- identical to the
            // pre-seam loop.
            TickOutcome::Tick
        }
    }

    fn tick_done(&self) {}
}

/// No-op: `Drop`'s existing `notify_all` on the provider's `cv` pair
/// (the same pair `ProductionTickSource` waits on) already unblocks a
/// parked production wait, so there is nothing extra to do here.
struct NoopInterrupt;

impl TickInterrupt for NoopInterrupt {
    fn interrupt(&self) {}
}

/// Provider that exposes the local wall clock under `{time}`.
pub struct TimeProvider {
    /// User-supplied format spec. Shared with the cache so callers
    /// can mutate it (e.g. when the settings UI lands).
    format: Mutex<String>,
    /// Per-provider monotonic version counter. Phase F cache uses
    /// this to invalidate stale runs. `Arc` so the timer thread can
    /// hold its own handle and bump the counter once per tick; the
    /// `version()` trait method is then a pure atomic load and
    /// `get_value()` never touches this counter.
    version: Arc<AtomicU64>,
    /// Timer-thread coordination. `stop` is flipped to `true` by the
    /// `Drop` impl. `cv` is the provider's own `(Mutex<()>, Condvar)`
    /// pair; `Drop` always notifies it (unchanged), and the
    /// production tick source waits on this same pair.
    stop: Arc<AtomicBool>,
    cv: Arc<(Mutex<()>, Condvar)>,
    /// `JoinHandle` for the timer thread. `Drop` takes the handle out
    /// of the `Option` and joins it so the test runner can verify
    /// `TimeProvider` does not leak threads (TS-perf-3).
    join: Mutex<Option<JoinHandle<()>>>,
    /// Handle used by `Drop` to unblock whichever tick source is
    /// active. `None` when no timer thread was spawned (`Self::new`).
    interrupt: Option<Arc<dyn TickInterrupt>>,
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
            version: Arc::new(AtomicU64::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            cv: Arc::new((Mutex::new(()), Condvar::new())),
            join: Mutex::new(None),
            interrupt: None,
        }
    }

    /// Construct with a self-owned timer thread.
    ///
    /// The thread fires `wake` every `refresh.interval` so the egui
    /// frame schedules a redraw and `get_value` recomputes the wall
    /// clock. `Drop` stops + joins the thread.
    pub fn with_wake(format: impl Into<String>, wake: WakeFn, refresh: RefreshConfig) -> Self {
        let mut provider = Self::new(format);
        let source = ProductionTickSource {
            stop: provider.stop.clone(),
            cv: provider.cv.clone(),
            interval: refresh.interval,
        };
        provider.interrupt = Some(Arc::new(NoopInterrupt));
        provider.spawn_timer(wake, source);
        provider
    }

    /// Construct with a self-owned timer thread driven by the
    /// test-only manual tick source instead of real time. Returns the
    /// provider plus the [`ManualTickController`] that drives it.
    #[cfg(test)]
    pub(crate) fn with_manual_source(
        format: impl Into<String>,
        wake: WakeFn,
    ) -> (Self, ManualTickController) {
        let mut provider = Self::new(format);
        let shared = Arc::new(ManualShared::default());
        let source = ManualTickSource {
            stop: provider.stop.clone(),
            shared: shared.clone(),
        };
        let controller = ManualTickController {
            shared: shared.clone(),
        };
        provider.interrupt = Some(Arc::new(ManualInterrupt { shared }));
        provider.spawn_timer(wake, source);
        (provider, controller)
    }

    fn spawn_timer<S: TickSource + 'static>(&self, wake: WakeFn, source: S) {
        let stop = self.stop.clone();
        let version = self.version.clone();
        let handle = std::thread::Builder::new()
            .name("time-provider-timer".into())
            .spawn(move || timer_loop(stop, source, wake, version))
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
        // Interrupt whichever tick source is active: for the
        // production source this adds nothing beyond the notify_all
        // above; for the manual source it is what lets a thread
        // parked on a tick that never comes exit.
        if let Some(interrupt) = &self.interrupt {
            interrupt.interrupt();
        }
        if let Some(handle) = self.join.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

/// Single loop body shared by every tick source: ask for the next
/// tick, leave without effect on `Stop` (or if stop was set while the
/// outcome was pending), otherwise bump `version` then `wake()`, then
/// tell the source the tick is done.
fn timer_loop<S: TickSource>(
    stop: Arc<AtomicBool>,
    source: S,
    wake: WakeFn,
    version: Arc<AtomicU64>,
) {
    while !stop.load(Ordering::Relaxed) {
        let outcome = source.next_tick();
        if matches!(outcome, TickOutcome::Stop) || stop.load(Ordering::Relaxed) {
            break;
        }
        // Bump first, then wake: when the main thread services the
        // resulting redraw, the run-cache's (template, version) key
        // will already have shifted so the lookup misses and
        // `get_value()` re-runs against the current `SystemTime`.
        // Without the bump on this side, a cache hit would keep
        // returning the previous tick's formatted string forever.
        version.fetch_add(1, Ordering::Relaxed);
        wake();
        source.tick_done();
    }
}

/// Test-only manual tick source / controller pair (task0001). Lets a
/// test deliver ticks and wait for their completion instead of
/// relying on real time (NFR2).
///
/// The pair shares one mutex-protected [`ManualState`] and one
/// condition variable. `ManualTickSource` (this module) is owned
/// solely by the timer thread; `ManualTickController` is owned by
/// test code; `ManualInterrupt` is the handle the provider's `Drop`
/// holds to unblock a parked source.
#[cfg(test)]
#[derive(Default)]
struct ManualState {
    /// Ticks staged by `hold` but not yet released -- invisible to
    /// the timer thread.
    held: u64,
    /// Ticks released and available for `next_tick` to consume.
    available: u64,
    /// Running count of completed ticks (`tick_done` calls).
    completed_total: u64,
    /// True while `next_tick` is blocked with nothing available.
    parked: bool,
    /// True once the source side has dropped (timer-thread exit,
    /// including unwinding).
    closed: bool,
}

#[cfg(test)]
struct ManualShared {
    state: Mutex<ManualState>,
    cv: Condvar,
}

#[cfg(test)]
impl Default for ManualShared {
    fn default() -> Self {
        Self {
            state: Mutex::new(ManualState::default()),
            cv: Condvar::new(),
        }
    }
}

/// Source side: owned solely by the timer thread. Its `Drop` marks
/// the shared state closed and wakes every controller waiter -- this
/// runs whenever the timer thread's loop exits, including unwinding,
/// since it is a local of `timer_loop`'s stack frame.
#[cfg(test)]
struct ManualTickSource {
    stop: Arc<AtomicBool>,
    shared: Arc<ManualShared>,
}

#[cfg(test)]
impl TickSource for ManualTickSource {
    fn next_tick(&self) -> TickOutcome {
        let mut guard = self.shared.state.lock().unwrap();
        loop {
            // Stop takes priority even if a tick is available, and is
            // re-checked under the shared mutex every time round this
            // loop -- this is what makes `ManualInterrupt::interrupt`
            // race-free (no lost wake-up).
            if self.stop.load(Ordering::Relaxed) {
                return TickOutcome::Stop;
            }
            if guard.available > 0 {
                guard.available -= 1;
                return TickOutcome::Tick;
            }
            guard.parked = true;
            self.shared.cv.notify_all();
            guard = self.shared.cv.wait(guard).unwrap();
            guard.parked = false;
        }
    }

    fn tick_done(&self) {
        let mut guard = self.shared.state.lock().unwrap();
        guard.completed_total += 1;
        drop(guard);
        self.shared.cv.notify_all();
    }
}

#[cfg(test)]
impl Drop for ManualTickSource {
    fn drop(&mut self) {
        let mut guard = self.shared.state.lock().unwrap();
        guard.closed = true;
        drop(guard);
        self.shared.cv.notify_all();
    }
}

/// Interrupt handle held by the provider (not the timer thread) so
/// `Drop` can unblock a source parked in `next_tick` even though the
/// `ManualTickSource` value itself is owned by the timer thread.
#[cfg(test)]
struct ManualInterrupt {
    shared: Arc<ManualShared>,
}

#[cfg(test)]
impl TickInterrupt for ManualInterrupt {
    fn interrupt(&self) {
        // Take the shared mutex before notifying: `next_tick`
        // re-checks stop under the same mutex before blocking, so
        // there is no window where a notify can be sent between that
        // check and the blocking wait (no lost wake-up).
        let guard = self.shared.state.lock().unwrap();
        drop(guard);
        self.shared.cv.notify_all();
    }
}

/// Controller side: owned by test code, drives the manual tick
/// source and waits for its effects.
#[cfg(test)]
pub(crate) struct ManualTickController {
    shared: Arc<ManualShared>,
}

#[cfg(test)]
impl ManualTickController {
    /// Stage `k` ticks the timer thread cannot see yet. No effect on
    /// counts until [`Self::release_and_wait`].
    pub(crate) fn hold(&self, k: u64) {
        let mut guard = self.shared.state.lock().unwrap();
        guard.held += k;
    }

    /// Make every held tick available, then block without any
    /// timeout until each released tick has been recorded as
    /// completed or the state is closed. Returns the number
    /// completed.
    ///
    /// Because completion is recorded under the shared mutex after
    /// `wake()` returns, once this returns the caller observes the
    /// version bump and every effect of `wake()` for each completed
    /// tick.
    pub(crate) fn release_and_wait(&self) -> u64 {
        let mut guard = self.shared.state.lock().unwrap();
        let released = guard.held;
        guard.available += released;
        guard.held = 0;
        let baseline = guard.completed_total;
        drop(guard);
        self.shared.cv.notify_all();

        let mut guard = self.shared.state.lock().unwrap();
        loop {
            let done = guard.completed_total - baseline;
            if done >= released || guard.closed {
                return done.min(released);
            }
            guard = self.shared.cv.wait(guard).unwrap();
        }
    }

    /// Block until the timer thread is parked or the state is closed.
    pub(crate) fn wait_parked(&self) {
        let mut guard = self.shared.state.lock().unwrap();
        while !guard.parked && !guard.closed {
            guard = self.shared.cv.wait(guard).unwrap();
        }
    }

    /// Whether the timer-thread side is gone.
    pub(crate) fn closed(&self) -> bool {
        self.shared.state.lock().unwrap().closed
    }
}

/// Shared wait-and-check helper (test-only): the single place the
/// timer tests wait for progress and assert exact counts. Contains no
/// sleep, no deadline and no timeout (NFR2) -- progress depends only
/// on `release_and_wait`'s handshake.
#[cfg(test)]
pub(crate) fn release_and_assert_ticks(
    controller: &ManualTickController,
    wake_count: impl Fn() -> u64,
    version: impl Fn() -> u64,
    wake_baseline: u64,
    version_baseline: u64,
    n: u64,
) {
    let completed = controller.release_and_wait();
    assert_eq!(
        completed, n,
        "expected {n} ticks completed, got {completed}"
    );
    assert_eq!(
        wake_count(),
        wake_baseline + n,
        "wake count mismatch after {n} ticks"
    );
    assert_eq!(
        version(),
        version_baseline + n,
        "version mismatch after {n} ticks"
    );
}

impl VariableProvider for TimeProvider {
    fn name(&self) -> &str {
        "time"
    }

    fn get_value(&self, _argument: Option<&str>) -> String {
        // Pure read: the timer thread owns the version bump (see
        // `timer_loop`). `get_value` only formats the current wall
        // clock against the user's format spec.
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let fmt = self.format.lock().unwrap().clone();
        let (y, mo, d, h, mi, s) = crate::localtime::local_components(secs as i64);
        format_with(&fmt, y, mo, d, h, mi, s)
    }

    fn get_color(&self, _argument: Option<&str>) -> Option<CssColor> {
        None
    }

    fn version(&self, _argument: Option<&str>) -> u64 {
        // Pure atomic load matching the `VariableProvider` trait
        // contract. The timer thread owns the per-tick bump (see
        // `timer_loop`), so callers can poll `version()` as often as
        // they want without producing any side effect.
        self.version.load(Ordering::Relaxed)
    }
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

    // Date/time decomposition tests moved with the functions to
    // `crate::localtime` (see native-poc/src/localtime.rs).

    // ── TS-29 + TS-perf-3: timer thread + Drop join ─────────────

    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    fn counter_wake() -> (WakeFn, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let wake: WakeFn = Arc::new(move || {
            c2.fetch_add(1, Ordering::Relaxed);
        });
        (wake, count)
    }

    /// TS-29 (task0001 rewrite, FR5): TimeProvider's timer thread
    /// calls `wake()` exactly once per delivered tick. Driven through
    /// the manual tick source so the count is exact and independent
    /// of real time (NFR2) -- no sleep, no fixed window.
    #[test]
    fn time_provider_timer_thread_calls_wake_on_interval() {
        let (wake, count) = counter_wake();
        let (p, ctrl) = TimeProvider::with_manual_source("HH:mm:ss", wake);
        assert_eq!(count.load(Ordering::Relaxed), 0, "no wake before delivery");
        let v0 = p.version(None);
        const N: u64 = 3;
        let mut wake_baseline = 0u64;
        let mut version_baseline = v0;
        for _ in 0..N {
            ctrl.hold(1);
            release_and_assert_ticks(
                &ctrl,
                || count.load(Ordering::Relaxed) as u64,
                || p.version(None),
                wake_baseline,
                version_baseline,
                1,
            );
            wake_baseline += 1;
            version_baseline += 1;
        }
        assert_eq!(count.load(Ordering::Relaxed) as u64, N);
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

    /// Regression for the cache-freeze bug: the timer thread MUST
    /// bump `version` every tick so the `RunCacheKey`-based per-row
    /// cache invalidates without depending on `get_value` being
    /// called between ticks. Before this contract was enforced, the
    /// status-bar clock froze on an idle PTY because the cache hit
    /// kept returning the previous tick's run-list and `get_value`
    /// (the only bump site at the time) was never reached.
    ///
    /// (task0001 rewrite, FR6): driven through the manual tick source
    /// for an exact count (NFR2) -- no sleep, no fixed window. We
    /// never call `get_value` -- the only bump site under test is the
    /// timer thread.
    #[test]
    fn time_provider_timer_thread_bumps_version_per_tick_without_get_value() {
        let (wake, count) = counter_wake();
        let (p, ctrl) = TimeProvider::with_manual_source("HH:mm:ss", wake);
        let v0 = p.version(None);
        const N: u64 = 3;
        ctrl.hold(N);
        release_and_assert_ticks(
            &ctrl,
            || count.load(Ordering::Relaxed) as u64,
            || p.version(None),
            0,
            v0,
            N,
        );
        drop(p);
    }

    /// New for task0001 (AC-3, FR4): dropping a provider whose timer
    /// thread is parked in the manual source returns; the controller
    /// then reports closed and neither the wake count nor the
    /// retained version counter moved. A tick released after the drop
    /// reports zero completed, does not block, and causes no further
    /// wake or version bump.
    #[test]
    fn time_provider_drop_while_parked_reports_closed_without_wake_or_version_bump() {
        let (wake, count) = counter_wake();
        let (p, ctrl) = TimeProvider::with_manual_source("HH:mm:ss", wake);
        ctrl.wait_parked();
        // Keep our own handle to the version counter -- `p` is about
        // to be dropped and we still need to read it afterwards.
        let version = p.version.clone();
        let v0 = version.load(Ordering::Relaxed);
        let w0 = count.load(Ordering::Relaxed);

        drop(p);

        assert!(ctrl.closed(), "controller must report closed after drop");
        assert_eq!(count.load(Ordering::Relaxed), w0, "no wake from drop");
        assert_eq!(
            version.load(Ordering::Relaxed),
            v0,
            "no version bump from drop"
        );

        // A tick released after the drop reports zero completed, does
        // not block, and causes no wake or version bump.
        ctrl.hold(1);
        let completed = ctrl.release_and_wait();
        assert_eq!(completed, 0, "no one is left to complete a post-drop tick");
        assert_eq!(count.load(Ordering::Relaxed), w0);
        assert_eq!(version.load(Ordering::Relaxed), v0);
    }

    /// New for task0001 (AC-6, FR8, D4): regression test for the
    /// delayed-tick handshake. Ticks held while the timer thread is
    /// parked must NOT advance wake/version until the controller
    /// explicitly releases and waits for them -- there is no sleep
    /// here, and there must not be one. If the shared helper's
    /// handshake ever regresses to "sleep, then check counts", the
    /// held ticks are never released and this test fails regardless
    /// of how long the sleep is, because nothing here delivers a tick
    /// except `release_and_wait`.
    #[test]
    fn time_provider_delayed_ticks_only_progress_via_release_and_wait_handshake() {
        let (wake, count) = counter_wake();
        let (p, ctrl) = TimeProvider::with_manual_source("HH:mm:ss", wake);
        ctrl.wait_parked();
        let v0 = p.version(None);
        const N: u64 = 3;
        ctrl.hold(N);
        // No sleep: immediately after holding, nothing has been
        // released yet, so wake/version must still be at baseline.
        assert_eq!(count.load(Ordering::Relaxed) as u64, 0);
        assert_eq!(p.version(None), v0);
        release_and_assert_ticks(
            &ctrl,
            || count.load(Ordering::Relaxed) as u64,
            || p.version(None),
            0,
            v0,
            N,
        );
        drop(p);
    }

    /// `VariableProvider::version` MUST be a pure atomic load. The
    /// trait's other impls (`cwd`, `git_branch`, `command`) honor
    /// that contract and the per-row cache assumes it. This test
    /// pins it for `TimeProvider`: calling `version` repeatedly
    /// against a parked-timer instance (1-minute interval) must not
    /// advance the counter on its own. Without this guarantee the
    /// `status_bar_view_model_changed` predicate in `App` becomes a
    /// silently side-effecting call.
    #[test]
    fn time_provider_version_is_pure_load() {
        let (wake, _count) = counter_wake();
        let p = TimeProvider::with_wake(
            "HH:mm:ss",
            wake,
            // Interval long enough that no timer tick fires during
            // the test window.
            RefreshConfig {
                interval: Duration::from_secs(60),
            },
        );
        let v0 = p.version(None);
        for _ in 0..1000 {
            assert_eq!(p.version(None), v0);
        }
        drop(p);
    }
}
