//! High-quality color-emoji downscale.
//!
//! swash rasterizes color emoji from a font's CBDT bitmap strike via
//! `StrikeWith::BestFit`, which is a single-tap bilinear scale. That is
//! fine for gentle reductions but softens badly at the ~10x reductions a
//! small status-bar glyph needs (a 128px strike → ~12px). The fix mirrors
//! what the WebView/Skia path does: rasterize near the strike's native
//! size (cheap, gentle swash scaling) and do the large reduction here with
//! a Lanczos3 filter (proper area averaging).
//!
//! This lives in `render::` alongside the rasterizer / glyph cache / atlas
//! because it is pixel-level image processing, not UI layout. The status
//! bar's `ui::emoji_cache` is a thin coordinator that calls into here.

use image::{ImageBuffer, Rgba, imageops::FilterType, imageops::resize};

/// Resolution swash rasterizes color emoji at before the Lanczos3
/// downscale. Rasterizing near the strike's native size keeps swash's own
/// scaling gentle, then [`lanczos3_downscale_rgba`] handles the large part
/// — matching the WebView/Skia area-averaging quality. A requested display
/// size at or above this needs no supersample (swash's scaling is already
/// gentle).
pub const HQ_SOURCE_PX: f32 = 96.0;

/// Lanczos3 downscale of a straight-alpha RGBA buffer. Alpha is
/// premultiplied before resampling and un-premultiplied after so the
/// transparent emoji border doesn't bleed dark/colored fringes into
/// the visible pixels (straight-alpha resampling would average RGB
/// from fully-transparent texels).
pub fn lanczos3_downscale_rgba(
    src_w: u32,
    src_h: u32,
    src_rgba: &[u8],
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    // Premultiply.
    let mut pm = Vec::with_capacity(src_rgba.len());
    for px in src_rgba.chunks_exact(4) {
        let a = px[3] as u16;
        pm.push((px[0] as u16 * a / 255) as u8);
        pm.push((px[1] as u16 * a / 255) as u8);
        pm.push((px[2] as u16 * a / 255) as u8);
        pm.push(px[3]);
    }
    // `pm` is built directly above as exactly `src_w * src_h * 4` bytes, so
    // `from_raw` cannot fail. Assert it rather than returning a src-sized
    // buffer the caller would feed to `ColorImage` against the (different)
    // dst dimensions, which egui would reject with a panic.
    let src: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(src_w, src_h, pm)
        .expect("premultiplied RGBA buffer is exactly src_w*src_h*4 bytes");
    let dst = resize(&src, dst_w, dst_h, FilterType::Lanczos3);

    // Un-premultiply.
    let mut out = dst.into_raw();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u16;
        if a > 0 {
            px[0] = ((px[0] as u16 * 255 + a / 2) / a).min(255) as u8;
            px[1] = ((px[1] as u16 * 255 + a / 2) / a).min(255) as u8;
            px[2] = ((px[2] as u16 * 255 + a / 2) / a).min(255) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_halves_dimensions_and_preserves_rgba_length() {
        // 2x2 opaque red → 1x1.
        let src = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
        let out = lanczos3_downscale_rgba(2, 2, &src, 1, 1);
        assert_eq!(out.len(), 4, "1x1 RGBA must be 4 bytes");
        assert_eq!(out[3], 255, "opaque source stays opaque");
    }

    #[test]
    fn transparent_border_does_not_bleed_into_color() {
        // Left column opaque white, right column fully transparent.
        // Premultiplied resampling must keep the surviving pixel white,
        // not a darkened average with the transparent texels' RGB.
        let src = vec![
            255, 255, 255, 255, 0, 0, 0, 0, // row 0
            255, 255, 255, 255, 0, 0, 0, 0, // row 1
        ];
        let out = lanczos3_downscale_rgba(2, 2, &src, 1, 1);
        assert_eq!(out.len(), 4);
        // With premultiplied resampling the un-premultiplied RGB stays at
        // (or very near) white; a straight-alpha bug would pull it grey.
        assert!(
            out[0] > 200,
            "RGB must not be darkened by transparent texels: {out:?}"
        );
    }
}
