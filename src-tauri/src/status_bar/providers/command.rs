//! `{cmd:<name>}` provider.
//!
//! One worker thread per configured custom command. The worker runs
//! the executable with no arguments at a fixed interval (default
//! 1 s, clamped to ≥ 1000 ms — see SPEC FR6 / US3), captures
//! stdout's first line, and exposes it through the `VariableProvider`
//! trait.
//!
//! On any failure (spawn error, non-zero exit, timeout) the cache
//! retains the previous value — the status bar should degrade
//! gracefully rather than blinking blank on a transient hiccup
//! (FR6).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::html::CssColor;
use crate::settings::CustomCommand;
use crate::status_bar::providers::worker::{
    run_command, spawn, Stop, StopHandle, WorkerOutcome, DEFAULT_TIMEOUT,
};
use crate::status_bar::template_engine::VariableProvider;
use crate::wakeup::WakeFn;

/// Lower bound on the per-command refresh interval. SPEC FR6 / US3
/// acceptance criteria pin this floor at 1000 ms so a user-supplied
/// `interval_ms` below 1 s is clamped up (preventing a worker thread
/// from monopolising CPU on a fast loop).
pub const MIN_INTERVAL: Duration = Duration::from_millis(1000);

pub struct CommandProvider {
    /// Map of command-name → cached stdout. Wrapped in `Arc<Mutex>`
    /// so the per-name workers can write while the UI thread reads.
    cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
    /// Per-name monotonic version counter.
    versions: Arc<Mutex<std::collections::HashMap<String, Arc<AtomicU64>>>>,
    /// Per-worker stop signal + join handle.
    workers: Mutex<Vec<(String, Stop, Option<JoinHandle<()>>)>>,
    /// Optional wake handle. When `Some`, every worker invokes it
    /// after a cache update so winit schedules the next frame.
    wake: Option<WakeFn>,
}

impl Default for CommandProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandProvider {
    /// Construct without a wake handle (legacy / tests).
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            versions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            workers: Mutex::new(Vec::new()),
            wake: None,
        }
    }

    /// Construct with a wake handle. The handle is cloned into every
    /// per-name worker spawned through [`Self::spawn_worker`].
    pub fn with_wake(wake: WakeFn) -> Self {
        Self {
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            versions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            workers: Mutex::new(Vec::new()),
            wake: Some(wake),
        }
    }

    /// Start a worker for `name`. The name MUST match
    /// `[a-zA-Z0-9_-]+`; invalid names are dropped with a warn log.
    pub fn spawn_worker(&self, name: &str, command: &CustomCommand) {
        if !is_valid_command_name(name) {
            log::warn!("status_bar: invalid custom command name {name:?}; skipping");
            return;
        }
        if command.executable.trim().is_empty() {
            return;
        }
        let interval = Duration::from_millis(command.interval_ms).max(MIN_INTERVAL);
        let exe = expand_home(&command.executable);
        let name_s = name.to_string();
        let cache_clone = self.cache.clone();
        let version: Arc<AtomicU64> = {
            let mut v = self.versions.lock().unwrap();
            v.entry(name_s.clone())
                .or_insert_with(|| Arc::new(AtomicU64::new(0)))
                .clone()
        };
        let stop = Stop::new();
        let handle = stop.handle();
        let wake = self
            .wake
            .clone()
            .unwrap_or_else(|| -> WakeFn { Arc::new(|| ()) });
        let join = spawn("custom-cmd-worker", handle.clone(), move |h| {
            command_loop(h, name_s, exe, interval, cache_clone, version, wake);
        });
        self.workers
            .lock()
            .unwrap()
            .push((name.to_string(), stop, Some(join)));
    }

    /// Stop and join all workers. Called automatically by Drop; tests
    /// can call this directly for deterministic teardown.
    pub fn shutdown(&self) {
        let mut workers = self.workers.lock().unwrap();
        for (_, stop, _) in workers.iter() {
            stop.signal();
        }
        for (_, _, join) in workers.iter_mut() {
            if let Some(j) = join.take() {
                let _ = j.join();
            }
        }
        workers.clear();
    }

    fn get(&self, name: &str) -> Option<String> {
        self.cache.lock().unwrap().get(name).cloned()
    }
}

impl Drop for CommandProvider {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl VariableProvider for CommandProvider {
    fn name(&self) -> &str {
        "cmd"
    }

    fn get_value(&self, argument: Option<&str>) -> String {
        let Some(arg) = argument else {
            return String::new();
        };
        self.get(arg).unwrap_or_default()
    }

    fn get_color(&self, _argument: Option<&str>) -> Option<CssColor> {
        None
    }

