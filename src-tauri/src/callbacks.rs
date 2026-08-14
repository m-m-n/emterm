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
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use term_core::callbacks::TerminalCallbacks;

use crate::render::theme::Theme;
use crate::settings::Settings;
use crate::status_bar::osc_dispatcher::{StatusBarOscDispatcher, try_dispatch_statusbar};
use crate::status_bar::providers::CwdProvider;

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
/// OSC 102 (wire 9999): mux inband frame (Windows ConPTY fallback, pre-mux).
///
/// `term_core` embeds no mux number (NFR5). The host injects the
/// `MUX_OSC_PARAM → OSC_MUX_INBAND` (9999 → 102) mapping via
/// `TerminalCore::register_osc_app_param` (see `tabs.rs` at tab spawn);
/// `term_core` then delivers the otherwise-unknown OSC 9999 here as
/// `on_osc(102, …)`. The `emterm-mux;` prefix is recognized in this app
/// layer, not in the core.
pub const OSC_MUX_INBAND: u8 = 102;
/// OSC 255: unknown / unmapped action.
pub const OSC_UNKNOWN: u8 = 255;

// ── Log message constants ───────────────────────────────────────────────

/// Emitted when an OSC 52 payload is dropped because of the policy gate
/// (read disabled or size over `clipboard_max_size_osc52`).
pub const LOG_OSC52_DENIED: &str = "LOG_OSC52_DENIED";
/// Emitted when an OSC 9 notification is suppressed by the rate limiter.
pub const LOG_NOTIFY_RATE_LIMIT: &str = "LOG_NOTIFY_RATE_LIMIT";

// ── task0001: notification log redaction ──────────────────────────────
//
// (osc9-notify-log-redaction IMPLEMENTATION.md "Redaction renderer" /
// "Diagnostic ID" contracts, "Redacted record format"). Module-private:
// the only callers are the two notification log sites in this module
// (`handle_notify`'s rate-limit branch and `NotifyRustSink::send`'s
// dispatch-success branch) — never exported from the crate (D4/D5). Not
// Unix-gated (D4): unlike the escape helpers below, the dispatch-success
// site exists on every supported platform.

/// Process-global keyed hash state backing [`notification_diagnostic_id`]
/// (D3): a single randomly-seeded `RandomState`, created lazily on first
/// use and then shared immutably by every thread for the rest of the
/// process run. Never per-call, never per-thread, and never logged —
/// nothing in this module reads or exposes the key itself.
fn notify_diagnostic_key() -> &'static RandomState {
    static KEY: OnceLock<RandomState> = OnceLock::new();
    KEY.get_or_init(RandomState::new)
}

