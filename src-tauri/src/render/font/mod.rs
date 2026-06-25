//! Font module aggregator.
//!
//! Phase 2 of font-swash-migration lands the boundary types (`traits.rs`),
//! the glyph cache + atlas (`cache.rs`, `atlas.rs`), and the ab_glyph
//! adapter (`ab_glyph_adapter.rs`). Phase 3 layers the swash adapter,
//! fontdb-backed resolver, and the fallback chain on top.
//!
//! The renderer never imports the engine adapters directly; it always
//! talks to the cache, which owns a boxed `dyn GlyphRasterizer`. That
//! keeps the cache + atlas independent of the active engine and makes
//! the `Settings::font_engine` flag a one-line constructor swap.

pub mod ab_glyph_adapter;
pub mod atlas;
pub mod cache;
pub mod fallback;
pub mod presentation;
pub mod resolver;
pub mod swash_adapter;
pub mod traits;
pub mod user_dir;

pub use atlas::{Atlas, AtlasRegion};
pub use cache::{CacheStats, GlyphCache, GlyphKey};
pub use traits::{AtlasFormat, FontId, GlyphBitmap, GlyphRasterizer, ShapedGlyph};
