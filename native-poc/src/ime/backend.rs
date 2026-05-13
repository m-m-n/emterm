//! Phase 4-G-A: `ImeBackend` trait + ancillary types.
//!
//! The trait is the *only* seam between [`crate::app::App`] and the OS
//! IME clients (`X11Backend` / `WaylandBackend` / `WindowsBackend`). The
//! App holds a `Box<dyn ImeBackend>`, calls `dispatch_key_event` first
//! on every keyboard input, and drains queued events once per
//! event-loop tick via `pump`.
//!
//! See `doc/tasks/ime-native-integration/SPEC.md` §API Design and
//! §FR4-FR9.

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::pty::input::Modifiers;

/// Events produced by an IME backend and routed by the App to the
/// existing Phase 4-E layer (`on_ime_preedit` / `on_ime_commit` /
/// `on_ime_focus_lost`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeEvent {
    /// Composition-in-progress text to render under the cursor.
    Preedit(String),
    /// Finalized composition bytes to write to the active PTY.
    Commit(String),
    /// IM server signaled focus-out / disable; clear preedit overlay.
    FocusOut,
}

/// Result of `ImeBackend::dispatch_key_event`. `Consumed` skips the
/// remaining tao key path (no `tao_key_to_bytes` write). `Passthrough`
/// lets the existing Phase 4 path run unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDispatchResult {
    Consumed,
    Passthrough,
}

/// Why `ImeBackend::init` (or its factory wrapper) refused to bring up
/// a real backend. The App falls back to [`crate::ime::null::NullBackend`]
/// in every case and logs the reason exactly once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeInitError {
    /// The platform protocol is not available (no XIM server, no
    /// `zwp_text_input_manager_v3`, no `SetWindowSubclass`, the host
    /// is not the expected display server type, etc.).
    Unavailable(String),
    /// The `RawWindowHandle` / `RawDisplayHandle` variant did not match
    /// what this backend supports (e.g. X11 backend asked to init on a
    /// Wayland display).
    HandleType(String),
    /// A raw X11 / Wayland / Win32 call failed.
    PlatformError(String),
}

impl std::fmt::Display for ImeInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImeInitError::Unavailable(s) => write!(f, "Unavailable({s})"),
            ImeInitError::HandleType(s) => write!(f, "HandleType({s})"),
            ImeInitError::PlatformError(s) => write!(f, "PlatformError({s})"),
        }
    }
}

/// Captured from tao `WindowEvent::KeyboardInput` and converted into a
/// platform-neutral shape. The X11 backend rehydrates this into an
/// `XKeyPressedEvent` for `XFilterEvent`; the Wayland and Windows
/// backends ignore the body and always return `Passthrough` (their own
/// listeners are the IME truth source).
#[derive(Debug, Clone, Copy)]
pub struct RawKeyEvent {
    /// tao `scancode` / `physical_key` u32 — the raw OS scan code.
    pub physical_key_code: u32,
    /// `true` for key press, `false` for release.
    pub state_pressed: bool,
    /// Active modifier mask at the time of the event.
    pub mods: Modifiers,
}

/// OS-side IME client. Implemented by `NullBackend` (always available),
/// `X11Backend`, `WaylandBackend`, `WindowsBackend`.
///
/// Object-safety: `init` is intentionally **not** part of this trait so
/// the App can store a `Box<dyn ImeBackend>` regardless of the concrete
/// type. Each impl defines its own `init` (or factory) and the
/// [`build_backend`] free function below routes to the right one.
pub trait ImeBackend: Send {
    /// Offered every `WindowEvent::KeyboardInput`. `Consumed` means the
    /// IM server swallowed the key (composition open, candidate
    /// chosen, etc.); the App skips `tao_key_to_bytes` for this event.
    fn dispatch_key_event(&mut self, raw: &RawKeyEvent) -> KeyDispatchResult;

    /// Inform the IM server where the active cursor cell currently
    /// sits, so the candidate window can track it. Called by
    /// `App::notify_cursor_rect_if_changed` only when the cell changed
    /// — never every frame.
    fn notify_cursor_rect(&mut self, x_px: i32, y_px: i32, w_px: i32, h_px: i32);

