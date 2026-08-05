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

    /// Intentionally a no-op (tmux-startup-query-response-leak task0001).
    ///
    /// `term_core` fires this callback AND writes the identical bytes into
    /// its own single-slot `response_buffer` for every synthesized DSR /
    /// DA / XTWINOPS / DECRPM reply (see
    /// `crates/term_core/src/csi_dispatch.rs`'s device-response arms,
    /// each of which calls both `write_response` and
    /// `fire_device_response_callback`). `Tab`'s three write-back sites
    /// (`process_outer_via_core`, `apply_active_pane_output`,
    /// `apply_queued_live_output`) already poll `TerminalCore::
    /// take_response()` after every parse and deliver the result via the
    /// mux-aware `write_device_response` — that is the sole intended PTY
    /// delivery route (see `take_response`'s doc). This callback used to
    /// ALSO queue the bytes here (`NativeCallbackState::device_responses`,
    /// drained once per pump in `Tab::process_combined` and written raw,
    /// bypassing mux routing) — a second, redundant delivery of the same
    /// reply that violated exactly-once delivery: in the plain-tab
    /// context the querying application (e.g. tmux) received the reply
    /// twice, and having already consumed the first copy for capability
    /// negotiation, forwarded the second as ordinary input, which echoed
    /// onto the screen. Left as a no-op rather than removed from the
    /// trait so the callback surface does not need to change for a
    /// native-only fix.
    fn on_device_response(&self, _data: &[u8]) {}

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
        assert_eq!(h.theme.lock().fg, crate::render::theme::DEFAULT_TERMINAL_FG);
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
    fn osc_112_resets_cursor_fg_to_active_scheme_color() {
        // task0003 AC-3: OSC 112 restores the ACTIVE SCHEME's cursor
        // color, not a hard-coded preset. `scheme_cursor_fg` stands in
        // for a non-default scheme's cursor color (as `apply_color_scheme`
        // would seed it); `cursor_fg` stands in for an OSC 12 override.
        let h = default_harness();
        {
            let mut theme = h.theme.lock();
            theme.scheme_cursor_fg = Rgb(9, 8, 7);
            theme.cursor_fg = Rgb(1, 2, 3);
        }
        h.cb.on_osc(OSC_RESET_CURSOR_FG, "");
        assert_eq!(h.theme.lock().cursor_fg, Rgb(9, 8, 7));
        assert_ne!(h.theme.lock().cursor_fg, Theme::DEFAULT_CURSOR_FG);
        assert!(h.cb.take_theme_dirty());
    }

    // ── task0004 AC-5: on_reset restores an active OSC 12 override ────

    #[test]
    fn on_reset_restores_active_cursor_override_to_active_scheme_color() {
        let h = default_harness();
        h.theme.lock().scheme_cursor_fg = Rgb(9, 8, 7);
        h.cb.on_osc(OSC_SET_CURSOR_FG, "rgb:01/02/03");
        assert!(h.cb.take_theme_dirty(), "OSC 12 itself marked dirty");
        assert!(h.theme.lock().cursor_fg_override_active);

        h.cb.on_reset();

        let theme = h.theme.lock();
        assert_eq!(theme.cursor_fg, Rgb(9, 8, 7));
        assert!(!theme.cursor_fg_override_active);
        drop(theme);
        assert!(
            h.cb.take_theme_dirty(),
            "on_reset marks dirty when it changed cursor_fg"
        );
    }

    #[test]
    fn on_reset_is_a_noop_without_an_active_override() {
        let h = default_harness();
        h.cb.on_reset();
        assert!(!h.cb.take_theme_dirty());
    }

    #[test]
    fn osc_133_callback_is_a_noop_for_native_state() {
        // OSC 133 marks are now captured in `term_core`
        // (`push_pending_prompt_mark`) and drained by the tab via
        // `take_prompt_marks`. The callback retains its dispatch arm only to
        // keep the wasm/WebView `on_osc(133, …)` contract; it must not mutate
        // any `NativeCallbackState`.
        let h = default_harness();
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "A");
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;42");
        let s = h.state.lock();
        assert!(s.title.is_none());
        assert!(s.osc_queue.is_empty());
        assert!(s.pending_notifications.is_empty());
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
    fn osc_100_strips_emterm_namespace_token_from_real_wire_form() {
        // The real CLI wire form is `OSC 777;emterm;<kind>;…` and term_core
        // delivers the `emterm;` prefix intact. The extension arm must strip
        // it once so the viewer sees the post-namespace `<kind>;<verb>;…`.
        let h = default_harness();
        h.cb.on_osc(
            OSC_EMTERM_EXTENSION,
            "emterm;markdown;begin;id=x;format=gfm",
        );
        let s = h.state.lock();
        assert_eq!(s.osc_queue.len(), 1);
        assert_eq!(s.osc_queue[0].payload, "markdown;begin;id=x;format=gfm");
    }

    // ── task0005: OSC 777 agent-status routing ────────────────────────

    #[test]
    fn osc_100_agent_status_set_routes_to_pending_agent_status_not_osc_queue() {
        let h = default_harness();
        h.cb.on_osc(
            OSC_EMTERM_EXTENSION,
            "emterm;agent-status;v=1;state=working;name=claude",
        );
        let s = h.state.lock();
        assert_eq!(
            s.pending_agent_status,
            vec![crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Working,
                name: Some("claude".to_string()),
            }]
        );
        assert!(s.osc_queue.is_empty());
    }

    #[test]
    fn osc_100_agent_status_clear_routes_to_pending_agent_status() {
        let h = default_harness();
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;clear");
        let s = h.state.lock();
        assert_eq!(
            s.pending_agent_status,
            vec![crate::agent_status::AgentStatusEvent::Clear]
        );
    }

    #[test]
    fn osc_100_agent_status_invalid_payload_falls_through_to_osc_queue() {
        // A malformed agent-status payload (missing `state`) is rejected by
        // `crate::agent_status::parse`, so the extension arm falls through
        // to the legacy viewer queue exactly as any other unrecognized OSC
        // 777 payload would — it is not silently dropped.
        let h = default_harness();
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;v=1");
        let s = h.state.lock();
        assert!(s.pending_agent_status.is_empty());
        assert_eq!(s.osc_queue.len(), 1);
    }

    // ── agent-exit-after-icon (task0002): pending_latch_feed ordering ──

    #[test]
    fn osc_100_agent_status_set_pushes_set_marker_to_latch_feed() {
        let h = default_harness();
        h.cb.on_osc(
            OSC_EMTERM_EXTENSION,
            "emterm;agent-status;v=1;state=working",
        );
        assert_eq!(h.state.lock().pending_latch_feed, vec![LatchFeedEvent::Set]);
    }

    #[test]
    fn osc_100_agent_status_clear_pushes_clear_marker_to_latch_feed() {
        let h = default_harness();
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;clear");
        assert_eq!(
            h.state.lock().pending_latch_feed,
            vec![LatchFeedEvent::Clear]
        );
    }

    #[test]
    fn osc_100_invalid_agent_status_payload_does_not_push_to_latch_feed() {
        let h = default_harness();
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;v=1");
        assert!(h.state.lock().pending_latch_feed.is_empty());
    }

    #[test]
    fn osc_133_a_and_d_push_prompt_mark_candidates_to_latch_feed() {
        let h = default_harness();
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;0");
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "A");
        assert_eq!(
            h.state.lock().pending_latch_feed,
            vec![
                LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::CommandEnd),
                LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::PromptStart),
            ]
        );
    }

    #[test]
    fn osc_133_unrecognized_kind_does_not_push_to_latch_feed() {
        let h = default_harness();
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "Z");
        assert!(h.state.lock().pending_latch_feed.is_empty());
    }

    #[test]
    fn set_and_live_133_marks_preserve_true_relative_order_in_latch_feed() {
        // FR4: OSC 777 Set/Clear and OSC 133 D/A candidates share ONE
        // ordered log (`pending_latch_feed`), reflecting the true
        // synchronous `on_osc` call order — not two independently
        // populated queues that a caller would have to re-interleave.
        let h = default_harness();
        h.cb.on_osc(
            OSC_EMTERM_EXTENSION,
            "emterm;agent-status;v=1;state=working",
        );
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "D;0");
        h.cb.on_osc(OSC_SEMANTIC_PROMPT, "A");
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "emterm;agent-status;clear");
        assert_eq!(
            h.state.lock().pending_latch_feed,
            vec![
                LatchFeedEvent::Set,
                LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::CommandEnd),
                LatchFeedEvent::PromptMark(crate::prompts::PromptMarkKind::PromptStart),
                LatchFeedEvent::Clear,
            ]
        );
    }

    // ── Phase D: OSC 777 statusbar routing ────────────────────────────

    #[test]
    fn osc_100_with_statusbar_prefix_routes_to_dispatcher() {
        let mut h = default_harness();
        let dispatcher = Arc::new(StatusBarOscDispatcher::new());
        h.cb.set_statusbar_dispatcher(dispatcher.clone());
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "statusbar;set;left;hi");
        // Dispatched: state updated.
        assert_eq!(dispatcher.snapshot().left, "hi");
        // NOT pushed to osc_queue.
        assert!(h.state.lock().osc_queue.is_empty());
    }

    #[test]
    fn osc_100_without_statusbar_prefix_still_pushes_to_queue_when_dispatcher_present() {
        let mut h = default_harness();
        let dispatcher = Arc::new(StatusBarOscDispatcher::new());
        h.cb.set_statusbar_dispatcher(dispatcher.clone());
        h.cb.on_osc(OSC_EMTERM_EXTENSION, "markdown;hello");
        assert_eq!(h.state.lock().osc_queue.len(), 1);
        assert_eq!(h.state.lock().osc_queue[0].payload, "markdown;hello");
        // Dispatcher untouched.
        assert!(dispatcher.snapshot().left.is_empty());
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

    // ── TS-13: pre-mux OSC 9999 emterm-mux Welcome reaches the mux APC sink ─
    #[test]
    fn osc_9999_emterm_mux_inband_routed_to_pending_apc() {
        // A pre-mux Windows-ConPTY Welcome arrives as an OSC 9999
        // `emterm-mux;<base64>` frame. `term_core` no longer special-cases it
        // (NFR5): it now reaches the app via `on_osc(OSC_MUX_INBAND, …)`. The
        // app layer recognizes the `emterm-mux;` prefix and routes the full
        // frame string into the same `pending_apc` sink `on_apc` feeds, so
        // `partition_apc_for_mux` can establish mux.
        let h = default_harness();
        let frame = "emterm-mux;V2VsY29tZQ==";
        h.cb.on_osc(OSC_MUX_INBAND, frame);
        let s = h.state.lock();
        assert_eq!(s.pending_apc.len(), 1, "frame buffered into pending_apc");
        assert_eq!(s.pending_apc[0], frame.as_bytes().to_vec());
        // It must NOT leak into the OSC viewer queue or set a title.
        assert!(s.osc_queue.is_empty());
        assert!(s.title.is_none());
    }

    #[test]
    fn osc_9999_non_mux_prefix_is_dropped() {
        // OSC 9999 whose data lacks the `emterm-mux;` prefix is not a mux
        // frame and must be dropped (parity with the old term_core guard,
        // now enforced in the app layer).
        let h = default_harness();
        h.cb.on_osc(OSC_MUX_INBAND, "something-else;data");
        let s = h.state.lock();
        assert!(s.pending_apc.is_empty());
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
}
