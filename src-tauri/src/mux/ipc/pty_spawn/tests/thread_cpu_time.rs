//! lib-budget-tests-load-tolerance task0001 (FR3, FR4, FR6, FR7, FR8, NFR3,
//! NFR4): the calling thread's CPU time, the CPU budget meter built on it and
//! the judgment of a total against a budget.
//!
//! A wall-clock budget fails a test under machine load even when the code under
//! test is fast. The CPU time of the calling thread does not grow while the
//! thread waits for a core, sleeps or blocks, and never includes the work of
//! the other test threads libtest runs in parallel. A test therefore passes its
//! measured calls through a [`CpuMeter`] and judges the total with
//! [`CpuMeter::judge`].
//!
//! mux-strip-concat-budget-overrun task0002 (FR3, NFR4) adds the judgment of
//! two thread CPU times measured at input sizes N and 4N ([`judge_scaling`]):
//! a cost is linear when the time at 4N is strictly below 8 times the time at
//! N. The judgment reads no clock and holds no platform-specific code.
//!
//! The only platform-specific code is the OS clock reading
//! (`read_os_thread_cpu_time`) and the interpretation of its status
//! (`check_clock_status`). The module reads no wall clock except in the two
//! tests that contrast the helper with one (TS-1, TS-3).

use std::io;
use std::thread::ThreadId;
use std::time::Duration;

// ── The thread CPU-time reading (FR3, FR4, NFR3) ─────────────────────────

/// The total CPU time (user + kernel) the calling thread has consumed so far.
///
/// Two readings on the same thread never decrease. CPU time of any other
/// thread is never included, and time the calling thread spends sleeping or
/// blocked adds nothing. Panics when the OS clock reports a failure (FR4); it
/// never returns zero and never falls back to a wall-clock reading.
pub(super) fn thread_cpu_time() -> Duration {
    read_os_thread_cpu_time()
}

/// Unix: the POSIX per-thread CPU-time clock.
#[cfg(unix)]
fn read_os_thread_cpu_time() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid, exclusively borrowed `timespec` that outlives
    // the call, and `CLOCK_THREAD_CPUTIME_ID` is a clock id every supported
    // Unix target defines.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    // The error detail is read straight after the call, before anything else
    // can overwrite it.
    check_clock_status(status, &io::Error::last_os_error());
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Unix: `clock_gettime` reports success with 0 and failure with -1 (the OS
/// error is in `errno`). A failure panics; nothing is returned in its place.
#[cfg(unix)]
fn check_clock_status(status: i32, os_error: &io::Error) {
    if status != 0 {
        panic!("clock_gettime(CLOCK_THREAD_CPUTIME_ID) failed: {os_error}");
    }
}

/// Windows: the Win32 thread-times query on the current thread, summing the
/// kernel time and the user time (both in 100 ns units).
#[cfg(windows)]
fn read_os_thread_cpu_time() -> Duration {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

    const ZERO: FILETIME = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut creation, mut exit, mut kernel, mut user) = (ZERO, ZERO, ZERO, ZERO);
    // SAFETY: `GetCurrentThread` returns the calling thread's pseudo handle,
    // which needs no closing, and the four `FILETIME` out-pointers are valid,
    // distinct and outlive the call.
    let status = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    // The error code is read straight after the call, before anything else can
    // overwrite it.
    check_clock_status(status, &io::Error::last_os_error());
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

/// Windows: `GetThreadTimes` reports success with a nonzero value and failure
/// with 0 (the error code is `GetLastError`). A failure panics; nothing is
/// returned in its place.
#[cfg(windows)]
fn check_clock_status(status: i32, os_error: &io::Error) {
    if status == 0 {
        panic!(
            "GetThreadTimes failed: last error code {}",
            os_error.raw_os_error().unwrap_or_default()
        );
    }
}

// ── The CPU budget meter and its judgment (FR2, FR8) ─────────────────────

/// A per-test accumulator of thread CPU time, owned by one thread.
///
/// Every measured call runs on the thread that created the meter; the meter is
/// not shared between threads. The total grows by exactly the CPU time of the
/// calls passed through [`CpuMeter::measure`]: work outside them (building
/// inputs, comparisons, releasing returned values) is not counted.
pub(super) struct CpuMeter {
    owner: ThreadId,
    total: Duration,
}

impl CpuMeter {
    /// A meter owned by the calling thread, with a total of zero.
    pub(super) fn new() -> Self {
        Self {
            owner: std::thread::current().id(),
            total: Duration::ZERO,
        }
    }

    /// Runs `call`, adds the CPU time the calling thread spent in it to the
    /// total and hands back the call's result unchanged. The result leaves the
    /// measured region, so releasing it is not counted.
    pub(super) fn measure<R>(&mut self, call: impl FnOnce() -> R) -> R {
        assert_eq!(
            std::thread::current().id(),
            self.owner,
            "a CpuMeter measures only on the thread that owns it"
        );
        let before = thread_cpu_time();
        let result = call();
        let after = thread_cpu_time();
        self.total += after - before;
        result
    }

    /// The thread CPU time of the calls measured so far.
    pub(super) fn total(&self) -> Duration {
        self.total
    }

    /// The total judged against `budget`.
    pub(super) fn judge(&self, budget: Duration) -> BudgetVerdict {
        judge_budget(self.total, budget)
    }
}

/// The outcome of judging a total against a budget; it carries both so an
/// assertion message can show them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BudgetVerdict {
    pub(super) total: Duration,
    pub(super) budget: Duration,
}

