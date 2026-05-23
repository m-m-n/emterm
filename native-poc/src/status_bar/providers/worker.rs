//! Shared worker-thread infrastructure for IO-bound providers
//! (Git branch, custom commands). Each worker owns a thread that
//! sleeps for `interval`, spawns a `std::process::Command`, applies a
//! 5 s timeout, and writes the result into a shared cache. The UI
//! thread reads from the cache without blocking.
//!
//! No external crates. Locking uses `std::sync::{Mutex, Condvar}`;
//! shutdown uses `Arc<AtomicBool>` flipped from the Drop impl.

use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Per-spawn outcome. `Stdout` carries the first line on success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerOutcome {
    /// Process exited within the timeout with status 0. The string
    /// holds the trimmed first line of stdout.
    Stdout(String),
    /// Process exited within the timeout with non-zero status.
    NonZeroExit(i32, String),
    /// Process exceeded the timeout and was killed.
    Timeout,
    /// `std::process::Command::spawn` failed entirely (e.g.
    /// executable not found).
    SpawnError(String),
}

/// Per-spawn outcome that retains the **full** captured stdout instead
/// of just the first line. Used by callers that need to inspect every
/// line of output (e.g. `git status --porcelain` for dirty-file
/// classification).
///
/// Kept separate from [`WorkerOutcome`] so existing callers don't pay
/// the "I might want the whole buffer later" cost on the first-line
/// hot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerOutcomeFull {
    /// Process exited within the timeout with status 0. The string
    /// holds the full captured stdout (no trim).
    Stdout(String),
    /// Process exited within the timeout with non-zero status. The
    /// string holds the full captured stdout (no trim).
    NonZeroExit(i32, String),
    /// Process exceeded the timeout and was killed.
    Timeout,
    /// `std::process::Command::spawn` failed entirely (e.g.
    /// executable not found).
    SpawnError(String),
}

/// Default per-spawn timeout. 5 seconds matches the WebView build.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Run `command` with `args` and `cwd`, capturing stdout. Applies
/// `timeout` and kills the child if it overruns. Stdin is closed
/// (`Stdio::null()`).
///
/// The function blocks the calling thread, so callers must invoke it
/// from a dedicated worker — not the UI thread.
pub fn run_command(
    command: &str,
    args: &[&str],
    cwd: Option<&str>,
    timeout: Duration,
) -> WorkerOutcome {
    match run_command_inner(command, args, cwd, timeout) {
        RawOutcome::Stdout(code, out) => {
            let first = out.lines().next().unwrap_or("").trim().to_string();
            if code == 0 {
                WorkerOutcome::Stdout(first)
            } else {
                WorkerOutcome::NonZeroExit(code, first)
            }
        }
        RawOutcome::Timeout => WorkerOutcome::Timeout,
        RawOutcome::SpawnError(e) => WorkerOutcome::SpawnError(e),
    }
}

/// Like [`run_command`], but returns the **full** captured stdout
/// instead of just the trimmed first line.
///
/// Callers that need every line of output (e.g. `git status
/// --porcelain` for dirty / untracked classification) use this to
/// avoid a second `Command::spawn` of the same external process.
pub fn run_command_full(
    command: &str,
    args: &[&str],
    cwd: Option<&str>,
    timeout: Duration,
) -> WorkerOutcomeFull {
    match run_command_inner(command, args, cwd, timeout) {
        RawOutcome::Stdout(code, out) => {
            if code == 0 {
                WorkerOutcomeFull::Stdout(out)
            } else {
                WorkerOutcomeFull::NonZeroExit(code, out)
            }
        }
        RawOutcome::Timeout => WorkerOutcomeFull::Timeout,
        RawOutcome::SpawnError(e) => WorkerOutcomeFull::SpawnError(e),
    }
}

/// Internal raw spawn result, shared by [`run_command`] (first-line
/// trim) and [`run_command_full`] (full stdout). Centralising the
/// spawn + wait + read code keeps the two public flavours in lock-step
/// (same timeout / stdin-null / stderr-null contract).
enum RawOutcome {
    /// `(exit_code, full_stdout_string)`.
    Stdout(i32, String),
    Timeout,
    SpawnError(String),
}

fn run_command_inner(
    command: &str,
    args: &[&str],
    cwd: Option<&str>,
    timeout: Duration,
) -> RawOutcome {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return RawOutcome::SpawnError(e.to_string()),
    };
    wait_with_timeout(child, timeout)
}

fn wait_with_timeout(mut child: Child, timeout: Duration) -> RawOutcome {
    let start = Instant::now();
    let poll = Duration::from_millis(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    use std::io::Read;
                    let _ = stdout.read_to_string(&mut out);
                }
                let code = status.code().unwrap_or(-1);
                return RawOutcome::Stdout(code, out);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return RawOutcome::Timeout;
                }
                std::thread::sleep(poll);
            }
            Err(e) => {
                return RawOutcome::SpawnError(e.to_string());
            }
        }
    }
}

/// Worker-thread shutdown coordinator. The owner constructs a `Stop`,
/// shares it with the worker, and calls `signal()` on drop to wake
/// the worker out of `Condvar::wait_timeout`.
pub struct Stop {
    flag: Arc<AtomicBool>,
    cv: Arc<(Mutex<()>, Condvar)>,
}

