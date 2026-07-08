//! Per-frame view model for the status-bar widget.
//!
//! The view model decouples the egui draw layer from the runtime
//! (template engine + providers + OSC dispatcher + mux state). The
//! drawer reads `Vec<RichTextRun>` lists and a small amount of
//! per-row metadata; it does not touch `App`.

use crate::html::RichTextRun;

/// 3-row layered model. `enabled = false` short-circuits drawing.
///
/// `PartialEq` is provided so the render loop can compare the current
/// frame's view model against the previous one and bypass the
/// dirty-row skip optimization when status-bar content has changed
/// (e.g. the wall clock ticked over) on an otherwise-idle PTY.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusBarViewModel {
    pub enabled: bool,
    /// Optional font-size override (egui logical points). `None`
    /// keeps the widget's default.
    pub font_size: Option<f32>,
    pub osc: OscRow,
    pub app_line1: AppRow,
    pub app_line2: AppRow,
}

/// OSC layer (3rd row). `left` / `right` carry raw post-strip text
/// (no HTML, no styling). `forced_visible` reflects the OSC writer's
/// most recent show/hide request; `None` means "auto" (visible only
/// when at least one side is non-empty).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OscRow {
    pub left: String,
    pub right: String,
    pub forced_visible: Option<bool>,
}

impl OscRow {
    /// `true` when the row should appear given the auto-hide rule
    /// (FR12).
    ///
    /// Mux attachment alone does NOT render the row. The daemon
    /// signals "I'm pushing OSC content" by setting
    /// `forced_visible = Some(true)` (in `runtime::build_osc_row` when
    /// a `StatusUpdateMsg` arrives); until then both sides are empty
    /// and the row stays hidden — empty content (the daemon explicitly
    /// pushed `left = ""` / `right = ""`) and absent content (no
    /// `StatusUpdateMsg` yet) are distinguished by `forced_visible`,
    /// not by `has_mux_session`.
    pub fn should_render(&self) -> bool {
        if self.forced_visible == Some(false) {
            return false;
        }
        if self.forced_visible == Some(true) {
            return true;
        }
        !self.left.is_empty() || !self.right.is_empty()
    }
}

/// App row (Lines 1 & 2). Each side is a pre-resolved styled run
/// list ready for egui rendering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppRow {
    pub left: Vec<RichTextRun>,
    pub right: Vec<RichTextRun>,
}

impl AppRow {
    /// `true` when at least one side has visible runs (i.e. non-empty
    /// text or a line break). Used by FR12 for App Line 2 auto-hide.
    pub fn has_content(&self) -> bool {
        runs_have_content(&self.left) || runs_have_content(&self.right)
    }
}

fn runs_have_content(runs: &[RichTextRun]) -> bool {
    runs.iter().any(|r| !r.text.is_empty() || r.line_break)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc_row_auto_hides_when_empty_and_not_forced() {
        let row = OscRow::default();
        assert!(!row.should_render());
    }

    #[test]
    fn osc_row_renders_when_either_side_present() {
        let row = OscRow {
            left: "hi".to_string(),
            ..Default::default()
        };
        assert!(row.should_render());
    }

    /// Mux attachment alone must NOT render the row — distinguishes
    /// "daemon explicitly pushed empty content" (forced_visible Some(true))
    /// from "no StatusUpdateMsg received yet" (both sides empty,
    /// forced_visible None). Without this rule a freshly-attached tab
    /// shows an empty OSC band before the daemon's first push.
    #[test]
    fn osc_row_hides_when_only_mux_attached_with_no_content() {
        let row = OscRow::default();
        assert!(!row.should_render());
    }

    #[test]
    fn osc_row_force_hide_overrides_content() {
        let row = OscRow {
            left: "hi".to_string(),
            forced_visible: Some(false),
            ..Default::default()
        };
        assert!(!row.should_render());
    }

    #[test]
    fn osc_row_force_show_overrides_empty() {
        let row = OscRow {
            forced_visible: Some(true),
            ..Default::default()
        };
        assert!(row.should_render());
    }

    #[test]
    fn app_row_has_content_for_text_run() {
        let row = AppRow {
            left: vec![RichTextRun {
                text: "x".to_string(),
                bold: false,
                italic: false,
                underline: false,
                color: None,
                line_break: false,
            }],
            ..Default::default()
        };
        assert!(row.has_content());
    }

    #[test]
    fn app_row_empty_returns_false() {
        assert!(!AppRow::default().has_content());
    }
}
