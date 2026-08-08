//! Status-bar widget.
//!
//! Renders a 3-row [`egui::TopBottomPanel`] fixed at the bottom of
//! the window. Rows top-to-bottom:
//!
//! 1. **App Line 1** — local templates resolved by the
//!    `TemplateEngine` (typically `{time}` / `{cwd}`). Auto-hidden
//!    when its resolved content is empty.
//! 2. **App Line 2** — second template row, auto-hidden when both
//!    sides are empty (FR12).
//! 3. **OSC row** — the OSC 777;statusbar dispatcher's layer state.
//!    Auto-hidden when empty unless `show` was requested.
//!
//! The widget is pure over [`StatusBarViewModel`]; the render
//! pipeline projects the active tab + runtime state into the view
//! model once per frame.

use egui::{Align, Color32, FontFamily, FontId, Image, Layout, Margin, RichText};

use crate::html::{CssColor, RichTextRun};
use crate::status_bar::{AppRow, OscRow, StatusBarViewModel};
use crate::ui::emoji_cache::{EmojiResources, TextSegment, split_segments};
use crate::ui::md3;

/// Per-row visual height in egui logical points. Three rows render
/// stacked; the panel height multiplies this by the number of
/// visible rows.
pub const ROW_HEIGHT: f32 = 22.0;
/// Default font size for App rows; OSC row also uses this unless the
/// view model overrides `font_size`.
const DEFAULT_FONT_SIZE: f32 = 12.0;

/// Number of rows the status bar will paint for `view_model` this
/// frame (0 when disabled or every row is auto-hidden).
pub fn visible_row_count(view_model: &StatusBarViewModel) -> u32 {
    if !view_model.enabled {
        return 0;
    }
    let osc_visible = view_model.osc.should_render();
    // App Line 1 auto-hides on the same resolved-content rule as App
    // Line 2 (no separate "always visible" carve-out).
    let app1_visible = view_model.app_line1.has_content();
    let app2_visible = view_model.app_line2.has_content();
    (osc_visible as u32) + (app1_visible as u32) + (app2_visible as u32)
}

/// Panel height in egui logical points. The terminal grid layout uses
/// this to reserve room above/below the cell area so the bottom row
/// never gets covered by the status-bar panel (and, when the panel
/// sits on top, so cells don't render behind it).
pub fn panel_height_logical(view_model: &StatusBarViewModel) -> f32 {
    ROW_HEIGHT * visible_row_count(view_model) as f32
}

/// Render the status bar. Returns immediately (no panel inserted)
/// when `view_model.enabled` is false.
///
/// `emoji` is `Some` in production so color-emoji clusters render via
/// swash-rasterized images; tests pass `None` to keep the egui-only
/// text path in play.
pub fn draw(
    ctx: &egui::Context,
    view_model: &StatusBarViewModel,
    emoji: Option<&EmojiResources<'_>>,
) {
    let visible_rows = visible_row_count(view_model);
    if visible_rows == 0 {
        return;
    }

    let mut panel = egui::TopBottomPanel::bottom("native-poc-status-bar");
    let frame = egui::Frame::none()
        .fill(md3::surface_container())
        // Match the WebView's per-row `padding: 0 8px`: inset content
        // 8px horizontally so text isn't flush against the panel edge.
        .inner_margin(Margin::symmetric(8.0, 0.0));
    panel = panel
        .frame(frame)
        .show_separator_line(false)
        .exact_height(ROW_HEIGHT * visible_rows as f32);

    let app1_visible = view_model.app_line1.has_content();
    let app2_visible = view_model.app_line2.has_content();
    let osc_visible = view_model.osc.should_render();

    let font_size = view_model.font_size.unwrap_or(DEFAULT_FONT_SIZE);

    panel.show(ctx, |ui| {
        // Drop the default vertical item_spacing so rows stack flush
        // against each other and against the panel edges (the panel
        // is fixed at ROW_HEIGHT × visible_rows).
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            // FR1 layer order: OSC layer, App Line 1, App Line 2
            // (top-to-bottom, regardless of panel placement).
            if osc_visible {
                draw_osc_row(ui, &view_model.osc, font_size, emoji);
            }
            if app1_visible {
                draw_app_row(ui, &view_model.app_line1, font_size, emoji);
            }
            if app2_visible {
                draw_app_row(ui, &view_model.app_line2, font_size, emoji);
            }
        });
    });
}

