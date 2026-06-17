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
pub fn draw_placeholder(ctx: &egui::Context, app: &App, window_maximized: bool) -> FrameEvents {
    draw_terminal(ctx, app, window_maximized)
}

/// Draw the active tab. If no tabs exist, draws a hint message. The
/// caller is responsible for applying the returned events (if any) —
/// title-bar actions hit `winit::Window` directly, tab-bar actions go
/// through `App::apply_tab_event` post-frame.
///
/// `window_maximized` is forwarded to the CSD title bar so it can
/// swap the maximize glyph for the restore (overlapped-squares) one
/// when the window is already maximized.
pub fn draw_terminal(ctx: &egui::Context, app: &App, window_maximized: bool) -> FrameEvents {
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
            // FR1 (WebView parity): a mux-attached tab renders one sub-tab per
            // window (`[N] name`) instead of the plain title, whenever the
            // group holds at least one window. The group is dissolved only at
            // zero windows (`is_group()` false → the `Option` is cleared on the
            // last `PtyExited`), falling back to the plain title.
            if let Some(group) = &t.mux_group {
                if group.is_group() {
                    item = item.with_mux_cells(crate::ui::tab_bar::mux_group_render_model(group));
                }
            }
            // `tab_activity_indicator` gates the dot's rendering only;
            // the underlying activity state (and notifications) is
            // tracked regardless — WebView `main.ts` parity.
            item =
                item.with_activity(app.settings.tab_activity_indicator && t.activity.has_activity);
            item
        })
        .collect();
    let tab_event = if items.is_empty() || !app.show_tab_bar {
        None
    } else {
        crate::ui::tab_bar::draw(ctx, &items, app.active)
    };

    // Phase 4-D: status-bar panel. Inserted before the central panel
    // (egui sizes top/bottom panels first, then the central panel
    // takes the remaining rect). The widget itself decides top vs
    // bottom from settings.
    let status_vm = app.status_bar_view_model();
    let emoji_resources = crate::ui::status_bar::EmojiResources {
        rasterizer: app.font_rasterizer.as_ref(),
        fallback: &app.font_fallback,
        cache: &app.emoji_texture_cache,
    };
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
pub fn draw_profile_selector_overlay(
    ctx: &egui::Context,
    app: &mut App,
) -> Option<crate::ui::profile_selector::ProfileSelectorEvent> {
    if !app.profile_selector.visible {
        return None;
    }
    let (selector_title, new_tab_title, global_label, badge) = match app.locale {
        crate::i18n::Locale::Ja => ("プロファイル", "新しいタブ", "グローバル設定", "デフォルト"),
        crate::i18n::Locale::En => ("Profiles", "New Tab", "Global Settings", "Default"),
    };
    let mut rows: Vec<crate::ui::profile_selector::ProfileRow<'_>> = Vec::new();
    // New-tab chooser mode (`+` button): a synthetic "Global Settings"
    // row leads the list and the dialog is titled "New Tab" (WebView
    // `handleNewTabClick` parity).
    let title = if app.profile_selector.include_global {
        rows.push(crate::ui::profile_selector::ProfileRow {
            name: global_label,
            shell_path: "",
            is_default: false,
        });
        new_tab_title
    } else {
        selector_title
    };
    rows.extend(
        app.settings
            .profiles
            .iter()
            .map(|p| crate::ui::profile_selector::ProfileRow {
                name: &p.name,
                shell_path: &p.shell_path,
                is_default: p.is_default,
            }),
    );
    crate::ui::profile_selector::draw(ctx, &mut app.profile_selector, &rows, title, badge)
}

