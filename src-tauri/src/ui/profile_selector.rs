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

use egui::{Align2, Area, FontId, Frame, Id, Margin, Order, Rounding, Sense};

use crate::ui::dialog::tokens;
use crate::ui::keybinds::KeybindTable;
use crate::ui::md3;

/// Dialog surface width (`dialogs.layout.width-compact`).
const DIALOG_MAX_W: f32 = tokens::WIDTH_COMPACT;
/// Dialog padding (`dialogs.layout.padding`).
const DIALOG_PAD: f32 = tokens::PADDING;
/// Dialog corner radius (`dialogs.layout.corner-radius`).
const DIALOG_ROUNDING: f32 = tokens::CORNER_RADIUS;
/// Title font size / bottom margin (`title-large`).
const TITLE_FONT: f32 = tokens::TITLE_LARGE_SIZE;
const TITLE_MARGIN_BOTTOM: f32 = tokens::TITLE_TO_BODY_MARGIN;
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
    /// Tmux rows discovered when the new-tab chooser opened (task0001).
    /// Appended after the profile rows (`Global -> profiles -> tmux
    /// entries`); empty outside chooser mode. Handed in by
    /// `App::open_new_tab_chooser` — this module never runs discovery
    /// itself (IMPLEMENTATION.md: "UI never calls Discovery directly")
    /// and holds no tmux knowledge beyond rendering the label it is
    /// handed.
    pub tmux_entries: Vec<TmuxRow>,
}

