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
use mux_ipc::protocol::{MessageType, MuxMessage, StatusUpdateMsg, WelcomeMsg};

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
    /// When `Some(name)`, this tab is attached to a remote mux session
    /// and the tab bar prefixes the title with `[mux:<name>]`. Phase
    /// 4-B introduces the field; Phase 4-C (APC redesign) populates it
    /// from the daemon's `Welcome::Accepted.sessions[active]` arriving
    /// as an APC frame inside the PTY output.
    pub mux_session_name: Option<String>,
    /// Phase 4-C: most recent status-update payload received from the
    /// daemon. Phase 4-D's status-bar widget (`ui::status_bar`) reads
    /// this through `App::status_bar_state()`.
    pub mux_status_state: Option<StatusUpdateMsg>,
    /// Phase 4-E: per-tab IME preedit composition state. Driven by
    /// `egui::Event::Ime(ImeEvent::Preedit/Commit)` events routed
    /// through `App` and rendered as an underline overlay by
    /// `render::cursor::draw_cursor_with_preedit`.
    pub preedit_state: crate::ime::preedit::State,
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
        statusbar_dispatcher: Option<
            Arc<crate::status_bar::osc_dispatcher::StatusBarOscDispatcher>,
        >,
        cwd_provider: Option<Arc<crate::status_bar::providers::CwdProvider>>,
    ) -> Self {
        // bounded(4096): allows up to ~64MB of in-flight PTY chunks
        // (4096 × 16KB reader buffer) before the reader thread blocks
        // on send. The previous 256-slot queue (~4MB) made bursty
        // producers like `seq 1 10000000` slow to a crawl because the
        // shell side filled the queue inside a few milliseconds and
        // then had to wait for the main thread to drain one frame's
        // worth (12 ms budget) at a time. The trade-off is up to 64MB
        // of resident memory per tab during a sustained burst.
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(4096);
        let pty = match PtySession::spawn(cols, rows, tx) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!("failed to spawn shell PTY: {e}");
                None
            }
        };

        // Construct the core and install our native callbacks.
        let mut core = TerminalCore::new(cols, rows, scrollback_lines);
        // Seed `cursor_blink` from settings before OSC / DECTCEM
        // sequences have had a chance to override it; the default
        // inside `TerminalCore` is `true`, so this only matters when
        // `settings.json` opts out (`"cursor_blink": false`).
        core.set_cursor_blink(settings.cursor_blink);
        let cb_state = Arc::new(Mutex::new(NativeCallbackState::default()));
        // Seed the theme from settings (font_size_pt + cursor_style)
        // so the first frame renders at the user's configured size
        // and cursor shape. OSC 4 / 10 / 11 / 12 / 22 may still
        // mutate any field at runtime via `NativeCallbacks`.
        let theme = Arc::new(Mutex::new(Theme::from_settings(settings.as_ref())));
        let mut callbacks = NativeCallbacks::new(cb_state.clone(), theme.clone(), settings);
        if let Some(dispatcher) = statusbar_dispatcher {
            callbacks.set_statusbar_dispatcher(dispatcher);
        }
        if let Some(provider) = cwd_provider {
            callbacks.set_cwd_provider(provider);
        }
        core.callbacks = Some(Box::new(callbacks));

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
            mux_session_name: None,
            mux_status_state: None,
            preedit_state: crate::ime::preedit::State::default(),
        }
    }

    /// Pause the native PTY reader. Subsequent PTY output goes into the
    /// per-session ring buffer until [`Tab::resume_native_pty`] is called.
    /// Phase 4-C kept this in place for a future "freeze native output
    /// while mux owns the screen" affordance; the APC-redesigned mux
    /// path does not currently invoke it (the bridge CLI runs in the
    /// same PTY, so there is no second stream to pause).
    #[allow(dead_code)]
    pub fn pause_native_pty(&self) {
        if let Some(p) = &self.pty {
            p.set_paused(true);
        }
    }

    /// Drain the ring buffer and replay the bytes into `core`, then resume
    /// the native PTY reader. Counterpart of [`Tab::pause_native_pty`];
    /// unused in the APC-redesigned mux path but retained for the same
    /// future-use rationale.
    #[allow(dead_code)]
    pub fn resume_native_pty(&mut self) {
        if let Some(p) = &self.pty {
            let drained = p.drain_ring();
            if !drained.is_empty() {
                self.core.lock().process_pty_data(&drained);
            }
            if p.ring_overflowed() {
                log::warn!(
                    "mux pause ring buffer overflowed; some native PTY output \
                     was discarded for tab {:?}",
                    self.title
                );
            }
            p.set_paused(false);
        }
    }

    /// Route one decoded mux message into this tab. Called by `App::pump_all`
    /// after the APC decoder ([`crate::mux::apc::try_decode_emterm_mux`])
    /// produced a typed `MuxMessage`. Returns true when the visible state
    /// changed (caller schedules a redraw).
    ///
    /// `Snapshot` payloads are raw PTY-shaped bytes the daemon captured
    /// from the active window — they are replayed into `term_core` via
    /// `reset_and_replay`. Everything else either updates a side-channel
    /// (status bar, session name) or is logged and ignored — the bridge
    /// CLI continues to own the underlying socket protocol, so native-poc
    /// only needs to react to messages that mutate its own state.
    pub fn apply_mux_message(&mut self, msg: MuxMessage) -> bool {
        match msg.msg_type {
            MessageType::Snapshot | MessageType::SnapshotRestore => {
                self.core.lock().reset_and_replay(&msg.payload);
                log::debug!(
                    "mux apc: applied {:?} ({} bytes) for tab {:?}",
                    msg.msg_type,
                    msg.payload.len(),
                    self.title
                );
                true
            }
            MessageType::PtyOutput => {
                // The daemon's continuous PTY stream: feed it into term_core
                // as a normal byte stream (NOT a reset). Without this the
                // mux session looks frozen after the initial Snapshot.
                self.core.lock().process_pty_data(&msg.payload);
                true
            }
            MessageType::StatusUpdate => match msg.decode_payload::<StatusUpdateMsg>() {
                Some(payload) => {
                    self.mux_status_state = Some(payload);
                    true
                }
                None => {
                    log::warn!("mux apc: malformed StatusUpdate payload");
                    false
                }
            },
            MessageType::Welcome => match msg.decode_payload::<WelcomeMsg>() {
                Some(WelcomeMsg::Accepted { sessions, .. }) => {
                    let active = sessions.first().map(|s| s.name.clone());
                    if let Some(name) = active {
                        log::info!("mux apc: tab {:?} attached to session {name}", self.title);
                        self.mux_session_name = Some(name);
                        true
                    } else {
                        false
                    }
                }
                Some(WelcomeMsg::Rejected { reason }) => {
                    log::warn!("mux apc: handshake rejected: {reason}");
                    false
                }
                None => {
                    log::warn!("mux apc: malformed Welcome payload");
                    false
                }
            },
            MessageType::PtyExited => {
                log::info!("mux apc: remote pane exited for tab {:?}", self.title);
                false
            }
            other => {
                log::debug!("mux apc: unhandled message type {other:?}");
                false
            }
        }
    }

    /// Title rendered by the tab bar. Currently identical to
    /// `self.title` — the `[mux:<session>]` prefix is applied by the
    /// widget (see `ui::tab_bar::render_label`).
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() {
            "shell"
        } else {
            self.title.as_str()
        }
    }

    /// Drain pending PTY events into the terminal core. Returns true if
    /// anything changed (caller should request a redraw).
    ///
    /// Frame budget (FRAME_BUDGET_MS): bound how long one `pump` call
    /// spends inside `process_pty_data`. Bursty producers like
    /// `seq 1 10000000` would otherwise let a single pump tick eat the
    /// whole frame and freeze input/render until the burst drained.
    /// When the budget is exhausted we stop draining and request another
    /// frame via `crate::wakeup::wake()` so the remainder is processed
    /// on the next about_to_wait pass.
    pub fn pump(&mut self) -> bool {
        const FRAME_BUDGET_MS: u128 = 12;
        // Coalesce target: drain as many Data chunks as the frame budget
        // allows into one contiguous buffer, then run a SINGLE
        // process_pty_data + flush_grapheme_buffer + take_mode_actions
        // cycle. Per-chunk lock/flush/take overhead was the dominant
        // cost when the shell wrote tiny lines (`seq` produced ~41
        // bytes/chunk in benchmarking, so 200 chunks × 60µs overhead
        // ate the whole 12ms budget per frame and capped throughput
        // around 1MB/s). Coalescing makes one PTY chunk and 200 PTY
        // chunks cost roughly the same.
        const COALESCE_CAP: usize = 1024 * 1024;
        let start = std::time::Instant::now();
        let mut changed = false;
        let mut yielded = false;
        let mut combined: Vec<u8> = Vec::new();
        let mut saw_exit: Option<ExitReason> = None;
        while let Ok(evt) = self.events.try_recv() {
            match evt {
                PtyEvent::Data(bytes) => {
                    if combined.is_empty() {
                        combined = bytes;
                    } else {
                        combined.extend_from_slice(&bytes);
                    }
                    if combined.len() >= COALESCE_CAP
                        || start.elapsed().as_millis() >= FRAME_BUDGET_MS
                    {
                        yielded = true;
                        break;
                    }
                }
                PtyEvent::Exited { reason } => {
                    saw_exit = Some(reason);
                    break;
                }
            }
        }
        if !combined.is_empty() {
            let mut c = self.core.lock();
            c.process_pty_data(&combined);
            // Force-flush any grapheme cluster left buffered by the
            // parser (e.g. a lone emoji codepoint at the tail of an
            // IME-commit echo). Without this the cluster sits in
            // `grapheme_buffer` until the next non-extending codepoint
            // arrives, so the glyph stays invisible and the cursor
            // doesn't advance until the user types something else
            // (typical symptom: SKK `/smile` → 😄 only appears after
            // pressing space).
            c.flush_grapheme_buffer();
            let actions = c.take_mode_actions();
            drop(c);
            if let Some(new_alt) = parse_alt_screen_action(&actions) {
                self.alt_screen = new_alt;
            }
            changed = true;
        }
        if let Some(reason) = saw_exit {
            match reason {
                ExitReason::Eof => log::info!("tab {:?} exited: EOF", self.title),
                ExitReason::ReadError(e) => log::warn!("tab {:?} read error: {e}", self.title),
            }
            self.exited = true;
            changed = true;
        }
        // Yielded mid-burst: schedule another wakeup so the next about_to_wait
        // continues draining instead of waiting for the 16ms WaitUntil deadline.
        if yielded {
            crate::wakeup::wake();
        }

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
            // Split the APC stream: payloads addressed to the `emterm-mux;`
            // inband protocol are decoded and applied to this tab's state;
            // everything else (Kitty graphics) falls through to the image
            // pipeline. `pending_dcs` is image-only (SIXEL).
            let (image_apc, mux_messages) = partition_apc_for_mux(pending_apc);
            for msg in mux_messages {
                if self.apply_mux_message(msg) {
                    changed = true;
                }
            }
            if (!image_apc.is_empty() || !pending_dcs.is_empty())
                && self.drain_and_decode_images(&image_apc, &pending_dcs)
            {
                changed = true;
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

/// Split a drained `pending_apc` buffer into the (image-pipeline,
/// mux-message) halves. APC payloads that start with `emterm-mux;` are
/// decoded into typed `MuxMessage`s via
/// [`crate::mux::apc::try_decode_emterm_mux`]; the rest pass through to
/// the existing Kitty Graphics decoder. Decode failures on a clearly
/// mux-prefixed payload are dropped (the helper already logs at `warn`)
/// rather than fed to the image pipeline — they cannot be valid Kitty.
fn partition_apc_for_mux(apc: Vec<Vec<u8>>) -> (Vec<Vec<u8>>, Vec<MuxMessage>) {
    let mut images: Vec<Vec<u8>> = Vec::with_capacity(apc.len());
    let mut mux: Vec<MuxMessage> = Vec::new();
    for payload in apc {
        if payload.starts_with(b"emterm-mux;") {
            if let Some(msg) = crate::mux::apc::try_decode_emterm_mux(&payload) {
                mux.push(msg);
            }
            // Malformed mux payload — already logged inside the decoder;
            // do NOT forward to the image pipeline.
        } else {
            images.push(payload);
        }
    }
    (images, mux)
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
