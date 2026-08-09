//! CPU-side instance building: glyph shaping into `CellInstance`
//! lists, the per-row instance cache, and its scroll-event rotation.

use super::*;

/// Growth factor applied to the persistent instance / uniform GPU buffers
/// (task0003 FR4/AC-4) when the required upload size exceeds the buffer's
/// current capacity. `1.5` bounds the number of reallocations to
/// `O(log_1.5(n))` under monotone growth (à la common dynamic-array
/// implementations) while keeping the worst-case overshoot modest.
const BUFFER_GROWTH_FACTOR: f64 = 1.5;

/// Minimum buffer capacity in bytes. Keeps a small grid (a handful of
/// instances) from reallocating on every single-cell change by giving a
/// freshly created buffer reasonable headroom up front.
pub(in crate::render::terminal_grid_pass) const MIN_BUFFER_CAPACITY_BYTES: u64 = 4096;

/// Pure growth-policy function for the persistent GPU buffers (task0003
/// AC-4): given a buffer's current capacity and the byte size actually
/// required this frame, returns the capacity the buffer should be
/// (re)created at.
///
/// - Never returns less than `current_capacity` when `required` already
///   fits — capacity never decreases (shrinking is out of scope per the
///   task plan).
/// - Never returns less than `required` — always covers the required size.
/// - Grows geometrically (`required * BUFFER_GROWTH_FACTOR`, floored at
///   `MIN_BUFFER_CAPACITY_BYTES`) rather than to the exact `required` size,
///   so a monotonically growing grid triggers `O(log n)` reallocations
///   instead of one every frame.
pub(in crate::render::terminal_grid_pass) fn grow_capacity(current_capacity: u64, required: u64) -> u64 {
    if required <= current_capacity {
        return current_capacity;
    }
    let grown = ((required as f64) * BUFFER_GROWTH_FACTOR).ceil() as u64;
    grown.max(MIN_BUFFER_CAPACITY_BYTES).max(required)
}

/// One screen row's cached ready-to-upload instance data (task0003
/// FR3/FR4), split into the background list and foreground list.
///
/// The split preserves the two-pass ordering invariant
/// [`GridInstanceBuilder::build_instances`] relies on: concatenating every
/// row's `bg` list (in row order) followed by every row's `fg` list (in
/// row order) reproduces exactly the same instance sequence a from-scratch
/// `build_instances` call over the same cells would produce, because
/// `render::collect_cell_inputs` always emits cells in row-major order.
/// Without this split, reusing a per-row `[bg, fg]` pair *as a unit* would
/// let one row's bg quad land ahead of an *earlier* row's fg quad in
/// concatenation order, resurrecting the tall-glyph-overhang clipping bug
/// `build_instances`'s two-pass ordering was written to prevent (see its
/// doc comment).
#[derive(Debug, Clone, Default)]
pub(in crate::render::terminal_grid_pass) struct RowInstances {
    pub(in crate::render::terminal_grid_pass) bg: Vec<CellInstance>,
    pub(in crate::render::terminal_grid_pass) fg: Vec<CellInstance>,
}

/// Per-row instance cache (task0003 FR3/FR4). Keyed by screen row index
/// (`Vec` index == row); `None` means "not yet built against the current
/// content" and must be rebuilt before [`RowCache::concat_all`] can rely
/// on it.
///
/// Invalidation is driven entirely by the caller (`WindowHost::render`)
/// handing in the row set `App::dirty_rows_this_frame` already computed
/// for the skip decision (task0003 D2/D3): scroll, resize, font/theme
/// change, and fold+selection all already force that set to every row
/// (`0..rows`) upstream in `App`, so no separate "clear on resize" signal
/// is needed here — rebuilding every row via [`RowCache::resize`] +
/// [`GridInstanceBuilder::rebuild_dirty_rows`] IS the cache drop.
#[derive(Debug, Default)]
pub(in crate::render::terminal_grid_pass) struct RowCache {
    pub(in crate::render::terminal_grid_pass) rows: Vec<Option<RowInstances>>,
}

