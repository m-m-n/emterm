//! Glyph cache: (FontId, GlyphId, size bucket, subpixel bucket) → AtlasRegion.
//!
//! Phase 2 of font-swash-migration. The cache memoizes rasterizer output
//! and the resulting atlas region so per-frame draws never re-rasterize
//! the same glyph at the same size + subpixel offset. Cache observability
//! is exposed for NFR4 (future diagnostics UI).
//!
//! `EMTERM_FONT_PERF=1` (Phase 5) is honored here: cache misses time the
//! rasterize step and log per-glyph durations at `warn` level so they
//! survive release-build log filtering.

use std::collections::HashMap;
use std::time::Instant;

use super::atlas::{Atlas, AtlasRegion};
use super::traits::{FontId, GlyphRasterizer};

/// Cache key: identifies a unique glyph rasterization request.
///
/// `size_bucket` rounds the requested size to integer pixels (a sufficient
/// resolution for terminal cells; subpixel sizes are not used today).
/// `subpixel_bucket` is the horizontal fractional pen offset bucketed into
/// 64ths of a pixel — kept for future LCD-style positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font: FontId,
    pub glyph_id: u32,
    pub size_bucket: u32,
    pub subpixel_bucket: u8,
}

impl GlyphKey {
    pub fn new(font: FontId, glyph_id: u32, size_px: f32, subpixel: f32) -> Self {
        Self {
            font,
            glyph_id,
            size_bucket: size_px.round().max(0.0) as u32,
            subpixel_bucket: ((subpixel.fract().abs() * 64.0).round() as u32 % 64) as u8,
        }
    }
}

/// Sentinel marker for "rasterizer returned `None`": the cache stores this
/// so subsequent identical lookups skip the (failed) rasterize call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Empty,             // valid zero-size rasterize result (whitespace)
    Some(AtlasRegion), // rasterizer succeeded, region uploaded
    Missing,           // rasterizer returned None — cluster must fall through chain
}

/// Cache statistics (NFR4 observability).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub missing: u64,
}

impl CacheStats {
    pub fn lookups(&self) -> u64 {
        self.hits + self.misses + self.missing
    }
}

/// Glyph cache + atlas owner.
///
/// The cache holds the canonical mapping from a `GlyphKey` to the
/// resulting `AtlasRegion` (or sentinel). Atlas storage is owned here so
/// callers cannot lose the binding by dropping the atlas separately.
#[derive(Debug)]
pub struct GlyphCache {
    atlas: Atlas,
    slots: HashMap<GlyphKey, Slot>,
    stats: CacheStats,
    perf_log: bool,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphCache {
    pub fn new() -> Self {
        let perf_log = std::env::var("EMTERM_FONT_PERF")
            .map(|v| v != "0")
            .unwrap_or(false);
        Self {
            atlas: Atlas::new(),
            slots: HashMap::new(),
            stats: CacheStats::default(),
            perf_log,
        }
    }

    /// Look up a glyph; on miss, rasterize via `rasterizer` and upload to
    /// the atlas.
    ///
    /// Return values:
    /// - `Some(region)` with `region.is_empty() == false` — hit or fresh upload.
    /// - `Some(region)` with `region.is_empty() == true` — sentinel for
    ///   zero-size bitmap (whitespace).
    /// - `None` — rasterizer returned `None`; caller must walk the
    ///   fallback chain.
    pub fn get_or_rasterize(
        &mut self,
        rasterizer: &dyn GlyphRasterizer,
        key: GlyphKey,
    ) -> Option<AtlasRegion> {
        if let Some(slot) = self.slots.get(&key).copied() {
            return match slot {
                Slot::Some(r) => {
                    self.stats.hits += 1;
                    Some(r)
                }
                Slot::Empty => {
                    self.stats.hits += 1;
                    Some(AtlasRegion {
                        format: super::traits::AtlasFormat::Alpha,
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    })
                }
                Slot::Missing => {
                    self.stats.missing += 1;
                    None
                }
            };
        }
        let started = if self.perf_log {
            Some(Instant::now())
        } else {
            None
        };
        let raster_result = rasterizer.raster(key.font, key.glyph_id, key.size_bucket as f32);
        if let Some(t0) = started {
            let dt = t0.elapsed();
            log::warn!(
                "[EMTERM_FONT_PERF] glyph rasterize: font={} glyph={} size={} elapsed_us={}",
                key.font.0,
                key.glyph_id,
                key.size_bucket,
                dt.as_micros(),
            );
        }
        match raster_result {
            None => {
                self.slots.insert(key, Slot::Missing);
                self.stats.missing += 1;
                None
            }
            Some(bitmap) if bitmap.is_empty() => {
                self.slots.insert(key, Slot::Empty);
                self.stats.misses += 1;
                Some(AtlasRegion {
                    format: bitmap.format,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                })
            }
            Some(bitmap) => {
                let region = self.atlas.upload(&bitmap);
                self.slots.insert(key, Slot::Some(region));
                self.stats.misses += 1;
                Some(region)
            }
        }
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Total number of cached slots (hits + uploads + missing sentinels).
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }
}

#[cfg(test)]
mod tests {
    use super::super::traits::{AtlasFormat, GlyphBitmap, ShapedGlyph};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts rasterize calls so we can prove that the second
    /// `get_or_rasterize` hits the cache rather than calling the
    /// rasterizer again.
    struct CountingRasterizer {
        calls: AtomicUsize,
        ret: Option<GlyphBitmap>,
    }

