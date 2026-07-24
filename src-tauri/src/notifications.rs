//! Tab activity tracking + desktop notification policy.
//!
//! Ports the WebView build's `src/tab-bar/tab-activity-tracker.ts`
//! (per-tab activity state + 1 s output-mark throttle) and
//! `src/notification/notification-manager.ts` (window-focus gate +
//! per-tab 5 s notification throttle + tab-title sanitizing) onto the
//! native tab loop:
//!
//! - `App::pump_all` detects output / BEL / process-exit on **inactive**
//!   tabs and calls [`TabActivityState::mark`]; the active tab never
//!   accumulates activity (same as the WebView tracker's active-tab
//!   early-return).
//! - A registered mark lights the tab bar's activity dot
//!   (`settings.tab_activity_indicator` gates the *rendering*, not the
//!   state — mirroring `main.ts` where `showActivityDot` is the gated
//!   call) and, when the window is unfocused and
//!   `settings.notification_enabled` holds, dispatches one desktop
//!   notification through the shared
//!   [`crate::callbacks::NotificationSink`].
//! - Notification bodies are localized per the resolved
//!   [`crate::i18n::Locale`] (the `language` setting); the strings
//!   match the WebView locales' `settings.notification.body.*`.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use crate::i18n::Locale;
use crate::settings::Settings;

// Re-exported so callers building [`AgentTransition`] values need only
// `crate::notifications` — the wire state enum is owned by `mux_ipc`
// (shared with the daemon/GUI protocol and the core `agent_status` module
// per IMPLEMENTATION.md's shared-component contract).
pub use mux_ipc::protocol::AgentState;

/// Activity types, mirroring the WebView `ActivityType` union
/// (`src/tab-bar/types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    ProcessExit,
    Output,
    Bell,
}

/// Output activity marking throttle (max 1/sec per tab) — WebView
/// `TabActivityTracker.OUTPUT_THROTTLE_MS`.
pub const OUTPUT_THROTTLE: Duration = Duration::from_millis(1000);

/// Desktop notification throttle (per tab) — WebView
/// `NotificationManager.NOTIFICATION_THROTTLE_MS`. `ProcessExit`
/// bypasses the check but still re-arms the window.
pub const NOTIFICATION_THROTTLE: Duration = Duration::from_millis(5000);

/// Notification summary line — matches the WebView build's
/// `sendNotification({ title: "eMterm", ... })`.
pub const NOTIFICATION_TITLE: &str = "eMterm";

/// Per-tab activity / throttle state. Lives on [`crate::tabs::Tab`] so
/// it drops together with the tab — the WebView build performs the same
/// cleanup in `cleanupTab` on `tab:closed`.
#[derive(Debug, Default)]
pub struct TabActivityState {
    /// Whether the tab bar shows the unread-activity dot for this tab.
    /// Cleared every frame on the active tab (WebView clears on
    /// `tab:activated`; per-frame clearing covers every switch path —
    /// click, keybind, reorder, tab reap — without per-path hooks).
    pub has_activity: bool,
    /// Last accepted `Output` mark (1 s throttle window).
    last_output_mark: Option<Instant>,
    /// Last dispatched desktop notification (5 s throttle window).
    last_notification: Option<Instant>,
}

impl TabActivityState {
    /// Register one activity event. Returns `true` when the event takes
    /// effect (dot lights up, notification dispatch follows), `false`
    /// when the 1 s output throttle swallowed it. The per-kind settings
    /// gate lives in [`kind_enabled`]; the active-tab skip lives at the
    /// call site (`App::pump_all`).
    pub fn mark(&mut self, kind: ActivityKind, now: Instant) -> bool {
        if kind == ActivityKind::Output {
            if let Some(prev) = self.last_output_mark {
                if now.duration_since(prev) < OUTPUT_THROTTLE {
                    return false;
                }
            }
            self.last_output_mark = Some(now);
        }
        self.has_activity = true;
        true
    }

