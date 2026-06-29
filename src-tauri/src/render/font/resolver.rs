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

/// Bundled CJK Bold face bytes. Wired as the bold variant of the bundled
/// CJK Regular so SGR-bold CJK cells render with real Bold weight even
/// when the host lacks a `Noto Sans JP` Bold installation.
pub const BUNDLED_CJK_BOLD_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansCJKjp-Bold.otf");

/// Bundled color emoji font bytes (CBDT / COLR).
pub const BUNDLED_EMOJI_COLOR_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoColorEmoji.ttf");

/// Bundled monochrome emoji font bytes (outline-only). Used for text-default
/// emoji code points (e.g. U+23F5 `⏵`) and for VS15-attached selections.
pub const BUNDLED_EMOJI_MONO_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoEmoji-Regular.ttf");

/// Bundled base monospace font bytes (Inconsolata). Used as the last-resort
/// ASCII / Latin face when no system monospace family is configured.
///
/// CFF (.otf) format chosen over TrueType (.ttf): at terminal cell sizes
/// swash's TT-bytecode hinting aggressively snaps Bold stems to the pixel
/// grid (verified via histogram probe: full-ink/edge-AA ratio jumps from
/// 1.2 to 2.2 vs the CFF cut), reading as "no antialiasing" on bold cells.
/// The CFF outlines keep their AA gradients at small sizes.
pub const BUNDLED_BASE_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/Inconsolata-Regular.otf");

/// Bundled base monospace Bold face bytes. Wired as the bold variant of
/// the bundled Inconsolata Regular so SGR-bold Latin cells render with
/// real Bold weight even when the host lacks an Inconsolata installation.
///
/// CFF (.otf) for the same swash-hinting reasons as [`BUNDLED_BASE_FONT`].
pub const BUNDLED_BASE_BOLD_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/Inconsolata-Bold.otf");

/// Bundled symbols font bytes (Noto Sans Symbols 2). Covers code points the
/// other bundled faces miss — prompt arrows (`❯` U+276F), media controls
/// (`⏵` U+23F5), geometric shapes used by TUIs.
pub const BUNDLED_SYMBOLS_FONT: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansSymbols2-Regular.ttf");

/// Logical role of a registered font in the fallback chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontRole {
    /// Base monospace face: drives ASCII / Latin glyphs.
    Base,
    /// CJK base coverage (Han, Hiragana, Katakana, Hangul).
    Cjk,
    /// Color emoji (CBDT, COLR, sbix).
    ColorEmoji,
    /// Monochrome emoji (outline glyphs; used for text-default emoji).
    MonochromeEmoji,
    /// Secondary fallback (e.g. Segoe UI Emoji on Windows).
    Secondary,
    /// User-supplied additional fallback (settings, user font directory).
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

/// Rank a face within a family for [`Resolver::register_system_family`]:
/// lower is better. Mirrors a simplified CSS font-matching order
/// (stretch > style > weight) so a family installed with many faces
/// resolves to the upright face closest to `target_weight` instead of
/// whichever face the system scan happens to enumerate first.
fn face_match_penalty(
    weight: u16,
    target_weight: u16,
    style_normal: bool,
    stretch_normal: bool,
) -> u32 {
    let stretch_penalty: u32 = if stretch_normal { 0 } else { 2000 };
    let style_penalty: u32 = if style_normal { 0 } else { 1000 };
    let weight_dist = (weight as i32 - target_weight as i32).unsigned_abs();
    stretch_penalty + style_penalty + weight_dist
}

/// Process-wide host font database, loaded once on first use.
///
/// `load_system_fonts()` enumerates every installed font file (tens to
/// hundreds of ms on font-heavy hosts); startup used to repeat that
/// scan for every family lookup (terminal base / bold / CJK / emoji,
/// plus the egui chrome's `ui_font_family`). Sharing one instance
/// bounds the cost to a single scan. fontdb holds only face metadata —
/// font bytes still load on demand — so keeping it for the process
/// lifetime is cheap.
static FONT_DB: std::sync::OnceLock<fontdb::Database> = std::sync::OnceLock::new();

/// Resolve the shared host font database (first call performs the
/// scan; later calls are free). Panics propagate to the caller —
/// `Resolver::scan_system_fonts` wraps its call in `catch_unwind` to
/// preserve its bundled-fonts-only fallback.
pub(crate) fn shared_font_db() -> &'static fontdb::Database {
    FONT_DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