impl RowCache {
    /// Ensure the cache has exactly `row_count` slots. A size change (grid
    /// resize) drops every existing entry — positions and glyph metrics
    /// baked into old entries no longer apply to the new dimensions.
    pub(in crate::render::terminal_grid_pass) fn resize(&mut self, row_count: usize) {
        if self.rows.len() != row_count {
            self.rows = vec![None; row_count];
        }
    }

    /// Store freshly rebuilt instance data for `row`. No-op if `row` is
    /// out of range (defensive; callers keep `row < row_count`).
    pub(in crate::render::terminal_grid_pass) fn set(&mut self, row: u16, instances: RowInstances) {
        if let Some(slot) = self.rows.get_mut(row as usize) {
            *slot = Some(instances);
        }
    }

    /// Concatenate every cached row's instances into the two-pass order
    /// (see the [`RowCache`] doc): all backgrounds in row order, then all
    /// foregrounds in row order. Rows without a cached entry contribute
    /// nothing — production callers always rebuild the full dirty set
    /// before calling this, guaranteeing full population; the permissive
    /// behavior here just keeps this method panic-free for tests that
    /// exercise a partially populated cache.
    pub(in crate::render::terminal_grid_pass) fn concat_all(&self) -> Vec<CellInstance> {
        let mut bgs: Vec<CellInstance> = Vec::new();
        let mut fg: Vec<CellInstance> = Vec::new();
        for row in self.rows.iter().flatten() {
            bgs.extend_from_slice(&row.bg);
        }
        for row in self.rows.iter().flatten() {
            fg.extend_from_slice(&row.fg);
        }
        bgs.extend(fg);
        bgs
    }

    /// Rotate cached row entries to keep tracking term_core's full-screen
    /// single-line scroll optimization (task0006; see
    /// `ring_buffer::scroll_up_internal`'s `count == 1` full-screen path —
    /// the core shifts its own dirty bits + viewport mapping by this
    /// amount on the promise that the renderer shifts its representation
    /// the same way).
    ///
    /// `direction` / `count` come straight from
    /// `TerminalCore::get_scroll_event_direction()` /
    /// `get_scroll_event_count()`. `cell_h` is the same per-row pixel
    /// height [`GridInstanceBuilder::build_instances_split`] used to bake
    /// each cached [`CellInstance`]'s `cell_xy` — every instance's pixel
    /// position is `f(cell.row)`, so moving a row's cached entry to a
    /// different slot index alone would leave it painting at its OLD
    /// screen row's pixel position; this method also translates every
    /// kept instance's Y coordinate by `count * cell_h` so the moved data
    /// paints at its new row's position, exactly as a rebuild with the
    /// row field decremented by `count` would.
    ///
    /// `count == 0` (no pending event) is a no-op.
    ///
    /// Otherwise: every cached row entry moves `count` positions toward
    /// index 0 (`cache[i] = cache[i + count]`, Y-translated as above),
    /// and the last `count` slots become `None`. A vacated slot is "must
    /// rebuild" by [`RowCache`] construction even if the core's own dirty
    /// set happens not to name it — defense in depth; in practice the
    /// core's `mark_row_dirty(bottom)` call already names the vacated
    /// rows every time (task0006 Test Notes).
    ///
    /// An unrecognized `direction` (term_core does not currently emit
    /// anything but the "Up" encoding, but this consumer does not trust
    /// an unknown value to mean "Up") or a `count` that reaches/exceeds
    /// the row count degenerates to dropping every cached entry —
    /// correctness over the optimization when the up-shift assumption
    /// cannot be trusted.
    pub(in crate::render::terminal_grid_pass) fn rotate_for_scroll_event(&mut self, direction: u8, count: u16, cell_h: f32) {
        if count == 0 {
            return;
        }
        let len = self.rows.len();
        if len == 0 {
            return;
        }
        if direction != SCROLL_DIRECTION_UP || count as usize >= len {
            for slot in &mut self.rows {
                *slot = None;
            }
            return;
        }
        let count = count as usize;
        self.rows.rotate_left(count);
        let shift_up = count as f32 * cell_h;
        for row in self.rows[..len - count].iter_mut().flatten() {
            for instance in row.bg.iter_mut().chain(row.fg.iter_mut()) {
                instance.cell_xy[1] -= shift_up;
            }
        }
        for slot in &mut self.rows[len - count..] {
            *slot = None;
        }
    }
}

