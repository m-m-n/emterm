//! Fallback chain: walk `[base, user-fallbacks..., cjk, emoji, secondary]`
//! and memoize `(BaseFontId, Codepoint) → FontId`.
//!
//! Phase 3 of font-swash-migration (FR8). The chain is constructed once at
//! startup from a `Resolver`; runtime mutation is not supported.

use std::collections::HashMap;

use parking_lot::Mutex;

use super::presentation::{EmojiPresentation, VS15, VS16, presentation_for};
use super::resolver::Resolver;
use super::traits::{FontId, GlyphRasterizer};

/// Read-only ordered list of `(role-tagged) FontId`s that the chain walks
/// for every codepoint, plus a memo of decisions.
pub struct FallbackChain {
    base: FontId,
    chain: Vec<FontId>,
    /// First color-emoji font discovered when the chain was constructed
    /// via `from_resolver`. Used by `resolve_for_cluster` to short-circuit
    /// to color emoji when the cluster's presentation is `Color`.
    emoji: Option<FontId>,
    /// First monochrome-emoji font discovered when the chain was
    /// constructed via `from_resolver`. Used by `resolve_for_cluster`
    /// for text-default emoji (e.g. U+23F5 `⏵`) and VS15-attached
    /// clusters so they pick up the outline glyph instead of the BW
    /// base font (which has no glyph for them).
    mono_emoji: Option<FontId>,
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
            mono_emoji: None,
            bold_variants: HashMap::new(),
            memo: Mutex::new(HashMap::new()),
        }
    }

    /// Mark a font in the chain as the preferred color-emoji font. Used
    /// by `resolve_for_cluster` to short-circuit VS-16-bearing clusters
    /// to color emoji instead of letting the per-codepoint walk pick
    /// the BW base font for dual-presentation codepoints (e.g. U+26A0).
    pub fn set_emoji(&mut self, id: FontId) {
        self.emoji = Some(id);
        // Memo entries computed before this hint don't know about the
        // VS-16 short-circuit. Clear so subsequent resolves re-decide.
        self.memo.lock().clear();
    }

    /// Mark a font in the chain as the preferred monochrome-emoji font.
    /// Used by `resolve_for_cluster` to route text-default emoji
    /// (e.g. U+23F5 `⏵`) and VS15-attached clusters to the outline
    /// face instead of falling through to the BW base monospace font.
    pub fn set_mono_emoji(&mut self, id: FontId) {
        self.mono_emoji = Some(id);
        // Same reasoning as `set_emoji`: prior memo entries pre-date
        // the dispatch hint and must be re-decided.
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
        for f in resolver.by_role(FontRole::ColorEmoji) {
            if emoji.is_none() {
                emoji = Some(f.id);
            }
            extras.push(f.id);
        }
        // Monochrome emoji sits between color emoji and the secondary
        // fallback so text-default emoji code points (e.g. U+23F5 `⏵`)
        // pick up the outline glyph instead of falling through to the
        // base monospace font, which has no glyph for them.
        let mut mono_emoji: Option<FontId> = None;
        for f in resolver.by_role(FontRole::MonochromeEmoji) {
            if mono_emoji.is_none() {
                mono_emoji = Some(f.id);
            }
            extras.push(f.id);
        }
        for f in resolver.by_role(FontRole::Secondary) {
            extras.push(f.id);
        }
        let mut chain = Self::new(base, extras);
        chain.emoji = emoji;
        chain.mono_emoji = mono_emoji;
        chain
    }

    /// Resolve a grapheme cluster to a font using
    /// [`presentation_for`][super::presentation::presentation_for] to
    /// decide whether the cluster wants the color emoji face, the
    /// monochrome emoji face, or the regular text chain.
    ///
    /// Dispatch (FR5):
    /// - VS-16 (U+FE0F) anywhere in the cluster → color emoji first.
    /// - VS-15 (U+FE0E) anywhere in the cluster → monochrome emoji first.
    /// - Combining Enclosing Keycap (U+20E3) in the cluster → color emoji
    ///   first regardless of the base character (the keycap base
    ///   characters `0..=9`, `*`, `#` resolve to `NotEmoji` for bare
    ///   instances; only the full keycap cluster routes to emoji).
    /// - Otherwise consult `presentation_for(first, None)`:
    ///   - `Color` → color emoji first.
    ///   - `Monochrome` → monochrome emoji first.
    ///   - `NotEmoji` → regular text chain (`Self::resolve`).
    ///
    /// When the preferred-side font does not cover the code point we
    /// fall through to the opposite emoji side (if any) before letting
    /// the rest of the text chain take its turn — this is the
    /// "opposite-side fallback before tofu" rule from the SPEC.
    pub fn resolve_for_cluster(
        &self,
        rasterizer: &dyn GlyphRasterizer,
        cluster: &str,
    ) -> Option<FontId> {
        let mut first_cp: Option<char> = None;
        let mut explicit_vs: Option<char> = None;
        let mut has_keycap = false;
        for ch in cluster.chars() {
            if first_cp.is_none() {
                first_cp = Some(ch);
            }
            match ch {
                VS16 if explicit_vs.is_none() => explicit_vs = Some(VS16),
                VS15 if explicit_vs.is_none() => explicit_vs = Some(VS15),
                '\u{20E3}' => has_keycap = true,
                _ => {}
            }
        }
        let first = first_cp?;
        let first_cp_u32 = first as u32;

        // Keycap clusters (`<base> [VS16] U+20E3`) override the
        // `NotEmoji` default the bare base character carries.
        let presentation = if has_keycap {
            EmojiPresentation::Color
        } else {
            presentation_for(first, explicit_vs)
        };

        match presentation {
            EmojiPresentation::Color => {
                if let Some(emoji) = self.emoji {
                    if rasterizer.has_codepoint(emoji, first_cp_u32) {
                        return Some(emoji);
                    }
                }
                // Opposite-side fallback: try monochrome before tofu.
                if let Some(mono) = self.mono_emoji {
                    if rasterizer.has_codepoint(mono, first_cp_u32) {
                        return Some(mono);
                    }
                }
                self.resolve(rasterizer, first_cp_u32)
            }
            EmojiPresentation::Monochrome => {
                if let Some(mono) = self.mono_emoji {
                    if rasterizer.has_codepoint(mono, first_cp_u32) {
                        return Some(mono);
                    }
                }
                // Opposite-side fallback: try color emoji before tofu.
                if let Some(emoji) = self.emoji {
                    if rasterizer.has_codepoint(emoji, first_cp_u32) {
                        return Some(emoji);
                    }
                }
                self.resolve(rasterizer, first_cp_u32)
            }
            EmojiPresentation::NotEmoji => self.resolve(rasterizer, first_cp_u32),
        }
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

    // ── resolve_for_cluster: presentation_for dispatch (SPEC FR5) ──

    /// FR5: U+23F5 (BLACK MEDIUM RIGHT-POINTING TRIANGLE) is a
    /// text-default emoji (Emoji=Yes, Emoji_Presentation=No). A bare
    /// instance must resolve to the monochrome-emoji face even when the
    /// color emoji font lacks the glyph (the U+23F5 was the actual
    /// Windows tofu case that motivated the bundle redesign).
    #[test]
    fn resolve_for_cluster_text_default_emoji_uses_mono() {
        // FontId(1) = base ASCII, FontId(2) = color emoji (no U+23F5),
        // FontId(3) = mono emoji (covers U+23F5).
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41));
        covers.insert((FontId(2), 0x1F600));
        covers.insert((FontId(3), 0x23F5));
        let raster = TableRasterizer { covers };

        let resolved = chain.resolve_for_cluster(&raster, "\u{23F5}");
        assert_eq!(
            resolved,
            Some(FontId(3)),
            "U+23F5 must resolve via MonochromeEmoji role"
        );
    }

    /// FR5: VS16 forces color presentation even for text-default
    /// code points like U+23F5.
    #[test]
    fn resolve_for_cluster_vs16_forces_color() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41));
        covers.insert((FontId(2), 0x23F5));
        covers.insert((FontId(3), 0x23F5));
        let raster = TableRasterizer { covers };

        let cluster: String = ['\u{23F5}', '\u{FE0F}'].iter().collect();
        assert_eq!(
            chain.resolve_for_cluster(&raster, &cluster),
            Some(FontId(2)),
            "U+23F5 + VS16 must resolve via ColorEmoji role"
        );
    }

    /// FR5: VS15 forces monochrome presentation even for emoji-default
    /// code points like U+1F600.
    #[test]
    fn resolve_for_cluster_vs15_forces_mono() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41));
        covers.insert((FontId(2), 0x1F600));
        covers.insert((FontId(3), 0x1F600));
        let raster = TableRasterizer { covers };

        let cluster: String = ['\u{1F600}', '\u{FE0E}'].iter().collect();
        assert_eq!(
            chain.resolve_for_cluster(&raster, &cluster),
            Some(FontId(3)),
            "U+1F600 + VS15 must resolve via MonochromeEmoji role"
        );
    }

    /// FR5: U+1F600 (GRINNING FACE) is an emoji-default code point. A
    /// bare instance must resolve via the color emoji face.
    #[test]
    fn resolve_for_cluster_emoji_default_uses_color() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41));
        covers.insert((FontId(2), 0x1F600));
        // Monochrome also covers — color must still win.
        covers.insert((FontId(3), 0x1F600));
        let raster = TableRasterizer { covers };

        assert_eq!(
            chain.resolve_for_cluster(&raster, "\u{1F600}"),
            Some(FontId(2)),
            "bare U+1F600 must resolve via ColorEmoji role"
        );
    }

    /// "Opposite-side fallback before tofu": when the preferred-side
    /// emoji font lacks the code point, the dispatcher tries the
    /// opposite emoji side before letting the chain walk on.
    #[test]
    fn resolve_for_cluster_opposite_side_fallback() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41));
        // U+1F600 is emoji-default (wants color) but only the
        // monochrome font carries the glyph here.
        covers.insert((FontId(3), 0x1F600));
        let raster = TableRasterizer { covers };

        assert_eq!(
            chain.resolve_for_cluster(&raster, "\u{1F600}"),
            Some(FontId(3)),
            "color-preferring code point falls back to monochrome face"
        );

        // And the reverse: text-default emoji whose monochrome face
        // lacks the glyph falls back to color.
        let mut covers2 = HashSet::new();
        covers2.insert((FontId(1), 0x41));
        covers2.insert((FontId(2), 0x23F5));
        let raster2 = TableRasterizer { covers: covers2 };
        let mut chain2 = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain2.set_emoji(FontId(2));
        chain2.set_mono_emoji(FontId(3));
        assert_eq!(
            chain2.resolve_for_cluster(&raster2, "\u{23F5}"),
            Some(FontId(2)),
            "mono-preferring code point falls back to color face"
        );
    }

    /// ASCII (NotEmoji) must NOT touch the emoji faces even when they
    /// happen to cover the codepoint. The base font wins.
    #[test]
    fn resolve_for_cluster_ascii_stays_on_base() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x41));
        covers.insert((FontId(2), 0x41));
        covers.insert((FontId(3), 0x41));
        let raster = TableRasterizer { covers };

        assert_eq!(
            chain.resolve_for_cluster(&raster, "A"),
            Some(FontId(1)),
            "ASCII must stay on the base font"
        );
    }

    /// Digits in isolation (no keycap suffix) are NotEmoji — they must
    /// resolve via the base font, NOT the monochrome emoji face. This
    /// guards against the digit / `presentation_for` regression where
    /// every ASCII digit would have routed through Noto Emoji and
    /// broken grid alignment.
    #[test]
    fn resolve_for_cluster_bare_digit_stays_on_base() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x35)); // '5'
        // Both emoji faces also cover '5' (Noto Emoji actually ships
        // keycap variants), but they must NOT win for a bare digit.
        covers.insert((FontId(2), 0x35));
        covers.insert((FontId(3), 0x35));
        let raster = TableRasterizer { covers };

        assert_eq!(
            chain.resolve_for_cluster(&raster, "5"),
            Some(FontId(1)),
            "bare digit must resolve via base, not emoji"
        );
    }

    /// Full keycap cluster (`5 VS16 U+20E3`) routes to the color emoji
    /// face — the cluster-level dispatcher detects the keycap suffix
    /// and overrides the bare-digit NotEmoji default.
    #[test]
    fn resolve_for_cluster_keycap_routes_to_color() {
        let mut chain = FallbackChain::new(FontId(1), [FontId(2), FontId(3)]);
        chain.set_emoji(FontId(2));
        chain.set_mono_emoji(FontId(3));
        let mut covers = HashSet::new();
        covers.insert((FontId(1), 0x35));
        covers.insert((FontId(2), 0x35));
        let raster = TableRasterizer { covers };

        let cluster: String = ['5', '\u{FE0F}', '\u{20E3}'].iter().collect();
        assert_eq!(
            chain.resolve_for_cluster(&raster, &cluster),
            Some(FontId(2)),
            "5 + VS16 + U+20E3 must resolve via ColorEmoji role"
        );
    }
}