    /// Per-tab notification throttle (WebView `NotificationManager.notify`
    /// lines 63-80): non-`ProcessExit` kinds are suppressed inside the
    /// 5 s window; every dispatched notification (including `ProcessExit`)
    /// re-arms it. The `notification_enabled` / window-focus gates live
    /// at the call site so this state stays a pure timer.
    pub fn should_notify(&mut self, kind: ActivityKind, now: Instant) -> bool {
        if kind != ActivityKind::ProcessExit {
            if let Some(prev) = self.last_notification {
                if now.duration_since(prev) < NOTIFICATION_THROTTLE {
                    return false;
                }
            }
        }
        self.last_notification = Some(now);
        true
    }

    /// Clear the dot (active-tab per-frame reset / tab activation).
    pub fn clear(&mut self) {
        self.has_activity = false;
    }
}

/// Per-kind settings gate — WebView `TabActivityTracker.markActivity`
/// lines 67-72. Gating here (before `mark`) means a disabled kind
/// produces neither a dot nor a notification, same as the WebView build.
pub fn kind_enabled(settings: &Settings, kind: ActivityKind) -> bool {
    match kind {
        ActivityKind::ProcessExit => settings.notify_on_process_exit,
        ActivityKind::Output => settings.notify_on_output,
        ActivityKind::Bell => settings.notify_on_bell,
    }
}

thread_local! {
    /// CSI escape-sequence stripper — WebView `sendDesktopNotification`
    /// first replace: `/\x1b\[[0-9;]*[a-zA-Z]/g`.
    static CSI_RE: regex::Regex =
        regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").expect("CSI_RE compiles");
}

/// Raw-input cap applied before the regex pass in [`sanitize_title`].
/// Tab titles come from untrusted OSC 0/2 payloads (term_core caps the
/// OSC buffer at 16 MiB), but the notification body only ever shows 100
/// chars — 4096 chars of raw input leaves ample headroom for CSI noise
/// around them while bounding the scan/allocation cost.
const SANITIZE_INPUT_CAP: usize = 4096;

/// Sanitize a tab title for notification display: strip ANSI CSI
/// sequences, drop remaining C0/C1 control characters, truncate to 100
/// characters. Mirrors `NotificationManager.sendDesktopNotification`.
pub fn sanitize_title(title: &str) -> String {
    // Bound the work before the regex pass so a pathological multi-MiB
    // title cannot force a full-length scan + allocation.
    let bounded: String = title.chars().take(SANITIZE_INPUT_CAP).collect();
    let stripped = CSI_RE.with(|re| re.replace_all(&bounded, "").into_owned());
    stripped
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            // [\x00-\x1f\x7f-\x9f] — C0 controls, DEL, C1 controls.
            !(cp <= 0x1f || (0x7f..=0x9f).contains(&cp))
        })
        .take(100)
        .collect()
}

/// Notification body — WebView `formatNotificationBody`. The per-kind
/// suffix comes from the resolved locale; the strings are the WebView
/// locales' `settings.notification.body.*` values verbatim.
pub fn notification_body(sanitized_title: &str, kind: ActivityKind, locale: Locale) -> String {
    let msg = match (locale, kind) {
        (Locale::En, ActivityKind::ProcessExit) => "Process exited",
        (Locale::En, ActivityKind::Output) => "New output",
        (Locale::En, ActivityKind::Bell) => "Bell",
        (Locale::Ja, ActivityKind::ProcessExit) => "プロセスが終了しました",
        (Locale::Ja, ActivityKind::Output) => "新しい出力",
        (Locale::Ja, ActivityKind::Bell) => "ベル",
    };
    format!("{sanitized_title}: {msg}")
}

// ── Agent-status notifications (task0007 / FR9) ──────────────────────────
//
// blocked/done transitions on panes the user is not looking at fire OS
// notifications, gated by (IMPLEMENTATION.md "Notification gating"):
// qualifying transition (blocked/done) -> pane not visible (foreground
// window + displayed tab) -> both `Settings::agent_status_notifications`
// and the existing global `Settings::notification_enabled` on -> the
// per-pane rate limit not exceeded.
//
// The gating decision ([`should_fire_agent_notification`]) is a pure
// function so it is testable without the GUI event loop or the
// `AgentStatusModel` (task0005) that owns the drained transition queue —
// every input (transition, visibility, settings, rate-limit state) is
// passed in explicitly, matching this module's existing
// [`TabActivityState`] / [`kind_enabled`] pattern above.

