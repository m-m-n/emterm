//! Phase 4-G-3: winit IME bridge.
//!
//! [`WinitImeBridge`] is the platform backend for X11 / Wayland /
//! Windows. It tracks two independent boolean states — `has_preedit`
//! and `ime_enabled` — and translates winit
//! `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }` into the
//! platform-neutral [`ImeEvent`] queue that [`crate::app::App::pump_ime`]
//! drains into the existing Phase 4-E routes
//! (`on_ime_preedit / on_ime_commit / on_ime_focus_lost`).
//!
//! The bridge is intentionally thin: winit already owns the XIM /
//! zwp_text_input_v3 / IMM32 details that the previous self-built
//! backends struggled with on tao 0.34. We only need to:
//!
//! 1. Enable / disable the IME via `Window::request_ime_update`
//!    (`ImeRequest::Enable` / `Disable`) so winit attaches to the IM
//!    server at startup and detaches on shutdown.
//! 2. Mirror `Ime::*` into `ImeEvent::*` plus update `has_preedit` /
//!    `ime_enabled` so [`Self::dispatch_key_event`] can suppress PTY
//!    writes that the IM server has already swallowed.
//! 3. Forward cursor cell changes as `ImeRequest::Update` cursor-area
//!    requests, rate-limited to actual cell movement
//!    (`notify_cursor_rect` dedup).
//!
//! ## Why two states, and why the key-suppression gate is platform-conditional
//!
//! `Ime::Enabled` / `Ime::Disabled` do not delimit the same thing on
//! every platform, verified against the pinned `winit 0.31.0-beta.2`
//! sources:
//!
//! - `winit-wayland/src/seat/text_input/mod.rs` emits `Ime::Enabled`
//!   from `TextInputEvent::Enter` — once per focus-in, spanning
//!   ordinary direct input for the whole time the window has focus.
//!   The same file emits an empty `Ime::Preedit` only to clear a
//!   previous preedit.
//! - `winit-x11/src/ime/mod.rs` emits `Ime::Enabled` when the XIC is
//!   created and allowed, likewise spanning direct input.
//! - `winit-win32/src/event_loop.rs` maps `Ime::Enabled` /
//!   `Ime::Disabled` to `WM_IME_STARTCOMPOSITION` /
//!   `WM_IME_ENDCOMPOSITION`, so on Windows the pair delimits exactly
//!   one composition; `winit-win32/src/ime.rs` treats a zero-length
//!   `GCS_COMPSTR` as a legitimate empty preedit inside that live
//!   composition.
//!
//! Gating key suppression on the lifecycle flag would therefore
//! swallow all typing on Unix, and gating on preedit-emptiness alone
//! would let Windows candidate-navigation keys leak through
//! mid-composition. `has_preedit` (the last observed preedit was
//! non-empty) is the correct gate on every target except Windows;
//! `ime_enabled` (the lifecycle is open) is the correct gate on
//! Windows. See SPEC.md "Rationale for the platform split" for the
//! full source citations.

use std::collections::VecDeque;
use std::sync::Arc;

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::Ime as WinitIme;
use winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeHint, ImePurpose, ImeRequest, ImeRequestData, Window,
};

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
        let request = if allowed {
            // Enable with hint/purpose + cursor-area capabilities. The
            // (0,0)/(0,0) cursor area is a placeholder — the first
            // `notify_cursor_rect` after enable replaces it with the
            // real caret cell. Purpose `Normal` documents "plain
            // terminal text", replacing the former standalone
            // `set_ime_purpose` call.
            let caps = ImeCapabilities::new()
                .with_hint_and_purpose()
                .with_cursor_area();
            let data = ImeRequestData::default()
                .with_hint_and_purpose(ImeHint::NONE, ImePurpose::Normal)
                .with_cursor_area(
                    winit::dpi::Position::Physical(PhysicalPosition::new(0, 0)),
                    winit::dpi::Size::Physical(PhysicalSize::new(1, 1)),
                );
            let Some(enable) = ImeEnableRequest::new(caps, data) else {
                // Unreachable by construction (caps and data match),
                // but never panic inside the input path.
                return;
            };
            ImeRequest::Enable(enable)
        } else {
            ImeRequest::Disable
        };
        // AlreadyEnabled / NotSupported are fine: notify_focus(true)
        // re-fires on every focus-in, which was idempotent under the
        // old set_ime_allowed contract too.
        let _ = self.0.request_ime_update(request);
    }

    fn set_ime_cursor_area(&self, x: i32, y: i32, width: i32, height: i32) {
        // NotEnabled (rect pushed while the IME is off) is fine — the
        // enable path re-seeds the area and notify_cursor_rect keeps
        // updating it afterwards.
        let _ = self.0.request_ime_update(ImeRequest::Update(
            ImeRequestData::default().with_cursor_area(
                winit::dpi::Position::Physical(PhysicalPosition::new(x, y)),
                winit::dpi::Size::Physical(PhysicalSize::new(
                    width.max(1) as u32,
                    height.max(1) as u32,
                )),
            ),
        ));
    }
}

