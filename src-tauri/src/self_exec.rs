//! Self-binary mismatch detection and shared self-spawn helper.
//!
//! eMterm launches its settings panel, viewers, and mux daemon by
//! re-executing itself via `std::env::current_exe()`. When the on-disk
//! binary is replaced (e.g. `apt`/`dpkg` `rename(2)`), the running process
//! keeps the old, now-unlinked inode and `current_exe()` starts returning
//! `/usr/bin/emterm (deleted)`, so child `spawn()` fails with `ENOENT`.
//!
//! This module detects that mismatch **reactively** — only when a self-spawn
//! fails — and raises a process-global `RESTART_REQUIRED` flag the App
//! consumes once per frame to show a restart toast. Linux only.
//!
//! ## Resolution vs detection (critical)
//!
//! Spawning and detection use **different** executable references on purpose:
//!
//! - [`self_exe_path`] (used by every spawn site) resolves `current_exe()`
//!   fresh on each call — exactly as the four sites do today. After a
//!   `rename(2)` replacement the running process's `current_exe()` returns
//!   `…/emterm (deleted)`, so the spawn still fails with `ENOENT`. That
//!   failure is the reactive trigger.
//! - [`self_binary_missing`] (detection) instead `stat`s the **startup
//!   baseline path** captured at [`init`] and compares its current
//!   `(device, inode)` to the recorded identity.
//!
//! If spawning used the baseline clean path instead, a replaced binary would
//! spawn the *new* version successfully — the spawn would not fail and the
//! reactive toast would not fire. Hence the deliberate split.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Identity of the executable captured at startup.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
struct SelfExeId {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

/// One-time startup baseline. `Some(None)` means "init ran but `current_exe()`
/// failed" (detection disabled); `None` means init has not run yet.
static SELF_EXE: OnceLock<Option<SelfExeId>> = OnceLock::new();

/// Set when a self-spawn fails AND the running binary no longer matches the
/// on-disk binary. Drained by the App each frame via [`restart_required`].
static RESTART_REQUIRED: AtomicBool = AtomicBool::new(false);

/// Capture the self-binary baseline once at GUI startup.
///
/// Records `(path, device, inode)` of the executable. If `current_exe()` or
/// the metadata read fails, records "no baseline" so detection stays disabled
/// (no false toast). Idempotent: later calls are ignored.
pub fn init() {
    let _ = SELF_EXE.set(capture_baseline());
}

/// Resolve `current_exe()` and read its `(device, inode)`. Returns `None`
/// when resolution or the metadata read fails (→ detection disabled).
#[cfg(target_os = "linux")]
fn capture_baseline() -> Option<SelfExeId> {
    use std::os::unix::fs::MetadataExt as _;
    let path = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(&path).ok()?;
    Some(SelfExeId {
        path,
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// Non-Linux: detection is a no-op, so the baseline is always absent.
#[cfg(not(target_os = "linux"))]
fn capture_baseline() -> Option<SelfExeId> {
    None
}

/// Pure mismatch predicate (no globals, no filesystem).
///
/// `current` is the `(dev, ino)` read of the baseline path, or `None` when the
/// path can no longer be `stat`-ed (ENOENT). Returns `true` when the current
/// read is absent OR differs from the baseline.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_missing(baseline: &SelfExeId, current: Option<(u64, u64)>) -> bool {
    match current {
        None => true,
        Some((dev, ino)) => (dev, ino) != (baseline.dev, baseline.ino),
    }
}

/// Read the baseline path's current `(device, inode)`.
///
/// Propagates the `stat` error so the caller can distinguish "path is gone"
/// (`NotFound` → mismatch) from a transient / permission error (→ fall back to
/// not-missing, per NFR2).
#[cfg(target_os = "linux")]
fn read_current_dev_ino(baseline: &SelfExeId) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::metadata(&baseline.path)?;
    Ok((meta.dev(), meta.ino()))
}

/// Inode-comparison detection.
///
/// Returns `false` when no baseline was established. On non-Linux targets this
/// is a compile-time no-op returning `false`. Otherwise compares the baseline
/// path's current `(device, inode)` to the recorded identity: a differing
/// inode or a `NotFound` `stat` → `true`; any other `stat` error falls back to
/// `false` (NFR2), so a flaky / permission-denied `stat` never raises a false
/// restart toast.
pub fn self_binary_missing() -> bool {
    let Some(Some(baseline)) = SELF_EXE.get() else {
        // No baseline (init never ran, or `current_exe()` failed at init).
        return false;
    };
    #[cfg(target_os = "linux")]
    {
        match read_current_dev_ino(baseline) {
            Ok(cur) => is_missing(baseline, Some(cur)),
            // Path gone (uninstalled / replaced-and-deleted) → mismatch.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => is_missing(baseline, None),
            // Any other stat error is not a confirmed replacement.
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = baseline;
        false
    }
}

/// Resolver used by spawn sites. Resolves via `current_exe()` fresh on each
/// call — identical to today's per-site behavior. It does NOT return the
/// startup baseline path (see the module-level "Resolution vs detection").
pub fn self_exe_path() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

/// Called by spawn sites after a failed self-spawn. If [`self_binary_missing`]
/// reports a mismatch, sets `RESTART_REQUIRED` and wakes the winit event loop
/// so the App paints a frame and shows the toast — even when the failure
/// originated off the App thread (e.g. the image-viewer worker).
pub fn note_spawn_failure() {
    if self_binary_missing() {
        RESTART_REQUIRED.store(true, Ordering::Release);
        crate::wakeup::wake();
    }
}

/// The App reads this each frame to arm the toast. Swap-resets the flag so a
/// single failure arms the toast exactly once.
pub fn restart_required() -> bool {
    RESTART_REQUIRED.swap(false, Ordering::AcqRel)
}

/// Non-consuming peek at the same flag [`restart_required`] drains.
///
/// Returns the current value and leaves it unchanged — any number of
/// consecutive peeks observe the same result. Feeds the App's pending-work
/// predicate (frame-skip-pending-work task0001), which needs to know a
/// restart is about to be requested without stealing the one-shot consume
/// [`restart_required`] performs to arm the toast exactly once.
///
/// Target-neutral like [`RESTART_REQUIRED`] itself: only the setters
/// ([`note_spawn_failure`]) are Linux-gated, so this needs no new target
/// gating — on non-Linux targets the flag is simply never raised.
pub fn restart_pending() -> bool {
    RESTART_REQUIRED.load(Ordering::Acquire)
}

/// Test-only exclusivity seam over the process-global restart flag
/// (frame-skip-pending-work task0001, AC-2). `RESTART_REQUIRED` is shared by
/// every test in the `--lib` binary, which cargo runs at default (multi-
/// threaded) parallelism — a bare setter let two concurrently scheduled
/// tests race on the same flag (see
/// `feature-docs/flaky-frame-work-pending-test/DIAGNOSIS.md`). Every test
/// that raises, clears, consumes, or observes `RESTART_REQUIRED` — directly
/// or through [`restart_pending`] / [`restart_required`] / any App method
/// that calls them — must hold a [`RestartFlagTestGuard`] for the full span
/// between its own first touch and its last observation.
#[cfg(test)]
static RESTART_FLAG_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard granting exclusive access to `RESTART_REQUIRED` for one
/// test's span. See [`RestartFlagTestGuard::acquire`].
#[cfg(test)]
pub(crate) struct RestartFlagTestGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl RestartFlagTestGuard {
    /// Acquire the seam for the caller's test span.
    ///
    /// **Pre**: none — this is the entry point that establishes exclusivity.
    /// **Post**: the flag reads `false` immediately after this call, so a
    /// span never inherits a value some other party left behind, and the
    /// caller may then raise it explicitly via [`Self::set`].
    ///
    /// Recovers from a poisoned lock (`unwrap_or_else` on the `LockResult`)
    /// so a panic inside one test's span can never cascade into every later
    /// test failing to acquire the seam — the Shared Components contract's
    /// failure-isolation clause.
    pub(crate) fn acquire() -> Self {
        let lock = RESTART_FLAG_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        RESTART_REQUIRED.store(false, Ordering::Release);
        Self { _lock: lock }
    }

    /// Acquire the seam without resetting the flag.
    ///
    /// Same exclusivity and poison-recovery behavior as [`Self::acquire`],
    /// but does not clear `RESTART_REQUIRED` on entry. Lets a caller open a
    /// second exclusive span immediately after another span's guard has
    /// dropped, and observe under the lock whatever that drop left behind —
    /// without the reset masking the very value being verified.
    pub(crate) fn acquire_preserving_flag() -> Self {
        let lock = RESTART_FLAG_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self { _lock: lock }
    }

    /// Raise or clear the flag within this exclusive span.
    pub(crate) fn set(&self, value: bool) {
        RESTART_REQUIRED.store(value, Ordering::Release);
    }
}

/// **Post** (span end): the flag is always returned to `false` here,
/// regardless of what the span left it at — including when the span ends
/// via an unwinding panic, since `Drop` still runs during unwind. No party
/// outside this span can observe a value the span did not itself establish.
#[cfg(test)]
impl Drop for RestartFlagTestGuard {
    fn drop(&mut self) {
        RESTART_REQUIRED.store(false, Ordering::Release);
    }
}

/// Build `Command::new(self_exe_path()?)`, let the caller configure it, spawn,
/// and on `Err` call [`note_spawn_failure`] before returning the error.
pub fn spawn_self(
    configure: impl FnOnce(&mut std::process::Command),
) -> std::io::Result<std::process::Child> {
    let exe = self_exe_path()?;
    let mut cmd = std::process::Command::new(&exe);
    configure(&mut cmd);
    match cmd.spawn() {
        Ok(child) => Ok(child),
        Err(e) => {
            note_spawn_failure();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline(dev: u64, ino: u64) -> SelfExeId {
        SelfExeId {
            path: PathBuf::from("/usr/bin/emterm"),
            dev,
            ino,
        }
    }

    // TS-1: baseline (device, inode) equals current read → false.
    #[test]
    fn is_missing_false_when_dev_ino_match() {
        let b = baseline(7, 42);
        assert!(!is_missing(&b, Some((7, 42))));
    }

    // TS-2: current inode differs from baseline → true.
    #[test]
    fn is_missing_true_when_inode_differs() {
        let b = baseline(7, 42);
        assert!(is_missing(&b, Some((7, 99))));
        // device differs too → true.
        assert!(is_missing(&b, Some((8, 42))));
    }

    // TS-3: current read absent (path gone / ENOENT) → true.
    #[test]
    fn is_missing_true_when_current_absent() {
        let b = baseline(7, 42);
        assert!(is_missing(&b, None));
    }

    // TS-4: no baseline recorded → detection disabled (false).
    //
    // `SELF_EXE` is a process-global `OnceLock`. In the unit-test process
    // `init()` is never called, so `SELF_EXE.get()` is `None` and
    // `self_binary_missing()` must report `false` (detection disabled).
    #[test]
    fn self_binary_missing_false_without_baseline() {
        assert!(
            SELF_EXE.get().is_none(),
            "test process must not have an initialized baseline"
        );
        assert!(!self_binary_missing());
    }

    // ── frame-skip-pending-work task0001, AC-1: restart_pending() peeks
    // without clearing; restart_required() keeps its swap-reset consume
    // semantics unchanged (TS-4, TS-5). ──

    // TS-4: two consecutive peeks after raising the flag both report true —
    // a peek must never consume.
    #[test]
    fn restart_pending_reports_true_across_consecutive_peeks() {
        let guard = RestartFlagTestGuard::acquire();
        guard.set(true);
        assert!(restart_pending(), "first peek must observe the raised flag");
        assert!(
            restart_pending(),
            "a second consecutive peek must observe the same value, unconsumed"
        );
        guard.set(false);
        assert!(!restart_pending(), "flag must be clear after restoration");
    }

    // TS-5: after any number of peeks, restart_required() still arms the
    // toast exactly once — true on the first read after a failure, false on
    // every subsequent read.
    #[test]
    fn restart_required_still_consumes_once_after_peeks() {
        let guard = RestartFlagTestGuard::acquire();
        guard.set(true);
        assert!(restart_pending());
        assert!(restart_pending(), "peeking again must not consume");
        assert!(
            restart_required(),
            "restart_required() must report true exactly once after a raise"
        );
        assert!(
            !restart_required(),
            "a second restart_required() call must report false — consumed"
        );
        assert!(
            !restart_pending(),
            "the flag must read clear after being consumed"
        );
    }

    // ── frame-skip-pending-work task0001, AC-6: regression guard for the
    // exclusivity seam itself. These assert the seam's stated contract
    // (Shared Components, IMPLEMENTATION.md) rather than the restart-flag
    // semantics above — they fail to compile/pass if `RestartFlagTestGuard`
    // is reverted to the old bare `test_set_restart_required` setter. No
    // sleep, no wall-clock threshold, no thread-interleaving assumption. ──

    // The flag must be clear once a span's guard has dropped, regardless of
    // what the span itself left it at — the seam's postcondition.
    #[test]
    fn restart_flag_test_guard_clears_the_flag_when_the_span_ends() {
        {
            let guard = RestartFlagTestGuard::acquire();
            guard.set(true);
            assert!(restart_pending(), "sanity: the span did raise the flag");
        }
        // Guard dropped — span ended without an explicit `set(false)`. Open
        // a second exclusive span (without resetting) so the observation
        // below is itself made under the lock, never outside any span.
        let _guard = RestartFlagTestGuard::acquire_preserving_flag();
        assert!(
            !restart_pending(),
            "the flag must read clear once the exclusive span has ended"
        );
    }

    // A panic inside one span must not poison the seam for every later
    // span, and must not leave the flag stuck raised — failure isolation.
    #[test]
    fn restart_flag_test_guard_stays_usable_after_a_panicking_span() {
        let panicked = std::panic::catch_unwind(|| {
            let guard = RestartFlagTestGuard::acquire();
            guard.set(true);
            panic!("simulated panic inside an exclusive span");
        });
        assert!(
            panicked.is_err(),
            "the simulated panic must have propagated"
        );

        // A poisoned std::sync::Mutex would make every later `.lock()` call
        // return `Err` without this recovery; acquiring here must succeed
        // (not hang, not panic). Use the non-resetting variant so the read
        // below observes what the aborted span actually left behind,
        // instead of `acquire()`'s own reset making the assertion vacuous.
        let guard = RestartFlagTestGuard::acquire_preserving_flag();
        assert!(
            !restart_pending(),
            "the flag must not be stuck raised after a panicking span"
        );
        drop(guard);

        // `PoisonError::into_inner` does not clear the mutex's poison, so
        // the plain entry point every other test uses still traverses its
        // own recovery path here: acquiring must succeed, not panic.
        let reacquired = RestartFlagTestGuard::acquire();
        assert!(
            !restart_pending(),
            "acquire() must remain usable after a panicking span"
        );
        drop(reacquired);
    }
}
