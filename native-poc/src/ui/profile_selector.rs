//! Modal profile selector (egui overlay).
//!
//! Port of the WebView build's `src/profile/profile-selector.ts` +
//! `settings-panel.css` (`.profile-selector-*`). A dimmed full-window
//! layer hosts a centered MD3 dialog listing the configured profiles;
//! each row shows the profile name, an optional "Default" badge, and the
//! profile's `shell_path`.
//!
//! Interaction split (mirrors the search bar):
//! - **Keyboard** (Up/Down/Home/End wrap-around, Enter/Space confirm,
//!   Escape cancel) is handled one layer up in `window_host` so it works
//!   regardless of egui focus; while the selector is visible no key
//!   reaches the PTY.
//! - **Pointer** (row click confirms, click outside the dialog cancels)
//!   is handled here and reported via [`ProfileSelectorEvent`].

use egui::{Align2, Area, Color32, FontId, Frame, Id, Margin, Order, Rounding, Sense};

use crate::ui::md3;

/// Dialog max width (`.profile-selector-dialog { max-width: 400px }`).
const DIALOG_MAX_W: f32 = 400.0;
/// Dialog padding (`padding: 24px`).
const DIALOG_PAD: f32 = 24.0;
/// Dialog corner radius (`--md-sys-shape-corner-extra-large` = 28px).
const DIALOG_ROUNDING: f32 = 28.0;
/// Title font size / bottom margin (`.profile-selector-title`).
const TITLE_FONT: f32 = 22.0;
const TITLE_MARGIN_BOTTOM: f32 = 16.0;
/// Row padding (`.profile-selector-item { padding: 12px 16px }`).
const ROW_PAD_X: f32 = 16.0;
const ROW_PAD_Y: f32 = 12.0;
/// Row corner radius (`--md-sys-shape-corner-medium` = 12px).
const ROW_ROUNDING: f32 = 12.0;
/// Gap between rows (`.profile-selector-list { gap: 4px }`).
const ROW_GAP: f32 = 4.0;
/// Gap between name / badge / shell inside a row (`gap: 8px`).
const ROW_INNER_GAP: f32 = 8.0;
/// Name / shell font sizes (`.profile-selector-item-name` / `-shell`).
const NAME_FONT: f32 = 14.0;
const SHELL_FONT: f32 = 12.0;
/// Badge metrics (`.profile-default-badge`).
const BADGE_FONT: f32 = 11.0;
const BADGE_PAD_X: f32 = 8.0;
const BADGE_PAD_Y: f32 = 2.0;
/// Overlay scrim `rgba(0, 0, 0, 0.5)`.
const SCRIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 128);
/// List viewport cap relative to the window height (`max-height: 60vh`
/// on the dialog; the list scrolls inside it).
const DIALOG_MAX_H_FRAC: f32 = 0.6;

/// Modal state. Lives on `App` so the keyboard path in `window_host` and
/// the egui draw path share the highlight cursor.
#[derive(Debug, Default)]
pub struct ProfileSelectorState {
    /// Whether the modal is on screen (and capturing the keyboard).
    pub visible: bool,
    /// Highlighted row index (the WebView's `activeIndex`).
    pub selected: usize,
    /// Set on the frame the modal opens; the draw path scrolls the
    /// highlighted row into view when this is on.
    pub scroll_request: bool,
    /// New-tab chooser mode (the `+` button / `TabEvent::New`): a
    /// synthetic "Global Settings" row is prepended at index 0 and the
    /// title becomes "New Tab". Port of the WebView's
    /// `handleNewTabClick` dialog ("Global Settings" + each profile,
    /// default profile preselected); the WebView's MD3 select + Open
    /// button becomes a list row choice here.
    pub include_global: bool,
}

impl ProfileSelectorState {
    /// Open the modal with the highlight reset to the first row
    /// (profiles only — the `profile_selector` keybind).
    pub fn open(&mut self) {
        self.visible = true;
        self.selected = 0;
        self.scroll_request = true;
        self.include_global = false;
    }