/// Direction code emitted by `TerminalCore::get_scroll_event_direction()`
/// for an "Up" full-screen single-line scroll
/// (`ring_buffer::ScrollDirection::Up` encodes as `1`; `0` means "no
/// event"). term_core does not currently emit any other non-zero code;
/// [`RowCache::rotate_for_scroll_event`] treats anything else as
/// untrusted and degenerates to a full cache drop rather than silently
/// assuming "Up" semantics for an unrecognized encoding (task0006 AC-2).
pub(in crate::render::terminal_grid_pass) const SCROLL_DIRECTION_UP: u8 = 1;

/// CPU-side (device-free) half of [`TerminalGridPass`]: glyph shaping plus
/// the task0003 per-row instance cache. Split out from the GPU-owning
/// struct so unit tests can exercise the row-cache rebuild logic (TS-4 /
/// TS-5) directly against the real implementation instead of a hand-
/// maintained mirror — `TerminalGridPass::new` is the only piece that
/// actually needs a wgpu device (pipeline + bind-group-layout + sampler).
pub(in crate::render::terminal_grid_pass) struct GridInstanceBuilder {
    /// Cache + atlas live behind a mutex so the App can hand the same Arc
    /// to multiple consumers (Phase 5+). Rasterization calls
    /// `cache.get_or_rasterize` during a row (re)build.
    pub(in crate::render::terminal_grid_pass) cache: Arc<Mutex<GlyphCache>>,
    /// Resolved fallback chain consulted per grapheme cluster.
    fallback: Arc<FallbackChain>,
    /// Active rasterizer (Swash or AbGlyph, picked at startup from
    /// `Settings::font_engine`).
    rasterizer: Arc<dyn GlyphRasterizer>,
    /// Per-row instance cache (task0003 FR3/FR4).
    row_cache: RowCache,
}

impl GridInstanceBuilder {
    pub(in crate::render::terminal_grid_pass) fn new(
        cache: Arc<Mutex<GlyphCache>>,
        fallback: Arc<FallbackChain>,
        rasterizer: Arc<dyn GlyphRasterizer>,
    ) -> Self {
        Self {
            cache,
            fallback,
            rasterizer,
            row_cache: RowCache::default(),
        }
    }

    /// CPU-side build path (no GPU). Computes the instance list for the
    /// supplied grid input.
    ///
    /// This split exists so unit tests can exercise the per-cell pipeline
    /// (TS-font-13 / TS-font-14) without standing up a wgpu device.
    pub(in crate::render::terminal_grid_pass) fn build_instances(&self, cells: &[CellInput], metrics: CellMetrics) -> Vec<CellInstance> {
        let (mut bgs, fg) = self.build_instances_split(cells, metrics);
        bgs.extend(fg);
        bgs
    }

