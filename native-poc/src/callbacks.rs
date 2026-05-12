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
    /// Pending APC (Kitty Graphics) payloads buffered by `on_apc`. Drained
    /// by `Tab::pump` after locking `TerminalCore` so the cursor row/col
    /// can be snapshotted and passed to `term_images::ImageProcessor`.
    /// The callback itself only sees `&self` on `NativeCallbacks` and so
    /// has no access to the core state — buffer-then-drain is the simplest
    /// correct pattern.
    pub pending_apc: Vec<Vec<u8>>,
    /// Pending DCS (SIXEL) payloads, same buffering rationale as
    /// `pending_apc`.
    pub pending_dcs: Vec<Vec<u8>>,
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
        // Phase 5: buffer the payload; `Tab::pump` decodes it under the
        // core lock so cursor coordinates are stable.
        log::debug!("APC buffered: {} bytes", data.len());
        self.state.lock().pending_apc.push(data.to_vec());
    }

    fn on_dcs(&self, data: &[u8]) {
        // Phase 5: buffer the payload; `Tab::pump` decodes it under the
        // core lock so cursor coordinates are stable.
        log::debug!("DCS buffered: {} bytes", data.len());
        self.state.lock().pending_dcs.push(data.to_vec());
    }

    fn on_bell(&self) {
        self.state.lock().bell_count += 1;
        log::debug!("BEL");
    }

    fn on_device_response(&self, data: &[u8]) {
        self.state.lock().device_responses.push(data.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cb() -> (NativeCallbacks, Arc<Mutex<NativeCallbackState>>) {
        let s = Arc::new(Mutex::new(NativeCallbackState::default()));
        (NativeCallbacks::new(s.clone()), s)
    }

    #[test]
    fn on_apc_buffers_payload_into_pending_apc() {
        let (n, s) = cb();
        n.on_apc(b"Ga=q;");
        let st = s.lock();
        assert_eq!(st.pending_apc.len(), 1);
        assert_eq!(st.pending_apc[0], b"Ga=q;".to_vec());
        assert!(st.pending_dcs.is_empty());
    }

    #[test]
    fn on_dcs_buffers_payload_into_pending_dcs() {
        let (n, s) = cb();
        n.on_dcs(b"0;0;0q");
        let st = s.lock();
        assert_eq!(st.pending_dcs.len(), 1);
        assert_eq!(st.pending_dcs[0], b"0;0;0q".to_vec());
        assert!(st.pending_apc.is_empty());
    }

    #[test]
    fn on_apc_appends_in_order_across_multiple_calls() {
        let (n, s) = cb();
        n.on_apc(b"a");
        n.on_apc(b"b");
        n.on_apc(b"c");
        let st = s.lock();
        assert_eq!(
            st.pending_apc,
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
    }

    #[test]
    fn on_osc_title_still_populates_title() {
        let (n, s) = cb();
        n.on_osc(OSC_SET_TITLE, "hello");
        assert_eq!(s.lock().title.as_deref(), Some("hello"));
    }

    #[test]
    fn on_bell_increments_counter() {
        let (n, s) = cb();
        n.on_bell();
        n.on_bell();
        assert_eq!(s.lock().bell_count, 2);
    }
}