    /// Open in new-tab chooser mode with a leading "Global Settings"
    /// row. `selected` is the initial highlight **row** index (0 =
    /// global, `i + 1` = profile `i`); the caller passes the default
    /// profile's row when one exists (WebView parity: the select is
    /// preseeded with the default profile, else "Global Settings").
    pub fn open_with_global(&mut self, selected: usize) {
        self.visible = true;
        self.selected = selected;
        self.scroll_request = true;
        self.include_global = true;
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Move the highlight by one row, wrapping (WebView parity:
    /// `(activeIndex + 1) % profiles.length` and the `+ len` mirror).
    pub fn move_selection(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let len = len as isize;
        let next = (self.selected as isize + delta).rem_euclid(len);
        self.selected = next as usize;
        self.scroll_request = true;
    }

    /// Jump the highlight to the first / last row (Home / End).
    pub fn select_edge(&mut self, end: bool, len: usize) {
        if len == 0 {
            return;
        }
        self.selected = if end { len - 1 } else { 0 };
        self.scroll_request = true;
    }

    /// Decode a row index into a domain [`Choice`]. This is the SINGLE
    /// site that knows the synthetic-row offset: in new-tab chooser mode
    /// (`include_global`) row 0 is the "Global Settings" row and row
    /// `i + 1` maps to `profiles[i]`; otherwise every row maps directly
    /// to `profiles[row]`. Both the renderer (which prepends the Global
    /// row when `include_global`) and the confirm path go through this
    /// one mapping, so the offset can never drift between them.
    pub fn row_to_choice(&self, row: usize) -> Choice {
        if self.include_global {
            match row.checked_sub(1) {
                None => Choice::Global,
                Some(i) => Choice::Profile(i),
            }
        } else {
            Choice::Profile(row)
        }
    }

    /// Inverse of [`Self::row_to_choice`] for a profile index: the row a
    /// given `profiles[i]` occupies. Used by `open_with_global` callers
    /// to preselect the default profile's row.
    pub fn profile_row(&self, profile_index: usize) -> usize {
        profile_index + usize::from(self.include_global)
    }
}

/// A resolved selector choice, decoded from a row index by
/// [`ProfileSelectorState::row_to_choice`]. Centralizing the
/// synthetic-row offset here keeps the renderer (which prepends the
/// Global row) and the confirm path from drifting apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// New-tab chooser "Global Settings" row — spawn with global settings.
    Global,
    /// Profile at this index into `settings.profiles`.
    Profile(usize),
}

/// One pointer interaction yielded by a selector frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSelectorEvent {
    /// A row was clicked — spawn a tab with that profile (index into
    /// `settings.profiles`).
    Confirm(usize),
    /// The scrim outside the dialog was clicked — dismiss.
    Cancel,
}

/// Row display data extracted from a profile (name + flags only; the
/// caller maps `app_settings::Profile` so this module stays UI-only).
pub struct ProfileRow<'a> {
    pub name: &'a str,
    pub shell_path: &'a str,
    pub is_default: bool,
}

