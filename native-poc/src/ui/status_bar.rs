//! Status-bar widget (Phase 4-D).
//!
//! Renders an [`egui::TopBottomPanel`] (top or bottom, per
//! [`crate::settings::StatusBarPosition`]) containing — depending on the
//! active tab's state:
//!
//! - **Mux mode** (`StatusBarState::mux` is `Some`): session name +
//!   daemon-supplied left/right segments (the daemon embeds the window
//!   list with the active window flagged) + local-clock `HH:MM:SS`.
//! - **Not in mux mode** (`StatusBarState::mux` is `None`): only the
//!   local clock.
//! - **Disabled** (`settings.statusbar.enabled = false`): no panel is
//!   inserted at all, and the [`egui::CentralPanel`] covers the full
//!   window. [`draw`] returns immediately in that case.
//!
//! The widget is intentionally pure: it receives all state through
//! parameters and never reaches into `App`. The caller (the render
//! pipeline in `render::draw_terminal`) is responsible for projecting
//! the active tab into a [`StatusBarState`] once per frame.

use std::time::SystemTime;

use egui::{Align, Color32, FontFamily, FontId, Layout, RichText};
use mux_ipc::protocol::StatusUpdateMsg;

use crate::settings::{Settings, StatusBarPosition};

/// Per-frame view-model for the status-bar widget. The caller projects
/// the active tab + the wall clock into this struct once per frame.
#[derive(Debug, Clone, Default)]
pub struct StatusBarState {
    /// When `Some`, the active tab is attached to a mux session. The
    /// session name is rendered as the leading segment of the panel;
    /// the daemon-supplied `left` / `right` strings carry the window
    /// list / active-window marker.
    pub mux: Option<MuxStatus>,
    /// Local wall clock formatted as `HH:MM:SS`. The widget itself does
    /// not query the clock so tests can drive deterministic values.
    /// Use [`format_local_clock`] to produce this from
    /// [`SystemTime::now`].
    pub clock_hhmmss: String,
}

/// View of an attached mux session for the status bar.
#[derive(Debug, Clone)]
pub struct MuxStatus {
    /// Session name (e.g. `"main"`); from `Tab::mux_session_name`.
    pub session_name: String,
    /// Daemon-supplied status payload. The `left` string typically
    /// holds the window list with the active window marked
    /// (e.g. `"1:shell 2:nvim* 3:test"`); `right` holds the right-aligned
    /// segment (e.g. hostname / battery). The widget renders both
    /// verbatim.
    pub status: StatusUpdateMsg,
}

/// Visual height of the status-bar strip in egui logical points.
/// Matches the tab-bar height so vertical rhythm stays consistent.
const STATUS_BAR_HEIGHT: f32 = 22.0;

/// Render the status bar. Returns immediately (no panel inserted) when
/// `settings.statusbar.enabled` is false.
///
/// This function is pure over `(state, settings)` — the only side
/// effect is the egui draw-list mutation through `ctx`. The caller
/// owns clock generation and `StatusUpdateMsg` plumbing.
pub fn draw(ctx: &egui::Context, state: &StatusBarState, settings: &Settings) {
    if !settings.statusbar.enabled {
        return;
    }

    let mut panel = match settings.statusbar.position {
        StatusBarPosition::Top => egui::TopBottomPanel::top("native-poc-status-bar"),
        StatusBarPosition::Bottom => egui::TopBottomPanel::bottom("native-poc-status-bar"),
    };
    panel = panel.exact_height(STATUS_BAR_HEIGHT);

    panel.show(ctx, |ui| {
        // Subtle background. We do not pin a specific color; egui's
        // panel default already differentiates from the central panel.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // Use a slightly smaller monospace font so window-list
            // segments line up cleanly. Tests inspect text content,
            // not styling.
            let font = FontId::new(12.0, FontFamily::Monospace);

            match &state.mux {
                Some(mux) => {
                    // Session badge (leading).
                    ui.label(
                        RichText::new(format!("[mux:{}]", mux.session_name))
                            .strong()
                            .font(font.clone()),
                    );
                    // Daemon-supplied left segment (window list, active
                    // window typically marked by the daemon — the widget
                    // renders verbatim).
                    if !mux.status.left.is_empty() {
                        ui.label(RichText::new(&mux.status.left).font(font.clone()));
                    }
                    // Right-aligned cluster: daemon's `right` segment +
                    // clock. We split right-to-left inside a
                    // `right_to_left` layout so the clock sits flush
                    // against the panel edge.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&state.clock_hhmmss)
                                .font(font.clone())
                                .color(Color32::LIGHT_GRAY),
                        );
                        if !mux.status.right.is_empty() {
                            ui.add_space(8.0);
                            ui.label(RichText::new(&mux.status.right).font(font.clone()));
                        }
                    });
                }
                None => {
                    // No mux: only the clock, flush against the right
                    // edge.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&state.clock_hhmmss)
                                .font(font.clone())
                                .color(Color32::LIGHT_GRAY),
                        );
                    });
                }
            }
        });
    });
}

