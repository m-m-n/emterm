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
//!
//! Phase 6 extends the OSC match arm to cover **every** `action_type`
//! emitted by `term_core::osc_handler` (0, 1, 2, 4, 7, 8, 9, 10, 11, 12,
//! 22, 52, 104, 110, 111, 112, 133, 100 (wire 777), 101 (wire 1337), 255),
//! adds OSC 9 notifications via the `NotificationSink` trait (with
//! production `NotifyRustSink` and a `TestSink` for unit tests), and
//! gates OSC 52 by `settings.clipboard_read_osc52` /
//! `clipboard_max_size_osc52`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use term_core::callbacks::TerminalCallbacks;

use crate::render::theme::Theme;
use crate::settings::Settings;

// ── OSC action_type constants (matches term_core::osc_handler) ───────────

/// OSC 0: set window title and icon name.
pub const OSC_SET_TITLE_AND_ICON: u8 = 0;
/// OSC 1: set icon name only.
pub const OSC_SET_ICON_NAME: u8 = 1;
/// OSC 2: set window title only.
pub const OSC_SET_TITLE: u8 = 2;
/// OSC 4: set indexed palette entry.
pub const OSC_SET_COLOR_PALETTE: u8 = 4;
/// OSC 7: set working directory (file:// URI in `data`).
pub const OSC_SET_WORKING_DIRECTORY: u8 = 7;
/// OSC 8: hyperlink (term_core already stored the URI; this callback is
/// observational).
pub const OSC_HYPERLINK: u8 = 8;
/// OSC 9: desktop notification.
pub const OSC_NOTIFICATION: u8 = 9;
/// OSC 10: set default foreground color.
pub const OSC_SET_FG: u8 = 10;
/// OSC 11: set default background color.
pub const OSC_SET_BG: u8 = 11;
/// OSC 12: set cursor foreground color.
pub const OSC_SET_CURSOR_FG: u8 = 12;
/// OSC 22: cursor style.
pub const OSC_CURSOR_STYLE: u8 = 22;
/// OSC 52: clipboard read/write.
pub const OSC_CLIPBOARD: u8 = 52;
/// OSC 104: reset palette entry/all.
pub const OSC_RESET_COLOR_PALETTE: u8 = 104;
/// OSC 110: reset default foreground.
pub const OSC_RESET_FG: u8 = 110;
/// OSC 111: reset default background.
pub const OSC_RESET_BG: u8 = 111;
/// OSC 112: reset cursor color.
pub const OSC_RESET_CURSOR_FG: u8 = 112;
/// OSC 133: semantic prompt mark.
pub const OSC_SEMANTIC_PROMPT: u8 = 133;
/// OSC 100 (wire 777): emterm extension (viewer/markdown/image).
pub const OSC_EMTERM_EXTENSION: u8 = 100;
/// OSC 101 (wire 1337): iTerm2 protocol.
pub const OSC_ITERM2: u8 = 101;
/// OSC 255: unknown / unmapped action.
pub const OSC_UNKNOWN: u8 = 255;

// ── Log message constants ───────────────────────────────────────────────

/// Emitted when an OSC 52 payload is dropped because of the policy gate
/// (read disabled or size over `clipboard_max_size_osc52`).
pub const LOG_OSC52_DENIED: &str = "LOG_OSC52_DENIED";
/// Emitted when an OSC 9 notification is suppressed by the rate limiter.
pub const LOG_NOTIFY_RATE_LIMIT: &str = "LOG_NOTIFY_RATE_LIMIT";

// ── Public types ────────────────────────────────────────────────────────

/// Emterm OSC viewer-spawn request decoded from an OSC 777 payload. Phase 6
/// passes these to the (future) Wry viewer spawner. For now native-poc only
/// records them so we can verify the dispatch path end-to-end.
#[derive(Debug, Clone)]
pub struct EmtermOscRequest {
    /// Raw payload — read by the (Phase 5+) Wry viewer spawner.
    #[allow(dead_code)]
    pub payload: String,
}

