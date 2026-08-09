//! Grid → egui draw routines.
//!
//! Phase 6 swap: the renderer reads the grid through `term_core` accessors
//! (`get_cell_char`, `get_cell_fg/bg/flags`, `get_cursor_*`) instead of the
//! Phase 1 PoC's bespoke `Grid` type. Colors are decoded from the packed
//! `u32` returned by `get_cell_fg/bg`.
//!
//! Sub-phase 2 (dirty-row diff): the per-cell loop below still iterates the
//! full grid on every invocation, but the caller (`window_host::render`)
//! now skips the entire egui run when `App::dirty_rows_this_frame` is empty.
//! egui's immediate-mode pipeline rebuilds tessellation per frame, so true
//! per-row skipping requires a persistent offscreen target — that lives in
//! a future sub-phase. Today the savings come from frame-level skip plus
//! `term_core::clear_dirty()` consumption synchronized with each rendered
//! frame.
//!
//! Sub-phase 3 (cursor + SGR full reflection): `cell_style` honors every
//! `term_core::cell::STYLE_*` flag we track today (bold via weight, dim via
//! alpha, italic via egui italic face, underline as a horizontal line,
//! reverse by swapping fg/bg, hidden by clamping fg to bg, strikethrough
//! as an overlay line). `draw_cursor` reads the cursor's style/blink/
//! visibility/color getters so the renderer is ready to respond as soon
//! as the parser routes for DECSCUSR / DECTCEM / OSC 22 / OSC 12 land in
//! sub-phase 6. Double / curly underline plus SGR 58 underline color
//! await a future term_core extension (only a single `STYLE_UNDERLINE`
//! bit exists today). Per-cell `STYLE_BLINK` is rendered statically
//! (no animation) to avoid two competing blink phases against the
//! cursor; revisit when sub-phase 6 fires.

pub mod app_icon;
pub mod block_drawing;
pub mod box_drawing;
pub mod cursor;
pub mod emoji_resample;
pub mod font;
pub mod terminal_grid_pass;
pub mod theme;

use std::time::Duration;

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use term_core::cell::{
    STYLE_BLINK, STYLE_BOLD, STYLE_DIM, STYLE_HIDDEN, STYLE_ITALIC, STYLE_REVERSE,
    STYLE_STRIKETHROUGH, STYLE_UNDERLINE,
};
use term_core::terminal_core::TerminalCore;
use term_core::{char_width, is_ambiguous_width};

use crate::app::{App, BLINK_HALF_MS};
use crate::render::terminal_grid_pass::{CellInput, GlyphFit};
use crate::render::theme::{Rgb, Theme};
use crate::selection::Selection;
use crate::settings::AmbiguousWidthMode;

/// Fallback cell width in logical pixels when the rasterizer can't
/// measure "M" (e.g. test builds with a stub rasterizer that returns
/// no glyphs). Picked to roughly match Inconsolata 13pt so failure
/// modes still produce a usable grid.
pub const FALLBACK_CELL_W: f32 = 8.5;
/// Fallback cell height in logical pixels. Mirrors [`FALLBACK_CELL_W`]'s
/// intent — used only when the rasterizer cannot supply metrics for
/// the base font.
pub const FALLBACK_CELL_H: f32 = 17.0;

/// Peak overlay opacity of the visual-bell flash. The WebView build
/// brightens the whole container via `filter: brightness(2)`; a 25 %
/// white wash over the cell grid reads as the same "blink" without a
/// post-processing pass.
const BELL_FLASH_MAX_ALPHA: f32 = 0.25;

/// Compute the per-cell width and height (logical pixels) for a given
/// font + size, mirroring the legacy WebView build's
/// `ctx.measureText("M").width` / `ceil(ascent + descent)` path. The
/// returned values are at egui's logical-pixel scale (1.0×); the
/// renderer multiplies by `pixels_per_point` for the physical-pixel
/// metrics handed to wgpu.
///
/// Returns the [`FALLBACK_CELL_W`] / [`FALLBACK_CELL_H`] pair when the
/// rasterizer cannot shape "M" against the base font (typically only
/// in test builds whose font stack has no registered glyphs).
pub fn compute_cell_dims(
    rasterizer: &dyn crate::render::font::traits::GlyphRasterizer,
    fallback: &crate::render::font::fallback::FallbackChain,
    font_size_px: f32,
) -> (f32, f32) {
    let base = fallback.base();
    // Width: shape "M" against the base font and read the advance off
    // the resulting glyph bitmap. For monospace coding fonts every
    // glyph has the same advance, so the single-character probe is
    // sufficient.
    //
    // Rounded to whole pixels: the WebView build's `measureText("M")`
    // goes through FreeType under full hinting, which grid-fits the
    // advance to an integer (13 pt Inconsolata: 8.667 → 9 px). Using the
    // font's raw fractional advance made every cell ~1/3 px narrower
    // than the WebView build, read as "the right side of each cell is
    // missing a pixel" (glyph inked edge-to-edge with no gap).
    let advance = rasterizer
        .shape("M", base, font_size_px)
        .first()
        .and_then(|g| rasterizer.raster(g.font, g.glyph_id, g.size_px))
        .map(|b| b.advance.round().max(1.0))
        .filter(|a| a.is_finite() && *a > 0.0)
        .unwrap_or(FALLBACK_CELL_W);
    // Height: ascent + descent matches the WebView build's
    // `ceil(ascent + descent)`. `line_gap` is intentionally excluded
    // so the grid stays tight (most monospace coding fonts ship a
    // zero line gap anyway).
    let height = rasterizer
        .font_metrics(base, font_size_px)
        .map(|m| (m.ascent + m.descent).ceil())
        .filter(|h| h.is_finite() && *h > 0.0)
        .unwrap_or(FALLBACK_CELL_H);
    (advance, height)
}

