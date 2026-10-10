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
/// Device responses (DSR / DA / XTWINOPS / DECRPM replies) are
/// deliberately NOT part of this trait (tmux-startup-query-response-leak
/// task0002, review-round-1 rework, D5 — an `on_device_response` method
/// existed here through task0001 and was removed). `TerminalCore` holds
/// exactly one delivery channel for those: the ordered pending-response
/// store drained via [`TerminalCore::take_response`]. A second,
/// callback-based channel for the same bytes would let a host observe —
/// and a careless one deliver — a query's reply more than once, which is
/// exactly the leak task0001 fixed; task0002 removes the redundant
/// channel outright instead of leaving it as a documented no-op.
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

    /// Screen-aware OSC dispatch (osc7501-alt-screen-prompt-mark SC-1).
    ///
    /// `term_core` calls THIS method — never [`Self::on_osc`] directly — once
    /// per dispatched OSC, with the same `action_type` and `data` that
    /// `on_osc` has always received, plus `alt_screen_active`: whether the
    /// alternate screen was active when the OSC was dispatched. The flag is
    /// the very `MODE_ALT_SCREEN` bit that the OSC 133 live-mark gate in
    /// `osc_handler` reads in the same dispatch, so a host can tell which
    /// notifications correspond to marks `term_core` kept (`false`) and
    /// which it suppressed (`true`). It is read at the dispatch itself, so it
    /// is correct across the buffer-switch interrupts inside one resumed
    /// parse call and cannot go stale when callbacks move between cores.
    ///
    /// Fired in the same synchronous call position `on_osc` always had.
    ///
    /// Default: forward to [`Self::on_osc`] with the same action id and
    /// payload, dropping the flag. A host that does not override this method
    /// therefore observes exactly the sequence of `on_osc` calls it observed
    /// before this method existed. An override replaces `on_osc` for the
    /// dispatch path: `term_core` never calls both for one OSC, so an
    /// override that still wants the `on_osc` behavior for some actions
    /// forwards to it explicitly (exactly once per notification).
    fn on_osc_with_screen(&self, action_type: u8, data: &str, alt_screen_active: bool) {
        let _ = alt_screen_active;
        self.on_osc(action_type, data);
    }

    /// Application Program Command payload (used by the Kitty image
    /// protocol). `data` is the raw APC bytes.
    fn on_apc(&self, data: &[u8]);

    /// Device Control String payload (used by Sixel).
    fn on_dcs(&self, data: &[u8]);

    /// BEL (0x07).
    fn on_bell(&self);

    /// A full terminal reset (RIS, `ESC c`) just ran (cursor-settings-fix
    /// FR4). `term_core` clears its own cursor shape/blink overrides
    /// unconditionally inside `TerminalCore::reset()`; a host that layers a
    /// GUI-side cursor-COLOR override on top (OSC 12, tracked entirely
    /// outside `term_core` — see `TerminalCore::apply_osc`'s callback
    /// consumer) uses this hook to mirror that clearing. Fired synchronously
    /// from `reset()`, in the same parse-order position as `on_osc` /
    /// `on_bell`, so a byte stream that continues past the reset with a
    /// fresh OSC 12 in the SAME chunk (`ESC c` followed later by
    /// `OSC 12;...`) still applies after this fires — a host must not defer
    /// the restore past the end of the enclosing parse call, or it would
    /// clobber that later OSC 12.
    ///
    /// Default no-op: most hosts (and the `term_core`-internal test
    /// doubles) have no such GUI-side state to restore.
    fn on_reset(&self) {}
}

// ── Fire methods ────────────────────────────────────────
//
// These are called from the various CSI / OSC / APC / DCS / C0 handlers.
// They short-circuit when no callback sink is installed, matching the
// previous wasm/native behaviour (wasm: no callback = silent drop;
// native: stub previously did the same).

