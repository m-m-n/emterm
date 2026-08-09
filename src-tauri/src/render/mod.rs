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

mod cell_inputs;
pub use cell_inputs::*;

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

mod overlays;
pub use overlays::*;

#[cfg(test)]
mod tests;