/// winit-backed IME bridge. See module docs for the contract.
pub struct WinitImeBridge {
    /// Platform window the bridge talks to (set_ime_allowed /
    /// set_ime_cursor_area).
    window: Box<dyn BridgeWindow>,
    /// `true` when the most recently observed `Ime::Preedit` carried
    /// non-empty text. Cleared by an empty preedit, a commit, or a
    /// disable event. Gates `dispatch_key_event` on every target
    /// except Windows (see module docs, "Why two states").
    has_preedit: bool,
    /// `true` while the IME lifecycle is open: set by `Ime::Enabled`,
    /// cleared by `Ime::Disabled`. Gates `dispatch_key_event` on
    /// Windows, where the pair delimits exactly one composition (see
    /// module docs, "Why two states").
    ime_enabled: bool,
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
        // The IME purpose (`Normal`) rides along in the Enable request
        // issued by `with_handle` → `set_ime_allowed(true)`.
        Ok(Self::with_handle(Box::new(WinitWindowHandle(window))))
    }

    /// Test entry: build a bridge against an arbitrary [`BridgeWindow`]
    /// implementation (a mock from the test module).
    pub(crate) fn with_handle(window: Box<dyn BridgeWindow>) -> Self {
        window.set_ime_allowed(true);
        Self {
            window,
            has_preedit: false,
            ime_enabled: false,
            queue: VecDeque::new(),
            last_cursor_area: None,
        }
    }

    /// Push a winit `WindowEvent::Ime` payload through the bridge.
    /// Translates each variant into the appropriate [`ImeEvent`] and
    /// updates `has_preedit` / `ime_enabled`. Total over every variant;
    /// never panics.
    pub fn on_winit_ime(&mut self, ime: &WinitIme) {
        match ime {
            WinitIme::Enabled => {
                // The IME lifecycle opened. On every non-Windows
                // target this spans the whole focus duration (see
                // module docs), so it must not gate key suppression by
                // itself — has_preedit is left untouched.
                self.ime_enabled = true;
            }
            WinitIme::Preedit(text, _cursor) => {
                self.has_preedit = !text.is_empty();
                // Always mirror the preedit, including empty, so the
                // overlay clears (no change to the neutral event
                // queue's shape).
                self.queue.push_back(ImeEvent::Preedit(text.clone()));
            }
            WinitIme::Commit(text) => {
                self.has_preedit = false;
                if !text.is_empty() {
                    self.queue.push_back(ImeEvent::Commit(text.clone()));
                }
            }
            WinitIme::Disabled => {
                // IM server detached (user toggled IME off, focus
                // lost from compositor's POV, fcitx5/ibus crashed).
                // Close the lifecycle, clear the preedit state, and
                // signal the App to drop any preedit overlay.
                self.has_preedit = false;
                self.ime_enabled = false;
                self.queue.push_back(ImeEvent::FocusOut);
            }
            WinitIme::DeleteSurrounding { .. } => {
                // New in winit 0.31: the IM server asks the editor to
                // delete text surrounding the cursor/selection. The
                // existing Ghostty-derived pipeline (Preedit/Commit/
                // Enabled/Disabled) has no editor-side hook for this
                // yet, so it is a documented no-op rather than a
                // migration regression — wiring it up is out of scope
                // for the winit version bump. Leaves has_preedit /
                // ime_enabled unchanged.
            }
        }
    }
}

/// Pure key-suppression gate (FR11). Total over all eight
/// `(has_preedit, ime_enabled, windows_gate)` combinations: passing `true`
/// for `windows_gate` selects the Windows rule, and the answer then equals
/// `ime_enabled` alone — `has_preedit` never influences it; passing `false`
/// selects the rule for every other target, and the answer then equals
/// `has_preedit` alone — `ime_enabled` never influences it.
///
/// [`WinitImeBridge::dispatch_key_event`] is the only production caller,
/// passing `cfg!(windows)` as `windows_gate`. The selector exists as a
/// runtime parameter (not a `#[cfg]` branch) precisely so unit tests can
/// drive *both* platform rules from a single development host (NFR3) —
/// pass a literal `true` or `false` regardless of the host's actual target.
///
/// Constant-time boolean read, no allocation, no locking (NFR1).
fn should_suppress_key(has_preedit: bool, ime_enabled: bool, windows_gate: bool) -> bool {
    if windows_gate {
        ime_enabled
    } else {
        has_preedit
    }
}

