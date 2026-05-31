//! Status-bar emoji rendering helper.
//!
//! egui 0.29's text path is backed by `ab_glyph`, which cannot raster
//! color-emoji tables (CBDT/COLR). The terminal grid sidesteps this by
//! routing through `SwashRasterizer` and emitting RGBA quads via the
//! custom wgpu pass; the status bar, which draws with `ui.label()`,
//! has no such escape hatch.
//!
//! This module provides both halves of the workaround:
//!
//! 1. [`split_segments`] walks a text run by grapheme cluster and
//!    groups consecutive clusters into [`TextSegment::Text`] or
//!    [`TextSegment::Emoji`] spans. Text spans render unchanged via
//!    `ui.label()`; emoji spans go through the cache below.
//! 2. [`EmojiTextureCache`] memoizes `(cluster, size_px)` -> egui
//!    [`TextureHandle`] so each color-emoji glyph is rasterized once
//!    per font-size bucket and reused across frames.
//!
//! The cache lives on `App` (process-lifetime) so font updates only
//! take effect on the next app launch — matching the terminal grid's
//! `GlyphCache` policy. Call [`EmojiTextureCache::clear`] when the
//! user changes the active font size to drop stale textures.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use unicode_segmentation::UnicodeSegmentation;

use crate::render::emoji_resample::{lanczos3_downscale_rgba, HQ_SOURCE_PX};
use crate::render::font::fallback::FallbackChain;
use crate::render::font::traits::{AtlasFormat, GlyphRasterizer};

/// Pre-filter: a grapheme cluster is a color-emoji candidate when any
/// of its codepoints is pictographic or carries an emoji-forming
/// modifier.
///
/// The per-codepoint range test is delegated to
/// [`crate::render::font::fallback::is_pictographic`] so the two paths
/// share one source of truth. On top of that we recognise the
/// cluster-level modifiers that only make sense across a whole
/// grapheme:
/// - **VS-16** (U+FE0F) — the emoji-presentation selector that turns a
///   dual-presentation base (e.g. `⏏\u{FE0F}`) into color emoji.
/// - **Keycap** (U+20E3) — the combining enclosing keycap, e.g.
///   `1\u{FE0F}\u{20E3}` (`1️⃣`), whose base is a plain ASCII digit.
/// - **Regional indicators** (U+1F1E6..=U+1F1FF) — already caught by
///   `is_pictographic`'s `>= 0x1F000` tail, listed here for clarity.
///
/// This is only a prefilter: a candidate still has to be covered by the
/// emoji font (`FallbackChain::resolve_for_cluster`) to render as color,
/// so a generous match never mis-paints a non-emoji glyph.
pub fn cluster_is_emoji(cluster: &str) -> bool {
    use crate::render::font::fallback::is_pictographic;
    cluster.chars().any(|ch| {
        let cp = ch as u32;
        is_pictographic(cp) || cp == 0xFE0F || cp == 0x20E3
    })
}

/// One segment of a status-bar text run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSegment<'a> {
    /// Plain text that should render via egui's normal text path.
    Text(&'a str),
    /// One or more consecutive emoji grapheme clusters. Each cluster
    /// is rasterized separately by the consumer; this variant only
    /// groups them so the call site can iterate per-cluster without
    /// re-scanning the whole run.
    Emoji(&'a str),
}

/// Walk `text` by grapheme cluster and group consecutive clusters of
/// the same kind (text / emoji). Empty input returns an empty vector.
pub fn split_segments(text: &str) -> Vec<TextSegment<'_>> {
    let mut out: Vec<TextSegment<'_>> = Vec::new();
    let mut seg_start: Option<usize> = None;
    let mut seg_is_emoji = false;
    for (start, cluster) in text.grapheme_indices(true) {
        let is_emoji = cluster_is_emoji(cluster);
        match seg_start {
            Some(_) if seg_is_emoji == is_emoji => {}
            Some(prev_start) => {
                push_segment(&mut out, &text[prev_start..start], seg_is_emoji);
                seg_start = Some(start);
                seg_is_emoji = is_emoji;
            }
            None => {
                seg_start = Some(start);
                seg_is_emoji = is_emoji;
            }
        }
    }
    if let Some(prev_start) = seg_start {
        push_segment(&mut out, &text[prev_start..], seg_is_emoji);
    }
    out
}

fn push_segment<'a>(out: &mut Vec<TextSegment<'a>>, slice: &'a str, is_emoji: bool) {
    if slice.is_empty() {
        return;
    }
    out.push(if is_emoji {
        TextSegment::Emoji(slice)
    } else {
        TextSegment::Text(slice)
    });
}

/// Cache key: cluster string + rounded pixel size. `cluster` is owned
/// because the cache outlives the &str slices that produce it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EmojiKey {
    cluster: String,
    size_px: u32,
}

/// Process-lifetime cache of swash-rasterized emoji textures.
///
/// `None` entries memoize "rasterizer / fallback chain could not
/// produce a glyph" so we don't retry on every frame for a cluster
/// the bundled emoji font doesn't cover.
#[derive(Default)]
pub struct EmojiTextureCache {
    entries: HashMap<EmojiKey, Option<TextureHandle>>,
}