/// Derive the diagnostic ID for a `(title, body)` pair (the "Diagnostic
/// ID" contract): 16 lowercase hexadecimal characters, fixed width.
/// Equal pairs hash equal within one process run regardless of the
/// calling thread (backed by the shared per-run key above); different
/// pairs differ with negligible collision probability. `str`'s `Hash`
/// impl appends a length-implying terminator byte after each field, so
/// hashing `title` then `body` in sequence cannot collide with a
/// differently-split concatenation of the same bytes.
fn notification_diagnostic_id(title: &str, body: &str) -> String {
    let mut hasher = notify_diagnostic_key().build_hasher();
    title.hash(&mut hasher);
    body.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The redaction renderer (the "Redaction renderer" contract): render the
/// three allow-listed metadata fields for a `(title, body)` pair as
/// defined by "Redacted record format" — title length, body length (both
/// UTF-8 byte counts, D1) and the diagnostic ID, in that fixed order, as
/// space-separated `name=value` pairs whose names carry their unit. Pure,
/// total (every input pair, including empty strings, yields a rendering —
/// no failure branch) and performs no I/O; contains no character
/// sequence copied from either input (D5: no fourth field, ever).
fn redact_notification(title: &str, body: &str) -> String {
    format!(
        "title_len_bytes={} body_len_bytes={} diag_id={}",
        title.len(),
        body.len(),
        notification_diagnostic_id(title, body)
    )
}

// ── Public types ────────────────────────────────────────────────────────

/// Emterm OSC viewer-spawn request decoded from an OSC 777 payload. Phase 6
/// passes these to the (future) Wry viewer spawner. For now native-poc only
/// records them so we can verify the dispatch path end-to-end.
#[derive(Debug, Clone)]
pub struct EmtermOscRequest {
    /// Raw payload — drained by [`crate::viewer::ViewerSpawner`] and routed
    /// by viewer kind.
    pub payload: String,
}

/// One entry in [`NativeCallbackState::pending_latch_feed`]
/// (agent-exit-after-icon SPEC FR4): captures OSC 777 agent-status
/// `Set`/`Clear` reports and OSC 133 `A`/`D`/`B`/`C` mark CANDIDATES in
/// true synchronous `on_osc` call order — a single ordered log, so the
/// plain-tab inferred-clear latch (task0002 deviation; see
/// `crate::agent_status_exit_latch`) never has to reconstruct relative
/// order from two independently-scheduled queues.
///
/// "Candidate" because `on_osc(OSC_SEMANTIC_PROMPT, …)` fires
/// unconditionally (see that match arm's doc), unlike
/// `TerminalCore::push_pending_prompt_mark`, which suppresses marks
/// observed on the alternate screen. `Tab::process_outer_via_core`
/// cross-references this log against `take_prompt_marks()`'s
/// alt-screen-filtered output (drained in the same pump) via
/// `agent_status_model::reconcile_latch_feed` to keep only genuinely live
/// candidates before feeding `AgentStatusModel`'s per-tab latch (FR5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatchFeedEvent {
    /// An OSC 777 agent-status `Set` report (any state/name).
    Set,
    /// An OSC 777 agent-status `Clear` report.
    Clear,
    /// An OSC 133 mark CANDIDATE, not yet confirmed live.
    PromptMark(crate::prompts::PromptMarkKind),
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
        // osc9-notify-log-redaction task0001 (IMPLEMENTATION.md D2): this
        // sink is the sole egress point for every notification producer
        // (OSC 9, tab activity, agent status, link-hover), so it is also
        // the single place both log records below can be redacted from.
        // Render the redacted metadata from the values as RECEIVED on
        // entry, before the `#[cfg(unix)]` escape gate below shadows
        // `title`/`body` with their escaped forms — otherwise this
        // dispatch-success record and a later rate-limit suppression
        // record for a repeat of the same notification would carry two
        // unrelated diagnostic IDs, breaking the within-run correlation
        // FR3 exists for.
        let redacted = redact_notification(title, body);

        // task0001 (IMPLEMENTATION.md D1 案(b)): this is the sole D-Bus
        // egress point for every notification producer (OSC 9, tab
        // activity, agent status, link-hover) — escape here, once, gated
        // by a fresh per-send capability query (D2: not cached; the same
        // decision drives BOTH title and body, see `escape_for_send`).
        // notification-markup-fail-closed SPEC: the gate is fail-closed —
        // a failed capability query escapes both fields, same as a
        // confirmed `body-markup` capability; only an explicit,
        // successful "body-markup absent" report passes text through
        // unescaped.
        // Windows notify-rust has no `get_capabilities()` export
        // (XDG-only surface), so the whole gate is `#[cfg(unix)]`; the
        // `.show()` call below is unchanged from before this task (FR5).
        #[cfg(unix)]
        let (title_owned, body_owned) =
            escape_for_send(title, body, &notify_rust::get_capabilities());
        #[cfg(unix)]
        let title = title_owned.as_str();
        #[cfg(unix)]
        let body = body_owned.as_str();

        match notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .show()
        {
            // osc9-notify-log-redaction task0001 (FR5): the literal
            // prefix and colon-space separator are unchanged; only the
            // interpolated content changes, from raw title text to the
            // allow-listed redacted rendering.
            Ok(_) => log::debug!("notify-rust dispatched: {redacted}"),
            // task0001 (FR7): the dispatch-error record is unchanged —
            // the notify-rust error value carries no notification text.
            Err(e) => log::warn!("notify-rust failed: {e}"),
        }
    }
}

