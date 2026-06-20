//! Parent-side settings-window launcher.
//!
//! The settings UI runs in a separate child process (`self --settings`,
//! see [`crate::settings_window`]). This module owns the parent half:
//! spawning the child (single instance), watching its stdout for the
//! save-event line, and exposing a "settings were saved" flag the winit
//! loop polls to reload + apply `settings.json` live.

use std::io::BufRead as _;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the stdout reader thread when the child reports a persisted
/// save; drained by the winit loop via [`take_saved`].
static SETTINGS_SAVED: AtomicBool = AtomicBool::new(false);

/// True once if a save event arrived since the last call (swap-reset).
pub fn take_saved() -> bool {
    SETTINGS_SAVED.swap(false, Ordering::AcqRel)
}

/// Test hook: raise the saved flag as the reader thread would.
#[cfg(test)]
pub fn mark_saved_for_test() {
    SETTINGS_SAVED.store(true, Ordering::Release);
}

/// Launcher abstraction so `App` logic is unit-testable without spawning
/// real processes.
pub trait SettingsWindowLauncher {
    /// Open the settings window (or focus the intent on the already
    /// running instance — currently: do nothing if it is still alive).
    fn open(&mut self);
}

/// Production launcher: one child process at a time.
pub struct ProcessSettingsLauncher {
    child: Option<std::process::Child>,
}

impl ProcessSettingsLauncher {
    pub fn new() -> Self {
        Self { child: None }
    }

    /// True while the previously spawned child is still running. Reaps a
    /// finished child as a side effect.
    fn child_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(_status)) => {
                    self.child = None;
                    false
                }
                Ok(None) => true,
                Err(e) => {
                    log::warn!("settings launcher: try_wait failed: {e}");
                    self.child = None;
                    false
                }
            },
            None => false,
        }
    }
}

impl SettingsWindowLauncher for ProcessSettingsLauncher {
    fn open(&mut self) {
        if self.child_running() {
            // Single instance: a second request while the window is open
            // is a no-op (the WM owns window raising).
            log::warn!("settings launcher: window already open; ignoring");
            return;
        }
        let mut child = match crate::self_exec::spawn_self(|c| {
            c.arg("--settings").stdout(std::process::Stdio::piped());
        }) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("settings launcher: failed to spawn child ({e}); terminal unaffected");
                return;
            }
        };
        log::warn!("settings launcher: spawned child pid={}", child.id());

        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || watch_child_stdout(stdout));
        }
        self.child = Some(child);
    }
}

/// Reader-thread body: raise the saved flag (and wake the winit loop) for
/// every save-event line the child prints. Ends when the pipe closes.
fn watch_child_stdout(stdout: impl std::io::Read) {
    let reader = std::io::BufReader::new(stdout);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if is_saved_event(&line) {
            SETTINGS_SAVED.store(true, Ordering::Release);
            crate::wakeup::wake();
        }
    }
}

/// Line predicate, separated for tests.
fn is_saved_event(line: &str) -> bool {
    line.trim() == crate::settings_window::SAVED_EVENT_LINE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_event_line_matches_exactly() {
        assert!(is_saved_event("EMTERM_SETTINGS_SAVED"));
        assert!(is_saved_event("  EMTERM_SETTINGS_SAVED\r"));
        assert!(!is_saved_event("EMTERM_SETTINGS_SAVED extra"));
        assert!(!is_saved_event("something else"));
    }

    #[test]
    fn stdout_watcher_raises_the_flag_per_event_line() {
        let _ = take_saved();
        let input = b"noise\nEMTERM_SETTINGS_SAVED\n".to_vec();
        watch_child_stdout(std::io::Cursor::new(input));
        assert!(take_saved());
        assert!(!take_saved(), "flag is swap-reset");
    }
}