impl EmojiTextureCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or rasterize an emoji cluster. Returns `None` when no font
    /// in the fallback chain covers the cluster (e.g. the bundled
    /// emoji font is missing this glyph) — caller should fall back
    /// to plain-text rendering in that case.
    pub fn get_or_rasterize(
        &mut self,
        ctx: &Context,
        rasterizer: &dyn GlyphRasterizer,
        fallback: &FallbackChain,
        cluster: &str,
        size_px: f32,
    ) -> Option<TextureHandle> {
        let key = EmojiKey {
            cluster: cluster.to_string(),
            size_px: size_px.round().max(1.0) as u32,
        };
        if let Some(entry) = self.entries.get(&key) {
            return entry.clone();
        }
        let handle = rasterize_to_texture(ctx, rasterizer, fallback, cluster, size_px);
        self.entries.insert(key, handle.clone());
        handle
    }
}

fn rasterize_to_texture(
    ctx: &Context,
    rasterizer: &dyn GlyphRasterizer,
    fallback: &FallbackChain,
    cluster: &str,
    size_px: f32,
) -> Option<TextureHandle> {
    let font_id = fallback.resolve_for_cluster(rasterizer, cluster)?;

    // Color emoji (RGBA, bitmap-strike sourced) benefit from the
    // supersample + Lanczos3 path; alpha glyphs (outline fonts) scale
    // cleanly at the target size already, so rasterize those directly.
    // When the requested size is already >= the HQ source, swash's
    // scaling is gentle enough — skip the extra downscale.
    let supersample = size_px < HQ_SOURCE_PX;
    let raster_at = if supersample { HQ_SOURCE_PX } else { size_px };

    let shaped = rasterizer.shape(cluster, font_id, raster_at);
    let glyph = shaped.into_iter().next()?;
    let bitmap = rasterizer.raster(glyph.font, glyph.glyph_id, glyph.size_px)?;
    if bitmap.width == 0 || bitmap.height == 0 {
        return None;
    }
    let src_rgba = match bitmap.format {
        AtlasFormat::Rgba => bitmap.pixels,
        AtlasFormat::Alpha => alpha_to_rgba(&bitmap.pixels),
    };

    // Downscale RGBA color glyphs to the target size with Lanczos3.
    // Alpha glyphs were rasterized at the target size already.
    let (w, h, rgba) = if supersample && bitmap.format == AtlasFormat::Rgba {
        let scale = size_px / raster_at;
        let dst_w = ((bitmap.width as f32 * scale).round() as u32).max(1);
        let dst_h = ((bitmap.height as f32 * scale).round() as u32).max(1);
        let resized = lanczos3_downscale_rgba(bitmap.width, bitmap.height, &src_rgba, dst_w, dst_h);
        (dst_w, dst_h, resized)
    } else {
        (bitmap.width, bitmap.height, src_rgba)
    };

    let image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
    let name = format!(
        "emoji:{}@{}",
        cluster.chars().next().map(|c| c as u32).unwrap_or(0),
        size_px.round().max(1.0) as u32
    );
    Some(ctx.load_texture(name, image, TextureOptions::LINEAR))
}

/// Promote a single-channel alpha bitmap (returned by swash for fonts
/// without color tables) to opaque-white RGBA so it can ride the same
/// `ColorImage` path as color-bitmap output.
fn alpha_to_rgba(alpha: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(alpha.len() * 4);
    for &a in alpha {
        out.push(255);
        out.push(255);
        out.push(255);
        out.push(a);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_text_has_no_emoji_segments() {
        let segs = split_segments("hello world");
        assert_eq!(segs, vec![TextSegment::Text("hello world")]);
    }

    #[test]
    fn empty_input_yields_no_segments() {
        assert!(split_segments("").is_empty());
    }

    #[test]
    fn pure_emoji_input_groups_into_single_segment() {
        let segs = split_segments("\u{1F600}\u{1F601}");
        assert_eq!(segs, vec![TextSegment::Emoji("\u{1F600}\u{1F601}")]);
    }

    #[test]
    fn mixed_run_splits_at_emoji_boundaries() {
        // "PWD: 📁 /home" => text, emoji, text.
        let input = "PWD: \u{1F4C1} /home";
        let segs = split_segments(input);
        assert_eq!(
            segs,
            vec![
                TextSegment::Text("PWD: "),
                TextSegment::Emoji("\u{1F4C1}"),
                TextSegment::Text(" /home"),
            ]
        );
    }

    #[test]
    fn dingbat_range_is_treated_as_emoji() {
        // U+2705 ✅ is dual-presentation but lives in the pictographic
        // range — the prefilter routes it to swash.
        let segs = split_segments("ok \u{2705}");
        assert_eq!(
            segs,
            vec![TextSegment::Text("ok "), TextSegment::Emoji("\u{2705}")]
        );
    }

    #[test]
    fn variation_selector_groups_with_base() {
        // U+26A0 U+FE0F (warning + VS-16) is one grapheme cluster and
        // any char carrying VS-16 trips the pictographic prefilter.
        let segs = split_segments("\u{26A0}\u{FE0F}!");
        assert_eq!(
            segs,
            vec![
                TextSegment::Emoji("\u{26A0}\u{FE0F}"),
                TextSegment::Text("!"),
            ]
        );
    }

    #[test]
    fn cjk_is_not_emoji() {
        // Japanese characters live in 0x3000..0x9FFF, outside the
        // pictographic ranges — they must stay on the text path.
        let segs = split_segments("\u{3042}\u{3044}");
        assert_eq!(segs, vec![TextSegment::Text("\u{3042}\u{3044}")]);
    }

    #[test]
    fn box_drawing_is_not_emoji() {
        // U+2514 (└) sits between the pictographic ranges and must
        // not be routed to the emoji font.
        let segs = split_segments("\u{2514}\u{2500}\u{2518}");
        assert_eq!(segs, vec![TextSegment::Text("\u{2514}\u{2500}\u{2518}")]);
    }
}