/// One transition drained from `AgentStatusModel`'s transition queue
/// (IMPLEMENTATION.md's GUI `AgentStatusModel` contract:
/// `{pane, old_state, new_state, name}`). Pane identity lives in the
/// caller's own rate-limit / visibility key, not here — this struct
/// carries only the fields the gating decision and notification body need.
///
/// `old_state` mirrors `AgentStatusModel::Transition::old_state`'s
/// `Option<AgentState>` shape (task0009 wiring): a pane's very first
/// report has no prior state, so a non-optional `AgentState` would force
/// an arbitrary default at the wire-up site. Not read by
/// [`should_fire_agent_notification`] or [`agent_notification_body`]
/// today — kept for parity with the drained model event and any future
/// consumer.
#[derive(Debug, Clone)]
pub struct AgentTransition {
    pub old_state: Option<AgentState>,
    pub new_state: AgentState,
    /// Sanitized agent name (sanitization is guaranteed upstream by the
    /// core `agent_status` module); `None` when the pane never reported
    /// one.
    pub name: Option<String>,
}

/// Minimum interval between fired agent-status notifications for one pane
/// (AC-4). Distinct from [`NOTIFICATION_THROTTLE`] (tab-activity
/// notifications key on output/bell cadence; agent-status notifications
/// key on state transitions, which are far less frequent).
pub const AGENT_NOTIFICATION_RATE_LIMIT: Duration = Duration::from_secs(30);

/// Whether `state` is a qualifying transition target for an agent
/// notification (AC-1): only `Blocked` and `Done` fire; `Working`/`Idle`
/// never do.
pub fn is_qualifying_agent_state(state: AgentState) -> bool {
    matches!(state, AgentState::Blocked | AgentState::Done)
}

/// Pure gating decision for one drained agent-status transition
/// (AC-1..AC-4). `rate_limit_ok` is a read-only check the caller obtains
/// from [`AgentNotificationRateLimiter::is_within_limit`] *before* calling
/// this function; the caller then records the fire (if any) via
/// [`AgentNotificationRateLimiter::record`] — this function performs no
/// mutation itself.
pub fn should_fire_agent_notification(
    new_state: AgentState,
    pane_visible: bool,
    agent_notifications_enabled: bool,
    global_notifications_enabled: bool,
    rate_limit_ok: bool,
) -> bool {
    is_qualifying_agent_state(new_state)
        && !pane_visible
        && agent_notifications_enabled
        && global_notifications_enabled
        && rate_limit_ok
}

/// Per-pane rate limiter for agent-status notifications (AC-4). Generic
/// over the caller's own pane-key type — the concrete key (a plain tab's
/// stable id vs. a mux pane's `public_pane_id`) is owned by the
/// integration wiring that drains `AgentStatusModel`, not this module.
///
/// Only a notification that actually fires re-arms the window: a
/// transition suppressed by another gate (visibility, either settings
/// switch) is dropped, not queued, and must not extend the limiter window
/// (IMPLEMENTATION.md: "suppressed notifications are dropped (not
/// queued)"). Callers therefore consult [`Self::is_within_limit`] (a pure
/// read) as one input to [`should_fire_agent_notification`], and only call
/// [`Self::record`] once that combined decision is `true`.
#[derive(Debug)]
pub struct AgentNotificationRateLimiter<K> {
    last_fired: HashMap<K, Instant>,
}