impl BudgetVerdict {
    /// Within exactly when the total is strictly less than the budget.
    pub(super) fn is_within(&self) -> bool {
        self.total < self.budget
    }
}

impl std::fmt::Display for BudgetVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "thread CPU time {:?} against the budget {:?}",
            self.total, self.budget
        )
    }
}

/// Judges `total` against `budget`: within exactly when `total < budget`.
pub(super) fn judge_budget(total: Duration, budget: Duration) -> BudgetVerdict {
    BudgetVerdict { total, budget }
}

// ── The scaling-ratio judgment (mux-strip-concat-budget-overrun task0002, FR3) ──

/// The outcome of judging two thread CPU times measured on the same thread at
/// input sizes N (`smaller`) and 4N (`larger`); it carries both times so an
/// assertion message can show them with their ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ScalingVerdict {
    pub(super) smaller: Duration,
    pub(super) larger: Duration,
}

/// The scaling ratio at and above which a cost is not linear: the geometric mean
/// of the linear ratio 4 and the quadratic ratio 16 for a 4x larger input.
const NON_LINEAR_RATIO: u128 = 8;

impl ScalingVerdict {
    /// Linear exactly when the smaller time is above zero and `larger /
    /// smaller` is strictly below 8. A ratio of exactly 8, a ratio above 8 and
    /// a zero smaller time are not linear.
    ///
    /// The comparison is on whole nanoseconds, `larger < 8 * smaller`, so the
    /// boundary involves no floating-point rounding and a zero smaller time
    /// divides nothing.
    pub(super) fn is_linear(&self) -> bool {
        let smaller = self.smaller.as_nanos();
        smaller > 0 && self.larger.as_nanos() < smaller * NON_LINEAR_RATIO
    }

    /// `larger / smaller`; `None` when the smaller time is zero.
    pub(super) fn ratio(&self) -> Option<f64> {
        (!self.smaller.is_zero()).then(|| self.larger.as_secs_f64() / self.smaller.as_secs_f64())
    }
}

impl std::fmt::Display for ScalingVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "thread CPU time {:?} at N and {:?} at 4N, ",
            self.smaller, self.larger
        )?;
        match self.ratio() {
            Some(ratio) => write!(f, "ratio {ratio:.2} (linear below {NON_LINEAR_RATIO})"),
            None => write!(f, "ratio undefined (the time at N is zero)"),
        }
    }
}