/// Render an App row: left runs flow left-to-right, right runs flow
/// right-to-left. Shared with App Line 1 / 2.
fn draw_app_row(
    ui: &mut egui::Ui,
    row: &AppRow,
    font_size: f32,
    emoji: Option<&EmojiResources<'_>>,
) {
    let font = FontId::new(font_size, FontFamily::Monospace);
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        // No inter-widget spacing: a row's runs / text / emoji segments
        // form one continuous string (like the WebView, where a layer's
        // left/right section is a single text node). Separators such as
        // " | " already live in the template text, so an extra 8px gap
        // between every segment — and especially around each emoji
        // `Image` — would make the row read as disjoint chips.
        ui.spacing_mut().item_spacing.x = 0.0;
        // Split the row into two fixed half-width slots. Each section is
        // left-aligned inside its slot and truncates its tail with `…`
        // (mirrors the WebView's `max-width: 50%`). The left slot starts
        // at the row start; the right slot starts at the centre, so the
        // right section's left edge is pinned to the middle and its
        // overflow drops off the right — keeping `🤖 5h …` visible.
        let section_w = ui.available_width() * 0.5;
        // Left section: left-aligned at the row start, capped at half
        // the row, tail-truncated.
        draw_section(ui, runs_to_atoms(&row.left), &font, section_w, false, emoji);
        // Right section: fills the remaining width and right-aligns
        // against the panel edge (`draw_section` wraps it in a
        // right-to-left layout). Capped at the same half-row width so it
        // cannot cross the centre into the left section.
        draw_section(ui, runs_to_atoms(&row.right), &font, section_w, true, emoji);
    });
}

/// Render the OSC row.
fn draw_osc_row(
    ui: &mut egui::Ui,
    row: &OscRow,
    font_size: f32,
    emoji: Option<&EmojiResources<'_>>,
) {
    let font = FontId::new(font_size, FontFamily::Monospace);
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        // See `draw_app_row`: segments are one continuous string.
        ui.spacing_mut().item_spacing.x = 0.0;
        // Left and right each cap at half the row (see `draw_app_row`).
        let section_w = ui.available_width() * 0.5;
        let mut left_atoms: Vec<DrawAtom> = Vec::new();
        if !row.left.is_empty() {
            // OSC row text is post-strip plain text — push it through the
            // same segmenter so color-emoji clusters become images.
            push_text_atoms(&mut left_atoms, &row.left, &AtomStyle::plain());
        }
        // Left section at the row start, right section right-aligned
        // against the panel edge — each half-width and tail-truncated
        // (see `draw_app_row`).
        draw_section(ui, left_atoms, &font, section_w, false, emoji);
        if !row.right.is_empty() {
            let style = AtomStyle {
                color: Some(Color32::LIGHT_GRAY),
                ..AtomStyle::plain()
            };
            draw_section(
                ui,
                plain_to_atoms(&row.right, &style),
                &font,
                section_w,
                true,
                emoji,
            );
        }
    });
}