/// OSC 133 semantic-prompt sub-type.
///
/// See <https://gitlab.freedesktop.org/Per_Bothner/specifications/-/blob/master/proposals/semantic-prompts.md>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMarkKind {
    /// `A` — prompt start.
    PromptStart,
    /// `B` — command start (user input begins).
    CommandStart,
    /// `C` — command exec (user input ends, command runs).
    CommandExec,
    /// `D` — command end (exit-code-bearing).
    CommandEnd,
}

/// A persisted OSC 133 mark. `row` is the absolute scrollback-aware row at
/// the moment the mark was received; consumers (future search/jump UI)
/// use this to step between prompts.
#[derive(Debug, Clone)]
pub struct PromptMark {
    pub kind: PromptMarkKind,
    /// Best-effort row index — currently `0` because `NativeCallbacks` has
    /// no read access to `TerminalCore` (callbacks are `&self`); the
    /// renderer (which *does* have the core) can backfill this when it
    /// drains marks.
    pub row: u32,
    /// Optional exit code attached to a `CommandEnd` mark.
    pub exit_code: Option<i32>,
}

/// Trait abstracting the OS-notification surface. `NotifyRustSink` is the
/// production impl; `TestSink` captures `send` calls in unit tests so we
/// can verify rate-limit behavior without depending on a real D-Bus
/// connection.
pub trait NotificationSink: Send + Sync {
    fn send(&self, title: &str, body: &str);
}

/// Production sink that uses `notify-rust` to deliver desktop
/// notifications over D-Bus on Linux. On Windows the same crate uses the
/// platform's toast API. Failures (e.g. no D-Bus in a container) are
/// logged but never panic.
pub struct NotifyRustSink;

impl NotificationSink for NotifyRustSink {
    fn send(&self, title: &str, body: &str) {
        match notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .show()
        {
            Ok(_) => log::debug!("notify-rust dispatched: {title}"),
            Err(e) => log::warn!("notify-rust failed: {e}"),
        }
    }
}

/// Rate limiter that suppresses identical `(title, body)` pairs within a
/// fixed dedupe window (default 1 s, per IMPLEMENTATION.md OQ3).
///
/// `clock` is injected to keep tests deterministic.
pub struct NotificationRateLimiter {
    last: Mutex<HashMap<(String, String), Instant>>,
    window: Duration,
    clock: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl std::fmt::Debug for NotificationRateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationRateLimiter")
            .field("window", &self.window)
            .finish()
    }
}

impl NotificationRateLimiter {
    /// Construct a rate limiter with the production wall clock and the
    /// default 1 s dedupe window.
    pub fn with_default_clock() -> Self {
        Self::new(Duration::from_secs(1), Box::new(Instant::now))
    }

    /// Construct a rate limiter with an explicit `window` and `clock`
    /// closure. The closure must be `Send + Sync` because `NativeCallbacks`
    /// is shared across threads.
    pub fn new(window: Duration, clock: Box<dyn Fn() -> Instant + Send + Sync>) -> Self {
        Self {
            last: Mutex::new(HashMap::new()),
            window,
            clock,
        }
    }

    /// Returns `true` if the caller should emit the notification, `false`
    /// if it duplicates one emitted within the dedupe window. On `true`
    /// the limiter records the current clock for future dedupe.
    pub fn should_emit(&self, title: &str, body: &str) -> bool {
        let now = (self.clock)();
        let mut map = self.last.lock();

        // Opportunistic GC: evict entries older than 4× the window so the
        // map cannot grow unbounded for long-running sessions that emit
        // many distinct notifications.
        let stale_cutoff = self.window.checked_mul(4).unwrap_or(self.window);
        map.retain(|_, &mut t| now.duration_since(t) <= stale_cutoff);

        let key = (title.to_string(), body.to_string());
        match map.get(&key) {
            Some(&prev) if now.duration_since(prev) < self.window => false,
            _ => {
                map.insert(key, now);
                true
            }
        }
    }
}

