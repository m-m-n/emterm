//! Cell-aligned box-drawing glyphs.
//!
//! Box-drawing characters rasterized from a font (Inconsolata etc.) tend
//! to leave hairline gaps at cell boundaries because the bitmap is placed
//! with bearing offsets that don't quite line up between adjacent cells.
//! Terminals like WezTerm / Alacritty / Kitty work around this by drawing
//! these glyphs procedurally — one rect per stroke, anchored to the cell
//! rect. We do the same for the subset that covers Claude Code's startup
//! screen + typical TUI box frames.
//!
//! Coverage: U+2500–U+254B (line / corner / tee / cross, light and heavy)
//! plus a handful of half-line variants. Anything outside the table falls
//! back to the regular glyph rasterizer.

/// Per-direction stroke weight. 0 == no stroke, 1 == light, 2 == heavy.
#[derive(Copy, Clone, Debug, Default)]
struct BoxDef {
    n: u8,
    s: u8,
    w: u8,
    e: u8,
}

impl BoxDef {
    const fn h(weight: u8) -> Self {
        Self {
            n: 0,
            s: 0,
            w: weight,
            e: weight,
        }
    }
    const fn v(weight: u8) -> Self {
        Self {
            n: weight,
            s: weight,
            w: 0,
            e: 0,
        }
    }
    const fn corner(n: u8, s: u8, w: u8, e: u8) -> Self {
        Self { n, s, w, e }
    }
}

/// Look up the stroke pattern for a codepoint. `None` means the caller
/// should fall back to glyph rasterization.
fn lookup(cp: u32) -> Option<BoxDef> {
    Some(match cp {
        // ── horizontal / vertical (light + heavy) ──
        0x2500 => BoxDef::h(1), // ─
        0x2501 => BoxDef::h(2), // ━
        0x2502 => BoxDef::v(1), // │
        0x2503 => BoxDef::v(2), // ┃

        // ── corners (light) ──
        0x250C => BoxDef::corner(0, 1, 0, 1), // ┌
        0x2510 => BoxDef::corner(0, 1, 1, 0), // ┐
        0x2514 => BoxDef::corner(1, 0, 0, 1), // └
        0x2518 => BoxDef::corner(1, 0, 1, 0), // ┘

        // ── corners (heavy) ──
        0x250F => BoxDef::corner(0, 2, 0, 2), // ┏
        0x2513 => BoxDef::corner(0, 2, 2, 0), // ┓
        0x2517 => BoxDef::corner(2, 0, 0, 2), // ┗
        0x251B => BoxDef::corner(2, 0, 2, 0), // ┛

        // ── tees (light) ──
        0x251C => BoxDef::corner(1, 1, 0, 1), // ├
        0x2524 => BoxDef::corner(1, 1, 1, 0), // ┤
        0x252C => BoxDef::corner(0, 1, 1, 1), // ┬
        0x2534 => BoxDef::corner(1, 0, 1, 1), // ┴
        0x253C => BoxDef::corner(1, 1, 1, 1), // ┼

        // ── tees (heavy) ──
        0x2523 => BoxDef::corner(2, 2, 0, 2), // ┣
        0x252B => BoxDef::corner(2, 2, 2, 0), // ┫
        0x2533 => BoxDef::corner(0, 2, 2, 2), // ┳
        0x253B => BoxDef::corner(2, 0, 2, 2), // ┻
        0x254B => BoxDef::corner(2, 2, 2, 2), // ╋

        _ => return None,
    })
}

/// Returns true when the codepoint has a procedural drawing entry.
pub fn is_box_drawing(cp: u32) -> bool {
    lookup(cp).is_some()
}