/// Lightweight draw-time style for a section atom. Normalised to
/// `Color32` so the App row (`CssColor` runs) and OSC row (plain text
/// with an optional color override) share one truncation + drawing
/// pipeline.
#[derive(Clone, PartialEq)]
struct AtomStyle {
    color: Option<Color32>,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl AtomStyle {
    /// Default (inherit foreground, no decoration). Used for App rows
    /// and for the ellipsis glyph.
    fn plain() -> Self {
        Self {
            color: None,
            bold: false,
            italic: false,
            underline: false,
        }
    }

    fn from_run(run: &RichTextRun) -> Self {
        Self {
            color: run.color.as_ref().map(css_color_to_color32),
            bold: run.bold,
            italic: run.italic,
            underline: run.underline,
        }
    }
}

/// One measurable, drawable unit of a status-bar section. Splitting to
/// this grain lets the section measure its full width, then truncate at
/// a clean atom boundary (per-character for text, per-cluster for
/// emoji) and append an ellipsis when it overflows its half of the row.
#[derive(Clone)]
enum DrawAtom {
    /// A single non-space character.
    Char(char, AtomStyle),
    /// One emoji grapheme cluster (rendered as a color image).
    Emoji(String, AtomStyle),
    /// One monospace space-width advance. Kept as its own atom so it is
    /// never trimmed at a galley edge (the `🤖 5h` boundary-space bug).
    Space,
    /// An explicit fixed gap in logical points (mux-badge separator,
    /// degraded line break).
    FixedGap(f32),
}

/// Flatten styled runs into a source-order atom list.
fn runs_to_atoms(runs: &[RichTextRun]) -> Vec<DrawAtom> {
    let mut out = Vec::new();
    for run in runs {
        if run.line_break {
            // Single-strip layout: a line break degrades to a fixed gap.
            out.push(DrawAtom::FixedGap(8.0));
            continue;
        }
        let style = AtomStyle::from_run(run);
        push_text_atoms(&mut out, &run.text, &style);
    }
    out
}

/// Flatten a plain (post-strip) string into atoms sharing one style.
fn plain_to_atoms(text: &str, style: &AtomStyle) -> Vec<DrawAtom> {
    let mut out = Vec::new();
    push_text_atoms(&mut out, text, style);
    out
}

/// Append `text`'s atoms (color-emoji clusters as `Emoji`, spaces as
/// `Space`, the rest per-character) to `out`.
fn push_text_atoms(out: &mut Vec<DrawAtom>, text: &str, style: &AtomStyle) {
    use unicode_segmentation::UnicodeSegmentation;
    for segment in split_segments(text) {
        match segment {
            TextSegment::Text(t) => {
                for ch in t.chars() {
                    if ch == ' ' {
                        out.push(DrawAtom::Space);
                    } else {
                        out.push(DrawAtom::Char(ch, style.clone()));
                    }
                }
            }
            TextSegment::Emoji(t) => {
                for cluster in t.graphemes(true) {
                    out.push(DrawAtom::Emoji(cluster.to_string(), style.clone()));
                }
            }
        }
    }
}

/// Width of one monospace space at `font`, in egui logical points.
fn space_width(ui: &egui::Ui, font: &FontId) -> f32 {
    ui.fonts(|f| f.glyph_width(font, ' '))
}

/// Width of an emoji cluster: the cached texture's logical width, or the
/// text-fallback width when no emoji stack is available / the glyph is
/// uncovered. Mirrors the sizing in [`emit_emoji_cluster_chain`].
fn emoji_atom_width(
    ui: &egui::Ui,
    cluster: &str,
    font: &FontId,
    emoji: Option<&EmojiResources<'_>>,
) -> f32 {
    if let Some(em) = emoji {
        let ppp = ui.ctx().pixels_per_point();
        let raster_px = font.size * crate::settings::PT_TO_PX * ppp;
        if let Some(tex) = em.cache.lock().get_or_rasterize(
            ui.ctx(),
            em.rasterizer,
            em.fallback,
            cluster,
            raster_px,
        ) {
            return tex.size_vec2().x / ppp;
        }
    }
    cluster
        .chars()
        .map(|c| ui.fonts(|f| f.glyph_width(font, c)))
        .sum()
}

/// Draw a section's atoms within a `section_w`-wide slot, dropping the
/// overflow from the right behind a trailing `…`.
///
/// Both sections truncate their tail (keeping the leading, most
/// important content — the right section's `🤖 5h …`). They differ only
/// in placement inside the slot: the left section is left-aligned at the
/// row start (`align_right = false`); the right section is right-aligned
/// against the panel edge (`align_right = true`) so short content hugs
/// the right edge, while overflowing content fills the slot from the
/// centre and truncates at the right. Capping both slots at half the row
/// keeps them from overlapping at the centre.
fn draw_section(
    ui: &mut egui::Ui,
    atoms: Vec<DrawAtom>,
    font: &FontId,
    section_w: f32,
    align_right: bool,
    emoji: Option<&EmojiResources<'_>>,
) {
    if atoms.is_empty() {
        return;
    }

    // Streaming truncation: walk the atoms front-to-back in the same
    // same-style-run coalescing units `draw_atoms` paints, accumulating
    // each unit's *drawn* width, and stop as soon as the kept prefix
    // would exceed the slot. We never measure (or, for emoji, rasterize)
    // the overflow tail — the OSC layer is terminal-controlled, so a
    // long payload must not force whole-string measurement/raster work
    // every frame. Measuring in the same coalescing units as drawing
    // also means the truncation budget and the painted width share one
    // basis (no per-atom-sum vs. coalesced-galley drift), so the kept
    // prefix never paints past `section_w` into the other half.
    let (kept, truncated) = truncate_atoms_to_width(ui, atoms, font, section_w, emoji);
    let kept = if truncated {
        let mut kept = kept;
        kept.push(DrawAtom::Char('\u{2026}', AtomStyle::plain()));
        kept
    } else {
        kept
    };
    if kept.is_empty() {
        return;
    }

    if align_right {
        // Right-align within the slot: a right-to-left layout pins the
        // content against the slot's right edge, but we draw the kept
        // atoms inside a left-to-right child sized to their exact width
        // so their reading order (and the emoji pixel-grid snap, which
        // only runs in an LTR layout) is preserved.
        //
        // Size the child with the *drawn* width (`measure_atoms_drawn`),
        // not the per-atom sum: `draw_atoms` coalesces same-style runs
        // into one galley, and a galley's width differs slightly from
        // summing each glyph's advance. Over a long run (e.g. a 40-cell
        // progress bar) that drift accumulates and would leave a visible
        // gap between the content and the right edge.
        let kept_w = measure_atoms_drawn(ui, &kept, font, emoji);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(kept_w, ROW_HEIGHT),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    draw_atoms(ui, &kept, font, emoji);
                },
            );
        });
    } else {
        draw_atoms(ui, &kept, font, emoji);
    }
}