/// Shared mutable state populated by `NativeCallbacks` and drained by the
/// `Tab` / UI layer.
#[derive(Debug, Default)]
pub struct NativeCallbackState {
    /// Latest OSC 0/2 title received from the shell.
    pub title: Option<String>,
    /// Latest OSC 1 icon name received from the shell. We surface this
    /// only as state because there is no icon-bar UI in Phase 6.
    pub icon_name: Option<String>,
    /// Latest OSC 7 current working directory. Useful for future
    /// "duplicate tab here" affordances.
    pub cwd: Option<String>,
    /// Pending emterm-extension viewer requests.
    pub osc_queue: Vec<EmtermOscRequest>,
    /// BEL counter (used for visual-bell or audible-bell hooks in later phases).
    pub bell_count: u32,
    /// Pending OSC 9 notifications, drained by `Tab::pump` and dispatched
    /// to the `NotificationSink`. Buffering here keeps the callback fast
    /// (no D-Bus round-trip inside `process_pty_data`).
    pub pending_notifications: Vec<(String, String)>,
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
    /// OSC 133 prompt marks accumulated in arrival order. Future search /
    /// jump UI reads this; Phase 6 only persists.
    pub prompt_marks: Vec<PromptMark>,
    /// Pending OSC 52 *write* payloads — `(target, decoded_text)`. The UI
    /// thread drains these into `arboard`, because arboard's
    /// `Clipboard::new()` cannot be safely shared across threads.
    pub pending_clipboard_writes: Vec<(String, String)>,
    /// Pending OSC 52 *read* requests — `target` only. The UI thread
    /// reads from `arboard` and posts the response back through the PTY.
    pub pending_clipboard_reads: Vec<String>,
    /// Latched flag — set when any OSC color / style mutation modified
    /// the shared `Theme`. `Tab::pump` drains it and calls
    /// `TerminalCore::mark_all_dirty` so the next frame repaints with
    /// the new palette.
    pub theme_dirty: bool,
}

/// `TerminalCallbacks` implementation for native consumers.
pub struct NativeCallbacks {
    state: Arc<Mutex<NativeCallbackState>>,
    theme: Arc<Mutex<Theme>>,
    settings: Arc<Settings>,
    sink: Arc<dyn NotificationSink>,
    rate_limiter: Arc<NotificationRateLimiter>,
}

impl NativeCallbacks {
    /// Construct callbacks with the production `NotifyRustSink`.
    pub fn new(
        state: Arc<Mutex<NativeCallbackState>>,
        theme: Arc<Mutex<Theme>>,
        settings: Arc<Settings>,
    ) -> Self {
        Self::with_sink(
            state,
            theme,
            settings,
            Arc::new(NotifyRustSink),
            Arc::new(NotificationRateLimiter::with_default_clock()),
        )
    }

    /// Construct callbacks with a caller-provided sink (used by tests).
    pub fn with_sink(
        state: Arc<Mutex<NativeCallbackState>>,
        theme: Arc<Mutex<Theme>>,
        settings: Arc<Settings>,
        sink: Arc<dyn NotificationSink>,
        rate_limiter: Arc<NotificationRateLimiter>,
    ) -> Self {
        Self {
            state,
            theme,
            settings,
            sink,
            rate_limiter,
        }
    }

    fn mark_theme_dirty(&self) {
        self.state.lock().theme_dirty = true;
    }

    /// Returns and resets the "theme changed" flag. `Tab::pump` calls
    /// this once per frame and, if `true`, asks `TerminalCore` to mark
    /// every row dirty so the new palette takes effect on the next frame.
    pub fn take_theme_dirty(&self) -> bool {
        let mut st = self.state.lock();
        let was = st.theme_dirty;
        st.theme_dirty = false;
        was
    }

    // ── per-action handlers ────────────────────────────────────────────

    fn handle_title(&self, data: &str) {
        self.state.lock().title = Some(data.to_string());
    }

    fn handle_title_and_icon(&self, data: &str) {
        let mut s = self.state.lock();
        s.title = Some(data.to_string());
        s.icon_name = Some(data.to_string());
    }

    fn handle_icon_name(&self, data: &str) {
        self.state.lock().icon_name = Some(data.to_string());
    }

    fn handle_cwd(&self, data: &str) {
        self.state.lock().cwd = Some(data.to_string());
    }

    fn handle_theme(&self, action_type: u8, data: &str) {
        let changed = self.theme.lock().apply_osc(action_type, data);
        if changed {
            self.mark_theme_dirty();
        }
    }

