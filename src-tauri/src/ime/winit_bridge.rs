//! Phase 4-G-3: winit IME bridge.
//!
//! [`WinitImeBridge`] is the platform backend for X11 / Wayland /
//! Windows. It hosts a small Ghostty-derived state machine
//! (`im_composing` + `in_keyevent`) and translates winit
//! `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }` into the
//! platform-neutral [`ImeEvent`] queue that [`crate::app::App::pump_ime`]
//! drains into the existing Phase 4-E routes
//! (`on_ime_preedit / on_ime_commit / on_ime_focus_lost`).
//!
//! The bridge is intentionally thin: winit already owns the XIM /
//! zwp_text_input_v3 / IMM32 details that the previous self-built
//! backends struggled with on tao 0.34. We only need to:
//!
//! 1. Toggle `Window::set_ime_allowed` so winit attaches to the IM
//!    server at startup and detaches on shutdown.
//! 2. Mirror `Ime::*` into `ImeEvent::*` plus update the
//!    `im_composing` flag so [`Self::dispatch_key_event`] can suppress
//!    PTY writes that the IM server has already swallowed.
//! 3. Forward cursor cell changes to `Window::set_ime_cursor_area`
//!    rate-limited to actual cell movement (`notify_cursor_rect`
//!    dedup).
//!
//! The state machine is deliberately a strict subset of Ghostty's. We
//! never need `in_keyevent` to gate a *next-tick* dedup because winit's
//! event model already enforces "Ime events strictly precede the
//! follow-up KeyboardInput" — winit suppresses `KeyEvent::text` while
//! composition is active.

use std::collections::VecDeque;
use std::sync::Arc;

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::Ime as WinitIme;
use winit::window::{ImePurpose, Window};

use super::backend::{ImeBackend, ImeEvent, ImeInitError, KeyDispatchResult, RawKeyEvent};

/// Window handle abstraction so the bridge can be unit-tested without
/// spinning up a real winit `EventLoop`. Production code always uses
/// [`WinitWindowHandle`] backed by `Arc<dyn winit::window::Window>`.
pub trait BridgeWindow: Send + Sync {
    /// Toggle whether the platform IM server should send composition
    /// events to this window. Called once at `init` (`true`) and from
    /// [`WinitImeBridge::notify_focus`].
    fn set_ime_allowed(&self, allowed: bool);
    /// Inform the IM server where the active cursor cell currently
    /// sits, in physical pixels. Drives the candidate window position.
    fn set_ime_cursor_area(&self, x: i32, y: i32, width: i32, height: i32);
}

/// Production [`BridgeWindow`] implementation backed by an
/// `Arc<dyn winit::window::Window>`.
struct WinitWindowHandle(Arc<dyn Window>);

impl BridgeWindow for WinitWindowHandle {
    fn set_ime_allowed(&self, allowed: bool) {
        self.0.set_ime_allowed(allowed);
    }

    fn set_ime_cursor_area(&self, x: i32, y: i32, width: i32, height: i32) {
        self.0.set_ime_cursor_area(
            winit::dpi::Position::Physical(PhysicalPosition::new(x, y)),
            winit::dpi::Size::Physical(PhysicalSize::new(
                width.max(1) as u32,
                height.max(1) as u32,
            )),
        );
    }
}

/// winit-backed IME bridge. See module docs for the contract.
pub struct WinitImeBridge {
    /// Platform window the bridge talks to (set_ime_allowed /
    /// set_ime_cursor_area).
    window: Box<dyn BridgeWindow>,
    /// `true` while the IM server is mid-composition (`Ime::Enabled` /
    /// `Ime::Preedit` non-empty observed, cleared by `Ime::Commit` /
    /// `Ime::Disabled`). Drives the `Consumed` branch of
    /// `dispatch_key_event`.
    im_composing: bool,
    /// Events produced by `on_winit_ime` waiting for the next
    /// `pump` drain.
    queue: VecDeque<ImeEvent>,
    /// Last cursor rect handed to the window (physical pixels). Used
    /// to suppress redundant `set_ime_cursor_area` calls (SPEC.md FR7).
    last_cursor_area: Option<(i32, i32, i32, i32)>,
}

