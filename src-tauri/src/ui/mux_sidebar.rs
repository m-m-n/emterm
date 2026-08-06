//! Vertical tab sidebar widget (task0004).
//!
//! Draws the mux window list (number + name + active mark) for the two
//! placement variants pinned in `IMPLEMENTATION.md :: Shared Components ::
//! Sidebar widget ui::mux_sidebar`:
//!
//! - [`Placement::Persistent`] — a right [`egui::SidePanel`], `surface
//!   container low` background, 1 px `outline_variant` separator on the
//!   terminal-facing (left) edge. Reserves grid WIDTH only — the terminal
//!   grid's x-origin is identical with and without this panel showing
//!   (2026-07-20 right-edge placement update; see IMPLEMENTATION.md
//!   cross-task decision 2).
//! - [`Placement::Overlay`] — a floating card: a right-edge
//!   [`egui::Area`] drawn over the terminal (no grid geometry impact),
//!   inset by a uniform 16 px (`spacing-md`) from the terminal area's
//!   top/right/bottom edges, 12 px corner radius (`corner-medium`) on all
//!   four corners, `surface_container_high` background at 92% alpha
//!   (slight translucency), NO separator line (the rounded edge is the
//!   only boundary), elevation-3 shadow (reusing
//!   [`crate::ui::dialog::tokens::elevation_shadow`] — the same token the
//!   modal dialogs use, so no new shadow color is introduced here). No
//!   scrim, no open/close animation. (2026-07-20 floating-card reshape;
//!   see `IMPLEMENTATION.md` cross-task decision 3 update, task0007.)
//!
//! Contract (pinned in `IMPLEMENTATION.md`): input is an ordered entry list
//! (window index, display name, active flag) plus a placement variant;
//! output is at most one clicked window index. The widget never talks to
//! the daemon — task0005 routes the result into the existing
//! `TabEvent::MuxSwitch` path. Wiring into the frame (grid inset, overlay
//! open/close state, visibility rules) is out of scope for this module.
//!
//! Follows `ui/tab_bar.rs` module conventions: small view-model structs +
//! pure draw functions, colors exclusively via [`crate::ui::md3`]
//! accessors (AC-5).

use egui::{Color32, FontId, Pos2, Rect, Rounding, ScrollArea, Sense, Stroke, Ui, Vec2};

use crate::agent_status_model::Aggregated;

use super::emoji_cache::EmojiResources;
use super::md3;
use super::tab_bar::{AGENT_BADGE_SLOT_WIDTH, paint_agent_badge};

// ── width formula (Shared Components: "Sidebar width function") ─────────

/// Lower clamp bound of [`sidebar_width`], in egui logical points.
const MIN_WIDTH: f32 = 180.0;
/// Upper clamp bound of [`sidebar_width`].
const MAX_WIDTH: f32 = 320.0;
/// Fraction of the window's inner width the sidebar targets before
/// clamping.
const WIDTH_FRACTION: f32 = 0.22;

/// The one width formula shared by both placement variants (task0005 also
/// uses this for the persistent-mode grid inset). Deterministic, no state.
pub fn sidebar_width(window_inner_width: f32) -> f32 {
    (window_inner_width * WIDTH_FRACTION).clamp(MIN_WIDTH, MAX_WIDTH)
}

// ── placement ─────────────────────────────────────────────────────────

/// Which surface the sidebar renders as. Selection is data-driven
/// (`settings.mux.window_sidebar_overlay` + the runtime overlay-open flag)
/// at the task0005 call site; this module only draws the variant it is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Right `SidePanel` that participates in layout (task0005 insets the
    /// terminal grid's usable WIDTH by [`sidebar_width`] while this is
    /// showing; the grid's x-origin is unaffected — task0006 right-edge
    /// placement update).
    Persistent,
    /// Right-edge `Area` drawn after the central panel; contributes no
    /// grid inset.
    Overlay,
}

// ── hit-region geometry (task0010: winit-side wheel routing) ───────────
//
// The winit `MouseWheel` handler (`window_host.rs`) has no egui context to
// query `ctx.available_rect()` from, so the sidebar's region is expressed
// here as pure functions of quantities the handler already tracks (window
// size, title/tab bar heights, the status bar's bottom inset). `draw_overlay`
// calls [`overlay_card_rect`] itself (below) rather than duplicating its
// math, so this section and the paint path share ONE derivation
// (IMPLEMENTATION.md cross-task decision 3.5) — a manual, independently
// re-derived winit-side guard is exactly the defect class the round-2
// scrollbar click-guard regression came from.

/// Vertical space the CSD title bar + tab strip reserve above the terminal
/// area, in logical px. Mirrors the two `TopBottomPanel::top` panels
/// `render::draw_terminal` shows before the sidebar (title bar, then tab
/// bar) — the same two functions `window_host::cell_metrics_px`'s
/// `origin_y` and the existing tab-bar-strip wheel/click guards read.
pub fn top_chrome_inset(show_tab_bar: bool) -> f32 {
    super::title_bar::TITLE_BAR_HEIGHT + super::tab_bar::effective_tab_bar_height(show_tab_bar)
}

/// The persistent panel's own screen rect (task0006 right-edge placement).
/// Spans the FULL window height below `top_chrome` — the status bar's
/// height must NOT be subtracted here: `render::draw_terminal` shows the
/// persistent `SidePanel` BEFORE the status-bar panel, so egui claims the
/// sidebar's vertical extent first and the status bar lays out only in the
/// narrower central column that remains (pinned by
/// `tests::ac4_persistent_hit_region_matches_the_real_panel_rect_from_the_frame_composition_order`,
/// which runs the real panel order and compares against this formula).
/// Horizontal span is exactly `width` (the shared [`sidebar_width`] value),
/// flush against the window's right edge.
pub fn persistent_panel_rect(window_size: Vec2, top_chrome: f32, width: f32) -> Rect {
    Rect::from_min_max(
        egui::pos2(window_size.x - width, top_chrome),
        egui::pos2(window_size.x, window_size.y),
    )
}

/// The terminal area the overlay card anchors to — mirrors
/// `ctx.available_rect()` at the point [`draw_overlay`] runs: title bar +
/// tab bar reserved on top, the status bar's height reserved at the
/// bottom. `CentralPanel` does not shrink `available_rect` further (egui's
/// `pass_state::allocate_central_panel` is a no-op on it), so this is also
/// exactly the central panel's rect.
pub fn terminal_area_rect(window_size: Vec2, top_chrome: f32, bottom_chrome: f32) -> Rect {
    Rect::from_min_max(
        egui::pos2(0.0, top_chrome),
        egui::pos2(window_size.x, window_size.y - bottom_chrome),
    )
}

/// The overlay card's own rect, given the terminal area it anchors to.
/// SHARED by [`draw_overlay`] (the paint path) and [`point_in_sidebar`]
/// (the hit-region path) — the single derivation IMPLEMENTATION.md
/// cross-task decision 3.5 requires. See [`draw_overlay`]'s doc comment
/// for the margin/width reasoning.
pub fn overlay_card_rect(terminal_area: Rect, width: f32) -> Rect {
    Rect::from_min_size(
        egui::pos2(
            terminal_area.right() - OVERLAY_MARGIN - width,
            terminal_area.top() + OVERLAY_MARGIN,
        ),
        Vec2::new(width, terminal_area.height() - 2.0 * OVERLAY_MARGIN),
    )
}

/// AC-1/AC-2/AC-3: whether `point` (logical px, window-relative) lies
/// inside the currently-visible sidebar's region. `visible_placement` is
/// `None` when the sidebar is hidden (local tab, or overlay mode with the
/// runtime flag closed) — always `false` then (AC-3 no-op). Otherwise
/// `true` exactly inside the persistent panel strip
/// ([`persistent_panel_rect`]) or the overlay card ([`overlay_card_rect`])
/// for the respective placement, `false` elsewhere. `top_chrome` /
/// `bottom_chrome` are logical-px insets the caller already tracks
/// (title+tab bar height, status bar bottom inset).
pub fn point_in_sidebar(
    point: Pos2,
    visible_placement: Option<Placement>,
    window_size: Vec2,
    top_chrome: f32,
    bottom_chrome: f32,
) -> bool {
    let Some(placement) = visible_placement else {
        return false;
    };
    let width = sidebar_width(window_size.x);
    let rect = match placement {
        Placement::Persistent => persistent_panel_rect(window_size, top_chrome, width),
        Placement::Overlay => overlay_card_rect(
            terminal_area_rect(window_size, top_chrome, bottom_chrome),
            width,
        ),
    };
    rect.contains(point)
}

// ── view-model ────────────────────────────────────────────────────────

/// One row in the sidebar: a window's position, display name, and whether
/// it is the group's active window. Order matches
/// [`crate::mux::window_group::MuxWindowGroup::windows`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarEntry {
    /// Window position (0-based) within the group; the click target.
    pub window_index: usize,
    /// Display name shown in the row.
    pub name: String,
    /// Whether this is the active window (the only visual "active" mark —
    /// row background + text color).
    pub active: bool,
    /// The window's pane id (wire `u32`; one pane per window entry in this
    /// app's mux model). Keys the badge / public-ID lookups below
    /// (task0006).
    pub pane_id: u32,
    /// task0006 AC-1/AC-2: this pane's aggregated agent-status badge.
    /// `None` when the pane has never reported a state — no badge renders
    /// and no layout space is reserved for it. Attached by the caller
    /// (the render pipeline reads `App::agent_status`; this module stays
    /// free of `App`); [`build_entries`] always leaves this `None`.
    pub badge: Option<Aggregated>,
}