/// Per-cell paint parameters resolved from a `term_core` cell + active
/// palette + selection state.
struct CellStyle {
    fg: Color32,
    bg: Color32,
    // Read by future Resolver-driven weight / style selection; the prior
    // painter.text() path read these for egui font face, which is now gone.
    #[allow(dead_code)]
    bold: bool,
    #[allow(dead_code)]
    italic: bool,
    underline: bool,
    strikethrough: bool,
}

/// Bundle of widget events emitted by the chrome (title bar / tab
/// bar) during a single frame. Either field may be `None` when no
/// interaction landed this frame. The render loop applies them in
/// the order: title bar → tab bar, mirroring their on-screen stack.
pub struct FrameEvents {
    pub title: Option<crate::ui::TitleBarEvent>,
    pub tab: Option<crate::ui::TabEvent>,
    /// Scrollbar thumb interaction: jump the active tab's viewport to
    /// this absolute scrollback offset (rows back from live). Applied
    /// by `window_host` after the egui pass via `App::scroll_set_offset`
    /// — the renderer only holds `&App`.
    pub scroll_to: Option<u32>,
    /// Search-bar interaction emitted this frame (query change, toggle,
    /// next / prev, close). Applied post-frame by `window_host` against
    /// `App` (re-run search / navigate / close). `None` when the overlay
    /// is hidden or nothing was interacted with.
    pub search: Option<crate::ui::search_bar::SearchBarEvent>,
    /// Profile-selector pointer interaction emitted this frame (row
    /// click confirm / scrim click cancel). Applied post-frame by
    /// `window_host` against `App`. `None` when the modal is hidden or
    /// nothing was clicked.
    pub profile: Option<crate::ui::profile_selector::ProfileSelectorEvent>,
    /// SFTP overlay/dialog/toast interaction emitted this frame. Applied
    /// post-frame by `window_host` against `App` (confirm upload / overwrite,
    /// cancel a session). `None` when nothing was interacted with.
    pub sftp: Option<SftpFrameEvent>,
}

impl FrameEvents {
    /// Whether ANY event fired this frame. Consumed by `window_host::render`
    /// to gate post-event recomputation (the second fold-layout refresh).
    /// Kept next to the field list so adding a field prompts extending it —
    /// a call-site re-enumeration would drift silently when a new overlay
    /// event is added.
    pub fn any(&self) -> bool {
        self.title.is_some()
            || self.tab.is_some()
            || self.scroll_to.is_some()
            || self.search.is_some()
            || self.profile.is_some()
            || self.sftp.is_some()
    }
}

/// A post-frame action requested by the SFTP overlay layer.
#[derive(Debug, Clone, PartialEq)]
pub enum SftpFrameEvent {
    /// The upload dialog was confirmed (run the duplicate check).
    ConfirmUpload,
    /// The upload dialog was cancelled.
    CancelUpload,
    /// The overwrite dialog was confirmed (upload despite duplicates).
    ConfirmOverwrite,
    /// The overwrite dialog was cancelled.
    CancelOverwrite,
    /// A running upload's toast cancel control was clicked.
    CancelSession(String),
    /// The tab-close guard was confirmed (cancel the tab's uploads, close it).
    ConfirmClose,
    /// The tab-close guard was dismissed (keep the tab open).
    CancelClose,
}

/// Phase-1 placeholder kept for compatibility; routes to the real renderer
/// when a tab exists.
pub fn draw_placeholder(
    ctx: &egui::Context,
    app: &App,
    window_maximized: bool,
    mux_sidebar_opacity: f32,
) -> FrameEvents {
    draw_terminal(ctx, app, window_maximized, mux_sidebar_opacity)
}