impl ProfileSelectorState {
    /// Open the modal with the highlight reset to the first row
    /// (profiles only — the `profile_selector` keybind).
    pub fn open(&mut self) {
        self.visible = true;
        self.selected = 0;
        self.scroll_request = true;
        self.include_global = false;
        self.tmux_entries.clear();
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
        // The caller (`App::open_new_tab_chooser`) sets `tmux_entries`
        // right after this returns; start from empty so a stale list
        // from a previous chooser session never leaks in.
        self.tmux_entries.clear();
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
    /// (`include_global`) row 0 is the "Global Settings" row, rows
    /// `1..=num_profiles` map to `profiles[i]`, and any row beyond that
    /// maps to a tmux entry (`tmux_entries[i]`) — the combined ordering
    /// is `Global -> profiles -> tmux entries`. Outside chooser mode
    /// every row maps directly to `profiles[row]` (tmux rows never show
    /// there, so `num_profiles` is unused on that path). Both the
    /// renderer (which prepends the Global row and appends the tmux
    /// rows) and the confirm path go through this one mapping, so the
    /// offsets can never drift between them.
    pub fn row_to_choice(&self, row: usize, num_profiles: usize) -> Choice {
        if self.include_global {
            match row.checked_sub(1) {
                None => Choice::Global,
                Some(i) if i < num_profiles => Choice::Profile(i),
                Some(i) => Choice::Tmux(i - num_profiles),
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

impl ProfileSelectorState {
    /// Decide which row carries which shortcut label (the new-tab
    /// chooser's hints). Returns one optional label per row the dialog
    /// shows, in row order: `Global -> profiles -> tmux rows` in chooser
    /// mode, profiles only in selector mode.
    ///
    /// - Chooser mode: the Global row gets the reachable `new_tab_global`
    ///   label; the row of the FIRST profile whose flag is set (the
    ///   profile the `new_tab` keybind opens, see
    ///   `crate::profiles::default_profile`) gets the reachable `new_tab`
    ///   label; every other row — other profiles, later flagged profiles,
    ///   tmux rows — gets none.
    /// - Selector mode (`include_global` off): every entry is `None`.
    ///
    /// `default_flags` is the ordered `is_default` flag of
    /// `settings.profiles`. Row identity is decoded through
    /// [`Self::row_to_choice`], the single site that knows the row
    /// offsets. Pure: no egui context, no App, no I/O; the result is
    /// recomputed by the caller each frame and never stored.
    pub fn row_shortcut_labels(
        &self,
        default_flags: &[bool],
        keybinds: &KeybindTable,
    ) -> Vec<Option<String>> {
        let num_profiles = default_flags.len();
        let row_count = if self.include_global {
            1 + num_profiles + self.tmux_entries.len()
        } else {
            num_profiles
        };
        if !self.include_global {
            return vec![None; row_count];
        }
        let first_default = default_flags.iter().position(|&flag| flag);
        (0..row_count)
            .map(|row| match self.row_to_choice(row, num_profiles) {
                Choice::Global => keybinds.new_tab_global_label(),
                Choice::Profile(i) if Some(i) == first_default => keybinds.new_tab_label(),
                Choice::Profile(_) | Choice::Tmux(_) => None,
            })
            .collect()
    }
}

/// Horizontal geometry of one row, computed from the row's right edge and
/// the measured width of its shortcut label. Pure (no egui context), so
/// the layout rules are unit-testable; the painter reads the results.
///
/// A row without a label keeps today's geometry: the shell path runs to
/// the row's right padding and name / badge are not bounded.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RowGeometry {
    /// The label's `(left, right)` x span; `None` for a row without one.
    /// The right edge sits `ROW_PAD_X` inside the row's right edge.
    label: Option<(f32, f32)>,
    /// Right limit of the row's content. A labeled row stops
    /// `ROW_INNER_GAP` before the label's left edge; an unlabeled row at
    /// the row's right padding.
    content_right: f32,
    /// Whether name and badge must stay left of `content_right` (labeled
    /// rows only).
    bound_name_and_badge: bool,
}

impl RowGeometry {
    fn new(row_right: f32, label_width: Option<f32>) -> Self {
        match label_width {
            Some(width) => {
                let right = row_right - ROW_PAD_X;
                let left = right - width;
                Self {
                    label: Some((left, right)),
                    content_right: left - ROW_INNER_GAP,
                    bound_name_and_badge: true,
                }
            }
            None => Self {
                label: None,
                content_right: row_right - ROW_PAD_X,
                bound_name_and_badge: false,
            },
        }
    }

    /// Width the name may take when it starts at `name_x`, or `None` when
    /// the name is not bounded (painted at its natural width, as before).
    /// The badge's width and the gap in front of it are reserved first so
    /// the name yields (is ellipsized) rather than the badge.
    fn name_max_width(&self, name_x: f32, badge_width: Option<f32>) -> Option<f32> {
        if !self.bound_name_and_badge {
            return None;
        }
        let reserved = badge_width.map_or(0.0, |w| ROW_INNER_GAP + w);
        Some((self.content_right - name_x - reserved).max(0.0))
    }

    /// Whether a badge `badge_width` wide starting at `badge_x` stays
    /// within the content boundary (always true for unlabeled rows).
    fn badge_fits(&self, badge_x: f32, badge_width: f32) -> bool {
        !self.bound_name_and_badge || badge_x + badge_width <= self.content_right + 0.01
    }

    /// Width available to the shell path starting at `x`; `0.0` when no
    /// width remains (the painter then skips the shell path).
    fn shell_avail(&self, x: f32) -> f32 {
        (self.content_right - x).max(0.0)
    }
}

/// Color of a row's secondary text (shell path and shortcut label) for
/// the row's state: on-secondary-container on the highlighted row,
/// on-surface-variant otherwise (a hovered row keeps the normal color).
fn shell_color(active: bool) -> egui::Color32 {
    if active {
        md3::state_layer(md3::on_secondary_container(), 0.7)
    } else {
        md3::on_surface_variant()
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
    /// Tmux row at this index into
    /// [`ProfileSelectorState::tmux_entries`] — spawn a tab attached to
    /// that entry's session (or socket, for a fallback entry).
    Tmux(usize),
}

/// One tmux row for the new-tab chooser (task0001): the label text to
/// show and the PTY spawn argv to use if the row is confirmed. Built by
/// the Application layer from `tmux_sockets::enumerate()` via the
/// shared label / attach-argument rules (`tmux_sockets::label` /
/// `tmux_sockets::attach_args`) — this module holds no tmux knowledge
/// beyond rendering the label it is handed, which keeps it reachable on
/// every platform even though `tmux_sockets` itself is Unix-only (the
/// non-Unix stub in `app.rs` simply returns an empty list of these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxRow {
    pub label: String,
    pub argv: Vec<String>,
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
    /// Canonical text of the shortcut that opens this row's target,
    /// drawn right-aligned on the row. `None` draws the row as before.
    pub shortcut: Option<&'a str>,
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
            painter.rect_filled(screen, 0.0, tokens::SCRIM_COLOR);
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
                .shadow(tokens::elevation_shadow())
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
                            // Tmux rows (new-tab chooser mode only):
                            // appended after the profile rows, matching
                            // `row_to_choice`'s `Global -> profiles -> tmux
                            // entries` ordering. Cloned out of `state` up
                            // front so the per-row `&mut state` borrow below
                            // (needed for the highlight/scroll bookkeeping)
                            // does not conflict with iterating the field.
                            let tmux_entries = state.tmux_entries.clone();
                            let base = rows.len();
                            for (i, entry) in tmux_entries.iter().enumerate() {
                                let tmux_row = ProfileRow {
                                    name: &entry.label,
                                    shell_path: "",
                                    is_default: false,
                                    // Tmux rows never carry a shortcut label.
                                    shortcut: None,
                                };
                                if let Some(idx) =
                                    draw_row(ui, state, base + i, &tmux_row, default_badge)
                                {
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
    // The shell path and the shortcut label share one color resolution.
    let shell_color = shell_color(active);

    let name_x = rect.min.x + ROW_PAD_X;
    let cy = rect.center().y;

    // Shortcut label (right-aligned, vertically centered like the shell
    // path). Painted first: the geometry below keeps the name, badge and
    // shell path out of its way.
    let label_galley = row
        .shortcut
        .filter(|text| !text.is_empty())
        .map(|text| painter.layout_no_wrap(text.to_string(), shell_font.clone(), shell_color));
    let geometry = RowGeometry::new(rect.max.x, label_galley.as_ref().map(|g| g.size().x));
    if let (Some(galley), Some((label_left, _))) = (label_galley, geometry.label) {
        painter.galley(
            egui::pos2(label_left, cy - galley.size().y / 2.0),
            galley,
            shell_color,
        );
    }

    // Default badge metrics are needed up front: on a labeled row its
    // width (and the gap before it) is reserved before the name gets the
    // remaining width.
    let badge_galley = row.is_default.then(|| {
        painter.layout_no_wrap(
            default_badge.to_string(),
            badge_font,
            md3::on_primary_container(),
        )
    });
    let badge_w = badge_galley
        .as_ref()
        .map(|g| g.size().x + BADGE_PAD_X * 2.0);

    // Name. A labeled row bounds it at the content boundary and ellipsizes
    // its end (the same single-row width-limited LayoutJob the shell path
    // uses); an unlabeled row paints it at its natural width, as before.
    let natural_name = painter.layout_no_wrap(row.name.to_string(), name_font.clone(), name_color);
    let name_galley = match geometry.name_max_width(name_x, badge_w) {
        Some(max_w) if natural_name.size().x > max_w => (max_w > 0.0).then(|| {
            let mut job = egui::text::LayoutJob::simple_singleline(
                row.name.to_string(),
                name_font,
                name_color,
            );
            job.wrap.max_width = max_w;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            painter.layout_job(job)
        }),
        _ => Some(natural_name),
    };
    let mut x = name_x;
    if let Some(galley) = name_galley {
        let name_pos = egui::pos2(x, cy - galley.size().y / 2.0);
        x += galley.size().x;
        painter.galley(name_pos, galley, name_color);
    }
    x += ROW_INNER_GAP;

    // Default badge (primary-container pill). Never painted past the
    // content boundary of a labeled row.
    if let (Some(badge_galley), Some(badge_w)) = (badge_galley, badge_w) {
        if geometry.badge_fits(x, badge_w) {
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
    }

    // Shell path (truncated at the content boundary: the row's right
    // padding, or the label's left edge minus the inner gap)
    if !row.shell_path.is_empty() {
        let avail = geometry.shell_avail(x);
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
            tmux_entries: Vec::new(),
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
        // Outside chooser mode `num_profiles` is unused (no tmux rows ever
        // show there); pass an arbitrary value to prove that.
        assert_eq!(s.row_to_choice(0, 0), Choice::Profile(0));
        assert_eq!(s.row_to_choice(2, 0), Choice::Profile(2));
        assert_eq!(s.profile_row(2), 2);
    }

    #[test]
    fn row_to_choice_chooser_mode_offsets_global() {
        let mut s = ProfileSelectorState::default();
        s.open_with_global(0); // include_global = true
        let num_profiles = 3;
        assert_eq!(s.row_to_choice(0, num_profiles), Choice::Global);
        assert_eq!(s.row_to_choice(1, num_profiles), Choice::Profile(0));
        assert_eq!(s.row_to_choice(3, num_profiles), Choice::Profile(2));
        // profile_row is the inverse: profiles[2] sits at row 3.
        assert_eq!(s.profile_row(2), 3);
        assert_eq!(
            s.row_to_choice(s.profile_row(2), num_profiles),
            Choice::Profile(2)
        );
    }

    fn tmux_row(label: &str) -> TmuxRow {
        TmuxRow {
            label: label.to_string(),
            argv: Vec::new(),
        }
    }

    // AC-6: combined ordering `Global -> N profiles -> M tmux entries`.
    #[test]
    fn row_to_choice_chooser_mode_appends_tmux_after_profiles() {
        let mut s = ProfileSelectorState::default();
        s.open_with_global(0);
        s.tmux_entries = vec![tmux_row("tmux: dev"), tmux_row("tmux: work")];
        let num_profiles = 2;
        // row 0 = Global, rows 1-2 = the 2 profiles, rows 3-4 = the 2
        // tmux entries.
        assert_eq!(s.row_to_choice(0, num_profiles), Choice::Global);
        assert_eq!(s.row_to_choice(1, num_profiles), Choice::Profile(0));
        assert_eq!(s.row_to_choice(2, num_profiles), Choice::Profile(1));
        assert_eq!(s.row_to_choice(3, num_profiles), Choice::Tmux(0));
        assert_eq!(s.row_to_choice(4, num_profiles), Choice::Tmux(1));
    }

    // AC-6: M = 0 reproduces today's (pre-tmux) chooser-mode behavior.
    #[test]
    fn row_to_choice_zero_tmux_entries_matches_pre_tmux_behavior() {
        let mut s = ProfileSelectorState::default();
        s.open_with_global(0);
        assert!(s.tmux_entries.is_empty());
        let num_profiles = 2;
        assert_eq!(s.row_to_choice(0, num_profiles), Choice::Global);
        assert_eq!(s.row_to_choice(1, num_profiles), Choice::Profile(0));
        assert_eq!(s.row_to_choice(2, num_profiles), Choice::Profile(1));
    }

    // `open`/`open_with_global` must not leak a previous chooser
    // session's tmux list into a fresh session.
    #[test]
    fn open_clears_stale_tmux_entries() {
        let mut s = ProfileSelectorState::default();
        s.open_with_global(0);
        s.tmux_entries = vec![tmux_row("tmux: dev")];
        s.open();
        assert!(s.tmux_entries.is_empty());
    }

    #[test]
    fn open_with_global_clears_stale_tmux_entries() {
        let mut s = ProfileSelectorState::default();
        s.tmux_entries = vec![tmux_row("tmux: dev")];
        s.open_with_global(0);
        assert!(s.tmux_entries.is_empty());
    }

    // ── shortcut labels on the new-tab chooser rows ─────────────────
    //
    // new-tab-menu-shortcut-hints task0001 (FR1-FR3, FR6, FR8, NFR4).
    // The assignment is pure: no egui context, no App.

    use crate::settings::KeybindSettings;
    use crate::ui::keybinds::KeybindTable;

    const GLOBAL_LABEL: &str = "Ctrl+Shift+G";
    const NEW_TAB_LABEL: &str = "Ctrl+Shift+T";

    fn table_with(edit: impl FnOnce(&mut KeybindSettings)) -> KeybindTable {
        let mut kb = KeybindSettings::default();
        edit(&mut kb);
        KeybindTable::from_settings(&kb)
    }

    fn chooser_state(tmux_rows: usize) -> ProfileSelectorState {
        let mut s = ProfileSelectorState::default();
        s.open_with_global(0);
        s.tmux_entries = (0..tmux_rows)
            .map(|i| tmux_row(&format!("tmux: {i}")))
            .collect();
        s
    }

    fn labels(v: &[Option<&str>]) -> Vec<Option<String>> {
        v.iter().map(|l| l.map(str::to_string)).collect()
    }

    // AC-3 (TS-4): profiles [A (default), B] and one tmux row.
    #[test]
    fn shortcut_labels_global_and_default_profile_only() {
        let s = chooser_state(1);
        assert_eq!(
            s.row_shortcut_labels(&[true, false], &KeybindTable::default()),
            labels(&[Some(GLOBAL_LABEL), Some(NEW_TAB_LABEL), None, None])
        );
    }

    // AC-3 (TS-5): no default profile -> only the Global row is labeled.
    #[test]
    fn shortcut_labels_without_default_profile_label_only_global() {
        let s = chooser_state(0);
        assert_eq!(
            s.row_shortcut_labels(&[false, false], &KeybindTable::default()),
            labels(&[Some(GLOBAL_LABEL), None, None])
        );
    }

    // AC-3 (TS-5): two flagged profiles -> only the first carries the label.
    #[test]
    fn shortcut_labels_first_flagged_profile_wins() {
        let s = chooser_state(0);
        assert_eq!(
            s.row_shortcut_labels(&[true, true], &KeybindTable::default()),
            labels(&[Some(GLOBAL_LABEL), Some(NEW_TAB_LABEL), None])
        );
    }

    // AC-3: the label follows the flagged profile's row, wherever it is.
    #[test]
    fn shortcut_labels_follow_a_later_default_profile() {
        let s = chooser_state(2);
        assert_eq!(
            s.row_shortcut_labels(&[false, true, false], &KeybindTable::default()),
            labels(&[
                Some(GLOBAL_LABEL),
                None,
                Some(NEW_TAB_LABEL),
                None,
                None,
                None
            ])
        );
    }

    // AC-3: chooser mode without profiles: Global + tmux rows only.
    #[test]
    fn shortcut_labels_without_profiles_cover_global_and_tmux_rows() {
        let s = chooser_state(1);
        assert_eq!(
            s.row_shortcut_labels(&[], &KeybindTable::default()),
            labels(&[Some(GLOBAL_LABEL), None])
        );
    }

    // AC-3: one entry per row the dialog shows, in row order.
    #[test]
    fn shortcut_labels_have_one_entry_per_row() {
        let s = chooser_state(3);
        let out = s.row_shortcut_labels(&[false, true], &KeybindTable::default());
        assert_eq!(out.len(), 1 + 2 + 3);
    }

    // AC-5 (FR8): selector mode (include_global off) never labels a row,
    // even with a default profile, and lists profiles only.
    #[test]
    fn shortcut_labels_selector_mode_has_no_labels() {
        let mut s = ProfileSelectorState::default();
        s.open();
        assert_eq!(
            s.row_shortcut_labels(&[true, false], &KeybindTable::default()),
            labels(&[None, None])
        );
    }

    // AC-4 (FR6): the Global row loses its label to copy / paste /
    // profile_selector; the default-profile row is unaffected.
    #[test]
    fn shortcut_labels_global_row_hidden_by_higher_priority_actions() {
        for edit in [
            (|kb: &mut KeybindSettings| kb.new_tab_global = kb.copy.clone())
                as fn(&mut KeybindSettings),
            |kb| kb.new_tab_global = kb.paste.clone(),
            |kb| kb.new_tab_global = kb.profile_selector.clone(),
        ] {
            let table = table_with(edit);
            let out = chooser_state(0).row_shortcut_labels(&[true], &table);
            assert_eq!(out[0], None, "Global row hidden");
            assert_eq!(out[1].as_deref(), Some(NEW_TAB_LABEL), "profile row kept");
        }
    }

    // AC-4 (FR6): the default-profile row loses its label to copy / paste
    // / profile_selector / new_tab_global; the Global row is unaffected.
    #[test]
    fn shortcut_labels_default_profile_row_hidden_by_higher_priority_actions() {
        for edit in [
            (|kb: &mut KeybindSettings| kb.new_tab = kb.copy.clone()) as fn(&mut KeybindSettings),
            |kb| kb.new_tab = kb.paste.clone(),
            |kb| kb.new_tab = kb.profile_selector.clone(),
            |kb| kb.new_tab = kb.new_tab_global.clone(),
        ] {
            let table = table_with(edit);
            let out = chooser_state(0).row_shortcut_labels(&[true], &table);
            assert_eq!(out[1], None, "default-profile row hidden");
            assert_eq!(out[0].as_deref(), Some(GLOBAL_LABEL), "Global row kept");
        }
    }

    // AC-4 (FR6): new_tab equal to new_tab_global -> the profile row has
    // no label and the Global row keeps its own.
    #[test]
    fn shortcut_labels_new_tab_equal_to_global_hides_only_the_profile_row() {
        let table = table_with(|kb| kb.new_tab = kb.new_tab_global.clone());
        let out = chooser_state(0).row_shortcut_labels(&[false, true], &table);
        assert_eq!(out, labels(&[Some(GLOBAL_LABEL), None, None]));
    }

    // AC-4 (FR6): a collision with a LOWER-priority action (close_tab)
    // does not hide the default-profile row's label.
    #[test]
    fn shortcut_labels_lower_priority_collision_keeps_the_profile_label() {
        let table = table_with(|kb| kb.new_tab = kb.close_tab.clone());
        let out = chooser_state(0).row_shortcut_labels(&[true], &table);
        assert_eq!(out[1].as_deref(), Some("Ctrl+Shift+W"));
    }

    // ── row geometry (pure) ──────────────────────────────────────────
    //
    // AC-7 (FR7, NFR1): label rectangle, content boundary and shell-path
    // range computed from the row's right edge and the measured label
    // width, with no egui context.

    const ROW_RIGHT: f32 = 400.0;
    const LABEL_W: f32 = 80.0;

    #[test]
    fn geometry_label_right_edge_is_row_right_minus_pad() {
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        let (left, right) = g.label.expect("labeled row has a label span");
        assert_eq!(right, ROW_RIGHT - ROW_PAD_X);
        assert_eq!(left, right - LABEL_W);
    }

    #[test]
    fn geometry_content_boundary_is_inner_gap_before_the_label() {
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        let (label_left, _) = g.label.unwrap();
        assert_eq!(g.content_right, label_left - ROW_INNER_GAP);
    }

    #[test]
    fn geometry_shell_path_range_ends_at_the_content_boundary() {
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        let start = 100.0;
        assert_eq!(g.shell_avail(start), g.content_right - start);
    }

    #[test]
    fn geometry_shell_path_range_is_empty_when_no_width_remains() {
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        // Starting exactly at, or beyond, the boundary leaves nothing.
        assert_eq!(g.shell_avail(g.content_right), 0.0);
        assert_eq!(g.shell_avail(g.content_right + 25.0), 0.0);
    }

    #[test]
    fn geometry_name_is_bounded_at_the_content_boundary() {
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        let name_x = 16.0;
        assert_eq!(
            g.name_max_width(name_x, None),
            Some(g.content_right - name_x)
        );
    }

    #[test]
    fn geometry_badge_width_and_gap_are_reserved_before_the_name() {
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        let name_x = 16.0;
        let badge_w = 60.0;
        assert_eq!(
            g.name_max_width(name_x, Some(badge_w)),
            Some(g.content_right - name_x - ROW_INNER_GAP - badge_w)
        );
    }

    #[test]
    fn geometry_name_width_never_goes_negative() {
        // A very wide label (or badge) leaves the name no room.
        let g = RowGeometry::new(ROW_RIGHT, Some(ROW_RIGHT));
        assert_eq!(g.name_max_width(16.0, None), Some(0.0));
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        assert_eq!(g.name_max_width(16.0, Some(ROW_RIGHT)), Some(0.0));
    }

    #[test]
    fn geometry_badge_must_end_within_the_content_boundary() {
        let g = RowGeometry::new(ROW_RIGHT, Some(LABEL_W));
        let badge_w = 60.0;
        assert!(g.badge_fits(g.content_right - badge_w, badge_w));
        assert!(!g.badge_fits(g.content_right - badge_w + 1.0, badge_w));
    }

    // An unlabeled row keeps today's geometry: the shell path runs to the
    // row's right padding, and name / badge are not bounded at all.
    #[test]
    fn geometry_unlabeled_row_matches_todays_layout() {
        let g = RowGeometry::new(ROW_RIGHT, None);
        assert_eq!(g.label, None);
        assert_eq!(g.content_right, ROW_RIGHT - ROW_PAD_X);
        for x in [100.0_f32, 380.0, 390.0] {
            // Today: `(rect.max.x - ROW_PAD_X - x).max(0.0)`.
            assert_eq!(g.shell_avail(x), (ROW_RIGHT - ROW_PAD_X - x).max(0.0));
        }
        assert_eq!(g.name_max_width(16.0, None), None);
        assert_eq!(g.name_max_width(16.0, Some(60.0)), None);
        assert!(g.badge_fits(ROW_RIGHT * 2.0, 60.0));
    }

    // The label and the shell path share one color resolution.
    #[test]
    fn shell_color_resolution_is_per_row_state() {
        assert_eq!(shell_color(false), md3::on_surface_variant());
        assert_eq!(
            shell_color(true),
            md3::state_layer(md3::on_secondary_container(), 0.7)
        );
    }

    // ── painted rows (headless egui pass) ────────────────────────────
    //
    // Runs the real `draw` in a context without a window and inspects the
    // emitted shapes: the label, name, badge and shell path positions and
    // the label's font size / color. Complements the pure geometry tests.

    fn flatten<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::Shape>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }

    /// Shapes of one frame of the selector dialog, flattened.
    fn paint_selector(
        state: &mut ProfileSelectorState,
        rows: &[ProfileRow<'_>],
    ) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        // The first frame only sizes the egui areas (their content is not
        // emitted) and the areas then fade in. Frames with advancing time
        // run the fade to completion so fills carry their exact colors.
        let mut output = egui::FullOutput::default();
        for frame in 0..4 {
            let mut input = input.clone();
            input.time = Some(f64::from(frame));
            output = ctx.run(input, |ctx| {
                draw(ctx, state, rows, "New Tab", "Default");
            });
        }
        let mut flat = Vec::new();
        for clipped in &output.shapes {
            flatten(&clipped.shape, &mut flat);
        }
        flat.into_iter().cloned().collect()
    }

    /// The text a galley actually paints: its rows' glyphs, which carry
    /// the `…` of an ellipsized single-row job (`Galley::text` is the
    /// source text before any truncation).
    fn painted_text(t: &egui::epaint::TextShape) -> String {
        t.galley.rows.iter().map(|row| row.text()).collect()
    }

    /// The text shape whose painted text is exactly `text`.
    fn text_shape<'a>(shapes: &'a [egui::Shape], text: &str) -> &'a egui::epaint::TextShape {
        shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if painted_text(t) == text => Some(t),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no painted text {text:?}"))
    }

    fn has_text(shapes: &[egui::Shape], text: &str) -> bool {
        shapes
            .iter()
            .any(|s| matches!(s, egui::Shape::Text(t) if painted_text(t) == text))
    }

    /// Right edge of the highlighted row (the filled `secondary_container`
    /// rect), and the row's x span.
    fn active_row_rect(shapes: &[egui::Shape]) -> egui::Rect {
        shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r) if r.fill == md3::secondary_container() => Some(r.rect),
                _ => None,
            })
            .expect("highlighted row rect")
    }

    fn chooser_selected(row: usize) -> ProfileSelectorState {
        let mut s = ProfileSelectorState::default();
        s.open_with_global(row);
        s
    }

    fn row<'a>(
        name: &'a str,
        shell_path: &'a str,
        is_default: bool,
        shortcut: Option<&'a str>,
    ) -> ProfileRow<'a> {
        ProfileRow {
            name,
            shell_path,
            is_default,
            shortcut,
        }
    }

    // AC-7: the label sits at the row's right edge minus ROW_PAD_X, uses
    // the shell font size and the shell path's color for the row state.
    #[test]
    fn painted_label_is_right_aligned_with_shell_font_and_color() {
        let mut state = chooser_selected(0);
        let rows = [row("Global Settings", "", false, Some("Ctrl+Shift+G"))];
        let shapes = paint_selector(&mut state, &rows);
        let rect = active_row_rect(&shapes);
        let label = text_shape(&shapes, "Ctrl+Shift+G");
        assert!(
            (label.pos.x + label.galley.size().x - (rect.max.x - ROW_PAD_X)).abs() < 0.01,
            "label right edge {} vs row right {} - pad",
            label.pos.x + label.galley.size().x,
            rect.max.x
        );
        let format = &label.galley.job.sections[0].format;
        assert_eq!(format.font_id.size, SHELL_FONT);
        assert_eq!(format.color, shell_color(true), "highlighted row color");
        // Vertically centered on the row like the shell path.
        let center = label.pos.y + label.galley.size().y / 2.0;
        assert!((center - rect.center().y).abs() < 0.01);
    }

    #[test]
    fn painted_label_uses_the_normal_shell_color_on_a_normal_row() {
        // Row 1 is highlighted; the labeled Global row (row 0) is normal.
        let mut state = chooser_selected(1);
        let rows = [
            row("Global Settings", "", false, Some("Ctrl+Shift+G")),
            row("a", "/bin/zsh", true, None),
        ];
        let shapes = paint_selector(&mut state, &rows);
        let label = text_shape(&shapes, "Ctrl+Shift+G");
        assert_eq!(
            label.galley.job.sections[0].format.color,
            shell_color(false)
        );
    }

    // AC-7 (FR7): a long name is ellipsized; neither it nor the badge
    // paint under the label, and the shell path is not drawn when no
    // width remains.
    #[test]
    fn painted_long_name_and_badge_stay_left_of_the_label() {
        let mut state = chooser_selected(0);
        let long =
            "a-very-long-profile-name-that-would-run-under-the-label-if-unbounded-xxxxxxxxxx";
        let rows = [row(long, "/usr/bin/zsh", true, Some("Ctrl+Shift+T"))];
        let shapes = paint_selector(&mut state, &rows);
        let rect = active_row_rect(&shapes);
        let label = text_shape(&shapes, "Ctrl+Shift+T");
        let boundary = label.pos.x - ROW_INNER_GAP;

        // The full name is not painted; an ellipsized one is, ending "…".
        assert!(!has_text(&shapes, long));
        let name = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if painted_text(t).starts_with("a-very-long") => Some(t),
                _ => None,
            })
            .expect("ellipsized name");
        assert!(
            painted_text(name).ends_with('…'),
            "{:?}",
            painted_text(name)
        );

        // The badge pill (primary_container fill) ends before the label.
        let badge = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r) if r.fill == md3::primary_container() => Some(r.rect),
                _ => None,
            })
            .expect("badge");
        assert!(
            badge.max.x <= boundary + 0.01,
            "badge {badge:?} vs {boundary}"
        );
        assert!(name.pos.x + name.galley.size().x <= badge.min.x);
        // No width remains for the shell path.
        assert!(!has_text(&shapes, "/usr/bin/zsh"));
        // Everything stays inside the row.
        assert!(rect.min.x <= name.pos.x);
    }

    // AC-7 (FR7): the shell path ends ROW_INNER_GAP before the label.
    #[test]
    fn painted_shell_path_ends_before_the_label() {
        let mut state = chooser_selected(0);
        let shell = "/a/rather/long/shell/path/that/needs/truncating/at/the/label/boundary/bin/zsh";
        let rows = [row("a", shell, false, Some("Ctrl+Shift+T"))];
        let shapes = paint_selector(&mut state, &rows);
        let label = text_shape(&shapes, "Ctrl+Shift+T");
        let shell_shape = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if painted_text(t).starts_with("/a/rather") => Some(t),
                _ => None,
            })
            .expect("shell path");
        assert!(
            shell_shape.pos.x + shell_shape.galley.size().x <= label.pos.x - ROW_INNER_GAP + 0.01
        );
    }

    // Rows without a label are painted as before: the whole name and the
    // shell path are drawn, and no label text exists.
    #[test]
    fn painted_unlabeled_row_is_unchanged() {
        let mut state = chooser_selected(0);
        let rows = [row("work", "/bin/zsh", true, None)];
        let shapes = paint_selector(&mut state, &rows);
        assert!(has_text(&shapes, "work"));
        assert!(has_text(&shapes, "/bin/zsh"));
        assert!(has_text(&shapes, "Default"));
        assert!(!has_text(&shapes, "Ctrl+Shift+T"));
    }

    // A short name with a label keeps name, badge and shell path whole.
    #[test]
    fn painted_short_name_with_label_is_not_ellipsized() {
        let mut state = chooser_selected(0);
        let rows = [row("work", "/bin/zsh", true, Some("Ctrl+Shift+T"))];
        let shapes = paint_selector(&mut state, &rows);
        assert!(has_text(&shapes, "work"));
        assert!(has_text(&shapes, "/bin/zsh"));
        assert!(has_text(&shapes, "Default"));
        assert!(has_text(&shapes, "Ctrl+Shift+T"));
    }
}
