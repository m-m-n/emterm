//! OSC `777;statusbar;…` dispatcher.
//!
//! Routes OSC 777 payloads addressed to the status bar into a shared
//! [`OscLayerState`] without touching the existing `osc_queue` path
//! used by `markdown` / `image` / `viewer` extensions. The dispatcher
//! is wired from [`crate::callbacks::NativeCallbacks::on_osc`] via
//! [`try_dispatch_statusbar`].
//!
//! Subcommand grammar (each segment delimited by `;`):
//! - `statusbar;set;left;<content>`    → strip tags into `state.left`
//! - `statusbar;set;right;<content>`   → strip tags into `state.right`
//! - `statusbar;clear`                 → clear both sides
//! - `statusbar;clear;left`            → clear left only
//! - `statusbar;clear;right`           → clear right only
//! - `statusbar;show`                  → forced_visible = Some(true)
//! - `statusbar;hide`                  → forced_visible = Some(false)
//! - everything else                   → log + ignore
//!
//! `set` content may itself contain `;`; the dispatcher uses
//! `splitn(N, ';')` so a content body with embedded `;` survives
//! intact.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::html::strip_html_tags;
use crate::wakeup;

/// Per-frame OSC-row state. Populated by the dispatcher; read by the
/// view-model builder.
#[derive(Debug, Clone, Default)]
pub struct OscLayerState {
    pub left: String,
    pub right: String,
    /// `Some(true)` means the OSC writer requested the layer to stay
    /// visible even when both sides are empty (and vice versa). The
    /// renderer treats `None` as "auto-hide on empty".
    pub forced_visible: Option<bool>,
}

/// Dispatcher. Cheap to clone (shares the inner state via `Arc`).
pub struct StatusBarOscDispatcher {
    state: Arc<Mutex<OscLayerState>>,
}

impl Default for StatusBarOscDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBarOscDispatcher {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(OscLayerState::default())),
        }
    }

    /// Construct a dispatcher backed by a pre-existing state handle.
    /// Used by [`crate::status_bar::runtime::StatusBarRuntime`] to share
    /// the same state between the dispatcher and the view-model builder.
    pub fn with_state(state: Arc<Mutex<OscLayerState>>) -> Self {
        Self { state }
    }

    /// Snapshot the current OSC-layer state.
    pub fn snapshot(&self) -> OscLayerState {
        self.state.lock().clone()
    }

    /// Underlying state handle, shared with the runtime / view-model.
    pub fn state_handle(&self) -> Arc<Mutex<OscLayerState>> {
        self.state.clone()
    }

    /// Drop the `forced_visible` flag back to `None` (auto-hide).
    /// Used by [`crate::status_bar::runtime::StatusBarRuntime`] on
    /// mux disconnect so the OSC row vanishes when no OSC 777 writer
    /// is actively pinning it open. Distinct from `handle(&["hide"])`
    /// (which sets `Some(false)` and would suppress a subsequent
    /// non-empty `set`).
    pub fn reset_forced_visible(&self) {
        let mut state = self.state.lock();
        state.forced_visible = None;
    }

    /// Apply a parsed token list. `tokens[0]` MUST already be the
    /// subcommand (`set` / `clear` / `show` / `hide`) — the leading
    /// `statusbar` prefix is consumed by [`try_dispatch_statusbar`].
    pub fn handle(&self, tokens: &[&str]) {
        let mut state = self.state.lock();
        match tokens {
            ["set", "left", content] => {
                state.left = strip_html_tags(content);
                state.forced_visible = Some(true);
            }
            ["set", "right", content] => {
                state.right = strip_html_tags(content);
                state.forced_visible = Some(true);
            }
            ["clear"] => {
                state.left.clear();
                state.right.clear();
            }
            ["clear", "left"] => state.left.clear(),
            ["clear", "right"] => state.right.clear(),
            ["show"] => state.forced_visible = Some(true),
            ["hide"] => state.forced_visible = Some(false),
            _ => {
                log::warn!("OSC 777;statusbar: unknown subcommand {tokens:?}; ignoring");
                return;
            }
        }
        drop(state);
        wakeup::wake();
    }
}