/// Draw the active tab. If no tabs exist, draws a hint message. The
/// caller is responsible for applying the returned events (if any) —
/// title-bar actions hit `winit::Window` directly, tab-bar actions go
/// through `App::apply_tab_event` post-frame.
///
/// `window_maximized` is forwarded to the CSD title bar so it can
/// swap the maximize glyph for the restore (overlapped-squares) one
/// when the window is already maximized. `mux_sidebar_opacity` (task0002)
/// is this frame's already-resolved overlay-card whole-card opacity
/// multiplier (`App::resolve_mux_sidebar_opacity`, called by
/// `window_host::render` before this pass since this function only holds
/// `&App`) — threaded into the overlay draw call below; the persistent
/// variant ignores it (AC-8/FR10).
pub fn draw_terminal(
    ctx: &egui::Context,
    app: &App,
    window_maximized: bool,
    mux_sidebar_opacity: f32,
) -> FrameEvents {
    // Per-frame theme seeded from settings (font_size_pt + cursor
    // style). Active-tab OSC mutations live on `Tab::theme`; layering
    // those on top of the settings-derived base lets OSC 4/10/11/12/22
    // re-skin the running session without losing the user-configured
    // font size. Falls back to the settings-only base when no tab is
    // attached yet (initial frame).
    let theme = match app.active_tab() {
        Some(tab) => tab.theme.lock().clone(),
        None => Theme::from_settings(app.settings.as_ref()),
    };

    // Custom CSD title bar — sits above everything else so its
    // glyph buttons stay clickable regardless of tab / status state.
    // The window runs with `with_decorations(false)`, so without this
    // there would be no close / minimize / maximize affordance.
    let icon = app_icon::texture_id(ctx);
    let title_event = crate::ui::title_bar::draw(ctx, "eMterm", window_maximized, icon);

    // agent-badge-emoji-distinction task0001 Design 3: the emoji-resource
    // bundle (glyph-rasterizer handle + font fallback chain + emoji
    // texture cache handle) is built once per frame BEFORE the tab-bar
    // draw so the tab bar, both mux-sidebar variants, and the status bar
    // all consume the SAME handles this frame.
    let emoji_resources = crate::ui::emoji_cache::EmojiResources {
        rasterizer: app.font_rasterizer.as_ref(),
        fallback: &app.font_fallback,
        cache: &app.emoji_texture_cache,
    };

    // Phase 4-B: real tab bar widget. We build a lightweight view-
    // model from the live tabs vector once per frame.
    let items: Vec<crate::ui::tab_bar::TabBarItem> = app
        .tabs
        .iter()
        .map(|t| {
            let mut item =
                crate::ui::tab_bar::TabBarItem::new(t.display_title()).with_stable_id(t.stable_id);
            if let Some(name) = &t.mux_session_name {
                item = item.with_mux_session(name.clone());
            }
            // task0005 D1: a mux-attached tab collapses to a single cell
            // labelled `mux: <active window name>` instead of the WebView-
            // parity inline sub-tab expansion — the window list moved to the
            // `ui::mux_sidebar` widget (drawn below). The group is dissolved
            // only at zero windows (`is_group()` false → the `Option` is
            // cleared on the last `PtyExited`), falling back to the plain
            // title.
            if let Some(group) = &t.mux_group {
                if group.is_group() {
                    let name = group
                        .active_window()
                        .map(|w| w.name.clone())
                        .unwrap_or_default();
                    item = item.with_mux_active_window_name(name);
                }
            }
            // `tab_activity_indicator` gates the dot's rendering only;
            // the underlying activity state (and notifications) is
            // tracked regardless — WebView `main.ts` parity.
            item =
                item.with_activity(app.settings.tab_activity_indicator && t.activity.has_activity);
            // task0006 AC-1/AC-2: aggregated agent-status badge across the
            // tab's own status and every pane in its mux group (if any).
            item = item.with_agent_badge(app.agent_status_badge_for(t));
            item
        })
        .collect();
    let mut tab_event = if items.is_empty() || !app.show_tab_bar {
        None
    } else {
        // FR4: read (do not clear) the one-shot scroll-into-view signal here —
        // `draw_terminal` holds `&App` (immutable). The strip consumes it for
        // one frame; `window_host` clears it post-frame where `&mut App` is
        // available.
        crate::ui::tab_bar::draw(
            ctx,
            &items,
            app.active,
            app.scroll_active_tab_into_view(),
            Some(&emoji_resources),
        )
    };

    // task0005 FR2/FR4/FR5: the mux window-sidebar. `mux_sidebar_visibility`
    // gates on settings + the runtime overlay flag + whether the active tab
    // is mux-attached (D1/D2/D3 in IMPLEMENTATION.md). The persistent
    // variant is drawn here (between the tab bar and the central panel) as
    // a RIGHT `SidePanel` (task0006 update) so it reserves grid WIDTH via
    // egui's own panel layout — matching the usable-width reduction
    // `window_host::grid_size` applies on the wgpu side. It never shifts
    // the grid's x-origin (`render::cursor` / `draw_search_highlights`
    // carry no sidebar term). The overlay variant draws after the central
    // panel (below) so it floats over the terminal without affecting
    // layout.
    let sidebar_visibility = app.mux_sidebar_visibility();
    let sidebar_entries: Vec<crate::ui::mux_sidebar::SidebarEntry> = match sidebar_visibility {
        crate::app::MuxSidebarVisibility::Hidden => Vec::new(),
        _ => app
            .active_tab()
            .and_then(|t| t.mux_group.as_ref())
            .map(crate::ui::mux_sidebar::build_entries)
            .unwrap_or_default()
            .into_iter()
            // task0006 AC-1: attach each window's pane badge from
            // `App::agent_status` — `build_entries` stays pure over the mux
            // group alone.
            .map(|mut e| {
                e.badge = app.agent_status_pane_badge(e.pane_id);
                e
            })
            .collect(),
    };
    let sidebar_width = crate::ui::mux_sidebar::sidebar_width(ctx.screen_rect().width());
    if sidebar_visibility == crate::app::MuxSidebarVisibility::Persistent {
        // The sidebar's click result routes into the SAME
        // `TabEvent::MuxSwitch` application path the (now-collapsed) inline
        // sub-tab click used (`App::apply_tab_event`'s existing arm) — the
        // widget itself never sends mux messages (task0005 AC-2).
        let outcome = crate::ui::mux_sidebar::draw(
            ctx,
            &sidebar_entries,
            crate::ui::mux_sidebar::Placement::Persistent,
            sidebar_width,
            mux_sidebar_opacity,
            Some(&emoji_resources),
        );
        if let Some(window) = outcome.switch_to_window {
            if tab_event.is_none() {
                tab_event = Some(crate::ui::TabEvent::MuxSwitch {
                    tab: app.active,
                    window,
                });
            }
        }
    }

    // Phase 4-D: status-bar panel. Inserted before the central panel
    // (egui sizes top/bottom panels first, then the central panel
    // takes the remaining rect). The widget itself decides top vs
    // bottom from settings.
    let status_vm = app.status_bar_view_model();
    crate::ui::status_bar::draw(ctx, &status_vm, Some(&emoji_resources));

    let mut scroll_to = None;
    egui::CentralPanel::default()
        // Phase 4-H (FR12): the central panel no longer paints the cell
        // background — `TerminalGridPass` clears the swapchain to the
        // theme background and emits per-cell solid quads where the SGR
        // bg differs. Using `Color32::TRANSPARENT` keeps egui's overlay
        // (cursor + IME preedit underline) on top of the wgpu-rendered
        // cells without painting an opaque rect that would hide them.
        .frame(egui::Frame::none().fill(Color32::TRANSPARENT))
        .show(ctx, |ui| {
            if let Some(tab) = app.active_tab() {
                let core = tab.core.lock();
                draw_cursor(ui, &core, &theme, app);
                // Fold summary overlays: full-width tinted bars with the
                // `▶ command  — N lines` text over each summary row whose
                // cells `collect_cell_inputs` left blank. No-op when no
                // region is collapsed (`app.fold_layout()` is `None`).
                draw_fold_summaries(ui, app);
                // Search match highlights: translucent rects over the
                // matched cells (current match amber, others yellow),
                // painted on the same egui overlay layer as the cursor +
                // bell flash. Read-only over `app.search`.
                draw_search_highlights(ui, &core, app);
                // Preedit rendering is owned by the wgpu cell pass via
                // `apply_preedit_overlay` (reverse-video cells). The
                // legacy egui underline overlay was removed so it
                // doesn't stack on top of the inline reverse-video
                // composition cells.
                let scrollbar_view = crate::ui::scrollbar::ScrollbarView {
                    mode: app.settings.show_scrollbar,
                    scrollback_len: core.get_scrollback_length(),
                    viewport_rows: core.rows() as u32,
                    scroll_offset: app.scroll_offset(),
                    alt_screen: app.alt_screen,
                };
                drop(core);
                scroll_to = crate::ui::scrollbar::draw(ui, &scrollbar_view);
            } else {
                ui.colored_label(Color32::LIGHT_GRAY, "no tab — shell may have exited");
            }
            // Visual bell: approximate the WebView's 150 ms
            // `brightness(2) → 1` ease-out (`.terminal-bell-flash`,
            // src/styles.css) with a white overlay whose alpha decays
            // quadratically over the terminal area. `about_to_wait`
            // polls `App::needs_bell_repaint` to keep frames coming
            // while the flash is live.
            if let Some(t) = app.visual_bell_progress() {
                let fade = (1.0 - t) * (1.0 - t); // ease-out decay
                let alpha = (BELL_FLASH_MAX_ALPHA * fade * 255.0) as u8;
                ui.painter().rect_filled(
                    ui.max_rect(),
                    0.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
                );
            }
        });

    // task0005 D3: the overlay sidebar draws after the central panel so it
    // floats over the terminal area without affecting grid geometry (zero
    // inset — NFR1). Same click-routing contract as the persistent variant
    // above.
    if sidebar_visibility == crate::app::MuxSidebarVisibility::Overlay {
        let outcome = crate::ui::mux_sidebar::draw(
            ctx,
            &sidebar_entries,
            crate::ui::mux_sidebar::Placement::Overlay,
            sidebar_width,
            mux_sidebar_opacity,
            Some(&emoji_resources),
        );
        if let Some(window) = outcome.switch_to_window {
            if tab_event.is_none() {
                tab_event = Some(crate::ui::TabEvent::MuxSwitch {
                    tab: app.active,
                    window,
                });
            }
        }
    }

    // Keep blinking cursors animating. egui only repaints on demand, so we
    // schedule a wake-up at the half-period. Frame-level skip in
    // `window_host::render` still kicks in when `dirty_rows_this_frame`
    // returns empty (i.e. cursor blink-disabled or cursor row never
    // entered the dirty set this frame), so this only wakes us up when
    // we genuinely need to re-evaluate.
    if let Some(tab) = app.active_tab() {
        let core = tab.core.lock();
        if core.get_cursor_blink() {
            ctx.request_repaint_after(Duration::from_millis(BLINK_HALF_MS as u64));
        }
    }

    // Status-bar periodic redraw is now provider-owned: each
    // Provider that needs periodic updates (TimeProvider timer
    // thread, GitBranch / Command worker threads) holds an
    // `Arc<WakeFn>` and invokes it directly. Event-driven providers
    // (CwdProvider) wake on OSC 7 receipt. `egui::Context::
    // request_repaint_after` does not bridge to winit so the prior
    // `request_repaint_after(Duration::from_secs(1))` floor was a
    // no-op in release builds — see SPEC.md Notes section.

    FrameEvents {
        title: title_event,
        tab: tab_event,
        scroll_to,
        // The search overlay is drawn separately by `draw_search_overlay`
        // (it needs `&mut App`); `draw_terminal` never populates this.
        search: None,
        // Likewise drawn separately by `draw_profile_selector_overlay`.
        profile: None,
        // Likewise drawn separately by `draw_sftp_overlay`.
        sftp: None,
    }
}