    /// Window focus state change. Backends typically forward this to
    /// `XSetICFocus` / `XUnsetICFocus`, `enable` / `disable`, or rely
    /// on the OS to propagate `WM_SETFOCUS` / `WM_KILLFOCUS`.
    fn notify_focus(&mut self, focused: bool);

    /// Drain any queued `ImeEvent`s into `events`. Called once per
    /// event-loop tick by [`crate::app::App::pump_ime`]. Bounded to
    /// 1024 events per call (`IME_E901`); overflow is dropped with a
    /// single warn log.
    fn pump(&mut self, events: &mut Vec<ImeEvent>);

    /// Stable name used in startup / fallback logs ("null", "x11",
    /// "wayland", "windows"). Useful for asserting in tests which
    /// concrete backend was installed.
    fn name(&self) -> &'static str;
}

/// Max events drained from any single backend per pump call. See
/// SPEC §Error Codes IME_E901.
pub const PUMP_BUDGET: usize = 1024;

/// Startup factory. Resolves env > settings > runtime probe in that
/// order and returns the appropriate backend. The App always gets *a*
/// backend (never errors): on any failure, [`crate::ime::null::NullBackend`]
/// is returned and the reason is logged exactly once at `warn`.
///
/// Resolution rules:
///
/// 1. `EMTERM_NATIVE_IME=0` → `NullBackend`, log: `"ime: native integration disabled (env)"`.
/// 2. `settings.ime.native_integration == false` → `NullBackend`, log: `"ime: native integration disabled (settings)"`.
/// 3. otherwise → try the OS-appropriate real backend via [`build_platform_backend`]
///    - success → real backend + `info`: `"ime: <protocol> initialized"`
///    - failure → `NullBackend` + `warn`: `"ime: native integration disabled (<reason>)"`
pub fn build_backend(
    window: Option<RawWindowHandle>,
    display: Option<RawDisplayHandle>,
    settings: &crate::settings::ImeSettings,
    env: &dyn EnvLookup,
) -> Box<dyn ImeBackend> {
    if matches!(env.get("EMTERM_NATIVE_IME"), Some(ref v) if v == "0") {
        log::warn!("ime: native integration disabled (env)");
        return Box::new(crate::ime::null::NullBackend::new());
    }
    if !settings.native_integration {
        log::warn!("ime: native integration disabled (settings)");
        return Box::new(crate::ime::null::NullBackend::new());
    }
    match build_platform_backend(window, display) {
        Ok(b) => {
            log::info!("ime: {} initialized", b.name());
            b
        }
        Err(e) => {
            log::warn!("ime: native integration disabled ({e})");
            Box::new(crate::ime::null::NullBackend::new())
        }
    }
}

/// Build the OS-appropriate platform backend. Phase 4-G-A scaffolds
/// the dispatcher; the concrete OS backends are added in later phases:
///
/// - 4-G-B: `RawDisplayHandle::Xlib` → `X11Backend`
/// - 4-G-C: `RawDisplayHandle::Wayland` → `WaylandBackend`
/// - 4-G-D: `cfg(windows)` → `WindowsBackend`
///
/// Until those phases land, this returns
/// `Err(ImeInitError::Unavailable("no platform backend compiled in"))`
/// so the factory falls back to `NullBackend` and the App behavior
/// matches Phase 4 exactly.
pub fn build_platform_backend(
    window: Option<RawWindowHandle>,
    display: Option<RawDisplayHandle>,
) -> Result<Box<dyn ImeBackend>, ImeInitError> {
    // Phase 4-G-B: Linux X11 (XIM) backend probe. tao 0.34 reports
    // `RawDisplayHandle::Xlib` when the user runs under an X server
    // (including XWayland).
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(RawDisplayHandle::Xlib(_)) = display {
            return crate::ime::x11::X11Backend::init(window, display)
                .map(|b| Box::new(b) as Box<dyn ImeBackend>);
        }
        // Phase 4-G-C: Wayland probe.
        if let Some(RawDisplayHandle::Wayland(_)) = display {
            return crate::ime::wayland::WaylandBackend::init(window, display)
                .map(|b| Box::new(b) as Box<dyn ImeBackend>);
        }
    }

    // Phase 4-G-D will add the Windows probe below.

    let _ = (window, display);
    Err(ImeInitError::Unavailable(
        "no platform backend compiled in".to_string(),
    ))
}

