//! `{agent_status}` provider.
//!
//! Holds the state word of the active tab's composite agent-status
//! aggregate (the tab itself plus, while mux-attached, every pane of its
//! window group), or the empty string when no key carries a state.
//!
//! The provider never reads terminal data itself: its only input is the
//! value `App` sets through [`AgentStatusProvider::set_value`] each time it
//! builds the status bar view model. Every set that changes the stored
//! value advances the version (so the per-row run cache invalidates) and
//! fires the shared wake function (so the frame is redrawn); a set that
//! repeats the stored value has no side effect.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::html::CssColor;
use crate::status_bar::template_engine::VariableProvider;
use crate::wakeup::WakeFn;

pub struct AgentStatusProvider {
    value: Mutex<String>,
    version: AtomicU64,
    /// Wake handle invoked from [`Self::set_value`] when the value
    /// changes. The provider has no polling thread: wakes are
    /// event-driven. `None` is for unit tests that only exercise the
    /// value and version.
    wake: Option<WakeFn>,
}

impl AgentStatusProvider {
    /// Construct without a wake handle. Retained for unit tests.
    pub fn new() -> Self {
        Self {
            value: Mutex::new(String::new()),
            version: AtomicU64::new(0),
            wake: None,
        }
    }

    /// Construct with a wake handle that [`Self::set_value`] invokes
    /// whenever the stored value changes.
    pub fn with_wake(wake: WakeFn) -> Self {
        Self {
            value: Mutex::new(String::new()),
            version: AtomicU64::new(0),
            wake: Some(wake),
        }
    }

    /// Replace the stored value.
    ///
    /// When `value` differs from the stored one, the version advances and
    /// the installed [`WakeFn`] fires once so the egui frame schedules a
    /// redraw. Setting the stored value again has no side effect.
    pub fn set_value(&self, value: &str) {
        let mut stored = self.value.lock().unwrap();
        if stored.as_str() == value {
            return;
        }
        *stored = value.to_string();
        // Bumped under the lock, after the store: a reader that sees the
        // new version also sees (at least) the new value.
        self.version.fetch_add(1, Ordering::Relaxed);
        drop(stored);
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

impl Default for AgentStatusProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl VariableProvider for AgentStatusProvider {
    fn name(&self) -> &str {
        "agent_status"
    }

    fn get_value(&self, _argument: Option<&str>) -> String {
        self.value.lock().unwrap().clone()
    }

    fn get_color(&self, _argument: Option<&str>) -> Option<CssColor> {
        None
    }

    fn version(&self, _argument: Option<&str>) -> u64 {
        self.version.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    fn counting_wake() -> (WakeFn, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let wake: WakeFn = Arc::new(move || {
            c2.fetch_add(1, Ordering::Relaxed);
        });
        (wake, count)
    }

    #[test]
    fn provider_is_named_agent_status() {
        assert_eq!(AgentStatusProvider::new().name(), "agent_status");
    }

    #[test]
    fn fresh_provider_resolves_to_the_empty_string_at_version_zero() {
        let p = AgentStatusProvider::new();
        assert_eq!(p.get_value(None), "");
        assert_eq!(p.version(None), 0);
    }

    #[test]
    fn set_value_is_returned_by_get_value_and_ignores_the_argument() {
        let p = AgentStatusProvider::new();
        p.set_value("working");
        assert_eq!(p.get_value(None), "working");
        assert_eq!(p.get_value(Some("anything")), "working");
    }

    /// AC-3: a different value advances the version and fires the wake
    /// function once.
    #[test]
    fn ac3_set_value_with_a_different_value_bumps_version_and_wakes_once() {
        let (wake, count) = counting_wake();
        let p = AgentStatusProvider::with_wake(wake);
        let v0 = p.version(None);
        p.set_value("blocked");
        assert!(p.version(None) > v0);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    /// AC-3: setting the same value again changes neither the version nor
    /// the wake count.
    #[test]
    fn ac3_set_value_with_the_same_value_changes_neither_version_nor_wake() {
        let (wake, count) = counting_wake();
        let p = AgentStatusProvider::with_wake(wake);
        p.set_value("done");
        let v1 = p.version(None);
        p.set_value("done");
        p.set_value("done");
        assert_eq!(p.version(None), v1);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    /// AC-3: setting the empty string on a fresh provider is not a change.
    #[test]
    fn ac3_setting_the_empty_string_on_a_fresh_provider_is_not_a_change() {
        let (wake, count) = counting_wake();
        let p = AgentStatusProvider::with_wake(wake);
        p.set_value("");
        assert_eq!(p.version(None), 0);
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    /// AC-3: clearing (word -> empty string) is a change like any other.
    #[test]
    fn ac3_clearing_to_the_empty_string_bumps_version_and_wakes() {
        let (wake, count) = counting_wake();
        let p = AgentStatusProvider::with_wake(wake);
        p.set_value("idle");
        let v1 = p.version(None);
        p.set_value("");
        assert!(p.version(None) > v1);
        assert_eq!(p.get_value(None), "");
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    /// Reading the value never advances the version: only `set_value`
    /// does.
    #[test]
    fn get_value_never_bumps_the_version() {
        let p = AgentStatusProvider::new();
        p.set_value("working");
        let v1 = p.version(None);
        let _ = p.get_value(None);
        let _ = p.get_value(None);
        assert_eq!(p.version(None), v1);
    }

    #[test]
    fn set_value_without_a_wake_handle_is_safe_and_still_bumps_the_version() {
        let p = AgentStatusProvider::new();
        p.set_value("working");
        assert!(p.version(None) >= 1);
    }
}