    fn handle_notify(&self, data: &str) {
        let (title, body) = parse_osc9(data, self.state.lock().title.as_deref());
        if self.rate_limiter.should_emit(&title, &body) {
            self.state
                .lock()
                .pending_notifications
                .push((title.clone(), body.clone()));
            self.sink.send(&title, &body);
        } else {
            log::warn!("{LOG_NOTIFY_RATE_LIMIT}: '{title}' / '{body}'");
        }
    }

    fn handle_clipboard(&self, data: &str) {
        let action = match parse_osc52(data) {
            Some(a) => a,
            None => return,
        };
        match action {
            Osc52Action::Write { target, payload } => {
                // Decode base64; reject if invalid or oversized.
                let decoded = match base64_decode(&payload) {
                    Some(d) => d,
                    None => {
                        log::debug!("OSC 52: invalid base64");
                        return;
                    }
                };
                if decoded.len() > self.settings.clipboard_max_size_osc52 as usize {
                    log::warn!(
                        "{LOG_OSC52_DENIED}: write target={target} len={} cap={}",
                        decoded.len(),
                        self.settings.clipboard_max_size_osc52
                    );
                    return;
                }
                let text = match String::from_utf8(decoded) {
                    Ok(t) => t,
                    Err(_) => {
                        log::debug!("OSC 52: non-utf8 payload");
                        return;
                    }
                };
                self.state
                    .lock()
                    .pending_clipboard_writes
                    .push((target, text));
            }
            Osc52Action::Query { target } => {
                if !self.settings.clipboard_read_osc52 {
                    log::warn!("{LOG_OSC52_DENIED}: read target={target} (disabled)");
                    return;
                }
                self.state.lock().pending_clipboard_reads.push(target);
            }
            Osc52Action::Clear { target } => {
                self.state
                    .lock()
                    .pending_clipboard_writes
                    .push((target, String::new()));
            }
        }
    }

    fn handle_semantic_prompt(&self, data: &str) {
        // term_core does not yet hand us the row, so we record marks in
        // arrival order. Future renderer integration can correlate the
        // mark with the cursor row by reading the core under the same
        // lock that drains `prompt_marks`.
        let (kind, exit_code) = parse_osc133(data);
        if let Some(kind) = kind {
            self.state.lock().prompt_marks.push(PromptMark {
                kind,
                row: 0,
                exit_code,
            });
        }
    }
}