/// Format a `SystemTime` as a local `HH:MM:SS` string.
///
/// On unix we use `libc::localtime_r` so the result respects the host
/// `TZ`. On non-unix targets we fall back to UTC computed from
/// `SystemTime::duration_since(UNIX_EPOCH)`; native-poc Phase 4 ships on
/// Linux + Windows but the unix path covers Linux today and the Windows
/// port (Phase 4-E) can extend this helper.
pub fn format_local_clock(now: SystemTime) -> String {
    let secs_since_epoch = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (h, m, s) = local_hms(secs_since_epoch);
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[cfg(unix)]
fn local_hms(secs_since_epoch: i64) -> (u32, u32, u32) {
    use std::mem::MaybeUninit;
    // SAFETY: `libc::localtime_r` writes a full `tm` struct into the
    // provided pointer; we pass a stack-allocated `MaybeUninit<tm>` and
    // only read it if the call returns a non-null pointer.
    let t: libc::time_t = secs_since_epoch as libc::time_t;
    let mut tm_buf: MaybeUninit<libc::tm> = MaybeUninit::uninit();
    let result = unsafe { libc::localtime_r(&t, tm_buf.as_mut_ptr()) };
    if result.is_null() {
        // Fall back to UTC arithmetic if the libc call fails (e.g. a
        // pathological TZ env). Mirrors the non-unix path.
        return utc_hms(secs_since_epoch);
    }
    let tm = unsafe { tm_buf.assume_init() };
    (tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32)
}

#[cfg(not(unix))]
fn local_hms(secs_since_epoch: i64) -> (u32, u32, u32) {
    utc_hms(secs_since_epoch)
}

/// Compute HH:MM:SS in UTC by integer arithmetic on a Unix timestamp.
/// Used as the non-unix fallback and as a backstop when `localtime_r`
/// fails on unix.
fn utc_hms(secs_since_epoch: i64) -> (u32, u32, u32) {
    let day_secs = secs_since_epoch.rem_euclid(86_400) as u32;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    (h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::RawInput;

    fn make_settings(enabled: bool, position: StatusBarPosition) -> Settings {
        let mut s = Settings::new();
        s.statusbar.enabled = enabled;
        s.statusbar.position = position;
        s
    }

    /// Walk the egui shape list produced by `draw` and concatenate every
    /// rendered text run. We use this in TS-status-1/2 to assert that
    /// substrings (session name, clock, etc.) made it into the frame.
    fn collected_text(items: &[egui::epaint::ClippedShape]) -> String {
        let mut out = String::new();
        for cs in items {
            walk_shape(&cs.shape, &mut out);
        }
        out
    }

    fn walk_shape(shape: &egui::epaint::Shape, out: &mut String) {
        use egui::epaint::Shape;
        match shape {
            Shape::Text(t) => {
                for row in &t.galley.rows {
                    for g in &row.glyphs {
                        out.push(g.chr);
                    }
                    out.push('\n');
                }
            }
            Shape::Vec(v) => {
                for s in v {
                    walk_shape(s, out);
                }
            }
            _ => {}
        }
    }

    /// Drive one egui pass and return all clipped shapes.
    fn run_one_frame(
        state: &StatusBarState,
        settings: &Settings,
    ) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 200.0),
        ));
        let output = ctx.run(input, |ctx| {
            // Insert a central panel so the layout reflects real usage —
            // the status bar widget reserves vertical space via
            // `TopBottomPanel`, and the central panel takes the rest.
            draw(ctx, state, settings);
            egui::CentralPanel::default().show(ctx, |_ui| {});
        });
        output.shapes
    }

    /// Run one egui pass and return:
    /// 1. the clipped shape list (for text scraping)
    /// 2. the central panel's `max_rect` after layout (used to detect
    ///    whether the status-bar panel reserved any vertical space).
    fn run_with_central_rect(
        state: &StatusBarState,
        settings: &Settings,
    ) -> (Vec<egui::epaint::ClippedShape>, egui::Rect) {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 200.0));
        let mut input = RawInput::default();
        input.screen_rect = Some(screen);
        let mut central_rect = egui::Rect::NOTHING;
        let output = ctx.run(input, |ctx| {
            draw(ctx, state, settings);
            egui::CentralPanel::default().show(ctx, |ui| {
                central_rect = ui.max_rect();
            });
        });
        (output.shapes, central_rect)
    }

    // ── TS-status-1: mux mode renders session + window list + clock ─────

    #[test]
    fn renders_session_window_list_and_clock_in_mux_mode() {
        let state = StatusBarState {
            mux: Some(MuxStatus {
                session_name: "main".to_string(),
                // Daemon-rendered window list with active window
                // marked by `*`. The widget renders the string
                // verbatim.
                status: StatusUpdateMsg {
                    left: "1:shell 2:nvim* 3:test".to_string(),
                    right: "host01".to_string(),
                },
            }),
            clock_hhmmss: "12:34:56".to_string(),
        };
        let settings = make_settings(true, StatusBarPosition::Bottom);
        let shapes = run_one_frame(&state, &settings);
        let text = collected_text(&shapes);
        assert!(
            text.contains("[mux:main]"),
            "session badge missing: {text:?}"
        );
        assert!(text.contains("1:shell"), "first window missing: {text:?}");
        assert!(
            text.contains("2:nvim*"),
            "active-window marker missing: {text:?}"
        );
        assert!(text.contains("3:test"), "third window missing: {text:?}");
        assert!(text.contains("host01"), "right segment missing: {text:?}");
        assert!(text.contains("12:34:56"), "clock missing: {text:?}");
    }

    // ── TS-status-2: non-mux mode shows only the clock ──────────────────

    #[test]
    fn renders_only_clock_when_no_mux_state() {
        let state = StatusBarState {
            mux: None,
            clock_hhmmss: "01:02:03".to_string(),
        };
        let settings = make_settings(true, StatusBarPosition::Bottom);
        let shapes = run_one_frame(&state, &settings);
        let text = collected_text(&shapes);
        assert!(
            text.contains("01:02:03"),
            "clock must render in non-mux mode: {text:?}"
        );
        assert!(
            !text.contains("[mux:"),
            "no mux badge expected when state.mux is None: {text:?}"
        );
    }

    #[test]
    fn renders_only_clock_when_no_mux_state_and_position_top() {
        let state = StatusBarState {
            mux: None,
            clock_hhmmss: "23:59:59".to_string(),
        };
        let settings = make_settings(true, StatusBarPosition::Top);
        let shapes = run_one_frame(&state, &settings);
        let text = collected_text(&shapes);
        assert!(text.contains("23:59:59"));
    }

    // ── TS-status-3: enabled=false inserts no panel ─────────────────────

    #[test]
    fn disabled_status_bar_does_not_insert_panel() {
        let state = StatusBarState {
            mux: Some(MuxStatus {
                session_name: "main".to_string(),
                status: StatusUpdateMsg {
                    left: "1:shell".to_string(),
                    right: "host".to_string(),
                },
            }),
            clock_hhmmss: "00:00:00".to_string(),
        };
        // Compare central-panel heights with vs without the status bar
        // enabled. When disabled, the widget must not insert a panel —
        // the central panel grows to exactly the same height it would
        // have if `draw` had been a no-op (modulo egui's intrinsic
        // margins, which are identical across both runs).
        let disabled = make_settings(false, StatusBarPosition::Bottom);
        let enabled = make_settings(true, StatusBarPosition::Bottom);
        let (shapes_off, central_off) = run_with_central_rect(&state, &disabled);
        let (_shapes_on, central_on) = run_with_central_rect(&state, &enabled);
        assert!(
            central_off.height() > central_on.height() + STATUS_BAR_HEIGHT - 1.0,
            "disabled status bar must leave the central panel at least \
             {STATUS_BAR_HEIGHT}px taller than the enabled case \
             (off={central_off:?}, on={central_on:?})"
        );
        let text = collected_text(&shapes_off);
        assert!(
            !text.contains("[mux:main]"),
            "no status text expected when disabled: {text:?}"
        );
        assert!(
            !text.contains("00:00:00"),
            "no clock expected when disabled: {text:?}"
        );
    }

    #[test]
    fn enabled_status_bar_reserves_panel_height() {
        // Sanity: enabling the widget reduces the central-panel height
        // by approximately `STATUS_BAR_HEIGHT` vs the disabled case.
        let state = StatusBarState {
            mux: None,
            clock_hhmmss: "00:00:00".to_string(),
        };
        let disabled = make_settings(false, StatusBarPosition::Bottom);
        let enabled = make_settings(true, StatusBarPosition::Bottom);
        let (_, central_off) = run_with_central_rect(&state, &disabled);
        let (_, central_on) = run_with_central_rect(&state, &enabled);
        assert!(
            central_off.height() > central_on.height(),
            "enabling the status bar must shrink the central panel \
             (off={central_off:?}, on={central_on:?})"
        );
    }

    // ── Clock formatter ─────────────────────────────────────────────────

    #[test]
    fn utc_hms_from_known_epoch() {
        // 1970-01-01T00:00:00Z
        assert_eq!(utc_hms(0), (0, 0, 0));
        // 1970-01-01T01:02:03Z = 3723 s
        assert_eq!(utc_hms(3723), (1, 2, 3));
        // 1970-01-01T23:59:59Z = 86399 s
        assert_eq!(utc_hms(86_399), (23, 59, 59));
        // 1970-01-02T00:00:00Z = 86400 s — day wraps to 0.
        assert_eq!(utc_hms(86_400), (0, 0, 0));
    }

    #[test]
    fn format_local_clock_returns_hhmmss_shape() {
        // We can't pin a specific value without bringing TZ into the
        // test, but we can assert the textual shape.
        let s = format_local_clock(SystemTime::now());
        assert_eq!(s.len(), 8, "format must be HH:MM:SS, got {s:?}");
        let bytes = s.as_bytes();
        assert_eq!(bytes[2], b':');
        assert_eq!(bytes[5], b':');
        assert!(s.chars().filter(|c| c.is_ascii_digit()).count() == 6);
    }
}
