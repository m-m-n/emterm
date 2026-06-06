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
//! - Notification bodies are fixed English strings; native-poc has no
//!   i18n layer (the `language` setting is not consumed yet).

use std::time::{Duration, Instant};

use crate::settings::Settings;

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
/// suffix is fixed English (`settings.notification.body.*` in the
/// WebView locales); native-poc has no i18n layer.
pub fn notification_body(sanitized_title: &str, kind: ActivityKind) -> String {
    let msg = match kind {
        ActivityKind::ProcessExit => "Process exited",
        ActivityKind::Output => "New output",
        ActivityKind::Bell => "Bell",
    };
    format!("{sanitized_title}: {msg}")
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
            notification_body("tab", ActivityKind::ProcessExit),
            "tab: Process exited"
        );
        assert_eq!(
            notification_body("tab", ActivityKind::Output),
            "tab: New output"
        );
        assert_eq!(notification_body("tab", ActivityKind::Bell), "tab: Bell");
    }
}
