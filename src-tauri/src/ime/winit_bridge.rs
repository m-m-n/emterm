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
//! 2. **Deferred detach.** `flush` now holds a pending DISABLE
//!    (`pending_allow == Some(false)`) instead of delivering it while a
//!    composition is still open, tracked by the new `composition_alive`
//!    field. `composition_alive` is deliberately distinct from
//!    `ime_enabled`: `notify_focus(false)` clears `ime_enabled`
//!    immediately (FR10, below), which would make the composition look
//!    closed before the real `Ime::Disabled` arrives; `composition_alive`
//!    is set only by `Ime::Enabled` and cleared only by `Ime::Disabled`,
//!    so it survives focus loss. A held detach never blocks a pending
//!    cursor-area call in the same flush; an ENABLE is never held; a
//!    focus-in recorded while a detach is held overwrites `pending_allow`
//!    (last-writer-wins), so the detach is then never delivered. If
//!    `Ime::Disabled` never arrives the detach stays held indefinitely —
//!    an accepted, SPEC-settled failure mode with no timeout machinery.
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
            let held = !allowed && self.composition_alive;
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

    #[derive(Default, Debug, Clone)]
    struct MockWindowInner {
        allowed_calls: Vec<bool>,
        cursor_calls: Vec<(i32, i32, i32, i32)>,
        /// Interleaved record of which kind fired, in call order —
        /// `allowed_calls` / `cursor_calls` alone lose the relative
        /// ordering between the two kinds (AC-4 / SPEC TS-3).
        call_order: Vec<CallKind>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CallKind {
        Allowed,
        Cursor,
    }

    impl BridgeWindow for MockWindow {
        fn set_ime_allowed(&self, allowed: bool) {
            let mut inner = self.inner.lock().unwrap();
            inner.allowed_calls.push(allowed);
            inner.call_order.push(CallKind::Allowed);
        }
        fn set_ime_cursor_area(&self, x: i32, y: i32, w: i32, h: i32) {
            let mut inner = self.inner.lock().unwrap();
            inner.cursor_calls.push((x, y, w, h));
            inner.call_order.push(CallKind::Cursor);
        }
    }

    impl MockWindow {
        /// Clone the recorded state out from behind the lock and drop
        /// the guard immediately. Tests must assert against the
        /// snapshot, never against a `MutexGuard` held across
        /// `assert_eq!` — if the assertion panics while the guard is
        /// still alive the mutex is left poisoned, and `WinitImeBridge`'s
        /// `Drop` locking the same mutex during unwind turns one failed
        /// assertion into a full test-binary abort (a real failure mode
        /// hit while red-checking this suite; see task0001 report).
        fn snapshot(&self) -> MockWindowInner {
            self.inner.lock().unwrap().clone()
        }
        /// Reset the recorded state, e.g. to discharge the
        /// constructor's initial recorded enable before asserting on a
        /// later turn's calls.
        fn reset(&self) {
            *self.inner.lock().unwrap() = MockWindowInner::default();
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

    // ── TS-winit-7 / AC-3 (SPEC TS-2): notify_cursor_rect dedups
    //    identical rects; the same-rect dedup keeps working across a
    //    flush boundary (each distinct rect reaches the window exactly
    //    once, duplicates in between reach it zero times).
    #[test]
    fn cursor_rect_is_deduplicated_after_flush() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        b.notify_cursor_rect(10, 20, 9, 18);
        b.flush();
        b.notify_cursor_rect(10, 20, 9, 18); // exact same → not even recorded
        b.flush(); // → no-op, nothing pending
        b.notify_cursor_rect(11, 20, 9, 18); // moved → recorded, then flushed
        b.flush();
        let inner = mock.snapshot();
        assert_eq!(
            inner.cursor_calls,
            vec![(10, 20, 9, 18), (11, 20, 9, 18)],
            "duplicate calls must be suppressed"
        );
    }

    // ── AC-3 (SPEC TS-2): multiple notify_cursor_rect calls recorded
    //    before a single flush coalesce into exactly one
    //    set_ime_cursor_area call carrying the LAST recorded rect.
    #[test]
    fn cursor_rect_multiple_records_before_flush_coalesce_to_last() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        b.notify_cursor_rect(10, 20, 9, 18);
        b.notify_cursor_rect(10, 20, 9, 18); // dup → not recorded again
        b.notify_cursor_rect(11, 20, 9, 18); // moved → overwrites pending
        b.notify_cursor_rect(11, 20, 9, 18); // dup of the new rect → not recorded again
        b.flush();
        let inner = mock.snapshot();
        assert_eq!(
            inner.cursor_calls,
            vec![(11, 20, 9, 18)],
            "several pending updates before one flush must coalesce to a single call"
        );
    }

    // ── AC-7 (SPEC TS-6): construction-time enable is recorded, not
    //    called immediately — the window sees nothing until the first
    //    flush, then exactly `[true]`.
    #[test]
    fn init_records_allow_true_and_flushes_it_on_first_flush() {
        let (mut b, mock) = make_bridge();
        assert!(
            mock.snapshot().allowed_calls.is_empty(),
            "construction must only record the enable, not call the window yet"
        );
        b.flush();
        assert_eq!(mock.snapshot().allowed_calls, vec![true]);
    }

    // ── AC-2 (SPEC TS-1): notify_focus(true) records; no window call
    //    happens until flush, and flush produces exactly one call.
    #[test]
    fn notify_focus_true_records_until_flush_then_calls_exactly_once() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        let before = mock.snapshot().allowed_calls.len();
        b.notify_focus(true);
        assert_eq!(
            mock.snapshot().allowed_calls.len(),
            before,
            "notify_focus must not reach the window before flush"
        );
        b.flush();
        assert_eq!(&mock.snapshot().allowed_calls[before..], &[true]);
    }

    #[test]
    fn notify_focus_propagates_to_set_ime_allowed_after_flush() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        b.notify_focus(false);
        b.flush();
        b.notify_focus(true);
        b.flush();
        assert_eq!(mock.snapshot().allowed_calls, vec![true, false, true]);
    }

    // ── AC-4 (SPEC TS-3): allow-state and cursor-area recorded in the
    //    same turn flush in order allow → cursor.
    #[test]
    fn flush_orders_allow_before_cursor_within_same_turn() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        mock.reset();
        b.notify_focus(true);
        b.notify_cursor_rect(5, 6, 7, 8);
        b.flush();
        assert_eq!(
            mock.snapshot().call_order,
            vec![CallKind::Allowed, CallKind::Cursor]
        );
    }

    // ── AC-5 (SPEC TS-4): a flush with nothing recorded makes no calls
    //    at all, of either kind.
    #[test]
    fn flush_with_nothing_recorded_makes_no_calls() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        let before = mock.snapshot();
        b.flush(); // nothing new recorded since the previous flush
        let after = mock.snapshot();
        assert_eq!(after.allowed_calls.len(), before.allowed_calls.len());
        assert_eq!(after.cursor_calls.len(), before.cursor_calls.len());
    }

    // ── AC-6 (SPEC TS-5): Drop with a pending (never-flushed) allow
    //    request discards it and calls set_ime_allowed(false) exactly
    //    once — the constructor's recorded `true` never reaches the
    //    window at all.
    #[test]
    fn drop_disables_ime() {
        let mock = Arc::new(MockWindow::default());
        {
            let _b = WinitImeBridge::with_handle(Box::new(SharedMock(mock.clone())));
            // drop happens at scope end; the constructor's pending
            // allow=true was never flushed.
        }
        assert_eq!(mock.snapshot().allowed_calls, vec![false]);
    }

    // ── AC-6 (SPEC TS-5): a pending cursor-area request left over at
    //    Drop time is discarded too, not flushed.
    #[test]
    fn drop_discards_pending_cursor_rect() {
        let mock = Arc::new(MockWindow::default());
        {
            let mut b = WinitImeBridge::with_handle(Box::new(SharedMock(mock.clone())));
            b.notify_cursor_rect(1, 2, 3, 4); // recorded, never flushed
            drop(b);
        }
        let inner = mock.snapshot();
        assert_eq!(inner.allowed_calls, vec![false]);
        assert!(
            inner.cursor_calls.is_empty(),
            "pending cursor rect must be discarded, not flushed, on Drop"
        );
    }

    // ── task0001 (windows-imm32-ime-direct) "Deferred detach" ──────────

    // ── AC-1 (SPEC TS1): with a composition open, focus loss followed
    //    by a flush delivers no detach; once Disabled arrives, the next
    //    flush delivers the detach exactly once, and a further flush
    //    delivers nothing.
    #[test]
    fn held_detach_delivers_exactly_once_after_disabled() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        mock.reset();

        b.on_winit_ime(&WinitIme::Enabled);
        b.notify_focus(false);
        b.flush();
        assert!(
            mock.snapshot().allowed_calls.is_empty(),
            "a detach must be held (not delivered) while the composition is alive"
        );

        b.on_winit_ime(&WinitIme::Disabled);
        b.flush();
        assert_eq!(
            mock.snapshot().allowed_calls,
            vec![false],
            "the held detach must deliver exactly once after Disabled"
        );

        mock.reset();
        b.flush();
        assert!(
            mock.snapshot().allowed_calls.is_empty(),
            "a further flush must deliver nothing — delivery consumed the pending state"
        );
    }

    // ── AC-2 (SPEC TS2): a focus-in recorded while a detach is held
    //    overwrites the pending allow-state (last-writer-wins), so the
    //    detach is never delivered.
    #[test]
    fn focus_in_during_held_detach_overwrites_pending_state_so_detach_never_delivers() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        mock.reset();

        b.on_winit_ime(&WinitIme::Enabled);
        b.notify_focus(false);
        b.flush();
        assert!(
            mock.snapshot().allowed_calls.is_empty(),
            "detach must still be held"
        );

        b.notify_focus(true); // overwrites pending_allow = Some(false)
        b.flush();
        assert_eq!(
            mock.snapshot().allowed_calls,
            vec![true],
            "the focus-in must win; the held detach must never reach the window"
        );
    }

    // ── AC-3 (FR3 regression guard): with no composition alive, focus
    //    loss followed by a flush delivers the detach on that same
    //    flush — current (non-composing) behavior is preserved.
    #[test]
    fn detach_without_live_composition_delivers_on_same_flush() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        mock.reset();

        // No Enabled was ever observed, so composition_alive is false.
        b.notify_focus(false);
        b.flush();
        assert_eq!(mock.snapshot().allowed_calls, vec![false]);
    }

    // ── AC-5: a held detach does not block cursor-area delivery — with
    //    a composition alive, a pending detach plus a pending cursor
    //    area flush as a cursor-area-only delivery in that turn.
    #[test]
    fn held_detach_does_not_block_pending_cursor_area_delivery() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        mock.reset();

        b.on_winit_ime(&WinitIme::Enabled);
        b.notify_focus(false);
        b.notify_cursor_rect(1, 2, 3, 4);
        b.flush();

        let inner = mock.snapshot();
        assert!(inner.allowed_calls.is_empty(), "the detach must stay held");
        assert_eq!(
            inner.cursor_calls,
            vec![(1, 2, 3, 4)],
            "the cursor area must still flush even while the detach is held"
        );
    }

    // ── Edge case (Test Notes): Enabled → Disabled → focus loss → flush
    //    delivers the detach immediately, because the composition was
    //    already closed before focus was lost.
    #[test]
    fn focus_loss_after_composition_already_closed_delivers_detach_immediately() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        mock.reset();

        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Disabled);
        b.notify_focus(false);
        b.flush();
        assert_eq!(mock.snapshot().allowed_calls, vec![false]);
    }

    // ── Regression guard: an ENABLE recorded while a composition is
    //    alive is never held — only a pending DISABLE can be held.
    #[test]
    fn pending_enable_is_never_held_even_with_live_composition() {
        let (mut b, mock) = make_bridge();
        b.flush(); // discharge the constructor's recorded allow=true
        mock.reset();

        b.on_winit_ime(&WinitIme::Enabled);
        b.notify_focus(true); // records allow=true, not a detach
        b.flush();
        assert_eq!(
            mock.snapshot().allowed_calls,
            vec![true],
            "an enable must never be held regardless of composition_alive"
        );
    }

    // ── AC-9 (SPEC TS-8): a second consecutive Enabled (no intervening
    //    Disabled) triggers the anomaly latch on its second occurrence
    //    only; the first Enabled from a fresh bridge is a normal
    //    transition and must not warn.
    #[test]
    fn double_enabled_second_occurrence_warns_and_latches() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        assert!(
            !b.enabled_anomaly_warned,
            "first Enabled from a fresh bridge must not warn"
        );
        b.on_winit_ime(&WinitIme::Enabled);
        assert!(
            b.enabled_anomaly_warned,
            "second consecutive Enabled must warn and latch"
        );
        // Latch stays set on further repeats — no un-latching.
        b.on_winit_ime(&WinitIme::Enabled);
        assert!(b.enabled_anomaly_warned);
    }

    // ── AC-9 (SPEC TS-8): a second consecutive Disabled (no
    //    intervening Enabled) triggers the anomaly latch on its second
    //    occurrence only; the first Disabled that closes a real
    //    composition is a normal transition and must not warn.
    #[test]
    fn double_disabled_second_occurrence_warns_and_latches() {
        let (mut b, _mock) = make_bridge();
        b.on_winit_ime(&WinitIme::Enabled);
        b.on_winit_ime(&WinitIme::Disabled);
        assert!(
            !b.disabled_anomaly_warned,
            "the Disabled that closes a real Enabled lifecycle must not warn"
        );
        b.on_winit_ime(&WinitIme::Disabled);
        assert!(
            b.disabled_anomaly_warned,
            "second consecutive Disabled must warn and latch"
        );
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