    /// Split form of [`Self::build_instances`]: returns the background and
    /// foreground instance lists separately instead of concatenating them.
    /// Shared by the full-grid path (`build_instances`) and the per-row
    /// cache rebuild ([`Self::rebuild_dirty_rows`]) so both stay byte-for-
    /// byte consistent with each other by construction — a single row's
    /// split output, concatenated with every other row's in row order (see
    /// [`RowCache`]), reproduces exactly what a monolithic `build_instances`
    /// call over the same cells would produce.
    fn build_instances_split(
        &self,
        cells: &[CellInput],
        metrics: CellMetrics,
    ) -> (Vec<CellInstance>, Vec<CellInstance>) {
        // Two-pass instance ordering: all background quads first, then
        // every foreground quad (glyphs, box / block-drawing strokes,
        // decoration lines). Without this split, the per-cell `[bg,
        // glyph, deco]` interleave meant row N+1's bg quad was pushed
        // AFTER row N's glyph quad — and since the pass runs without a
        // depth test and instances draw in submission order, the next
        // row's bg overwrote any glyph that overflowed cell_h. Tall
        // single-cell glyphs (U+25FB ◻ from Noto Sans Symbols 2 / Noto
        // Emoji, CJK descenders) had their bottom edge erased.
        //
        // Two passes give Alpha / RGBA glyph quads a clean natural
        // overhang (their fragment output drops to alpha=0 outside the
        // covered pixels, so no bg leaks into adjacent rows). The
        // subpixel path is opaque across the whole quad — see the Y
        // clip added in `glyph_instance` for that page.
        let mut bgs = Vec::with_capacity(cells.len());
        let mut fg = Vec::with_capacity(cells.len() * 2);
        let mut cache = self.cache.lock();
        // Pre-compute the per-cell baseline using the base font's real
        // ascent + line height. Without this we used the rough
        // `size_px * 0.8` approximation in every glyph, which made
        // glyphs from fonts with different intrinsic ascents
        // (Inconsolata vs Noto Sans JP vs Noto Color Emoji) drift
        // visibly inside the cell.
        let base_metrics = self
            .rasterizer
            .font_metrics(self.fallback.base(), metrics.font_size_px);
        let base_ascent = base_metrics
            .map(|m| m.ascent)
            .unwrap_or(metrics.font_size_px * 0.8);
        let base_line_height = base_metrics
            .map(|m| m.line_height())
            .unwrap_or(metrics.font_size_px);
        // Center the line vertically inside the cell so cells with a
        // small font but tall cell (e.g. cell_h=17 / line_height≈16)
        // get balanced top / bottom padding instead of the text being
        // anchored to the very top. `compute_v_pad` (task0002 AC-3) is
        // shared with `render::cursor::draw_block_cursor`'s overlay glyph
        // path so the two never drift apart on this formula again.
        let v_pad = compute_v_pad(metrics.cell_h, base_line_height);
        for cell in cells {
            let x = metrics.origin[0] + cell.col as f32 * metrics.cell_w;
            let y = metrics.origin[1] + cell.row as f32 * metrics.cell_h;
            let w = metrics.cell_w * (cell.width_cells.max(1) as f32);
            let h = metrics.cell_h;
            // Background quad → `bgs`. `bg_extend_below` extends the
            // bg downward so reverse-video preedit cells cover CJK
            // glyph descenders that naturally rasterize past `cell_h`.
            if cell.draw_background {
                let bg_h = h + cell.bg_extend_below.max(0.0);
                bgs.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, bg_h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.bg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: 0,
                });
            }
            // Glyph quad. Empty / whitespace clusters skip this.
            if !cell.glyph.is_empty() && cell.glyph != " " {
                // Box-drawing short-circuit: stroke the cell rect with
                // solid quads instead of rasterizing the font glyph so
                // adjacent cells meet without hairline gaps. Falls
                // through to the regular glyph path for non-box cps.
                let first_cp = cell.glyph.chars().next().map(|c| c as u32).unwrap_or(0);
                if let Some(rects) = crate::render::box_drawing::rects_for(first_cp, w, h) {
                    for (rx, ry, rw, rh) in rects {
                        fg.push(CellInstance {
                            cell_xy: [x + rx, y + ry],
                            cell_wh: [rw, rh],
                            atlas_uv: [0.0, 0.0, 0.0, 0.0],
                            fg_rgba: pack_rgba(cell.fg_rgba),
                            bg_rgba: pack_rgba(cell.bg_rgba),
                            page: PAGE_SOLID,
                            flags: FLAG_FG_FILL,
                        });
                    }
                } else if let Some((rects, alpha_override)) =
                    crate::render::block_drawing::rects_for(first_cp, w, h)
                {
                    // Shade characters supply an alpha override; we
                    // preserve the cell's fg RGB and patch only the A
                    // channel so the alpha-blend stage paints a
                    // partially-transparent fg fill over the bg.
                    let mut fgc = cell.fg_rgba;
                    if let Some(a) = alpha_override {
                        fgc[3] = a;
                    }
                    for (rx, ry, rw, rh) in rects {
                        fg.push(CellInstance {
                            cell_xy: [x + rx, y + ry],
                            cell_wh: [rw, rh],
                            atlas_uv: [0.0, 0.0, 0.0, 0.0],
                            fg_rgba: pack_rgba(fgc),
                            bg_rgba: pack_rgba(cell.bg_rgba),
                            page: PAGE_SOLID,
                            flags: FLAG_FG_FILL,
                        });
                    }
                } else if let Some(instance) = self.glyph_instance(
                    &mut cache,
                    cell,
                    x,
                    y,
                    w,
                    h,
                    metrics.font_size_px,
                    base_ascent,
                    v_pad,
                ) {
                    fg.push(instance);
                }
            }
            // Decoration lines: rendered as thin solid quads inside the
            // shader by branching on `flags`. We emit one decoration
            // instance per active decoration so the shader can place the
            // line at the correct sub-rect.
            if cell.underline {
                fg.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.fg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: FLAG_UNDERLINE,
                });
            }
            if cell.strikethrough {
                fg.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.fg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: FLAG_STRIKETHROUGH,
                });
            }
        }
        (bgs, fg)
    }

    /// Rebuild the cache entries for exactly `dirty_rows` from
    /// `dirty_cells` (already restricted to those rows by the caller — see
    /// `render::collect_cell_inputs`'s `only_rows` mode). Cells for each
    /// row must appear contiguously and in the same ascending order as
    /// `dirty_rows` — guaranteed when `dirty_cells` came from
    /// `collect_cell_inputs(..., Some(dirty_rows))`, since that function
    /// walks rows in the given order and `App::dirty_rows_this_frame`
    /// returns a sorted, deduplicated set. Returns the number of rows
    /// rebuilt (`== dirty_rows.len()`) for the `EMTERM_RENDER_PERF`
    /// rows-rebuilt counter.
    fn rebuild_dirty_rows(
        &mut self,
        dirty_rows: &[u16],
        dirty_cells: &[CellInput],
        metrics: CellMetrics,
        row_count: u16,
    ) -> usize {
        self.row_cache.resize(row_count as usize);
        if dirty_rows.is_empty() {
            return 0;
        }
        let mut idx = 0usize;
        for &row in dirty_rows {
            let start = idx;
            while idx < dirty_cells.len() && dirty_cells[idx].row == row {
                idx += 1;
            }
            let (bg, fg) = self.build_instances_split(&dirty_cells[start..idx], metrics);
            self.row_cache.set(row, RowInstances { bg, fg });
        }
        dirty_rows.len()
    }

    /// CPU-side entry point for the cached (non-preedit) render path:
    /// rebuild exactly the dirty rows, then concatenate the full per-row
    /// cache into one instance sequence. Returns `(instances,
    /// rows_rebuilt)`.
    pub(in crate::render::terminal_grid_pass) fn rebuild_and_collect(
        &mut self,
        dirty_rows: &[u16],
        dirty_cells: &[CellInput],
        metrics: CellMetrics,
        row_count: u16,
    ) -> (Vec<CellInstance>, usize) {
        let rebuilt = self.rebuild_dirty_rows(dirty_rows, dirty_cells, metrics, row_count);
        (self.row_cache.concat_all(), rebuilt)
    }

    /// Consume term_core's accumulated scroll event (task0006): delegates
    /// to [`RowCache::rotate_for_scroll_event`]. Called once per rendered
    /// frame by `window_host::render`, before either the ordinary or the
    /// IME-preedit-shadow dirty-row rebuild, so the per-row cache tracks
    /// the same up-shift the core's ring buffer already performed.
    /// `cell_h` must be the same value the caller's `CellMetrics` uses
    /// this frame — see [`RowCache::rotate_for_scroll_event`] for why the
    /// rotation needs it.
    pub(in crate::render::terminal_grid_pass) fn apply_scroll_event(&mut self, direction: u8, count: u16, cell_h: f32) {
        self.row_cache
            .rotate_for_scroll_event(direction, count, cell_h);
    }

    /// Resolve a single cell's glyph to a `CellInstance`. Returns `None`
    /// when no font in the fallback chain covers the cluster — caller
    /// emits no glyph instance (background + decoration still fire).
    ///
    /// `base_ascent` and `v_pad` are pre-computed by the caller from
    /// the base font's real metrics so all glyphs in the grid share a
    /// consistent baseline regardless of which fallback font supplied
    /// the bitmap.
    fn glyph_instance(
        &self,
        cache: &mut GlyphCache,
        cell: &CellInput,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        size_px: f32,
        base_ascent: f32,
        v_pad: f32,
    ) -> Option<CellInstance> {
        // Prefer the emoji font when the cluster carries VS-16, so that
        // codepoints with dual presentation (e.g. U+26A0 warning sign)
        // get the colored emoji glyph rather than the BW base-font one.
        let font_id = self
            .fallback
            .resolve_for_cluster(&*self.rasterizer, &cell.glyph)?;
        // FR5 (task0002): when the cluster resolved to the color emoji
        // font, strip U+FE0F (VS16) from the string handed to the shaper
        // — swash's ligature matcher does not skip default-ignorable
        // variation selectors, so a keycap cluster (`5 FE0F 20E3`) shaped
        // verbatim fails the `<digit> + 20E3` GSUB ligature match and
        // decomposes. Font selection above already ran against the FULL
        // cluster including VS16; only the shaping input changes here.
        let shaping_input = self.fallback.shaping_cluster(&cell.glyph, font_id);
        // SGR bold: swap in the resolved font's real bold face when one
        // is registered (e.g. Inconsolata → Inconsolata Bold). Coverage
        // is resolved on the regular face; the bold face of the same
        // family carries the same repertoire. Fonts without a bold
        // variant (bundled CJK, emoji) keep their regular face.
        let font_id = if cell.bold {
            self.fallback.bold_variant(font_id).unwrap_or(font_id)
        } else {
            font_id
        };
        let shaped = self.rasterizer.shape(&shaping_input, font_id, size_px);
        let g = shaped.first()?;
        if g.glyph_id == 0 {
            return None;
        }
        let key = GlyphKey::new(font_id, g.glyph_id, size_px, 0.0);
        let cached = cache.get_or_rasterize(&*self.rasterizer, key)?;
        let region = cached.region;
        let advance = cached.advance;
        if region.is_empty() {
            return None;
        }
        let page = match region.format {
            AtlasFormat::Alpha => PAGE_ALPHA,
            AtlasFormat::Rgba => PAGE_RGBA,
            AtlasFormat::Subpixel => PAGE_SUBPIXEL,
        };
        // UV rect inside the atlas page; converted from pixel space to
        // normalized [0..1] in the vertex shader using the uniform-side
        // page dimensions.
        let u0 = region.x as f32;
        let v0 = region.y as f32;
        let u1 = (region.x + region.width) as f32;
        let v1 = (region.y + region.height) as f32;
        // Place the glyph quad at its natural bitmap size + bearing
        // offset inside the cell rather than stretching the bitmap to
        // fill the cell. Baseline is anchored to the BASE font's real
        // ascent so all glyphs share a consistent horizontal line, with
        // `v_pad` centering the line vertically inside the cell.
        let mut glyph_w = region.width as f32;
        let mut glyph_h = region.height as f32;
        let baseline = y + v_pad + base_ascent;
        let mut glyph_x = x + region.bearing_left as f32;
        let mut glyph_y = baseline - region.bearing_top as f32;
        // Shrink the glyph quad according to `cell.fit`. See [`GlyphFit`]
        // for the variant semantics. Branch is fully predictable per
        // call site (`HorizontalOnly` for ordinary cells, `Both` for
        // IME preedit), so the per-cell cost is one match + one branch
        // + the divide; ASCII collapses to `sx = 1.0` because
        // `advance == cell_w` for monospace.
        if cell.fit.horizontal() && glyph_w > 0.0 && glyph_h > 0.0 {
            // Horizontal scale. The reference width depends on the
            // fit mode:
            //
            //   GlyphFit::HorizontalOnly — ordinary cells. Use the
            //   font's DESIGN advance so a hinted Latin monospace
            //   glyph (Inconsolata 'm' / 'w' at 13 pt) with bitmap
            //   11 px and advance 9 px sees `sx = 9/9 = 1.0` (no
            //   shrink — the 2 px AA overhang is fed to the
            //   subpixel-clip path further down). A CJK Dingbat
            //   fallback with `advance ≈ 1.5 × cell_w` sees `sx
            //   ≈ 0.67` and the entire bitmap (including its AA
            //   skirt) is scaled down to fit the cell footprint.
            //
            //   GlyphFit::Both — IME preedit overlay. The reverse-
            //   video bg must visibly enclose the WHOLE bitmap,
            //   AA overhang included, so the reference width is
            //   the bitmap pixel width (`glyph_w`). Without this,
            //   a preedit U+25BD ▽ whose bitmap exceeds the cell
            //   would have its right edge bleed past the highlight
            //   bg even though `advance == cell_w` — the regression
            //   the original `fit_glyph_to_cell` path was added to
            //   prevent.
            //
            // `advance.is_finite() && > 0.0` guards against the rare
            // case where the rasterizer reports a malformed advance
            // (e.g. `units_per_em == 0` → Inf) — falling back to
            // `glyph_w` keeps the math sound under such inputs.
            let h_reference = match cell.fit {
                GlyphFit::Both => glyph_w,
                _ if advance.is_finite() && advance > 0.0 => advance,
                _ => glyph_w,
            };
            let sx = (w / h_reference).min(1.0);
            // Vertical scale so the bitmap fits the cell height
            // *measured against where the glyph currently lands*.
            // Only kicks in when `cell.fit == GlyphFit::Both` (IME
            // preedit). Ordinary cells (`HorizontalOnly`) keep
            // vertical at 1.0 so a 1-2 px CJK descender doesn't
            // trigger a uniform 5-10% shrink that crushes the whole
            // glyph just to contain the descender.
            let sy = if cell.fit.vertical() {
                let top_overflow = (y - glyph_y).max(0.0);
                let bottom_overflow = ((glyph_y + glyph_h) - (y + h)).max(0.0);
                if top_overflow + bottom_overflow > 0.0 {
                    (h / (glyph_h + top_overflow + bottom_overflow)).min(1.0)
                } else {
                    1.0
                }
            } else {
                1.0
            };
            let scale = sx.min(sy);
            if scale < 1.0 {
                glyph_w *= scale;
                glyph_h *= scale;
                // Re-center horizontally inside the cell.
                glyph_x = x + (w - glyph_w) * 0.5;
                // Keep the baseline pinned so adjacent clusters with
                // different bitmap heights still line up — otherwise
                // each glyph would re-anchor to the cell's vertical
                // center and the row's baseline would jitter cluster
                // by cluster (visible as zig-zag during preedit).
                let scaled_bearing_top = region.bearing_top as f32 * scale;
                glyph_y = baseline - scaled_bearing_top;
                // If the scaled glyph still overshoots the cell rect
                // after baseline placement, clamp the top/bottom into
                // the cell so the reverse-video bg keeps it contained.
                let overshoot_top = (y - glyph_y).max(0.0);
                let overshoot_bot = ((glyph_y + glyph_h) - (y + h)).max(0.0);
                if overshoot_top > 0.0 {
                    glyph_y += overshoot_top;
                } else if overshoot_bot > 0.0 {
                    glyph_y -= overshoot_bot;
                }
            }
        }
        if glyph_w <= 0.0 || glyph_h <= 0.0 {
            return None;
        }
        // Snap the glyph quad to the physical pixel grid. The cell pitch
        // is fractional (e.g. 8.667 px), so unrounded quad origins land
        // between pixels and the Linear atlas sample smears every glyph
        // by a pixel — visibly blurry/washed-out at terminal sizes. The
        // quad size stays at the bitmap's integer size, so a snapped
        // origin gives an exact 1:1 texel-to-pixel mapping. Background
        // quads intentionally stay fractional (rounding them would open
        // hairline gaps between adjacent cells).
        let mut glyph_x = glyph_x.round();
        let mut glyph_y = glyph_y.round();
        let mut glyph_w = glyph_w;
        let mut glyph_h = glyph_h;
        let mut u0 = u0;
        let mut u1 = u1;
        let mut v0 = v0;
        let mut v1 = v1;
        // Subpixel glyphs: clip the quad to the cell rect in BOTH axes.
        // swash's hinted bitmaps can be wider than the cell (Inconsolata
        // 'm' / 'w' at 13 pt: left=-1, width=11 vs 9-px cells), and the
        // subpixel shader composites the FULL quad against the cell's bg
        // color opaquely — an overhanging quad would paint this cell's
        // bg outside the cell, visible as a 1-px bg-colored fringe next
        // to reverse-video runs (e.g. ls's /dev/shm highlight). Alpha /
        // RGBA pages alpha-blend (bg never leaks), so they keep the
        // natural overhang like the WebView build's Canvas fillText.
        //
        // The Y clip catches the analogous vertical bleed: tall glyphs
        // (U+25FB ◻, CJK descenders) whose bitmap exceeds cell_h would
        // otherwise paint this cell's bg color into the row above /
        // below as a colored stripe.
        if page == PAGE_SUBPIXEL {
            // Snap the cell bounds to the pixel grid before clipping. The
            // glyph quad is already pixel-snapped (integer origin + integer
            // bitmap width from .round() above), so comparing it against
            // UNrounded fractional cell bounds (which occur under fractional
            // HiDPI scale factors where cell_w = cell_w_logical × ppp) would
            // shave a sub-pixel sliver off every glyph and shift the UV off
            // the 1:1 texel mapping, causing per-glyph blur. Snapping makes
            // the comparison integer-vs-integer: fitting glyphs pass through
            // untouched and only true ≥1px overhang is trimmed.
            if let Some((cx, cw, cu0, cu1)) =
                clip_quad_to_cell_x(glyph_x, glyph_w, u0, u1, x.round(), (x + w).round())
            {
                glyph_x = cx;
                glyph_w = cw;
                u0 = cu0;
                u1 = cu1;
            } else {
                return None;
            }
            if let Some((cy, ch, cv0, cv1)) =
                clip_quad_to_cell_y(glyph_y, glyph_h, v0, v1, y.round(), (y + h).round())
            {
                glyph_y = cy;
                glyph_h = ch;
                v0 = cv0;
                v1 = cv1;
            } else {
                return None;
            }
        }
        Some(CellInstance {
            cell_xy: [glyph_x, glyph_y],
            cell_wh: [glyph_w, glyph_h],
            atlas_uv: [u0, v0, u1, v1],
            fg_rgba: pack_rgba(cell.fg_rgba),
            bg_rgba: pack_rgba(cell.bg_rgba),
            page,
            flags: 0,
        })
    }
}