/// Draw the SFTP drop overlay, upload/overwrite dialogs, and progress toasts,
/// returning any post-frame action. The `now` frame time drives the toast
/// auto-dismiss (monotonic, wall-clock-free).
pub fn draw_sftp_overlay(ctx: &egui::Context, app: &mut App) -> Option<SftpFrameEvent> {
    use crate::sftp::ui::HoverOverlay;

    let loc = app.locale;
    let t = |ja: &'static str, en: &'static str| match loc {
        crate::i18n::Locale::Ja => ja,
        crate::i18n::Locale::En => en,
    };

    let mut event: Option<SftpFrameEvent> = None;

    // ── Drop hover overlay ───────────────────────────────────────
    if let Some(hover) = &app.sftp_ui.hover {
        let msg = match hover {
            HoverOverlay::SshUpload => t("ドロップしてアップロード", "Drop to upload"),
            HoverOverlay::Paste => t("ドロップしてパスを貼り付け", "Drop to paste paths"),
        };
        egui::Area::new(egui::Id::new("sftp_drop_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(msg);
                });
            });
    }

    // ── Upload confirmation dialog ───────────────────────────────
    if let Some(dialog) = app.sftp_ui.upload_dialog.clone() {
        egui::Window::new(t("アップロードの確認", "Confirm upload"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(t("アップロードするファイル:", "Files to upload:"));
                for p in &dialog.paths {
                    ui.label(p.to_string_lossy());
                }
                ui.separator();
                ui.label(format!(
                    "{} {}",
                    t("送信先:", "Destination:"),
                    dialog.remote_dir
                ));
                ui.horizontal(|ui| {
                    if ui.button(t("アップロード", "Upload")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        event = Some(SftpFrameEvent::ConfirmUpload);
                    }
                    if ui.button(t("キャンセル", "Cancel")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        event = Some(SftpFrameEvent::CancelUpload);
                    }
                });
            });
    }

    // ── Overwrite confirmation dialog ────────────────────────────
    if let Some(dialog) = app.sftp_ui.overwrite_dialog.clone() {
        egui::Window::new(t("上書きの確認", "Confirm overwrite"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(t(
                    "次のファイルは既に存在します:",
                    "These files already exist:",
                ));
                for name in &dialog.duplicates {
                    ui.label(name);
                }
                ui.horizontal(|ui| {
                    // SPEC US3/FR6: Cancel is the default focus for the
                    // destructive overwrite dialog. Bare Enter must NOT confirm
                    // the overwrite — both Enter and Escape map to Cancel, and
                    // Overwrite requires an explicit button click.
                    if ui.button(t("上書き", "Overwrite")).clicked() {
                        event = Some(SftpFrameEvent::ConfirmOverwrite);
                    }
                    if ui.button(t("キャンセル", "Cancel")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        event = Some(SftpFrameEvent::CancelOverwrite);
                    }
                });
            });
    }

    // ── Tab-close guard dialog ───────────────────────────────────
    if app.sftp_ui.close_guard.is_some() {
        egui::Window::new(t("タブを閉じますか?", "Close this tab?"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(t(
                    "このタブにはアップロード中のファイルがあります。閉じると中止されます。",
                    "This tab has uploads in progress. Closing will cancel them.",
                ));
                ui.horizontal(|ui| {
                    if ui.button(t("閉じる", "Close")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        event = Some(SftpFrameEvent::ConfirmClose);
                    }
                    if ui.button(t("キャンセル", "Cancel")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        event = Some(SftpFrameEvent::CancelClose);
                    }
                });
            });
    }

    // ── Progress toasts (top-right stack) ────────────────────────
    if !app.sftp_ui.toasts.toasts.is_empty() {
        egui::Area::new(egui::Id::new("sftp_toasts"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
            .show(ctx, |ui| {
                for toast in &app.sftp_ui.toasts.toasts {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let status = sftp_status_label(&toast.status, t);
                            ui.label(format!("{} — {}", toast.file_name, status));
                            // Only running uploads can be cancelled.
                            if matches!(
                                toast.status,
                                crate::sftp::SftpUploadStatus::Preparing
                                    | crate::sftp::SftpUploadStatus::Uploading
                            ) && ui.button(t("中止", "Cancel")).clicked()
                            {
                                event =
                                    Some(SftpFrameEvent::CancelSession(toast.session_id.clone()));
                            }
                        });
                    });
                }
            });
    }

    event
}

fn sftp_status_label(
    status: &crate::sftp::SftpUploadStatus,
    t: impl Fn(&'static str, &'static str) -> &'static str,
) -> &'static str {
    use crate::sftp::SftpUploadStatus as S;
    match status {
        S::Preparing => t("準備中", "Preparing"),
        S::Uploading => t("アップロード中", "Uploading"),
        S::Completed => t("完了", "Completed"),
        S::Failed => t("失敗", "Failed"),
        S::Cancelled => t("中止", "Cancelled"),
    }
}