/// Build the ordered entry list from a tab's mux window group. Preserves
/// order, numbering, and names; marks exactly the active window (AC-2).
/// An empty group yields an empty list. `badge` is left `None` here (pure
/// over the group alone) — the caller attaches it from `App::agent_status`
/// before drawing.
pub fn build_entries(group: &crate::mux::window_group::MuxWindowGroup) -> Vec<SidebarEntry> {
    let active = group.active_index();
    let pane_ids = group.pane_ids();
    group
        .windows()
        .iter()
        .enumerate()
        .map(|(i, w)| SidebarEntry {
            window_index: i,
            name: w.name.clone(),
            active: i == active,
            pane_id: pane_ids.get(i).copied().unwrap_or(0),
            badge: None,
        })
        .collect()
}

// ── row geometry (design decisions restated as constants) ──────────────

/// Row height — 40 px full-radius pill.
const ROW_HEIGHT: f32 = 40.0;
/// Horizontal padding inside each row.
const ROW_HORIZONTAL_PAD: f32 = 12.0;
/// Vertical gap between rows.
const ROW_GAP: f32 = 4.0;
/// Gap between the number column and the name.
const NUMBER_NAME_GAP: f32 = 8.0;
/// Panel padding, vertical (top/bottom).
const PANEL_PAD_VERTICAL: f32 = 12.0;
/// Panel padding, horizontal (left/right).
const PANEL_PAD_HORIZONTAL: f32 = 8.0;
/// Number label font size.
const NUMBER_FONT_SIZE: f32 = 12.0;
/// Name label font size.
const NAME_FONT_SIZE: f32 = 14.0;
/// Fixed narrow column width the number right-aligns into.
const NUMBER_COLUMN_WIDTH: f32 = 20.0;
/// Separator hairline width (the persistent variant's terminal-facing left
/// edge only; the overlay variant paints no separator — AC-3).
const SEPARATOR_WIDTH: f32 = 1.0;

// ── agent-status badge (task0006; slot unified in task0001) ─────────────

/// Gap between the badge slot and the name that follows it.
const BADGE_GAP: f32 = 6.0;

// ── overlay floating-card geometry (task0007) ───────────────────────────

/// Uniform margin (`spacing-md`) from the terminal area's top/right/bottom
/// edges the overlay card is inset by.
const OVERLAY_MARGIN: f32 = 16.0;
/// Overlay card corner radius (`corner-medium`), all four corners.
const OVERLAY_CORNER_RADIUS: f32 = 12.0;
/// Overlay card background alpha, applied to `surface_container_high`'s
/// alpha channel (slight translucency; list text stays readable).
const OVERLAY_FILL_ALPHA: f32 = 0.92;

// ── draw ─────────────────────────────────────────────────────────────

/// Per-frame result of [`draw`]: at most one window switch, reported when
/// a row body is clicked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarOutcome {
    /// The clicked entry's window index, if a row body was clicked.
    pub switch_to_window: Option<usize>,
}

/// Draw the sidebar for the given placement, returning this frame's
/// [`SidebarOutcome`]. `opacity` (task0002 FR6/FR10) is the whole-card
/// multiplier the caller (`render::draw_terminal`, reading
/// `App::resolve_mux_sidebar_opacity`) resolved this frame — the
/// `Persistent` variant ignores it entirely (always fully opaque, AC-8);
/// only `draw_overlay` applies it.
pub fn draw(
    ctx: &egui::Context,
    entries: &[SidebarEntry],
    placement: Placement,
    width: f32,
    opacity: f32,
    emoji: Option<&EmojiResources<'_>>,
) -> SidebarOutcome {
    match placement {
        Placement::Persistent => draw_persistent(ctx, entries, width, emoji),
        Placement::Overlay => draw_overlay(ctx, entries, width, opacity, emoji),
    }
}

/// Persistent variant: a right `SidePanel` that participates in layout.
/// `surface_container_low` background, 1 px `outline_variant` separator on
/// the terminal-facing (left) edge. `inner_margin` stays zero on the
/// `Frame` itself (mirrors `tab_bar`'s convention) so `ui.max_rect()`
/// inside the closure is the panel's full (pre-padding) rect; the 12/8 px
/// panel padding is then applied manually before laying out rows.
fn draw_persistent(
    ctx: &egui::Context,
    entries: &[SidebarEntry],
    width: f32,
    emoji: Option<&EmojiResources<'_>>,
) -> SidebarOutcome {
    let mut outcome = SidebarOutcome::default();
    let frame = egui::Frame::none()
        .fill(md3::surface_container_low())
        .inner_margin(egui::Margin::ZERO);
    egui::SidePanel::right("mux-sidebar-persistent")
        .frame(frame)
        .exact_width(width)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let panel_rect = ui.max_rect();
            #[cfg(test)]
            tests::LAST_PERSISTENT_PANEL_RECT.with(|c| *c.borrow_mut() = Some(panel_rect));
            ui.painter().vline(
                panel_rect.left() + SEPARATOR_WIDTH / 2.0,
                panel_rect.top()..=panel_rect.bottom(),
                Stroke::new(SEPARATOR_WIDTH, md3::outline_variant()),
            );
            let content_rect =
                panel_rect.shrink2(Vec2::new(PANEL_PAD_HORIZONTAL, PANEL_PAD_VERTICAL));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                outcome = draw_rows(ui, entries, emoji);
            });
        });
    outcome
}

/// Overlay variant: a floating card. A right-edge `Area` drawn after the
/// central panel (`Order::Foreground`), inset by [`OVERLAY_MARGIN`] from
/// the terminal area's top/right/bottom edges (its width is exactly the
/// caller-supplied `width` — the shared width function's value — so the
/// card's left edge is `width` inward of its right edge, not independently
/// inset). Contributes no grid inset — task0005 draws it without touching
/// the terminal grid geometry. [`OVERLAY_CORNER_RADIUS`] corner radius on
/// all four corners, `surface_container_high` background at
/// [`OVERLAY_FILL_ALPHA`] alpha (slight translucency), NO separator line
/// (AC-3 — the card's rounded edge is the only boundary; the persistent
/// variant's left-edge separator is unchanged), elevation-3 shadow reused
/// from the shared dialog token. No scrim, no open/close animation.
fn draw_overlay(
    ctx: &egui::Context,
    entries: &[SidebarEntry],
    width: f32,
    opacity: f32,
    emoji: Option<&EmojiResources<'_>>,
) -> SidebarOutcome {
    let mut outcome = SidebarOutcome::default();
    // Use the remaining central-panel area (post title-bar / tab-bar /
    // status-bar `TopBottomPanel`s), not the full window `screen_rect()`,
    // so the card's margins are measured from the terminal-facing region
    // and it never covers the titlebar's minimize/maximize/close buttons
    // or the tab/status bars.
    let terminal_area = ctx.available_rect();
    // task0010: the rect computation is factored into `overlay_card_rect`
    // so the winit-side hit-region helper (`point_in_sidebar`) shares this
    // exact derivation instead of re-deriving its own numbers.
    let rect = overlay_card_rect(terminal_area, width);
    let fill = md3::state_layer(md3::surface_container_high(), OVERLAY_FILL_ALPHA);

    egui::Area::new(egui::Id::new("mux-sidebar-overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .show(ctx, |ui| {
            // task0002 FR6/D2: the toolkit's own whole-widget opacity
            // facility (`Ui::set_opacity`), applied once at the card's
            // container level — `Painter::add`/`set` multiply every shape
            // (fill, shadow, text, badges, icons) painted through a
            // painter cloned from this `ui` by `opacity` afterward
            // (`Ui::new_child` / `Frame::begin`/`end` all clone the
            // parent's painter, so this single call covers the whole
            // subtree below). At `opacity == 1.0` this is a no-op
            // (`Painter::add`/`set` only transform when `opacity_factor <
            // 1.0`), so the bright-state appearance stays byte-identical
            // to before this feature (AC-8). Deliberately NOT hand-
            // multiplying individual colors — this module must not gain
            // raw color constructors (AC-10 / NFR2).
            ui.set_opacity(opacity);
            // `egui::Area` caches this id's geometry in per-frame-crossing
            // `AreaState` and only ever consults `default_size` on the very
            // first ("sizing pass") frame — from the second frame on,
            // `state.size.get_or_insert_with(..)` (egui::containers::area::
            // Area::begin) returns the PREVIOUS frame's cached size
            // unconditionally, ignoring `default_size` entirely. And
            // `Ui::set_min_size` "can't shrink the ui, only make it
            // larger" (its own doc comment) — so on a window SHRINK, the
            // outer `ui` handed to this closure keeps the old, now-too-
            // large `max_rect` no matter what we do to it. Neither
            // mechanism can ever make the painted geometry track a
            // shrinking window (task0009: the per-frame `rect` computed
            // above must be authoritative over both, in both directions).
            //
            // Escape both by allocating a BRAND-NEW child `Ui` pinned
            // exactly to the freshly computed `rect` every frame — a new
            // `Ui` gets a fresh `Placer` built directly from the `max_rect`
            // we hand it (no history, no grow-only accumulation), so
            // everything laid out inside is authoritatively sized off THIS
            // frame's `rect`, never a stale cached one.
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                // Still register `rect` as this Ui's own minimum size so
                // it bubbles back up (`Ui::allocate_new_ui` advances the
                // PARENT's placer by the child's `min_rect`) into what the
                // `Area` caches as `AreaState.size` for hit-testing next
                // frame — irrelevant to the painted geometry (which no
                // longer depends on that cache) but keeps click/hover
                // routing consistent with the visible card even on the
                // very next frame before this override runs again.
                ui.set_min_size(rect.size());

                let frame = egui::Frame::none()
                    .fill(fill)
                    .rounding(Rounding::same(OVERLAY_CORNER_RADIUS))
                    .inner_margin(egui::Margin::ZERO)
                    .shadow(crate::ui::dialog::tokens::elevation_shadow());

                // `Frame::show` is not used here: `egui::containers::frame::
                // Prepared::paint` paints `content_ui.min_rect() +
                // inner_margin` — the CONTENT's actual bounding box, not the
                // rect `panel_rect` below intends. Since the row list is
                // manually inset by the panel padding before being laid out,
                // `content_ui.min_rect()` would otherwise collapse to that
                // padded-in (smaller) rect, so the background silently
                // shrinks to hug the rows on the right/bottom edges — the
                // defect task0008 fixes (IMPLEMENTATION.md cross-task
                // decision 3, 2026-07-20 update 2: the computed card rect is
                // the sole authority for the painted background).
                let mut prepared = frame.begin(ui);

                // Pin the frame's own content ui to its full intended size
                // BEFORE laying out the smaller, padding-shrunk row content,
                // so `content_ui.min_rect()` cannot end up equal to just the
                // padded content region.
                let panel_rect = prepared.content_ui.max_rect();
                prepared.content_ui.set_min_size(panel_rect.size());

                let content_rect =
                    panel_rect.shrink2(Vec2::new(PANEL_PAD_HORIZONTAL, PANEL_PAD_VERTICAL));
                prepared.content_ui.allocate_new_ui(
                    egui::UiBuilder::new().max_rect(content_rect),
                    |ui| {
                        outcome = draw_rows(ui, entries, emoji);
                    },
                );

                #[cfg(test)]
                {
                    // Mirrors `Prepared::paint`'s private `paint_rect`
                    // computation exactly, so the test hook asserts the rect
                    // the background is ACTUALLY painted with — not a
                    // precomputed copy (the escaped-review defect class: the
                    // task0007 hook recorded the intended rect, which stayed
                    // "correct" even while the real paint silently
                    // disagreed).
                    let painted_rect = prepared.content_ui.min_rect() + prepared.frame.inner_margin;
                    tests::LAST_OVERLAY_CARD.with(|c| {
                        *c.borrow_mut() = Some(tests::OverlayCardDebug {
                            rect: painted_rect,
                            content_rect,
                            fill,
                            rounding: OVERLAY_CORNER_RADIUS,
                        });
                    });
                }

                prepared.end(ui);
            });
        });
    outcome
}

