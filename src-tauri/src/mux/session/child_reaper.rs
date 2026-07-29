//! Reaps a mux pane's shell child process off the daemon's async runtime.
//!
//! task0001: the daemon retains a spawned shell's child-process handle for
//! the pane's lifetime and reaps it on every teardown path, so a
//! long-running daemon never accumulates zombie shells and no teardown path
//! ever blocks on a child's exit (SPEC FR5/FR6/FR7, IMPLEMENTATION.md D3).
//!
//! Leaf module (IMPLEMENTATION.md "Layer Structure"): depends only on the
//! `portable_pty` child contract and the logging facade — no dependency
//! back into `pane`/`session` state, so [`reap_child_blocking`] is
//! unit-testable without a PTY or a `MuxPane`. `pane_id` is accepted as a
//! plain `u32` (not `super::pane::PaneId`) to keep that leaf property.

use std::time::{Duration, Instant};

/// Grace period the blocking reap procedure waits for a child to exit on
/// its own before escalating to a kill (task0001 SPEC A2, task plan D3).
///
/// A normally exiting shell (hangup delivered by dropping the PTY master,
/// task plan D2) exits well within this window, so the kill escalation
/// stays exceptional.
pub(super) const DEFAULT_GRACE_PERIOD: Duration = Duration::from_millis(500);

/// Interval at which [`reap_child_blocking`] polls the child's exit status
/// during the grace period (task0001 SPEC A2, task plan D3).
pub(super) const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Blocking reap procedure (task plan D3 item 1; SPEC FR5/FR6/FR7).
///
/// Polls `child`'s non-blocking exit check at `poll_interval` until either
/// the child exits or `grace_period` elapses, then — only if still alive —
/// sends a kill and performs a final blocking reap. Never panics and never
/// propagates an error: every failure path is absorbed here (logged at
/// `warn` where the SPEC error table calls for it) so a caller handing off
/// a child never has to handle a reap failure itself.
///
/// `pane_id` is log context only.
///
/// Postconditions:
/// - A child that exits during the grace period is reaped by that
///   observation; no kill is ever sent for it (AC-4).
/// - A child still alive when the grace period elapses (or whose exit
///   status could not be determined) is killed, then blocking-reaped
///   (AC-5). There is no unbounded wait before the kill: the poll loop's
///   own wait is capped by `grace_period`, and the final `wait()` after a
///   kill returns promptly because the child is now dying/dead.
/// - A kill error (SPEC error table: "child already gone" -> ignore) is
///   ignored; the procedure still proceeds to the final blocking reap
///   (AC-6).
/// - A blocking-reap error, or an exit-poll error, is logged at `warn` and
///   ends the procedure without panicking (AC-6).
pub(super) fn reap_child_blocking(
    pane_id: u32,
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    grace_period: Duration,
    poll_interval: Duration,
) {
    let deadline = Instant::now() + grace_period;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                // Exited on its own — already reaped by this observation.
                return;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(poll_interval);
            }
            Err(e) => {
                // SPEC error table: "cannot determine" -> escalate to kill
                // + wait immediately, without waiting out the rest of the
                // grace period.
                log::warn!(
                    "child_reaper: pane {} failed to poll child exit status; \
                     escalating to kill: {}",
                    pane_id,
                    e
                );
                break;
            }
        }
    }

    // Escalate: the child either wedged past the grace period or its exit
    // status could not be determined. A kill error means the child is
    // already gone — ignored; the procedure still proceeds to reap it.
    let _ = child.kill();

    if let Err(e) = child.wait() {
        log::warn!(
            "child_reaper: pane {} blocking reap failed after kill escalation: {}",
            pane_id,
            e
        );
    }
}