/// Draw the modal profile selector when it is open. Runs as a separate
/// pass after `draw_terminal` (same `&mut App` split as
/// [`draw_search_overlay`]) so the modal floats above the chrome.
mod overlays;
pub use overlays::*;

/// Walk the terminal grid and build a `Vec<CellInput>` suitable for
/// [`crate::render::terminal_grid_pass::TerminalGridPass::prepare`].
///
/// Phase 4-H (FR12): the cell loop that used to call `painter.text()` /
/// `painter.line_segment()` / `painter.rect_filled()` now emits per-cell
/// inputs consumed by the custom wgpu pass. Selection is encoded via the
/// existing fg/bg swap in [`resolve_cell_style_from_packed`] (no separate
/// selection quad).
///
/// Grid instance data is a pure function of terminal content + theme +
/// selection/hover/search state — never of cursor position, blink phase,
/// or window focus. The filled block cursor is drawn as an egui overlay
/// by [`cursor::draw_block_cursor`] instead (see `draw_cursor`); this
/// function no longer takes a cursor parameter.
///
/// `scroll_offset` is the active tab's scrollback offset in rows (`0` =
/// live tail). When non-zero the renderer reads scrollback rows for the
/// portion of the viewport that has scrolled below the live region. The
/// absolute-row model matches [`crate::app`] and `draw_search_highlights`:
/// absolute rows `0..scrollback_len` are scrollback (oldest first) and
/// `scrollback_len..` are the live viewport. The top visible absolute row
/// is `scrollback_len - scroll_offset`. `scroll_offset == 0` reproduces the
/// original live-only output exactly.
///
/// `fold_layout` is `Some` only when the active tab has at least one
/// collapsed fold region (the caller gates on
/// [`crate::fold::FoldManager::has_collapsed_regions`], mirroring the
/// WebView's `getCollapsedRegions().length > 0`). When present, each screen
/// row's *actual* buffer row comes from the layout
/// ([`crate::fold::FoldLayout::rows`]) instead of the linear
/// `visible_start + row`, and summary rows emit no cells (the summary text is
/// drawn as an egui overlay by [`draw_fold_summaries`]). When `None` the
/// linear scrollback path above is used unchanged, so the non-folded /
/// existing behavior is bit-for-bit identical.
///
/// `only_rows` (task0003 FR3/FR4): when `Some(rows)`, only the given screen
/// rows are walked — the per-row instance cache rebuild path in
/// `render::terminal_grid_pass` uses this to avoid re-reading `core` for
/// rows a frame did not mark dirty. `rows` must be sorted ascending
/// (`App::dirty_rows_this_frame` already returns a sorted, deduplicated
/// `Vec`); out-of-range entries (`>= core.rows()`) are skipped rather than
/// panicking. `None` walks every row `0..core.rows()` — the existing
/// full-grid behavior, reproduced bit-for-bit so pre-existing callers are
/// unaffected.
// The renderer hot path resolves a cell from its core + theme + selection +
// width policy + cursor + hover + scroll + fold layout; these are distinct
// per-frame inputs read at the single `window_host::render` call site, so a
// flat signature is kept rather than introducing a params struct for one
// caller (mirroring `Tab::spawn_shell`).
#[allow(clippy::too_many_arguments)]
pub fn collect_cell_inputs(
    core: &TerminalCore,
    theme: &Theme,
    selection: Option<&Selection>,
    width_mode: AmbiguousWidthMode,
    hovered_link: Option<&[(u16, u16, u16)]>,
    scroll_offset: u32,
    fold_layout: Option<&crate::fold::FoldLayout>,
    only_rows: Option<&[u16]>,
) -> Vec<CellInput> {
    let cols = core.cols();
    let rows = core.rows();
    let bg_default = rgb_to_egui(theme.bg);

    // task0003: walk only the requested row subset when the caller supplies
    // one; otherwise fall back to the full `0..rows` walk (the pre-existing
    // behavior every caller before task0003 relied on). `full_range` is
    // declared unconditionally so the `None` arm's `Vec` outlives the
    // `row_iter` borrow below.
    let full_range: Vec<u16>;
    let row_iter: &[u16] = match only_rows {
        Some(subset) => subset,
        None => {
            full_range = (0..rows).collect();
            &full_range
        }
    };
    let mut out: Vec<CellInput> = Vec::with_capacity((cols as usize) * row_iter.len());

    let scrollback_len = core.get_scrollback_length();
    // Top visible absolute row (saturating: the offset can momentarily
    // exceed the live length while content scrolls under a pinned viewport).
    let visible_start = scrollback_len.saturating_sub(scroll_offset);

    for &row in row_iter {
        if row >= rows {
            // Defensive: a stale/out-of-range row in `only_rows` (e.g. a
            // dirty set computed just before a shrink-resize) contributes
            // no cells rather than reading out of bounds.
            continue;
        }
        // Resolve the absolute buffer row this screen row shows. With a
        // fold layout the mapping is non-linear (collapsed bodies are
        // hidden, summary rows draw no cells); without one it is the linear
        // scrollback model. `continue` on a summary row leaves the cell
        // grid empty there so `draw_fold_summaries` can paint the overlay.
        let abs_row = match fold_layout {
            Some(layout) => match layout.rows.get(row as usize) {
                Some(crate::fold::FoldRowKind::Cells { actual_line }) => *actual_line,
                // Summary rows (and rows past the layout, which cannot occur
                // since `rows == viewport_rows`) emit no cells.
                _ => continue,
            },
            None => visible_start + row as u32,
        };
        if abs_row < scrollback_len {
            // Scrollback row: decode the styled cells once and emit one
            // `CellInput` per kept (width > 0) cell. `term_core` already
            // drops the width-0 trailing halves of wide glyphs, so the
            // resulting column sequence matches the viewport iterator's
            // "advance past wide cells" behavior (see
            // `search::build_logical_lines`).
            let cells = core.get_scrollback_row_cells_styled(abs_row);
            let mut col = 0u16;
            for cell in cells {
                if col >= cols {
                    break;
                }
                // Selection is absolute-row-based: this scrollback cell is
                // tested against its own absolute row (`abs_row`), so the
                // highlight tracks the buffer content as the viewport
                // scrolls rather than staying pinned to a screen row.
                let selected = selection.map(|s| s.contains(abs_row, col)).unwrap_or(false);
                let mut style =
                    resolve_cell_style_from_packed(theme, cell.fg, cell.bg, cell.flags, selected);
                if cell_in_hovered_link(hovered_link, row, col) {
                    style.underline = true;
                }
                let cell_width_cells = visible_width(&cell.glyph, width_mode);
                out.push(CellInput {
                    col,
                    row,
                    width_cells: cell_width_cells.max(1),
                    glyph: cell.glyph,
                    fg_rgba: color32_to_rgba(style.fg),
                    bg_rgba: color32_to_rgba(style.bg),
                    underline: style.underline,
                    strikethrough: style.strikethrough,
                    draw_background: style.bg != bg_default,
                    bg_extend_below: 0.0,
                    // Horizontal advance-based shrink-to-fit
                    // (ambiguous-width-rendering SPEC FR2). Glyphs whose
                    // design advance exceeds their cell footprint are
                    // scaled down so they fit (e.g. U+273B ✻ rasterized
                    // from a CJK Gothic fallback at ~1.5 em). Monospace
                    // ascii has advance == cell_w → sx = 1.0 (no
                    // shrink), so Latin AA overhang from hinted bitmaps
                    // keeps its existing subpixel-clip path.
                    fit: GlyphFit::HorizontalOnly,
                    bold: style.bold,
                });
                col = col.saturating_add(cell_width_cells.max(1) as u16);
            }
            continue;
        }

        // Live viewport row: `abs_row - scrollback_len` is the live-ring row
        // whose content we read. The cell still *appears* at the on-screen
        // `row`, so hover / cursor are addressed by `row` (their
        // viewport-coordinate space), but the selection is keyed off the
        // cell's absolute row (`abs_row`) so it tracks the buffer content as
        // the viewport scrolls. When `scroll_offset == 0` these coincide,
        // reproducing the original live-only output exactly.
        let content_row = (abs_row - scrollback_len) as u16;
        let mut col = 0u16;
        while col < cols {
            let flags = core.get_cell_flags(col, content_row);
            let packed_fg = core.get_cell_fg(col, content_row);
            let packed_bg = core.get_cell_bg(col, content_row);
            let selected = selection.map(|s| s.contains(abs_row, col)).unwrap_or(false);
            let mut style =
                resolve_cell_style_from_packed(theme, packed_fg, packed_bg, flags, selected);
            // Hover underline: a cell inside the hovered link's physical
            // span gets `underline = true` regardless of its SGR state.
            // Matches the WebView build's hover-only underline (no Ctrl
            // required to underline; Ctrl only opens the link).
            if cell_in_hovered_link(hovered_link, row, col) {
                style.underline = true;
            }
            let ch = core.get_cell_char(col, content_row);
            let cell_width_cells = visible_width(&ch, width_mode);

            out.push(CellInput {
                col,
                row,
                width_cells: cell_width_cells.max(1),
                glyph: ch,
                fg_rgba: color32_to_rgba(style.fg),
                bg_rgba: color32_to_rgba(style.bg),
                underline: style.underline,
                strikethrough: style.strikethrough,
                draw_background: style.bg != bg_default,
                bg_extend_below: 0.0,
                // Advance-based shrink-to-fit (SPEC FR2): see the
                // matching comment in the scrollback branch above.
                fit: GlyphFit::HorizontalOnly,
                bold: style.bold,
            });

            col = col.saturating_add(cell_width_cells.max(1) as u16);
        }
    }
    out
}

