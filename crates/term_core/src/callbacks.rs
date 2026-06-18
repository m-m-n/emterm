//! Terminal callbacks abstraction.
//!
//! Previously these callbacks crossed the wasm boundary as
//! `Option<js_sys::Function>` stored on `TerminalCore`. With the Phase 2
//! split between `term_core` (pure Rust) and `wasm/` (thin wrapper) we
//! abstract them behind a trait. The wasm thin wrapper owns a struct that
//! implements this trait by calling the corresponding `js_sys::Function`
//! handles. Native consumers (native-poc, future native terminal) provide
//! their own implementations.
//!
//! The trait surface mirrors the previous `wasm/src/callbacks.rs` 1:1;
//! Phase 2 does not introduce new callback categories.
use crate::terminal_core::TerminalCore;

/// Sink for terminal-driven side effects (OSC titles, BEL, etc.).
///
/// All methods take `&self` because `TerminalCore` invokes them while
/// holding `&self` (no interior mutability required for the wasm wrapper
/// which simply forwards to `js_sys::Function::callN`).
///
/// `Send` is required so a `TerminalCore` — which stores an
/// `Option<Box<dyn TerminalCallbacks>>` — is itself `Send` and can be
/// constructed on a worker thread and moved to the main thread (the
/// mux off-thread snapshot replay; see
/// [`TerminalCore::build_from_snapshot`]). The native consumer
/// (`NativeCallbacks`) is already `Send` (it holds only `Arc`s of
/// `Send + Sync` types). The off-thread builder itself always leaves
/// `callbacks` as `None`, so no callback ever actually crosses the
/// thread boundary — the bound only keeps the *type* movable.
pub trait TerminalCallbacks: Send {
    /// OSC dispatch: `action_type` is the emterm-internal action id
    /// produced by the OSC handler; `data` is the OSC payload (UTF-8 string).
    fn on_osc(&self, action_type: u8, data: &str);

    /// Application Program Command payload (used by the Kitty image
    /// protocol). `data` is the raw APC bytes.
    fn on_apc(&self, data: &[u8]);

    /// Device Control String payload (used by Sixel).
    fn on_dcs(&self, data: &[u8]);

    /// BEL (0x07).
    fn on_bell(&self);

    /// Response bytes the terminal wants to send back to the application
    /// (e.g., CSI device-response queries). `data` is the response payload.
    fn on_device_response(&self, data: &[u8]);
}

// ── Fire methods ────────────────────────────────────────
//
// These are called from the various CSI / OSC / APC / DCS / C0 handlers.
// They short-circuit when no callback sink is installed, matching the
// previous wasm/native behaviour (wasm: no callback = silent drop;
// native: stub previously did the same).

impl TerminalCore {
    pub(crate) fn fire_osc_callback(&self, action_type: u8, data: &str) {
        if let Some(cb) = self.callbacks.as_deref() {
            cb.on_osc(action_type, data);
        }
    }

    pub(crate) fn fire_apc_callback(&self, data: &[u8]) {
        if let Some(cb) = self.callbacks.as_deref() {
            cb.on_apc(data);
        }
    }

    pub(crate) fn fire_dcs_callback(&self, data: &[u8]) {
        if let Some(cb) = self.callbacks.as_deref() {
            cb.on_dcs(data);
        }
    }

    pub(crate) fn fire_bell_callback(&self) {
        if let Some(cb) = self.callbacks.as_deref() {
            cb.on_bell();
        }
    }

    pub(crate) fn fire_device_response_callback(&self) {
        if let Some(cb) = self.callbacks.as_deref() {
            let data = &self.response_buffer[..self.response_len as usize];
            cb.on_device_response(data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalCallbacks;
    use crate::terminal_core::TerminalCore;
    use std::sync::Mutex;

    /// Test impl that records every callback invocation. Uses `Mutex`
    /// (not `RefCell`) so the recorder is `Send + Sync` and an
    /// `Arc<Recorder>` satisfies the `TerminalCallbacks: Send` bound the
    /// off-thread snapshot replay relies on.
    #[derive(Default)]
    struct Recorder {
        osc: Mutex<Vec<(u8, String)>>,
        apc: Mutex<Vec<Vec<u8>>>,
        dcs: Mutex<Vec<Vec<u8>>>,
        bell: Mutex<usize>,
        device: Mutex<Vec<Vec<u8>>>,
    }

    impl TerminalCallbacks for Recorder {
        fn on_osc(&self, action_type: u8, data: &str) {
            self.osc
                .lock()
                .unwrap()
                .push((action_type, data.to_string()));
        }
        fn on_apc(&self, data: &[u8]) {
            self.apc.lock().unwrap().push(data.to_vec());
        }
        fn on_dcs(&self, data: &[u8]) {
            self.dcs.lock().unwrap().push(data.to_vec());
        }
        fn on_bell(&self) {
            *self.bell.lock().unwrap() += 1;
        }
        fn on_device_response(&self, data: &[u8]) {
            self.device.lock().unwrap().push(data.to_vec());
        }
    }

    #[test]
    fn test_fire_osc_callback_none_does_not_panic() {
        let core = TerminalCore::new(80, 24, 0);
        core.fire_osc_callback(2, "title");
    }

    #[test]
    fn test_fire_apc_callback_none_does_not_panic() {
        let core = TerminalCore::new(80, 24, 0);
        core.fire_apc_callback(b"\x1b_test\x1b\\");
    }

    #[test]
    fn test_fire_dcs_callback_none_does_not_panic() {
        let core = TerminalCore::new(80, 24, 0);
        core.fire_dcs_callback(b"\x1bPtest\x1b\\");
    }

    #[test]
    fn test_fire_bell_callback_none_does_not_panic() {
        let core = TerminalCore::new(80, 24, 0);
        core.fire_bell_callback();
    }

    #[test]
    fn test_fire_device_response_callback_none_does_not_panic() {
        let core = TerminalCore::new(80, 24, 0);
        core.fire_device_response_callback();
    }

    #[test]
    fn test_callbacks_none_by_default() {
        let core = TerminalCore::new(80, 24, 0);
        assert!(core.callbacks.is_none());
    }

    #[test]
    fn test_callbacks_dispatch_through_trait() {
        let mut core = TerminalCore::new(80, 24, 0);
        let recorder = std::sync::Arc::new(Recorder::default());
        struct Forward(std::sync::Arc<Recorder>);
        impl TerminalCallbacks for Forward {
            fn on_osc(&self, a: u8, d: &str) {
                self.0.on_osc(a, d)
            }
            fn on_apc(&self, d: &[u8]) {
                self.0.on_apc(d)
            }
            fn on_dcs(&self, d: &[u8]) {
                self.0.on_dcs(d)
            }
            fn on_bell(&self) {
                self.0.on_bell()
            }
            fn on_device_response(&self, d: &[u8]) {
                self.0.on_device_response(d)
            }
        }
        core.callbacks = Some(Box::new(Forward(recorder.clone())));
        core.fire_osc_callback(2, "hello");
        core.fire_bell_callback();
        assert_eq!(recorder.osc.lock().unwrap().len(), 1);
        assert_eq!(*recorder.bell.lock().unwrap(), 1);
    }
}
