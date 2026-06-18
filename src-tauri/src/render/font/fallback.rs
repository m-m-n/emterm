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
    /// Regular-face → Bold-face substitutions. After a cluster resolves
    /// to a font, callers rendering a bold cell swap in the registered
    /// bold variant (when one exists) via [`FallbackChain::bold_variant`].
    /// Fonts without an entry render bold cells with their regular face.
    bold_variants: HashMap<FontId, FontId>,
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
            bold_variants: HashMap::new(),
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

    /// Register `bold` as the bold-face substitute for `regular`. No memo
    /// invalidation needed: resolution always happens on regular faces
    /// and the substitution is applied after the fact.
    pub fn set_bold_variant(&mut self, regular: FontId, bold: FontId) {
        self.bold_variants.insert(regular, bold);
    }

    /// The bold-face substitute for `id`, when one was registered.
    pub fn bold_variant(&self, id: FontId) -> Option<FontId> {
        self.bold_variants.get(&id).copied()
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
    ///
    /// When `cp` lies in a pictographic range ([`is_pictographic`]) and
    /// [`set_emoji`] has marked an emoji font, that font is checked
    /// **first** so codepoints like ✅ U+2705 or 🟢 U+1F7E2 prefer the
    /// color-emoji glyph even when a text font earlier in the chain
    /// (Noto Sans JP carries BW glyphs for many of these) reports
    /// coverage. The range is intentionally narrow so ASCII, Latin,
    /// CJK ideographs, and box-drawing characters keep their text-font
    /// resolution.
    pub fn resolve(&self, rasterizer: &dyn GlyphRasterizer, cp: u32) -> Option<FontId> {
        if let Some(cached) = self.memo.lock().get(&(self.base, cp)).copied() {
            return cached;
        }
        let mut answer: Option<FontId> = None;
        if is_pictographic(cp) {
            if let Some(emoji) = self.emoji {
                if rasterizer.has_codepoint(emoji, cp) {
                    answer = Some(emoji);
                }
            }
        }
        if answer.is_none() {
            for &font in &self.chain {
                if rasterizer.has_codepoint(font, cp) {
                    answer = Some(font);
                    break;
                }
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

/// Conservative pictographic-range test used by [`FallbackChain::resolve`]
/// to decide whether the marked emoji font should be consulted before
/// the regular chain. The range is intentionally narrow:
///
/// - Watch / hourglass (U+231A..=U+231B), media + clock controls
///   (U+23E9..=U+23F3, e.g. ⏰ ⏳) and pause/stop/record
///   (U+23F8..=U+23FA) — these live in the Miscellaneous Technical
///   block but are color-emoji in Noto; the surrounding technical
///   symbols are not, so the emoji-font `has_codepoint` check still
///   routes non-emoji to the text chain.
/// - Misc Symbols + Dingbats (U+2600..=U+27BF) — covers ✅ ✓ ☂ ☀ etc.
///   that exist in both BW text fonts and color emoji fonts; we want
///   the color variant when the cluster reaches the bare codepoint.
/// - All BMP-plus codepoints at or above U+1F000 — the SMP "Emoticons"
///   / "Pictographs" / "Transport" / "Symbols Ext-A/B" / "Flags" /
///   "Regional Indicators" live here. ASCII (U+0000..=U+007F),
///   Latin extensions, CJK ideographs (U+3000..=U+9FFF), Hangul,
///   and box-drawing (U+2500..=U+25FF) are *outside* this set so
///   they keep their regular text-font resolution.
///
/// Tests pin the boundary values so future tweaks notice when ASCII /
/// CJK / box-drawing accidentally start preferring the emoji font.
///
/// The BMP spans below mirror the singletons + ranges of the Unicode
/// `Emoji` property that live below U+1F000 (the SMP blocks are caught
/// wholesale by the `>= 0x1F000` tail). The list is deliberately
/// generous: a codepoint only *prefers* the emoji font here, and the
/// preference is a no-op unless that font actually covers the glyph
/// (`resolve` falls through to the text chain otherwise). Block-drawing
/// (U+2580..=U+259F) is intentionally excluded so progress-bar glyphs
/// like `░▒▓█` keep their crisp text/`block_drawing` rendering.
///
/// `crate::ui::emoji_cache::cluster_is_emoji` calls into this function
/// so the two stay in sync; it adds the cluster-level modifiers (VS-16,
/// keycap) that only make sense across a whole grapheme.
pub(crate) fn is_pictographic(cp: u32) -> bool {
    matches!(cp,
        0x00A9 | 0x00AE                  // © ®
        | 0x203C | 0x2049                // ‼ ⁉
        | 0x2122 | 0x2139                // ™ ℹ
        | 0x2194..=0x2199                // ↔ ↕ ↖ ↗ ↘ ↙
        | 0x21A9..=0x21AA                // ↩ ↪
        | 0x231A..=0x231B                // ⌚ ⌛
        | 0x2328                         // ⌨
        | 0x23CF                         // ⏏
        | 0x23E9..=0x23F3                // ⏩‥⏳ media + clocks
        | 0x23F8..=0x23FA                // ⏸ ⏹ ⏺
        | 0x24C2                         // Ⓜ
        | 0x25AA..=0x25AB                // ▪ ▫
        | 0x25B6 | 0x25C0                // ▶ ◀
        | 0x25FB..=0x25FE                // ◻ ◼ ◽ ◾
        | 0x2600..=0x27BF                // misc symbols + dingbats
        | 0x2934..=0x2935                // ⤴ ⤵
        | 0x2B05..=0x2B07                // ⬅ ⬆ ⬇
        | 0x2B1B..=0x2B1C                // ⬛ ⬜
        | 0x2B50 | 0x2B55                // ⭐ ⭕
        | 0x3030 | 0x303D                // 〰 〽
        | 0x3297 | 0x3299                // ㊗ ㊙
    ) || cp >= 0x1F000
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

    /// Bold variant registration: registered ids round-trip, unknown ids
    /// return None so callers keep the regular face.
    #[test]
    fn bold_variant_roundtrip_and_miss() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2)]);
        chain.set_bold_variant(FontId(1), FontId(9));
        assert_eq!(chain.bold_variant(FontId(1)), Some(FontId(9)));
        assert_eq!(chain.bold_variant(FontId(2)), None);
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

    /// Regression: when a CJK / text font earlier in the chain happens
    /// to carry a BW glyph for a pictographic codepoint (✅ U+2705,
    /// 🟢 U+1F7E2), the emoji font marked via `set_emoji` must still
    /// win so the user sees color emoji.
    #[test]
    fn resolve_prefers_emoji_in_pictographic_range() {
        // FontId(1) = base ASCII, FontId(2) = "CJK" that also carries
        // BW glyphs for 0x2705 and 0x1F7E2, FontId(3) = emoji marked
        // with `set_emoji` and covering the same pictographs.
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41));
        covers.insert((FontId(2), 0x3042));
        covers.insert((FontId(2), 0x2705)); // BW ✓ in the CJK font
        covers.insert((FontId(2), 0x1F7E2)); // BW 🟢 in the CJK font
        covers.insert((FontId(3), 0x2705));
        covers.insert((FontId(3), 0x1F7E2));
        covers.insert((FontId(3), 0x1F600));
        let raster = TableRasterizer { covers };

        // Pictographs prefer the emoji font even though the CJK font
        // would have matched first by chain order.
        assert_eq!(chain.resolve(&raster, 0x2705), Some(FontId(3)));
        assert_eq!(chain.resolve(&raster, 0x1F7E2), Some(FontId(3)));

        // ASCII / CJK keep their previous fonts because the emoji
        // font does not cover them.
        assert_eq!(chain.resolve(&raster, 0x41), Some(FontId(1)));
        assert_eq!(chain.resolve(&raster, 0x3042), Some(FontId(2)));
    }

    /// Regression: bundled `NotoColorEmoji.ttf` happens to carry glyphs
    /// for some non-pictographic codepoints (e.g. ASCII digits used in
    /// keycap clusters, or stray symbols in the Latin block). Before
    /// the narrow [`is_pictographic`] check, the emoji-first preference
    /// would shadow the regular text fonts for *every* codepoint the
    /// emoji font happened to cover — leaving ordinary ASCII /
    /// box-drawing letters rendered in the emoji font.
    #[test]
    fn resolve_does_not_prefer_emoji_outside_pictographic_range() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(3));
        let mut covers = HashSet::new();
        // ASCII 'A' covered by both base AND emoji — base must win.
        covers.insert((FontId(1), 0x41));
        covers.insert((FontId(3), 0x41));
        // Box drawing └ (U+2514) covered by base AND emoji — base wins.
        covers.insert((FontId(1), 0x2514));
        covers.insert((FontId(3), 0x2514));
        // CJK U+3042 covered by CJK AND emoji — CJK wins.
        covers.insert((FontId(2), 0x3042));
        covers.insert((FontId(3), 0x3042));
        let raster = TableRasterizer { covers };

        assert_eq!(chain.resolve(&raster, 0x41), Some(FontId(1)));
        assert_eq!(chain.resolve(&raster, 0x2514), Some(FontId(1)));
        assert_eq!(chain.resolve(&raster, 0x3042), Some(FontId(2)));
    }

    /// Regression: when a pictographic codepoint (e.g. `❯` U+276F shown
    /// by starship) is NOT covered by the emoji font, the chain walk
    /// must consult the `FontRole::Secondary` slot and resolve to a
    /// symbol font in it. `app.rs` slots `Noto Sans Symbols2` etc.
    /// here per SPEC FR8's `font_family_fallback...` position, and a
    /// regression in `resolve` that stops walking after the emoji
    /// short-circuit (or that reorders the chain so Secondary lands
    /// after emoji again) would silently drop `❯` and friends back
    /// to a tofu glyph.
    #[test]
    fn resolve_pictographic_falls_to_secondary_when_emoji_misses() {
        // Chain shape mirrors the production build:
        //   FontId(1) = base (Inconsolata-equivalent, ASCII only)
        //   FontId(2) = secondary (Symbols2-equivalent, covers U+276F)
        //   FontId(3) = emoji (Noto Color Emoji-equivalent, covers
        //               U+1F600 but NOT U+276F — current real-world
        //               coverage of the bundled emoji font).
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41)); // base: 'A'
        covers.insert((FontId(2), 0x276F)); // secondary: ❯
        covers.insert((FontId(3), 0x1F600)); // emoji: 😀
        let raster = TableRasterizer { covers };
        // U+276F is inside `is_pictographic`'s 0x2600..=0x27BF range,
        // so `resolve` checks the emoji font FIRST. The emoji font
        // misses, the chain walks on, and Secondary catches it.
        assert_eq!(chain.resolve(&raster, 0x276F), Some(FontId(2)));
        // U+1F600 stays on the emoji font (sanity check the
        // short-circuit still wins when the emoji font covers a
        // pictographic codepoint).
        assert_eq!(chain.resolve(&raster, 0x1F600), Some(FontId(3)));
    }

    /// Pin the boundary values of [`is_pictographic`] so a future tweak
    /// that accidentally widens the range (and re-introduces the
    /// "ASCII gets emoji glyph" regression) trips the test.
    #[test]
    fn is_pictographic_boundary_values() {
        // Outside the range
        assert!(!is_pictographic(0x0041)); // 'A'
        assert!(!is_pictographic(0x3042)); // あ
        assert!(!is_pictographic(0x2500)); // box drawing ─
        assert!(!is_pictographic(0x25FF)); // last box drawing
        // Block-drawing must stay text-rendered: the status-bar
        // progress bars are built from these (`░▒▓█`), and a color
        // emoji glyph would wreck the bar. Pin the whole block.
        assert!(!is_pictographic(0x2580)); // ▀ upper half block
        assert!(!is_pictographic(0x2588)); // █ full block
        assert!(!is_pictographic(0x2591)); // ░ light shade (the bar's empty cell)
        assert!(!is_pictographic(0x2593)); // ▓ dark shade
        assert!(!is_pictographic(0x259F)); // last block-drawing
        assert!(!is_pictographic(0x2300)); // ⌀ diameter sign (technical, BW)
        assert!(!is_pictographic(0x2319)); // just below the watch span
        assert!(!is_pictographic(0x23E8)); // just below the clock-control span
        assert!(!is_pictographic(0x23F4)); // ⏴ BW arrow, just above ⏳
        assert!(!is_pictographic(0x1EFFF)); // just below SMP emoji block
        // Inside the range
        assert!(is_pictographic(0x231A)); // ⌚ watch
        assert!(is_pictographic(0x231B)); // ⌛ hourglass
        assert!(is_pictographic(0x23E9)); // ⏩ fast-forward
        assert!(is_pictographic(0x23F0)); // ⏰ alarm clock
        assert!(is_pictographic(0x23F3)); // ⏳ hourglass-flowing (the status-bar case)
        assert!(is_pictographic(0x23F8)); // ⏸ pause
        assert!(is_pictographic(0x23FA)); // ⏺ record
        assert!(is_pictographic(0x2600)); // ☀ first dingbat-ish
        assert!(is_pictographic(0x2705)); // ✅
        assert!(is_pictographic(0x27BF)); // ➿ last dingbat
        assert!(is_pictographic(0x1F000)); // 🀀
        assert!(is_pictographic(0x1F600)); // 😀
        assert!(is_pictographic(0x1F7E2)); // 🟢
        assert!(is_pictographic(0x1FAFF)); // upper pictographs ext-A
    }
}