/// Tiny indirection over `std::env::var` so tests can drive the
/// fallback decision tree deterministically without poking the real
/// process environment (which leaks across `#[test]` threads).
pub trait EnvLookup {
    fn get(&self, key: &str) -> Option<String>;
}

/// Production lookup: read from `std::env::var`.
pub struct ProcessEnv;

impl EnvLookup for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Test helpers (mock backend + env stub) reused across
    //! `backend.rs`, `app.rs`, and the integration tests for Phase
    //! 4-G-A regression guards.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Configurable mock backend driven by a shared `MockState`.
    /// `MockBackend::dispatch_key_event` returns whatever `state.next_dispatch`
    /// holds; `pump` drains `state.queue`.
    pub struct MockBackend {
        pub state: Arc<Mutex<MockState>>,
    }

    #[derive(Default)]
    pub struct MockState {
        /// Result returned by the next call to `dispatch_key_event`.
        /// Reset to `Passthrough` after each call so tests can stage
        /// per-call decisions.
        pub next_dispatch: KeyDispatchResult,
        /// Events queued for the next `pump` to drain.
        pub queue: Vec<ImeEvent>,
        /// Records.
        pub dispatch_calls: u32,
        pub pump_calls: u32,
        pub cursor_calls: Vec<(i32, i32, i32, i32)>,
        pub focus_calls: Vec<bool>,
    }

    impl Default for KeyDispatchResult {
        fn default() -> Self {
            KeyDispatchResult::Passthrough
        }
    }

    impl MockBackend {
        pub fn new() -> (Self, Arc<Mutex<MockState>>) {
            let state = Arc::new(Mutex::new(MockState::default()));
            (
                MockBackend {
                    state: state.clone(),
                },
                state,
            )
        }
    }

    impl ImeBackend for MockBackend {
        fn dispatch_key_event(&mut self, _raw: &RawKeyEvent) -> KeyDispatchResult {
            let mut s = self.state.lock().unwrap();
            s.dispatch_calls += 1;
            let r = s.next_dispatch;
            s.next_dispatch = KeyDispatchResult::Passthrough;
            r
        }

        fn notify_cursor_rect(&mut self, x: i32, y: i32, w: i32, h: i32) {
            self.state.lock().unwrap().cursor_calls.push((x, y, w, h));
        }

        fn notify_focus(&mut self, focused: bool) {
            self.state.lock().unwrap().focus_calls.push(focused);
        }

        fn pump(&mut self, events: &mut Vec<ImeEvent>) {
            let mut s = self.state.lock().unwrap();
            s.pump_calls += 1;
            // Bounded drain: SPEC says max PUMP_BUDGET events per pump.
            let drained = s.queue.drain(..).take(PUMP_BUDGET);
            events.extend(drained);
        }

        fn name(&self) -> &'static str {
            "mock"
        }
    }

    /// Map-backed env lookup. Tests stage individual keys via `with`.
    #[derive(Default)]
    pub struct StubEnv {
        pub vars: HashMap<String, String>,
    }

    impl StubEnv {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.into(), value.into());
            self
        }
    }

    impl EnvLookup for StubEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::settings::ImeSettings;

    fn pressed() -> RawKeyEvent {
        RawKeyEvent {
            physical_key_code: 0x26, // arbitrary
            state_pressed: true,
            mods: Modifiers::NONE,
        }
    }

    // ── TS-backend-1 ─────────────────────────────────────────────────
    // `NullBackend::dispatch_key_event` returns Passthrough for every
    // key. Covered in `null.rs` tests; backbone test here pins the
    // contract for *any* MockBackend default state (matches Phase 4
    // behavior).
    #[test]
    fn mock_backend_default_dispatch_is_passthrough() {
        let (mut b, _) = MockBackend::new();
        let r = b.dispatch_key_event(&pressed());
        assert_eq!(r, KeyDispatchResult::Passthrough);
    }

    // ── TS-backend-2 ─────────────────────────────────────────────────
    #[test]
    fn mock_backend_pump_empty_by_default() {
        let (mut b, _) = MockBackend::new();
        let mut out = Vec::new();
        b.pump(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn mock_backend_pump_drains_queue() {
        let (mut b, state) = MockBackend::new();
        state
            .lock()
            .unwrap()
            .queue
            .push(ImeEvent::Commit("x".into()));
        state
            .lock()
            .unwrap()
            .queue
            .push(ImeEvent::Preedit("y".into()));
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], ImeEvent::Commit("x".into()));
        assert_eq!(out[1], ImeEvent::Preedit("y".into()));
    }

    #[test]
    fn mock_backend_pump_respects_budget() {
        let (mut b, state) = MockBackend::new();
        for _ in 0..(PUMP_BUDGET + 5) {
            state
                .lock()
                .unwrap()
                .queue
                .push(ImeEvent::Commit("a".into()));
        }
        let mut out = Vec::new();
        b.pump(&mut out);
        assert_eq!(out.len(), PUMP_BUDGET);
    }

    // ── TS-fallback-1: EMTERM_NATIVE_IME=0 short-circuits to Null ────
    #[test]
    fn factory_env_disable_yields_null() {
        let env = StubEnv::new().with("EMTERM_NATIVE_IME", "0");
        // settings would otherwise allow real backend.
        let settings = ImeSettings {
            native_integration: true,
        };
        let backend = build_backend(None, None, &settings, &env);
        assert_eq!(backend.name(), "null");
    }

    #[test]
    fn factory_env_disable_overrides_settings() {
        // Even with settings true the env wins.
        let env = StubEnv::new().with("EMTERM_NATIVE_IME", "0");
        let settings = ImeSettings {
            native_integration: true,
        };
        let b = build_backend(None, None, &settings, &env);
        assert_eq!(b.name(), "null");
    }

    // ── TS-fallback-2: settings disable yields Null ──────────────────
    #[test]
    fn factory_settings_disable_yields_null_without_env() {
        let env = StubEnv::new();
        let settings = ImeSettings {
            native_integration: false,
        };
        let b = build_backend(None, None, &settings, &env);
        assert_eq!(b.name(), "null");
    }

    // ── TS-fallback-3: init failure → Null (no platform backend) ─────
    #[test]
    fn factory_init_failure_falls_back_to_null() {
        // No handles + no platform-backend compiled in returns
        // Unavailable, which the factory catches.
        let env = StubEnv::new();
        let settings = ImeSettings {
            native_integration: true,
        };
        let b = build_backend(None, None, &settings, &env);
        assert_eq!(b.name(), "null");
    }

    // ── ImeInitError Display ─────────────────────────────────────────
    #[test]
    fn init_error_display_renders_variant_and_reason() {
        let cases = [
            (
                ImeInitError::Unavailable("no xim".into()),
                "Unavailable(no xim)",
            ),
            (
                ImeInitError::HandleType("wrong".into()),
                "HandleType(wrong)",
            ),
            (
                ImeInitError::PlatformError("xopen fail".into()),
                "PlatformError(xopen fail)",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(format!("{err}"), expected);
        }
    }

    #[test]
    fn process_env_reads_real_var_when_unset_returns_none() {
        // Pick an env var that should never be set.
        let env = ProcessEnv;
        let key = "EMTERM_NATIVE_IME_DEFINITELY_UNSET_KEY_XYZZY";
        // Unconditionally remove just in case a previous test in this
        // process set it (each test owns its key naming so the global
        // env stays untouched).
        std::env::remove_var(key);
        assert!(env.get(key).is_none());
    }
}