impl TerminalCallbacks for NativeCallbacks {
    fn on_osc(&self, action_type: u8, data: &str) {
        match action_type {
            OSC_SET_TITLE_AND_ICON => self.handle_title_and_icon(data),
            OSC_SET_ICON_NAME => self.handle_icon_name(data),
            OSC_SET_TITLE => self.handle_title(data),
            OSC_SET_COLOR_PALETTE
            | OSC_SET_FG
            | OSC_SET_BG
            | OSC_SET_CURSOR_FG
            | OSC_CURSOR_STYLE
            | OSC_RESET_COLOR_PALETTE
            | OSC_RESET_FG
            | OSC_RESET_BG
            | OSC_RESET_CURSOR_FG => self.handle_theme(action_type, data),
            OSC_SET_WORKING_DIRECTORY => self.handle_cwd(data),
            OSC_HYPERLINK => {
                // term_core already registered the URI; native-poc only
                // logs here for traceability.
                log::debug!("OSC 8 hyperlink seen: {data}");
            }
            OSC_NOTIFICATION => self.handle_notify(data),
            OSC_CLIPBOARD => self.handle_clipboard(data),
            OSC_SEMANTIC_PROMPT => self.handle_semantic_prompt(data),
            OSC_EMTERM_EXTENSION => self.state.lock().osc_queue.push(EmtermOscRequest {
                payload: data.to_string(),
            }),
            OSC_ITERM2 => {
                // OQ7: log only — no inline-image subset is implemented.
                log::warn!("OSC 1337 (iTerm2) ignored: {} bytes", data.len());
            }
            OSC_UNKNOWN => {
                log::warn!("OSC unknown action_type=255: {} bytes", data.len());
            }
            _ => {
                log::debug!(
                    "OSC unhandled action_type={action_type}: {} bytes",
                    data.len()
                );
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

// ── OSC 9 parsing ───────────────────────────────────────────────────────

/// Split an OSC 9 payload into `(title, body)`.
///
/// xterm's OSC 9 is a free-form string; common conventions split on the
/// first `;` so that `\033]9;Build done;all green\007` becomes
/// `(title="Build done", body="all green")`. If no separator is present
/// we fall back to the current tab title (or "emterm") for the title.
fn parse_osc9(data: &str, fallback_title: Option<&str>) -> (String, String) {
    if let Some(idx) = data.find(';') {
        let title = data[..idx].trim();
        let body = data[idx + 1..].trim();
        if !title.is_empty() {
            return (title.to_string(), body.to_string());
        }
    }
    (
        fallback_title.unwrap_or("emterm").to_string(),
        data.trim().to_string(),
    )
}

// ── OSC 52 parsing ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Osc52Action {
    Write { target: String, payload: String },
    Query { target: String },
    Clear { target: String },
}

fn parse_osc52(data: &str) -> Option<Osc52Action> {
    if data.is_empty() {
        return None;
    }
    let idx = data.find(';')?;
    let target = data[..idx].to_string();
    let payload = &data[idx + 1..];
    if payload == "?" {
        Some(Osc52Action::Query { target })
    } else if payload.is_empty() {
        Some(Osc52Action::Clear { target })
    } else {
        Some(Osc52Action::Write {
            target,
            payload: payload.to_string(),
        })
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .ok()
}

// ── OSC 133 parsing ─────────────────────────────────────────────────────

/// Parse OSC 133 payload into `(kind, exit_code)`.
///
/// Forms (subset of the FinalTerm/iTerm2 spec):
/// - `A` → `PromptStart`
/// - `B` → `CommandStart`
/// - `C` → `CommandExec`
/// - `D` or `D;<n>` → `CommandEnd` (optional exit code)
fn parse_osc133(data: &str) -> (Option<PromptMarkKind>, Option<i32>) {
    let mut it = data.split(';');
    let head = it.next().unwrap_or("");
    let kind = match head {
        "A" => Some(PromptMarkKind::PromptStart),
        "B" => Some(PromptMarkKind::CommandStart),
        "C" => Some(PromptMarkKind::CommandExec),
        "D" => Some(PromptMarkKind::CommandEnd),
        _ => None,
    };
    let exit_code = if kind == Some(PromptMarkKind::CommandEnd) {
        it.next().and_then(|s| s.parse::<i32>().ok())
    } else {
        None
    };
    (kind, exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::theme::{CursorStyle, Rgb};

    // ── Test infrastructure ─────────────────────────────────────────────

    /// Capturing `NotificationSink` for unit tests.
    #[derive(Default)]
    struct TestSink {
        calls: Mutex<Vec<(String, String)>>,
    }

    impl TestSink {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().clone()
        }
    }

    impl NotificationSink for TestSink {
        fn send(&self, title: &str, body: &str) {
            self.calls
                .lock()
                .push((title.to_string(), body.to_string()));
        }
    }

    /// Bag of test handles so tests can poke at every shared piece without
    /// re-wiring constructors.
    struct Harness {
        cb: NativeCallbacks,
        state: Arc<Mutex<NativeCallbackState>>,
        theme: Arc<Mutex<Theme>>,
        #[allow(dead_code)]
        sink: Arc<TestSink>,
        #[allow(dead_code)]
        clock: Arc<Mutex<Instant>>,
    }

    fn harness(settings: Settings) -> Harness {
        let state = Arc::new(Mutex::new(NativeCallbackState::default()));
        let theme = Arc::new(Mutex::new(Theme::default()));
        let sink: Arc<TestSink> = TestSink::new();
        let clock = Arc::new(Mutex::new(Instant::now()));
        let clk = clock.clone();
        let rl = Arc::new(NotificationRateLimiter::new(
            Duration::from_secs(1),
            Box::new(move || *clk.lock()),
        ));
        let cb = NativeCallbacks::with_sink(
            state.clone(),
            theme.clone(),
            Arc::new(settings),
            sink.clone() as Arc<dyn NotificationSink>,
            rl,
        );
        Harness {
            cb,
            state,
            theme,
            sink,
            clock,
        }
    }

    fn default_harness() -> Harness {
        harness(Settings::default())
    }

    // ── Per-action_type dispatch tests ──────────────────────────────────

    #[test]
    fn osc_0_sets_title_and_icon() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_TITLE_AND_ICON, "hello");
        let s = h.state.lock();
        assert_eq!(s.title.as_deref(), Some("hello"));
        assert_eq!(s.icon_name.as_deref(), Some("hello"));
    }

    #[test]
    fn osc_1_sets_icon_name_only() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_ICON_NAME, "icon");
        let s = h.state.lock();
        assert_eq!(s.icon_name.as_deref(), Some("icon"));
        assert!(s.title.is_none());
    }