/// Whether physical cell `(row, col)` falls inside any span of the
/// hovered link. Each span is `(row, col_start, col_end)` with
/// `col_start <= col < col_end`.
fn cell_in_hovered_link(hovered_link: Option<&[(u16, u16, u16)]>, row: u16, col: u16) -> bool {
    match hovered_link {
        Some(spans) => spans
            .iter()
            .any(|&(r, cs, ce)| r == row && col >= cs && col < ce),
        None => false,
    }
}

/// Overlay an in-progress IME preedit composition onto an existing
/// `Vec<CellInput>` produced by [`collect_cell_inputs`].
///
/// Replaces the cells starting at `anchor` with one entry per character
/// of `text`, drawn in reverse video (theme.fg as background, theme.bg
/// as foreground) so composition stands out against the surrounding
/// committed text. Ambiguous-width characters (e.g. ▽ U+25BD) are
/// forced to a 1-cell footprint with their glyphs scaled to fit.
/// Wraps to the next row when the composition exceeds the right edge.
///
/// `bg_extend_below_px` extends the reverse-video bg quad downward by
/// the given physical-pixel amount so glyph descenders that rasterize
/// past `cell_h` are covered by the inverted background. Caller
/// supplies a value already scaled by `pixels_per_point`.
pub fn apply_preedit_overlay(
    cells: &mut Vec<CellInput>,
    anchor: crate::ime::preedit::Anchor,
    text: &str,
    theme: &Theme,
    cols: u16,
    rows: u16,
    bg_extend_below_px: f32,
) {
    if text.is_empty() || cols == 0 || rows == 0 {
        return;
    }
    let bg_default = rgb_to_egui(theme.bg);
    let fg_preedit = rgb_to_egui(theme.bg);
    let bg_preedit = rgb_to_egui(theme.fg);
    let bg_extend_below = bg_extend_below_px.max(0.0);

    let mut row = anchor.row.min(rows.saturating_sub(1));
    let mut col = anchor.col.min(cols.saturating_sub(1));
    let mut overlay: Vec<CellInput> = Vec::new();

    // Split on extended grapheme cluster boundaries so codepoint sequences
    // that compose into a single visual glyph (emoji + VS-16, ZWJ
    // sequences, regional indicator pairs, combining marks, …) land in
    // one cell. Without this, e.g. "⚠️" (U+26A0 + U+FE0F) renders as the
    // bare warning sign in one cell followed by an invisible variation
    // selector glyph in the next.
    use unicode_segmentation::UnicodeSegmentation;
    for cluster in text.graphemes(true) {
        if row >= rows {
            break;
        }
        let s: String = cluster.to_string();
        // Force ambiguous-width chars (e.g. ▽) to 1 cell so the
        // composition footprint matches the user's visual expectation
        // of "1 character = 1 cell" during preedit. `visible_width`
        // already upgrades VS-16-bearing clusters to 2 cells.
        let w = visible_width(&s, AmbiguousWidthMode::Narrow).max(1) as u16;
        if col + w > cols {
            row = row.saturating_add(1);
            col = 0;
            if row >= rows {
                break;
            }
        }
        overlay.push(CellInput {
            col,
            row,
            width_cells: w as u8,
            glyph: s,
            fg_rgba: color32_to_rgba(fg_preedit),
            bg_rgba: color32_to_rgba(bg_preedit),
            underline: false,
            strikethrough: false,
            draw_background: bg_preedit != bg_default,
            bg_extend_below,
            // IME preedit needs the full both-axis clamp so CJK
            // descenders past `cell_h` stay inside the highlight bg.
            fit: GlyphFit::Both,
            bold: false,
        });
        col = col.saturating_add(w);
    }

    if overlay.is_empty() {
        return;
    }

    // Remove any existing cells whose footprint overlaps a preedit cell
    // so the same column isn't drawn twice (the wgpu pass instances each
    // CellInput in submission order without a depth test).
    use std::collections::HashSet;
    let mut occupied: HashSet<(u16, u16)> = HashSet::new();
    for o in &overlay {
        for k in 0..o.width_cells.max(1) as u16 {
            occupied.insert((o.row, o.col.saturating_add(k)));
        }
    }
    cells.retain(|c| {
        for k in 0..c.width_cells.max(1) as u16 {
            if occupied.contains(&(c.row, c.col.saturating_add(k))) {
                return false;
            }
        }
        true
    });
    cells.extend(overlay);
}