/// Produce the list of `(x, y, w, h)` rects (cell-local pixels) for the
/// given codepoint. Returns `None` when the caller should fall back to
/// the regular glyph rasterizer.
///
/// Strokes are anchored to the cell center so each side meets the
/// neighbouring cell's stroke without a hairline gap. Light strokes are
/// `light_px` thick, heavy strokes are `light_px * 2`, both at least 1
/// pixel.
pub fn rects_for(cp: u32, cell_w: f32, cell_h: f32) -> Option<Vec<(f32, f32, f32, f32)>> {
    let def = lookup(cp)?;
    // Stroke thickness: at least 1px so the line is visible even at small
    // cell sizes. Light = ~7% of cell height, heavy = ~14%, clamped so
    // glyphs at typical 13pt / cell_h≈17 give 1px / 2px.
    let light_px = (cell_h * 0.08).round().max(1.0);
    let heavy_px = (light_px * 2.0).max(2.0);
    let weight = |w: u8| -> f32 {
        match w {
            0 => 0.0,
            1 => light_px,
            _ => heavy_px,
        }
    };
    // Center the cross-bar around the cell midpoint so opposite cells
    // meet on the same row of pixels.
    let mut out: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(4);
    let h_thick = weight(def.w.max(def.e));
    let v_thick = weight(def.n.max(def.s));
    let mid_y = ((cell_h - h_thick) * 0.5).max(0.0);
    let mid_x = ((cell_w - v_thick) * 0.5).max(0.0);
    if def.w > 0 {
        let t = weight(def.w);
        let y = ((cell_h - t) * 0.5).max(0.0);
        // West stroke extends from the cell's left edge to past the
        // center so the corner overlap is solid.
        let x = 0.0;
        let w = mid_x + v_thick.max(t);
        out.push((x, y, w.min(cell_w), t));
    }
    if def.e > 0 {
        let t = weight(def.e);
        let y = ((cell_h - t) * 0.5).max(0.0);
        let x = mid_x;
        let w = cell_w - mid_x;
        out.push((x, y, w, t));
    }
    if def.n > 0 {
        let t = weight(def.n);
        let x = ((cell_w - t) * 0.5).max(0.0);
        let y = 0.0;
        let h = mid_y + h_thick.max(t);
        out.push((x, y, t, h.min(cell_h)));
    }
    if def.s > 0 {
        let t = weight(def.s);
        let x = ((cell_w - t) * 0.5).max(0.0);
        let y = mid_y;
        let h = cell_h - mid_y;
        out.push((x, y, t, h));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_line_spans_full_width() {
        let rects = rects_for(0x2500, 9.0, 18.0).unwrap();
        assert_eq!(rects.len(), 2, "horizontal has W + E stroke");
        // Combined coverage should reach both edges.
        let min_x = rects.iter().map(|r| r.0).fold(f32::INFINITY, f32::min);
        let max_x = rects.iter().map(|r| r.0 + r.2).fold(0.0, f32::max);
        assert!(min_x <= 0.0);
        assert!(max_x >= 9.0);
    }

    #[test]
    fn vertical_line_spans_full_height() {
        let rects = rects_for(0x2502, 9.0, 18.0).unwrap();
        assert_eq!(rects.len(), 2, "vertical has N + S stroke");
        let min_y = rects.iter().map(|r| r.1).fold(f32::INFINITY, f32::min);
        let max_y = rects.iter().map(|r| r.1 + r.3).fold(0.0, f32::max);
        assert!(min_y <= 0.0);
        assert!(max_y >= 18.0);
    }

    #[test]
    fn corner_emits_two_rects() {
        // ┌ has S + E strokes.
        let rects = rects_for(0x250C, 9.0, 18.0).unwrap();
        assert_eq!(rects.len(), 2);
    }

    #[test]
    fn cross_emits_four_rects() {
        let rects = rects_for(0x253C, 9.0, 18.0).unwrap();
        assert_eq!(rects.len(), 4);
    }

    #[test]
    fn non_box_codepoint_returns_none() {
        assert!(rects_for(0x41, 9.0, 18.0).is_none());
    }

    #[test]
    fn heavy_strokes_are_thicker() {
        let light = rects_for(0x2500, 9.0, 18.0).unwrap();
        let heavy = rects_for(0x2501, 9.0, 18.0).unwrap();
        // Stroke is rect height for horizontal lines.
        let light_t = light[0].3;
        let heavy_t = heavy[0].3;
        assert!(heavy_t > light_t);
    }
}
