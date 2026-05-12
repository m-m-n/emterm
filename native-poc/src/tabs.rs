//! Tab type and lifecycle.
//!
//! Phase 6 swap: `Parser + Grid` (the Phase 1 PoC stand-ins) are replaced
//! by `term_core::TerminalCore`. Incoming PTY bytes are pushed through
//! `process_pty_data`; the grid state is read via `get_cell_*` /
//! `get_cursor_*` accessors. OSC titles and emterm-extension dispatches
//! are delivered through the shared `NativeCallbackState`.

use std::sync::Arc;

use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use term_core::terminal_core::TerminalCore;
use term_images::image_proc::{ImageEvent, ImageProcessor};

use crate::callbacks::{EmtermOscRequest, NativeCallbackState, NativeCallbacks};
use crate::image::{parse as image_parse, split_image_events};
use crate::pty::{ExitReason, PtyEvent, PtySession};
use crate::render::theme::Theme;
use crate::settings::Settings;

// Default scrollback capacity now lives on `Settings` (`DEFAULT_SCROLLBACK_LINES`
// in `crate::settings`); the caller passes the desired value into
// `Tab::spawn_shell`.

pub struct Tab {
    pub title: String,
    pub core: Arc<Mutex<TerminalCore>>,
    pub cb_state: Arc<Mutex<NativeCallbackState>>,
    /// Per-tab theme. OSC 4/10/11/12/22/104/110/111/112 mutate this in
    /// place through `NativeCallbacks::handle_theme`; the renderer reads
    /// the same `Arc<Mutex<Theme>>` so updates take effect on the next
    /// frame. Phase 6 only wires the dirty-flag and mutation path —
    /// rendering still reads `Theme::default()` until the wiring change
    /// lands in a later sub-phase. The Arc is shared so it stays alive
    /// for the lifetime of the tab.
    #[allow(dead_code)] // Phase 3 renderer wiring will read this.
    pub theme: Arc<Mutex<Theme>>,
    // Drop order matters: `events` must drop before `pty`. Struct fields
    // drop in declaration order, so `events` (the Receiver) closing first
    // disconnects the bounded(256) channel from the reader/writer side.
    // That lets the EOF `event_tx.send(PtyEvent::Exited)` in `reader_loop`
    // return `Err` immediately on shutdown instead of blocking forever
    // when the channel happens to be full — which would deadlock
    // `PtySession::Drop`'s `reader_join.join()` and freeze the X-button
    // close path (WM marks the window as 応答なし while we wait).
    pub events: Receiver<PtyEvent>,
    pub pty: Option<PtySession>,
    pub exited: bool,
    /// Whether the alternate screen buffer is currently active for this tab.
    /// Updated by `pump()` after draining `core.take_mode_actions()`.
    pub alt_screen: bool,
    /// Per-tab Kitty Graphics Protocol + SIXEL processor (CPU side).
    /// `Tab::pump` drains `cb_state.pending_apc` / `pending_dcs` into this
    /// to produce `ImageEvent`s. `Response` events are routed back to the
    /// PTY (Kitty OK / error replies); everything else is queued in
    /// `pending_image_events` for the GPU layer (owned by `WindowHost`)
    /// to consume.
    pub image_proc: ImageProcessor,
    /// Image events the GPU layer (in `WindowHost`) has not yet ingested.
    /// `Tab::drain_image_events` returns these.
    pub pending_image_events: Vec<ImageEvent>,
}

/// Mode action codes emitted by `TerminalCore` after CSI ?47h / ?47l /
/// ?1047h / ?1047l / ?1049h / ?1049l. Kept in sync with
/// `crates/term_core/src/csi_modes.rs`.
const MODE_ACTION_SWITCH_TO_ALT: u8 = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT: u8 = 2;
const MODE_ACTION_SWITCH_TO_MAIN: u8 = 3;
/// TS_FALLBACK escape codes — followed by 2 payload bytes. Skipped when
/// scanning for buffer-switch markers.
const MODE_ACTION_TS_FALLBACK_A: u8 = 0xFF;
const MODE_ACTION_TS_FALLBACK_B: u8 = 0xFE;