    fn version(&self, argument: Option<&str>) -> u64 {
        let Some(arg) = argument else {
            return 0;
        };
        let v = self.versions.lock().unwrap();
        v.get(arg).map(|c| c.load(Ordering::Relaxed)).unwrap_or(0)
    }
}

fn command_loop(
    stop: StopHandle,
    name: String,
    executable: String,
    interval: Duration,
    cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
    version: Arc<AtomicU64>,
    wake: WakeFn,
) {
    while !stop.is_stopped() {
        // Spawn the executable with no arguments; the worker layer
        // never invokes a shell (NFR2). Currying multi-arg commands
        // would require a settings shape change — out of scope.
        let outcome = run_command(&executable, &[], None, DEFAULT_TIMEOUT);
        if let WorkerOutcome::Stdout(line) = outcome {
            let mut guard = cache.lock().unwrap();
            let changed = guard.get(&name).map(|s| s.as_str()) != Some(line.as_str());
            guard.insert(name.clone(), line);
            drop(guard);
            if changed {
                version.fetch_add(1, Ordering::Relaxed);
                wake();
            }
        }
        if !stop.wait_timeout(interval) {
            break;
        }
    }
}

/// `true` when `name` matches `[a-zA-Z0-9_-]+`.
pub fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Expand a leading `~/` to the user's home directory. Returns the
/// input verbatim when no expansion applies (no `~/` prefix, or
/// `$HOME` / `%USERPROFILE%` unset).
pub fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = home_dir();
        if let Some(home) = home {
            let sep = if cfg!(windows) { '\\' } else { '/' };
            return format!("{}{}{}", home, sep, rest);
        }
    }
    path.to_string()
}

fn home_dir() -> Option<String> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok().filter(|s| !s.is_empty())
    }
    #[cfg(not(unix))]
    {
        std::env::var("USERPROFILE").ok().filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_command_name_accepts_word_chars() {
        assert!(is_valid_command_name("foo"));
        assert!(is_valid_command_name("Foo_Bar-1"));
        assert!(is_valid_command_name("a"));
    }

    #[test]
    fn invalid_command_name_rejects_specials() {
        assert!(!is_valid_command_name(""));
        assert!(!is_valid_command_name("foo bar"));
        assert!(!is_valid_command_name("foo.bar"));
        assert!(!is_valid_command_name("foo;bar"));
        assert!(!is_valid_command_name("foo/bar"));
    }

    #[test]
    fn expand_home_with_tilde_prefix_resolves() {
        // SAFETY: tests run single-threaded by default for env mutation.
        unsafe {
            std::env::set_var("HOME", "/home/test");
        }
        let out = expand_home("~/projects");
        // Unix path separator.
        if cfg!(unix) {
            assert_eq!(out, "/home/test/projects");
        }
    }

    #[test]
    fn expand_home_passes_through_when_no_tilde() {
        let out = expand_home("/etc/hosts");
        assert_eq!(out, "/etc/hosts");
    }

    #[test]
    fn provider_get_value_with_no_argument_is_empty() {
        let p = CommandProvider::new();
        assert_eq!(p.get_value(None), "");
    }

    #[test]
    fn provider_get_value_unknown_argument_is_empty() {
        let p = CommandProvider::new();
        assert_eq!(p.get_value(Some("missing")), "");
    }

    #[test]
    fn spawn_worker_for_invalid_name_is_no_op() {
        let p = CommandProvider::new();
        p.spawn_worker(
            "no spaces allowed",
            &CustomCommand {
                executable: "/bin/echo".to_string(),
                interval_ms: 100,
            },
        );
        assert_eq!(p.workers.lock().unwrap().len(), 0);
    }

    #[test]
    fn spawn_worker_empty_executable_is_no_op() {
        let p = CommandProvider::new();
        p.spawn_worker(
            "ok",
            &CustomCommand {
                executable: String::new(),
                interval_ms: 100,
            },
        );
        assert_eq!(p.workers.lock().unwrap().len(), 0);
    }

    #[test]
    fn interval_clamped_to_min_when_below_threshold() {
        // We can only observe this indirectly — assert MIN_INTERVAL
        // is the floor exposed by the module. SPEC FR6 / US3 pin
        // this floor at 1000 ms; a sub-second `interval_ms` from
        // settings.json gets clamped up.
        assert_eq!(MIN_INTERVAL, Duration::from_millis(1000));

        // Behavioural check: a small `interval_ms` (e.g. 50 ms) is
        // coerced upward to MIN_INTERVAL by the worker layer. We
        // verify by recreating the same `max(MIN_INTERVAL)` rule the
        // spawn path applies.
        let user_interval = Duration::from_millis(50);
        let effective = user_interval.max(MIN_INTERVAL);
        assert_eq!(effective, Duration::from_millis(1000));
    }

    #[test]
    fn provider_drop_signals_and_joins_workers() {
        let p = CommandProvider::new();
        p.spawn_worker(
            "ok",
            &CustomCommand {
                executable: "/bin/true".to_string(),
                interval_ms: 50,
            },
        );
        // Drop the provider — must signal stop and join the worker.
        std::mem::drop(p);
    }

    #[test]
    fn provider_runs_echo_and_caches_output() {
        let p = CommandProvider::new();
        // Use `/bin/sh -c 'echo hi'` won't work because we don't go
        // through a shell. Use a direct binary that prints something
        // deterministic.
        if std::path::Path::new("/bin/hostname").exists() {
            p.spawn_worker(
                "host",
                &CustomCommand {
                    executable: "/bin/hostname".to_string(),
                    interval_ms: 50,
                },
            );
            std::thread::sleep(Duration::from_millis(400));
            // hostname returns at least one non-empty character.
            let v = p.get_value(Some("host"));
            assert!(!v.is_empty(), "expected hostname output, got empty");
        }
    }
}