impl ImeBackend for WinitImeBridge {
    fn dispatch_key_event(&mut self, _raw: &RawKeyEvent) -> KeyDispatchResult {
        // While the IM server owns the key (per the platform's gate,
        // see module docs "Why two states" and `should_suppress_key`)
        // we must not emit the key bytes ourselves. winit already
        // suppresses `KeyEvent::text` during composition, but we still
        // need to block the named-key fallback path (Enter / arrows /
        // etc. are sometimes routed through the IM server even though
        // winit gives us the KeyEvent first).
        //
        // NFR1: constant-time boolean read, no allocation, no locking,
        // delegated to `should_suppress_key` (FR11) so the gate logic
        // itself — both platform branches — is exercisable by unit
        // tests on any host.
        if should_suppress_key(self.has_preedit, self.ime_enabled, cfg!(windows)) {
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
        if !focused {
            // FR10: `Ime::Disabled` is otherwise the only event that
            // clears `ime_enabled` (see module docs, "Why two
            // states"), but winit-win32's `ImeRequest::Disable` arm
            // (`winit-win32/src/window.rs`) returns without emitting
            // `Ime::Disabled`, unlike winit-wayland which synthesizes
            // it from the same request
            // (`winit-wayland/src/window/mod.rs`). A composition
            // interrupted by focus loss on Windows could therefore
            // latch the gate open and suppress every subsequent key,
            // with no second escape route (the commit event
            // deliberately does not clear `ime_enabled` either).
            // Clearing both states locally here makes the bridge's
            // own gate independent of that asymmetry; on Wayland/X11
            // the synthesized `Ime::Disabled` still follows and finds
            // both states already false (idempotent — no additional
            // neutral event is queued from this path). Focus GAIN
            // must not modify either state.
            self.has_preedit = false;
            self.ime_enabled = false;
        }
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
    // Non-Windows only: the Windows gate is `ime_enabled`, which neither
    // `Preedit` nor `Commit` changes here, so both dispatch assertions
    // below only describe non-Windows semantics.
    #[test]
    #[cfg(not(windows))]
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
        // has_preedit is already false; a follow-up Disabled must
        // still surface FocusOut exactly once and leave both states
        // false.
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
        // A bare empty preedit with no preceding Enable leaves both
        // states false: has_preedit stays false (no non-empty preedit
        // was ever observed) and ime_enabled stays false (no Enable
        // event fired). dispatch_key_event is Passthrough on every
        // target here, but for different reasons per platform —
        // non-Windows checks has_preedit, Windows checks ime_enabled,
        // and both happen to be false.
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

    // ── TS-1 (SKK regression, all targets): Enabled → Preedit("▽A") →
    //    dispatch is Consumed on every target (Windows via the open
    //    lifecycle, everywhere else via the non-empty preedit) →
    //    Preedit("") → dispatch is Consumed on Windows (the lifecycle is
    //    still open) and Passthrough elsewhere. Both preedits reach pump
    //    in order regardless of platform (FR2, platform-independent).
    #[test]
    fn enabled_then_non_empty_then_empty_preedit_matches_ts1_on_every_target() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("▽A".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed,
            "dispatch must be consumed while the non-empty preedit is current, on every target"
        );
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        let expected_after_empty = if cfg!(windows) {
            KeyDispatchResult::Consumed
        } else {
            KeyDispatchResult::Passthrough
        };
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            expected_after_empty,
            "Windows keeps suppressing inside the still-open lifecycle; every other target passes through"
        );
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(
            out,
            vec![
                ImeEvent::Preedit("▽A".into()),
                ImeEvent::Preedit(String::new()),
            ]
        );
    }

