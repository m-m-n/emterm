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
mod tests;
