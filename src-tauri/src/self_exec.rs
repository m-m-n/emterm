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
}