/// Pack an `egui::Color32` (already non-premultiplied RGBA8) into the
/// little-endian `[r, g, b, a]` layout the `CellInput` carries. The shader
/// re-expands this via `unpack4x8unorm`.
pub(in crate::render) fn color32_to_rgba(c: Color32) -> [u8; 4] {
    [c.r(), c.g(), c.b(), c.a()]
}

/// Resolve a cell's paint style from its packed `(fg, bg, flags)` triple and a
/// pre-computed selection flag. Shared by [`collect_cell_inputs`]'s live
/// viewport path (reading `get_cell_fg/bg/flags`) and its scrollback path
/// (reading the same packed representation from `term_core::ScrollbackCell`),
/// so both routes apply identical reverse / bold-brighten / selection / dim /
/// hidden handling.
///
/// `selected` is computed by the caller against the cell's on-screen viewport
/// row (the PoC selection model is viewport-coordinate-based and has no
/// absolute-row notion; see the selection coordinate-system note in `app.rs`).
pub(in crate::render) fn resolve_cell_style_from_packed(
    theme: &Theme,
    packed_fg: u32,
    packed_bg: u32,
    flags: u16,
    selected: bool,
) -> CellStyle {
    let bold = (flags & STYLE_BOLD) != 0;
    let dim = (flags & STYLE_DIM) != 0;
    let italic = (flags & STYLE_ITALIC) != 0;
    let underline = (flags & STYLE_UNDERLINE) != 0;
    // STYLE_BLINK is rendered statically today; cursor blink owns the
    // wake-up cadence. A future sub-phase can multiplex per-cell blink
    // off the same blink_started clock if needed.
    let _blink = (flags & STYLE_BLINK) != 0;
    let reverse = (flags & STYLE_REVERSE) != 0;
    let hidden = (flags & STYLE_HIDDEN) != 0;
    let strikethrough = (flags & STYLE_STRIKETHROUGH) != 0;

    // Reverse, layer 1 — packed-level swap: BEFORE bold-brighten / decoding
    // so the bold-brighten promotion sees the perceived foreground (FR7
    // in the WebView build: bold-brighten is foreground-only and applies
    // *after* reverse). This swap alone is sufficient for indexed /
    // truecolor cells: `packed_to_egui` returns `Some(...)` for those tags
    // and the fallback below is never consumed.
    let (effective_fg_packed, effective_bg_packed) = if reverse {
        (packed_bg, packed_fg)
    } else {
        (packed_fg, packed_bg)
    };

    // Bold-brightens: when `settings.bold_brightens_ansi_colors` is on
    // and the cell's foreground is an indexed color in `0..8`, promote
    // it to the bright variant (`idx + 8`). Truecolor / default-tag
    // foregrounds are untouched. Mirrors
    // `attributes.ts::getEffectiveForeground` in the WebView build.
    let effective_fg_packed = if bold && theme.bold_brightens_ansi_colors {
        bold_brighten_packed(effective_fg_packed)
    } else {
        effective_fg_packed
    };

    // Reverse, layer 2 — fallback swap: rescues the both-DEFAULT case.
    // `packed_to_egui` returns `None` for the `Default` tag, so without
    // this the `unwrap_or_else` arms would re-substitute the unswapped
    // `theme.fg` / `theme.bg`, turning the layer-1 swap into a NOP for
    // `\e[7m` on bare default-color cells. Selecting the fallback per
    // `reverse` ensures `theme.fg` / `theme.bg` swap takes effect. Indexed
    // / truecolor cells are unaffected because `packed_to_egui` returns
    // `Some(...)` and the fallback is never consumed.
    let (fg_fallback, bg_fallback) = if reverse {
        (theme.bg, theme.fg)
    } else {
        (theme.fg, theme.bg)
    };

    let mut fg = packed_to_egui(effective_fg_packed, fg_fallback, theme)
        .unwrap_or_else(|| rgb_to_egui(fg_fallback));
    let mut bg = packed_to_egui(effective_bg_packed, bg_fallback, theme)
        .unwrap_or_else(|| rgb_to_egui(bg_fallback));

    // Selection: invert again on top of any reverse already in effect.
    if selected {
        std::mem::swap(&mut fg, &mut bg);
    }

    // Dim: 50% alpha against the cell's background. We approximate by
    // pulling fg halfway toward bg; this preserves opacity so subsequent
    // overlay primitives (underline / strikethrough) still respect the
    // dim look without alpha-compositing tricks.
    if dim {
        fg = blend_toward(fg, bg, 0.5);
    }

    // Hidden / conceal: clamp fg to bg so the glyph is invisible. We do
    // this last so reverse / selection still produce the expected
    // background swatch.
    if hidden {
        fg = bg;
    }

    CellStyle {
        fg,
        bg,
        bold,
        italic,
        underline,
        strikethrough,
    }
}