    impl GlyphRasterizer for CountingRasterizer {
        fn shape(&self, _: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
            vec![ShapedGlyph {
                font,
                glyph_id: 42,
                size_px,
            }]
        }
        fn raster(&self, _: FontId, _: u32, _: f32) -> Option<GlyphBitmap> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.ret.clone()
        }
    }

    fn alpha_bitmap(w: u32, h: u32) -> GlyphBitmap {
        GlyphBitmap {
            format: AtlasFormat::Alpha,
            width: w,
            height: h,
            bearing: (0, 0),
            advance: w as f32,
            pixels: vec![0xFF; (w * h) as usize],
        }
    }

    /// TS-font-3: `get_or_rasterize` returns the same region on second call.
    #[test]
    fn cache_hit_returns_same_region_and_skips_raster() {
        let mut cache = GlyphCache::new();
        let r = CountingRasterizer {
            calls: AtomicUsize::new(0),
            ret: Some(alpha_bitmap(8, 16)),
        };
        let key = GlyphKey::new(FontId(1), 5, 13.0, 0.0);
        let first = cache.get_or_rasterize(&r, key).unwrap();
        let second = cache.get_or_rasterize(&r, key).unwrap();
        assert_eq!(first, second);
        assert_eq!(r.calls.load(Ordering::SeqCst), 1, "second call must hit");
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn missing_returns_none_and_caches_sentinel() {
        let mut cache = GlyphCache::new();
        let r = CountingRasterizer {
            calls: AtomicUsize::new(0),
            ret: None,
        };
        let key = GlyphKey::new(FontId(1), 5, 13.0, 0.0);
        assert!(cache.get_or_rasterize(&r, key).is_none());
        assert!(cache.get_or_rasterize(&r, key).is_none());
        // The sentinel must short-circuit the second call.
        assert_eq!(r.calls.load(Ordering::SeqCst), 1);
        assert_eq!(cache.stats().missing, 2);
    }

    #[test]
    fn empty_bitmap_caches_zero_size_region() {
        let mut cache = GlyphCache::new();
        let r = CountingRasterizer {
            calls: AtomicUsize::new(0),
            ret: Some(alpha_bitmap(0, 0)),
        };
        let key = GlyphKey::new(FontId(1), 32, 13.0, 0.0);
        let region = cache.get_or_rasterize(&r, key).unwrap();
        assert!(region.is_empty());
        // Second call hits the Empty sentinel without re-rastering.
        let _ = cache.get_or_rasterize(&r, key).unwrap();
        assert_eq!(r.calls.load(Ordering::SeqCst), 1);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn observable_accessors_increment_on_use() {
        let mut cache = GlyphCache::new();
        let r = CountingRasterizer {
            calls: AtomicUsize::new(0),
            ret: Some(alpha_bitmap(4, 4)),
        };
        assert_eq!(cache.len(), 0);
        let key = GlyphKey::new(FontId(2), 7, 13.0, 0.0);
        let _ = cache.get_or_rasterize(&r, key);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.stats().lookups(), 1);
    }

    #[test]
    fn key_bucketing_distinct_for_distinct_sizes() {
        let a = GlyphKey::new(FontId(1), 1, 13.0, 0.0);
        let b = GlyphKey::new(FontId(1), 1, 14.0, 0.0);
        assert_ne!(a, b);
    }
}