/// Draw the modal. Returns the pointer interaction, if any. The caller
/// applies [`ProfileSelectorEvent`] after the egui pass (same pattern as
/// the tab-bar events) and keeps drawing while `state.visible`.
pub fn draw(
    ctx: &egui::Context,
    state: &mut ProfileSelectorState,
    rows: &[ProfileRow<'_>],
    title: &str,
    default_badge: &str,
) -> Option<ProfileSelectorEvent> {
    if !state.visible {
        return None;
    }
    let mut event = None;
    let screen = ctx.screen_rect();

    // Scrim: full-window dim layer. A click that lands on the scrim (and
    // not on the dialog drawn above it) cancels, mirroring the WebView's
    // overlay click-to-dismiss.
    let scrim_response = Area::new(Id::new("profile-selector-scrim"))
        .order(Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let painter = ui.painter();
            painter.rect_filled(screen, 0.0, SCRIM);
            ui.allocate_rect(screen, Sense::click())
        })
        .inner;
    if scrim_response.clicked() {
        event = Some(ProfileSelectorEvent::Cancel);
    }

    let dialog_w = (screen.width() * 0.9).min(DIALOG_MAX_W);
    let max_h = screen.height() * DIALOG_MAX_H_FRAC;

    Area::new(Id::new("profile-selector-dialog"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            Frame::none()
                .fill(md3::surface_container_high())
                .rounding(Rounding::same(DIALOG_ROUNDING))
                .inner_margin(Margin::same(DIALOG_PAD))
                .shadow(egui::epaint::Shadow {
                    offset: egui::vec2(0.0, 8.0),
                    blur: 32.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(77),
                })
                .show(ui, |ui| {
                    ui.set_width(dialog_w - 2.0 * DIALOG_PAD);
                    ui.set_max_height(max_h - 2.0 * DIALOG_PAD);

                    ui.label(
                        egui::RichText::new(title)
                            .font(FontId::proportional(TITLE_FONT))
                            .color(md3::on_surface()),
                    );
                    ui.add_space(TITLE_MARGIN_BOTTOM);

                    egui::ScrollArea::vertical()
                        .max_height(max_h - 2.0 * DIALOG_PAD - TITLE_FONT - TITLE_MARGIN_BOTTOM)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = ROW_GAP;
                            for (i, row) in rows.iter().enumerate() {
                                if let Some(idx) = draw_row(ui, state, i, row, default_badge) {
                                    event = Some(ProfileSelectorEvent::Confirm(idx));
                                }
                            }
                        });
                });
        });

    state.scroll_request = false;
    event
}

