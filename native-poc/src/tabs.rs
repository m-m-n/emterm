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

use crate::callbacks::{EmtermOscRequest, NativeCallbackState, NativeCallbacks};
use crate::pty::{ExitReason, PtyEvent, PtySession};

/// Default scrollback line capacity. Phase 7's settings loader overrides this.
const DEFAULT_SCROLLBACK_LINES: u32 = 1000;

pub struct Tab {
    pub title: String,
    pub core: Arc<Mutex<TerminalCore>>,
    pub cb_state: Arc<Mutex<NativeCallbackState>>,
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
}

impl Tab {
    pub fn spawn_shell(title: impl Into<String>, cols: u16, rows: u16) -> Self {
        let (tx, rx) = crossbeam_channel::bounded::<PtyEvent>(256);
        let pty = match PtySession::spawn(cols, rows, tx) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!("failed to spawn shell PTY: {e}");
                None
            }
        };

        // Construct the core and install our native callbacks.
        let mut core = TerminalCore::new(cols, rows, DEFAULT_SCROLLBACK_LINES);
        let cb_state = Arc::new(Mutex::new(NativeCallbackState::default()));
        core.callbacks = Some(Box::new(NativeCallbacks::new(cb_state.clone())));

        Self {
            title: title.into(),
            core: Arc::new(Mutex::new(core)),
            cb_state,
            pty,
            events: rx,
            exited: false,
        }
    }

    /// Drain pending PTY events into the terminal core. Returns true if
    /// anything changed (caller should request a redraw).
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        while let Ok(evt) = self.events.try_recv() {
            match evt {
                PtyEvent::Data(bytes) => {
                    let mut c = self.core.lock();
                    c.process_pty_data(&bytes);
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
            drop(s);
            if !responses.is_empty() {
                for r in responses {
                    self.write(r);
                }
                changed = true;
            }
        }

        changed
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