/// Background-handoff entry point (task plan D3 item 2; SPEC FR5).
///
/// Spawns one detached OS thread per pane exit (named for diagnosability)
/// that runs [`reap_child_blocking`] with the default timing constants, and
/// returns immediately — this is what keeps `MuxPane::mark_exited`
/// non-blocking (SPEC NFR1). Per-exit threads (SPEC A1): pane exits occur
/// at human rates, so thread churn is negligible and no new plumbing
/// through pane creation is needed.
///
/// The spawned thread is never joined; if the daemon exits before it
/// finishes, the still-running child re-parents to init, which reaps it
/// (SPEC edge case) — no leak survives daemon exit.
pub(super) fn spawn_reaper(pane_id: u32, child: Box<dyn portable_pty::Child + Send + Sync>) {
    let result = std::thread::Builder::new()
        .name(format!("pane-reap-{pane_id}"))
        .spawn(move || {
            reap_child_blocking(pane_id, child, DEFAULT_GRACE_PERIOD, DEFAULT_POLL_INTERVAL);
        });
    if let Err(e) = result {
        log::warn!(
            "child_reaper: failed to spawn reap thread for pane {}: {}",
            pane_id,
            e
        );
    }
}

// ── Process-id based reaping (task plan task0007, IMPLEMENTATION.md D6) ────
//
// A pane restored from a handoff has no `portable_pty::Child` handle — the
// PTY library's handle cannot be rebuilt after the process image is
// replaced — but the daemon remains the parent of every pane child, so the
// platform's child-status collection still works by process id. This
// section applies the identical grace-then-terminate policy
// `reap_child_blocking` applies for an owned handle, reusing the SAME
// timing constants (AC-6) so the two paths behave identically from the
// outside. Unix only (task plan Design: "Guard the process-id path to
// Unix"); the handle-based path above stays available on all platforms.

/// Non-blocking poll of `pid`'s exit status via `waitpid(..., WNOHANG)`.
///
/// Returns `Ok(true)` when the process has exited (this call collected it)
/// OR was already collected before this call ever ran (`ECHILD` — AC-3: the
/// already-collected case must be detected here and treated as "done", not
/// as an error). Returns `Ok(false)` when the process is still running. Any
/// other `waitpid` failure is returned as an error for the caller to log and
/// escalate.
#[cfg(unix)]
fn try_wait_pid(pid: u32) -> std::io::Result<bool> {
    let mut status: libc::c_int = 0;
    // SAFETY: `WNOHANG` makes this call non-blocking; `&mut status` is a
    // valid local out-parameter for the duration of the call.
    let ret = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
    if ret > 0 {
        Ok(true)
    } else if ret == 0 {
        Ok(false)
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ECHILD) {
            // Already collected (or never our child) — AC-3: not an error.
            Ok(true)
        } else {
            Err(err)
        }
    }
}