/// Walk the terminal grid and build a `Vec<CellInput>` suitable for
/// [`crate::render::terminal_grid_pass::TerminalGridPass::prepare`].
///
/// Phase 4-H (FR12): the cell loop that used to call `painter.text()` /
/// `painter.line_segment()` / `painter.rect_filled()` now emits per-cell
/// inputs consumed by the custom wgpu pass. Selection is encoded via the
/// existing fg/bg swap in [`resolve_cell_style_from_packed`] (no separate
/// selection quad).
///
/// `block_cursor_cell` is `Some((col, row))` when a block-shaped cursor
/// is currently visible (blink-on, style=block, terminal-visible). The
/// matching cell gets its fg/bg swapped so it reads as a filled cursor
/// with the glyph in inverted color — matching the WebView build's
/// rendering. Underline / bar cursor shapes stay on the egui overlay
/// side and pass `None` here.
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
    block_cursor_cell: Option<(u16, u16)>,
    hovered_link: Option<&[(u16, u16, u16)]>,
    scroll_offset: u32,
    fold_layout: Option<&crate::fold::FoldLayout>,
) -> Vec<CellInput> {
    let cols = core.cols();
    let rows = core.rows();
    let bg_default = rgb_to_egui(theme.bg);
    let mut out: Vec<CellInput> = Vec::with_capacity((cols as usize) * (rows as usize));

    let scrollback_len = core.get_scrollback_length();
    // Top visible absolute row (saturating: the offset can momentarily
    // exceed the live length while content scrolls under a pinned viewport).
    let visible_start = scrollback_len.saturating_sub(scroll_offset);

    for row in 0..rows {
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
                if block_cursor_cell == Some((col, row)) {
                    std::mem::swap(&mut style.fg, &mut style.bg);
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
            if block_cursor_cell == Some((col, row)) {
                std::mem::swap(&mut style.fg, &mut style.bg);
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
        // of "1 character = 1 cell" during preedit.
        //
        // VS-16 (U+FE0F) explicitly requests emoji presentation; widen
        // to 2 cells so the colored emoji glyph (rather than the bare
        // BW codepoint) gets a wide slot to render in. Mirrors
        // `term_core::print_handler::flush_grapheme_buffer`.
        let has_vs16 = cluster.chars().any(|c| c as u32 == 0xFE0F);
        let w_raw = visible_width(&s, AmbiguousWidthMode::Narrow);
        let w = if has_vs16 { 2u16 } else { w_raw.max(1) as u16 };
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
fn color32_to_rgba(c: Color32) -> [u8; 4] {
    [c.r(), c.g(), c.b(), c.a()]
}

/// Cursor overlay: shape from `get_cursor_style`, blink from
/// `get_cursor_blink` modulated by `App::blink_visible_now`, visibility
/// from `get_cursor_visible`, color from `get_cursor_fg` (falls back to
/// the theme foreground when the field is at default).
fn draw_cursor(ui: &mut egui::Ui, core: &TerminalCore, theme: &Theme, app: &App) {
    if !core.get_cursor_visible() {
        return;
    }
    // Hide the cursor while scrolled back into history — the live cursor
    // position has no meaning over scrollback content. Matches the WebView
    // build, which skips cursor rendering when `scrollOffset !== 0`
    // (canvas-renderer.ts). The wgpu-side block cursor is suppressed at the
    // call site (`window_host::render`); this guards the egui overlay path
    // (underline / bar / hollow block).
    if app.scroll_offset() != 0 {
        return;
    }
    // Blink only when focused. An unfocused window holds the cursor at
    // its "on" phase so the steady outline is always visible — matches
    // WezTerm.
    if app.window_focused {
        let blink_enabled = core.get_cursor_blink();
        if !app.blink_visible_now(blink_enabled) {
            return;
        }
    }

    // Pin the cursor origin to the *same* logical-px anchor the wgpu
    // grid pass uses (see `window_host::cell_metrics_px`: origin =
    // `(LEFT_PAD, TITLE_BAR + TAB_BAR + status_top + TOP_PAD) * scale`).
    // Reading the origin from `ui.min_rect().min` introduced a
    // couple-pixel drift whenever egui's central panel added implicit
    // padding, which made the block cursor visibly overflow the
    // bottom of its cell. Status-bar top inset is omitted here on
    // purpose: the egui cursor overlay is painted inside the central
    // panel whose `min_rect` is already pushed down by the egui
    // top-status panel, so adding the inset would double-count it.
    let pad = app.settings.padding as f32;
    let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
    let origin = Pos2::new(pad, crate::ui::title_bar::TITLE_BAR_HEIGHT + tab_h + pad);
    let painter = ui.painter();

    let cell_w = app.cell_w_logical;
    let cell_h = app.cell_h_logical;
    let cx = origin.x + core.get_cursor_col() as f32 * cell_w;
    let cy = origin.y + core.get_cursor_row() as f32 * cell_h;

    let cursor_color = packed_to_egui(core.get_cursor_fg(), theme.fg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.fg));

    match core.get_cursor_style() {
        // 1 = underline. term_core clamps to 0..=2; once parser routes for
        // DECSCUSR land the mapping (block / underline / bar) becomes
        // observable here.
        1 => {
            let uy = cy + cell_h - 2.0;
            painter.line_segment(
                [Pos2::new(cx, uy), Pos2::new(cx + cell_w, uy)],
                Stroke::new(2.0, cursor_color),
            );
        }
        2 => {
            // Vertical bar at the left edge of the cell.
            painter.line_segment(
                [Pos2::new(cx, cy), Pos2::new(cx, cy + cell_h)],
                Stroke::new(2.0, cursor_color),
            );
        }
        _ => {
            // Block cursor: focused → filled cell (the grid pass swaps
            // fg/bg on the cursor cell, see `collect_cell_inputs`'s
            // `block_cursor_cell` param). Unfocused → hollow outline
            // here, matching WezTerm. egui 0.29 lacks
            // `StrokeKind::Inside`, so a centered 1-px stroke would
            // bleed half a pixel above / below the cell — inset the
            // rect by half the stroke width to keep the visible
            // outline flush with the IME reverse-video box.
            if !app.window_focused {
                const STROKE_W: f32 = 1.0;
                let inset = STROKE_W * 0.5;
                let rect = Rect::from_min_size(
                    Pos2::new(cx + inset, cy + inset),
                    Vec2::new(cell_w - STROKE_W, cell_h - STROKE_W),
                );
                painter.rect_stroke(rect, 0.0, Stroke::new(STROKE_W, cursor_color));
            }
        }
    }
}

/// Current-match highlight fill, the const (premultiplied) form of the
/// WebView's straight-alpha `rgba(230, 150, 30, 0.45)`:
/// `(230·0.45, 150·0.45, 30·0.45, 0.45·255) ≈ (104, 68, 14, 115)`.
const SEARCH_CURRENT_FILL: Color32 = Color32::from_rgba_premultiplied(104, 68, 14, 115);
/// Other-match highlight fill, the const form of `rgba(230, 230, 50, 0.3)`:
/// `(230·0.3, 230·0.3, 50·0.3, 0.3·255) ≈ (69, 69, 15, 77)`.
const SEARCH_OTHER_FILL: Color32 = Color32::from_rgba_premultiplied(69, 69, 15, 77);

/// Paint translucent rectangles over the cells of every search match
/// currently visible in the viewport. The current match uses the amber
/// fill; the rest use the yellow fill — matching the WebView's
/// `renderSearchHighlights` colors.
///
/// Absolute-row → screen-row conversion uses the same scroll model as
/// [`crate::app`]: the top visible absolute row is
/// `scrollback_len - scroll_offset`, so `screen_row = abs_row -
/// (scrollback_len - scroll_offset)`. Segments outside `0..rows` are
/// skipped. Cell rects use the same origin / metrics as [`draw_cursor`]
/// so the highlight lines up with the wgpu-rendered glyphs.
///
/// Fold-aware (`app.fold_layout()` is `Some`): a segment whose absolute row
/// lands inside a *collapsed* region is hidden (skipped), and the screen row
/// is derived through the fold mapping —
/// `screen_row = actual_line_to_display(abs_row) - display_start` — instead
/// of the linear `abs_row - visible_start`. This mirrors the WebView's
/// `renderSearchHighlights` fold branch (`getRegionAtLine` collapsed-skip +
/// `actualLineToDisplay`). Without a layout the linear path runs unchanged.
fn draw_search_highlights(ui: &mut egui::Ui, core: &TerminalCore, app: &App) {
    if !app.search.visible || app.search.matches.is_empty() {
        return;
    }
    let rows = core.rows();
    if rows == 0 {
        return;
    }
    let scrollback_len = core.get_scrollback_length();
    // Top visible absolute row (saturating: offset can momentarily exceed
    // the live length while content scrolls under a pinned viewport).
    let visible_start = scrollback_len.saturating_sub(app.scroll_offset());
    let fold_layout = app.fold_layout();

    // Same origin anchor as draw_cursor (status-bar top inset is handled
    // by the central panel's min_rect, so it is omitted here on purpose).
    let pad = app.settings.padding as f32;
    let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
    let origin = Pos2::new(pad, crate::ui::title_bar::TITLE_BAR_HEIGHT + tab_h + pad);
    let cell_w = app.cell_w_logical;
    let cell_h = app.cell_h_logical;
    let painter = ui.painter();

    let current = app.search.current_index;
    for (i, m) in app.search.matches.iter().enumerate() {
        let fill = if i as i32 == current {
            SEARCH_CURRENT_FILL
        } else {
            SEARCH_OTHER_FILL
        };
        for seg in &m.segments {
            let screen_row = match fold_layout {
                Some(layout) => {
                    // Hidden inside a collapsed region → no highlight.
                    if layout.region_at_line(seg.abs_row).is_some() {
                        continue;
                    }
                    let display_line = layout.actual_line_to_display(seg.abs_row);
                    // Off-screen above the visible window.
                    if display_line < layout.display_start {
                        continue;
                    }
                    display_line - layout.display_start
                }
                None => {
                    // Off-screen above / below the viewport.
                    if seg.abs_row < visible_start {
                        continue;
                    }
                    seg.abs_row - visible_start
                }
            };
            if screen_row >= rows as u32 {
                continue;
            }
            let x = origin.x + seg.col_start as f32 * cell_w;
            let y = origin.y + screen_row as f32 * cell_h;
            let w = (seg.col_end.saturating_sub(seg.col_start)) as f32 * cell_w;
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, cell_h));
            painter.rect_filled(rect, 0.0, fill);
        }
    }
}

/// Translucent fill behind a fold summary row, the const form of the
/// WebView's `rgba(60, 60, 80, 0.3)`:
/// `(60·0.3, 60·0.3, 80·0.3, 0.3·255) ≈ (18, 18, 24, 77)`.
const FOLD_SUMMARY_BG: Color32 = Color32::from_rgba_premultiplied(18, 18, 24, 77);
/// Summary text color for a non-error region, the const (premultiplied)
/// form of WebView `rgba(200, 200, 210, 0.7)`:
/// `(200·0.7, 200·0.7, 210·0.7, 0.7·255) ≈ (140, 140, 147, 178)`.
const FOLD_SUMMARY_FG: Color32 = Color32::from_rgba_premultiplied(140, 140, 147, 178);
/// Summary text color for a failed OSC 133 command (exit != 0): WebView `#ff6b6b`.
const FOLD_SUMMARY_ERR_FG: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);
/// Max characters of the command/label shown before truncating with "…"
/// (matches the WebView's `length > 80 ? substring(0, 77) + "..."`).
const FOLD_SUMMARY_MAX_NAME: usize = 80;

