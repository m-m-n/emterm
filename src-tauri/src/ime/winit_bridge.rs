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
//! ## Deferred flush (task0001, windows-skk-ime-hang)
//!
//! `BridgeWindow::set_ime_allowed` / `set_ime_cursor_area` are never
//! called synchronously from an event-dispatch method
//! (`with_handle` / `notify_focus` / `notify_cursor_rect`). Those
//! methods only *record* the intent — last-writer-wins for the allow
//! state, last-writer-wins plus the existing dedup for the cursor rect
//! — into `pending_allow` / `pending_cursor_area`. [`Self::flush`]
//! (the `ImeBackend::flush` override) is the only place, besides
//! `Drop`, that actually calls into `window`. `window_host` calls it
//! once per `about_to_wait` turn, which runs between winit's message
//! dispatches rather than inside one. This matters concretely on
//! Windows: `winit-win32` executes `request_ime_update` inline on the
//! event-loop thread, and that call exchanges synchronous messages
//! with IMM32 — issuing it from inside wndproc dispatch (as the
//! pre-task0001 code did from `with_handle` / `notify_focus`) is the
//! identified CorvusSKK deadlock mechanism (SPEC.md "Background
//! Analysis"). `Drop` keeps calling the detach directly and discards
//! any pending requests, because teardown is not part of event
//! dispatch (SPEC FR3) and a bridge about to be destroyed has no
//! future flush to run them.
//!
//! ## Windows IMM32-direct cursor area + deferred detach (task0001,
//! windows-imm32-ime-direct)
//!
//! Two further changes narrow the "queued intent → platform call" gap
//! specifically for Windows:
//!
//! 1. **Cursor-area routing split.** `WinitWindowHandle::set_ime_cursor_area`
//!    has two `#[cfg]`'d bodies. Every target except Windows keeps the
//!    winit-routed `ImeRequest::Update` call, byte-for-byte unchanged
//!    (SPEC FR5). On Windows it instead calls IMM32 directly —
//!    `ImmGetContext` → `ImmSetCompositionWindow` + `ImmSetCandidateWindow`
//!    → `ImmReleaseContext` — against the window's raw `HWND`
//!    (`raw_window_handle::HasWindowHandle`), never touching
//!    `request_ime_update` (SPEC FR1/FR2/FR6). `set_ime_allowed` (the
//!    enable/disable call) is unaffected and stays winit-routed on every
//!    target: winit gates all `WM_IME_*` processing on its own
//!    IME-capabilities state, so bypassing Enable would stop
//!    `WindowEvent::Ime` delivery entirely (SPEC FR4). A missing or
//!    non-Win32 window handle, or a null IMM32 context, is a silent
//!    no-op — no logging, no retry.
//!
//! 2. **Deferred detach — Windows only (SPEC FR5 / AC6).** `flush` holds
//!    a pending DISABLE (`pending_allow == Some(false)`) instead of
//!    delivering it while a composition is still open, tracked by the
//!    new `composition_alive` field, but only when `cfg!(windows)` is
//!    true. Every other target keeps the pre-task0001 behavior
//!    byte-for-byte: a pending DISABLE is delivered on the same flush
//!    regardless of `composition_alive`. This gate exists because X11
//!    has no observable event that could ever satisfy the hold's release
//!    condition — `winit-x11/src/ime/mod.rs` only emits `Ime::Disabled`
//!    from inside `create_context`, itself reachable only from the
//!    pending-DISABLE request the hold would be suppressing, so an
//!    ungated hold would never clear on X11. Like
//!    [`should_suppress_key`], the selector is a runtime parameter
//!    (`windows_gate`) on the pure predicate
//!    [`hold_pending_detach`], not a `#[cfg]` branch, so both platform
//!    rules stay unit-testable from a single development host.
//!    `composition_alive` is deliberately distinct from `ime_enabled`:
//!    `notify_focus(false)` clears `ime_enabled` immediately (FR10,
//!    below), which would make the composition look closed before the
//!    real `Ime::Disabled` arrives; `composition_alive` is set only by
//!    `Ime::Enabled` and cleared only by `Ime::Disabled`, so it survives
//!    focus loss. A held detach never blocks a pending cursor-area call
//!    in the same flush; an ENABLE is never held; a focus-in recorded
//!    while a detach is held overwrites `pending_allow`
//!    (last-writer-wins), so the detach is then never delivered. If
//!    `Ime::Disabled` never arrives the detach stays held indefinitely —
//!    an accepted, SPEC-settled failure mode with no timeout machinery,
//!    and one that can only occur on Windows given the gate above.
//!    `Drop` is unchanged: it still calls `set_ime_allowed(false)`
//!    directly and discards any pending state regardless of
//!    `composition_alive` — a known residual hole for a bridge swapped
//!    mid-composition, deliberately out of scope.
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
    /// events to this window. Called only from [`WinitImeBridge`]'s
    /// `flush` (the recorded intent from `init` / `notify_focus`) and
    /// from its `Drop` (direct detach; see module docs "Deferred
    /// flush").
    fn set_ime_allowed(&self, allowed: bool);
    /// Inform the IM server where the active cursor cell currently
    /// sits, in physical pixels. Drives the candidate window position.
    /// On the production Windows sink this bypasses winit and talks to
    /// IMM32 directly (task0001, windows-imm32-ime-direct); every other
    /// target stays winit-routed.
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

    /// Non-Windows targets: winit-routed cursor-area delivery, unchanged
    /// from the pre-task0001 behavior (SPEC FR5).
    #[cfg(not(windows))]
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

    /// Windows: IMM32-direct cursor-area delivery (SPEC FR1/FR2/FR6),
    /// bypassing `request_ime_update` entirely so the call never
    /// contends with winit-win32's window-state mutex (module docs,
    /// "Windows IMM32-direct cursor area + deferred detach" — that lock
    /// being held across an equivalent call is the identified CorvusSKK
    /// deadlock mechanism). Decision-free executor: every branch above
    /// (when to call this at all) lives in bridge state exercised by
    /// host-run unit tests (task plan Design §1); this method has no
    /// host-runnable test by design and is gated by the windows
    /// cross-target compile check (AC-6 gate 3) plus the feature-level
    /// real-device manual scenario (VERIFICATION TS5).
    #[cfg(windows)]
    fn set_ime_cursor_area(&self, x: i32, y: i32, width: i32, height: i32) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
        use windows_sys::Win32::UI::Input::Ime::{
            CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM, ImmGetContext,
            ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow,
        };

        // FR6 step 1: obtain the HWND via raw-window-handle. Any
        // unavailable or non-Win32 handle is a silent no-op — same
        // posture as the null-context case below.
        let Ok(handle) = self.0.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let hwnd = win32.hwnd.get() as HWND;

        // FR2 steps 2-3: acquire the input-method context; a null
        // context is the defined error case — return without any
        // further IMM32 call.
        let himc = unsafe { ImmGetContext(hwnd) };
        if himc.is_null() {
            return;
        }

        let rect = RECT {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
        let composition = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT { x, y: y + height },
            rcArea: rect,
        };
        let candidate = CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: POINT { x, y },
            rcArea: rect,
        };
        // FR2 steps 4-6: set the composition window, then the candidate
        // window, then release the context — always paired with the
        // successful acquisition above, regardless of the outcome of
        // the two set calls.
        unsafe {
            ImmSetCompositionWindow(himc, &composition);
            ImmSetCandidateWindow(himc, &candidate);
            ImmReleaseContext(hwnd, himc);
        }
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
    /// `true` while a composition is open on the platform's own terms:
    /// set by `Ime::Enabled`, cleared ONLY by `Ime::Disabled` (task0001
    /// "Deferred detach", module docs). Deliberately distinct from
    /// `ime_enabled` — `notify_focus(false)` clears `ime_enabled`
    /// immediately (FR10) but must NOT clear this, or a detach held
    /// during a live composition would look safe to deliver before the
    /// real `Ime::Disabled` arrives. `flush` reads this to decide
    /// whether a pending DISABLE must be held rather than delivered.
    composition_alive: bool,
    /// Events produced by `on_winit_ime` waiting for the next
    /// `pump` drain.
    queue: VecDeque<ImeEvent>,
    /// Last cursor rect *recorded* for the window (physical pixels,
    /// regardless of whether it has been flushed yet). Used to
    /// suppress redundant recordings of `set_ime_cursor_area` calls
    /// (SPEC.md FR7); independent of `pending_cursor_area`, which
    /// tracks only what the *next* flush still has to deliver.
    last_cursor_area: Option<(i32, i32, i32, i32)>,
    /// Recorded allow-state request awaiting the next flush.
    /// Last-writer-wins: a second `notify_focus` (or the constructor's
    /// initial enable) before any flush overwrites this rather than
    /// queuing both. `None` means nothing to flush.
    pending_allow: Option<bool>,
    /// Recorded cursor-area request awaiting the next flush.
    /// Last-writer-wins, same as `pending_allow`. `None` means nothing
    /// to flush.
    pending_cursor_area: Option<(i32, i32, i32, i32)>,
    /// Latch (FR4): set once a `WinitIme::Enabled` arrives while
    /// `ime_enabled` was already `true`, so the anomaly is logged only
    /// on its first occurrence.
    enabled_anomaly_warned: bool,
    /// Latch (FR4): set once a `WinitIme::Disabled` arrives while
    /// `ime_enabled` was already `false`, so the anomaly is logged
    /// only on its first occurrence.
    disabled_anomaly_warned: bool,
}

