//! IME backend plumbing and IME event routing for [`App`].

use std::time::Instant;

use crate::ime::backend::{ImeBackend, ImeEvent, KeyDispatchResult, PUMP_BUDGET, RawKeyEvent};

use super::App;

impl App {
    /// Replace the IME backend installed by `App::new`. Called once by
    /// `window_host::run` after the tao window exists and
    /// `ImeBackendFactory::build` has chosen the right OS backend.
    ///
    /// Phase 4-G-A: any non-`NullBackend` backend reports its `name()`
    /// via the trait. The App tracks whether the current backend is
    /// the passthrough so the event-loop hook can gate
    /// `WindowEvent::ReceivedImeText` (Phase 4 commit path) on
    /// "NullBackend only" — real backends emit `ImeEvent::Commit`
    /// instead.
    pub fn set_ime_backend(&mut self, backend: Box<dyn ImeBackend>) {
        self.ime_is_null = backend.name() == "null";
        self.ime_backend = backend;
    }

    /// `true` when the currently installed backend is the passthrough
    /// `NullBackend`. Used by `window_host` to decide whether
    /// `WindowEvent::ReceivedImeText` should drive the commit path.
    #[allow(dead_code)] // exercised via tests; production caller is window_host
    pub fn ime_is_null(&self) -> bool {
        self.ime_is_null
    }

    /// Offer a raw key event to the active backend before the existing
    /// `tao_key_to_bytes` path runs. Returns the backend's
    /// `KeyDispatchResult`: `Consumed` → skip `tao_key_to_bytes`,
    /// `Passthrough` → continue with the Phase 4 path. SPEC.md FR6.
    pub fn dispatch_key_event_via_ime(&mut self, raw: &RawKeyEvent) -> KeyDispatchResult {
        self.ime_backend.dispatch_key_event(raw)
    }

    /// Forward focus state to the active backend. Wired from
    /// `WindowEvent::Focused(b)` in `window_host`. SPEC.md FR8.
    pub fn notify_ime_focus(&mut self, focused: bool) {
        self.ime_backend.notify_focus(focused);
    }

    /// Forward a winit `WindowEvent::Ime` payload to the active
    /// backend. The default `ImeBackend::on_winit_ime` impl is a
    /// no-op; only `WinitImeBridge` overrides it. SPEC.md FR11.
    pub fn pass_winit_ime(&mut self, ime: &winit::event::Ime) {
        self.ime_backend.on_winit_ime(ime);
    }

    /// Flush any IME side-effect requests the active backend recorded
    /// since the previous flush. Called once per `about_to_wait` turn
    /// from `window_host` — never from inside winit's event dispatch,
    /// which is the whole point: on Windows, issuing the underlying
    /// IMM32 calls from inside wndproc dispatch is the identified
    /// CorvusSKK deadlock mechanism (task0001, windows-skk-ime-hang
    /// SPEC FR1). The default `ImeBackend::flush` impl is a no-op, so
    /// this is safe to call unconditionally regardless of which
    /// backend is installed.
    pub fn flush_ime(&mut self) {
        self.ime_backend.flush();
    }

    /// Drain queued `ImeEvent`s from the active backend and route them
    /// through the existing Phase 4-E layer
    /// (`on_ime_preedit` / `on_ime_commit` / `on_ime_focus_lost`).
    /// Bounded to `PUMP_BUDGET` events per tick; overflow is dropped
    /// with a single warn log (latched). SPEC.md FR5 + IME_E901.
    pub fn pump_ime(&mut self) -> bool {
        let mut events: Vec<ImeEvent> = Vec::new();
        self.ime_backend.pump(&mut events);
        if events.is_empty() {
            return false;
        }
        if events.len() >= PUMP_BUDGET && !self.ime_overflow_warned {
            log::warn!(
                "ime pump reached PUMP_BUDGET ({PUMP_BUDGET}); overflow events dropped (IME_E901)"
            );
            self.ime_overflow_warned = true;
        }
        let n = events.len();
        for ev in events {
            match ev {
                ImeEvent::Preedit(text) => self.on_ime_preedit(&text),
                ImeEvent::Commit(text) => self.on_ime_commit(&text),
                ImeEvent::FocusOut => self.on_ime_focus_lost(),
            }
        }
        log::debug!("ime pump routed {n} event(s)");
        true
    }