    // ── TS-5 (X11 ambiguous start/end shape, non-Windows only):
    //    SPEC.md assumption A3 — winit-x11 emits an empty Ime::Preedit
    //    for both composition start and composition end. This sequence
    //    exercises that ambiguous shape end to end: passthrough while no
    //    preedit has been observed yet, consumed once the preedit is
    //    non-empty, passthrough again once it goes back to empty.
    #[cfg(not(windows))]
    #[test]
    fn x11_ambiguous_empty_preedit_start_and_end_toggles_dispatch() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough,
            "leading empty preedit (X11 composition-start shape) must not suppress keys"
        );
        b.on_winit_ime(&WinitIme::Preedit("あ".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed,
            "non-empty preedit must suppress keys"
        );
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough,
            "trailing empty preedit (X11 composition-end shape) must unblock dispatch again"
        );
    }

    // ── AC-2: a subsequent non-empty preedit re-engages suppression on
    //          non-Windows targets.
    #[cfg(not(windows))]
    #[test]
    fn preedit_after_empty_reengages_dispatch_on_non_windows() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        b.on_winit_ime(&WinitIme::Preedit("y".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed
        );
    }

    // ── AC-3: Enable alone never causes Consumed on non-Windows
    //          targets (Wayland/X11 fire it for the whole focus
    //          lifetime, spanning ordinary direct input).
    #[cfg(not(windows))]
    #[test]
    fn enable_alone_never_consumes_on_non_windows() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough
        );
    }

    // ── Edge case: a commit arriving directly after an empty preedit
    //    keeps the queue ordered preedit, empty preedit, commit, and
    //    still unblocks dispatch on non-Windows targets.
    #[cfg(not(windows))]
    #[test]
    fn commit_after_empty_preedit_orders_queue_and_unblocks_dispatch_on_non_windows() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        b.on_winit_ime(&WinitIme::Commit("X".into()));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough
        );
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(
            out,
            vec![
                ImeEvent::Preedit("x".into()),
                ImeEvent::Preedit(String::new()),
                ImeEvent::Commit("X".into()),
            ]
        );
    }

    // ── AC-6 (non-Windows halves): DeleteSurrounding leaves the
    //    dispatch answer exactly as it was, whether a preedit is
    //    currently present or absent.
    #[cfg(not(windows))]
    #[test]
    fn delete_surrounding_leaves_dispatch_unchanged_when_preedit_present_non_windows() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.on_winit_ime(&WinitIme::DeleteSurrounding {
            before_bytes: 1,
            after_bytes: 0,
        });
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn delete_surrounding_leaves_dispatch_unchanged_when_preedit_absent_non_windows() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::DeleteSurrounding {
            before_bytes: 1,
            after_bytes: 0,
        });
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough
        );
    }

    // ── TS-10 / AC-1: predicate truth table. Total over all eight
    //    (has_preedit, ime_enabled, windows_gate) combinations: for
    //    windows_gate = true the answer is exactly ime_enabled
    //    regardless of has_preedit, and for windows_gate = false the
    //    answer is exactly has_preedit regardless of ime_enabled. Runs
    //    on every host — this is what pins FR11's platform selector as
    //    a runtime argument rather than a compile-time branch.
    #[test]
    fn should_suppress_key_truth_table_is_total_over_all_combinations() {
        for has_preedit in [false, true] {
            for ime_enabled in [false, true] {
                assert_eq!(
                    should_suppress_key(has_preedit, ime_enabled, true),
                    ime_enabled,
                    "windows_gate=true must answer exactly ime_enabled \
                     (has_preedit={has_preedit}, ime_enabled={ime_enabled})"
                );
                assert_eq!(
                    should_suppress_key(has_preedit, ime_enabled, false),
                    has_preedit,
                    "windows_gate=false must answer exactly has_preedit \
                     (has_preedit={has_preedit}, ime_enabled={ime_enabled})"
                );
            }
        }
    }

    // ── TS-6 / TS-11 (Windows empty active composition, asserted
    //    through the predicate so it executes on every host, not only
    //    when compiled for the Windows target): an empty preedit inside
    //    a live composition still suppresses under the Windows gate,
    //    because the gate is the lifecycle flag, not preedit emptiness.
    //    Passthrough again once Disabled closes the lifecycle.
    #[test]
    fn windows_gate_empty_preedit_inside_live_composition_still_suppresses() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        assert!(
            should_suppress_key(b.has_preedit, b.ime_enabled, true),
            "Windows gate must still suppress: the lifecycle is open even though the preedit is empty"
        );
        b.on_winit_ime(&WinitIme::Disabled);
        assert!(
            !should_suppress_key(b.has_preedit, b.ime_enabled, true),
            "Disabled closes the lifecycle, so the Windows gate must release"
        );
    }

    // ── TS-7 / TS-11 (Windows commit does not end the lifecycle,
    //    asserted through the predicate so it executes on every host):
    //    a commit occurring mid-composition does not release
    //    suppression before Disabled closes the lifecycle.
    #[test]
    fn windows_gate_commit_inside_live_composition_does_not_unblock() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        b.on_winit_ime(&WinitIme::Commit("X".into()));
        assert!(
            should_suppress_key(b.has_preedit, b.ime_enabled, true),
            "a commit mid-composition must not release the Windows gate"
        );
        b.on_winit_ime(&WinitIme::Disabled);
        assert!(
            !should_suppress_key(b.has_preedit, b.ime_enabled, true),
            "Disabled must release the Windows gate"
        );
    }

    // ── AC-6 (Windows half, asserted through the predicate so it
    //    executes on every host): DeleteSurrounding is state-neutral on
    //    the Windows gate too, whether a composition is currently open
    //    or not.
    #[test]
    fn windows_gate_delete_surrounding_leaves_state_unchanged_when_preedit_present() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.on_winit_ime(&WinitIme::DeleteSurrounding {
            before_bytes: 1,
            after_bytes: 0,
        });
        assert!(should_suppress_key(b.has_preedit, b.ime_enabled, true));
    }

    #[test]
    fn windows_gate_delete_surrounding_leaves_state_unchanged_when_preedit_absent() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::DeleteSurrounding {
            before_bytes: 1,
            after_bytes: 0,
        });
        assert!(!should_suppress_key(b.has_preedit, b.ime_enabled, true));
    }

    // ── TS-12 / AC-4: losing focus mid-composition clears both gate
    //    states, so the predicate answers passthrough for BOTH platform
    //    selectors — not just the host's own target.
    #[test]
    fn focus_loss_clears_gate_for_both_platform_selectors() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.notify_focus(false);
        assert!(
            !should_suppress_key(b.has_preedit, b.ime_enabled, true),
            "focus loss must release the Windows gate even mid-composition"
        );
        assert!(
            !should_suppress_key(b.has_preedit, b.ime_enabled, false),
            "focus loss must release the non-Windows gate too"
        );
    }

    // ── TS-13 / AC-5: focus gain on a freshly built bridge does not
    //    open the gate — both states stay false, so the predicate
    //    answers passthrough for both platform selectors. The sequence
    //    assertion in `notify_focus_propagates_to_set_ime_allowed` is
    //    unchanged by this test.
    #[test]
    fn focus_gain_on_fresh_bridge_leaves_gate_closed_for_both_platform_selectors() {
        let (mut b, _mock) = make_bridge();
        b.notify_focus(true);
        assert!(!should_suppress_key(b.has_preedit, b.ime_enabled, true));
        assert!(!should_suppress_key(b.has_preedit, b.ime_enabled, false));
    }

    // ── task0004 AC-1 / TS-6 dispatch counterpart (Windows target only):
    //    `windows_gate_empty_preedit_inside_live_composition_still_suppresses`
    //    above proves the RULE — that the predicate suppresses when
    //    `windows_gate = true` and the preedit is empty inside an open
    //    lifecycle. This test proves the WIRING — that
    //    `dispatch_key_event` itself reaches that answer through
    //    `cfg!(windows)`, not just that the predicate can be made to
    //    answer it when driven directly. A predicate-only pair would
    //    still pass if `dispatch_key_event` hardcoded `windows_gate =
    //    false`; only a test that calls `dispatch_key_event` under a
    //    real `cfg(windows)` build can catch that.
    #[test]
    #[cfg(windows)]
    fn windows_dispatch_empty_preedit_inside_live_composition_still_suppresses() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed,
            "dispatch_key_event must still suppress: the lifecycle is open even though the preedit is empty"
        );
        b.on_winit_ime(&WinitIme::Disabled);
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough,
            "Disabled closes the lifecycle, so dispatch_key_event must release"
        );
    }

    // ── task0004 AC-2 / TS-7 dispatch counterpart (Windows target only):
    //    `windows_gate_commit_inside_live_composition_does_not_unblock`
    //    above proves the RULE — that a commit mid-composition does not
    //    release the predicate when `windows_gate = true`. This test
    //    proves the WIRING — that `dispatch_key_event` reaches that same
    //    answer through `cfg!(windows)` rather than a hardcoded or
    //    miswired selector.
    #[test]
    #[cfg(windows)]
    fn windows_dispatch_commit_inside_live_composition_does_not_unblock() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
        b.on_winit_ime(&WinitIme::Preedit("".into(), None));
        b.on_winit_ime(&WinitIme::Commit("X".into()));
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Consumed,
            "a commit mid-composition must not release dispatch_key_event's suppression"
        );
        b.on_winit_ime(&WinitIme::Disabled);
        assert_eq!(
            b.dispatch_key_event(&raw_press()),
            KeyDispatchResult::Passthrough,
            "Disabled must release dispatch_key_event's suppression"
        );
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