impl WinitImeBridge {
    /// Build a bridge attached to a winit window. The returned bridge
    /// has *recorded* an allow-state enable request so the first
    /// `flush` will call `set_ime_allowed(true)` and winit starts
    /// surfacing `WindowEvent::Ime` events (see module docs "Deferred
    /// flush" — construction runs during `can_create_surfaces`, itself
    /// inside event dispatch, so it must not call the window
    /// directly).
    pub fn init(window: Arc<dyn Window>) -> Result<Self, ImeInitError> {
        // The IME purpose (`Normal`) rides along in the Enable request
        // issued when the recorded `pending_allow = Some(true)` is
        // flushed.
        Ok(Self::with_handle(Box::new(WinitWindowHandle(window))))
    }

    /// Test entry: build a bridge against an arbitrary [`BridgeWindow`]
    /// implementation (a mock from the test module).
    pub(crate) fn with_handle(window: Box<dyn BridgeWindow>) -> Self {
        Self {
            window,
            has_preedit: false,
            ime_enabled: false,
            composition_alive: false,
            queue: VecDeque::new(),
            last_cursor_area: None,
            pending_allow: Some(true),
            pending_cursor_area: None,
            enabled_anomaly_warned: false,
            disabled_anomaly_warned: false,
        }
    }

    /// Push a winit `WindowEvent::Ime` payload through the bridge.
    /// Translates each variant into the appropriate [`ImeEvent`] and
    /// updates `has_preedit` / `ime_enabled`. Total over every variant;
    /// never panics.
    pub fn on_winit_ime(&mut self, ime: &WinitIme) {
        match ime {
            WinitIme::Enabled => {
                // FR4 anomaly diagnostics: winit told us the lifecycle
                // opened while our own bookkeeping already considered
                // it open — a signal the platform's Enabled/Disabled
                // pairing assumption (module docs, "Why two states")
                // may not hold on this host. Latched: only the first
                // occurrence logs.
                if self.ime_enabled && !self.enabled_anomaly_warned {
                    log::warn!(
                        "ime: winit sent Ime::Enabled while the bridge already \
                         considered the IME lifecycle open (see task0001 SPEC FR4)"
                    );
                    self.enabled_anomaly_warned = true;
                }
                // The IME lifecycle opened. On every non-Windows
                // target this spans the whole focus duration (see
                // module docs), so it must not gate key suppression by
                // itself — has_preedit is left untouched.
                self.ime_enabled = true;
                // task0001 "Deferred detach": a composition is now
                // alive on the platform's own terms. Unlike
                // `ime_enabled`, this is never cleared by focus loss —
                // only by the matching `Ime::Disabled` below.
                self.composition_alive = true;
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
                // FR4 anomaly diagnostics: winit told us the lifecycle
                // closed while our own bookkeeping already considered
                // it closed. Latched: only the first occurrence logs.
                if !self.ime_enabled && !self.disabled_anomaly_warned {
                    log::warn!(
                        "ime: winit sent Ime::Disabled while the bridge already \
                         considered the IME lifecycle closed (see task0001 SPEC FR4)"
                    );
                    self.disabled_anomaly_warned = true;
                }
                // IM server detached (user toggled IME off, focus
                // lost from compositor's POV, fcitx5/ibus crashed).
                // Close the lifecycle, clear the preedit state, and
                // signal the App to drop any preedit overlay.
                self.has_preedit = false;
                self.ime_enabled = false;
                // task0001 "Deferred detach": the composition is now
                // actually closed — the next flush may deliver a held
                // detach (see `flush`).
                self.composition_alive = false;
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

/// Pure deferred-detach hold gate (SPEC FR5 / AC6). `true` means `flush`
/// must leave a pending DISABLE in `pending_allow` rather than deliver
/// it. Mirrors [`should_suppress_key`]'s design: `windows_gate` is a
/// runtime parameter, not a `#[cfg]` branch, so unit tests can drive
/// both platform rules from a single development host.
///
/// [`WinitImeBridge::flush`] is the only production caller, passing
/// `cfg!(windows)` as `windows_gate`. Only Windows can ever hold: the
/// module docs ("Windows IMM32-direct cursor area + deferred detach")
/// explain why an ungated hold would never clear on X11.
fn hold_pending_detach(allowed: bool, composition_alive: bool, windows_gate: bool) -> bool {
    windows_gate && !allowed && composition_alive
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
        // see noise on every frame when the cursor is stationary. This
        // dedup is against the last *recorded* rect, independent of
        // whether it has been flushed yet — repeated identical calls
        // between two flushes still record nothing after the first.
        let next = (x_px, y_px, w_px, h_px);
        if self.last_cursor_area == Some(next) {
            return;
        }
        self.last_cursor_area = Some(next);
        self.pending_cursor_area = Some(next);
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
        // FR8: record the allow-state intent so the platform IM server
        // attaches on focus-in and detaches on focus-out once flushed.
        // Deferred flush (module docs): last-writer-wins, no call to
        // `window` here. If the window already saw `Ime::Disabled`
        // from a focus-loss path, re-recording `allowed` here is still
        // idempotent once flushed.
        self.pending_allow = Some(focused);
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

    fn flush(&mut self) {
        // Deferred flush (module docs): the only place besides `Drop`
        // that calls into `window`. Order matters (SPEC AC-4): the
        // allow-state request must land before the cursor-area request
        // so a freshly enabled IM server already has somewhere to draw
        // the candidate window, matching the pre-task0001 enable→seed
        // order. At most one call per kind; nothing recorded means no
        // call and no allocation.
        //
        // task0001 "Deferred detach": a pending DISABLE
        // (`allowed == false`) is held — left in `pending_allow`, not
        // delivered — while `composition_alive` is still true; delivering
        // it now would detach the IM server mid-composition. An ENABLE
        // is never held. Holding does not take `pending_allow`, so a
        // later flush (once `Ime::Disabled` closes the composition, or a
        // focus-in overwrites the pending state first — last-writer-wins)
        // resolves it.
        if let Some(allowed) = self.pending_allow {
            let held = hold_pending_detach(allowed, self.composition_alive, cfg!(windows));
            if !held {
                self.pending_allow = None;
                self.window.set_ime_allowed(allowed);
            }
        }
        if let Some((x, y, w, h)) = self.pending_cursor_area.take() {
            self.window.set_ime_cursor_area(x, y, w, h);
        }
    }
}

impl Drop for WinitImeBridge {
    fn drop(&mut self) {
        // SPEC FR3 / AC-6: any pending allow-state or cursor-area
        // request is discarded here (never flushed) — a bridge about
        // to be destroyed has no future flush turn to run them, and
        // teardown is not part of event dispatch, so calling `window`
        // directly is safe. Detach from the IM server explicitly:
        // winit also drops the IC when the window is destroyed, but
        // releasing it eagerly avoids leaving a stale focus claim if
        // the bridge is replaced mid-session (e.g. switching to
        // NullBackend after a settings change).
        self.window.set_ime_allowed(false);
    }
}

#[cfg(test)]
mod tests;