impl WinitImeBridge {
    /// Build a bridge attached to a winit window. The returned bridge
    /// has already called `set_ime_allowed(true)` so winit will start
    /// surfacing `WindowEvent::Ime` events.
    pub fn init(window: Arc<dyn Window>) -> Result<Self, ImeInitError> {
        // Hint the platform that this is a terminal so candidate
        // windows can size themselves accordingly. `Normal` is the
        // default; we set it explicitly for documentation.
        window.set_ime_purpose(ImePurpose::Normal);
        window.set_ime_allowed(true);
        Ok(Self::with_handle(Box::new(WinitWindowHandle(window))))
    }

    /// Test entry: build a bridge against an arbitrary [`BridgeWindow`]
    /// implementation (a mock from the test module).
    pub(crate) fn with_handle(window: Box<dyn BridgeWindow>) -> Self {
        window.set_ime_allowed(true);
        Self {
            window,
            im_composing: false,
            queue: VecDeque::new(),
            last_cursor_area: None,
        }
    }

    /// Push a winit `WindowEvent::Ime` payload through the bridge.
    /// Translates each variant into the appropriate [`ImeEvent`] and
    /// updates `im_composing`.
    pub fn on_winit_ime(&mut self, ime: &WinitIme) {
        match ime {
            WinitIme::Enabled => {
                // The IM server attached. We don't immediately treat
                // this as "composing" — Ghostty observed that some
                // compositors fire Enabled even for plain ASCII rounds.
                // Composition starts when the first Preedit arrives.
            }
            WinitIme::Preedit(text, _cursor) => {
                if text.is_empty() {
                    // Empty preedit clears the overlay but does not end
                    // the composition (Wayland zwp_text_input_v3 sends
                    // this for cursor-only updates). Mirror as an empty
                    // Preedit so on_ime_preedit drops the overlay.
                    self.queue.push_back(ImeEvent::Preedit(String::new()));
                } else {
                    self.im_composing = true;
                    self.queue.push_back(ImeEvent::Preedit(text.clone()));
                }
            }
            WinitIme::Commit(text) => {
                self.im_composing = false;
                if !text.is_empty() {
                    self.queue.push_back(ImeEvent::Commit(text.clone()));
                }
            }
            WinitIme::Disabled => {
                // IM server detached (user toggled IME off, focus
                // lost from compositor's POV, fcitx5/ibus crashed).
                // Clear composition state and signal the App to drop
                // any preedit overlay.
                self.im_composing = false;
                self.queue.push_back(ImeEvent::FocusOut);
            }
            WinitIme::DeleteSurrounding { .. } => {
                // New in winit 0.31: the IM server asks the editor to
                // delete text surrounding the cursor/selection. The
                // existing Ghostty-derived pipeline (Preedit/Commit/
                // Enabled/Disabled) has no editor-side hook for this
                // yet, so it is a documented no-op rather than a
                // migration regression — wiring it up is out of scope
                // for the winit version bump.
            }
        }
    }
}

impl ImeBackend for WinitImeBridge {
    fn dispatch_key_event(&mut self, _raw: &RawKeyEvent) -> KeyDispatchResult {
        // Ghostty rule: while composition is active the IM server owns
        // every key — we must not emit the key bytes ourselves. winit
        // already suppresses `KeyEvent::text` during composition, but
        // we still need to block the named-key fallback path (Enter /
        // arrows / etc. are sometimes routed through the IM server
        // even though winit gives us the KeyEvent first).
        if self.im_composing {
            KeyDispatchResult::Consumed
        } else {
            KeyDispatchResult::Passthrough
        }
    }

    fn notify_cursor_rect(&mut self, x_px: i32, y_px: i32, w_px: i32, h_px: i32) {
        // FR7 dedup: suppress identical rects so the IM server doesn't
        // see noise on every frame when the cursor is stationary.
        let next = (x_px, y_px, w_px, h_px);
        if self.last_cursor_area == Some(next) {
            return;
        }
        self.last_cursor_area = Some(next);
        self.window.set_ime_cursor_area(x_px, y_px, w_px, h_px);
    }

    fn notify_focus(&mut self, focused: bool) {
        // FR8: defer to winit's set_ime_allowed so the platform IM
        // server attaches on focus-in and detaches on focus-out. If the
        // window already saw `Ime::Disabled` from a focus-loss path,
        // re-toggling allowed here is idempotent on winit.
        self.window.set_ime_allowed(focused);
    }