    /// Push the active cursor cell (in pixels) to the IME backend
    /// **only** when the (row, col) actually changed. Rate-limits the
    /// `XICAttribute::XNSpotLocation` / `set_cursor_rectangle` /
    /// `ImmSetCompositionWindow` calls so frequent redraws on a static
    /// cursor don't flood the IM server. SPEC.md FR7.
    ///
    /// `cell_w_px` / `cell_h_px` and `origin_x_px` / `origin_y_px` must
    /// match what `window_host` actually uses to lay out the grid; the
    /// computed cursor rect is in physical pixels.
    pub fn notify_cursor_rect_if_changed(
        &mut self,
        cell_w_px: u32,
        cell_h_px: u32,
        origin_x_px: i32,
        origin_y_px: i32,
    ) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let (row, col) = {
            let core = tab.core.lock();
            (core.get_cursor_row(), core.get_cursor_col())
        };
        if self.ime_last_cursor_cell == Some((row, col)) {
            return;
        }
        self.ime_last_cursor_cell = Some((row, col));
        let x = (col as i32) * (cell_w_px as i32) + origin_x_px;
        let y = (row as i32) * (cell_h_px as i32) + origin_y_px;
        self.ime_backend
            .notify_cursor_rect(x, y, cell_w_px as i32, cell_h_px as i32);
    }

    /// Phase 4-E: route an `egui::Event::Ime(ImeEvent::Preedit(_))`
    /// payload to the active tab's preedit state. The anchor is the
    /// current cursor cell of the active tab's `TerminalCore`. No-op
    /// when there is no active tab.
    ///
    /// tao 0.34 only surfaces `WindowEvent::ReceivedImeText` (commit);
    /// this method is the routing point for future preedit plumbing
    /// (richer IME via egui's `ImeEvent::Preedit` once available) and
    /// is exercised directly by the unit tests.
    ///
    /// Phase 4-G-E: when `EMTERM_IME_PERF=1` is set, the entry time
    /// is captured via `Instant::now()` and the delta to
    /// `needs_full_redraw = true` is logged at warn level so TS-perf-3
    /// can be measured on a release host (release builds drop debug
    /// + log levels below warn).
    #[allow(dead_code)]
    pub fn on_ime_preedit(&mut self, text: &str) {
        let perf = ime_perf_enabled();
        let t0 = perf.then(Instant::now);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let anchor = {
            let core = tab.core.lock();
            crate::ime::preedit::Anchor {
                row: core.get_cursor_row(),
                col: core.get_cursor_col(),
            }
        };
        tab.preedit_state.set(text, anchor);
        // The renderer skips frames when no row is dirty; force the
        // cursor row into the dirty set so the underline overlay
        // repaints immediately.
        self.needs_full_redraw = true;
        if let Some(start) = t0 {
            log::warn!(
                "ime perf [TS-perf-3] on_ime_preedit → needs_full_redraw: {} µs",
                start.elapsed().as_micros()
            );
        }
    }

    /// Phase 4-E: route an `egui::Event::Ime(ImeEvent::Commit(_))`
    /// payload to the active tab. Sanitizes the bytes via
    /// `ime::commit::write_commit` (same sanitizer the preedit state
    /// uses) and writes them to the active PTY exactly once. Then
    /// clears the preedit state so the overlay disappears. No-op when
    /// there is no active tab.
    ///
    /// Phase 4-G-E: when `EMTERM_IME_PERF=1` is set, the entry time
    /// is captured via `Instant::now()` and the delta to the
    /// `PtySession::write` return is logged at warn level so
    /// TS-perf-4 can be measured on a release host.
    pub fn on_ime_commit(&mut self, text: &str) {
        let perf = ime_perf_enabled();
        let t0 = perf.then(Instant::now);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        // mux mode: route the commit as a PtyInput frame (the bridge drops raw
        // stdin). Otherwise write directly to the PTY.
        if tab
            .mux_group
            .as_ref()
            .and_then(|g| g.active_pane_id())
            .is_some()
        {
            let bytes = crate::ime::commit::commit_bytes(text);
            if !bytes.is_empty() {
                tab.write_input(bytes);
            }
        } else if let Some(pty) = tab.pty.as_ref() {
            if let Err(e) = crate::ime::commit::write_commit(pty, text) {
                log::warn!("ime commit write failed: {e}");
            }
        }
        tab.preedit_state.clear();
        self.needs_full_redraw = true;
        if let Some(start) = t0 {
            log::warn!(
                "ime perf [TS-perf-4] on_ime_commit → PtySession::write: {} µs",
                start.elapsed().as_micros()
            );
        }
    }

    /// Phase 4-E: clear the active tab's preedit state. Called on
    /// focus loss and on active tab close.
    pub fn on_ime_focus_lost(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.preedit_state.clear();
            self.needs_full_redraw = true;
        }
    }

}

/// Phase 4-G-E performance instrumentation gate. Returns `true` when
/// the env `EMTERM_IME_PERF=1` is set. Cached on first call so the
/// hot path (called once per preedit / commit event) is a single
/// atomic load.
fn ime_perf_enabled() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static CACHED: AtomicU8 = AtomicU8::new(0); // 0 = unset, 1 = false, 2 = true
    match CACHED.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let enabled = std::env::var("EMTERM_IME_PERF")
                .ok()
                .map(|v| v == "1")
                .unwrap_or(false);
            CACHED.store(if enabled { 2 } else { 1 }, Ordering::Relaxed);
            enabled
        }
    }
}