/// Resolve the summary-line text for a region: the truncated command/label
/// (left), the "— N lines" tail with an optional "(exit N)" suffix (right),
/// and whether the region is an error (exit != 0). Split out from
/// [`draw_fold_summaries`] so the logic is unit-testable without an egui
/// context. Mirrors `renderSummaryLine` (renderer-fold.ts).
fn fold_summary_texts(region: &crate::fold::FoldRegion) -> (String, String, bool) {
    use crate::fold::FoldSource;
    let name = match region.source {
        FoldSource::Custom => region.label.as_deref().unwrap_or("..."),
        FoldSource::Osc133 => region.command_text.as_deref().unwrap_or("..."),
    };
    // Truncate by Unicode scalar count (matches the WebView's UTF-16-ish
    // `String.length` closely enough for ASCII commands; multi-byte names
    // truncate on a char boundary so the slice never panics).
    let truncated: String = if name.chars().count() > FOLD_SUMMARY_MAX_NAME {
        let head: String = name.chars().take(FOLD_SUMMARY_MAX_NAME - 3).collect();
        format!("{head}...")
    } else {
        name.to_string()
    };
    let left = format!("\u{25B6} {truncated}"); // ▶

    let mut right = format!("\u{2014} {} lines", region.line_count); // —
    if region.source == FoldSource::Osc133 {
        if let Some(code) = region.exit_code {
            right.push_str(&format!(" (exit {code})"));
        }
    }
    let is_error =
        region.source == FoldSource::Osc133 && matches!(region.exit_code, Some(c) if c != 0);
    (left, right, is_error)
}

