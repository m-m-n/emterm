//! Cell-aligned Block Elements (U+2580–U+259F).
//!
//! Same motivation as [`super::box_drawing`]: rasterizing block elements
//! from a font leaves gaps at cell boundaries because Inconsolata + Noto
//! Sans JP don't quite agree on advance / bearing. Terminals draw these
//! procedurally so adjacent cells produce a continuous filled region
//! (claude-code's startup banner is the canary).
//!
//! Coverage:
//! - U+2580 / U+2584 / U+2588: upper / lower / full block
//! - U+2581–U+2587: lower N/8 block
//! - U+2589–U+258F: left N/8 block
//! - U+2590: right half block
//! - U+2591–U+2593: light / medium / dark shade (alpha-blended fg)
//! - U+2594 / U+2595: upper / right one-eighth block
//! - U+2596–U+259F: quadrant variants
//!
//! Each entry returns the list of `(x, y, w, h)` rects to paint inside
//! the cell (cell-local pixels) and an optional fg color override. The
//! override is `Some(...)` only for shade characters, which paint a
//! semi-transparent fg over the cell rect.

/// Lookup result: list of cell-local rects + optional alpha override
/// for the fg color (used by shade characters to apply partial alpha
/// over the cell's foreground without losing the cell's actual RGB).
type Rects = Vec<(f32, f32, f32, f32)>;
pub type Result = (Rects, Option<u8>);

/// Returns true when the codepoint has a procedural block-element entry.
pub fn is_block(cp: u32) -> bool {
    (0x2580..=0x259F).contains(&cp)
}

/// Build the rects + optional fg override for the codepoint. Returns
/// `None` when the caller should fall back to font rasterization.
///
/// `fg` may be supplied via the override to apply alpha (shades). The
/// rect list is exhaustive — caller emits one `PAGE_SOLID` instance per
/// rect with the FG-fill flag set.
pub fn rects_for(cp: u32, w: f32, h: f32) -> Option<Result> {
    // ── Lower N/8 blocks (▁▂▃▄▅▆▇█): bottom-anchored fills. ──
    if (0x2581..=0x2588).contains(&cp) {
        let n = (cp - 0x2580) as f32; // 1..=8
        let frac = n / 8.0;
        let fill_h = (h * frac).round().max(1.0);
        return Some((vec![(0.0, h - fill_h, w, fill_h)], None));
    }
    // ── Left N/8 blocks (▏▎▍▌▋▊▉): left-anchored fills (heaviest =
    //    full block, but that is 0x2588 covered above). ──
    if (0x2589..=0x258F).contains(&cp) {
        // 0x2589 = LEFT 7/8 .. 0x258F = LEFT 1/8
        let n = 8 - (cp - 0x2588) as i32; // 7..1
        let frac = n as f32 / 8.0;
        let fill_w = (w * frac).round().max(1.0);
        return Some((vec![(0.0, 0.0, fill_w, h)], None));
    }
    Some(match cp {
        // ── Halves ──
        0x2580 => (vec![(0.0, 0.0, w, (h * 0.5).round().max(1.0))], None), // ▀
        0x2590 => {
            // ▐ right half
            let half = (w * 0.5).round().max(1.0);
            (vec![(w - half, 0.0, half, h)], None)
        }

        // ── Eighth-edge bars ──
        0x2594 => (vec![(0.0, 0.0, w, (h * 0.125).round().max(1.0))], None), // ▔
        0x2595 => {
            // ▕ right 1/8
            let tw = (w * 0.125).round().max(1.0);
            (vec![(w - tw, 0.0, tw, h)], None)
        }

        // ── Shades: fill the whole cell, fg with partial alpha. ──
        0x2591 => (vec![(0.0, 0.0, w, h)], Some(0x40)), // ░ ~25%
        0x2592 => (vec![(0.0, 0.0, w, h)], Some(0x80)), // ▒ ~50%
        0x2593 => (vec![(0.0, 0.0, w, h)], Some(0xC0)), // ▓ ~75%

        // ── Quadrants (2x2 sub-cells) ──
        // Sub-cell boundaries at the cell midpoint. We over-extend each
        // sub-rect by zero (no gap) — adjacent cells/quadrants align on
        // the same pixel boundary because mid_w/mid_h are rounded.
        _ => match cp {
            0x2596 => quad(false, false, true, false, w, h), // ▖ LL
            0x2597 => quad(false, false, false, true, w, h), // ▗ LR
            0x2598 => quad(true, false, false, false, w, h), // ▘ UL
            0x2599 => quad(true, false, true, true, w, h),   // ▙ UL+LL+LR
            0x259A => quad(true, false, false, true, w, h),  // ▚ UL+LR
            0x259B => quad(true, true, true, false, w, h),   // ▛ UL+UR+LL
            0x259C => quad(true, true, false, true, w, h),   // ▜ UL+UR+LR
            0x259D => quad(false, true, false, false, w, h), // ▝ UR
            0x259E => quad(false, true, true, false, w, h),  // ▞ UR+LL
            0x259F => quad(false, true, true, true, w, h),   // ▟ UR+LL+LR
            _ => return None,
        },
    })
}

