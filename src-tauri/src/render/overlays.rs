//! Overlay passes drawn above the terminal grid: profile selector,
//! SFTP progress, cursor, search highlights / search bar, and fold
//! summaries.

use super::*;

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
        use crate::ui::dialog::{Dialog, DialogOutcome};
        let dest_label = t("送信先:", "Destination:");
        let outcome = Dialog::<()>::confirm("アップロードの確認", "Confirm upload", loc)
            .body(|ui: &mut egui::Ui| {
                ui.label(t("アップロードするファイル:", "Files to upload:"));
                for p in &dialog.paths {
                    ui.label(p.to_string_lossy());
                }
                ui.separator();
                ui.label(format!("{} {}", dest_label, dialog.remote_dir));
            })
            .primary_button("アップロード", "Upload", || ())
            .show(ctx);
        match outcome {
            DialogOutcome::Confirmed(()) => event = Some(SftpFrameEvent::ConfirmUpload),
            DialogOutcome::Cancelled => event = Some(SftpFrameEvent::CancelUpload),
            DialogOutcome::Pending => {}
        }
    }

    // ── Overwrite confirmation dialog ────────────────────────────
    if let Some(dialog) = app.sftp_ui.overwrite_dialog.clone() {
        use crate::ui::dialog::{Dialog, DialogOutcome};
        // SPEC US3/FR6: Cancel is the default focus for the destructive
        // overwrite dialog. Bare Enter maps to Cancel (helper-enforced
        // for destructive-confirm kind). Overwrite requires an explicit
        // button click.
        let outcome = Dialog::<()>::destructive_confirm("上書きの確認", "Confirm overwrite", loc)
            .body(|ui: &mut egui::Ui| {
                ui.label(t(
                    "次のファイルは既に存在します:",
                    "These files already exist:",
                ));
                for name in &dialog.duplicates {
                    ui.label(name);
                }
            })
            .primary_button("上書き", "Overwrite", || ())
            .show(ctx);
        match outcome {
            DialogOutcome::Confirmed(()) => event = Some(SftpFrameEvent::ConfirmOverwrite),
            DialogOutcome::Cancelled => event = Some(SftpFrameEvent::CancelOverwrite),
            DialogOutcome::Pending => {}
        }
    }

    // ── Tab-close guard dialog ───────────────────────────────────
    if app.sftp_ui.close_guard.is_some() {
        use crate::ui::dialog::{Dialog, DialogOutcome};
        let outcome =
            Dialog::<()>::destructive_confirm("タブを閉じますか?", "Close this tab?", loc)
                .body(|ui: &mut egui::Ui| {
                    ui.label(t(
                        "このタブにはアップロード中のファイルがあります。閉じると中止されます。",
                        "This tab has uploads in progress. Closing will cancel them.",
                    ));
                })
                .primary_button("閉じる", "Close", || ())
                .show(ctx);
        match outcome {
            DialogOutcome::Confirmed(()) => event = Some(SftpFrameEvent::ConfirmClose),
            DialogOutcome::Cancelled => event = Some(SftpFrameEvent::CancelClose),
            DialogOutcome::Pending => {}
        }
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

    // ── Binary-mismatch restart toast (top-right, single) ────────
    // FR4/FR5/FR6: shown while armed; auto-dismissed by `pump_sftp` via frame
    // time. Anchored top-right with a vertical offset placed BELOW the SFTP
    // progress-toast stack (which grows downward from y=12) so the two never
    // overlap. The SFTP stack height is unknown here, so the offset is derived
    // from the visible SFTP toast count (a generous per-toast row estimate);
    // with no SFTP toasts the restart toast sits at the top.
    if app.restart_toast.active() {
        const SFTP_TOAST_ROW_PX: f32 = 44.0;
        let y_offset = 12.0 + app.sftp_ui.toasts.toasts.len() as f32 * SFTP_TOAST_ROW_PX;
        egui::Area::new(egui::Id::new("restart_toast"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, y_offset))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(t(
                        "eMterm が更新されました。再起動してください",
                        "eMterm was updated. Please restart.",
                    ));
                });
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

/// Cursor overlay: shape from `get_cursor_style`, blink from
/// `get_cursor_blink` modulated by `App::blink_visible_now`, visibility
/// from `get_cursor_visible`, color from `Theme.cursor_fg` via
/// [`cursor::resolve_cursor_color`] (task0003 D3) — the active color
/// scheme's cursor color, or an OSC 12 override while one is active;
/// never `TerminalCore::get_cursor_fg()` (the unrelated SGR pen
/// foreground).
pub(in crate::render) fn draw_cursor(ui: &mut egui::Ui, core: &TerminalCore, theme: &Theme, app: &App) {
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
    // task0006 (right-edge persistent placement): the persistent mux
    // sidebar reserves usable grid WIDTH only — the x-origin here is
    // identical with and without it, matching
    // `window_host::cell_metrics_px`'s un-inset origin_x.
    let origin = Pos2::new(pad, crate::ui::title_bar::TITLE_BAR_HEIGHT + tab_h + pad);
    let painter = ui.painter();

    let cell_w = app.cell_w_logical;
    let cell_h = app.cell_h_logical;
    let cx = origin.x + core.get_cursor_col() as f32 * cell_w;
    let cy = origin.y + core.get_cursor_row() as f32 * cell_h;

    let cursor_color = rgb_to_egui(cursor::resolve_cursor_color(theme));

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
            // Block cursor: focused → filled cell overlay, painted by
            // `cursor::draw_block_cursor` (the cell's resolved paint
            // style inverted — this used to be baked into the grid pass
            // via `collect_cell_inputs`'s removed `block_cursor_cell`
            // param). Unfocused → hollow outline here, matching WezTerm.
            // egui 0.29 lacks `StrokeKind::Inside`, so a centered 1-px
            // stroke would bleed half a pixel above / below the cell —
            // inset the rect by half the stroke width to keep the
            // visible outline flush with the IME reverse-video box.
            if app.window_focused {
                cursor::draw_block_cursor(painter, core, theme, app);
            } else {
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
pub(in crate::render) fn draw_search_highlights(ui: &mut egui::Ui, core: &TerminalCore, app: &App) {
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
    // task0006 (right-edge persistent placement): the persistent mux
    // sidebar reserves usable grid WIDTH only, so no x-origin term belongs
    // here either — identical to draw_cursor.
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
pub(in crate::render) fn fold_summary_texts(region: &crate::fold::FoldRegion) -> (String, String, bool) {
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
pub(in crate::render) fn draw_fold_summaries(ui: &mut egui::Ui, app: &App) {
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