/// Paint the fold summary overlays for every summary row in this frame's
/// [`crate::fold::FoldLayout`]. No-op when the active tab has no collapsed
/// regions (`app.fold_layout()` is `None`).
///
/// Each summary row gets a full-width translucent background plus left
/// (`▶ command`) and right (`— N lines [(exit N)]`) text, drawn on the same
/// egui overlay layer (and with the same origin / cell metrics) as the
/// cursor + search highlights so it lands exactly over the row whose cells
/// `collect_cell_inputs` left blank. Mirrors `renderSummaryLine`
/// (renderer-fold.ts): bg, ▶ icon, truncated name on the left, "— N lines"
/// (+ "(exit N)" for OSC 133) right-aligned, error rows in `#ff6b6b`.
fn draw_fold_summaries(ui: &mut egui::Ui, app: &App) {
    let Some(layout) = app.fold_layout() else {
        return;
    };
    let cols = app.cell_size.cols;
    let cell_w = app.cell_w_logical;
    let cell_h = app.cell_h_logical;
    let pad = app.settings.padding as f32;
    let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
    let origin = Pos2::new(pad, crate::ui::title_bar::TITLE_BAR_HEIGHT + tab_h + pad);
    // Summary text uses the terminal font size (logical px == egui points).
    let font_px = app.runtime_font_size_pt * crate::settings::PT_TO_PX;
    let font = FontId::monospace(font_px);
    let row_width = cols as f32 * cell_w;

    for (row, kind) in layout.rows.iter().enumerate() {
        let crate::fold::FoldRowKind::Summary { region } = kind else {
            continue;
        };
        let y = origin.y + row as f32 * cell_h;
        let bg_rect = Rect::from_min_size(Pos2::new(origin.x, y), Vec2::new(row_width, cell_h));
        ui.painter().rect_filled(bg_rect, 0.0, FOLD_SUMMARY_BG);

        let (left, right, is_error) = fold_summary_texts(region);
        let fg = if is_error {
            FOLD_SUMMARY_ERR_FG
        } else {
            FOLD_SUMMARY_FG
        };
        // Vertically center text in the cell row.
        let text_cy = y + cell_h * 0.5;
        // Left text: `char_width * 0.5` indent (WebView).
        ui.painter().text(
            Pos2::new(origin.x + cell_w * 0.5, text_cy),
            Align2::LEFT_CENTER,
            &left,
            font.clone(),
            fg,
        );
        // Right text: right-aligned with a `char_width * 0.5` right margin.
        ui.painter().text(
            Pos2::new(origin.x + row_width - cell_w * 0.5, text_cy),
            Align2::RIGHT_CENTER,
            &right,
            font.clone(),
            fg,
        );
    }
}