/// Judges the thread CPU time `smaller` measured at N against the time
/// `larger` measured at 4N.
pub(super) fn judge_scaling(smaller: Duration, larger: Duration) -> ScalingVerdict {
    ScalingVerdict { smaller, larger }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    /// CPU work the optimizer cannot remove: `iterations` dependent steps.
    fn burn(iterations: u64) -> u64 {
        let mut acc = 0u64;
        for i in 0..iterations {
            acc = black_box(acc.wrapping_mul(31).wrapping_add(i));
        }
        acc
    }

    /// The smaller of the two work sizes of TS-3 (about 50 ms or more of CPU on
    /// an unloaded debug build).
    const SMALL_WORK: u64 = 20_000_000;
    /// The larger work: several times the smaller.
    const LARGE_WORK: u64 = 4 * SMALL_WORK;

    // TS-1 (AC-2, FR6)
    #[test]
    fn thread_cpu_time_ignores_a_sleep_which_a_wall_clock_reading_includes() {
        let sleep = Duration::from_millis(300);
        let bound = sleep / 2;

        let wall = Instant::now();
        let before = thread_cpu_time();
        std::thread::sleep(sleep);
        let cpu_delta = thread_cpu_time() - before;
        let wall_elapsed = wall.elapsed();

        assert!(
            cpu_delta < bound,
            "the CPU delta {cpu_delta:?} across a {sleep:?} sleep is not below {bound:?}"
        );
        assert!(
            wall_elapsed >= bound,
            "the wall-clock elapsed time {wall_elapsed:?} meets the bound {bound:?}, \
             so a wall-clock helper would pass this case"
        );
    }

    // TS-2 (AC-3, FR6, NFR3)
    #[test]
    fn thread_cpu_time_excludes_the_cpu_time_of_other_threads() {
        const AMOUNT: Duration = Duration::from_millis(200);
        // A defective helper (one that never reaches AMOUNT) cannot hang the
        // test: the spawned thread stops at this wall-clock cap.
        const WALL_CAP: Duration = Duration::from_secs(30);

        let spawned = std::thread::spawn(|| {
            let wall = Instant::now();
            let start = thread_cpu_time();
            loop {
                black_box(burn(100_000));
                let used = thread_cpu_time() - start;
                if used >= AMOUNT || wall.elapsed() >= WALL_CAP {
                    return used;
                }
            }
        });
        let before = thread_cpu_time();
        let consumed = spawned.join().expect("the spawned thread panicked");
        let delta = thread_cpu_time() - before;

        assert!(
            consumed >= AMOUNT,
            "the spawned thread reached {consumed:?} of its own CPU time, not {AMOUNT:?}"
        );
        assert!(
            delta < consumed / 2,
            "the measuring thread's delta {delta:?} while waiting in join is not below \
             half of the spawned thread's {consumed:?}"
        );
    }

    // TS-3 (AC-1, FR3, FR7)
    #[test]
    fn thread_cpu_time_counts_the_calling_threads_own_cpu_work() {
        let wall = Instant::now();
        let before = thread_cpu_time();
        black_box(burn(black_box(SMALL_WORK)));
        let small = thread_cpu_time() - before;
        let small_wall = wall.elapsed();

        let before = thread_cpu_time();
        black_box(burn(black_box(LARGE_WORK)));
        let large = thread_cpu_time() - before;

        assert!(small > Duration::ZERO, "the smaller work's delta is zero");
        assert!(large > Duration::ZERO, "the larger work's delta is zero");
        assert!(
            large > small,
            "the larger work's delta {large:?} does not exceed the smaller's {small:?}"
        );
        assert!(
            small * 100 >= small_wall,
            "the smaller work's delta {small:?} is below 1% of its wall-clock duration \
             {small_wall:?}"
        );
    }

    /// The linear surrogate: one pass over the input.
    fn linear_surrogate(input: &[u8]) -> u64 {
        let mut acc = 0u64;
        for &b in input {
            acc = black_box(acc.wrapping_mul(31).wrapping_add(u64::from(b)));
        }
        acc
    }

    /// The quadratic surrogate: a rescan from the start for every input item.
    fn quadratic_surrogate(input: &[u8]) -> u64 {
        let mut acc = 0u64;
        for end in 0..input.len() {
            for &b in &input[..end] {
                acc = black_box(acc.wrapping_mul(31).wrapping_add(u64::from(b)));
            }
        }
        acc
    }

    // TS-4 (AC-5, FR8, NFR4)
    #[test]
    fn the_budget_judgment_rejects_a_quadratic_surrogate_and_accepts_a_linear_one() {
        // On an unloaded debug build the linear surrogate uses a tiny fraction
        // of the budget and the quadratic one well over five times the budget.
        let budget = Duration::from_millis(100);
        let input: Vec<u8> = (0..30_000u32).map(|i| (i % 251) as u8).collect();

        let mut linear = CpuMeter::new();
        black_box(linear.measure(|| linear_surrogate(black_box(&input))));
        let linear_verdict = linear.judge(budget);

        let mut quadratic = CpuMeter::new();
        black_box(quadratic.measure(|| quadratic_surrogate(black_box(&input))));
        let quadratic_verdict = quadratic.judge(budget);

        assert!(
            linear_verdict.is_within(),
            "the linear surrogate was judged over: {linear_verdict}"
        );
        assert!(
            !quadratic_verdict.is_within(),
            "the quadratic surrogate was judged within: {quadratic_verdict}"
        );
    }

    #[test]
    fn the_judgment_is_within_exactly_when_the_total_is_strictly_below_the_budget() {
        let budget = Duration::from_secs(10);
        assert!(judge_budget(budget - Duration::from_nanos(1), budget).is_within());
        assert!(!judge_budget(budget, budget).is_within());
        assert!(!judge_budget(budget + Duration::from_nanos(1), budget).is_within());
        assert!(judge_budget(Duration::ZERO, budget).is_within());
    }

    #[test]
    fn the_verdict_message_shows_the_total_and_the_budget() {
        let verdict = judge_budget(Duration::from_millis(1234), Duration::from_secs(10));
        let message = verdict.to_string();
        assert!(message.contains("1.234s"), "{message}");
        assert!(message.contains("10s"), "{message}");
    }

    // ── The scaling-ratio judgment (mux-strip-concat-budget-overrun task0002, FR3) ──

    // AC-1 (FR3, NFR4)
    #[test]
    fn the_scaling_judgment_is_linear_exactly_when_the_ratio_is_strictly_below_8() {
        let smaller = Duration::from_nanos(1_000);
        assert!(
            judge_scaling(smaller, Duration::from_nanos(7_999)).is_linear(),
            "a ratio just below 8 was judged not linear"
        );
        assert!(
            !judge_scaling(smaller, Duration::from_nanos(8_000)).is_linear(),
            "a ratio of exactly 8 was judged linear"
        );
        assert!(
            !judge_scaling(smaller, Duration::from_nanos(8_001)).is_linear(),
            "a ratio just above 8 was judged linear"
        );

        // The same boundary at the scale of the real measurements.
        let smaller = Duration::from_millis(100);
        assert!(judge_scaling(smaller, Duration::from_millis(400)).is_linear());
        assert!(judge_scaling(smaller, Duration::from_millis(799)).is_linear());
        assert!(!judge_scaling(smaller, Duration::from_millis(800)).is_linear());
        assert!(!judge_scaling(smaller, Duration::from_millis(1_600)).is_linear());
        // A larger time below the smaller one is a ratio below 8.
        assert!(judge_scaling(smaller, Duration::ZERO).is_linear());
    }

    // AC-1 (FR3, NFR4)
    #[test]
    fn the_scaling_judgment_rejects_a_zero_smaller_time_without_panicking() {
        for larger in [
            Duration::ZERO,
            Duration::from_nanos(1),
            Duration::from_secs(1),
        ] {
            let verdict = judge_scaling(Duration::ZERO, larger);
            assert!(
                !verdict.is_linear(),
                "a zero smaller time with larger {larger:?} was judged linear"
            );
            assert_eq!(verdict.ratio(), None, "larger {larger:?}");
            // Showing the verdict must not panic either.
            let _ = verdict.to_string();
        }
    }

    // AC-1 (FR3, NFR4)
    #[test]
    fn the_scaling_verdict_message_shows_both_times_and_the_ratio() {
        let verdict = judge_scaling(Duration::from_millis(125), Duration::from_millis(500));
        assert_eq!(verdict.smaller, Duration::from_millis(125));
        assert_eq!(verdict.larger, Duration::from_millis(500));
        assert_eq!(verdict.ratio(), Some(4.0));
        let message = verdict.to_string();
        assert!(message.contains("125ms"), "{message}");
        assert!(message.contains("500ms"), "{message}");
        assert!(message.contains("4.00"), "{message}");

        let zero = judge_scaling(Duration::ZERO, Duration::from_millis(1234)).to_string();
        assert!(zero.contains("0ns"), "{zero}");
        assert!(zero.contains("1.234s"), "{zero}");
    }

    /// The passes the linear surrogate of the scaling test makes over its input:
    /// they size the smaller measurement to about 100 ms or more of CPU time on
    /// a debug build without an input of tens of megabytes.
    const LINEAR_PASSES: usize = 50;
    /// The smaller input of the linear surrogate; the larger is four times it.
    const LINEAR_N: usize = 1_000_000;
    /// The smaller input of the quadratic surrogate; the larger is four times
    /// it, so its larger measurement is about 16 times the smaller.
    const QUADRATIC_N: usize = 10_000;

    /// The linear surrogate: `LINEAR_PASSES` passes of the one-pass surrogate.
    fn linear_scaling_surrogate(input: &[u8]) -> u64 {
        (0..LINEAR_PASSES).fold(0u64, |acc, _| {
            acc.wrapping_add(linear_surrogate(black_box(input)))
        })
    }

    fn surrogate_input(len: usize) -> Vec<u8> {
        (0..len as u32).map(|i| (i % 251) as u8).collect()
    }

    /// `surrogate` measured once through a `CpuMeter` on `small` (N) and once on
    /// `large` (4N), both on this thread, then judged.
    fn measured_scaling(surrogate: fn(&[u8]) -> u64, small: &[u8], large: &[u8]) -> ScalingVerdict {
        let mut at_n = CpuMeter::new();
        black_box(at_n.measure(|| surrogate(black_box(small))));
        let mut at_4n = CpuMeter::new();
        black_box(at_4n.measure(|| surrogate(black_box(large))));
        judge_scaling(at_n.total(), at_4n.total())
    }

    // AC-2 (FR3), the TS-4 manner of this module; AC-5 (NFR1, NFR4): the times
    // are written for the sizing read.
    #[test]
    fn the_scaling_judgment_rejects_a_quadratic_surrogate_and_accepts_a_linear_one() {
        let linear = measured_scaling(
            linear_scaling_surrogate,
            &surrogate_input(LINEAR_N),
            &surrogate_input(4 * LINEAR_N),
        );
        let quadratic = measured_scaling(
            quadratic_surrogate,
            &surrogate_input(QUADRATIC_N),
            &surrogate_input(4 * QUADRATIC_N),
        );
        println!("linear surrogate: {linear}");
        println!("quadratic surrogate: {quadratic}");

        assert!(
            linear.is_linear(),
            "the linear surrogate was judged not linear: {linear}"
        );
        assert!(
            quadratic.smaller > Duration::ZERO,
            "the quadratic surrogate's smaller measurement is zero: {quadratic}"
        );
        assert!(
            !quadratic.is_linear(),
            "the quadratic surrogate was judged linear: {quadratic}"
        );
    }

    #[test]
    fn the_meter_counts_only_the_measured_calls_and_returns_their_result_unchanged() {
        let mut meter = CpuMeter::new();
        let before = thread_cpu_time();
        let outside_before = black_box(burn(black_box(LARGE_WORK)));
        let result = meter.measure(|| burn(black_box(SMALL_WORK)));
        let outside_after = black_box(burn(black_box(LARGE_WORK)));
        let whole = thread_cpu_time() - before;

        assert_eq!(
            result,
            burn(SMALL_WORK),
            "the call's result is not handed back"
        );
        black_box((outside_before, outside_after));
        assert!(
            meter.total() > Duration::ZERO,
            "the measured call adds nothing"
        );
        assert!(
            meter.total() * 2 < whole,
            "the total {:?} counts work outside the measured call (the whole region took \
             {whole:?})",
            meter.total()
        );
    }

    #[test]
    fn the_meter_rejects_a_call_from_a_thread_other_than_its_owner() {
        let mut meter = CpuMeter::new();
        let outcome = std::thread::spawn(move || meter.measure(|| ())).join();
        assert!(
            outcome.is_err(),
            "a foreign thread measured through the meter"
        );
    }

    /// The status a failing OS call reports, and an OS error to go with it.
    #[cfg(unix)]
    const FAILURE: (i32, i32) = (-1, 22);
    #[cfg(windows)]
    const FAILURE: (i32, i32) = (0, 87);
    /// The clock API the panic message names.
    #[cfg(unix)]
    const CLOCK_API: &str = "clock_gettime";
    #[cfg(windows)]
    const CLOCK_API: &str = "GetThreadTimes";

    // TS-9 (AC-4, FR4)
    #[test]
    #[cfg_attr(unix, should_panic(expected = "clock_gettime"))]
    #[cfg_attr(windows, should_panic(expected = "GetThreadTimes"))]
    fn a_failure_status_panics_naming_the_clock_api() {
        let (status, code) = FAILURE;
        check_clock_status(status, &io::Error::from_raw_os_error(code));
    }

    #[test]
    fn a_failure_status_panic_message_names_the_clock_api_and_the_os_error() {
        let (status, code) = FAILURE;
        let panic = std::panic::catch_unwind(|| {
            check_clock_status(status, &io::Error::from_raw_os_error(code));
        })
        .expect_err("a failure status did not panic");
        let message = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("the panic payload is not a string");
        assert!(message.contains(CLOCK_API), "{message}");
        assert!(message.contains(&code.to_string()), "{message}");
    }

    #[test]
    fn a_success_status_does_not_panic() {
        #[cfg(unix)]
        let status = 0;
        #[cfg(windows)]
        let status = 1;
        check_clock_status(status, &io::Error::from_raw_os_error(0));
    }
}