/// Walk `atoms` front-to-back in `draw_atoms`' coalescing units and
/// keep the longest prefix whose drawn width fits `section_w`. Returns
/// `(kept_atoms, truncated)`; when `truncated` is true the caller
/// appends the ellipsis (its width is already reserved here).
///
/// Crucially this stops measuring at the first unit that overflows, so
/// the overflow tail of a long (terminal-controlled) section is never
/// measured or rasterized. Units are the same ones `draw_atoms` paints
/// — a maximal same-style `Char` run, a `Space`, a `FixedGap`, or one
/// `Emoji` cluster — so the budgeted width equals the painted width.
fn truncate_atoms_to_width(
    ui: &egui::Ui,
    atoms: Vec<DrawAtom>,
    font: &FontId,
    section_w: f32,
    emoji: Option<&EmojiResources<'_>>,
) -> (Vec<DrawAtom>, bool) {
    let ellipsis_w = ui.fonts(|f| f.glyph_width(font, '\u{2026}'));
    // Budget for content once an ellipsis might be needed. We don't know
    // yet whether we'll truncate, so reserve the ellipsis room up front;
    // if everything fits we return `truncated = false` and the full set.
    let budget_with_ellipsis = (section_w - ellipsis_w).max(0.0);

    let mut acc = 0.0;
    let mut kept: Vec<DrawAtom> = Vec::new();
    let mut i = 0;
    let mut truncated = false;
    while i < atoms.len() {
        // Determine the next coalescing unit [i, unit_end) and its width.
        let (unit_end, unit_w) = match &atoms[i] {
            DrawAtom::Char(_, style) => {
                let mut text = String::new();
                let mut j = i;
                while j < atoms.len() {
                    if let DrawAtom::Char(ch, st) = &atoms[j] {
                        if st == style {
                            text.push(*ch);
                            j += 1;
                            continue;
                        }
                    }
                    break;
                }
                (j, galley_width(ui, &text, font, style))
            }
            DrawAtom::Space => (i + 1, space_width(ui, font)),
            DrawAtom::FixedGap(w) => (i + 1, *w),
            DrawAtom::Emoji(cluster, _) => (i + 1, emoji_atom_width(ui, cluster, font, emoji)),
        };
        // Once we know more content follows the budget must leave room
        // for the ellipsis; the final unit may use the full `section_w`.
        let is_last_unit = unit_end >= atoms.len();
        let limit = if is_last_unit {
            section_w
        } else {
            budget_with_ellipsis
        };
        if acc + unit_w > limit {
            // The whole unit doesn't fit. A `Char` run is divisible —
            // keep as many leading characters as the budget allows so a
            // single long unstyled string (no separators) still shows a
            // prefix rather than collapsing to a bare `…`. Indivisible
            // units (Space / FixedGap / Emoji) are simply dropped.
            if let DrawAtom::Char(_, style) = &atoms[i] {
                // Always reserve ellipsis room here — splitting a run
                // means content is being dropped, so `…` will follow.
                let char_budget = budget_with_ellipsis;
                for k in i..unit_end {
                    if let DrawAtom::Char(ch, _) = &atoms[k] {
                        let cw = ui.fonts(|f| f.glyph_width(font, *ch));
                        if acc + cw > char_budget {
                            break;
                        }
                        acc += cw;
                        kept.push(DrawAtom::Char(*ch, style.clone()));
                    }
                }
            }
            truncated = true;
            break;
        }
        acc += unit_w;
        kept.extend(atoms[i..unit_end].iter().cloned());
        i = unit_end;
    }

    (kept, truncated)
}