/// Inspect an OSC 777 payload and, if it addresses the status bar,
/// dispatch it. Returns `true` when the payload was consumed (caller
/// MUST NOT push it onto `osc_queue`); `false` when the payload was
/// not for us.
///
/// `splitn` boundaries:
/// - First split at most 2 → `["statusbar", rest]`. If the prefix
///   doesn't match, return `false`.
/// - For `set`, split the remainder into at most 3 segments so the
///   content body keeps any embedded `;`.
/// - Other subcommands have no content body, so a plain `split(';')`
///   is safe.
pub fn try_dispatch_statusbar(dispatcher: &StatusBarOscDispatcher, payload: &str) -> bool {
    let mut head = payload.splitn(2, ';');
    let first = head.next().unwrap_or("");
    if first != "statusbar" {
        return false;
    }
    let rest = head.next().unwrap_or("");
    if rest.is_empty() {
        log::warn!("OSC 777;statusbar: empty subcommand");
        return true;
    }
    // Decide based on the next subcommand token.
    let mut sub = rest.splitn(2, ';');
    let cmd = sub.next().unwrap_or("");
    let tail = sub.next().unwrap_or("");
    match cmd {
        "set" => {
            // `set;<section>;<content...>` — content may contain `;`.
            let mut split = tail.splitn(2, ';');
            let section = split.next().unwrap_or("");
            let content = split.next().unwrap_or("");
            dispatcher.handle(&["set", section, content]);
        }
        "clear" => {
            if tail.is_empty() {
                dispatcher.handle(&["clear"]);
            } else {
                dispatcher.handle(&["clear", tail]);
            }
        }
        "show" => dispatcher.handle(&["show"]),
        "hide" => dispatcher.handle(&["hide"]),
        other => {
            log::warn!("OSC 777;statusbar: unknown subcommand {other:?}");
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> StatusBarOscDispatcher {
        StatusBarOscDispatcher::new()
    }

    // ── try_dispatch_statusbar prefix gating ─────────────────────────

    #[test]
    fn dispatcher_ignores_non_statusbar_prefix() {
        let d = fresh();
        assert!(!try_dispatch_statusbar(&d, "markdown;hello"));
        assert!(!try_dispatch_statusbar(&d, ""));
    }

    #[test]
    fn dispatcher_consumes_empty_subcommand() {
        let d = fresh();
        // `statusbar;` alone is consumed (and logged) — must not
        // fall through to the legacy queue.
        assert!(try_dispatch_statusbar(&d, "statusbar"));
        assert!(try_dispatch_statusbar(&d, "statusbar;"));
    }

    // ── set / clear / show / hide ────────────────────────────────────

    #[test]
    fn set_left_stores_stripped_text() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;left;hello");
        assert_eq!(d.snapshot().left, "hello");
        assert_eq!(d.snapshot().forced_visible, Some(true));
    }

    #[test]
    fn set_right_stores_stripped_text() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;right;hi");
        assert_eq!(d.snapshot().right, "hi");
    }

    #[test]
    fn set_strips_html_tags_and_script() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;left;<script>evil()</script>foo");
        assert_eq!(d.snapshot().left, "foo");
    }

    #[test]
    fn set_preserves_semicolons_in_content() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;left;a;b;c");
        assert_eq!(d.snapshot().left, "a;b;c");
    }

    #[test]
    fn clear_empties_both_sides() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;left;a");
        try_dispatch_statusbar(&d, "statusbar;set;right;b");
        try_dispatch_statusbar(&d, "statusbar;clear");
        let s = d.snapshot();
        assert!(s.left.is_empty());
        assert!(s.right.is_empty());
    }

    #[test]
    fn clear_left_only() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;left;a");
        try_dispatch_statusbar(&d, "statusbar;set;right;b");
        try_dispatch_statusbar(&d, "statusbar;clear;left");
        let s = d.snapshot();
        assert!(s.left.is_empty());
        assert_eq!(s.right, "b");
    }

    #[test]
    fn clear_right_only() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;left;a");
        try_dispatch_statusbar(&d, "statusbar;set;right;b");
        try_dispatch_statusbar(&d, "statusbar;clear;right");
        let s = d.snapshot();
        assert_eq!(s.left, "a");
        assert!(s.right.is_empty());
    }

    #[test]
    fn show_and_hide_toggle_forced_visible() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;hide");
        assert_eq!(d.snapshot().forced_visible, Some(false));
        try_dispatch_statusbar(&d, "statusbar;show");
        assert_eq!(d.snapshot().forced_visible, Some(true));
    }

    #[test]
    fn unknown_subcommand_logs_but_does_not_modify_state() {
        let d = fresh();
        try_dispatch_statusbar(&d, "statusbar;set;left;keep");
        try_dispatch_statusbar(&d, "statusbar;nonsense");
        assert_eq!(d.snapshot().left, "keep");
    }

    // ── shared state across clones ────────────────────────────────────

    #[test]
    fn dispatcher_with_state_observes_external_state() {
        let state = Arc::new(Mutex::new(OscLayerState::default()));
        let d1 = StatusBarOscDispatcher::with_state(state.clone());
        let d2 = StatusBarOscDispatcher::with_state(state.clone());
        try_dispatch_statusbar(&d1, "statusbar;set;left;x");
        assert_eq!(d2.snapshot().left, "x");
    }
}