impl<K> Default for AgentNotificationRateLimiter<K> {
    fn default() -> Self {
        Self {
            last_fired: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> AgentNotificationRateLimiter<K> {
    /// Read-only check: `true` when `key` is outside the rate-limit window
    /// (never fired, or its last fire is old enough). Does not record
    /// anything.
    pub fn is_within_limit(&self, key: &K, now: Instant) -> bool {
        match self.last_fired.get(key) {
            Some(prev) => now.duration_since(*prev) >= AGENT_NOTIFICATION_RATE_LIMIT,
            None => true,
        }
    }

    /// Record a fired notification's timestamp, (re)arming the window.
    pub fn record(&mut self, key: K, now: Instant) {
        self.last_fired.insert(key, now);
    }

    /// Drop bookkeeping for a pane that closed, mirroring
    /// `AgentStatusModel`'s "discard on tab/pane close" contract so a
    /// reused key does not inherit a stale window.
    pub fn discard(&mut self, key: &K) {
        self.last_fired.remove(key);
    }
}

/// Neutral fallback used in [`agent_notification_body`] when a transition's
/// pane never reported an agent name.
fn agent_name_fallback(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "Agent",
        Locale::Ja => "エージェント",
    }
}

/// Notification body for a qualifying agent-status transition: the
/// sanitized agent name (or the neutral fallback) plus the tab title, per
/// the task plan's body format ("uses the model's name ... or a neutral
/// fallback, plus the tab title").
pub fn agent_notification_body(
    transition: &AgentTransition,
    tab_title: &str,
    locale: Locale,
) -> String {
    let name = transition
        .name
        .as_deref()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| agent_name_fallback(locale));
    let state_msg = match (locale, transition.new_state) {
        (Locale::En, AgentState::Blocked) => "blocked",
        (Locale::En, AgentState::Done) => "done",
        (Locale::Ja, AgentState::Blocked) => "ブロック中",
        (Locale::Ja, AgentState::Done) => "完了",
        // Working/Idle never reach here in practice (the caller gates on
        // `is_qualifying_agent_state` first) — matched exhaustively rather
        // than panicking on an unexpected state.
        (Locale::En, AgentState::Working | AgentState::Idle) => "active",
        (Locale::Ja, AgentState::Working | AgentState::Idle) => "実行中",
    };
    format!("{name}: {tab_title} ({state_msg})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    // ── TabActivityState::mark ───────────────────────────────────────

    #[test]
    fn mark_bell_always_registers() {
        let mut s = TabActivityState::default();
        let now = t0();
        assert!(s.mark(ActivityKind::Bell, now));
        assert!(s.has_activity);
        // No throttle for bell marks (only the notification layer throttles).
        assert!(s.mark(ActivityKind::Bell, now));
    }

    #[test]
    fn mark_process_exit_always_registers() {
        let mut s = TabActivityState::default();
        assert!(s.mark(ActivityKind::ProcessExit, t0()));
        assert!(s.has_activity);
    }

    #[test]
    fn mark_output_throttles_inside_one_second() {
        let mut s = TabActivityState::default();
        let now = t0();
        assert!(s.mark(ActivityKind::Output, now));
        assert!(!s.mark(ActivityKind::Output, now + Duration::from_millis(999)));
        assert!(s.mark(ActivityKind::Output, now + Duration::from_millis(1000)));
    }

    #[test]
    fn output_throttle_does_not_block_other_kinds() {
        let mut s = TabActivityState::default();
        let now = t0();
        assert!(s.mark(ActivityKind::Output, now));
        assert!(s.mark(ActivityKind::Bell, now));
    }

    #[test]
    fn clear_resets_dot_but_keeps_throttle_windows() {
        let mut s = TabActivityState::default();
        let now = t0();
        assert!(s.mark(ActivityKind::Output, now));
        s.clear();
        assert!(!s.has_activity);
        // Output mark inside the window still throttled after clear —
        // matches the WebView tracker, where clearActivity leaves the
        // outputThrottleTimers map untouched.
        assert!(!s.mark(ActivityKind::Output, now + Duration::from_millis(500)));
    }

    // ── TabActivityState::should_notify ─────────────────────────────

    #[test]
    fn notify_throttles_inside_five_seconds() {
        let mut s = TabActivityState::default();
        let now = t0();
        assert!(s.should_notify(ActivityKind::Bell, now));
        assert!(!s.should_notify(ActivityKind::Bell, now + Duration::from_millis(4999)));
        assert!(s.should_notify(ActivityKind::Bell, now + Duration::from_millis(5000)));
    }

    #[test]
    fn process_exit_bypasses_notify_throttle_and_rearms_it() {
        let mut s = TabActivityState::default();
        let now = t0();
        assert!(s.should_notify(ActivityKind::Bell, now));
        // Inside the window, but ProcessExit goes through…
        let exit_at = now + Duration::from_millis(1000);
        assert!(s.should_notify(ActivityKind::ProcessExit, exit_at));
        // …and re-arms the window from its own dispatch time.
        assert!(!s.should_notify(ActivityKind::Output, exit_at + Duration::from_millis(4999)));
        assert!(s.should_notify(ActivityKind::Output, exit_at + Duration::from_millis(5000)));
    }

    // ── kind_enabled ─────────────────────────────────────────────────

    #[test]
    fn kind_enabled_follows_per_kind_settings() {
        let mut settings = Settings::default();
        // Defaults: exit=true, output=false, bell=true.
        assert!(kind_enabled(&settings, ActivityKind::ProcessExit));
        assert!(!kind_enabled(&settings, ActivityKind::Output));
        assert!(kind_enabled(&settings, ActivityKind::Bell));

        settings.notify_on_process_exit = false;
        settings.notify_on_output = true;
        settings.notify_on_bell = false;
        assert!(!kind_enabled(&settings, ActivityKind::ProcessExit));
        assert!(kind_enabled(&settings, ActivityKind::Output));
        assert!(!kind_enabled(&settings, ActivityKind::Bell));
    }

    // ── sanitize_title / notification_body ──────────────────────────

    #[test]
    fn sanitize_strips_csi_sequences() {
        assert_eq!(sanitize_title("\x1b[31mred\x1b[0m title"), "red title");
    }

    #[test]
    fn sanitize_strips_control_chars() {
        assert_eq!(sanitize_title("a\x07b\x00c\u{9f}d"), "abcd");
    }

    #[test]
    fn sanitize_truncates_at_100_chars() {
        let long = "あ".repeat(150);
        let out = sanitize_title(&long);
        assert_eq!(out.chars().count(), 100);
    }

    #[test]
    fn sanitize_passes_plain_titles_through() {
        assert_eq!(sanitize_title("zsh — ~/src"), "zsh — ~/src");
    }

    #[test]
    fn sanitize_bounds_pathological_input() {
        // A multi-100k title (OSC buffers allow up to 16 MiB) must not
        // be scanned in full — the input cap bounds the work and the
        // output still lands at the 100-char display cap.
        let huge = "x".repeat(100_000);
        let out = sanitize_title(&huge);
        assert_eq!(out.chars().count(), 100);
    }

    #[test]
    fn body_formats_match_webview_strings() {
        assert_eq!(
            notification_body("tab", ActivityKind::ProcessExit, Locale::En),
            "tab: Process exited"
        );
        assert_eq!(
            notification_body("tab", ActivityKind::Output, Locale::En),
            "tab: New output"
        );
        assert_eq!(
            notification_body("tab", ActivityKind::Bell, Locale::En),
            "tab: Bell"
        );
    }

    #[test]
    fn body_formats_match_webview_ja_strings() {
        // Values from src/i18n/locales/ja.json `settings.notification.body.*`.
        assert_eq!(
            notification_body("tab", ActivityKind::ProcessExit, Locale::Ja),
            "tab: プロセスが終了しました"
        );
        assert_eq!(
            notification_body("tab", ActivityKind::Output, Locale::Ja),
            "tab: 新しい出力"
        );
        assert_eq!(
            notification_body("tab", ActivityKind::Bell, Locale::Ja),
            "tab: ベル"
        );
    }

    // ── Agent-status notifications (task0007) ───────────────────────

    fn transition(new_state: AgentState) -> AgentTransition {
        AgentTransition {
            old_state: Some(AgentState::Working),
            new_state,
            name: Some("claude".to_string()),
        }
    }

    // AC-1: qualifying transition target.
    #[test]
    fn is_qualifying_agent_state_ac1_blocked_and_done_only() {
        assert!(is_qualifying_agent_state(AgentState::Blocked));
        assert!(is_qualifying_agent_state(AgentState::Done));
        assert!(!is_qualifying_agent_state(AgentState::Working));
        assert!(!is_qualifying_agent_state(AgentState::Idle));
    }

    // AC-1: a non-visible pane fires exactly one notification for
    // blocked/done; working/idle never fire, regardless of visibility.
    #[test]
    fn should_fire_ac1_blocked_and_done_fire_working_idle_never_fire() {
        for state in [AgentState::Blocked, AgentState::Done] {
            assert!(should_fire_agent_notification(
                state, false, true, true, true
            ));
        }
        for state in [AgentState::Working, AgentState::Idle] {
            assert!(!should_fire_agent_notification(
                state, false, true, true, true
            ));
        }
    }

    // AC-2: a qualifying transition on the visible pane does not fire.
    #[test]
    fn should_fire_ac2_visible_pane_does_not_fire() {
        assert!(!should_fire_agent_notification(
            AgentState::Blocked,
            true,
            true,
            true,
            true
        ));
        assert!(!should_fire_agent_notification(
            AgentState::Done,
            true,
            true,
            true,
            true
        ));
    }

    // AC-3: either settings switch off suppresses the notification.
    #[test]
    fn should_fire_ac3_either_settings_switch_off_suppresses() {
        // Agent-notification setting off.
        assert!(!should_fire_agent_notification(
            AgentState::Blocked,
            false,
            false,
            true,
            true
        ));
        // Global notification switch off.
        assert!(!should_fire_agent_notification(
            AgentState::Blocked,
            false,
            true,
            false,
            true
        ));
        // Both off.
        assert!(!should_fire_agent_notification(
            AgentState::Done,
            false,
            false,
            false,
            true
        ));
    }

    // AC-4: the rate-limit input gate suppresses when exceeded.
    #[test]
    fn should_fire_ac4_rate_limit_not_ok_suppresses() {
        assert!(!should_fire_agent_notification(
            AgentState::Blocked,
            false,
            true,
            true,
            false
        ));
    }

    // AC-4: two qualifying transitions on one pane inside the rate-limit
    // interval fire only the first; a transition after the interval fires.
    #[test]
    fn rate_limiter_ac4_throttles_within_window_then_allows_after() {
        let mut limiter: AgentNotificationRateLimiter<&str> =
            AgentNotificationRateLimiter::default();
        let now = Instant::now();

        // First transition: never fired for this key -> within limit.
        assert!(limiter.is_within_limit(&"pane-1", now));
        limiter.record("pane-1", now);

        // Second transition, just inside the window: throttled.
        let second = now + AGENT_NOTIFICATION_RATE_LIMIT - Duration::from_millis(1);
        assert!(!limiter.is_within_limit(&"pane-1", second));

        // A transition on a DIFFERENT pane is unaffected.
        assert!(limiter.is_within_limit(&"pane-2", second));

        // Third transition, exactly at the interval boundary: allowed.
        let third = now + AGENT_NOTIFICATION_RATE_LIMIT;
        assert!(limiter.is_within_limit(&"pane-1", third));
    }

    // AC-4 (design note): a notification suppressed by another gate must
    // not consume/extend the rate-limit window — only an actual fire calls
    // `record`.
    #[test]
    fn rate_limiter_suppressed_attempt_does_not_arm_window() {
        let limiter: AgentNotificationRateLimiter<&str> = AgentNotificationRateLimiter::default();
        let now = Instant::now();

        // Simulate a transition that was suppressed by visibility (the
        // caller never calls `record` because `should_fire_*` was false).
        let visible = true;
        let fire = should_fire_agent_notification(AgentState::Blocked, visible, true, true, true);
        assert!(!fire);
        // No `record` call — the window must still be open.
        assert!(limiter.is_within_limit(&"pane-1", now));
    }

    #[test]
    fn rate_limiter_discard_drops_bookkeeping_for_closed_pane() {
        let mut limiter: AgentNotificationRateLimiter<&str> =
            AgentNotificationRateLimiter::default();
        let now = Instant::now();
        limiter.record("pane-1", now);
        assert!(!limiter.is_within_limit(&"pane-1", now));
        limiter.discard(&"pane-1");
        assert!(limiter.is_within_limit(&"pane-1", now));
    }

    #[test]
    fn agent_notification_body_uses_sanitized_name_and_tab_title() {
        let body = agent_notification_body(&transition(AgentState::Blocked), "my-tab", Locale::En);
        assert_eq!(body, "claude: my-tab (blocked)");

        let body = agent_notification_body(&transition(AgentState::Done), "my-tab", Locale::Ja);
        assert_eq!(body, "claude: my-tab (完了)");
    }

    #[test]
    fn agent_notification_body_falls_back_to_neutral_name_when_absent() {
        let mut t = transition(AgentState::Blocked);
        t.name = None;
        assert_eq!(
            agent_notification_body(&t, "my-tab", Locale::En),
            "Agent: my-tab (blocked)"
        );
        assert_eq!(
            agent_notification_body(&t, "my-tab", Locale::Ja),
            "エージェント: my-tab (ブロック中)"
        );
    }
}
