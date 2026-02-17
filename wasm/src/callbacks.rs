/// Callback infrastructure for OSC/APC/DCS/BEL/device response.
///
/// On wasm32: real JS Function callbacks via js_sys.
/// On native: no-op stubs (for cargo test).
use crate::terminal_core::TerminalCore;

// ── Callback type (platform-dependent) ───────────────────

#[cfg(target_arch = "wasm32")]
pub(crate) type Callback = js_sys::Function;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type Callback = ();

// ── Setter methods (wasm32 only, exported via wasm_bindgen) ──

#[cfg(target_arch = "wasm32")]
mod setters {
    use js_sys::Function;
    use wasm_bindgen::prelude::*;

    use crate::terminal_core::TerminalCore;

    #[wasm_bindgen]
    impl TerminalCore {
        pub fn set_osc_callback(&mut self, callback: JsValue) {
            self.osc_callback = callback.dyn_into::<Function>().ok();
        }

        pub fn set_apc_callback(&mut self, callback: JsValue) {
            self.apc_callback = callback.dyn_into::<Function>().ok();
        }

        pub fn set_dcs_callback(&mut self, callback: JsValue) {
            self.dcs_callback = callback.dyn_into::<Function>().ok();
        }

        pub fn set_bell_callback(&mut self, callback: JsValue) {
            self.bell_callback = callback.dyn_into::<Function>().ok();
        }

        pub fn set_device_response_callback(&mut self, callback: JsValue) {
            self.device_response_callback = callback.dyn_into::<Function>().ok();
        }

        pub fn clear_callbacks(&mut self) {
            self.osc_callback = None;
            self.apc_callback = None;
            self.dcs_callback = None;
            self.bell_callback = None;
            self.device_response_callback = None;
        }
    }
}

// ── Fire methods (wasm32: real JS calls) ─────────────────

#[cfg(target_arch = "wasm32")]
impl TerminalCore {
    pub(crate) fn fire_osc_callback(&self, action_type: u8, data: &str) {
        if let Some(ref cb) = self.osc_callback {
            let _ = cb.call2(
                &wasm_bindgen::JsValue::NULL,
                &wasm_bindgen::JsValue::from(action_type),
                &wasm_bindgen::JsValue::from(data),
            );
        }
    }

    pub(crate) fn fire_apc_callback(&self, data: &[u8]) {
        if let Some(ref cb) = self.apc_callback {
            let array = js_sys::Uint8Array::from(data);
            let _ = cb.call1(&wasm_bindgen::JsValue::NULL, &array);
        }
    }

    pub(crate) fn fire_dcs_callback(&self, data: &[u8]) {
        if let Some(ref cb) = self.dcs_callback {
            let array = js_sys::Uint8Array::from(data);
            let _ = cb.call1(&wasm_bindgen::JsValue::NULL, &array);
        }
    }

    pub(crate) fn fire_bell_callback(&self) {
        if let Some(ref cb) = self.bell_callback {
            let _ = cb.call0(&wasm_bindgen::JsValue::NULL);
        }
    }

    pub(crate) fn fire_device_response_callback(&self) {
        if let Some(ref cb) = self.device_response_callback {
            let data = &self.response_buffer[..self.response_len as usize];
            let array = js_sys::Uint8Array::from(data);
            let _ = cb.call1(&wasm_bindgen::JsValue::NULL, &array);
        }
    }
}

// ── Fire methods (native: no-op stubs for cargo test) ────

#[cfg(not(target_arch = "wasm32"))]
impl TerminalCore {
    pub(crate) fn fire_osc_callback(&self, _action_type: u8, _data: &str) {}

    pub(crate) fn fire_apc_callback(&self, _data: &[u8]) {}

    pub(crate) fn fire_dcs_callback(&self, _data: &[u8]) {}

    pub(crate) fn fire_bell_callback(&self) {}

    pub(crate) fn fire_device_response_callback(&self) {}
}

#[cfg(test)]
mod tests {
    use crate::terminal_core::TerminalCore;

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
        assert!(core.osc_callback.is_none());
        assert!(core.apc_callback.is_none());
        assert!(core.dcs_callback.is_none());
        assert!(core.bell_callback.is_none());
        assert!(core.device_response_callback.is_none());
    }
}