impl Tab {
    pub fn spawn_shell(
        title: impl Into<String>,
        cols: u16,
        rows: u16,
        scrollback_lines: u32,
        settings: Arc<Settings>,
    ) -> Self {
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(256);
        let pty = match PtySession::spawn(cols, rows, tx) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!("failed to spawn shell PTY: {e}");
                None
            }
        };

        // Construct the core and install our native callbacks.
        let mut core = TerminalCore::new(cols, rows, scrollback_lines);
        let cb_state = Arc::new(Mutex::new(NativeCallbackState::default()));
        let theme = Arc::new(Mutex::new(Theme::default()));
        core.callbacks = Some(Box::new(NativeCallbacks::new(
            cb_state.clone(),
            theme.clone(),
            settings,
        )));

        Self {
            title: title.into(),
            core: Arc::new(Mutex::new(core)),
            cb_state,
            theme,
            pty,
            events: rx,
            exited: false,
            alt_screen: false,
            image_proc: ImageProcessor::new(),
            pending_image_events: Vec::new(),
        }
    }

    /// Drain pending PTY events into the terminal core. Returns true if
    /// anything changed (caller should request a redraw).
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        let mut had_data = false;
        while let Ok(evt) = self.events.try_recv() {
            match evt {
                PtyEvent::Data(bytes) => {
                    let mut c = self.core.lock();
                    c.process_pty_data(&bytes);
                    // Drain mode actions to keep our local alt-screen flag
                    // synced. `process_pty_data` interrupts on buffer
                    // switches, so subsequent chunks may need re-pumping —
                    // that's already the case for the data loop above.
                    let actions = c.take_mode_actions();
                    drop(c);
                    if let Some(new_alt) = parse_alt_screen_action(&actions) {
                        self.alt_screen = new_alt;
                    }
                    had_data = true;
                    changed = true;
                }
                PtyEvent::Exited { reason } => {
                    match reason {
                        ExitReason::Eof => {
                            log::info!("tab {:?} exited: EOF", self.title)
                        }
                        ExitReason::ReadError(e) => {
                            log::warn!("tab {:?} read error: {e}", self.title)
                        }
                    }
                    self.exited = true;
                    changed = true;
                }
            }
        }
        let _ = had_data; // reserved for future use (e.g. on_pty_output hook).

        // Sync title from callback state if the shell sent a new one.
        {
            let mut s = self.cb_state.lock();
            if let Some(t) = s.title.take() {
                if t != self.title {
                    self.title = t;
                    changed = true;
                }
            }

            // Send any device responses (e.g., DA1) back to the shell.
            let responses: Vec<Vec<u8>> = std::mem::take(&mut s.device_responses);
            // Drain buffered image-protocol payloads so we can decode them
            // outside the lock (the decoder needs cursor coords from `core`
            // — see comment in `drain_and_decode_images` below).
            let pending_apc: Vec<Vec<u8>> = std::mem::take(&mut s.pending_apc);
            let pending_dcs: Vec<Vec<u8>> = std::mem::take(&mut s.pending_dcs);
            // Phase 6: drain the theme-dirty latch. When an OSC 4/10/11/12/
            // 22/104/110/111/112 mutated the shared `Theme`, every row
            // must repaint with the new palette on the next frame.
            let theme_changed = std::mem::take(&mut s.theme_dirty);
            drop(s);
            if theme_changed {
                self.core.lock().mark_all_dirty();
                changed = true;
            }
            if !responses.is_empty() {
                for r in responses {
                    self.write(r);
                }
                changed = true;
            }
            if !pending_apc.is_empty() || !pending_dcs.is_empty() {
                if self.drain_and_decode_images(&pending_apc, &pending_dcs) {
                    changed = true;
                }
            }
        }

        changed
    }

    /// Decode buffered APC / DCS payloads via `term_images::ImageProcessor`,
    /// split off `ImageEvent::Response` and write the bytes back to the
    /// shell (Kitty OK / error replies), and queue the remaining state
    /// events in `pending_image_events` for the GPU layer to consume on
    /// its next ingest pass.
    ///
    /// Cursor coordinates are read from `self.core` so the resulting
    /// `Place` events anchor at the right cell. We snapshot them once at
    /// the start of the call: in practice the buffered payloads were
    /// produced by the same byte chunk that just landed, so the cursor
    /// has not moved between APC parsing and decode here.
    ///
    /// Returns true when anything was decoded (caller can request a
    /// redraw).
    fn drain_and_decode_images(&mut self, apc: &[Vec<u8>], dcs: &[Vec<u8>]) -> bool {
        if apc.is_empty() && dcs.is_empty() {
            return false;
        }
        let (cursor_row, cursor_col) = {
            let c = self.core.lock();
            (c.get_cursor_row() as u32, c.get_cursor_col() as u32)
        };

        let mut all_events: Vec<ImageEvent> = Vec::new();
        for bytes in apc {
            let events =
                image_parse::decode_apc(bytes, cursor_row, cursor_col, &mut self.image_proc);
            all_events.extend(events);
        }
        for bytes in dcs {
            let events =
                image_parse::decode_dcs(bytes, cursor_row, cursor_col, &mut self.image_proc);
            all_events.extend(events);
        }
        if all_events.is_empty() {
            return false;
        }

        let (state_events, responses) = split_image_events(all_events);
        // Route protocol responses (Kitty OK / error / query) back to the
        // shell. SIXEL does not currently emit responses but the API is
        // uniform.
        for resp in responses {
            self.write(resp.into_bytes());
        }
        if !state_events.is_empty() {
            self.pending_image_events.extend(state_events);
        }
        true
    }

    /// Drain the queue of image-state events accumulated by `pump`.
    /// `WindowHost` calls this once per frame and forwards the events to
    /// `ImageLayer::ingest`, which handles GPU texture upload + LRU
    /// eviction.
    pub fn drain_image_events(&mut self) -> Vec<ImageEvent> {
        std::mem::take(&mut self.pending_image_events)
    }

    pub fn write(&self, bytes: Vec<u8>) {
        if let Some(p) = &self.pty {
            p.write(bytes);
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(p) = &self.pty {
            p.resize(cols, rows);
        }
        self.core.lock().resize(cols, rows);
    }

    /// Drain queued emterm OSC viewer-spawn requests. Phase 5+ feeds these
    /// into the Wry viewer spawner.
    pub fn drain_osc(&self) -> Vec<EmtermOscRequest> {
        std::mem::take(&mut self.cb_state.lock().osc_queue)
    }
}