/// Linear blend two RGBA colors. `t = 0.0` returns `a`; `t = 1.0` returns
/// `b`. Used for the dim attribute fallback.
pub(in crate::render) fn blend_toward(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| -> u8 {
        let f = x as f32 + (y as f32 - x as f32) * t;
        f.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        a.a(),
    )
}

/// Compute display width of a grapheme under the active ambiguous-width
/// policy. Returns at least 1 so the iterator never wedges.
pub(in crate::render) fn visible_width(ch: &str, mode: AmbiguousWidthMode) -> u8 {
    let mut chars = ch.chars();
    let cp = chars.next().map(|c| c as u32).unwrap_or(0);
    if cp == 0 {
        return 1;
    }
    // Variation Selectors override the bare-codepoint presentation:
    // VS-15 (U+FE0E) forces text presentation (width 1) and has absolute
    // precedence — once seen, the answer is known immediately and we return
    // early. VS-16 (U+FE0F) forces emoji presentation (width 2) but we must
    // continue scanning in case a later VS-15 overrides it. Mirrors
    // `term_core::print_handler::flush_grapheme_buffer` exactly — without
    // this, the rendered footprint drifts from the cluster width term_core
    // reserved.
    let mut has_fe0f = false;
    for c in chars {
        match c as u32 {
            0xFE0E => return 1,
            0xFE0F => has_fe0f = true,
            _ => {}
        }
    }
    if has_fe0f {
        return 2;
    }
    if is_ambiguous_width(cp) {
        return mode.width_for_ambiguous();
    }
    let w = char_width(cp);
    w.max(1)
}

