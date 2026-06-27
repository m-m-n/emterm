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

/// Single source of truth for the light-stroke thickness used by every
/// thin line the renderer paints — procedural box-drawing strokes
/// (`rects_for`) and the SGR underline / strikethrough bands in
/// `terminal_grid_pass`. Keep callers funneled through this so the two
/// stay in sync as the formula evolves (HiDPI scale, font size).
///
/// `cell_h` is in physical pixels (logical cell height × pixels_per_point):
///   cell_h≈17 (13pt, scale=1.0) → 1px
///   cell_h≈34 (13pt, scale=2.0) → 2px
///   cell_h≈51 (13pt, scale=3.0) → 3px
#[inline]
pub fn light_stroke_px(cell_h: f32) -> f32 {
    (cell_h / 18.0).round().max(1.0)
}

/// Per-direction stroke weight. 0 == no stroke, 1 == light, 2 == heavy.
#[derive(Copy, Clone, Debug, Default)]
struct BoxDef {
    n: u8,
    s: u8,
    w: u8,
    e: u8,
    /// When true, each stroke stops short of the bend so the corner
    /// pixel itself is left empty. The eye reads that gap as an arc
    /// at hairline weight.
    arc: bool,
}

impl BoxDef {
    const fn h(weight: u8) -> Self {
        Self {
            n: 0,
            s: 0,
            w: weight,
            e: weight,
            arc: false,
        }
    }
    const fn v(weight: u8) -> Self {
        Self {
            n: weight,
            s: weight,
            w: 0,
            e: 0,
            arc: false,
        }
    }
    const fn corner(n: u8, s: u8, w: u8, e: u8) -> Self {
        Self {
            n,
            s,
            w,
            e,
            arc: false,
        }
    }
    const fn arc_corner(n: u8, s: u8, w: u8, e: u8) -> Self {
        Self {
            n,
            s,
            w,
            e,
            arc: true,
        }
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

        // ── arc corners (light, U+256D–U+2570) ──
        // Same stroke layout as the sharp corners, but `arc: true` tells
        // the rasterizer to omit the bend pixel — both strokes stop short
        // of the cross-bar so a 1px diagonal gap forms at the corner.
        // At hairline weight the eye reads that as a rounded corner.
        0x256D => BoxDef::arc_corner(0, 1, 0, 1), // ╭
        0x256E => BoxDef::arc_corner(0, 1, 1, 0), // ╮
        0x256F => BoxDef::arc_corner(1, 0, 1, 0), // ╯
        0x2570 => BoxDef::arc_corner(1, 0, 0, 1), // ╰

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
    // Stroke thickness: keep the line a hairline regardless of HiDPI
    // scale / font size. Earlier `cell_h * 0.08` rounded up to 2-3px on
    // HiDPI / large fonts, which read as a thick line rather than the
    // crisp hairline Alacritty / WezTerm draw at the same scale. The
    // shared `light_stroke_px` is the SSOT — see its doc comment.
    let light_px = light_stroke_px(cell_h);
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
    // Sharp corners overlap solidly at the bend by extending each stroke
    // into the cross-bar. Arc corners do the opposite: each stroke stops
    // one cross-bar thickness short so the bend pixel is left empty.
    if def.w > 0 {
        let t = weight(def.w);
        let y = ((cell_h - t) * 0.5).max(0.0);
        let x = 0.0;
        let w = if def.arc {
            mid_x
        } else {
            mid_x + v_thick.max(t)
        };
        out.push((x, y, w.min(cell_w), t));
    }
    if def.e > 0 {
        let t = weight(def.e);
        let y = ((cell_h - t) * 0.5).max(0.0);
        let (x, w) = if def.arc {
            let start = mid_x + v_thick;
            (start, (cell_w - start).max(0.0))
        } else {
            (mid_x, cell_w - mid_x)
        };
        out.push((x, y, w, t));
    }
    if def.n > 0 {
        let t = weight(def.n);
        let x = ((cell_w - t) * 0.5).max(0.0);
        let y = 0.0;
        let h = if def.arc {
            mid_y
        } else {
            mid_y + h_thick.max(t)
        };
        out.push((x, y, t, h.min(cell_h)));
    }
    if def.s > 0 {
        let t = weight(def.s);
        let x = ((cell_w - t) * 0.5).max(0.0);
        let (y, h) = if def.arc {
            let start = mid_y + h_thick;
            (start, (cell_h - start).max(0.0))
        } else {
            (mid_y, cell_h - mid_y)
        };
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
    fn arc_corners_emit_two_rects_like_sharp_corners() {
        // ╭ ╮ ╯ ╰ should each emit one horizontal + one vertical rect,
        // just like their sharp counterparts.
        for cp in [0x256D, 0x256E, 0x256F, 0x2570] {
            let rects = rects_for(cp, 9.0, 18.0).unwrap();
            assert_eq!(rects.len(), 2, "arc U+{:04X} should emit 2 rects", cp);
        }
    }

    #[test]
    fn arc_corner_omits_bend_pixel() {
        // The arc effect is produced by leaving the bend pixel empty.
        // Each arc/sharp pair is compared at a sample point inside that
        // pixel: sharp covers it, arc must not.
        let cell_w = 9.0_f32;
        let cell_h = 18.0_f32;
        let mid_x = ((cell_w - 1.0) * 0.5).max(0.0);
        let mid_y = ((cell_h - 1.0) * 0.5).max(0.0);
        // Pick a sample point inside the bend pixel (centerline column,
        // centerline row) and verify coverage.
        let sample = (mid_x + 0.25, mid_y + 0.25);
        let covers = |rects: &[(f32, f32, f32, f32)]| -> bool {
            rects.iter().any(|(x, y, w, h)| {
                sample.0 >= *x && sample.0 < x + w && sample.1 >= *y && sample.1 < y + h
            })
        };
        for (arc, sharp) in [
            (0x256D, 0x250C), // ╭ vs ┌
            (0x256E, 0x2510), // ╮ vs ┐
            (0x256F, 0x2518), // ╯ vs ┘
            (0x2570, 0x2514), // ╰ vs └
        ] {
            let arc_rects = rects_for(arc, cell_w, cell_h).unwrap();
            let sharp_rects = rects_for(sharp, cell_w, cell_h).unwrap();
            assert!(
                covers(&sharp_rects),
                "sharp U+{:04X} should cover bend pixel",
                sharp
            );
            assert!(
                !covers(&arc_rects),
                "arc U+{:04X} should NOT cover bend pixel",
                arc
            );
        }
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

    #[test]
    fn light_stroke_is_1px_at_default_cell() {
        // 13pt Inconsolata at scale=1.0 lands around cell_h≈17. The
        // light stroke must stay a 1px hairline so adjacent box-frame
        // lines don't read as bold rules.
        let rects = rects_for(0x2500, 9.0, 17.0).unwrap();
        assert!(
            (rects[0].3 - 1.0).abs() < 0.01,
            "light stroke should be 1px at cell_h=17"
        );
    }

    #[test]
    fn light_stroke_stays_thin_at_larger_font() {
        // 16pt at scale=1.0 pushes cell_h up to ~21. Before the
        // fix this rounded to a 2px line; the corrected formula keeps
        // it a 1px hairline so visual weight matches a 13pt frame.
        let rects = rects_for(0x2500, 11.0, 21.0).unwrap();
        assert!(
            (rects[0].3 - 1.0).abs() < 0.01,
            "light stroke should still be 1px at cell_h=21"
        );
    }

    #[test]
    fn light_stroke_scales_at_hidpi() {
        // scale=2.0 HiDPI (cell_h≈34): light should become 2 physical
        // px so the line stays 1 logical px (matches Alacritty / WezTerm
        // procedural-glyph behaviour on HiDPI compositors).
        let rects = rects_for(0x2500, 18.0, 34.0).unwrap();
        assert!(
            (rects[0].3 - 2.0).abs() < 0.01,
            "light stroke should be 2px at cell_h=34"
        );
    }
}