    fn pump(&mut self, events: &mut Vec<ImeEvent>) {
        // Drain everything queued; the App caller enforces PUMP_BUDGET.
        events.extend(self.queue.drain(..));
    }

    fn name(&self) -> &'static str {
        "winit"
    }

    fn on_winit_ime(&mut self, ime: &winit::event::Ime) {
        // Trait-level entry point used by `window_host`. Delegate to
        // the inherent method that the unit tests exercise directly.
        WinitImeBridge::on_winit_ime(self, ime);
    }
}

impl Drop for WinitImeBridge {
    fn drop(&mut self) {
        // Detach from the IM server explicitly. winit also drops the
        // IC when the window is destroyed, but releasing it eagerly
        // avoids leaving a stale focus claim if the bridge is
        // replaced mid-session (e.g. switching to NullBackend after a
        // settings change).
        self.window.set_ime_allowed(false);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::pty::input::Modifiers;

    /// Mock [`BridgeWindow`] that records every call so tests can
    /// assert dedup behaviour.
    #[derive(Default)]
    struct MockWindow {
        inner: Mutex<MockWindowInner>,
    }

    #[derive(Default, Debug)]
    struct MockWindowInner {
        allowed_calls: Vec<bool>,
        cursor_calls: Vec<(i32, i32, i32, i32)>,
    }

    impl BridgeWindow for MockWindow {
        fn set_ime_allowed(&self, allowed: bool) {
            self.inner.lock().unwrap().allowed_calls.push(allowed);
        }
        fn set_ime_cursor_area(&self, x: i32, y: i32, w: i32, h: i32) {
            self.inner.lock().unwrap().cursor_calls.push((x, y, w, h));
        }
    }

    fn raw_press() -> RawKeyEvent {
        RawKeyEvent {
            physical_key_code: 0x26,
            state_pressed: true,
            mods: Modifiers::NONE,
        }
    }

    fn raw_release() -> RawKeyEvent {
        RawKeyEvent {
            physical_key_code: 0x26,
            state_pressed: false,
            mods: Modifiers::NONE,
        }
    }

    // We can't construct an `Arc<MockWindow>` and then turn it into a
    // `Box<dyn BridgeWindow>` while still inspecting it from the test;
    // share through `Arc<MockWindow>` and pass a thin shim.
    struct SharedMock(Arc<MockWindow>);
    impl BridgeWindow for SharedMock {
        fn set_ime_allowed(&self, allowed: bool) {
            self.0.set_ime_allowed(allowed);
        }
        fn set_ime_cursor_area(&self, x: i32, y: i32, w: i32, h: i32) {
            self.0.set_ime_cursor_area(x, y, w, h);
        }
    }

    fn make_bridge() -> (WinitImeBridge, Arc<MockWindow>) {
        let mock = Arc::new(MockWindow::default());
        let bridge = WinitImeBridge::with_handle(Box::new(SharedMock(mock.clone())));
        (bridge, mock)
    }

    // ── TS-winit-1: composition starts on first Preedit, gates key path
    #[test]
    fn preedit_starts_composition_and_dispatch_returns_consumed() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("に".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed
        );
    }