/// Measure the width `draw_atoms` will actually occupy, mirroring its
/// run-coalescing: consecutive same-style `Char`s are measured as one
/// galley (matching how they're painted), so the figure agrees with the
/// rendered width even when per-glyph advances drift from the galley's
/// laid-out width.
fn measure_atoms_drawn(
    ui: &egui::Ui,
    atoms: &[DrawAtom],
    font: &FontId,
    emoji: Option<&EmojiResources<'_>>,
) -> f32 {
    let mut total = 0.0;
    let mut i = 0;
    while i < atoms.len() {
        match &atoms[i] {
            DrawAtom::Char(_, style) => {
                let mut text = String::new();
                while i < atoms.len() {
                    if let DrawAtom::Char(ch, st) = &atoms[i] {
                        if st == style {
                            text.push(*ch);
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                total += galley_width(ui, &text, font, style);
            }
            DrawAtom::Space => {
                total += space_width(ui, font);
                i += 1;
            }
            DrawAtom::FixedGap(w) => {
                total += *w;
                i += 1;
            }
            DrawAtom::Emoji(cluster, _) => {
                total += emoji_atom_width(ui, cluster, font, emoji);
                i += 1;
            }
        }
    }
    total
}

/// Lay out `text` as a single non-wrapping galley and return its width
/// in egui logical points. Matches how `emit_styled_atom` coalesces a
/// same-style run into one `ui.label` galley; the monospace font means
/// bold / italic don't change the advance, so style is irrelevant to the
/// width and is omitted here.
fn galley_width(ui: &egui::Ui, text: &str, font: &FontId, _style: &AtomStyle) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let galley =
        ui.fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE));
    galley.size().x
}

