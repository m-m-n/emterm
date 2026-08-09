//! Grid cell-input assembly: walk the terminal grid into the
//! `Vec<CellInput>` consumed by `terminal_grid_pass`, the IME preedit
//! overlay, and the packed-color / cell-style resolution helpers.

use super::*;

/// Per-cell paint parameters resolved from a `term_core` cell + active
/// palette + selection state.
pub(in crate::render) struct CellStyle {
    pub(in crate::render) fg: Color32,
    pub(in crate::render) bg: Color32,
    // Read by future Resolver-driven weight / style selection; the prior
    // painter.text() path read these for egui font face, which is now gone.
    #[allow(dead_code)]
    pub(in crate::render) bold: bool,
    #[allow(dead_code)]
    pub(in crate::render) italic: bool,
    pub(in crate::render) underline: bool,
    pub(in crate::render) strikethrough: bool,
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

pub(in crate::render) fn packed_to_egui(
    packed: u32,
    _fallback: Rgb,
    theme: &Theme,
) -> Option<Color32> {
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