impl Default for Stop {
    fn default() -> Self {
        Self::new()
    }
}

impl Stop {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            cv: Arc::new((Mutex::new(()), Condvar::new())),
        }
    }

    pub fn handle(&self) -> StopHandle {
        StopHandle {
            flag: self.flag.clone(),
            cv: self.cv.clone(),
        }
    }

    /// Set the stop flag and wake the worker.
    pub fn signal(&self) {
        self.flag.store(true, Ordering::Relaxed);
        let (_, cv) = &*self.cv;
        cv.notify_all();
    }
}

#[derive(Clone)]
pub struct StopHandle {
    flag: Arc<AtomicBool>,
    cv: Arc<(Mutex<()>, Condvar)>,
}

impl StopHandle {
    /// `true` once `Stop::signal()` was called.
    pub fn is_stopped(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// Sleep until either `timeout` elapses or `signal()` is called.
    /// Returns `true` when the wait completed by timeout (caller may
    /// proceed); `false` means the stop flag is set.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let (m, cv) = &*self.cv;
        let lock = m.lock().unwrap();
        let (_lock, _wait_res) = cv.wait_timeout(lock, timeout).unwrap();
        !self.is_stopped()
    }
}

/// Run `work` on a worker thread until `Stop` is signalled. The
/// closure receives the stop handle so it can break out of inner
/// loops responsively. Returns a `JoinHandle` that can be joined
/// after `Stop::signal()`.
pub fn spawn<F>(name: &str, stop: StopHandle, work: F) -> JoinHandle<()>
where
    F: FnOnce(StopHandle) + Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || work(stop))
        .expect("failed to spawn worker thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_captures_stdout_first_line() {
        let outcome = run_command(
            "/bin/sh",
            &["-c", "printf 'hello\\nworld'"],
            None,
            DEFAULT_TIMEOUT,
        );
        match outcome {
            WorkerOutcome::Stdout(s) => assert_eq!(s, "hello"),
            other => panic!("expected Stdout, got {other:?}"),
        }
    }

    #[test]
    fn run_command_reports_non_zero_exit() {
        let outcome = run_command("/bin/sh", &["-c", "exit 7"], None, DEFAULT_TIMEOUT);
        match outcome {
            WorkerOutcome::NonZeroExit(code, _) => assert_eq!(code, 7),
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn run_command_times_out_on_long_running_process() {
        let outcome = run_command(
            "/bin/sh",
            &["-c", "sleep 30"],
            None,
            Duration::from_millis(200),
        );
        assert_eq!(outcome, WorkerOutcome::Timeout);
    }

    #[test]
    fn run_command_spawn_error_for_missing_executable() {
        let outcome = run_command("/no/such/binary", &[], None, DEFAULT_TIMEOUT);
        assert!(matches!(outcome, WorkerOutcome::SpawnError(_)));
    }

    #[test]
    fn stop_signal_releases_wait_timeout() {
        let stop = Stop::new();
        let handle = stop.handle();
        let t = std::thread::spawn(move || {
            // Long timeout — should be cut short by `signal`.
            let ok = handle.wait_timeout(Duration::from_secs(60));
            ok
        });
        std::thread::sleep(Duration::from_millis(50));
        stop.signal();
        let result = t.join().unwrap();
        // After signal, is_stopped is true → wait_timeout returns false.
        assert!(!result);
    }

    #[test]
    fn run_command_full_captures_all_lines() {
        let outcome = run_command_full(
            "/bin/sh",
            &["-c", "printf 'hello\\nworld\\n'"],
            None,
            DEFAULT_TIMEOUT,
        );
        match outcome {
            WorkerOutcomeFull::Stdout(s) => {
                assert!(s.contains("hello"), "missing first line: {s:?}");
                assert!(s.contains("world"), "missing second line: {s:?}");
            }
            other => panic!("expected Stdout, got {other:?}"),
        }
    }

    #[test]
    fn run_command_full_reports_non_zero_exit() {
        let outcome = run_command_full(
            "/bin/sh",
            &["-c", "printf 'partial\\n'; exit 3"],
            None,
            DEFAULT_TIMEOUT,
        );
        match outcome {
            WorkerOutcomeFull::NonZeroExit(code, body) => {
                assert_eq!(code, 3);
                assert!(body.contains("partial"));
            }
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn run_command_full_times_out() {
        let outcome = run_command_full(
            "/bin/sh",
            &["-c", "sleep 30"],
            None,
            Duration::from_millis(200),
        );
        assert_eq!(outcome, WorkerOutcomeFull::Timeout);
    }

    #[test]
    fn run_command_full_spawn_error_for_missing_executable() {
        let outcome = run_command_full("/no/such/binary", &[], None, DEFAULT_TIMEOUT);
        assert!(matches!(outcome, WorkerOutcomeFull::SpawnError(_)));
    }

    #[test]
    fn spawn_joins_on_drop_pattern() {
        let stop = Stop::new();
        let handle = stop.handle();
        let join = spawn("test-worker", handle.clone(), |h| {
            // Wake once, then loop until stop.
            while !h.is_stopped() {
                h.wait_timeout(Duration::from_millis(10));
            }
        });
        // Signal and join.
        stop.signal();
        join.join().expect("worker join");
    }
}