/// Draw a single profile row. Returns `Some(index)` when clicked.
fn draw_row(
    ui: &mut egui::Ui,
    state: &mut ProfileSelectorState,
    index: usize,
    row: &ProfileRow<'_>,
    default_badge: &str,
) -> Option<usize> {
    let active = index == state.selected;

    let name_font = FontId::proportional(NAME_FONT);
    let shell_font = FontId::proportional(SHELL_FONT);
    let badge_font = FontId::proportional(BADGE_FONT);

    let row_h = ROW_PAD_Y * 2.0 + NAME_FONT * 20.0 / 14.0; // line-height 20px
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());

    if state.scroll_request && active {
        ui.scroll_to_rect(rect, None);
    }
    // Hovering moves the highlight (a hover state and the active state
    // are visually distinct in the WebView, but keyboard + pointer both
    // drive a single cursor here; the WebView's hover tint shows on the
    // hovered row anyway via the paint below).
    let hovered = response.hovered();

    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, ROW_ROUNDING, md3::secondary_container());
    } else if hovered {
        // on-surface @ 8% state layer.
        painter.rect_filled(
            rect,
            ROW_ROUNDING,
            md3::state_layer(md3::on_surface(), 0.08),
        );
    }

    let name_color = if active {
        md3::on_secondary_container()
    } else {
        md3::on_surface()
    };
    let shell_color = if active {
        md3::state_layer(md3::on_secondary_container(), 0.7)
    } else {
        md3::on_surface_variant()
    };

    let mut x = rect.min.x + ROW_PAD_X;
    let cy = rect.center().y;

    // Name
    let name_galley = painter.layout_no_wrap(row.name.to_string(), name_font, name_color);
    let name_pos = egui::pos2(x, cy - name_galley.size().y / 2.0);
    painter.galley(name_pos, name_galley.clone(), name_color);
    x += name_galley.size().x + ROW_INNER_GAP;

    // Default badge (primary-container pill)
    if row.is_default {
        let badge_galley = painter.layout_no_wrap(
            default_badge.to_string(),
            badge_font,
            md3::on_primary_container(),
        );
        let badge_w = badge_galley.size().x + BADGE_PAD_X * 2.0;
        let badge_h = badge_galley.size().y + BADGE_PAD_Y * 2.0;
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(x, cy - badge_h / 2.0),
            egui::vec2(badge_w, badge_h),
        );
        painter.rect_filled(badge_rect, badge_h / 2.0, md3::primary_container());
        painter.galley(
            egui::pos2(x + BADGE_PAD_X, cy - badge_galley.size().y / 2.0),
            badge_galley,
            md3::on_primary_container(),
        );
        x += badge_w + ROW_INNER_GAP;
    }

    // Shell path (truncated to the row's right padding)
    if !row.shell_path.is_empty() {
        let avail = (rect.max.x - ROW_PAD_X - x).max(0.0);
        if avail > 0.0 {
            let mut job = egui::text::LayoutJob::simple_singleline(
                row.shell_path.to_string(),
                shell_font,
                shell_color,
            );
            job.wrap.max_width = avail;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let galley = painter.layout_job(job);
            painter.galley(
                egui::pos2(x, cy - galley.size().y / 2.0),
                galley,
                shell_color,
            );
        }
    }

    if response.clicked() {
        Some(index)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WebView parity: `(activeIndex ± 1 + len) % len` wrap-around and
    // Home/End edge jumps (`profile-selector.ts` handleKeydown).

    #[test]
    fn open_resets_selection() {
        let mut s = ProfileSelectorState {
            visible: false,
            selected: 3,
            scroll_request: false,
            include_global: true,
        };
        s.open();
        assert!(s.visible);
        assert_eq!(s.selected, 0);
        assert!(s.scroll_request);
        assert!(!s.include_global);
    }

    #[test]
    fn open_with_global_seeds_selection_and_flag() {
        let mut s = ProfileSelectorState::default();
        // Default profile at profiles[1] → row 2 preselected.
        s.open_with_global(2);
        assert!(s.visible);
        assert!(s.include_global);
        assert_eq!(s.selected, 2);
    }

    #[test]
    fn move_selection_wraps_forward_and_backward() {
        let mut s = ProfileSelectorState::default();
        s.open();
        s.move_selection(1, 3);
        assert_eq!(s.selected, 1);
        s.move_selection(1, 3);
        s.move_selection(1, 3);
        assert_eq!(s.selected, 0, "wraps past the end");
        s.move_selection(-1, 3);
        assert_eq!(s.selected, 2, "wraps before the start");
    }

    #[test]
    fn move_selection_empty_list_is_noop() {
        let mut s = ProfileSelectorState::default();
        s.move_selection(1, 0);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn select_edge_home_end() {
        let mut s = ProfileSelectorState::default();
        s.select_edge(true, 5);
        assert_eq!(s.selected, 4);
        s.select_edge(false, 5);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn row_to_choice_selector_mode_is_direct() {
        let mut s = ProfileSelectorState::default();
        s.open(); // include_global = false
        assert_eq!(s.row_to_choice(0), Choice::Profile(0));
        assert_eq!(s.row_to_choice(2), Choice::Profile(2));
        assert_eq!(s.profile_row(2), 2);
    }

    #[test]
    fn row_to_choice_chooser_mode_offsets_global() {
        let mut s = ProfileSelectorState::default();
        s.open_with_global(0); // include_global = true
        assert_eq!(s.row_to_choice(0), Choice::Global);
        assert_eq!(s.row_to_choice(1), Choice::Profile(0));
        assert_eq!(s.row_to_choice(3), Choice::Profile(2));
        // profile_row is the inverse: profiles[2] sits at row 3.
        assert_eq!(s.profile_row(2), 3);
        assert_eq!(s.row_to_choice(s.profile_row(2)), Choice::Profile(2));
    }
}