impl TerminalCore {
    /// Notify the installed callbacks of one dispatched OSC through SC-1
    /// ([`TerminalCallbacks::on_osc_with_screen`]), together with the
    /// alternate-screen state read at this very dispatch. Both dispatch sites
    /// in `osc_handler` (the OSC 8 early return and the general path) fire
    /// through here, so every dispatched OSC yields exactly one notification
    /// and the flag is the same `MODE_ALT_SCREEN` bit the OSC 133 live-mark
    /// gate reads for that dispatch. A single bit test: no allocation, no
    /// I/O, no lock.
    pub(crate) fn fire_osc_callback(&self, action_type: u8, data: &str) {
        if let Some(cb) = self.callbacks.as_deref() {
            cb.on_osc_with_screen(
                action_type,
                data,
                self.get_mode(crate::terminal_core::MODE_ALT_SCREEN),
            );
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

    /// Fired from [`TerminalCore::reset`] — see [`TerminalCallbacks::on_reset`].
    pub(crate) fn fire_reset_callback(&self) {
        if let Some(cb) = self.callbacks.as_deref() {
            cb.on_reset();
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
        reset: Mutex<usize>,
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
        fn on_reset(&self) {
            *self.reset.lock().unwrap() += 1;
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
    fn test_fire_reset_callback_none_does_not_panic() {
        let core = TerminalCore::new(80, 24, 0);
        core.fire_reset_callback();
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
        }
        core.callbacks = Some(Box::new(Forward(recorder.clone())));
        core.fire_osc_callback(2, "hello");
        core.fire_bell_callback();
        assert_eq!(recorder.osc.lock().unwrap().len(), 1);
        assert_eq!(*recorder.bell.lock().unwrap(), 1);
    }

    /// Build a core whose callbacks forward to a fresh `Recorder`.
    fn core_with_recorder() -> (TerminalCore, std::sync::Arc<Recorder>) {
        let mut core = TerminalCore::new(80, 24, 0);
        let recorder = std::sync::Arc::new(Recorder::default());
        struct Fwd(std::sync::Arc<Recorder>);
        impl TerminalCallbacks for Fwd {
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
            fn on_reset(&self) {
                self.0.on_reset()
            }
        }
        core.callbacks = Some(Box::new(Fwd(recorder.clone())));
        (core, recorder)
    }

    // ── on_reset (cursor-settings-fix task0004 AC-5) ──────────────────

    #[test]
    fn test_reset_fires_on_reset_callback() {
        let (mut core, recorder) = core_with_recorder();
        core.reset();
        assert_eq!(*recorder.reset.lock().unwrap(), 1);
    }

    #[test]
    fn test_ris_bytes_fire_on_reset_callback() {
        // AC-5: feeding RIS bytes through the normal parse path fires the
        // same signal as a direct `reset()` call (RIS dispatches through
        // `esc_full_reset` -> `reset()`).
        let (mut core, recorder) = core_with_recorder();
        core.process_pty_data_fully(b"\x1bc");
        assert_eq!(*recorder.reset.lock().unwrap(), 1);
    }

    // ── NFR5: app-layer OSC param override (term_core holds no mux constant) ──

    #[test]
    fn osc_app_param_override_maps_unknown_param_to_app_action_type() {
        // A core that registered the mux OSC param (9999 → 102) maps an
        // otherwise-unknown OSC 9999 to the injected action_type and delivers
        // it via on_osc — term_core itself embeds no `9999`/`emterm-mux;`.
        let (mut core, recorder) = core_with_recorder();
        core.register_osc_app_param(9999, 102);
        core.process_pty_data_fully(b"\x1b]9999;emterm-mux;QQ==\x1b\\");
        assert_eq!(
            recorder.osc.lock().unwrap().as_slice(),
            &[(102u8, "emterm-mux;QQ==".to_string())]
        );
    }

    #[test]
    fn osc_app_param_unregistered_is_unknown() {
        // Without registration, term_core does not recognize OSC 9999: it is
        // delivered as action_type 255 (Unknown), proving the mux protocol
        // number is not baked into term_core.
        let (mut core, recorder) = core_with_recorder();
        core.process_pty_data_fully(b"\x1b]9999;emterm-mux;QQ==\x1b\\");
        assert_eq!(
            recorder.osc.lock().unwrap().as_slice(),
            &[(255u8, "emterm-mux;QQ==".to_string())]
        );
    }

    #[test]
    fn osc_app_param_override_does_not_shadow_native_osc() {
        // An override is consulted ONLY for params term_core does not natively
        // handle. Registering a native param (e.g. OSC 2 SetTitle) must NOT
        // hijack its native action_type.
        let (mut core, recorder) = core_with_recorder();
        core.register_osc_app_param(2, 200);
        core.process_pty_data_fully(b"\x1b]2;hi\x1b\\");
        assert_eq!(
            recorder.osc.lock().unwrap().as_slice(),
            &[(2u8, "hi".to_string())]
        );
    }

    // ── SC-1: screen-aware OSC notification (osc7501-alt-screen-prompt-mark
    // task0001 AC-1 / AC-2) ───────────────────────────────────────────────

    /// Callbacks that override the screen-aware notification and record every
    /// one of them as `(action_type, payload, alt_screen_active)`. The
    /// existing `on_osc` is a deliberate no-op: the override replaces it for
    /// the dispatch path, so a recorded entry per OSC proves the dispatch
    /// went through SC-1 exactly once.
    #[derive(Default)]
    struct ScreenRecorder {
        osc: Mutex<Vec<(u8, String, bool)>>,
    }

    struct ScreenForward(std::sync::Arc<ScreenRecorder>);

    impl TerminalCallbacks for ScreenForward {
        fn on_osc(&self, _action_type: u8, _data: &str) {}
        fn on_osc_with_screen(&self, action_type: u8, data: &str, alt_screen_active: bool) {
            self.0
                .osc
                .lock()
                .unwrap()
                .push((action_type, data.to_string(), alt_screen_active));
        }
        fn on_apc(&self, _data: &[u8]) {}
        fn on_dcs(&self, _data: &[u8]) {}
        fn on_bell(&self) {}
    }

    /// Feed `bytes` to a fresh core whose callbacks override SC-1 and return
    /// the recorded notifications.
    fn screen_notifications(bytes: &[u8]) -> Vec<(u8, String, bool)> {
        let mut core = TerminalCore::new(80, 24, 0);
        let recorder = std::sync::Arc::new(ScreenRecorder::default());
        core.callbacks = Some(Box::new(ScreenForward(recorder.clone())));
        core.process_pty_data_fully(bytes);
        let seen = recorder.osc.lock().unwrap().clone();
        seen
    }

    fn note(action: u8, data: &str, alt: bool) -> (u8, String, bool) {
        (action, data.to_string(), alt)
    }

    #[test]
    fn ac1_an_osc_after_each_alt_screen_enable_is_reported_with_alt_true() {
        for enable in ["\x1b[?1049h", "\x1b[?1047h", "\x1b[?47h"] {
            let bytes = format!("\x1b]2;main\x07{enable}\x1b]2;alt\x07");
            assert_eq!(
                screen_notifications(bytes.as_bytes()),
                vec![note(2, "main", false), note(2, "alt", true)],
                "switch sequence {enable:?}"
            );
        }
    }

    #[test]
    fn ac1_an_osc_after_the_matching_disable_in_the_same_parse_call_is_reported_with_alt_false() {
        for (enable, disable) in [
            ("\x1b[?1049h", "\x1b[?1049l"),
            ("\x1b[?1047h", "\x1b[?1047l"),
            ("\x1b[?47h", "\x1b[?47l"),
        ] {
            let bytes = format!("{enable}\x1b]2;alt\x07{disable}\x1b]2;back\x07");
            assert_eq!(
                screen_notifications(bytes.as_bytes()),
                vec![note(2, "alt", true), note(2, "back", false)],
                "switch sequences {enable:?} / {disable:?}"
            );
        }
    }

    #[test]
    fn ac1_an_osc_after_a_ris_issued_on_the_alt_screen_is_reported_with_alt_false() {
        assert_eq!(
            screen_notifications(b"\x1b[?1049h\x1b]2;alt\x07\x1bc\x1b]2;after-ris\x07"),
            vec![note(2, "alt", true), note(2, "after-ris", false)]
        );
    }

    #[test]
    fn ac1_each_dispatched_osc_yields_exactly_one_notification_in_byte_order() {
        // Mixes the OSC 8 early-return dispatch site with the general site,
        // terminators BEL and ST, and a known and an unknown OSC number.
        let bytes: &[u8] = b"\x1b]8;;http://a.example\x07\
\x1b]133;A\x07\
\x1b[?1049h\
\x1b]8;;\x1b\\\
\x1b]133;B\x1b\\\
\x1b]9999;unknown\x07\
\x1b[?1049l\
\x1b]2;t\x07";
        assert_eq!(
            screen_notifications(bytes),
            vec![
                note(8, ";http://a.example", false),
                note(133, "A", false),
                note(8, ";", true),
                note(133, "B", true),
                note(255, "unknown", true),
                note(2, "t", false),
            ]
        );
    }

    #[test]
    fn ac1_no_installed_callbacks_means_no_notification_and_no_panic() {
        let mut core = TerminalCore::new(80, 24, 0);
        assert!(core.callbacks.is_none());
        core.process_pty_data_fully(b"\x1b[?1049h\x1b]2;alt\x07\x1b]8;;u\x07");
    }

    #[test]
    fn ac2_callbacks_implementing_only_on_osc_see_the_unchanged_sequence() {
        // `core_with_recorder` installs callbacks that implement ONLY the
        // existing notification. A stream mixing main-screen and
        // alt-screen OSCs reaches them as the same (action, payload)
        // sequence, in byte order, with no extra and no missing entries.
        let (mut core, recorder) = core_with_recorder();
        core.process_pty_data_fully(
            b"\x1b]2;one\x07\x1b[?1049h\x1b]133;A\x07\x1b]8;;http://x\x07\
\x1b[?1049l\x1b]133;B\x07\x1b]2;two\x1b\\",
        );
        assert_eq!(
            recorder.osc.lock().unwrap().as_slice(),
            &[
                (2u8, "one".to_string()),
                (133u8, "A".to_string()),
                (8u8, ";http://x".to_string()),
                (133u8, "B".to_string()),
                (2u8, "two".to_string()),
            ]
        );
    }

    #[test]
    fn ac2_the_default_screen_aware_notification_forwards_to_on_osc_regardless_of_the_flag() {
        let recorder = Recorder::default();
        recorder.on_osc_with_screen(133, "A", false);
        recorder.on_osc_with_screen(133, "A", true);
        assert_eq!(
            recorder.osc.lock().unwrap().as_slice(),
            &[(133u8, "A".to_string()), (133u8, "A".to_string())]
        );
    }
}