/// Draw atoms left-to-right, coalescing consecutive same-style chars
/// into one label so kerning / pixel-snapping match a single text node.
fn draw_atoms(
    ui: &mut egui::Ui,
    atoms: &[DrawAtom],
    font: &FontId,
    emoji: Option<&EmojiResources<'_>>,
) {
    let mut i = 0;
    while i < atoms.len() {
        match &atoms[i] {
            DrawAtom::Char(_, style) => {
                let mut text = String::new();
                while i < atoms.len() {
                    if let DrawAtom::Char(ch, st) = &atoms[i] {
                        if st == style {
                            text.push(*ch);
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                emit_styled_atom(ui, &text, font, style);
            }
            DrawAtom::Space => {
                ui.add_space(space_width(ui, font));
                i += 1;
            }
            DrawAtom::FixedGap(w) => {
                ui.add_space(*w);
                i += 1;
            }
            DrawAtom::Emoji(cluster, style) => {
                emit_emoji_cluster_chain(ui, cluster, font, emoji, |ui, fallback| {
                    emit_styled_atom(ui, fallback, font, style);
                });
                i += 1;
            }
        }
    }
}

fn emit_styled_atom(ui: &mut egui::Ui, text: &str, font: &FontId, style: &AtomStyle) {
    if text.is_empty() {
        return;
    }
    let mut rt = RichText::new(text).font(font.clone());
    if style.bold {
        rt = rt.strong();
    }
    if style.italic {
        rt = rt.italics();
    }
    if style.underline {
        rt = rt.underline();
    }
    if let Some(color) = style.color {
        rt = rt.color(color);
    }
    ui.label(rt);
}

/// Walk an emoji span by grapheme cluster and emit one `Image` per
/// cluster (via the texture cache). Clusters the cache cannot
/// rasterize get re-rendered through `text_fallback`, which lets the
/// caller preserve the surrounding row's styling.
fn emit_emoji_cluster_chain<F>(
    ui: &mut egui::Ui,
    text: &str,
    font: &FontId,
    emoji: Option<&EmojiResources<'_>>,
    mut text_fallback: F,
) where
    F: FnMut(&mut egui::Ui, &str),
{
    use unicode_segmentation::UnicodeSegmentation;

    let Some(emoji) = emoji else {
        // No cache available (tests / startup race) — degrade to
        // egui text path.
        text_fallback(ui, text);
        return;
    };

    // Rasterize at physical-pixel resolution (logical size ×
    // pixels_per_point) and blit the texture 1:1 — display each glyph
    // at its real texel dimensions ÷ ppp so egui never up/down-scales
    // it. Forcing a fixed `font.size` square via `fit_to_exact_size`
    // squished swash's (slightly larger, non-square) emoji bitmap.
    //
    // We also snap the paint rect's origin to the physical-pixel grid.
    // `texels / ppp` is an exact-integer physical size, so snapping the
    // origin lands both edges on pixel boundaries — without it the
    // sub-pixel x offset of each cluster in the horizontal strip made
    // `TextureOptions::LINEAR` blend with neighbouring texels, so the
    // same cached glyph looked crisp at one x and blurry at another.
    let ppp = ui.ctx().pixels_per_point();
    // Emoji are sized off the CSS-compatible point value, matching the
    // WebView: there a color emoji fills the `font-size` em box, which
    // the browser scales by 96/72 (pt -> px). egui's `font.size` is
    // already in logical points (px-equivalent), so without the
    // `PT_TO_PX` factor the glyph renders 72/96 smaller than the
    // WebView. The cache supersamples + Lanczos3-downscales internally
    // so this target size stays sharp.
    let emoji_pt = font.size * crate::settings::PT_TO_PX;
    let raster_px = emoji_pt * ppp;
    let snap = |v: f32| (v * ppp).round() / ppp;
    // Atoms are always painted left-to-right (right alignment is handled
    // by `draw_section` wrapping the kept atoms in a sized LTR child), so
    // a forward cluster walk keeps `🤖🎉` reading as `🤖🎉`.
    for cluster in text.graphemes(true) {
        let handle = emoji.cache.lock().get_or_rasterize(
            ui.ctx(),
            emoji.rasterizer,
            emoji.fallback,
            cluster,
            raster_px,
        );
        match handle {
            Some(texture) => {
                let size_pts = texture.size_vec2() / ppp;
                // Align the layout cursor to the physical-pixel grid
                // BEFORE allocating, so the reserved slot and the painted
                // rect share the same grid-snapped origin — otherwise the
                // glyph paints up to half a pixel off its allocated slot
                // relative to adjacent text. `size_pts` is an exact
                // integer pixel count (texels / ppp), so once the origin
                // is aligned the advance keeps the next widget on-grid
                // too. `add_space` only moves forward, hence `ceil`; this
                // is meaningful only in a left-to-right layout (the
                // right-to-left sections fall back to paint-only snap).
                if ui.layout().main_dir() == egui::Direction::LeftToRight {
                    let cursor_x = ui.cursor().min.x;
                    let pad = (cursor_x * ppp).ceil() / ppp - cursor_x;
                    if pad > 0.0 {
                        ui.add_space(pad);
                    }
                }
                let (rect, _resp) = ui.allocate_exact_size(size_pts, egui::Sense::hover());
                let snapped = egui::Rect::from_min_size(
                    egui::pos2(snap(rect.min.x), snap(rect.min.y)),
                    size_pts,
                );
                Image::new(&texture).paint_at(ui, snapped);
            }
            None => {
                // Cluster missing from the emoji font — degrade to
                // text so the user at least sees a placeholder.
                text_fallback(ui, cluster);
            }
        }
    }
}

fn css_color_to_color32(color: &CssColor) -> Color32 {
    color.to_egui().unwrap_or(Color32::LIGHT_GRAY)
}

#[cfg(test)]
mod tests;