/// Decode `term_core::cell::PackedColor::to_u32()` into an egui color.
/// Returns `None` only for the `Default` tag, in which case the caller
/// substitutes the active palette fallback. `tag` legend:
/// `0`=default, `1`=indexed (the index lives in `r`), `2`=truecolor RGB.
/// Promote indexed-color packed value 0-7 → 8-15 (xterm "bold brightens"
/// behavior). Truecolor / default-tag values pass through unchanged so
/// the caller can apply this unconditionally to bolded foregrounds.
pub(in crate::render) fn bold_brighten_packed(packed: u32) -> u32 {
    let tag = (packed >> 24) as u8;
    if tag != 1 {
        return packed;
    }
    let idx = (packed >> 16) as u8;
    if idx >= 8 {
        return packed;
    }
    // Clear the old index byte and write idx+8 back into the same slot.
    (packed & 0xFF00_FFFF) | ((idx as u32 + 8) << 16)
}

pub(in crate::render) fn packed_to_egui(packed: u32, _fallback: Rgb, theme: &Theme) -> Option<Color32> {
    let tag = (packed >> 24) as u8;
    let r = (packed >> 16) as u8;
    let g = (packed >> 8) as u8;
    let b = packed as u8;
    match tag {
        0 => None,
        1 => Some(rgb_to_egui(palette_lookup(theme, r))),
        2 => Some(Color32::from_rgb(r, g, b)),
        _ => None,
    }
}

pub(in crate::render) fn rgb_to_egui(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// Resolve a palette index to an `Rgb`. Indices 0..16 come from the
/// active theme's 16-color palette (which OSC 4 / OSC 104 will later
/// mutate); 16..256 use the standard xterm 6x6x6 cube + grayscale ramp.
fn palette_lookup(theme: &Theme, idx: u8) -> Rgb {
    if (idx as usize) < 16 {
        theme.palette16[idx as usize]
    } else {
        palette_256(idx)
    }
}

/// Standard xterm 256-color palette mapping for indices 16..255.
fn palette_256(idx: u8) -> Rgb {
    if idx < 16 {
        Theme::default().palette16[idx as usize]
    } else if idx < 232 {
        // 6x6x6 color cube.
        let i = idx - 16;
        let r = i / 36;
        let g = (i % 36) / 6;
        let b = i % 6;
        let to_byte = |n: u8| -> u8 { if n == 0 { 0 } else { 55 + n * 40 } };
        Rgb(to_byte(r), to_byte(g), to_byte(b))
    } else {
        // Grayscale ramp.
        let n = idx - 232;
        let v = 8 + n * 10;
        Rgb(v, v, v)
    }
}

#[cfg(test)]
mod tests;