    #[test]
    fn osc_2_sets_title_only() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_TITLE, "win-title");
        let s = h.state.lock();
        assert_eq!(s.title.as_deref(), Some("win-title"));
        assert!(s.icon_name.is_none());
    }

    #[test]
    fn osc_4_sets_palette_and_marks_theme_dirty() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_COLOR_PALETTE, "5;rgb:11/22/33");
        assert_eq!(h.theme.lock().palette256[5], Some(Rgb(0x11, 0x22, 0x33)));
        assert!(h.cb.take_theme_dirty());
        // Second drain returns false (latch behavior).
        assert!(!h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_7_sets_cwd() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_WORKING_DIRECTORY, "file:///home/me");
        assert_eq!(h.state.lock().cwd.as_deref(), Some("file:///home/me"));
    }

    #[test]
    fn osc_8_is_logged_only() {
        let h = default_harness();
        h.cb.on_osc(OSC_HYPERLINK, "id=42;https://example.com");
        // No state mutation expected.
        let s = h.state.lock();
        assert!(s.title.is_none());
        assert!(s.icon_name.is_none());
    }

    #[test]
    fn osc_9_emits_notification() {
        let h = default_harness();
        h.cb.on_osc(OSC_NOTIFICATION, "Build done;all green");
        let calls = h.sink.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "Build done");
        assert_eq!(calls[0].1, "all green");
        assert_eq!(h.state.lock().pending_notifications.len(), 1);
    }

    #[test]
    fn osc_9_no_separator_uses_fallback_title() {
        let h = default_harness();
        // Title pre-populated by an earlier OSC 2.
        h.cb.on_osc(OSC_SET_TITLE, "the-title");
        h.cb.on_osc(OSC_NOTIFICATION, "body only");
        let calls = h.sink.calls();
        assert_eq!(calls[0].0, "the-title");
        assert_eq!(calls[0].1, "body only");
    }

    #[test]
    fn osc_10_sets_fg_and_marks_theme_dirty() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_FG, "rgb:11/22/33");
        assert_eq!(h.theme.lock().fg, Rgb(0x11, 0x22, 0x33));
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_11_sets_bg_and_marks_theme_dirty() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_BG, "#445566");
        assert_eq!(h.theme.lock().bg, Rgb(0x44, 0x55, 0x66));
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_12_sets_cursor_fg_and_marks_theme_dirty() {
        let h = default_harness();
        h.cb.on_osc(OSC_SET_CURSOR_FG, "rgb:aa/bb/cc");
        assert_eq!(h.theme.lock().cursor_fg, Rgb(0xaa, 0xbb, 0xcc));
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_22_updates_cursor_style() {
        let h = default_harness();
        h.cb.on_osc(OSC_CURSOR_STYLE, "underline");
        assert_eq!(h.theme.lock().cursor_style, CursorStyle::Underline);
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_52_write_default_allows_within_quota() {
        let h = default_harness();
        // "hi" -> base64 "aGk="
        h.cb.on_osc(OSC_CLIPBOARD, "c;aGk=");
        let s = h.state.lock();
        assert_eq!(s.pending_clipboard_writes.len(), 1);
        assert_eq!(s.pending_clipboard_writes[0].0, "c");
        assert_eq!(s.pending_clipboard_writes[0].1, "hi");
    }

    #[test]
    fn osc_52_query_default_allows_read() {
        let h = default_harness();
        h.cb.on_osc(OSC_CLIPBOARD, "p;?");
        let s = h.state.lock();
        assert_eq!(s.pending_clipboard_reads, vec!["p".to_string()]);
    }

    #[test]
    fn osc_52_query_denied_when_read_disabled() {
        let mut settings = Settings::default();
        settings.clipboard_read_osc52 = false;
        let h = harness(settings);
        h.cb.on_osc(OSC_CLIPBOARD, "c;?");
        assert!(h.state.lock().pending_clipboard_reads.is_empty());
    }

    #[test]
    fn osc_52_write_denied_when_over_quota() {
        let mut settings = Settings::default();
        // 3 bytes max → "hello" (5 bytes) must be rejected.
        settings.clipboard_max_size_osc52 = 3;
        let h = harness(settings);
        // "hello" -> base64 "aGVsbG8="
        h.cb.on_osc(OSC_CLIPBOARD, "c;aGVsbG8=");
        assert!(h.state.lock().pending_clipboard_writes.is_empty());
    }

    #[test]
    fn osc_52_clear_pushes_empty_write() {
        let h = default_harness();
        h.cb.on_osc(OSC_CLIPBOARD, "c;");
        let s = h.state.lock();
        assert_eq!(s.pending_clipboard_writes.len(), 1);
        assert_eq!(s.pending_clipboard_writes[0].1, "");
    }

    #[test]
    fn osc_104_resets_palette() {
        let h = default_harness();
        h.theme.lock().palette256[5] = Some(Rgb(1, 2, 3));
        h.cb.on_osc(OSC_RESET_COLOR_PALETTE, "");
        assert!(h.theme.lock().palette256.iter().all(|e| e.is_none()));
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_110_resets_fg() {
        let h = default_harness();
        h.theme.lock().fg = Rgb(1, 2, 3);
        h.cb.on_osc(OSC_RESET_FG, "");
        assert_eq!(h.theme.lock().fg, Rgb::WHITE);
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_111_resets_bg() {
        let h = default_harness();
        h.theme.lock().bg = Rgb(1, 2, 3);
        h.cb.on_osc(OSC_RESET_BG, "");
        assert_eq!(h.theme.lock().bg, Rgb::BLACK);
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_112_resets_cursor_fg() {
        let h = default_harness();
        h.theme.lock().cursor_fg = Rgb(1, 2, 3);
        h.cb.on_osc(OSC_RESET_CURSOR_FG, "");
        assert_eq!(h.theme.lock().cursor_fg, Theme::DEFAULT_CURSOR_FG);
        assert!(h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_133_a_records_prompt_start() {
        let h = default_harness();
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "A");
        let s = h.state.lock();
        assert_eq!(s.prompt_marks.len(), 1);
        assert_eq!(s.prompt_marks[0].kind, PromptMarkKind::PromptStart);
        assert!(s.prompt_marks[0].exit_code.is_none());
    }

    #[test]
    fn osc_133_b_c_d_records_each() {
        let h = default_harness();
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "B");
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "C");
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;0");
        let s = h.state.lock();
        assert_eq!(s.prompt_marks.len(), 3);
        assert_eq!(s.prompt_marks[0].kind, PromptMarkKind::CommandStart);
        assert_eq!(s.prompt_marks[1].kind, PromptMarkKind::CommandExec);
        assert_eq!(s.prompt_marks[2].kind, PromptMarkKind::CommandEnd);
        assert_eq!(s.prompt_marks[2].exit_code, Some(0));
    }

    #[test]
    fn osc_133_d_with_nonzero_exit_code() {
        let h = default_harness();
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;42");
        assert_eq!(h.state.lock().prompt_marks[0].exit_code, Some(42));
    }

    #[test]
    fn osc_100_emterm_extension_pushes_to_queue() {
        let h = default_harness();
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "markdown;hello");
        let s = h.state.lock();
        assert_eq!(s.osc_queue.len(), 1);
        assert_eq!(s.osc_queue[0].payload, "markdown;hello");
    }

    #[test]
    fn osc_101_iterm2_is_logged_only() {
        let h = default_harness();
        h.cb.on_osc(OSC_ITERM2, "File=name=foo:");
        let s = h.state.lock();
        assert!(s.title.is_none());
        assert!(s.osc_queue.is_empty());
    }

    #[test]
    fn osc_255_unknown_is_logged_only() {
        let h = default_harness();
        h.cb.on_osc(OSC_UNKNOWN, "something");
        let s = h.state.lock();
        assert!(s.title.is_none());
        assert!(s.osc_queue.is_empty());
    }

    // ── Rate limiter behavior ───────────────────────────────────────────

    #[test]
    fn rate_limiter_dedupes_identical_pair_within_window() {
        let h = default_harness();
        h.cb.on_osc(OSC_NOTIFICATION, "title;body");
        h.cb.on_osc(OSC_NOTIFICATION, "title;body");
        // Only the first call reaches the sink.
        assert_eq!(h.sink.calls().len(), 1);
    }

    #[test]
    fn rate_limiter_allows_after_window_elapsed() {
        let h = default_harness();
        h.cb.on_osc(OSC_NOTIFICATION, "title;body");
        // Advance the injected clock past the dedupe window.
        {
            let mut clk = h.clock.lock();
            *clk += Duration::from_secs(2);
        }
        h.cb.on_osc(OSC_NOTIFICATION, "title;body");
        assert_eq!(h.sink.calls().len(), 2);
    }

    #[test]
    fn rate_limiter_distinct_pairs_not_deduped() {
        let h = default_harness();
        h.cb.on_osc(OSC_NOTIFICATION, "A;1");
        h.cb.on_osc(OSC_NOTIFICATION, "A;2");
        h.cb.on_osc(OSC_NOTIFICATION, "B;1");
        assert_eq!(h.sink.calls().len(), 3);
    }

    // ── Existing-behavior regression coverage ──────────────────────────

    #[test]
    fn on_apc_buffers_payload_into_pending_apc() {
        let h = default_harness();
        h.cb.on_apc(b"Ga=q;");
        let st = h.state.lock();
        assert_eq!(st.pending_apc.len(), 1);
        assert_eq!(st.pending_apc[0], b"Ga=q;".to_vec());
        assert!(st.pending_dcs.is_empty());
    }

    #[test]
    fn on_dcs_buffers_payload_into_pending_dcs() {
        let h = default_harness();
        h.cb.on_dcs(b"0;0;0q");
        let st = h.state.lock();
        assert_eq!(st.pending_dcs.len(), 1);
        assert_eq!(st.pending_dcs[0], b"0;0;0q".to_vec());
        assert!(st.pending_apc.is_empty());
    }

    #[test]
    fn on_apc_appends_in_order_across_multiple_calls() {
        let h = default_harness();
        h.cb.on_apc(b"a");
        h.cb.on_apc(b"b");
        h.cb.on_apc(b"c");
        let st = h.state.lock();
        assert_eq!(
            st.pending_apc,
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
    }

    #[test]
    fn on_bell_increments_counter() {
        let h = default_harness();
        h.cb.on_bell();
        h.cb.on_bell();
        assert_eq!(h.state.lock().bell_count, 2);
    }

    // ── Parser micro-tests ──────────────────────────────────────────────

    #[test]
    fn parse_osc52_write() {
        assert_eq!(
            parse_osc52("c;aGk="),
            Some(Osc52Action::Write {
                target: "c".into(),
                payload: "aGk=".into(),
            })
        );
    }

    #[test]
    fn parse_osc52_query() {
        assert_eq!(
            parse_osc52("p;?"),
            Some(Osc52Action::Query { target: "p".into() })
        );
    }

    #[test]
    fn parse_osc52_clear() {
        assert_eq!(
            parse_osc52("c;"),
            Some(Osc52Action::Clear { target: "c".into() })
        );
    }

    #[test]
    fn parse_osc52_missing_separator_returns_none() {
        assert_eq!(parse_osc52("garbage"), None);
        assert_eq!(parse_osc52(""), None);
    }

    #[test]
    fn parse_osc9_with_separator() {
        let (t, b) = parse_osc9("Title;Body", None);
        assert_eq!(t, "Title");
        assert_eq!(b, "Body");
    }

    #[test]
    fn parse_osc9_no_separator_uses_fallback() {
        let (t, b) = parse_osc9("just body", Some("fallback"));
        assert_eq!(t, "fallback");
        assert_eq!(b, "just body");
    }

    #[test]
    fn parse_osc9_empty_title_uses_fallback() {
        let (t, b) = parse_osc9(";body", Some("fb"));
        assert_eq!(t, "fb");
        assert_eq!(b, ";body");
    }

    #[test]
    fn parse_osc133_unknown_returns_none() {
        assert_eq!(parse_osc133("Z").0, None);
    }
}
