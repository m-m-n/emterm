//! Phase 4-G-A: `NullBackend`.
//!
//! The fallback IME backend the App holds when:
//!
//! - `EMTERM_NATIVE_IME=0` is set, or
//! - `settings.ime.native_integration == false`, or
//! - the OS-appropriate backend's `init` returned an `ImeInitError`.
//!
//! In all three cases the App must behave exactly as Phase 4 did:
//! `WindowEvent::ReceivedImeText` still routes through `on_ime_commit`,
//! `tao_key_to_bytes` runs unchanged, and there is no preedit overlay
//! because the backend never emits `ImeEvent::Preedit`.
//!
//! This impl is intentionally empty — `dispatch_key_event` always
//! returns `Passthrough`, `pump` produces an empty vec, and the
//! cursor / focus notifications are dropped. See SPEC.md §FR9.

use super::backend::{ImeBackend, ImeEvent, KeyDispatchResult, RawKeyEvent};

/// Passthrough-only backend. Holds no state.
#[derive(Debug, Default)]
pub struct NullBackend {
    /// Records whether `notify_focus` was ever called with `false`. Kept
    /// for symmetry with the real backends but otherwise unused — the
    /// App's `on_ime_focus_lost` is wired directly from the
    /// `WindowEvent::Focused(false)` handler in `window_host`.
    last_focused: Option<bool>,
}

impl NullBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ImeBackend for NullBackend {
    fn dispatch_key_event(&mut self, _raw: &RawKeyEvent) -> KeyDispatchResult {
        // Phase 4 behavior: every key falls through to `tao_key_to_bytes`.
        KeyDispatchResult::Passthrough
    }

    fn notify_cursor_rect(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {
        // No IM server to notify.
    }

    fn notify_focus(&mut self, focused: bool) {
        self.last_focused = Some(focused);
    }

    fn pump(&mut self, _events: &mut Vec<ImeEvent>) {
        // NullBackend never produces events. `_events` is left untouched
        // so callers that share the vec across pumps still see only the
        // events from other producers (today: none).
    }

    fn name(&self) -> &'static str {
        "null"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::input::Modifiers;

    fn pressed() -> RawKeyEvent {
        RawKeyEvent {
            physical_key_code: 0x26,
            state_pressed: true,
            mods: Modifiers::NONE,
        }
    }

    // ── TS-backend-1: NullBackend dispatch is always Passthrough ─────

    #[test]
    fn dispatch_returns_passthrough_for_pressed_key() {
        let mut b = NullBackend::new();
        assert_eq!(
            b.dispatch_key_event(&pressed()),
            KeyDispatchResult::Passthrough
        );
    }

    #[test]
    fn dispatch_returns_passthrough_for_released_key() {
        let mut b = NullBackend::new();
        let mut raw = pressed();
        raw.state_pressed = false;
        assert_eq!(b.dispatch_key_event(&raw), KeyDispatchResult::Passthrough);
    }

    #[test]
    fn dispatch_returns_passthrough_under_modifiers() {
        let mut b = NullBackend::new();
        let raw = RawKeyEvent {
            physical_key_code: 0x26,
            state_pressed: true,
            mods: Modifiers {
                ctrl: true,
                alt: true,
                shift: true,
            },
        };
        assert_eq!(b.dispatch_key_event(&raw), KeyDispatchResult::Passthrough);
    }

    // ── TS-backend-2: NullBackend pump produces an empty vec ─────────

    #[test]
    fn pump_produces_no_events() {
        let mut b = NullBackend::new();
        let mut out = Vec::new();
        b.pump(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn pump_preserves_existing_events_in_buffer() {
        // Defensive: pump must not clobber events from a hypothetical
        // earlier producer. NullBackend just doesn't push.
        let mut b = NullBackend::new();
        let mut out = vec![ImeEvent::Commit("prev".into())];
        b.pump(&mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], ImeEvent::Commit("prev".into()));
    }

    #[test]
    fn notify_cursor_rect_is_a_noop() {
        let mut b = NullBackend::new();
        b.notify_cursor_rect(10, 20, 9, 18);
        // No public observable state; the test asserts compilation and
        // absence of panic.
    }

    #[test]
    fn notify_focus_records_last_value() {
        let mut b = NullBackend::new();
        assert_eq!(b.last_focused, None);
        b.notify_focus(true);
        assert_eq!(b.last_focused, Some(true));
        b.notify_focus(false);
        assert_eq!(b.last_focused, Some(false));
    }

    #[test]
    fn name_is_null() {
        let b = NullBackend::new();
        assert_eq!(b.name(), "null");
    }
}