/// Draw the floating search bar overlay (when visible) and return the
/// interaction it emitted this frame. Mutates `app.search` (query +
/// toggles) and consumes the one-shot `app.search_focus_request`.
///
/// Kept separate from [`draw_terminal`] (which holds `&App`) because the
/// bar's TextEdit needs `&mut` access to the live query buffer.
pub fn draw_search_overlay(
    ctx: &egui::Context,
    app: &mut App,
) -> Option<crate::ui::search_bar::SearchBarEvent> {
    if !app.search.visible {
        return None;
    }
    // Top inset = chrome stacked above the terminal area (CSD title bar +
    // tab strip). The bar floats `TOP_OFFSET` below it (see search_bar).
    let top_inset = crate::ui::title_bar::TITLE_BAR_HEIGHT
        + crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
    let focus = app.search_focus_request;
    app.search_focus_request = false;
    crate::ui::search_bar::draw(ctx, &mut app.search, top_inset, focus)
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
fn resolve_cell_style_from_packed(
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

    // Reverse: swap source packed colors BEFORE bold-brighten / decoding
    // so the bold-brighten promotion sees the perceived foreground (FR7
    // in the WebView build: bold-brighten is foreground-only and applies
    // *after* reverse).
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

    let mut fg = packed_to_egui(effective_fg_packed, theme.fg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.fg));
    let mut bg = packed_to_egui(effective_bg_packed, theme.bg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.bg));

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
fn blend_toward(a: Color32, b: Color32, t: f32) -> Color32 {
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
fn visible_width(ch: &str, mode: AmbiguousWidthMode) -> u8 {
    let cp = ch.chars().next().map(|c| c as u32).unwrap_or(0);
    if cp == 0 {
        return 1;
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
fn bold_brighten_packed(packed: u32) -> u32 {
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

fn packed_to_egui(packed: u32, _fallback: Rgb, theme: &Theme) -> Option<Color32> {
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

fn rgb_to_egui(rgb: Rgb) -> Color32 {
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
        let to_byte = |n: u8| -> u8 {
            if n == 0 {
                0
            } else {
                55 + n * 40
            }
        };
        Rgb(to_byte(r), to_byte(g), to_byte(b))
    } else {
        // Grayscale ramp.
        let n = idx - 232;
        let v = 8 + n * 10;
        Rgb(v, v, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_narrow_for_ascii() {
        assert_eq!(visible_width("A", AmbiguousWidthMode::Narrow), 1);
        assert_eq!(visible_width("a", AmbiguousWidthMode::Wide), 1);
    }

    #[test]
    fn visible_width_wide_for_cjk() {
        // U+4E00 is "wide" unconditionally — both modes must report 2.
        assert_eq!(visible_width("一", AmbiguousWidthMode::Narrow), 2);
        assert_eq!(visible_width("一", AmbiguousWidthMode::Wide), 2);
    }

    #[test]
    fn visible_width_respects_ambiguous_mode() {
        // U+25A0 (BLACK SQUARE) is in the Unicode "Ambiguous" East-Asian
        // width class.
        assert_eq!(visible_width("■", AmbiguousWidthMode::Narrow), 1);
        assert_eq!(visible_width("■", AmbiguousWidthMode::Wide), 2);
    }

    #[test]
    fn visible_width_minimum_one_for_empty_or_combining() {
        assert_eq!(visible_width("", AmbiguousWidthMode::Narrow), 1);
        // U+0301 (combining acute accent) reports width 0 from
        // display_width; visible_width must floor to 1 so iteration
        // makes progress.
        assert_eq!(visible_width("\u{0301}", AmbiguousWidthMode::Narrow), 1);
    }

    #[test]
    fn blend_toward_endpoints_match_inputs() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(255, 255, 255);
        assert_eq!(blend_toward(a, b, 0.0), a);
        assert_eq!(blend_toward(a, b, 1.0).r(), 255);
    }

    #[test]
    fn blend_toward_midpoint_is_average() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(200, 100, 50);
        let m = blend_toward(a, b, 0.5);
        assert_eq!(m.r(), 100);
        assert_eq!(m.g(), 50);
        assert_eq!(m.b(), 25);
    }

    #[test]
    fn bold_brighten_packed_promotes_indexed_0_7() {
        // tag=1 (indexed), index=3 (yellow) → index=11 (bright yellow)
        let packed_red = (1u32 << 24) | (1u32 << 16);
        assert_eq!(
            bold_brighten_packed(packed_red),
            (1u32 << 24) | (9u32 << 16)
        );

        let packed_yellow = (1u32 << 24) | (3u32 << 16);
        assert_eq!(
            bold_brighten_packed(packed_yellow),
            (1u32 << 24) | (11u32 << 16)
        );
    }

    #[test]
    fn bold_brighten_packed_leaves_already_bright_alone() {
        // index 8..16 are already bright; pass through unchanged.
        let packed = (1u32 << 24) | (10u32 << 16);
        assert_eq!(bold_brighten_packed(packed), packed);
    }

    #[test]
    fn bold_brighten_packed_leaves_truecolor_alone() {
        // tag=2 (truecolor); RGB bits live where the indexed-form `index`
        // byte does, so blindly adding 8 would corrupt the red channel.
        let packed = (2u32 << 24) | 0x00_AA_BB_CC;
        assert_eq!(bold_brighten_packed(packed), packed);
    }

    #[test]
    fn bold_brighten_packed_leaves_default_tag_alone() {
        // tag=0 (default fg). bold_brighten must not mutate.
        let packed = 0u32;
        assert_eq!(bold_brighten_packed(packed), packed);
    }

    #[test]
    fn packed_to_egui_default_returns_none() {
        let theme = Theme::default();
        assert!(packed_to_egui(0x00_00_00_00, Rgb::WHITE, &theme).is_none());
    }

    #[test]
    fn packed_to_egui_indexed_uses_theme_palette() {
        let theme = Theme::default();
        // index = 1 (red) → palette16[1] = WezTerm scheme Rgb(0xff, 0x00, 0x00).
        let packed = 0x01_01_00_00; // tag=1, r=1
        let c = packed_to_egui(packed, Rgb::WHITE, &theme).unwrap();
        assert_eq!(c.r(), 0xff);
        assert_eq!(c.g(), 0x00);
        assert_eq!(c.b(), 0x00);
    }

    #[test]
    fn packed_to_egui_truecolor_returns_exact_rgb() {
        let theme = Theme::default();
        let packed = 0x02_AA_BB_CC; // tag=2, r=AA, g=BB, b=CC
        let c = packed_to_egui(packed, Rgb::WHITE, &theme).unwrap();
        assert_eq!((c.r(), c.g(), c.b()), (0xAA, 0xBB, 0xCC));
    }

    // ── font-swash-migration: Theme dead_code resolution (FR10) ────────

    /// TS-font-11: `Theme::default().font_family` is `"monospace"` and
    /// `font_size_pt` is `13.0` (regression guard).
    #[test]
    fn theme_default_font_family_is_monospace() {
        let t = Theme::default();
        assert_eq!(t.font_family, "monospace");
        assert!((t.font_size_pt - 13.0).abs() < f32::EPSILON);
    }

    // ── Phase 4-H: collect_cell_inputs ────────────────────────────────

    /// `collect_cell_inputs` produces exactly `cols * rows` entries —
    /// one per logical cell — even when the grid is mostly blank. The
    /// `TerminalGridPass::build_instances` consumer filters
    /// whitespace / empty clusters internally, so the renderer can
    /// pass the full grid through without an extra pre-filter pass.
    #[test]
    fn collect_cell_inputs_emits_one_entry_per_cell() {
        let mut core = TerminalCore::new(5, 2, 100);
        core.process_pty_data(b"ABCDE");
        let theme = Theme::default();
        let inputs = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            0,
            None,
        );
        // 5 cols × 2 rows = 10 cell entries.
        assert_eq!(inputs.len(), 10);
        // Row 0 should carry the literal glyphs in column order.
        let row0: String = inputs
            .iter()
            .filter(|c| c.row == 0)
            .map(|c| c.glyph.as_str())
            .collect();
        assert_eq!(row0, "ABCDE");
    }

    /// Wide CJK cells advance the iterator by two columns and report
    /// `width_cells = 2`. The cell at `col+1` would normally be the
    /// trailing half of the wide glyph; `collect_cell_inputs` skips it
    /// (`col` advances past it) so a single instance covers the whole
    /// wide rectangle.
    #[test]
    fn collect_cell_inputs_handles_wide_cells() {
        let mut core = TerminalCore::new(4, 1, 100);
        core.process_pty_data("あA".as_bytes());
        let theme = Theme::default();
        let inputs = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            0,
            None,
        );
        assert_eq!(inputs[0].glyph, "あ");
        assert_eq!(inputs[0].width_cells, 2);
        // Column 2 holds the 'A'; column 1 was skipped (trailing half of あ).
        let a = inputs.iter().find(|c| c.glyph == "A").expect("A present");
        assert_eq!(a.col, 2);
        assert_eq!(a.width_cells, 1);
    }

    /// Decoration flags propagate from `STYLE_UNDERLINE` /
    /// `STYLE_STRIKETHROUGH` SGR bits onto the `CellInput`.
    #[test]
    fn collect_cell_inputs_propagates_decoration_flags() {
        let mut core = TerminalCore::new(3, 1, 100);
        // SGR 4 = underline; SGR 9 = strikethrough.
        core.process_pty_data(b"\x1b[4mU\x1b[0m\x1b[9mS\x1b[0mN");
        let theme = Theme::default();
        let inputs = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            0,
            None,
        );
        let u = inputs.iter().find(|c| c.glyph == "U").expect("U present");
        let s = inputs.iter().find(|c| c.glyph == "S").expect("S present");
        let n = inputs.iter().find(|c| c.glyph == "N").expect("N present");
        assert!(u.underline);
        assert!(!u.strikethrough);
        assert!(s.strikethrough);
        assert!(!s.underline);
        assert!(!n.underline);
        assert!(!n.strikethrough);
    }

    /// Non-default background colors set `draw_background = true`; the
    /// default-background cells leave it `false` so the wgpu pass can
    /// skip the background quad (the swapchain clear covers it).
    #[test]
    fn collect_cell_inputs_draw_background_only_when_non_default() {
        let mut core = TerminalCore::new(3, 1, 100);
        // SGR 41 = red background.
        core.process_pty_data(b"\x1b[41mR\x1b[0mN");
        let theme = Theme::default();
        let inputs = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            0,
            None,
        );
        let r = inputs.iter().find(|c| c.glyph == "R").expect("R present");
        let n = inputs.iter().find(|c| c.glyph == "N").expect("N present");
        assert!(r.draw_background);
        assert!(!n.draw_background);
    }

    // ── Scrollback rendering (scroll_offset) ──────────────────────────

    /// Helper: collect the on-screen glyph for a given row in reading order,
    /// trimming trailing blanks so the assertions read cleanly.
    fn row_text(inputs: &[CellInput], row: u16) -> String {
        let mut cells: Vec<&CellInput> = inputs.iter().filter(|c| c.row == row).collect();
        cells.sort_by_key(|c| c.col);
        cells
            .iter()
            .map(|c| c.glyph.as_str())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// `scroll_offset == 0` produces output identical to the pre-scrollback
    /// path: the live viewport is read row-for-row regardless of how much
    /// scrollback exists behind it.
    #[test]
    fn collect_cell_inputs_offset_zero_matches_live() {
        let mut core = TerminalCore::new(5, 2, 100);
        // Push "L0".."L3" so L0/L1 land in scrollback and L2/L3 are live.
        core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3");
        assert!(core.get_scrollback_length() >= 2);
        let theme = Theme::default();
        let live = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            0,
            None,
        );
        // Live viewport shows the last two logical lines.
        assert_eq!(row_text(&live, 0), "L2");
        assert_eq!(row_text(&live, 1), "L3");
    }

    /// A non-zero offset surfaces scrollback rows: scrolling back by the full
    /// viewport height shows the oldest two rows that had scrolled off.
    #[test]
    fn collect_cell_inputs_offset_shows_scrollback() {
        let mut core = TerminalCore::new(5, 2, 100);
        core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3");
        let scrollback_len = core.get_scrollback_length();
        assert_eq!(scrollback_len, 2, "L0 and L1 evicted into scrollback");
        let theme = Theme::default();
        // Offset = 2 (one full viewport back) → top of view is absolute row
        // `scrollback_len - 2 = 0`, so rows 0/1 show the scrollback L0/L1.
        let scrolled = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            2,
            None,
        );
        assert_eq!(row_text(&scrolled, 0), "L0");
        assert_eq!(row_text(&scrolled, 1), "L1");
    }

    /// An offset that straddles the scrollback↔viewport seam shows a
    /// scrollback row on top and a live viewport row below it.
    #[test]
    fn collect_cell_inputs_offset_spans_boundary() {
        let mut core = TerminalCore::new(5, 2, 100);
        core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3");
        assert_eq!(core.get_scrollback_length(), 2);
        let theme = Theme::default();
        // Offset = 1: top visible absolute row = scrollback_len - 1 = 1
        // (scrollback L1), bottom = absolute row 2 (live L2).
        let scrolled = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            1,
            None,
        );
        assert_eq!(row_text(&scrolled, 0), "L1");
        assert_eq!(row_text(&scrolled, 1), "L2");
    }

    /// A wide CJK glyph in a scrollback row reports `width_cells = 2` and the
    /// following cell starts at `col + 2` (the width-0 continuation half is
    /// dropped by the term_core accessor), matching the live-viewport path.
    #[test]
    fn collect_cell_inputs_scrollback_handles_wide_cells() {
        let mut core = TerminalCore::new(4, 1, 100);
        // Row 0 carries "あA"; printing a second line scrolls it off into
        // scrollback (1-row viewport).
        core.process_pty_data("あA\r\nX".as_bytes());
        assert_eq!(core.get_scrollback_length(), 1);
        let theme = Theme::default();
        let scrolled = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            1,
            None,
        );
        let wide = scrolled
            .iter()
            .find(|c| c.glyph == "あ")
            .expect("あ present in scrollback row");
        assert_eq!(wide.col, 0);
        assert_eq!(wide.width_cells, 2);
        let a = scrolled
            .iter()
            .find(|c| c.glyph == "A")
            .expect("A present in scrollback row");
        // Column 2 holds the 'A'; column 1 (trailing half of あ) was skipped.
        assert_eq!(a.col, 2);
        assert_eq!(a.width_cells, 1);
    }

    /// Scrollback cells carry their SGR style: a bold-underlined cell that
    /// scrolled off keeps `bold` / `underline` set on its `CellInput`.
    #[test]
    fn collect_cell_inputs_scrollback_preserves_style() {
        let mut core = TerminalCore::new(5, 1, 100);
        // SGR 1 = bold, 4 = underline; then scroll the styled row off.
        core.process_pty_data(b"\x1b[1;4mB\x1b[0m\r\nX");
        assert_eq!(core.get_scrollback_length(), 1);
        let theme = Theme::default();
        let scrolled = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            1,
            None,
        );
        let b = scrolled
            .iter()
            .find(|c| c.glyph == "B")
            .expect("B present in scrollback row");
        assert!(b.bold);
        assert!(b.underline);
    }

    // ── Fold-aware rendering ──────────────────────────────────────────

    /// With a fold layout, `collect_cell_inputs` reads each screen row's
    /// *actual* buffer row from the layout (not the linear scrollback
    /// model), and emits no cells for a summary row.
    #[test]
    fn collect_cell_inputs_fold_layout_maps_rows_and_skips_summary() {
        // 5 logical lines L0..L4; 5-row viewport so nothing evicts (all
        // live). Collapse a region over actual rows 1..3 (L1, L2).
        let mut core = TerminalCore::new(5, 5, 100);
        core.process_pty_data(b"L0\r\nL1\r\nL2\r\nL3\r\nL4");
        assert_eq!(core.get_scrollback_length(), 0);

        let mut fm = crate::fold::FoldManager::new();
        // Region 1..3 (rows L1,L2) collapsed → summary at display line 1,
        // hides 1 body row (line_count 2 - 1).
        fm.register_osc133_region(1, 3, "cmd".to_string(), Some(0));
        fm.toggle_fold(1);
        // total_actual = 5, hidden = 1, total_display = 4.
        // viewport = 5, offset 0 → display_start = max(0, 4-5-0) = 0.
        let layout = fm.build_layout(0, 5, 0);

        let theme = Theme::default();
        let inputs = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            0,
            Some(&layout),
        );

        // Screen row 0 = actual L0.
        assert_eq!(row_text(&inputs, 0), "L0");
        // Screen row 1 = summary → no cells emitted by collect_cell_inputs.
        assert_eq!(row_text(&inputs, 1), "");
        assert!(
            !inputs.iter().any(|c| c.row == 1),
            "summary row must emit no CellInput (overlay draws it)"
        );
        // Screen row 2 = actual L3 (the body L1/L2 collapsed onto the
        // summary, so the next visible buffer row is L3).
        assert_eq!(row_text(&inputs, 2), "L3");
        // Screen row 3 = actual L4.
        assert_eq!(row_text(&inputs, 3), "L4");
    }

    /// Without a fold layout (`None`), the row mapping is the unchanged
    /// linear path even when collapsed regions exist in the manager — the
    /// caller gates passing `Some` on `has_collapsed_regions`, so `None`
    /// must reproduce the pre-fold behavior exactly.
    #[test]
    fn collect_cell_inputs_none_layout_is_linear() {
        let mut core = TerminalCore::new(5, 3, 100);
        core.process_pty_data(b"L0\r\nL1\r\nL2");
        let theme = Theme::default();
        let inputs = collect_cell_inputs(
            &core,
            &theme,
            None,
            AmbiguousWidthMode::Narrow,
            None,
            None,
            0,
            None,
        );
        assert_eq!(row_text(&inputs, 0), "L0");
        assert_eq!(row_text(&inputs, 1), "L1");
        assert_eq!(row_text(&inputs, 2), "L2");
    }

    // ── fold_summary_texts ────────────────────────────────────────────

    fn osc133_region(cmd: &str, exit: Option<i32>, lines: u32) -> crate::fold::FoldRegion {
        crate::fold::FoldRegion {
            id: format!("osc133:{lines}"),
            start_line: 0,
            end_line: lines,
            collapsed: true,
            source: crate::fold::FoldSource::Osc133,
            command_text: Some(cmd.to_string()),
            label: None,
            exit_code: exit,
            line_count: lines,
        }
    }

    #[test]
    fn fold_summary_texts_osc133_with_exit_zero() {
        let r = osc133_region("ls -la", Some(0), 12);
        let (left, right, err) = fold_summary_texts(&r);
        assert_eq!(left, "\u{25B6} ls -la");
        assert_eq!(right, "\u{2014} 12 lines (exit 0)");
        assert!(!err);
    }

    #[test]
    fn fold_summary_texts_osc133_nonzero_exit_is_error() {
        let r = osc133_region("make", Some(2), 40);
        let (_, right, err) = fold_summary_texts(&r);
        assert_eq!(right, "\u{2014} 40 lines (exit 2)");
        assert!(err);
    }

    #[test]
    fn fold_summary_texts_osc133_no_exit_omits_suffix() {
        let r = osc133_region("running", None, 5);
        let (_, right, err) = fold_summary_texts(&r);
        assert_eq!(right, "\u{2014} 5 lines");
        assert!(!err);
    }

    #[test]
    fn fold_summary_texts_custom_uses_label() {
        let r = crate::fold::FoldRegion {
            id: "custom:0".to_string(),
            start_line: 0,
            end_line: 3,
            collapsed: true,
            source: crate::fold::FoldSource::Custom,
            command_text: None,
            label: Some("Build Output".to_string()),
            exit_code: None,
            line_count: 3,
        };
        let (left, right, err) = fold_summary_texts(&r);
        assert_eq!(left, "\u{25B6} Build Output");
        assert_eq!(right, "\u{2014} 3 lines");
        assert!(!err);
    }

    #[test]
    fn fold_summary_texts_truncates_long_name() {
        // 100-char command → "▶ " + 77 chars + "...".
        let cmd = "a".repeat(100);
        let r = osc133_region(&cmd, Some(0), 1);
        let (left, _, _) = fold_summary_texts(&r);
        let expected_name = format!("{}...", "a".repeat(77));
        assert_eq!(left, format!("\u{25B6} {expected_name}"));
        // The displayed name is exactly 80 chars (77 + "...").
        let name = left.strip_prefix("\u{25B6} ").unwrap();
        assert_eq!(name.chars().count(), 80);
    }

    #[test]
    fn fold_summary_texts_short_name_not_truncated() {
        let cmd = "a".repeat(80);
        let r = osc133_region(&cmd, Some(0), 1);
        let (left, _, _) = fold_summary_texts(&r);
        // Exactly 80 chars is the boundary: not truncated.
        assert_eq!(left, format!("\u{25B6} {cmd}"));
    }
}
