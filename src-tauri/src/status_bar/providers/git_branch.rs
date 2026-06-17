//! `{git_branch}` provider.
//!
//! A worker thread polls `git rev-parse --abbrev-ref HEAD` and
//! `git status --porcelain` against the active tab's cwd at a
//! configurable interval (default 5 s). The result is cached and
//! exposed through the `VariableProvider` trait.
//!
//! Color hints:
//! - `clean`     → `#4caf50`  (porcelain output is empty)
//! - `dirty`     → `#f9a825`  (porcelain has tracked changes)
//! - `untracked` → `#9e9e9e`  (porcelain has only `??` lines)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::html::CssColor;
use crate::status_bar::providers::worker::{
    run_command, run_command_full, spawn, Stop, StopHandle, WorkerOutcome, WorkerOutcomeFull,
    DEFAULT_TIMEOUT,
};
use crate::status_bar::providers::CwdSource;
use crate::status_bar::template_engine::VariableProvider;
use crate::wakeup::WakeFn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchStatus {
    Clean,
    Dirty,
    Untracked,
}

impl BranchStatus {
    fn to_color(&self) -> CssColor {
        let (r, g, b) = match self {
            BranchStatus::Clean => (0x4c, 0xaf, 0x50),
            BranchStatus::Dirty => (0xf9, 0xa8, 0x25),
            BranchStatus::Untracked => (0x9e, 0x9e, 0x9e),
        };
        CssColor::Hex { r, g, b }
    }
}

#[derive(Debug, Default, Clone)]
pub struct GitCache {
    pub branch: Option<String>,
    pub status: Option<BranchStatus>,
}

pub struct GitBranchProvider {
    cache: Arc<Mutex<GitCache>>,
    version: Arc<AtomicU64>,
    stop: Stop,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl GitBranchProvider {
    /// Construct + start the worker without a wake callback. The
    /// background thread still runs but `Drop`-only join semantics
    /// are unchanged. Retained for unit tests that don't exercise
    /// the wake fan-out.
    #[cfg(test)]
    pub fn start(cwd_source: CwdSource, interval: Duration) -> Self {
        Self::start_with_wake(cwd_source, interval, Arc::new(|| ()))
    }

    /// Construct + start the worker. `cwd_source` returns the current
    /// active tab's cwd; the worker re-reads it every cycle. `wake`
    /// is invoked whenever a refresh updates the branch / status
    /// cache so the winit event loop schedules the next frame.
    pub fn start_with_wake(cwd_source: CwdSource, interval: Duration, wake: WakeFn) -> Self {
        let cache: Arc<Mutex<GitCache>> = Arc::new(Mutex::new(GitCache::default()));
        let version = Arc::new(AtomicU64::new(0));
        let stop = Stop::new();
        let handle = stop.handle();
        let cache_clone = cache.clone();
        let version_clone = version.clone();
        let interval = interval.max(Duration::from_secs(1));
        let join = spawn("git-branch-worker", handle.clone(), move |h| {
            git_branch_loop(h, cwd_source, interval, cache_clone, version_clone, wake);
        });
        Self {
            cache,
            version,
            stop,
            join: Mutex::new(Some(join)),
        }
    }

    pub fn snapshot(&self) -> GitCache {
        self.cache.lock().unwrap().clone()
    }
}

impl Drop for GitBranchProvider {
    fn drop(&mut self) {
        self.stop.signal();
        if let Some(join) = self.join.lock().unwrap().take() {
            let _ = join.join();
        }
    }
}

impl VariableProvider for GitBranchProvider {
    fn name(&self) -> &str {
        "git_branch"
    }

    fn get_value(&self, _argument: Option<&str>) -> String {
        self.cache
            .lock()
            .unwrap()
            .branch
            .clone()
            .unwrap_or_default()
    }

    fn get_color(&self, _argument: Option<&str>) -> Option<CssColor> {
        self.cache
            .lock()
            .unwrap()
            .status
            .as_ref()
            .map(|s| s.to_color())
    }