/// (task0001, notification-markup-fail-closed SPEC) Apply the per-send
/// escape decision to both `title` (summary) and `body` under a SINGLE
/// evaluation of `capabilities` (D2: the caller queries
/// `get_capabilities()` exactly once per `send` and passes the result
/// here, so the two fields can never diverge within one send). Fail-closed
/// (FR1/FR3): pass-through applies ONLY when the query succeeded and the
/// returned list explicitly omits `body-markup`; every other outcome — a
/// query failure, or a successful list that contains `body-markup` —
/// escapes both fields via [`escape_body_markup`].
/// Private to this module — the only caller is `NotifyRustSink::send`;
/// pulled out of `send` so it is unit-testable without a real D-Bus
/// connection.
#[cfg(unix)]
fn escape_for_send<E>(
    title: &str,
    body: &str,
    capabilities: &Result<Vec<String>, E>,
) -> (String, String) {
    if body_markup_absence_confirmed(capabilities) {
        (title.to_string(), body.to_string())
    } else {
        (escape_body_markup(title), escape_body_markup(body))
    }
}

/// (previous task — notification-body-markup-escape SPEC Component 1,
/// TS1/TS2/TS3) Escape notify-rust body-markup meta characters. `&` is
/// replaced first, then `<` and `>`, so the entity references this
/// function produces are never re-escaped by the `&` pass (AC-2 / FR1 of
/// that SPEC). Pure: no I/O. Unchanged by the notification-markup-fail-
/// closed SPEC (NFR3) — that SPEC only inverts the capability decision
/// that gates which of `escape_for_send`'s two branches calls this
/// function. Unix-only because the sole caller (`NotifyRustSink::send`)
/// only reaches it behind the `#[cfg(unix)]` capability gate above.
#[cfg(unix)]
fn escape_body_markup(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// (task0001 Component 2, TS1/TS2, notification-markup-fail-closed SPEC)
/// Interpret a `get_capabilities()` result under fail-closed semantics:
/// the EXPLICIT ABSENCE of `body-markup` support, confirmed only when the
/// fetch succeeded AND the returned list omits it. Any other outcome — a
/// fetch failure, or a successful list that contains `body-markup` —
/// returns `false`, and callers escape on `false` (escaping is the
/// default; pass-through is the narrow exception, FR1/FR3). Generic over
/// the error type so it composes directly with
/// `notify_rust::Result<Vec<String>>` (`E = notify_rust::error::Error`)
/// without pulling that type into tests.
#[cfg(unix)]
fn body_markup_absence_confirmed<E>(capabilities: &Result<Vec<String>, E>) -> bool {
    matches!(capabilities, Ok(caps) if !caps.iter().any(|c| c == "body-markup"))
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
    /// BEL counter. `Tab::pump` drains this into its per-frame bell
    /// latch; `App::pump_all` then dispatches `settings.bell_action`.
    pub bell_count: u32,
    /// Pending OSC 9 notifications, drained by `Tab::pump` and dispatched
    /// to the `NotificationSink`. Buffering here keeps the callback fast
    /// (no D-Bus round-trip inside `process_pty_data`).
    pub pending_notifications: Vec<(String, String)>,
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
    /// Parsed plain-tab `agent-status` OSC 777 events (SPEC FR5, task0005).
    /// `Tab::pump` drains this into its own per-pump latch
    /// (`pending_agent_status_events`); `App::pump_all` then applies each
    /// event to `App::agent_status` keyed by the tab's `stable_id`.
    pub pending_agent_status: Vec<crate::agent_status::AgentStatusEvent>,
    /// Ordered agent-status-latch feed (agent-exit-after-icon SPEC FR4;
    /// task0002 deviation). See [`LatchFeedEvent`]'s doc for the full
    /// contract. `Tab::process_outer_via_core` drains and reconciles this
    /// every pump.
    pub pending_latch_feed: Vec<LatchFeedEvent>,
}

/// `TerminalCallbacks` implementation for native consumers.
pub struct NativeCallbacks {
    state: Arc<Mutex<NativeCallbackState>>,
    theme: Arc<Mutex<Theme>>,
    settings: Arc<Settings>,
    sink: Arc<dyn NotificationSink>,
    rate_limiter: Arc<NotificationRateLimiter>,
    /// Optional OSC `777;statusbar` dispatcher. When present, OSC 777
    /// payloads addressed to `statusbar;…` are routed here before
    /// falling through to the legacy `osc_queue`. Phase 4-D + Phase
    /// D wire this; tests that don't exercise the status bar can
    /// leave it `None`.
    statusbar_dispatcher: Option<Arc<StatusBarOscDispatcher>>,
    /// Optional status-bar CwdProvider. When present, OSC 7 updates
    /// forward to `CwdProvider::set_cwd` which fires its injected
    /// wake handle so the egui frame schedules a redraw without
    /// polling (provider-ownership refresh-redraw).
    cwd_provider: Option<Arc<CwdProvider>>,
}

impl NativeCallbacks {
    /// Construct callbacks with a caller-supplied notification `sink`.
    ///
    /// `App` builds one production [`NotifyRustSink`] and clones the same
    /// `Arc` into every tab, so the OSC 9 path and the link-handling path
    /// (`WindowHost::open_file_in_editor`) share a single sink instance.
    pub fn new(
        state: Arc<Mutex<NativeCallbackState>>,
        theme: Arc<Mutex<Theme>>,
        settings: Arc<Settings>,
        sink: Arc<dyn NotificationSink>,
    ) -> Self {
        Self::with_sink(
            state,
            theme,
            settings,
            sink,
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
            statusbar_dispatcher: None,
            cwd_provider: None,
        }
    }

    /// Install a [`StatusBarOscDispatcher`] so that OSC 777 payloads
    /// addressed to `statusbar;…` are routed to the status-bar runtime
    /// instead of being pushed into `osc_queue`.
    pub fn set_statusbar_dispatcher(&mut self, dispatcher: Arc<StatusBarOscDispatcher>) {
        self.statusbar_dispatcher = Some(dispatcher);
    }

    /// Install the runtime's [`CwdProvider`] so OSC 7 updates fire
    /// the provider's wake handle (no polling, event-driven redraw).
    pub fn set_cwd_provider(&mut self, provider: Arc<CwdProvider>) {
        self.cwd_provider = Some(provider);
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
        // Forward to the status-bar CwdProvider so the provider can
        // bump its version counter and invoke its injected wake
        // handle (provider-ownership refresh-redraw). Without this
        // hook the status bar would only redraw on the next
        // unrelated frame trigger.
        if let Some(p) = &self.cwd_provider {
            p.set_cwd(Some(data));
        }
    }

    fn handle_theme(&self, action_type: u8, data: &str) {
        let changed = self.theme.lock().apply_osc(action_type, data);
        if changed {
            self.mark_theme_dirty();
        }
    }

    /// cursor-settings-fix FR4: a full terminal reset (RIS) just ran.
    /// Restore an active OSC 12 cursor-color override back to the scheme
    /// color, mirroring OSC 112 — `term_core` already cleared its own
    /// shape/blink overrides unconditionally inside `reset()` before firing
    /// this callback.
    fn handle_reset(&self) {
        let changed = self.theme.lock().restore_cursor_fg_on_full_reset();
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
            // osc9-notify-log-redaction task0001 (FR1/FR4): the marker
            // constant and its colon-space separator are unchanged; only
            // the interpolated content changes, from raw title/body text
            // to the allow-listed redacted rendering.
            log::warn!(
                "{LOG_NOTIFY_RATE_LIMIT}: {}",
                redact_notification(&title, &body)
            );
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
            OSC_SEMANTIC_PROMPT => {
                // OSC 133 marks are captured inside `term_core` (see
                // `TerminalCore::push_pending_prompt_mark`), which records
                // each mark with the absolute row it was emitted on. The
                // native consumer drains them via `take_prompt_marks` under
                // the core lock; this callback fires unconditionally (even
                // on the alternate screen, unlike `push_pending_prompt_mark`)
                // only to keep the wasm/WebView path's `on_osc(133, …)`
                // contract intact.
                //
                // agent-exit-after-icon (task0002 deviation): ALSO record a
                // mark CANDIDATE for the plain-tab inferred-clear latch feed
                // (FR4) — see `LatchFeedEvent`'s doc for why this is a
                // "candidate" (not yet alt-screen-confirmed) and how the
                // relative order versus OSC 777 Set/Clear below is
                // preserved by construction (both push into the SAME
                // `pending_latch_feed` from this single synchronous
                // callback).
                if let Some(kind) = data
                    .as_bytes()
                    .first()
                    .copied()
                    .and_then(crate::prompts::PromptMarkKind::from_byte)
                {
                    self.state
                        .lock()
                        .pending_latch_feed
                        .push(LatchFeedEvent::PromptMark(kind));
                }
                log::debug!("OSC 133 mark seen: {data}");
            }
            OSC_EMTERM_EXTENSION => {
                // Phase 4-C (APC redesign): mux no longer rides on OSC 777.
                // Control messages now flow via APC `emterm-mux;<base64>` in
                // the PTY stream (see `crate::mux::apc`). OSC 777 retains
                // its legacy role as the emterm-extension viewer trigger.
                //
                // Phase D (status-bar native port): payloads starting with
                // `statusbar;` are routed to the dispatcher first; only
                // unconsumed payloads fall through to the legacy queue.
                //
                // `term_core` delivers the OSC 777 payload with the leading
                // `emterm;` namespace token still attached (the real wire
                // form is `OSC 777;emterm;<kind>;…`, e.g.
                // `emterm;markdown;begin;…`). Strip it once here so both the
                // statusbar dispatcher and the viewer queue see the
                // post-namespace payload (`<kind>;<verb>;…`), matching the
                // WebView `src/markdown/session.ts` contract. `strip_prefix`
                // is a no-op for payloads that were already pre-stripped.
                let payload = data.strip_prefix("emterm;").unwrap_or(data);
                // Agent-status reports (SPEC FR1/FR5, task0005): recognized
                // before the statusbar dispatcher and the legacy viewer
                // queue since `agent-status;` shares the OSC 777 `emterm`
                // namespace but is consumed by a different model
                // (`agent_status_model`), not the markdown/JSON/YAML
                // viewer pipeline. `crate::agent_status::parse` expects the
                // full `emterm;agent-status;…` string (not yet stripped of
                // the namespace token), matching `data` here — it returns
                // `None` for any other OSC 777 kind, so this is a no-op
                // fall-through for everything else.
                if let Some(event) = crate::agent_status::parse(data) {
                    // agent-exit-after-icon (task0002 deviation, FR4): push
                    // the Set/Clear latch-feed marker into the SAME ordered
                    // log OSC 133 candidates use, under the SAME lock
                    // acquisition as the existing `pending_agent_status`
                    // push, so relative order between the two is exactly
                    // `on_osc`'s true synchronous call order.
                    let latch_marker = match &event {
                        crate::agent_status::AgentStatusEvent::Set { .. } => LatchFeedEvent::Set,
                        crate::agent_status::AgentStatusEvent::Clear => LatchFeedEvent::Clear,
                    };
                    let mut s = self.state.lock();
                    s.pending_latch_feed.push(latch_marker);
                    s.pending_agent_status.push(event);
                    return;
                }
                if let Some(dispatcher) = self.statusbar_dispatcher.as_ref() {
                    if try_dispatch_statusbar(dispatcher, payload) {
                        return;
                    }
                }
                self.state.lock().osc_queue.push(EmtermOscRequest {
                    payload: payload.to_string(),
                });
            }
            OSC_ITERM2 => {
                // OQ7: log only — no inline-image subset is implemented.
                log::warn!("OSC 1337 (iTerm2) ignored: {} bytes", data.len());
            }
            OSC_MUX_INBAND => {
                // OSC 9999 emterm-mux inband frame (Windows ConPTY fallback,
                // pre-mux). term_core no longer knows the mux protocol; recognize
                // the emterm-mux; prefix here and route into the same pending_apc
                // sink on_apc feeds so partition_apc_for_mux establishes mux.
                if data.starts_with(mux_ipc::protocol::APC_PREFIX) {
                    self.state.lock().pending_apc.push(data.as_bytes().to_vec());
                }
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

    fn on_reset(&self) {
        self.handle_reset();
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

#[cfg(test)]
mod tests;
