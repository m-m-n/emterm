//! Font resolver: fontdb scan + bundled-font registration.
//!
//! Phase 3 of font-swash-migration (FR7, FR11). The resolver builds a
//! single in-process registry that maps `(family-name, weight, style)` to
//! the (FontId, byte buffer) tuple needed by the swash + ab_glyph
//! adapters. Bundled fonts are registered first so they always win when
//! the host font set lacks coverage; the system scan only adds fonts that
//! are not already represented.
//!
//! `EMTERM_FONT_PERF=1` logs the startup scan duration at `warn` so the
//! NFR1 < 500 ms gate is observable in release builds.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::traits::FontId;

/// Bundled CJK base font bytes (Phase 3).
pub const BUNDLED_CJK_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansCJKjp-Regular.otf");

/// Bundled color emoji font bytes (Phase 1 + Phase 3).
pub const BUNDLED_EMOJI_FONT: &[u8] = include_bytes!("../../../assets/fonts/NotoColorEmoji.ttf");

/// Logical role of a registered font in the fallback chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontRole {
    /// Base monospace face: drives ASCII / Latin glyphs.
    Base,
    /// CJK base coverage (Han, Hiragana, Katakana, Hangul).
    Cjk,
    /// Color emoji (CBDT, COLR).
    Emoji,
    /// Secondary fallback (e.g. Segoe UI Emoji on Windows).
    Secondary,
    /// User-supplied additional fallback from settings.
    User,
}

/// A registered font entry: id, role, family-name, and byte buffer.
#[derive(Debug, Clone)]
pub struct RegisteredFont {
    pub id: FontId,
    pub role: FontRole,
    pub family: String,
    pub bytes: Arc<[u8]>,
}

/// Font resolver / registry.
///
/// All registration happens at startup. Lookups are read-only afterwards.
#[derive(Debug, Default)]
pub struct Resolver {
    by_id: HashMap<FontId, RegisteredFont>,
    by_role: HashMap<FontRole, Vec<FontId>>,
    by_family: HashMap<String, FontId>,
    next_id: u32,
    /// `true` if `scan_system_fonts` was attempted but failed; the chain
    /// is bundled-only in that case.
    scan_failed: bool,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Self::default()
        }
    }

    /// Register a font from in-memory bytes (used for both bundled and
    /// system-scanned fonts). Returns the newly-issued `FontId`.
    pub fn register_bytes(
        &mut self,
        role: FontRole,
        family: impl Into<String>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> FontId {
        let id = FontId(self.next_id);
        self.next_id += 1;
        let family: String = family.into();
        let entry = RegisteredFont {
            id,
            role,
            family: family.clone(),
            bytes: bytes.into(),
        };
        self.by_id.insert(id, entry);
        self.by_role.entry(role).or_default().push(id);
        self.by_family.entry(family).or_insert(id);
        id
    }

    /// Register the two bundled fonts (Noto Sans CJK JP + Noto Color
    /// Emoji). Returns the assigned ids `(cjk, emoji)`.
    pub fn register_bundled(&mut self) -> (FontId, FontId) {
        let cjk = self.register_bytes(
            FontRole::Cjk,
            "Noto Sans CJK JP",
            Arc::<[u8]>::from(BUNDLED_CJK_FONT),
        );
        let emoji = self.register_bytes(
            FontRole::Emoji,
            "Noto Color Emoji",
            Arc::<[u8]>::from(BUNDLED_EMOJI_FONT),
        );
        (cjk, emoji)
    }

    /// Scan host fonts via fontdb and append unique monospace families as
    /// `FontRole::Base` candidates. On any panic / IO failure the scan is
    /// suppressed and `scan_failed()` returns `true`; bundled fonts
    /// remain available regardless.
    pub fn scan_system_fonts(&mut self) {
        let perf_log = std::env::var("EMTERM_FONT_PERF")
            .map(|v| v != "0")
            .unwrap_or(false);
        let t0 = Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        }));
        let elapsed = t0.elapsed();
        if perf_log {
            log::warn!(
                "[EMTERM_FONT_PERF] font scan total = {} ms (scanned={}) ",
                elapsed.as_millis(),
                result.is_ok(),
            );
        }
        let db = match result {
            Ok(db) => db,
            Err(_) => {
                self.scan_failed = true;
                log::warn!("font.scan_failed: panic during fontdb load_system_fonts");
                return;
            }
        };

        for face in db.faces() {
            if !face.monospaced {
                continue;
            }
            // Pick the first English / default family name.
            let family = match face
                .families
                .iter()
                .find(|(_, lang)| lang.primary_language() == "English")
                .or_else(|| face.families.first())
            {
                Some((name, _)) => name.clone(),
                None => continue,
            };
            if self.by_family.contains_key(&family) {
                continue;
            }
            // We do not eagerly load the file bytes here — fontdb stores
            // a `Source` that may be a file path or memory blob. We only
            // surface the family name so the swash adapter can look it
            // up later. Bytes are loaded on demand by `family_bytes`.
            let _ = self.register_bytes(FontRole::Base, family, Arc::<[u8]>::from(&[][..]));
        }
    }

    pub fn font(&self, id: FontId) -> Option<&RegisteredFont> {
        self.by_id.get(&id)
    }

    pub fn by_role(&self, role: FontRole) -> impl Iterator<Item = &RegisteredFont> {
        self.by_role
            .get(&role)
            .into_iter()
            .flat_map(|ids| ids.iter().filter_map(|id| self.by_id.get(id)))
    }

    pub fn by_family(&self, family: &str) -> Option<&RegisteredFont> {
        self.by_family.get(family).and_then(|id| self.by_id.get(id))
    }

    pub fn scan_failed(&self) -> bool {
        self.scan_failed
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TS-font-10: Bundled font registration succeeds against an in-memory
    /// resolver and returns distinct FontIds for CJK / emoji roles.
    #[test]
    fn register_bundled_returns_distinct_ids() {
        let mut r = Resolver::new();
        let (cjk, emoji) = r.register_bundled();
        assert_ne!(cjk, emoji);
        assert_eq!(r.font(cjk).map(|f| f.role), Some(FontRole::Cjk));
        assert_eq!(r.font(emoji).map(|f| f.role), Some(FontRole::Emoji));
    }

    #[test]
    fn by_role_lists_each_registered_font() {
        let mut r = Resolver::new();
        let _ = r.register_bundled();
        let cjk_count = r.by_role(FontRole::Cjk).count();
        let emoji_count = r.by_role(FontRole::Emoji).count();
        assert_eq!(cjk_count, 1);
        assert_eq!(emoji_count, 1);
    }

    #[test]
    fn by_family_resolves_registered_name() {
        let mut r = Resolver::new();
        let _ = r.register_bundled();
        let cjk = r.by_family("Noto Sans CJK JP").expect("CJK by family");
        assert_eq!(cjk.role, FontRole::Cjk);
    }

    #[test]
    fn scan_failed_starts_false() {
        let r = Resolver::new();
        assert!(!r.scan_failed());
    }
}