    fn version(&self, _argument: Option<&str>) -> u64 {
        self.version.load(Ordering::Relaxed)
    }
}

fn git_branch_loop(
    stop: StopHandle,
    cwd_source: CwdSource,
    interval: Duration,
    cache: Arc<Mutex<GitCache>>,
    version: Arc<AtomicU64>,
    wake: WakeFn,
) {
    // Run once immediately so the first frame can render the branch.
    while !stop.is_stopped() {
        tick(&cwd_source, &cache, &version, &wake);
        // Sleep until interval expires or stop fires.
        if !stop.wait_timeout(interval) {
            break;
        }
    }
}

fn tick(
    cwd_source: &CwdSource,
    cache: &Arc<Mutex<GitCache>>,
    version: &Arc<AtomicU64>,
    wake: &WakeFn,
) {
    let cwd = match cwd_source() {
        Some(c) if !c.trim().is_empty() => c,
        _ => {
            // No cwd → clear cache.
            clear_cache(cache, version, wake);
            return;
        }
    };
    let branch = match run_command(
        "git",
        &["rev-parse", "--abbrev-ref", "HEAD"],
        Some(&cwd),
        DEFAULT_TIMEOUT,
    ) {
        WorkerOutcome::Stdout(s) if !s.is_empty() && !s.starts_with("fatal:") => s,
        WorkerOutcome::Stdout(_) | WorkerOutcome::NonZeroExit(_, _) => {
            clear_cache(cache, version, wake);
            return;
        }
        WorkerOutcome::Timeout | WorkerOutcome::SpawnError(_) => {
            // Keep previous cached value.
            return;
        }
    };
    // One `git status --porcelain` spawn per tick. We need the full
    // stdout (every line is a tracked / untracked marker), so go
    // through `run_command_full` which keeps the buffer intact.
    let status_out = match run_command_full(
        "git",
        &["status", "--porcelain"],
        Some(&cwd),
        DEFAULT_TIMEOUT,
    ) {
        WorkerOutcomeFull::Stdout(s) => s,
        // Non-zero (e.g. not a repo at all) → treat as empty so we
        // fall through to `Clean` rather than blanking the cache.
        // The earlier `git rev-parse` already gated the "no repo"
        // case; we should only reach here if the working tree is
        // valid but `git status` mis-behaves for some other reason.
        WorkerOutcomeFull::NonZeroExit(_, _) => String::new(),
        WorkerOutcomeFull::Timeout | WorkerOutcomeFull::SpawnError(_) => {
            // Keep previous cached value.
            return;
        }
    };
    let status = classify_status(&status_out);
    let mut guard = cache.lock().unwrap();
    let changed =
        guard.branch.as_deref() != Some(branch.as_str()) || guard.status.as_ref() != Some(&status);
    guard.branch = Some(branch);
    guard.status = Some(status);
    drop(guard);
    if changed {
        version.fetch_add(1, Ordering::Relaxed);
        wake();
    }
}

fn clear_cache(cache: &Arc<Mutex<GitCache>>, version: &Arc<AtomicU64>, wake: &WakeFn) {
    let mut guard = cache.lock().unwrap();
    if guard.branch.is_some() || guard.status.is_some() {
        guard.branch = None;
        guard.status = None;
        drop(guard);
        version.fetch_add(1, Ordering::Relaxed);
        wake();
    }
}

/// Classify `git status --porcelain` output into one of
/// Clean / Dirty / Untracked. Lines starting with `??` are
/// untracked-only markers; any other non-empty line means dirty.
pub fn classify_status(status_out: &str) -> BranchStatus {
    let mut saw_untracked = false;
    let mut saw_other = false;
    for line in status_out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with("??") {
            saw_untracked = true;
        } else {
            saw_other = true;
        }
    }
    if saw_other {
        BranchStatus::Dirty
    } else if saw_untracked {
        BranchStatus::Untracked
    } else {
        BranchStatus::Clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_status_clean() {
        assert_eq!(classify_status(""), BranchStatus::Clean);
        assert_eq!(classify_status("\n\n"), BranchStatus::Clean);
    }

    #[test]
    fn classify_status_dirty_for_tracked_modifications() {
        let out = " M src/lib.rs\n M README.md\n";
        assert_eq!(classify_status(out), BranchStatus::Dirty);
    }

    #[test]
    fn classify_status_untracked_only_for_question_marks() {
        let out = "?? new_file.rs\n";
        assert_eq!(classify_status(out), BranchStatus::Untracked);
    }

    #[test]
    fn classify_status_mixed_is_dirty() {
        let out = "?? new\n M tracked\n";
        assert_eq!(classify_status(out), BranchStatus::Dirty);
    }

    #[test]
    fn branch_status_colors_match_spec() {
        assert!(matches!(
            BranchStatus::Clean.to_color(),
            CssColor::Hex {
                r: 0x4c,
                g: 0xaf,
                b: 0x50
            }
        ));
        assert!(matches!(
            BranchStatus::Dirty.to_color(),
            CssColor::Hex {
                r: 0xf9,
                g: 0xa8,
                b: 0x25
            }
        ));
        assert!(matches!(
            BranchStatus::Untracked.to_color(),
            CssColor::Hex {
                r: 0x9e,
                g: 0x9e,
                b: 0x9e
            }
        ));
    }

    #[test]
    fn provider_drop_joins_worker() {
        // Construct a provider with a closure that always returns
        // None so the worker exits each tick quickly without git.
        let source: CwdSource = Arc::new(|| None);
        let p = GitBranchProvider::start(source, Duration::from_millis(20));
        // Drop should signal stop and join.
        std::mem::drop(p);
    }

    #[test]
    fn provider_returns_empty_when_no_cwd() {
        let source: CwdSource = Arc::new(|| None);
        let p = GitBranchProvider::start(source, Duration::from_millis(20));
        // Give the worker a moment to run a tick (no cwd → no spawn).
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(p.get_value(None), "");
        assert!(p.get_color(None).is_none());
    }

    /// Regression for the "git status spawned twice per tick" bug
    /// (multi-review HIGH). The worker MUST invoke `git status
    /// --porcelain` at most once per refresh cycle. We verify this by
    /// pointing `PATH` at a tempdir containing a `git` shim that
    /// appends the subcommand to a counter file; after one tick we
    /// expect a single `status` line.
    ///
    /// Skipped silently on non-Unix because the shim script uses a
    /// POSIX sh interpreter line.
    #[cfg(unix)]
    #[test]
    fn git_status_spawned_once_per_tick() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        // Create a unique tempdir under the system temp root. We
        // intentionally avoid a tempfile dep (sdd no-new-deps).
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let dir = std::env::temp_dir().join(format!("emterm-git-spawn-{pid}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();

        // The counter file records one line per git invocation
        // (subcommand name). The git shim writes here on every call.
        let counter = dir.join("invocations.log");

        // Shim: a tiny POSIX shell script that records the first arg
        // and emits a plausible stdout for each git subcommand the
        // worker uses.
        let shim_path = dir.join("git");
        {
            let mut f = std::fs::File::create(&shim_path).unwrap();
            writeln!(f, "#!/bin/sh").unwrap();
            writeln!(f, r#"echo "$1" >> "{}""#, counter.display()).unwrap();
            // `rev-parse --abbrev-ref HEAD` → print a branch name.
            // `status --porcelain` → print one tracked-modification
            // line so classify_status returns Dirty (any non-empty
            // outcome is fine — we just need a deterministic exit 0).
            writeln!(f, "case \"$1\" in").unwrap();
            writeln!(f, "  rev-parse) echo main ;;").unwrap();
            writeln!(f, "  status) echo ' M src/lib.rs' ;;").unwrap();
            writeln!(f, "  *) ;;").unwrap();
            writeln!(f, "esac").unwrap();
            writeln!(f, "exit 0").unwrap();
        }
        let mut perms = std::fs::metadata(&shim_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&shim_path, perms).unwrap();

        // Drive a single tick directly (no worker thread / sleep).
        // We craft a cwd_source that returns the tempdir; the actual
        // value doesn't matter to the shim, which ignores cwd.
        let dir_str = dir.to_string_lossy().into_owned();
        let cwd_source: CwdSource = Arc::new(move || Some(dir_str.clone()));
        let cache: Arc<Mutex<GitCache>> = Arc::new(Mutex::new(GitCache::default()));
        let version = Arc::new(AtomicU64::new(0));
        let wake: WakeFn = Arc::new(|| ());

        // Prepend the tempdir to PATH so the shim wins over the real
        // git. SAFETY: tests in this crate run single-threaded for
        // env mutation (matches the existing `expand_home` test).
        let prev_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.display(), prev_path);
        // SAFETY: tests in this crate run single-threaded for env mutation
        // (see `expand_home`).
        unsafe {
            std::env::set_var("PATH", &new_path);
        }

        super::tick(&cwd_source, &cache, &version, &wake);

        // Restore PATH before any assertion so a failure leaves the
        // process env clean for siblings.
        unsafe {
            std::env::set_var("PATH", &prev_path);
        }

        // Inspect the counter: one `rev-parse` + one `status`. The
        // double-spawn bug used to produce TWO `status` entries.
        let log = std::fs::read_to_string(&counter).expect("counter file written");
        let lines: Vec<&str> = log.lines().collect();
        let status_count = lines.iter().filter(|l| **l == "status").count();
        let rev_parse_count = lines.iter().filter(|l| **l == "rev-parse").count();
        assert_eq!(
            status_count, 1,
            "git status spawned {status_count} times in one tick (log: {log:?})"
        );
        assert_eq!(
            rev_parse_count, 1,
            "git rev-parse spawned {rev_parse_count} times in one tick (log: {log:?})"
        );

        // Cleanup: best-effort; ignore failures because Drop on the
        // tempdir is non-critical.
        let _ = std::fs::remove_dir_all(&dir);
        // Use PathBuf to silence the unused import warning when run on
        // platforms where this test is gated out.
        let _: PathBuf = shim_path;
    }
}