/// Send a single signal to `pid`.
#[cfg(unix)]
fn send_signal(pid: u32, signal: libc::c_int) -> std::io::Result<()> {
    // SAFETY: signaling an already-validated process id.
    let ret = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if ret == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Terminate `pid`, applying the same "graceful signal, then escalate"
/// policy the handle-based path gets for free from `portable_pty`'s own
/// `Child::kill()` (which sends `SIGHUP`, waits briefly, then escalates to
/// `SIGKILL`): send `SIGTERM` — a signal a process may catch and ignore
/// (AC-2 covers exactly this case) — poll briefly for exit, and escalate to
/// `SIGKILL` (which cannot be caught or ignored) if the process is still
/// alive after that short window. This guarantees the caller's subsequent
/// blocking reap never hangs forever on a stubborn child.
///
/// Errors from the initial signal (e.g. `ESRCH`: the process is already
/// gone) are returned to the caller, which — mirroring
/// `reap_child_blocking`'s handling of a `ChildKiller::kill()` error —
/// ignores them and proceeds to the final blocking reap regardless (SPEC
/// error table: "child already gone" -> ignore).
#[cfg(unix)]
fn kill_pid(pid: u32, poll_interval: Duration) -> std::io::Result<()> {
    send_signal(pid, libc::SIGTERM)?;

    for _ in 0..5 {
        if matches!(try_wait_pid(pid), Ok(true)) {
            return Ok(());
        }
        std::thread::sleep(poll_interval);
    }

    send_signal(pid, libc::SIGKILL)
}

/// Final blocking reap of `pid` after kill escalation.
#[cfg(unix)]
fn wait_pid_blocking(pid: u32) -> std::io::Result<()> {
    let mut status: libc::c_int = 0;
    // SAFETY: blocking `waitpid` on a pid this process is the parent of.
    let ret = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) };
    if ret == -1 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ECHILD) {
            // Already collected — nothing left to wait for.
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

/// Blocking reap procedure for a pane child referenced by raw process id
/// (task plan Design). Mirrors [`reap_child_blocking`] exactly: polls for
/// exit at `poll_interval` until either the process exits or `grace_period`
/// elapses, then — only if still alive — terminates and performs a final
/// blocking reap. Never panics.
///
/// `pane_id` is log context only.
///
/// Postconditions (mirroring [`reap_child_blocking`]):
/// - A process that exits during the grace period is collected by that
///   observation; no kill is ever sent for it (AC-1).
/// - A process still alive when the grace period elapses (or whose exit
///   status could not be determined) is killed, then blocking-reaped
///   (AC-2).
/// - A process id that has already been collected is detected via `ECHILD`
///   and the procedure returns promptly, without treating that as an error
///   (AC-3).
#[cfg(unix)]
pub(super) fn reap_pid_blocking(
    pane_id: u32,
    pid: u32,
    grace_period: Duration,
    poll_interval: Duration,
) {
    let deadline = Instant::now() + grace_period;
    loop {
        match try_wait_pid(pid) {
            Ok(true) => {
                // Exited (or already collected) — nothing left to do.
                return;
            }
            Ok(false) => {
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(poll_interval);
            }
            Err(e) => {
                log::warn!(
                    "child_reaper: pane {} failed to poll pid {} exit status; \
                     escalating to kill: {}",
                    pane_id,
                    pid,
                    e
                );
                break;
            }
        }
    }

    // Escalate: the process either wedged past the grace period or its exit
    // status could not be determined. A kill error (process already gone)
    // is ignored, mirroring the handle-based path's `let _ = child.kill();`.
    let _ = kill_pid(pid, poll_interval);

    if let Err(e) = wait_pid_blocking(pid) {
        log::warn!(
            "child_reaper: pane {} blocking reap of pid {} failed after kill escalation: {}",
            pane_id,
            pid,
            e
        );
    }
}

/// Background-handoff entry point for the process-id path (mirrors
/// [`spawn_reaper`] exactly): spawns one detached OS thread (named for
/// diagnosability) that runs [`reap_pid_blocking`] with the default timing
/// constants (AC-6: the same [`DEFAULT_GRACE_PERIOD`] /
/// [`DEFAULT_POLL_INTERVAL`] the handle-based path uses) and returns
/// immediately.
///
/// The spawned thread is never joined; if the daemon exits before it
/// finishes, the still-running process re-parents to init, which reaps it —
/// no leak survives daemon exit (mirrors [`spawn_reaper`]'s own doc).
#[cfg(unix)]
pub(super) fn spawn_reaper_pid(pane_id: u32, pid: u32) {
    let result = std::thread::Builder::new()
        .name(format!("pane-reap-{pane_id}"))
        .spawn(move || {
            reap_pid_blocking(pane_id, pid, DEFAULT_GRACE_PERIOD, DEFAULT_POLL_INTERVAL);
        });
    if let Err(e) = result {
        log::warn!(
            "child_reaper: failed to spawn pid-reap thread for pane {}: {}",
            pane_id,
            e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ── Real-child paths (TS-4, TS-5) ─────────────────────────────────────
    //
    // Wrap a standard-library child process in a thin adapter implementing
    // the portable-pty `Child`/`ChildKiller` contract (task plan Test
    // Notes), keeping these tests free of any PTY dependency (FR6).

    struct StdChildAdapter(std::process::Child);

    impl std::fmt::Debug for StdChildAdapter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("StdChildAdapter")
                .field("pid", &self.0.id())
                .finish()
        }
    }

    impl portable_pty::ChildKiller for StdChildAdapter {
        fn kill(&mut self) -> std::io::Result<()> {
            self.0.kill()
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            unimplemented!("not exercised by these tests")
        }
    }

    impl portable_pty::Child for StdChildAdapter {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            self.0
                .try_wait()
                .map(|opt| opt.map(|s| portable_pty::ExitStatus::with_exit_code(exit_code(&s))))
        }

        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            self.0
                .wait()
                .map(|s| portable_pty::ExitStatus::with_exit_code(exit_code(&s)))
        }

        fn process_id(&self) -> Option<u32> {
            Some(self.0.id())
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    fn exit_code(status: &std::process::ExitStatus) -> u32 {
        status.code().map(|c| c as u32).unwrap_or(1)
    }

    #[cfg(unix)]
    fn spawn_std_child(program: &str, args: &[&str]) -> std::process::Child {
        std::process::Command::new(program)
            .args(args)
            .spawn()
            .expect("failed to spawn test child process")
    }

    /// Poll `/proc/<pid>` until the pid is gone entirely (the outcome once
    /// `reap_child_blocking` has actually called `wait()` on it).
    #[cfg(unix)]
    fn assert_pid_reaped(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
            if Instant::now() >= deadline {
                panic!("pid {pid} should have been reaped, but /proc/{pid} still exists");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// AC-4 (TS-4): a child that exits promptly is reaped without a kill
    /// ever being sent — the poll loop observes the exit and returns well
    /// before the (deliberately generous) grace period would elapse.
    #[cfg(unix)]
    #[test]
    fn prompt_exit_is_reaped_without_kill() {
        let child = spawn_std_child("true", &[]);
        let pid = child.id();
        let adapter: Box<dyn portable_pty::Child + Send + Sync> = Box::new(StdChildAdapter(child));

        let started = Instant::now();
        reap_child_blocking(
            1,
            adapter,
            Duration::from_millis(500),
            Duration::from_millis(10),
        );
        assert!(
            started.elapsed() < Duration::from_millis(450),
            "prompt exit should not wait out the grace period"
        );
        assert_pid_reaped(pid);
    }

    /// AC-5 (TS-5): a child still alive when the grace period elapses is
    /// killed and then reaped; the procedure returns promptly — bounded by
    /// the shrunken grace period, not by the child's own (much longer)
    /// sleep duration.
    #[cfg(unix)]
    #[test]
    fn wedged_child_is_killed_and_reaped_within_deadline() {
        let child = spawn_std_child("sleep", &["30"]);
        let pid = child.id();
        let adapter: Box<dyn portable_pty::Child + Send + Sync> = Box::new(StdChildAdapter(child));

        let grace_period = Duration::from_millis(50);
        let started = Instant::now();
        reap_child_blocking(2, adapter, grace_period, Duration::from_millis(10));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "kill escalation must return promptly, not wait for the child's own sleep"
        );
        assert_pid_reaped(pid);
    }

    // ── Error paths (TS-6, TS-11): scripted double, no real process ───────

    #[derive(Debug, Clone, Copy)]
    enum TryWaitBehavior {
        /// Every `try_wait()` call returns `Err` (SPEC "cannot determine").
        AlwaysErr,
        /// Every `try_wait()` call returns `Ok(None)` (child never observed
        /// exiting on its own — the grace period always elapses).
        NeverExits,
    }

    /// A scripted double implementing the `portable_pty` child + killer
    /// contract with preprogrammed results (task plan Test Notes): no real
    /// process is involved.
    #[derive(Debug)]
    struct ScriptedChild {
        try_wait_behavior: TryWaitBehavior,
        kill_ok: bool,
        wait_ok: bool,
        try_wait_calls: Arc<AtomicU32>,
        kill_calls: Arc<AtomicU32>,
        wait_calls: Arc<AtomicU32>,
    }

    impl portable_pty::ChildKiller for ScriptedChild {
        fn kill(&mut self) -> std::io::Result<()> {
            self.kill_calls.fetch_add(1, Ordering::SeqCst);
            if self.kill_ok {
                Ok(())
            } else {
                Err(std::io::Error::other("scripted kill failure"))
            }
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            unimplemented!("not exercised by these tests")
        }
    }

    impl portable_pty::Child for ScriptedChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            self.try_wait_calls.fetch_add(1, Ordering::SeqCst);
            match self.try_wait_behavior {
                TryWaitBehavior::AlwaysErr => {
                    Err(std::io::Error::other("scripted try_wait failure"))
                }
                TryWaitBehavior::NeverExits => Ok(None),
            }
        }

        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            self.wait_calls.fetch_add(1, Ordering::SeqCst);
            if self.wait_ok {
                Ok(portable_pty::ExitStatus::with_exit_code(0))
            } else {
                Err(std::io::Error::other("scripted wait failure"))
            }
        }

        fn process_id(&self) -> Option<u32> {
            None
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    /// AC-6 (TS-6, first half): an exit-poll error escalates IMMEDIATELY —
    /// on the very first `try_wait()` call, without retrying or waiting out
    /// the (deliberately long, 5s) grace period — and the procedure returns
    /// without panicking.
    ///
    /// The SPEC error table additionally requires this path to log at
    /// `warn`; verified by inspection of the `log::warn!` call site in
    /// `reap_child_blocking`'s `try_wait` `Err(e)` arm, since capturing
    /// `env_logger` output inside this test harness is impractical (task
    /// plan Test Notes).
    #[test]
    fn exit_poll_error_escalates_immediately_and_does_not_panic() {
        let try_wait_calls = Arc::new(AtomicU32::new(0));
        let kill_calls = Arc::new(AtomicU32::new(0));
        let wait_calls = Arc::new(AtomicU32::new(0));
        let child = ScriptedChild {
            try_wait_behavior: TryWaitBehavior::AlwaysErr,
            kill_ok: true,
            wait_ok: true,
            try_wait_calls: try_wait_calls.clone(),
            kill_calls: kill_calls.clone(),
            wait_calls: wait_calls.clone(),
        };
        let boxed: Box<dyn portable_pty::Child + Send + Sync> = Box::new(child);

        let started = Instant::now();
        reap_child_blocking(3, boxed, Duration::from_secs(5), Duration::from_millis(10));

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "an exit-poll error must escalate immediately, not wait out the grace period"
        );
        assert_eq!(
            try_wait_calls.load(Ordering::SeqCst),
            1,
            "must escalate on the FIRST poll error, not retry"
        );
        assert_eq!(kill_calls.load(Ordering::SeqCst), 1);
        assert_eq!(wait_calls.load(Ordering::SeqCst), 1);
    }

    /// AC-6 (TS-6, second half): a blocking-reap (`wait()`) error after the
    /// kill escalation is absorbed — the procedure returns without
    /// panicking. Logged at `warn` per the `Err(e)` arm after `child.wait()`
    /// in `reap_child_blocking` — see the note on the sibling test above for
    /// why this is verified by inspection rather than captured output.
    #[test]
    fn blocking_reap_error_after_grace_period_is_absorbed_without_panicking() {
        let try_wait_calls = Arc::new(AtomicU32::new(0));
        let kill_calls = Arc::new(AtomicU32::new(0));
        let wait_calls = Arc::new(AtomicU32::new(0));
        let child = ScriptedChild {
            try_wait_behavior: TryWaitBehavior::NeverExits,
            kill_ok: true,
            wait_ok: false,
            try_wait_calls: try_wait_calls.clone(),
            kill_calls: kill_calls.clone(),
            wait_calls: wait_calls.clone(),
        };
        let boxed: Box<dyn portable_pty::Child + Send + Sync> = Box::new(child);

        // Returning at all (not panicking) is the primary assertion here.
        reap_child_blocking(
            4,
            boxed,
            Duration::from_millis(30),
            Duration::from_millis(5),
        );

        assert_eq!(kill_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            wait_calls.load(Ordering::SeqCst),
            1,
            "blocking reap must still run after the grace period elapses"
        );
    }

    /// AC-6 (TS-11): a `kill()` error (SPEC: "child already gone") is
    /// ignored — the procedure still proceeds to the blocking reap rather
    /// than aborting.
    #[test]
    fn kill_error_is_ignored_and_the_procedure_still_reaps() {
        let try_wait_calls = Arc::new(AtomicU32::new(0));
        let kill_calls = Arc::new(AtomicU32::new(0));
        let wait_calls = Arc::new(AtomicU32::new(0));
        let child = ScriptedChild {
            try_wait_behavior: TryWaitBehavior::NeverExits,
            kill_ok: false,
            wait_ok: true,
            try_wait_calls: try_wait_calls.clone(),
            kill_calls: kill_calls.clone(),
            wait_calls: wait_calls.clone(),
        };
        let boxed: Box<dyn portable_pty::Child + Send + Sync> = Box::new(child);

        reap_child_blocking(
            5,
            boxed,
            Duration::from_millis(30),
            Duration::from_millis(5),
        );

        assert_eq!(kill_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            wait_calls.load(Ordering::SeqCst),
            1,
            "AC-6: a kill error must not abort the procedure — the blocking reap still runs"
        );
    }

    // ── Process-id based paths (task plan task0007) ───────────────────────

    /// AC-1: a process that exits on its own is collected within the grace
    /// period, without ever sending a signal.
    #[cfg(unix)]
    #[test]
    fn process_id_prompt_exit_is_collected_within_grace_period() {
        let child = spawn_std_child("true", &[]);
        let pid = child.id();

        let started = Instant::now();
        reap_pid_blocking(
            101,
            pid,
            Duration::from_millis(500),
            Duration::from_millis(10),
        );
        assert!(
            started.elapsed() < Duration::from_millis(450),
            "prompt exit should not wait out the grace period"
        );
        assert_pid_reaped(pid);
        // Do not call `child.wait()` — this process id was already reaped
        // by `reap_pid_blocking` above via raw `waitpid`; the `Child` value
        // is simply dropped without waiting, mirroring the real-child tests
        // above.
        drop(child);
    }

    /// AC-2: a process that ignores the graceful signal (`SIGTERM`) is
    /// still terminated (escalated to `SIGKILL`) and collected once the
    /// grace period elapses.
    #[cfg(unix)]
    #[test]
    fn process_id_child_ignoring_sigterm_is_killed_and_reaped_within_deadline() {
        let child = spawn_std_child("sh", &["-c", "trap '' TERM; sleep 30"]);
        let pid = child.id();

        let grace_period = Duration::from_millis(50);
        let started = Instant::now();
        reap_pid_blocking(102, pid, grace_period, Duration::from_millis(10));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "kill escalation must return promptly, not wait for the child's own sleep, \
             even when it ignores SIGTERM"
        );
        assert_pid_reaped(pid);
        drop(child);
    }

    /// AC-3: a process id that has already been fully collected is detected
    /// promptly (via `ECHILD`) and does not wait out a (deliberately long)
    /// grace period, and is not treated as an error.
    #[cfg(unix)]
    #[test]
    fn process_id_already_collected_returns_promptly_without_error() {
        let child = spawn_std_child("true", &[]);
        let pid = child.id();

        // Reap it fully once through our own path, so the kernel has no
        // more status left to deliver for this pid.
        reap_pid_blocking(
            103,
            pid,
            Duration::from_millis(500),
            Duration::from_millis(10),
        );
        assert_pid_reaped(pid);
        drop(child);

        // AC-3: calling again for the SAME (now fully collected) pid must
        // return promptly, even though the grace period configured here is
        // deliberately long — proving the ECHILD path is taken immediately
        // rather than waiting out the grace period.
        let started = Instant::now();
        reap_pid_blocking(103, pid, Duration::from_secs(5), Duration::from_millis(10));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "an already-collected pid must not wait out the grace period"
        );
    }

    /// AC-6: `spawn_reaper_pid` — the process-id path's public entry point,
    /// exactly mirroring `spawn_reaper` — takes no timing parameters of its
    /// own, so it can only ever reap through `DEFAULT_GRACE_PERIOD` /
    /// `DEFAULT_POLL_INTERVAL`, the SAME constants the handle-based path
    /// uses. Exercises the real end-to-end entry point (background thread
    /// spawn + reap) rather than the lower-level `reap_pid_blocking` the
    /// other tests above call directly.
    #[cfg(unix)]
    #[test]
    fn spawn_reaper_pid_reaps_a_prompt_exiting_process_using_default_timing() {
        let child = spawn_std_child("true", &[]);
        let pid = child.id();

        spawn_reaper_pid(999, pid);

        assert_pid_reaped(pid);
        drop(child);
    }
}
