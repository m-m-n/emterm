//! Native-side `TerminalCallbacks` implementation.
//!
//! `term_core::TerminalCallbacks` is the trait abstraction introduced in
//! Phase 2 of the restructuring. The wasm thin wrapper provides a
//! `js_sys::Function`-backed implementation; this module provides the
//! native-side equivalent for `native-poc` (Phase 6).
//!
//! Methods on `TerminalCallbacks` take `&self`, so any state mutation goes
//! through interior mutability. We use a single `Arc<Mutex<State>>` so the
//! callbacks (fired on whichever thread is currently driving
//! `TerminalCore::process_pty_data`) and the UI (which drains state per
//! frame) can share it safely.

use std::sync::Arc;

use parking_lot::Mutex;
use term_core::callbacks::TerminalCallbacks;

/// OSC action ids emitted by `term_core::osc_handler` that this PoC cares about.
const OSC_SET_TITLE_AND_ICON: u8 = 0;
const OSC_SET_TITLE: u8 = 2;
const OSC_EMTERM_EXTENSION: u8 = 100; // 777 in the wire form

/// Emterm OSC viewer-spawn request decoded from an OSC 777 payload. Phase 6
/// passes these to the (future) Wry viewer spawner. For now native-poc only
/// records them so we can verify the dispatch path end-to-end.
#[derive(Debug, Clone)]
pub struct EmtermOscRequest {
    /// Raw payload — read by the (Phase 5+) Wry viewer spawner.
    #[allow(dead_code)]
    pub payload: String,
}

/// Shared mutable state populated by `NativeCallbacks` and drained by the
/// `Tab` / UI layer.
#[derive(Debug, Default)]
pub struct NativeCallbackState {
    /// Latest OSC 0/2 title received from the shell.
    pub title: Option<String>,
    /// Pending emterm-extension viewer requests.
    pub osc_queue: Vec<EmtermOscRequest>,
    /// BEL counter (used for visual-bell or audible-bell hooks in later phases).
    pub bell_count: u32,
    /// Device responses the terminal asked us to send back to the shell.
    /// `Tab::pump` drains this and feeds it into the PTY writer.
    pub device_responses: Vec<Vec<u8>>,
}

/// `TerminalCallbacks` implementation for native consumers.
pub struct NativeCallbacks {
    state: Arc<Mutex<NativeCallbackState>>,
}

impl NativeCallbacks {
    pub fn new(state: Arc<Mutex<NativeCallbackState>>) -> Self {
        Self { state }
    }
}

impl TerminalCallbacks for NativeCallbacks {
    fn on_osc(&self, action_type: u8, data: &str) {
        match action_type {
            OSC_SET_TITLE_AND_ICON | OSC_SET_TITLE => {
                self.state.lock().title = Some(data.to_string());
            }
            OSC_EMTERM_EXTENSION => {
                self.state.lock().osc_queue.push(EmtermOscRequest {
                    payload: data.to_string(),
                });
            }
            _ => {
                // Other OSCs are logged but not acted on in the PoC.
                log::debug!("unhandled OSC {action_type}: {} bytes", data.len());
            }
        }
    }

    fn on_apc(&self, data: &[u8]) {
        // PoC does not implement Kitty graphics (APC); log only.
        log::debug!("APC received: {} bytes", data.len());
    }

    fn on_dcs(&self, data: &[u8]) {
        // PoC does not implement Sixel (DCS); log only.
        log::debug!("DCS received: {} bytes", data.len());
    }

    fn on_bell(&self) {
        self.state.lock().bell_count += 1;
        log::debug!("BEL");
    }

    fn on_device_response(&self, data: &[u8]) {
        self.state.lock().device_responses.push(data.to_vec());
    }
}