/// Draw the scrollable row list into `ui` (already positioned/sized to the
/// panel's content area by the caller). Rows never shrink (AC-4); overflow
/// scrolls vertically — an empty list draws nothing (bare panel, no
/// placeholder text).
fn draw_rows(
    ui: &mut Ui,
    entries: &[SidebarEntry],
    emoji: Option<&EmojiResources<'_>>,
) -> SidebarOutcome {
    let mut outcome = SidebarOutcome::default();
    #[cfg(test)]
    tests::LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());

    ui.spacing_mut().item_spacing = Vec2::new(0.0, ROW_GAP);
    ScrollArea::vertical()
        .id_salt("mux-sidebar-rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, ROW_GAP);
            for entry in entries {
                let row_w = ui.available_width();
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(row_w, ROW_HEIGHT), Sense::click());

                #[cfg(test)]
                tests::LAST_ROW_RECTS.with(|c| c.borrow_mut().push(rect));

                let (bg, fg) = row_colors(entry, resp.hovered());
                if let Some(bg) = bg {
                    ui.painter()
                        .rect_filled(rect, Rounding::same(ROW_HEIGHT / 2.0), bg);
                }
                paint_row_content(ui, rect, entry, fg, emoji);

                if resp.clicked() && outcome.switch_to_window.is_none() {
                    outcome.switch_to_window = Some(entry.window_index);
                }
            }
        });
    outcome
}

/// Background / foreground color pair for a row, per the design decisions:
/// active → `secondary_container` / `on_secondary_container` (the only
/// active mark); inactive + hovered → 8% `on_surface` state layer over
/// `on_surface_variant` text; inactive + not hovered → no fill,
/// `on_surface_variant` text.
fn row_colors(entry: &SidebarEntry, hovered: bool) -> (Option<Color32>, Color32) {
    if entry.active {
        (
            Some(md3::secondary_container()),
            md3::on_secondary_container(),
        )
    } else if hovered {
        (
            Some(md3::state_layer(md3::on_surface(), md3::STATE_LAYER_HOVER)),
            md3::on_surface_variant(),
        )
    } else {
        (None, md3::on_surface_variant())
    }
}

/// Paint the `[number] [badge] name` content of one row: the number
/// right-aligned in a fixed narrow column, an optional agent-status badge
/// (task0006 AC-1/AC-2 — reserves space only when present; task0001
/// widens the reserved slot to [`AGENT_BADGE_SLOT_WIDTH`] and adds the
/// emoji-capable painting shared with `ui::tab_bar`), then the name
/// ellipsized to the remaining width, which ends at the row's own right
/// padding.
fn paint_row_content(
    ui: &Ui,
    rect: Rect,
    entry: &SidebarEntry,
    color: Color32,
    emoji: Option<&EmojiResources<'_>>,
) {
    let number_font = FontId::proportional(NUMBER_FONT_SIZE);
    let name_font = FontId::proportional(NAME_FONT_SIZE);

    let number_text = (entry.window_index + 1).to_string();
    let number_col_left = rect.left() + ROW_HORIZONTAL_PAD;
    let number_col_right = number_col_left + NUMBER_COLUMN_WIDTH;
    let number_galley = ui.fonts(|f| f.layout_no_wrap(number_text, number_font, color));
    let number_pos = egui::pos2(
        number_col_right - number_galley.size().x,
        rect.center().y - number_galley.size().y / 2.0,
    );
    ui.painter().galley(number_pos, number_galley, color);

    let mut name_left = number_col_right + NUMBER_NAME_GAP;
    if let Some(badge) = entry.badge {
        let badge_center = egui::pos2(name_left + AGENT_BADGE_SLOT_WIDTH / 2.0, rect.center().y);
        // task0001 AC-4: the identical shared painter the tab bar uses —
        // same glyph, box size, slot width, gap, and fallback rule
        // (NFR1), consuming the ONE decision function pair from
        // `ui::tab_bar` rather than a parallel reimplementation.
        paint_agent_badge(ui, badge_center, badge, emoji);
        name_left += AGENT_BADGE_SLOT_WIDTH + BADGE_GAP;
    }

    let name_right = rect.right() - ROW_HORIZONTAL_PAD;
    let max_w = (name_right - name_left).max(0.0);
    let name_galley = ui.fonts(|f| layout_ellipsized(f, &entry.name, &name_font, color, max_w));
    let name_pos = egui::pos2(name_left, rect.center().y - name_galley.size().y / 2.0);
    ui.painter().galley(name_pos, name_galley, color);
}