/// Build a list of quadrant rects from the four corner flags
/// `(upper_left, upper_right, lower_left, lower_right)`.
fn quad(ul: bool, ur: bool, ll: bool, lr: bool, w: f32, h: f32) -> Result {
    let mid_w = (w * 0.5).round();
    let mid_h = (h * 0.5).round();
    let mut rects: Rects = Vec::with_capacity(4);
    if ul {
        rects.push((0.0, 0.0, mid_w, mid_h));
    }
    if ur {
        rects.push((mid_w, 0.0, w - mid_w, mid_h));
    }
    if ll {
        rects.push((0.0, mid_h, mid_w, h - mid_h));
    }
    if lr {
        rects.push((mid_w, mid_h, w - mid_w, h - mid_h));
    }
    (rects, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_block_covers_cell() {
        let (rects, _) = rects_for(0x2588, 9.0, 18.0).unwrap();
        assert_eq!(rects.len(), 1);
        assert!((rects[0].2 - 9.0).abs() < 0.5);
        assert!((rects[0].3 - 18.0).abs() < 0.5);
    }

    #[test]
    fn lower_half_anchored_to_bottom() {
        let (rects, _) = rects_for(0x2584, 8.0, 16.0).unwrap();
        let (_, y, _, h) = rects[0];
        assert_eq!(y + h, 16.0);
    }

    #[test]
    fn left_seven_eighths_extends_from_left_edge() {
        let (rects, _) = rects_for(0x2589, 8.0, 16.0).unwrap();
        assert_eq!(rects[0].0, 0.0);
        assert!(rects[0].2 >= 5.0);
    }

    #[test]
    fn right_half_anchored_to_right_edge() {
        let (rects, _) = rects_for(0x2590, 8.0, 16.0).unwrap();
        let (x, _, w, _) = rects[0];
        assert_eq!(x + w, 8.0);
    }

    #[test]
    fn shade_returns_fg_alpha() {
        let (_, alpha) = rects_for(0x2592, 8.0, 16.0).unwrap();
        assert_eq!(alpha, Some(0x80));
    }

    #[test]
    fn quadrant_upper_left_plus_lower_right_emits_two_rects() {
        let (rects, _) = rects_for(0x259A, 8.0, 16.0).unwrap();
        assert_eq!(rects.len(), 2);
    }

    #[test]
    fn full_quadrant_set_covers_cell() {
        // ▟ = UR + LL + LR (3 quadrants); coverage should be 75% area.
        let (rects, _) = rects_for(0x259F, 8.0, 16.0).unwrap();
        let area: f32 = rects.iter().map(|r| r.2 * r.3).sum();
        let total = 8.0 * 16.0;
        assert!((area - total * 0.75).abs() < 1.0);
    }

    #[test]
    fn non_block_returns_none() {
        assert!(rects_for(0x41, 8.0, 16.0).is_none());
        assert!(rects_for(0x2500, 8.0, 16.0).is_none()); // box drawing
    }

    #[test]
    fn upper_eighth_anchored_to_top() {
        let (rects, _) = rects_for(0x2594, 8.0, 16.0).unwrap();
        assert_eq!(rects[0].1, 0.0);
    }
}