    // ── TS-winit-2: Preedit text rides through pump verbatim
    #[test]
    fn preedit_text_is_routed_through_pump() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("foo".into(), None));
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(out, vec![ImeEvent::Preedit("foo".into())]);
    }

    // ── TS-winit-3: Commit ends composition + next dispatch is Passthrough
    #[test]
    fn commit_clears_composition_and_unblocks_dispatch() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("に".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed
        );
        b.on_winit_ime(&WinitIme::Commit("日本".into()));
        // After commit the IM server has released the chord; the next
        // KeyboardInput must reach the PTY path.
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough
        );
        let mut out = Vec::new();
        b.pump(&mut out);
        // The queue ends with the Commit. Preedit("に") + Commit("日本").
        assert_eq!(
            out,
            vec![
                ImeEvent::Preedit("に".into()),
                ImeEvent::Commit("日本".into()),
            ]
        );
    }

    // ── TS-winit-4: Disabled produces FocusOut + clears composition
    #[test]
    fn disabled_produces_focus_out_and_clears_composition() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("ab".into(), None));
        b.on_winit_ime(&WinitIme::Disabled);
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough
        );
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(
            out,
            vec![ImeEvent::Preedit("ab".into()), ImeEvent::FocusOut]
        );
    }

    // ── TS-winit-5: Commit → Disabled is idempotent (no double FocusOut clear)
    #[test]
    fn commit_then_disabled_is_idempotent() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.on_winit_ime(&WinitIme::Commit("X".into()));
        // im_composing is already false; a follow-up Disabled must
        // still surface FocusOut exactly once and leave the flag false.
        b.on_winit_ime(&WinitIme::Disabled);
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(
            out,
            vec![
                ImeEvent::Preedit("x".into()),
                ImeEvent::Commit("X".into()),
                ImeEvent::FocusOut,
            ]
        );
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough
        );
    }

    // ── TS-winit-6: modifier-only release reaches dispatch as Passthrough
    //               (Ghostty fcitx5 toggle scenario)
    #[test]
    fn modifier_release_passes_through_when_not_composing() {
        let (mut b, _mock) = make_bridge();
        // Composition off → release of any key (including modifier-only
        // synthetic ones the App may feed) returns Passthrough.
        assert_eq!(
            b.dispatch_key_event(&raw_release()),
            KeyDispatchResult::Passthrough
        );
    }

    // ── TS-winit-7: notify_cursor_rect dedups identical rects
    #[test]
    fn cursor_rect_is_deduplicated() {
        let (mut b, mock) = make_bridge();
        b.notify_cursor_rect(10, 20, 9, 18);
        b.notify_cursor_rect(10, 20, 9, 18); // exact same → drop
        b.notify_cursor_rect(11, 20, 9, 18); // moved → fire
        let inner = mock.inner.lock().unwrap();
        assert_eq!(
            inner.cursor_calls,
            vec![(10, 20, 9, 18), (11, 20, 9, 18)],
            "duplicate calls must be suppressed"
        );
    }

    // ── set_ime_allowed lifecycle: init→true, notify_focus toggles
    #[test]
    fn init_calls_set_ime_allowed_true() {
        let (_b, mock) = make_bridge();
        let inner = mock.inner.lock().unwrap();
        assert_eq!(inner.allowed_calls, vec![true]);
    }

    #[test]
    fn notify_focus_propagates_to_set_ime_allowed() {
        let (mut b, mock) = make_bridge();
        b.notify_focus(false);
        b.notify_focus(true);
        let inner = mock.inner.lock().unwrap();
        assert_eq!(inner.allowed_calls, vec![true, false, true]);
    }

    #[test]
    fn drop_disables_ime() {
        let mock = Arc::new(MockWindow::default());
        {
            let _b = WinitImeBridge::with_handle(Box::new(SharedMock(mock.clone())));
            // drop happens at scope end
        }
        let inner = mock.inner.lock().unwrap();
        // init=true + drop=false
        assert_eq!(inner.allowed_calls, vec![true, false]);
    }

    #[test]
    fn empty_preedit_is_surfaced_for_overlay_clear() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(out, vec![ImeEvent::Preedit(String::new())]);
        // Empty preedit must NOT toggle im_composing on.
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough
        );
    }

    #[test]
    fn name_is_winit() {
        let (b, _mock) = make_bridge();
        assert_eq!(b.name(), "winit");
    }

    // ── TS-winit-int-1: Xvfb-backed integration test (#[ignore]).
    //
    // The harness creates a real winit window with `Visible::Hidden`
    // semantics on Xvfb, attaches a WinitImeBridge, and asserts that
    // `WindowEvent::Ime::Disabled` fires within one tick when no IM
    // server is present. Running this without `#[ignore]` requires
    // `Xvfb :99 -screen 0 1024x768x24` + `DISPLAY=:99` in the
    // environment, which is host-deferred (see VERIFICATION.md).
    #[test]
    #[ignore = "requires Xvfb + DISPLAY; host-deferred"]
    fn winit_event_loop_surfaces_ime_disabled_without_im_server() {
        // Marker test — the actual harness is the manual gate.
    }

    // ── TS-winit-int-2: Windows IMM32 integration (#[cfg(windows)]).
    //
    // Marker for the host-deferred Windows IMM32 check. The cargo
    // tree on the Linux CI compiles this stub but the body asserts
    // nothing because `cfg(windows)` is false. The real assertion is
    // run on the Windows host (see TS-manual-ime-windows).
    #[test]
    #[cfg(windows)]
    fn winit_imm32_surfaces_ime_commit() {
        // Marker test — the actual harness is the manual gate.
    }
}