/// Lay out `text` in `font_id` / `color`, ellipsizing with `…` when it
/// overflows `max_w`. Mirrors `tab_bar::layout_ellipsized` (kept local to
/// this module — the two widgets are independent per the task scope).
fn layout_ellipsized(
    fonts: &egui::text::Fonts,
    text: &str,
    font_id: &FontId,
    color: Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let full = fonts.layout_no_wrap(text.to_string(), font_id.clone(), color);
    if full.size().x <= max_w || text.is_empty() {
        return full;
    }
    let ell = "…";
    let char_offsets: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut lo = 0usize;
    let mut hi = char_offsets.len().saturating_sub(2);
    let mut best: Option<std::sync::Arc<egui::Galley>> = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = &text[..char_offsets[mid]];
        let g = fonts.layout_no_wrap(format!("{candidate}{ell}"), font_id.clone(), color);
        if g.size().x <= max_w {
            best = Some(g);
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    best.unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::window_group::{MuxWindow, MuxWindowGroup};
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
    use std::cell::RefCell;

    thread_local! {
        pub(super) static LAST_ROW_RECTS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
        pub(super) static LAST_OVERLAY_CARD: RefCell<Option<OverlayCardDebug>> =
            const { RefCell::new(None) };
        /// task0010 AC-4: the persistent `SidePanel`'s own rect
        /// (`ui.max_rect()`, pre-padding), recorded by `draw_persistent`
        /// so tests can compare the hit-region helper's output against the
        /// REAL panel rect egui laid out — not a re-derived assumption.
        pub(super) static LAST_PERSISTENT_PANEL_RECT: RefCell<Option<Rect>> =
            const { RefCell::new(None) };
    }

    /// Test-only snapshot of the overlay card's ACTUALLY-PAINTED geometry
    /// (`rect` mirrors `Prepared::paint`'s own computation, not a
    /// precomputed copy) plus the row content region derived from it
    /// (`content_rect`), recorded by `draw_overlay` (AC-1, AC-2, AC-3).
    #[derive(Debug, Clone, Copy)]
    pub(super) struct OverlayCardDebug {
        pub rect: Rect,
        pub content_rect: Rect,
        pub fill: Color32,
        pub rounding: f32,
    }

    fn win(id: u32, name: &str) -> MuxWindow {
        MuxWindow {
            id,
            name: name.to_string(),
        }
    }

    fn group_with(n: usize, active: usize) -> MuxWindowGroup {
        let mut g = MuxWindowGroup::new();
        let windows: Vec<MuxWindow> = (0..n).map(|i| win(i as u32, &format!("w{i}"))).collect();
        let panes: Vec<u32> = (0..n).map(|i| 100 + i as u32).collect();
        g.seed(windows, panes, active);
        g
    }

    fn entries(n: usize, active: usize) -> Vec<SidebarEntry> {
        (0..n)
            .map(|i| SidebarEntry {
                window_index: i,
                name: format!("w{i}"),
                active: i == active,
                pane_id: 100 + i as u32,
                badge: None,
            })
            .collect()
    }

    fn screen_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    fn row_rects(items: &[SidebarEntry], placement: Placement) -> Vec<Rect> {
        let ctx = egui::Context::default();
        // `egui::Area` (the overlay variant) runs an invisible "sizing
        // pass" on its very first frame and only settles into its final
        // geometry from the second frame on (the same reason
        // `run_with_click` below drives two frames before the click).
        // Prime that pass here too so the rects this returns match what a
        // later interactive frame — like `run_with_click`'s — will use.
        let mut priming = RawInput::default();
        priming.screen_rect = Some(screen_rect());
        let _ = ctx.run(priming, |ctx| {
            let _ = draw(ctx, items, placement, MIN_WIDTH, 1.0, None);
        });

        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, placement, MIN_WIDTH, 1.0, None);
        });
        LAST_ROW_RECTS.with(|c| c.borrow().clone())
    }

    fn run_with_click(
        items: &[SidebarEntry],
        placement: Placement,
        click_pos: Pos2,
    ) -> SidebarOutcome {
        let ctx = egui::Context::default();

        // egui's per-widget click/hover resolution for frame N is computed
        // from the widget rects *registered in frame N-1* (immediate-mode's
        // one-frame interaction delay — see `egui::interaction::interact`).
        // The overlay's `egui::Area` additionally runs an invisible
        // "sizing pass" on its very first-ever frame (before any
        // `AreaState` is in memory), during which its rows are not
        // registered as interactive. Two pointer-move-only priming frames
        // get past both effects — (1) the sizing pass, (2) a settled frame
        // whose widget rects are interactive — before the click is fired in
        // frame 3. The persistent (`SidePanel`) variant has no sizing pass,
        // so the extra priming frame there is a harmless no-op.
        for _ in 0..2 {
            let mut priming = RawInput::default();
            priming.screen_rect = Some(screen_rect());
            priming.events.push(Event::PointerMoved(click_pos));
            let _ = ctx.run(priming, |ctx| {
                let _ = draw(ctx, items, placement, MIN_WIDTH, 1.0, None);
            });
        }

        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        input.events.push(Event::PointerMoved(click_pos));
        input.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        });
        input.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        });
        let mut captured = SidebarOutcome::default();
        let _ = ctx.run(input, |ctx| {
            captured = draw(ctx, items, placement, MIN_WIDTH, 1.0, None);
        });
        captured
    }

    // ── AC-1: width function ─────────────────────────────────────────

    #[test]
    fn width_clamps_to_min_for_very_narrow_windows() {
        assert_eq!(sidebar_width(0.0), MIN_WIDTH);
        assert_eq!(sidebar_width(100.0), MIN_WIDTH);
    }

    #[test]
    fn width_clamps_to_max_for_very_wide_windows() {
        assert_eq!(sidebar_width(3000.0), MAX_WIDTH);
        assert_eq!(sidebar_width(10_000.0), MAX_WIDTH);
    }

    #[test]
    fn width_is_22_percent_within_bounds() {
        // 1000 * 0.22 = 220, inside [180, 320].
        assert!((sidebar_width(1000.0) - 220.0).abs() < 0.01);
    }

    #[test]
    fn width_boundary_at_min_clamp_edge() {
        let just_below = MIN_WIDTH / WIDTH_FRACTION - 5.0;
        assert_eq!(sidebar_width(just_below), MIN_WIDTH);
        let at_edge = MIN_WIDTH / WIDTH_FRACTION;
        assert!((sidebar_width(at_edge) - MIN_WIDTH).abs() < 0.05);
    }

    #[test]
    fn width_boundary_at_max_clamp_edge() {
        let at_edge = MAX_WIDTH / WIDTH_FRACTION;
        assert!((sidebar_width(at_edge) - MAX_WIDTH).abs() < 0.05);
        let just_above = at_edge + 5.0;
        assert_eq!(sidebar_width(just_above), MAX_WIDTH);
    }

    // ── AC-2: view-model mapping ─────────────────────────────────────

    #[test]
    fn build_entries_empty_group_is_empty() {
        let g = MuxWindowGroup::new();
        assert_eq!(build_entries(&g), Vec::new());
    }

    #[test]
    fn build_entries_single_window_is_active() {
        let g = group_with(1, 0);
        let got = build_entries(&g);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].window_index, 0);
        assert_eq!(got[0].name, "w0");
        assert!(got[0].active);
    }

    #[test]
    fn build_entries_preserves_order_numbering_and_single_active_flag() {
        let g = group_with(4, 2);
        let got = build_entries(&g);
        assert_eq!(got.len(), 4);
        for (i, e) in got.iter().enumerate() {
            assert_eq!(e.window_index, i);
            assert_eq!(e.name, format!("w{i}"));
        }
        let actives: Vec<bool> = got.iter().map(|e| e.active).collect();
        assert_eq!(actives, vec![false, false, true, false]);
    }

    // ── task0006 AC-1: persistent panel sits on the RIGHT edge ────────

    #[test]
    fn persistent_panel_rows_hug_the_right_edge_of_the_screen() {
        let items = entries(1, 0);
        let rects = row_rects(&items, Placement::Persistent);
        let expected_right = screen_rect().right() - PANEL_PAD_HORIZONTAL;
        assert!(
            (rects[0].right() - expected_right).abs() < 0.5,
            "persistent row right edge {} should hug the screen's right edge \
             (expected ~{expected_right}) — the panel must be right-placed",
            rects[0].right()
        );
        // And NOT left-placed: the row must not sit near x=0.
        assert!(
            rects[0].left() > screen_rect().width() / 2.0,
            "persistent row left edge {} should be in the right half of the \
             screen, not hugging the left edge",
            rects[0].left()
        );
    }

    #[test]
    fn persistent_and_overlay_rows_sit_16px_apart_on_the_right_edge() {
        // Both placement variants live on the right edge, but the overlay
        // is now a floating card inset by OVERLAY_MARGIN (16 px) from the
        // terminal area's right edge, while the persistent panel is flush
        // (task0007 AC-5: "overlay rows now sit 16 px further left").
        let items = entries(1, 0);
        let persistent_rects = row_rects(&items, Placement::Persistent);
        let overlay_rects = row_rects(&items, Placement::Overlay);
        let actual_offset = persistent_rects[0].right() - overlay_rects[0].right();
        assert!(
            (actual_offset - OVERLAY_MARGIN).abs() < 0.5,
            "persistent right {} and overlay right {} should differ by {OVERLAY_MARGIN}px, \
             got {actual_offset}",
            persistent_rects[0].right(),
            overlay_rects[0].right()
        );
    }

    // ── task0007/task0008 AC-1/AC-2/AC-3: overlay floating-card geometry ──

    fn draw_overlay_and_capture_card(items: &[SidebarEntry], width: f32) -> OverlayCardDebug {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, Placement::Overlay, width, 1.0, None);
        });
        LAST_OVERLAY_CARD
            .with(|c| *c.borrow())
            .expect("draw_overlay records the card geometry")
    }

    /// Runs one frame of the overlay variant and returns BOTH the
    /// actually-painted card geometry and every row rect from that SAME
    /// frame, so tests can cross-check two independently-produced hooks
    /// (task0008 AC-1/AC-2 — this is what the task0007 hook could not do,
    /// since it recorded a precomputed rect instead of the real paint).
    fn draw_overlay_and_capture_card_and_rows(
        items: &[SidebarEntry],
        width: f32,
    ) -> (OverlayCardDebug, Vec<Rect>) {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, Placement::Overlay, width, 1.0, None);
        });
        let card = LAST_OVERLAY_CARD
            .with(|c| *c.borrow())
            .expect("draw_overlay records the card geometry");
        let rows = LAST_ROW_RECTS.with(|c| c.borrow().clone());
        (card, rows)
    }

    #[test]
    fn ac1_overlay_card_painted_rect_is_inset_16px_from_terminal_area_top_right_bottom() {
        // AC-1: asserted against the rect the background is ACTUALLY
        // painted with (`OverlayCardDebug::rect` now mirrors
        // `Prepared::paint`'s own computation), not a precomputed copy.
        let items = entries(1, 0);
        let card = draw_overlay_and_capture_card(&items, MIN_WIDTH);
        let area = screen_rect();
        assert!(
            (card.rect.top() - (area.top() + OVERLAY_MARGIN)).abs() < 0.01,
            "top inset: card top {} vs expected {}",
            card.rect.top(),
            area.top() + OVERLAY_MARGIN
        );
        assert!(
            (card.rect.right() - (area.right() - OVERLAY_MARGIN)).abs() < 0.01,
            "right inset: card right {} vs expected {}",
            card.rect.right(),
            area.right() - OVERLAY_MARGIN
        );
        assert!(
            (card.rect.bottom() - (area.bottom() - OVERLAY_MARGIN)).abs() < 0.01,
            "bottom inset: card bottom {} vs expected {}",
            card.rect.bottom(),
            area.bottom() - OVERLAY_MARGIN
        );
        assert!(
            (card.rect.width() - MIN_WIDTH).abs() < 0.01,
            "card width {} should equal the shared width function's value {MIN_WIDTH}",
            card.rect.width()
        );
    }

    #[test]
    fn ac3_overlay_card_spans_full_inset_height_with_zero_entries() {
        // AC-3: with (even) no entries, the card must still span the full
        // inset height — the painted background does not shrink to hug
        // an empty/short content region.
        let items: Vec<SidebarEntry> = Vec::new();
        let card = draw_overlay_and_capture_card(&items, MIN_WIDTH);
        let area = screen_rect();
        assert!(
            (card.rect.top() - (area.top() + OVERLAY_MARGIN)).abs() < 0.01,
            "card top {} should stay {OVERLAY_MARGIN}px from the terminal area's top even \
                 with zero entries, got area.top() {}",
            card.rect.top(),
            area.top()
        );
        assert!(
            (card.rect.bottom() - (area.bottom() - OVERLAY_MARGIN)).abs() < 0.01,
            "card bottom {} should stay {OVERLAY_MARGIN}px from the terminal area's bottom \
             even with zero entries (the area below the last row renders as plain card \
             surface, not a shrunk-to-content background), got area.bottom() {}",
            card.rect.bottom(),
            area.bottom()
        );
    }

    #[test]
    fn ac2_overlay_row_rects_are_inset_from_the_painted_card_by_panel_padding() {
        // AC-2 (left/right/top): cross-checks the row-paint hook against
        // the card-paint hook from the SAME frame. Under the pre-fix
        // defect, the painted card rect collapsed to the padded content
        // rect, so row.right() == card.rect.right() (no inset) — this
        // test fails under that defect and passes once the card rect is
        // the authoritative, larger rect.
        let items = entries(1, 0);
        let (card, rows) = draw_overlay_and_capture_card_and_rows(&items, MIN_WIDTH);
        assert_eq!(rows.len(), 1);
        assert!(
            (rows[0].right() - (card.rect.right() - PANEL_PAD_HORIZONTAL)).abs() < 0.5,
            "row right edge {} should be inset {PANEL_PAD_HORIZONTAL}px from the painted \
             card's right edge {} — the row must not touch the card's edge",
            rows[0].right(),
            card.rect.right()
        );
        assert!(
            (rows[0].left() - (card.rect.left() + PANEL_PAD_HORIZONTAL)).abs() < 0.5,
            "row left edge {} should be inset {PANEL_PAD_HORIZONTAL}px from the painted \
             card's left edge {}",
            rows[0].left(),
            card.rect.left()
        );
        assert!(
            (rows[0].top() - (card.rect.top() + PANEL_PAD_VERTICAL)).abs() < 0.5,
            "first row top {} should be inset {PANEL_PAD_VERTICAL}px from the painted card's \
             top edge {}",
            rows[0].top(),
            card.rect.top()
        );
    }

    #[test]
    fn ac2_overlay_scroll_viewport_bottom_not_last_row_is_inset_from_the_painted_card() {
        // AC-2 (bottom): "bottom applies to the scroll viewport's extent,
        // not each row". With a single short row far from filling the
        // card, the row's OWN bottom sits nowhere near the card's bottom,
        // but the row list's scrollable content region
        // (`OverlayCardDebug::content_rect`, the same rect the row ui is
        // actually laid out into) must still reach exactly the panel
        // padding above the painted card's bottom edge.
        let items = entries(1, 0);
        let (card, rows) = draw_overlay_and_capture_card_and_rows(&items, MIN_WIDTH);
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].bottom() < card.rect.bottom() - PANEL_PAD_VERTICAL - 1.0,
            "sanity: with only one row the row's own bottom {} should sit well above the \
             card's padded bottom {} — otherwise this test isn't exercising the \
             viewport-vs-row distinction",
            rows[0].bottom(),
            card.rect.bottom() - PANEL_PAD_VERTICAL
        );
        assert!(
            (card.content_rect.bottom() - (card.rect.bottom() - PANEL_PAD_VERTICAL)).abs() < 0.01,
            "scroll viewport bottom {} should be {PANEL_PAD_VERTICAL}px above the painted \
             card's bottom edge {}, regardless of how far the rows themselves reach",
            card.content_rect.bottom(),
            card.rect.bottom()
        );
    }

    #[test]
    fn overlay_card_has_12px_corner_radius_and_92_percent_alpha_surface_container_high() {
        let items = entries(1, 0);
        let card = draw_overlay_and_capture_card(&items, MIN_WIDTH);
        assert_eq!(card.rounding, OVERLAY_CORNER_RADIUS);

        // `egui::Color32` stores premultiplied sRGB, so the RGB channels
        // are no longer the raw `surface_container_high` base once alpha
        // is applied (`md3::state_layer`/`from_rgba_unmultiplied` do
        // gamma-correct premultiplication internally) — mirrors the
        // reasoning in `md3::tests::state_layer_alpha_scales_with_opacity`.
        // We verify the alpha channel here; the channel-level blend is
        // exercised visually through the renderer.
        let expected_alpha = (OVERLAY_FILL_ALPHA * 255.0) as u8;
        assert_eq!(
            card.fill.a(),
            expected_alpha,
            "overlay fill alpha should be 92% of surface_container_high's alpha channel"
        );
    }

    // ── task0002 AC-8: whole-card opacity application ───────────────────

    /// The maximum fill alpha among the painted card-background rects
    /// (`Rounding::same(OVERLAY_CORNER_RADIUS)` distinguishes the card's
    /// own background from the row pills, which round at `ROW_HEIGHT /
    /// 2.0` instead) in a `FullOutput`'s raw (untessellated) shape list —
    /// `Painter::add`/`set` apply the opacity transform (`gamma_multiply`)
    /// before a shape ever reaches this list, so this reads the
    /// ACTUALLY-PAINTED alpha, not a precomputed value.
    fn max_card_fill_alpha(output: &egui::FullOutput) -> Option<u8> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Option<u8>) {
            use egui::epaint::Shape;
            match shape {
                Shape::Vec(v) => {
                    for s in v {
                        walk(s, out);
                    }
                }
                Shape::Rect(r) if r.rounding.nw == OVERLAY_CORNER_RADIUS => {
                    *out = Some(out.map_or(r.fill.a(), |m| m.max(r.fill.a())));
                }
                _ => {}
            }
        }
        let mut found = None;
        for cs in &output.shapes {
            walk(&cs.shape, &mut found);
        }
        found
    }

    #[test]
    fn ac8_overlay_opacity_multiplies_the_painted_cards_fill_alpha() {
        // `egui::Area`'s very first-ever frame is an invisible "sizing
        // pass" (see `run_with_click`'s doc comment above) — its shapes
        // resolve to `Shape::Noop` placeholders, not real paint. A priming
        // frame settles the `Area` before the frame under test, mirroring
        // every other real-paint helper in this module.
        fn run_and_capture(items: &[SidebarEntry], opacity: f32) -> egui::FullOutput {
            let ctx = egui::Context::default();
            let mut priming = RawInput::default();
            priming.screen_rect = Some(screen_rect());
            let _ = ctx.run(priming, |ctx| {
                let _ = draw(ctx, items, Placement::Overlay, MIN_WIDTH, opacity, None);
            });
            let mut input = RawInput::default();
            input.screen_rect = Some(screen_rect());
            ctx.run(input, |ctx| {
                let _ = draw(ctx, items, Placement::Overlay, MIN_WIDTH, opacity, None);
            })
        }

        let items = entries(1, 0);
        let output_full = run_and_capture(&items, 1.0);
        let alpha_full =
            max_card_fill_alpha(&output_full).expect("the card background rect must be painted");

        let output_half = run_and_capture(&items, 0.5);
        let alpha_half =
            max_card_fill_alpha(&output_half).expect("the card background rect must be painted");

        assert!(
            alpha_half < alpha_full,
            "opacity=0.5 should paint a lower alpha than opacity=1.0 \
             (full={alpha_full}, half={alpha_half})"
        );
        // egui's `Color32::gamma_multiply`: `(component as f32 * factor +
        // 0.5) as u8` — assert the exact linear relationship rather than
        // just "lower", so a future accidental double-application (or a
        // non-linear substitute) would fail this test.
        let expected_half = (alpha_full as f32 * 0.5 + 0.5) as u8;
        assert_eq!(
            alpha_half, expected_half,
            "alpha should scale exactly per egui's gamma_multiply"
        );
    }

    #[test]
    fn ac8_persistent_placement_ignores_the_opacity_input_entirely() {
        // A value that WOULD make the panel invisible if it were (wrongly)
        // applied — proves this isn't a no-op merely because both runs
        // happen to use the same "reasonable" opacity.
        let items = entries(2, 0);

        let ctx_full = egui::Context::default();
        let mut input_full = RawInput::default();
        input_full.screen_rect = Some(screen_rect());
        let output_full = ctx_full.run(input_full, |ctx| {
            let _ = draw(ctx, &items, Placement::Persistent, MIN_WIDTH, 1.0, None);
        });

        let ctx_zero = egui::Context::default();
        let mut input_zero = RawInput::default();
        input_zero.screen_rect = Some(screen_rect());
        let output_zero = ctx_zero.run(input_zero, |ctx| {
            let _ = draw(ctx, &items, Placement::Persistent, MIN_WIDTH, 0.0, None);
        });

        assert_eq!(
            format!("{:?}", output_full.shapes),
            format!("{:?}", output_zero.shapes),
            "the persistent variant's painted output must be byte-identical \
             regardless of the opacity value supplied — it never even reads \
             the parameter (draw_persistent's signature has none)"
        );
    }

    // ── task0009 AC-1/AC-2/AC-3: per-frame card rect authoritative across
    // resize (cached Area surface state must not leak stale geometry) ─────

    /// Renders TWO consecutive frames of the SAME egui context (so the
    /// `Area`'s cached per-id surface state carries over between them, the
    /// exact condition the single-frame `draw_overlay_and_capture_card`
    /// helper cannot exercise) with different screen rects / widths, and
    /// returns the SECOND frame's actually-painted card geometry.
    fn draw_overlay_two_frames_and_capture_card(
        items: &[SidebarEntry],
        screen1: Rect,
        width1: f32,
        screen2: Rect,
        width2: f32,
    ) -> OverlayCardDebug {
        let ctx = egui::Context::default();

        let mut input1 = RawInput::default();
        input1.screen_rect = Some(screen1);
        let _ = ctx.run(input1, |ctx| {
            let _ = draw(ctx, items, Placement::Overlay, width1, 1.0, None);
        });

        let mut input2 = RawInput::default();
        input2.screen_rect = Some(screen2);
        LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());
        let _ = ctx.run(input2, |ctx| {
            let _ = draw(ctx, items, Placement::Overlay, width2, 1.0, None);
        });
        LAST_OVERLAY_CARD
            .with(|c| *c.borrow())
            .expect("draw_overlay records the card geometry")
    }

    fn assert_card_matches_frame(card: OverlayCardDebug, screen: Rect, width: f32, label: &str) {
        assert!(
            (card.rect.top() - (screen.top() + OVERLAY_MARGIN)).abs() < 0.5,
            "{label}: card top {} should track the SECOND frame's terminal area top \
             (expected ~{})",
            card.rect.top(),
            screen.top() + OVERLAY_MARGIN
        );
        assert!(
            (card.rect.right() - (screen.right() - OVERLAY_MARGIN)).abs() < 0.5,
            "{label}: card right {} should track the SECOND frame's terminal area right \
             (expected ~{})",
            card.rect.right(),
            screen.right() - OVERLAY_MARGIN
        );
        assert!(
            (card.rect.bottom() - (screen.bottom() - OVERLAY_MARGIN)).abs() < 0.5,
            "{label}: card bottom {} should track the SECOND frame's terminal area bottom \
             (expected ~{})",
            card.rect.bottom(),
            screen.bottom() - OVERLAY_MARGIN
        );
        assert!(
            (card.rect.width() - width).abs() < 0.5,
            "{label}: card width {} should equal the SECOND frame's width-function value {width}, \
             not a stale cached value from the first frame",
            card.rect.width()
        );
    }

    #[test]
    fn ac1_ac2_resize_grow_small_to_large_card_follows_second_frame() {
        let items = entries(1, 0);
        let screen_small = Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0));
        let screen_large = Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let width_small = MIN_WIDTH;
        let width_large = 300.0;
        let card = draw_overlay_two_frames_and_capture_card(
            &items,
            screen_small,
            width_small,
            screen_large,
            width_large,
        );
        assert_card_matches_frame(card, screen_large, width_large, "grow small->large");
    }

    #[test]
    fn ac1_ac2_resize_shrink_large_to_small_card_follows_second_frame() {
        let items = entries(1, 0);
        let screen_large = Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let screen_small = Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0));
        let width_large = 300.0;
        let width_small = MIN_WIDTH;
        let card = draw_overlay_two_frames_and_capture_card(
            &items,
            screen_large,
            width_large,
            screen_small,
            width_small,
        );
        assert_card_matches_frame(card, screen_small, width_small, "shrink large->small");
    }

    #[test]
    fn ac3_overlay_row_insets_hold_on_post_resize_frame() {
        // task0008's row-inset assertions (8/12px) must still hold on the
        // frame immediately after a resize, not just on a freshly-created
        // context's first frame.
        let items = entries(1, 0);
        let screen1 = Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0));
        let screen2 = Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let width1 = MIN_WIDTH;
        let width2 = 300.0;
        let card =
            draw_overlay_two_frames_and_capture_card(&items, screen1, width1, screen2, width2);
        let rows = LAST_ROW_RECTS.with(|c| c.borrow().clone());
        assert_eq!(rows.len(), 1);
        assert!(
            (rows[0].right() - (card.rect.right() - PANEL_PAD_HORIZONTAL)).abs() < 0.5,
            "post-resize row right edge {} should stay inset {PANEL_PAD_HORIZONTAL}px from the \
             post-resize card's right edge {}",
            rows[0].right(),
            card.rect.right()
        );
        assert!(
            (rows[0].left() - (card.rect.left() + PANEL_PAD_HORIZONTAL)).abs() < 0.5,
            "post-resize row left edge {} should stay inset {PANEL_PAD_HORIZONTAL}px from the \
             post-resize card's left edge {}",
            rows[0].left(),
            card.rect.left()
        );
        assert!(
            (rows[0].top() - (card.rect.top() + PANEL_PAD_VERTICAL)).abs() < 0.5,
            "post-resize first row top {} should stay inset {PANEL_PAD_VERTICAL}px from the \
             post-resize card's top edge {}",
            rows[0].top(),
            card.rect.top()
        );
    }

    #[test]
    fn ac3_overlay_full_inset_height_holds_with_few_entries_post_resize() {
        // task0008 AC-3 re-asserted on a post-resize frame: with zero
        // entries the card must still span the full inset height of the
        // SECOND frame's terminal area, not the first frame's.
        let items: Vec<SidebarEntry> = Vec::new();
        let screen1 = Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let screen2 = Rect::from_min_size(Pos2::ZERO, egui::vec2(500.0, 400.0));
        let width1 = 300.0;
        let width2 = MIN_WIDTH;
        let card =
            draw_overlay_two_frames_and_capture_card(&items, screen1, width1, screen2, width2);
        assert!(
            (card.rect.top() - (screen2.top() + OVERLAY_MARGIN)).abs() < 0.5,
            "card top {} should stay {OVERLAY_MARGIN}px from the SECOND frame's terminal area \
             top even with zero entries, got screen2.top() {}",
            card.rect.top(),
            screen2.top()
        );
        assert!(
            (card.rect.bottom() - (screen2.bottom() - OVERLAY_MARGIN)).abs() < 0.5,
            "card bottom {} should stay {OVERLAY_MARGIN}px from the SECOND frame's terminal \
             area bottom even with zero entries, got screen2.bottom() {}",
            card.rect.bottom(),
            screen2.bottom()
        );
    }

    #[test]
    fn ac3_overlay_variant_paints_no_separator_line() {
        let src = include_str!("mux_sidebar.rs");
        let after_overlay_fn = src
            .split("fn draw_overlay(")
            .nth(1)
            .expect("draw_overlay fn present in source");
        // Isolate the function body up to the next top-level `fn`
        // declaration (there are no nested `fn`s inside draw_overlay,
        // only closures, so this boundary is unambiguous).
        let body = after_overlay_fn
            .split("\nfn ")
            .next()
            .unwrap_or(after_overlay_fn);
        assert!(
            !body.contains(".vline("),
            "overlay variant must not paint a separator line (AC-3); the persistent \
             variant's left-edge separator is unchanged"
        );
    }

    // ── AC-3: click reporting ────────────────────────────────────────

    #[test]
    fn clicking_a_row_reports_its_window_index_persistent() {
        let items = entries(3, 0);
        let target = row_rects(&items, Placement::Persistent)[2].center();
        let outcome = run_with_click(&items, Placement::Persistent, target);
        assert_eq!(outcome.switch_to_window, Some(2));
    }

    #[test]
    fn clicking_a_row_reports_its_window_index_overlay() {
        let items = entries(3, 0);
        let target = row_rects(&items, Placement::Overlay)[1].center();
        let outcome = run_with_click(&items, Placement::Overlay, target);
        assert_eq!(outcome.switch_to_window, Some(1));
    }

    #[test]
    fn clicking_below_all_rows_reports_nothing() {
        let items = entries(2, 0);
        let rects = row_rects(&items, Placement::Persistent);
        let below = Pos2::new(rects[0].center().x, rects[1].bottom() + 100.0);
        let outcome = run_with_click(&items, Placement::Persistent, below);
        assert_eq!(outcome, SidebarOutcome::default());
    }

    #[test]
    fn draw_with_empty_entries_does_not_panic_and_emits_nothing() {
        let items: Vec<SidebarEntry> = Vec::new();
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut captured = SidebarOutcome::default();
        let _ = ctx.run(input, |ctx| {
            captured = draw(ctx, &items, Placement::Persistent, MIN_WIDTH, 1.0, None);
        });
        assert_eq!(captured, SidebarOutcome::default());
    }

    // ── AC-4: truncation + overflow scrolling ─────────────────────────

    #[test]
    fn ellipsized_layout_truncates_when_overflowing_available_width() {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut result_w = 0.0f32;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = FontId::proportional(NAME_FONT_SIZE);
                let long = "a-very-long-mux-window-name-that-does-not-fit-in-the-row";
                let full =
                    ui.fonts(|f| f.layout_no_wrap(long.to_string(), font.clone(), Color32::WHITE));
                let max_w = full.size().x / 2.0;
                let truncated =
                    ui.fonts(|f| layout_ellipsized(f, long, &font, Color32::WHITE, max_w));
                result_w = truncated.size().x;
                assert!(
                    truncated.size().x <= max_w + 0.5,
                    "truncated width {} should respect the bound {}",
                    truncated.size().x,
                    max_w
                );
            });
        });
        assert!(result_w > 0.0);
    }

    #[test]
    fn ellipsized_layout_leaves_short_text_untouched() {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = FontId::proportional(NAME_FONT_SIZE);
                let full = ui
                    .fonts(|f| f.layout_no_wrap("short".to_string(), font.clone(), Color32::WHITE));
                let result =
                    ui.fonts(|f| layout_ellipsized(f, "short", &font, Color32::WHITE, 500.0));
                assert_eq!(result.size().x, full.size().x);
            });
        });
    }

    #[test]
    fn rows_never_shrink_and_all_lay_out_when_content_exceeds_available_height() {
        // 20 rows definitely exceed a 150px-tall viewport; the widget must
        // still lay out every row at full height (scrolling, not clipping
        // or shrinking rows to fit).
        let items = entries(20, 0);
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 150.0)));
        LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, &items, Placement::Persistent, MIN_WIDTH, 1.0, None);
        });
        let rects = LAST_ROW_RECTS.with(|c| c.borrow().clone());
        assert_eq!(
            rects.len(),
            20,
            "the scroll area must lay out every entry, not clip the list"
        );
        for r in &rects {
            assert!(
                (r.height() - ROW_HEIGHT).abs() < 0.5,
                "row height must stay fixed at {ROW_HEIGHT}, got {}",
                r.height()
            );
        }
    }

    // ── AC-5: colors exclusively via md3 accessors ────────────────────

    #[test]
    fn ac5_no_hardcoded_color_constructors_in_module_source() {
        let src = include_str!("mux_sidebar.rs");
        // Only scan the production code above the test module boundary —
        // this assertion's own string literals below would otherwise
        // self-match `include_str!`'s full-file contents.
        let production_src = src.split("\nmod tests {").next().unwrap_or(src);
        let forbidden = [
            "Color32::from_rgb(",
            "Color32::from_rgba_unmultiplied(",
            "Color32::from_rgba_premultiplied(",
            "Color32::from_gray(",
            "Color32::from_black_alpha(",
            "Color32::from_white_alpha(",
            "Color32::BLACK",
            "Color32::WHITE",
            "Color32::RED",
            "Color32::GREEN",
            "Color32::BLUE",
        ];
        for needle in forbidden {
            assert!(
                !production_src.contains(needle),
                "mux_sidebar.rs must source all colors from md3:: accessors (or the shared \
                 dialog elevation token); found hardcoded {needle}"
            );
        }
        assert!(
            production_src.contains("md3::"),
            "sanity: the module should reference md3 accessors at all"
        );
    }

    // ── task0010 AC-1: hit-region boundary ─────────────────────────────

    #[test]
    fn ac1_hit_region_true_exactly_inside_persistent_strip_false_just_outside() {
        let window_size = egui::vec2(800.0, 600.0);
        let top_chrome = 80.0;
        let bottom_chrome = 24.0;
        let width = sidebar_width(window_size.x);
        let visible = Some(Placement::Persistent);
        let rect = persistent_panel_rect(window_size, top_chrome, width);

        // Both edges (inclusive — `Rect::contains` is `<=` on every side)
        // plus the interior.
        for p in [rect.min, rect.max, rect.center()] {
            assert!(
                point_in_sidebar(p, visible, window_size, top_chrome, bottom_chrome),
                "{p:?} should be inside the persistent strip {rect:?}"
            );
        }
        // Just outside each of the four edges.
        for p in [
            egui::pos2(rect.min.x - 1.0, rect.center().y),
            egui::pos2(rect.center().x, rect.min.y - 1.0),
            egui::pos2(rect.max.x + 1.0, rect.center().y),
            egui::pos2(rect.center().x, rect.max.y + 1.0),
        ] {
            assert!(
                !point_in_sidebar(p, visible, window_size, top_chrome, bottom_chrome),
                "{p:?} should be outside the persistent strip {rect:?}"
            );
        }
    }

    #[test]
    fn ac1_hit_region_true_exactly_inside_overlay_card_false_just_outside() {
        let window_size = egui::vec2(800.0, 600.0);
        let top_chrome = 80.0;
        let bottom_chrome = 24.0;
        let width = sidebar_width(window_size.x);
        let visible = Some(Placement::Overlay);
        let terminal_area = terminal_area_rect(window_size, top_chrome, bottom_chrome);
        let rect = overlay_card_rect(terminal_area, width);

        for p in [rect.min, rect.max, rect.center()] {
            assert!(
                point_in_sidebar(p, visible, window_size, top_chrome, bottom_chrome),
                "{p:?} should be inside the overlay card {rect:?}"
            );
        }
        for p in [
            egui::pos2(rect.min.x - 1.0, rect.center().y),
            egui::pos2(rect.center().x, rect.min.y - 1.0),
            egui::pos2(rect.max.x + 1.0, rect.center().y),
            egui::pos2(rect.center().x, rect.max.y + 1.0),
        ] {
            assert!(
                !point_in_sidebar(p, visible, window_size, top_chrome, bottom_chrome),
                "{p:?} should be outside the overlay card {rect:?}"
            );
        }
    }

    #[test]
    fn ac1_hit_region_always_false_when_sidebar_hidden() {
        let window_size = egui::vec2(800.0, 600.0);
        // Includes points that WOULD be inside either region were the
        // sidebar visible, to prove `None` (hidden) overrides geometry
        // entirely rather than merely picking a variant.
        let width = sidebar_width(window_size.x);
        let candidates = [
            Pos2::ZERO,
            egui::pos2(window_size.x - width / 2.0, window_size.y - 1.0),
            egui::pos2(window_size.x - 20.0, 100.0),
            egui::pos2(window_size.x / 2.0, window_size.y / 2.0),
        ];
        for p in candidates {
            assert!(
                !point_in_sidebar(p, None, window_size, 80.0, 24.0),
                "{p:?} must be outside when the sidebar is hidden (visible_placement = None)"
            );
        }
    }

    // ── task0010 AC-4: hit-region geometry shares the draw path's ──────
    // ── derivation (no duplicated magic numbers) ────────────────────────

    #[test]
    fn ac4_persistent_hit_region_matches_the_real_panel_rect_from_the_frame_composition_order() {
        // Mirrors `render::draw_terminal`'s ACTUAL panel order: title bar
        // (top), tab bar (top), THEN the persistent sidebar `SidePanel`,
        // THEN the status bar (bottom), THEN the central panel — using the
        // SAME shared height constants/functions the real widgets use
        // (`title_bar::TITLE_BAR_HEIGHT`, `tab_bar::
        // effective_tab_bar_height`). The sidebar `SidePanel` claims its
        // full vertical span BEFORE the status-bar panel is added, so its
        // REAL rect reaches the very bottom of the window; this test pins
        // that against egui's actual panel layout (not a guessed
        // assumption) and checks `persistent_panel_rect` reproduces it.
        let ctx = egui::Context::default();
        let window_size = egui::vec2(800.0, 600.0);
        let mut input = RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, window_size));
        let show_tab_bar = true;
        let items = entries(1, 0);
        let width = MIN_WIDTH;

        LAST_PERSISTENT_PANEL_RECT.with(|c| *c.borrow_mut() = None);
        let _ = ctx.run(input, |ctx| {
            egui::TopBottomPanel::top("t0010-title")
                .exact_height(super::super::title_bar::TITLE_BAR_HEIGHT)
                .show(ctx, |_| {});
            egui::TopBottomPanel::top("t0010-tabbar")
                .exact_height(super::super::tab_bar::effective_tab_bar_height(
                    show_tab_bar,
                ))
                .show(ctx, |_| {});
            let _ = draw(ctx, &items, Placement::Persistent, width, 1.0, None);
            egui::TopBottomPanel::bottom("t0010-statusbar")
                .exact_height(40.0)
                .show(ctx, |_| {});
            egui::CentralPanel::default().show(ctx, |_| {});
        });

        let real_rect = LAST_PERSISTENT_PANEL_RECT
            .with(|c| *c.borrow())
            .expect("draw_persistent records the panel rect");
        let top_chrome = top_chrome_inset(show_tab_bar);
        let computed = persistent_panel_rect(window_size, top_chrome, width);
        assert!(
            (real_rect.top() - computed.top()).abs() < 0.5,
            "top: computed {computed:?} vs real {real_rect:?}"
        );
        assert!(
            (real_rect.bottom() - computed.bottom()).abs() < 0.5,
            "bottom: computed {computed:?} vs real {real_rect:?}"
        );
        assert!(
            (real_rect.left() - computed.left()).abs() < 0.5,
            "left: computed {computed:?} vs real {real_rect:?}"
        );
        assert!(
            (real_rect.right() - computed.right()).abs() < 0.5,
            "right: computed {computed:?} vs real {real_rect:?}"
        );
    }

    #[test]
    fn ac4_overlay_hit_region_matches_the_real_painted_card_with_top_and_bottom_chrome() {
        // Same idea for the overlay: run a real top+bottom chrome pair (in
        // the SAME order `render::draw_terminal` uses — chrome, then
        // `CentralPanel`, then the overlay draws) and compare the ACTUAL
        // painted card (`LAST_OVERLAY_CARD`, already the paint-rect
        // authority per IMPLEMENTATION.md decision 3 update 2) against
        // `overlay_card_rect(terminal_area_rect(...), width)`.
        let ctx = egui::Context::default();
        let window_size = egui::vec2(800.0, 600.0);
        let mut input = RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, window_size));
        let top_chrome = 80.0;
        let bottom_chrome = 24.0;
        let width = MIN_WIDTH;
        let items = entries(1, 0);

        let _ = ctx.run(input, |ctx| {
            egui::TopBottomPanel::top("t0010-chrome-top")
                .exact_height(top_chrome)
                .show(ctx, |_| {});
            egui::TopBottomPanel::bottom("t0010-chrome-bottom")
                .exact_height(bottom_chrome)
                .show(ctx, |_| {});
            egui::CentralPanel::default().show(ctx, |_| {});
            let _ = draw(ctx, &items, Placement::Overlay, width, 1.0, None);
        });

        let card = LAST_OVERLAY_CARD
            .with(|c| *c.borrow())
            .expect("draw_overlay records the card geometry");
        let computed = overlay_card_rect(
            terminal_area_rect(window_size, top_chrome, bottom_chrome),
            width,
        );
        assert!(
            (card.rect.top() - computed.top()).abs() < 0.5,
            "top: computed {computed:?} vs painted {:?}",
            card.rect
        );
        assert!(
            (card.rect.bottom() - computed.bottom()).abs() < 0.5,
            "bottom: computed {computed:?} vs painted {:?}",
            card.rect
        );
        assert!(
            (card.rect.left() - computed.left()).abs() < 0.5,
            "left: computed {computed:?} vs painted {:?}",
            card.rect
        );
        assert!(
            (card.rect.right() - computed.right()).abs() < 0.5,
            "right: computed {computed:?} vs painted {:?}",
            card.rect
        );
    }

    // ── task0006 AC-1/AC-2: build_entries carries pane_id, badge defaults ──

    #[test]
    fn build_entries_populates_pane_id_from_the_group_and_defaults_badge_to_none() {
        let g = group_with(2, 0);
        let got = build_entries(&g);
        assert_eq!(got[0].pane_id, 100);
        assert_eq!(got[1].pane_id, 101);
        for e in &got {
            assert_eq!(e.badge, None);
        }
    }

    // ── task0006 AC-2: badge absence reserves no name-column shift ──────

    fn name_text_x(items: &[SidebarEntry]) -> f32 {
        fn collect(shape: &egui::epaint::Shape, out: &mut Vec<(f32, String)>) {
            use egui::epaint::Shape;
            match shape {
                Shape::Text(t) => {
                    let mut s = String::new();
                    for row in &t.galley.rows {
                        for g in &row.glyphs {
                            s.push(g.chr);
                        }
                    }
                    if !s.is_empty() {
                        out.push((t.pos.x, s));
                    }
                }
                Shape::Vec(v) => {
                    for s in v {
                        collect(s, out);
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let output = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, Placement::Persistent, MIN_WIDTH, 1.0, None);
        });
        let mut shapes = Vec::new();
        for cs in &output.shapes {
            collect(&cs.shape, &mut shapes);
        }
        shapes
            .into_iter()
            .filter(|(_, s)| s.contains('w'))
            .map(|(x, _)| x)
            .fold(f32::MAX, f32::min)
    }

    #[test]
    fn badge_absent_leaves_the_name_column_at_its_pre_feature_position() {
        let mut without_badge = entries(1, 0);
        without_badge[0].badge = None;
        let mut with_badge = entries(1, 0);
        with_badge[0].badge = Some(Aggregated {
            state: crate::agent_status::AgentState::Working,
            unseen: true,
        });

        let x_without = name_text_x(&without_badge);
        let x_with = name_text_x(&with_badge);
        assert!(
            x_with > x_without,
            "a present badge should push the name right (x_with={x_with}, \
             x_without={x_without})"
        );

        // Two independently-built entries with no badge must agree exactly
        // — this is the AC-2 guarantee: absence never shifts the layout.
        let mut also_without_badge = entries(1, 0);
        also_without_badge[0].badge = None;
        assert_eq!(x_without, name_text_x(&also_without_badge));
    }

    // ── task0001: regression tests over the row's single hit target ────
    // AC-1/AC-9: `ac1_draw_rows_...` is a structural check confirming
    // `draw_rows` registers no interaction region and paints no glyph
    // beyond the row itself; AC-3: the exhaustive pattern in
    // `ac3_sidebar_outcome_...` compiles only when `SidebarOutcome`
    // carries exactly the window-switch field; AC-2:
    // `ac1_ac2_click_at_...` behaviorally confirms a click at the row's
    // former right-edge reserved region now switches windows.

    #[test]
    fn ac1_draw_rows_registers_no_interaction_region_besides_the_row_itself() {
        // AC-1: `draw_rows`'s per-entry loop must register exactly one hit
        // target (the row's own `allocate_exact_size(.., Sense::click())`)
        // and paint no glyph beyond the row's own content.
        let src = include_str!("mux_sidebar.rs");
        let start = src
            .find("fn draw_rows(")
            .expect("draw_rows present in source");
        let body = &src[start..];
        let end = body[1..].find("\nfn ").map(|i| i + 1).unwrap_or(body.len());
        let draw_rows_src = &body[..end];
        assert!(
            !draw_rows_src.contains("ui.interact("),
            "draw_rows must register no interaction region beyond the row's \
             own click sense (AC-1); found an extra `ui.interact(` call"
        );
        assert!(
            !draw_rows_src.contains("paint_copy_icon("),
            "draw_rows must paint no glyph beyond the row's own content \
             (AC-1); found a `paint_copy_icon(` call"
        );
    }

    #[test]
    fn ac3_sidebar_outcome_exposes_only_the_window_switch_result() {
        // AC-3: exhaustive struct pattern — compiles only when
        // `SidebarOutcome` has exactly this one field. Catches a
        // copy-pane-id field left behind by an incomplete removal at
        // compile time (rather than a runtime assertion).
        let SidebarOutcome {
            switch_to_window: _,
        } = SidebarOutcome::default();
    }

    #[test]
    fn ac1_ac2_click_at_the_row_s_former_icon_region_reports_a_window_switch() {
        // AC-2: a click positioned inside the region the icon used to
        // occupy — the row's right edge, inset by the row's own
        // horizontal padding — must fall through to the row's own click
        // sense and report a window switch, because no nested
        // interaction region is registered there anymore.
        let items = entries(1, 0);
        let row = row_rects(&items, Placement::Persistent)[0];
        let former_icon_region =
            egui::pos2(row.right() - ROW_HORIZONTAL_PAD - 10.0, row.center().y);
        let outcome = run_with_click(&items, Placement::Persistent, former_icon_region);
        assert_eq!(
            outcome.switch_to_window,
            Some(0),
            "a click at the former copy-icon position must switch windows \
             once the copy affordance is removed"
        );
    }

    // ── task0001 AC-4: identical shared painter, unified badge slot ─────

    // Standing up the REAL swash + bundled-font stack in a unit test is
    // impractical (per the task's Test Notes) — this stub mirrors the one
    // in `ui::tab_bar`'s test module so both widgets' AC-3/AC-4 tests
    // exercise the same texture-blit code path via `paint_agent_badge`.
    struct StubEmojiRasterizer;

    impl crate::render::font::traits::GlyphRasterizer for StubEmojiRasterizer {
        fn shape(
            &self,
            _cluster: &str,
            font: crate::render::font::traits::FontId,
            size_px: f32,
        ) -> Vec<crate::render::font::traits::ShapedGlyph> {
            vec![crate::render::font::traits::ShapedGlyph {
                font,
                glyph_id: 1,
                size_px,
            }]
        }

        fn raster(
            &self,
            _font: crate::render::font::traits::FontId,
            _glyph_id: u32,
            size_px: f32,
        ) -> Option<crate::render::font::traits::GlyphBitmap> {
            let n = size_px.round().max(1.0) as u32;
            Some(crate::render::font::traits::GlyphBitmap {
                format: crate::render::font::traits::AtlasFormat::Rgba,
                width: n,
                height: n,
                bearing: (0, 0),
                advance: size_px,
                pixels: vec![255u8; (n as usize) * (n as usize) * 4],
            })
        }

        fn has_codepoint(&self, _font: crate::render::font::traits::FontId, _cp: u32) -> bool {
            true
        }
    }

    fn stub_emoji_fallback() -> crate::render::font::fallback::FallbackChain {
        use crate::render::font::traits::FontId;
        let mut chain = crate::render::font::fallback::FallbackChain::new(FontId(1), [FontId(2)]);
        chain.set_emoji(FontId(2));
        chain
    }

    /// Collect the rects of every textured (image-blit) `Shape::Rect` —
    /// mirrors `ui::tab_bar::tests::collect_textured_rects`.
    fn collect_textured_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<Rect> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<Rect>) {
            use egui::epaint::Shape;
            match shape {
                Shape::Rect(r) if r.fill_texture_id != egui::TextureId::default() => {
                    out.push(r.rect);
                }
                Shape::Vec(v) => {
                    for s in v {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for cs in shapes {
            walk(&cs.shape, &mut out);
        }
        out
    }

    #[test]
    fn ac4_working_and_idle_badges_with_emoji_resources_paint_texture_blit() {
        for state in [
            crate::agent_status::AgentState::Working,
            crate::agent_status::AgentState::Idle,
        ] {
            let mut items = entries(1, 0);
            items[0].badge = Some(Aggregated {
                state,
                unseen: true,
            });
            let rasterizer = StubEmojiRasterizer;
            let fallback = stub_emoji_fallback();
            let cache = parking_lot::Mutex::new(crate::ui::emoji_cache::EmojiTextureCache::new());
            let emoji = EmojiResources {
                rasterizer: &rasterizer,
                fallback: &fallback,
                cache: &cache,
            };
            let ctx = egui::Context::default();
            let mut input = RawInput::default();
            input.screen_rect = Some(screen_rect());
            let output = ctx.run(input, |ctx| {
                let _ = draw(
                    ctx,
                    &items,
                    Placement::Persistent,
                    MIN_WIDTH,
                    1.0,
                    Some(&emoji),
                );
            });

            let textured = collect_textured_rects(&output.shapes);
            assert!(
                !textured.is_empty(),
                "{state:?}: expected a textured rect (emoji blit) for the badge"
            );
            for r in &textured {
                assert!(
                    r.width() <= AGENT_BADGE_SLOT_WIDTH + 0.01
                        && r.height() <= AGENT_BADGE_SLOT_WIDTH + 0.01,
                    "{state:?}: emoji blit must aspect-fit inside the identical \
                     {AGENT_BADGE_SLOT_WIDTH}px slot the tab bar uses; got {r:?}"
                );
            }
        }
    }

    // ── task0001 AC-5: unified 12px badge slot ───────────────────────────

    #[test]
    fn ac5_working_to_done_transition_causes_no_name_column_shift() {
        // The reserved slot width is unified across ALL states (Design
        // 4), so a badge state transition must never move the name
        // column even though `working` and `done` render different
        // emoji clusters (agent-badge-emoji task0001) and (with no
        // emoji resources supplied here) different fallback-circle
        // shapes.
        let mut working = entries(1, 0);
        working[0].badge = Some(Aggregated {
            state: crate::agent_status::AgentState::Working,
            unseen: true,
        });
        let mut done = entries(1, 0);
        done[0].badge = Some(Aggregated {
            state: crate::agent_status::AgentState::Done,
            unseen: true,
        });
        assert_eq!(
            name_text_x(&working),
            name_text_x(&done),
            "a working -> done badge transition must cause no name-column x-shift"
        );
    }
}
