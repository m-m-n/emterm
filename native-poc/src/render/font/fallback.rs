//! Fallback chain: walk `[base, user-fallbacks..., cjk, emoji, secondary]`
//! and memoize `(BaseFontId, Codepoint) → FontId`.
//!
//! Phase 3 of font-swash-migration (FR8). The chain is constructed once at
//! startup from a `Resolver`; runtime mutation is not supported.

use std::collections::HashMap;

use parking_lot::Mutex;

use super::resolver::Resolver;
use super::traits::{FontId, GlyphRasterizer};

/// Read-only ordered list of `(role-tagged) FontId`s that the chain walks
/// for every codepoint, plus a memo of decisions.
pub struct FallbackChain {
    base: FontId,
    chain: Vec<FontId>,
    /// First emoji-role font discovered when the chain was constructed
    /// via `from_resolver`. Used by `resolve_for_cluster` to short-circuit
    /// to color emoji when the cluster explicitly requests it (VS-16).
    emoji: Option<FontId>,
    memo: Mutex<HashMap<(FontId, u32), Option<FontId>>>,
}

impl FallbackChain {
    /// Build a chain rooted at `base`, including the supplied additional
    /// font ids in order. Duplicates are removed (preserving first
    /// position).
    pub fn new(base: FontId, additional: impl IntoIterator<Item = FontId>) -> Self {
        let mut chain = vec![base];
        for id in additional {
            if !chain.contains(&id) {
                chain.push(id);
            }
        }
        Self {
            base,
            chain,
            emoji: None,
            memo: Mutex::new(HashMap::new()),
        }
    }

    /// Mark a font in the chain as the preferred emoji font. Used by
    /// `resolve_for_cluster` to short-circuit VS-16-bearing clusters to
    /// color emoji instead of letting the per-codepoint walk pick the
    /// BW base font for dual-presentation codepoints (e.g. U+26A0).
    pub fn set_emoji(&mut self, id: FontId) {
        self.emoji = Some(id);
        // Memo entries computed before this hint don't know about the
        // VS-16 short-circuit. Clear so subsequent resolves re-decide.
        self.memo.lock().clear();
    }

    /// Build the canonical chain from a resolver: base → CJK → emoji.
    /// `base` is the caller-selected primary font id (usually the result
    /// of `Resolver::by_family(Theme::font_family)`).
    pub fn from_resolver(resolver: &Resolver, base: FontId) -> Self {
        use super::resolver::FontRole;
        let mut extras = Vec::new();
        for f in resolver.by_role(FontRole::User) {
            extras.push(f.id);
        }
        for f in resolver.by_role(FontRole::Cjk) {
            extras.push(f.id);
        }
        let mut emoji: Option<FontId> = None;
        for f in resolver.by_role(FontRole::Emoji) {
            if emoji.is_none() {
                emoji = Some(f.id);
            }
            extras.push(f.id);
        }
        for f in resolver.by_role(FontRole::Secondary) {
            extras.push(f.id);
        }
        let mut chain = Self::new(base, extras);
        chain.emoji = emoji;
        chain
    }

    /// Resolve a grapheme cluster to a font, preferring the emoji font
    /// when the cluster explicitly requests emoji presentation via
    /// VS-16 (U+FE0F). Falls back to the per-codepoint walk for the
    /// cluster's first codepoint otherwise.
    ///
    /// Without this distinction, dual-presentation codepoints like
    /// U+26A0 (warning sign) resolve to the BW base font even when the
    /// cluster carries an explicit VS-16, because the base font reports
    /// coverage for the bare codepoint.
    pub fn resolve_for_cluster(
        &self,
        rasterizer: &dyn GlyphRasterizer,
        cluster: &str,
    ) -> Option<FontId> {
        let mut first_cp: Option<u32> = None;
        let mut has_vs16 = false;
        for ch in cluster.chars() {
            let cp = ch as u32;
            if first_cp.is_none() {
                first_cp = Some(cp);
            }
            if cp == 0xFE0F {
                has_vs16 = true;
            }
        }
        let first = first_cp?;
        if has_vs16 {
            if let Some(emoji) = self.emoji {
                if rasterizer.has_codepoint(emoji, first) {
                    return Some(emoji);
                }
            }
        }
        self.resolve(rasterizer, first)
    }