/// Scan a `take_mode_actions()` payload for buffer-switch markers and
/// return the resulting alt-screen state, if any. Returns `None` when no
/// buffer-switch action is present (the caller keeps the previous flag).
///
/// The payload is a byte slice of action codes; TS_FALLBACK markers
/// (`0xFF` / `0xFE`) introduce a 3-byte entry (marker + mode_lo + mode_hi)
/// and must be skipped.
fn parse_alt_screen_action(actions: &[u8]) -> Option<bool> {
    let mut last: Option<bool> = None;
    let mut i = 0;
    while i < actions.len() {
        let code = actions[i];
        if code == MODE_ACTION_TS_FALLBACK_A || code == MODE_ACTION_TS_FALLBACK_B {
            i += 3;
            continue;
        }
        match code {
            MODE_ACTION_SWITCH_TO_ALT | MODE_ACTION_SAVE_AND_SWITCH_TO_ALT => {
                last = Some(true);
            }
            MODE_ACTION_SWITCH_TO_MAIN => {
                last = Some(false);
            }
            _ => {}
        }
        i += 1;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_alt_screen_switch_to_alt() {
        assert_eq!(parse_alt_screen_action(&[1]), Some(true));
        assert_eq!(parse_alt_screen_action(&[2]), Some(true));
    }

    #[test]
    fn parse_alt_screen_switch_to_main() {
        assert_eq!(parse_alt_screen_action(&[3]), Some(false));
    }

    #[test]
    fn parse_alt_screen_empty_returns_none() {
        assert_eq!(parse_alt_screen_action(&[]), None);
    }

    #[test]
    fn parse_alt_screen_skips_ts_fallback() {
        // 0xFF / 0xFE are followed by two mode bytes.
        assert_eq!(parse_alt_screen_action(&[0xFF, 0x01, 0x00]), None);
        assert_eq!(parse_alt_screen_action(&[0xFE, 0x07, 0x00, 1]), Some(true));
    }

    #[test]
    fn parse_alt_screen_takes_last_seen() {
        // Within one chunk: enter alt then leave → last is `false`.
        assert_eq!(parse_alt_screen_action(&[2, 3]), Some(false));
        // Enter alt twice → last is `true`.
        assert_eq!(parse_alt_screen_action(&[3, 2]), Some(true));
    }
}