/// Locate the best-matching face for `family` in the host font database
/// and eagerly read its bytes. Returns `(bytes, face_index)`; the index
/// is non-zero for `.ttc` / `.otc` collection members. Shared between
/// the terminal-grid resolver (swash ingest) and the egui chrome font
/// path (`settings.ui_font_family` → `FontDefinitions`).
pub(crate) fn load_system_family_bytes(
    family: &str,
    target_weight: u16,
    min_weight: Option<u16>,
) -> Option<(Arc<[u8]>, u32)> {
    let db = shared_font_db();
    // A family can ship many faces (Google's Inconsolata installs
    // Thin..Black all under the family name "Inconsolata"); picking
    // the first enumerated face made the base font an arbitrary
    // weight — often Bold, rendering every cell as if SGR bold were
    // set. Select the best face instead: normal stretch/style first,
    // then the weight closest to `target_weight`.
    let face = db
        .faces()
        .filter(|f| {
            f.families
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(family))
        })
        .min_by_key(|f| {
            face_match_penalty(
                f.weight.0,
                target_weight,
                f.style == fontdb::Style::Normal,
                f.stretch == fontdb::Stretch::Normal,
            )
        })?;
    // Bold lookups require a genuinely heavy face: a family that only
    // ships Regular would otherwise "win" the weight-700 query with
    // its 400 face and the renderer would re-register the regular
    // bytes as a phantom bold.
    if let Some(min) = min_weight {
        if face.weight.0 < min {
            return None;
        }
    }
    let source = face.source.clone();
    let index = face.index;
    let bytes: Arc<[u8]> = match source {
        fontdb::Source::Binary(b) => {
            let raw: &[u8] = b.as_ref().as_ref();
            Arc::<[u8]>::from(raw)
        }
        fontdb::Source::File(path) => match std::fs::read(&path) {
            Ok(buf) => Arc::<[u8]>::from(buf.as_slice()),
            Err(e) => {
                log::warn!(
                    "font.system_family.read_failed: family={} path={} err={}",
                    family,
                    path.display(),
                    e
                );
                return None;
            }
        },
        fontdb::Source::SharedFile(path, shared) => {
            let raw: &[u8] = shared.as_ref().as_ref();
            if raw.is_empty() {
                match std::fs::read(&path) {
                    Ok(buf) => Arc::<[u8]>::from(buf.as_slice()),
                    Err(e) => {
                        log::warn!(
                            "font.system_family.read_failed: family={} path={} err={}",
                            family,
                            path.display(),
                            e
                        );
                        return None;
                    }
                }
            } else {
                Arc::<[u8]>::from(raw)
            }
        }
    };
    if bytes.is_empty() {
        log::warn!("font.system_family.empty_bytes: family={}", family);
        return None;
    }
    Some((bytes, index))
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

    /// Register the bundled Bold cuts of the base (Inconsolata) and CJK
    /// (Noto Sans CJK JP) families. Returns `(base_bold, cjk_bold)`. The
    /// families are tagged `(bundled bold)` so `by_family` lookups never
    /// alias to them for ordinary regular-face requests.
    pub fn register_bundled_bold_faces(&mut self) -> (FontId, FontId) {
        let base = self.register_bytes(
            FontRole::Base,
            "Inconsolata (bundled bold)",
            Arc::<[u8]>::from(BUNDLED_BASE_BOLD_FONT),
        );
        let cjk = self.register_bytes(
            FontRole::Cjk,
            "Noto Sans CJK JP (bundled bold)",
            Arc::<[u8]>::from(BUNDLED_CJK_BOLD_FONT),
        );
        (base, cjk)
    }

    /// Register the five bundled fonts (Noto Sans CJK JP, Noto Color
    /// Emoji, Noto Emoji monochrome, Inconsolata base, Noto Sans
    /// Symbols 2). Returns the assigned ids
    /// `(cjk, color_emoji, mono_emoji, base, symbols)`.
    ///
    /// Family names carry a `(bundled)` suffix so a later
    /// [`Resolver::register_system_family`] call for the same family
    /// name (e.g. `"Noto Color Emoji"`) does not short-circuit on the
    /// bundled entry — callers that explicitly want the host-installed
    /// font (typically newer, with extended emoji coverage) get a
    /// distinct `FontId` instead of being silently aliased back to the
    /// bundled bytes.
    pub fn register_bundled(&mut self) -> (FontId, FontId, FontId, FontId, FontId) {
        let cjk = self.register_bytes(
            FontRole::Cjk,
            "Noto Sans CJK JP (bundled)",
            Arc::<[u8]>::from(BUNDLED_CJK_FONT),
        );
        let color = self.register_bytes(
            FontRole::ColorEmoji,
            "Noto Color Emoji (bundled)",
            Arc::<[u8]>::from(BUNDLED_EMOJI_COLOR_FONT),
        );
        let mono = self.register_bytes(
            FontRole::MonochromeEmoji,
            "Noto Emoji (bundled)",
            Arc::<[u8]>::from(BUNDLED_EMOJI_MONO_FONT),
        );
        let base = self.register_bytes(
            FontRole::Base,
            "Inconsolata (bundled)",
            Arc::<[u8]>::from(BUNDLED_BASE_FONT),
        );
        let symbols = self.register_bytes(
            FontRole::Secondary,
            "Noto Sans Symbols 2 (bundled)",
            Arc::<[u8]>::from(BUNDLED_SYMBOLS_FONT),
        );
        (cjk, color, mono, base, symbols)
    }

    /// Try to load a specific system font family with its file bytes and
    /// register it under the given role. Returns the assigned `FontId` if
    /// the family was found and bytes were successfully read. Returns
    /// `None` (and logs a warn) when the family is absent or the source
    /// could not be loaded.
    ///
    /// Phase 4-H follow-up: the legacy `scan_system_fonts` registers
    /// monospace family names with empty byte buffers (the swash adapter
    /// skips those), so it cannot reach the rasterizer. This helper is the
    /// pragmatic path until a richer system-font pipeline lands — callers
    /// pass the canonical family name (e.g. `"Inconsolata"`,
    /// `"Noto Sans JP"`) and we eagerly read the file into memory so the
    /// swash adapter can ingest it via `ingest_resolver`.
    pub fn register_system_family(&mut self, family: &str, role: FontRole) -> Option<FontId> {
        self.register_system_family_at_weight(family, role, 400, None, family)
    }

    /// Like [`Resolver::register_system_family`] but selects the family's
    /// Bold face (target weight 700). Returns `None` when the family has
    /// no face of weight ≥ 600 — callers should treat that as "no real
    /// bold available" and keep using the regular face. The font is
    /// registered under `"{family} (bold)"` so `by_family` lookups for
    /// the regular face never alias to the bold bytes.
    pub fn register_system_family_bold(&mut self, family: &str, role: FontRole) -> Option<FontId> {
        let registry_name = format!("{family} (bold)");
        self.register_system_family_at_weight(family, role, 700, Some(600), &registry_name)
    }

    fn register_system_family_at_weight(
        &mut self,
        family: &str,
        role: FontRole,
        target_weight: u16,
        min_weight: Option<u16>,
        registry_name: &str,
    ) -> Option<FontId> {
        if let Some(existing) = self.by_family.get(registry_name).copied() {
            if let Some(entry) = self.by_id.get(&existing) {
                if !entry.bytes.is_empty() {
                    return Some(existing);
                }
            }
        }
        let (bytes, index) = load_system_family_bytes(family, target_weight, min_weight)?;
        // .ttc / .otc collections expose multiple faces in a single file;
        // we currently only ingest face 0 in the swash path. Warn (but
        // still register) so we can revisit if multi-face collections
        // become important.
        if index != 0 {
            log::warn!(
                "font.system_family.non_zero_face_index: family={} index={} (only face 0 will rasterize)",
                family,
                index
            );
        }
        // Overwrite any prior (empty-bytes) entry under this registry name.
        self.by_family.remove(registry_name);
        Some(self.register_bytes(role, registry_name.to_string(), bytes))
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
        // The shared DB loads on first use; when an earlier family
        // lookup already triggered the scan this is a cache hit and the
        // perf log reports ~0 ms.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(shared_font_db));
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

    /// TS-9: bundled font registration returns five distinct FontIds —
    /// one each for CJK, color emoji, monochrome emoji, the base
    /// monospace face, and the symbols face.
    #[test]
    fn register_bundled_returns_distinct_ids() {
        let mut r = Resolver::new();
        let (cjk, color, mono, base, symbols) = r.register_bundled();
        let ids = [cjk, color, mono, base, symbols];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j], "ids[{i}] == ids[{j}]");
            }
        }
        assert_eq!(r.font(cjk).map(|f| f.role), Some(FontRole::Cjk));
        assert_eq!(r.font(color).map(|f| f.role), Some(FontRole::ColorEmoji));
        assert_eq!(
            r.font(mono).map(|f| f.role),
            Some(FontRole::MonochromeEmoji)
        );
        assert_eq!(r.font(base).map(|f| f.role), Some(FontRole::Base));
        assert_eq!(r.font(symbols).map(|f| f.role), Some(FontRole::Secondary));
        for id in ids {
            assert!(
                !r.font(id).unwrap().bytes.is_empty(),
                "bundled font {id:?} has empty bytes"
            );
        }
    }

    #[test]
    fn by_role_lists_each_registered_font() {
        let mut r = Resolver::new();
        let _ = r.register_bundled();
        assert_eq!(r.by_role(FontRole::Cjk).count(), 1);
        assert_eq!(r.by_role(FontRole::ColorEmoji).count(), 1);
        assert_eq!(r.by_role(FontRole::MonochromeEmoji).count(), 1);
        assert_eq!(r.by_role(FontRole::Base).count(), 1);
        assert_eq!(r.by_role(FontRole::Secondary).count(), 1);
    }

    /// Bundled Bold faces register as distinct ids tagged
    /// `(bundled bold)` so `by_family` lookups never alias them in
    /// place of the regular faces.
    #[test]
    fn register_bundled_bold_faces_registers_distinct_ids() {
        let mut r = Resolver::new();
        let (cjk_reg, _color, _mono, base_reg, _symbols) = r.register_bundled();
        let (base_bold, cjk_bold) = r.register_bundled_bold_faces();
        for (a, b) in [
            (base_reg, base_bold),
            (cjk_reg, cjk_bold),
            (base_bold, cjk_bold),
        ] {
            assert_ne!(a, b);
        }
        let base_bold_entry = r
            .by_family("Inconsolata (bundled bold)")
            .expect("base bold by family");
        assert_eq!(base_bold_entry.role, FontRole::Base);
        assert_eq!(base_bold_entry.id, base_bold);
        assert!(!base_bold_entry.bytes.is_empty());
        let cjk_bold_entry = r
            .by_family("Noto Sans CJK JP (bundled bold)")
            .expect("cjk bold by family");
        assert_eq!(cjk_bold_entry.role, FontRole::Cjk);
        assert_eq!(cjk_bold_entry.id, cjk_bold);
        assert!(!cjk_bold_entry.bytes.is_empty());
    }

    #[test]
    fn by_family_resolves_registered_name() {
        let mut r = Resolver::new();
        let _ = r.register_bundled();
        // Bundled families carry the `(bundled)` suffix so a later
        // `register_system_family("Noto Color Emoji", …)` does not
        // alias back to the bundled bytes.
        let cjk = r
            .by_family("Noto Sans CJK JP (bundled)")
            .expect("CJK by family");
        assert_eq!(cjk.role, FontRole::Cjk);
        let color = r
            .by_family("Noto Color Emoji (bundled)")
            .expect("color emoji by family");
        assert_eq!(color.role, FontRole::ColorEmoji);
        let mono = r.by_family("Noto Emoji (bundled)").expect("mono by family");
        assert_eq!(mono.role, FontRole::MonochromeEmoji);
        let base = r
            .by_family("Inconsolata (bundled)")
            .expect("base by family");
        assert_eq!(base.role, FontRole::Base);
        let symbols = r
            .by_family("Noto Sans Symbols 2 (bundled)")
            .expect("symbols by family");
        assert_eq!(symbols.role, FontRole::Secondary);
    }

    #[test]
    fn scan_failed_starts_false() {
        let r = Resolver::new();
        assert!(!r.scan_failed());
    }

    // ── face_match_penalty ──────────────────────────────────

    /// Regular (400, upright, normal stretch) beats every other weight
    /// in the family — Bold must not win the base-font slot.
    #[test]
    fn face_match_penalty_prefers_regular_weight() {
        let regular = face_match_penalty(400, 400, true, true);
        for w in [100u16, 200, 300, 500, 600, 700, 800, 900] {
            assert!(
                regular < face_match_penalty(w, 400, true, true),
                "weight {} unexpectedly ranked at least as good as Regular",
                w
            );
        }
    }

    /// An italic or condensed Regular ranks below any upright
    /// normal-stretch weight (stretch > style > weight ordering).
    #[test]
    fn face_match_penalty_orders_stretch_over_style_over_weight() {
        let upright_black = face_match_penalty(900, 400, true, true);
        let italic_regular = face_match_penalty(400, 400, false, true);
        let condensed_regular = face_match_penalty(400, 400, true, false);
        assert!(upright_black < italic_regular);
        assert!(italic_regular < condensed_regular);
    }

    /// With a bold target (700) the true Bold face wins over both the
    /// Regular face and heavier siblings like ExtraBold/Black.
    #[test]
    fn face_match_penalty_bold_target_prefers_700() {
        let bold = face_match_penalty(700, 700, true, true);
        for w in [100u16, 200, 300, 400, 500, 600, 800, 900] {
            assert!(
                bold < face_match_penalty(w, 700, true, true),
                "weight {} unexpectedly ranked at least as good as Bold",
                w
            );
        }
    }
}