    pub fn base(&self) -> FontId {
        self.base
    }

    pub fn chain(&self) -> &[FontId] {
        &self.chain
    }

    /// Resolve a codepoint to a font in the chain. Walks the chain in
    /// order, asking each rasterizer "do you cover this codepoint?".
    /// Memoizes the answer (including `None` for "all miss") per
    /// (base, codepoint).
    pub fn resolve(&self, rasterizer: &dyn GlyphRasterizer, cp: u32) -> Option<FontId> {
        if let Some(cached) = self.memo.lock().get(&(self.base, cp)).copied() {
            return cached;
        }
        let mut answer: Option<FontId> = None;
        for &font in &self.chain {
            if rasterizer.has_codepoint(font, cp) {
                answer = Some(font);
                break;
            }
        }
        self.memo.lock().insert((self.base, cp), answer);
        answer
    }

    /// Manually seed a memo entry (used by tests + warm-up paths).
    pub fn seed(&self, cp: u32, font: Option<FontId>) {
        self.memo.lock().insert((self.base, cp), font);
    }

    pub fn memo_len(&self) -> usize {
        self.memo.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::super::traits::{GlyphBitmap, ShapedGlyph};
    use super::*;
    use std::collections::HashSet;

    /// Test rasterizer that reports coverage from a static `(font, cp)`
    /// table. `raster` is never called by the fallback path itself, only
    /// `has_codepoint` is queried.
    struct TableRasterizer {
        covers: HashSet<(FontId, u32)>,
    }

    impl GlyphRasterizer for TableRasterizer {
        fn shape(&self, _: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
            vec![ShapedGlyph {
                font,
                glyph_id: 1,
                size_px,
            }]
        }
        fn raster(&self, _: FontId, _: u32, _: f32) -> Option<GlyphBitmap> {
            None
        }
        fn has_codepoint(&self, font: FontId, cp: u32) -> bool {
            self.covers.contains(&(font, cp))
        }
    }

    fn build_chain() -> (FallbackChain, TableRasterizer) {
        // FontId(1) = base (ASCII only), FontId(2) = CJK (only U+3042),
        // FontId(3) = emoji (only U+1F600).
        let chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41)); // 'A'
        covers.insert((FontId(2), 0x3042));
        covers.insert((FontId(3), 0x1F600));
        let raster = TableRasterizer { covers };
        (chain, raster)
    }

    /// TS-font-4: ASCII → base; U+3042 → CJK.
    #[test]
    fn resolve_ascii_returns_base() {
        let (chain, raster) = build_chain();
        assert_eq!(chain.resolve(&raster, 0x41), Some(FontId(1)));
    }

    #[test]
    fn resolve_cjk_returns_cjk_fallback() {
        let (chain, raster) = build_chain();
        assert_eq!(chain.resolve(&raster, 0x3042), Some(FontId(2)));
    }

    /// TS-font-5: U+1F600 → emoji.
    #[test]
    fn resolve_emoji_returns_emoji_fallback() {
        let (chain, raster) = build_chain();
        assert_eq!(chain.resolve(&raster, 0x1F600), Some(FontId(3)));
    }

    #[test]
    fn resolve_uncovered_returns_none_and_memoizes() {
        let (chain, raster) = build_chain();
        assert_eq!(chain.resolve(&raster, 0xFFFD), None);
        assert!(chain.memo_len() >= 1);
        // Second call must hit the memo (we cannot directly count
        // has_codepoint calls here, but the entry is present).
        assert_eq!(chain.resolve(&raster, 0xFFFD), None);
    }

    #[test]
    fn duplicate_chain_entries_are_dropped() {
        let chain = FallbackChain::new(FontId(1), [FontId(1), FontId(2), FontId(2)]);
        assert_eq!(chain.chain(), &[FontId(1), FontId(2)]);
    }
}
