//! Font stack construction, zoom and settings application for [`App`].

use std::sync::Arc;

use parking_lot::Mutex;

use crate::render::font::cache::GlyphCache;
use crate::render::font::fallback::FallbackChain;
use crate::render::font::resolver::Resolver;
use crate::render::font::traits::{FontId, GlyphRasterizer};
use crate::settings::{FontEngine, Settings};

use super::{App, build_mux_latch};

/// Per-step change applied by the `ZoomIn` / `ZoomOut` keybinds, in
/// logical points.
pub const FONT_SIZE_PT_STEP: f32 = 1.0;
/// Lower clamp for the runtime terminal font size (logical points).
pub const FONT_SIZE_PT_MIN: f32 = 6.0;
/// Upper clamp for the runtime terminal font size (logical points).
pub const FONT_SIZE_PT_MAX: f32 = 72.0;

/// Clamp a candidate terminal font size (logical points) into
/// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]. Split out as a pure
/// function so the zoom clamp can be unit-tested without constructing a
/// full `App` (which builds the font stack and a status-bar runtime).
pub fn clamp_font_size_pt(pt: f32) -> f32 {
    pt.clamp(FONT_SIZE_PT_MIN, FONT_SIZE_PT_MAX)
}

impl App {
    /// Phase 4-H startup wiring (FR12 + FR6 + FR7 + FR11). Build the
    /// resolver, register bundled fonts, branch on
    /// `Settings::font_engine` to construct either the Swash or AbGlyph
    /// rasterizer, build the fallback chain, and seed the glyph cache.
    /// The returned tuple is owned by `App`; the renderer's
    /// `TerminalGridPass` borrows clones of each `Arc`.
    ///
    /// The rasterizer in the returned tuple is fully initialized:
    /// `set_base_font` has already been called with `base_id` before
    /// returning, so callers do not need to call it again.
    pub(super) fn build_font_stack(
        settings: &Settings,
    ) -> (
        Arc<Resolver>,
        Arc<FallbackChain>,
        Arc<Mutex<GlyphCache>>,
        Arc<dyn GlyphRasterizer>,
        FontId,
    ) {
        #[cfg(not(test))]
        use crate::render::font::resolver::FontRole;

        let mut resolver = Resolver::new();
        // FR6 resolution priority (highest first):
        //   1. settings-supplied family
        //   2. user override directory
        //   3. system fonts
        //   4. bundled fonts
        //
        // Registration order matters for `by_family` lookups —
        // `Resolver::register_bytes` short-circuits on the first entry
        // for a given family name. We therefore register in highest →
        // lowest priority order.

        // Emoji families are bundled and fixed; user-side selection was
        // removed (see font-bundle-cleanup report). Only the bundled
        // `Noto Color Emoji` / `Noto Emoji` faces serve `FontRole::ColorEmoji`
        // / `FontRole::MonochromeEmoji` from now on.

        // 2. User override directory. The scan is silently a no-op when
        //    the directory does not exist. Skipped during tests so unit
        //    tests don't touch the real user env.
        #[cfg(not(test))]
        resolver.scan_user_dir();

        // 4. Bundled fonts. `register_bundled` registers CJK, color
        //    emoji, monochrome emoji, the base monospace face, and the
        //    symbols face (Noto Sans Symbols 2 → `❯` U+276F / `⏵`
        //    U+23F5 etc.). We keep handles to all of them so the chain
        //    composition below can promote the bundled base font over
        //    the bundled CJK font when the host monospace family is
        //    absent. The symbols face is registered as
        //    `FontRole::Secondary`, so the fallback chain picks it up
        //    automatically without an explicit local.
        let (bundled_cjk_id, emoji_id, bundled_mono_emoji_id, bundled_base_id, _symbols_id) =
            resolver.register_bundled();
        // Bundled Bold cuts so SGR-bold renders with real Bold weight even
        // when the host has no Inconsolata / Noto Sans JP installation.
        // Wired into the chain via `set_bold_variant` below.
        let (bundled_base_bold_id, bundled_cjk_bold_id) = resolver.register_bundled_bold_faces();

        // Host-font preferences sourced from `settings.font_family_fallback`:
        //   fallback[0] -> base (Latin / monospace)
        //   fallback[1] -> CJK fallback
        // Both slots are `Option<String>`: when the user has not specified
        // a family in that slot, the host scan is skipped entirely and the
        // bundled face wins — otherwise an installed `Inconsolata` /
        // `Noto Sans JP` would silently override the bundled fonts even
        // for users with an empty settings.json. The bundled CJK font's
        // Latin sub-set is not monospaced, so the chain still keeps the
        // bundled Inconsolata as the base when no host family is requested
        // (see `base_id` below).
        #[cfg(not(test))]
        let base_family: Option<String> = settings
            .font_family_fallback
            .first()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        #[cfg(not(test))]
        let cjk_family: Option<String> = settings
            .font_family_fallback
            .get(1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        #[cfg(not(test))]
        let inconsolata_id = base_family
            .as_deref()
            .and_then(|f| resolver.register_system_family(f, FontRole::Base));
        #[cfg(test)]
        let inconsolata_id: Option<FontId> = None;

        // SGR-bold faces: register the real Bold cut of the base / CJK
        // families when the user requested a host family AND that family
        // ships a face of weight ≥ 600. Either condition failing leaves
        // bold cells on the regular face.
        #[cfg(not(test))]
        let base_bold_id = match (base_family.as_deref(), inconsolata_id) {
            (Some(f), Some(_)) => resolver.register_system_family_bold(f, FontRole::Base),
            _ => None,
        };
        #[cfg(test)]
        let base_bold_id: Option<FontId> = None;

        #[cfg(not(test))]
        let noto_sans_jp_id = cjk_family
            .as_deref()
            .and_then(|f| resolver.register_system_family(f, FontRole::Cjk));
        #[cfg(test)]
        let noto_sans_jp_id: Option<FontId> = None;

        #[cfg(not(test))]
        let cjk_bold_id = match (cjk_family.as_deref(), noto_sans_jp_id) {
            (Some(f), Some(_)) => resolver.register_system_family_bold(f, FontRole::Cjk),
            _ => None,
        };
        #[cfg(test)]
        let cjk_bold_id: Option<FontId> = None;

        // `host_emoji_id` / `host_mono_emoji_id` are registered above
        // (before `register_bundled`) so that a settings-supplied family
        // wins the resolver's `by_family` lookup against bundled +
        // system entries (FR6 priority #1).

        // Symbol fallback: covered by the bundled
        // `Noto Sans Symbols 2 (bundled)` registered in
        // `register_bundled()` above (FontRole::Secondary). The
        // previous host-side family probe (Noto Sans Symbols2 /
        // Symbola / DejaVu) is now redundant on Linux and never fired
        // on Windows anyway — bundling guarantees `❯` U+276F /
        // `⏵` U+23F5 / surrounding shapes everywhere.

        // Best-effort wider system scan (logged at WARN on failure;
        // family-name only, byte loading is deferred). Tests skip the
        // scan to keep cargo test deterministic.
        #[cfg(not(test))]
        resolver.scan_system_fonts();

        let rasterizer: Arc<dyn GlyphRasterizer> = match settings.font_engine {
            FontEngine::Swash => {
                let swash = Arc::new(
                    crate::render::font::swash_adapter::SwashRasterizer::with_axes(
                        &settings.variable_font_axes,
                    ),
                );
                swash.ingest_resolver(&resolver);
                log::info!(
                    "font.antialias = {}",
                    if swash.subpixel() {
                        "subpixel-rgb"
                    } else {
                        "grayscale (EMTERM_SUBPIXEL=0)"
                    }
                );
                swash
            }
            FontEngine::AbGlyph => {
                // ab_glyph escape hatch: we wrap the bundled CJK font
                // (which carries a Latin sub-set) so ASCII still
                // renders. CJK / emoji return None and the fallback
                // chain stops — that is the documented degradation
                // path (FR5).
                if !settings.variable_font_axes.is_empty() {
                    log::warn!(
                        "font.variable_axes: ignored under font_engine = ab_glyph (swash only)"
                    );
                }
                match crate::render::font::ab_glyph_adapter::AbGlyphRasterizer::from_static_bytes(
                    crate::render::font::resolver::BUNDLED_CJK_FONT,
                    bundled_cjk_id,
                ) {
                    Some(r) => {
                        log::info!("font_engine = ab_glyph (escape hatch); CJK / emoji may tofu");
                        Arc::new(r)
                    }
                    None => {
                        // Honor the "axes ignored under ab_glyph" contract
                        // even on this fallback: the user explicitly chose
                        // the no-variable-font escape hatch, so a parse
                        // failure that lands us back on swash must not
                        // silently re-enable the axes we just warned were
                        // off (`new()`, not `with_axes`).
                        log::warn!(
                            "font.unknown_engine: ab_glyph failed to parse bundled CJK; falling back to swash (variable_font_axes stay ignored)"
                        );
                        let swash =
                            Arc::new(crate::render::font::swash_adapter::SwashRasterizer::new());
                        swash.ingest_resolver(&resolver);
                        swash
                    }
                }
            }
        };

        // Pick the chain root: prefer the host-installed monospace
        // family when it loaded successfully; otherwise the bundled
        // Inconsolata covers the ASCII / Latin role so the base layer
        // still renders monospaced even when no host font is available.
        // We only fall back to the bundled CJK font as a last resort
        // (its Latin sub-set is not monospaced and visibly skews grid
        // alignment) — that case only occurs if both the host base
        // family and the bundled base font registration failed.
        let base_id = inconsolata_id.unwrap_or(bundled_base_id);
        let mut extras: Vec<FontId> = Vec::new();
        // FR6 priority #2: user-override font directory. The fonts are
        // already registered under `FontRole::User`, but they have to
        // appear in the chain ahead of CJK + emoji + secondary so the
        // per-codepoint walk consults them first.
        #[cfg(not(test))]
        for f in resolver.by_role(FontRole::User) {
            extras.push(f.id);
            log::info!("font.user = {} (id={:?})", f.family, f.id);
        }
        if let Some(jp) = noto_sans_jp_id {
            extras.push(jp);
        }
        if base_id != bundled_cjk_id {
            // Keep the bundled CJK font as a last-resort CJK fallback
            // (covers KR / TC / SC / extended CJK that NSJP omits).
            extras.push(bundled_cjk_id);
        }
        // Symbol fallback families registered above as
        // FontRole::Secondary (`Noto Sans Symbols2`, `Symbola`, etc.)
        // occupy the `font_family_fallback...` slot in SPEC FR8's
        // `[base, font_family_fallback..., emoji_font]` chain order.
        // They cover codepoints the base + CJK fonts miss — most
        // visibly `❯` U+276F shown by starship.
        //
        // Note: for codepoints in `is_pictographic`'s range
        // (0x2600..=0x27BF + emoji blocks), `FallbackChain::resolve`
        // checks `self.emoji` FIRST regardless of chain order, so
        // Secondary only catches a pictographic codepoint when the
        // emoji font does NOT cover it. Today Noto Color Emoji omits
        // dingbat ornaments like U+276F / U+2731, so the Secondary
        // chain catches them as intended. If a future emoji font
        // gains dingbat coverage, the `resolve_pictographic_falls_to_
        // secondary_when_emoji_misses` regression test in
        // `render/font/fallback.rs` pins the "miss → Secondary"
        // contract so the regression surfaces immediately.
        #[cfg(not(test))]
        for f in resolver.by_role(FontRole::Secondary) {
            extras.push(f.id);
            log::info!("font.symbol = {} (id={:?})", f.family, f.id);
        }
        // Color and monochrome emoji come from the bundled Noto faces
        // exclusively. The bundle is SSOT — host emoji fonts are not
        // consulted (e.g. Windows' system Noto Color Emoji ships as
        // COLRv1+SVG which swash cannot raster).
        extras.push(emoji_id);
        extras.push(bundled_mono_emoji_id);
        let preferred_emoji_id = emoji_id;
        #[cfg(not(test))]
        match (&base_family, inconsolata_id) {
            (Some(family), Some(id)) => {
                log::info!("font.base = {} (id={:?})", family, id);
            }
            (Some(family), None) => {
                log::warn!(
                    "font.base = bundled Inconsolata ({:?} not found on host)",
                    family
                );
            }
            (None, _) => {
                log::info!("font.base = bundled Inconsolata (no user override)");
            }
        }
        #[cfg(not(test))]
        match (&cjk_family, noto_sans_jp_id) {
            (Some(family), Some(id)) => {
                log::info!("font.jp = {} (id={:?})", family, id);
            }
            (Some(family), None) => {
                log::warn!(
                    "font.jp = bundled Noto Sans CJK JP ({:?} not found on host)",
                    family
                );
            }
            (None, _) => {
                log::info!("font.jp = bundled Noto Sans CJK JP (no user override)");
            }
        }
        log::info!("font.emoji = bundled Noto Color Emoji (id={:?})", emoji_id);
        let mut chain = FallbackChain::new(base_id, extras);
        // Mark the preferred emoji font as the color-emoji source so
        // VS-16-bearing clusters (e.g. ⚠️ = U+26A0 + U+FE0F) and bare
        // pictographs (✅ U+2705, 🟢 U+1F7E2) resolve to it instead of
        // the BW base / CJK fonts that may also cover those codepoints.
        chain.set_emoji(preferred_emoji_id);
        // Mark the bundled monochrome-emoji font so text-default emoji
        // code points (e.g. U+23F5 `⏵`) and VS15-attached clusters
        // route to the outline face instead of the BW base monospace
        // font (which has no glyph for them). FR5 "opposite-side
        // fallback before tofu" is handled inside
        // `FallbackChain::resolve_for_cluster`.
        chain.set_mono_emoji(bundled_mono_emoji_id);
        // Wire the real bold faces so SGR-bold cells render with them.
        // Bundled Bold cuts always cover the bundled Regular faces so the
        // default config (no system override) still renders bold correctly.
        chain.set_bold_variant(bundled_base_id, bundled_base_bold_id);
        chain.set_bold_variant(bundled_cjk_id, bundled_cjk_bold_id);
        log::info!(
            "font.base.bold = bundled Inconsolata (id={:?})",
            bundled_base_bold_id
        );
        log::info!(
            "font.jp.bold = bundled Noto Sans CJK JP (id={:?})",
            bundled_cjk_bold_id
        );
        // Layer the host-installed Bold cuts on top when the user asked for
        // a system family and it ships a weight ≥ 600 face. These shadow
        // the bundled wiring above only for the system Regular ids.
        #[cfg(not(test))]
        if let (Some(regular), Some(bold)) = (inconsolata_id, base_bold_id) {
            chain.set_bold_variant(regular, bold);
            log::info!(
                "font.base.bold = {} (id={:?})",
                base_family.as_deref().unwrap_or(""),
                bold
            );
        }
        #[cfg(not(test))]
        if let (Some(regular), Some(bold)) = (noto_sans_jp_id, cjk_bold_id) {
            chain.set_bold_variant(regular, bold);
            log::info!(
                "font.jp.bold = {} (id={:?})",
                cjk_family.as_deref().unwrap_or(""),
                bold
            );
        }
        #[cfg(test)]
        {
            // Silence unused-variable lints in the test cfg where the
            // bold ids are compile-time `None`.
            let _ = (base_bold_id, cjk_bold_id);
        }

        // Font smoke diagnostic — release/non-test only. Forces the
        // result into `emterm.log` even when the host has not enabled
        // `log_recording_enabled`, so a Windows user can hand us the
        // log file without editing settings.json first. Runs once at
        // startup so the cost is bounded.
        #[cfg(not(test))]
        {
            let probe_cp: u32 = 0x1F600; // 😀
            let covers = rasterizer.has_codepoint(preferred_emoji_id, probe_cp);
            let summary_chain: Vec<String> = chain
                .chain()
                .iter()
                .map(|id| {
                    resolver
                        .font(*id)
                        .map(|f| format!("{:?}={}", id, f.family))
                        .unwrap_or_else(|| format!("{:?}=?", id))
                })
                .collect();
            crate::logging::force_log_line(
                log::Level::Info,
                &format!(
                    "font.diag.chain = [{}] base={:?} emoji={:?} covers_U+1F600={}",
                    summary_chain.join(", "),
                    base_id,
                    preferred_emoji_id,
                    covers,
                ),
            );
            log::info!(
                "font.diag.chain = [{}] base={:?} emoji={:?} covers_U+1F600={}",
                summary_chain.join(", "),
                base_id,
                preferred_emoji_id,
                covers,
            );
            // Shape + raster smoke test for U+1F600 at a representative
            // terminal cell pixel size. Empty shape / None raster is the
            // first visible symptom on Windows; the log line tells us
            // which stage broke (charmap miss vs raster failure vs
            // bitmap-strike unavailable).
            let shaped = rasterizer.shape("\u{1F600}", preferred_emoji_id, 17.0);
            let first = shaped.into_iter().next();
            match first {
                None => {
                    let msg = format!(
                        "font.diag.smoke: shape returned no glyphs for U+1F600 (font={:?})",
                        preferred_emoji_id,
                    );
                    crate::logging::force_log_line(log::Level::Warn, &msg);
                    log::warn!("{}", msg);
                }
                Some(g) => match rasterizer.raster(g.font, g.glyph_id, g.size_px) {
                    None => {
                        let msg = format!(
                            "font.diag.smoke: raster returned None glyph_id={} size_px={} font={:?}",
                            g.glyph_id, g.size_px, g.font,
                        );
                        crate::logging::force_log_line(log::Level::Warn, &msg);
                        log::warn!("{}", msg);
                    }
                    Some(b) => {
                        let nonzero = match b.format {
                            crate::render::font::traits::AtlasFormat::Rgba => {
                                b.pixels.chunks_exact(4).filter(|px| px[3] != 0).count()
                            }
                            _ => b.pixels.iter().filter(|&&v| v != 0).count(),
                        };
                        let msg = format!(
                            "font.diag.smoke: raster ok format={:?} w={} h={} advance={:.1} bytes={} nonzero={}",
                            b.format,
                            b.width,
                            b.height,
                            b.advance,
                            b.pixels.len(),
                            nonzero,
                        );
                        crate::logging::force_log_line(log::Level::Info, &msg);
                        log::info!("{}", msg);
                    }
                },
            }
        }

        rasterizer.set_base_font(base_id);
        (
            Arc::new(resolver),
            Arc::new(chain),
            Arc::new(Mutex::new(GlyphCache::new())),
            rasterizer,
            base_id,
        )
    }

    /// Increase the runtime terminal font size by one point (clamped to
    /// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]). Returns `true` when
    /// the size actually changed so the caller can reshape the grid.
    pub fn zoom_in(&mut self) -> bool {
        self.set_font_size_pt(self.runtime_font_size_pt + FONT_SIZE_PT_STEP)
    }

    /// Decrease the runtime terminal font size by one point (clamped).
    /// Returns `true` when the size actually changed.
    pub fn zoom_out(&mut self) -> bool {
        self.set_font_size_pt(self.runtime_font_size_pt - FONT_SIZE_PT_STEP)
    }

    /// Reset the runtime terminal font size back to the configured
    /// `settings.font_size`. Returns `true` when the size actually
    /// changed.
    pub fn zoom_reset(&mut self) -> bool {
        self.set_font_size_pt(self.settings.font_size)
    }

    /// Set the runtime terminal font size to `new_pt` (clamped to
    /// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]). On a real change:
    /// re-derive `cell_w_logical` / `cell_h_logical` from the font stack
    /// at the new pixel size, push the new point size into every tab's
    /// `Theme` (leaving the rest of each theme's OSC-mutated state
    /// intact), force a full redraw, and return `true`. Returns `false`
    /// (no mutation) when the clamped target equals the current size.
    ///
    /// The PTY grid is *not* reshaped here — the caller (`window_host`)
    /// owns the window pixel size and triggers a deferred resize so the
    /// new cell metrics produce the right `(cols, rows)` on the next
    /// frame.
    pub fn set_font_size_pt(&mut self, new_pt: f32) -> bool {
        let clamped = clamp_font_size_pt(new_pt);
        if (clamped - self.runtime_font_size_pt).abs() < f32::EPSILON {
            return false;
        }
        self.runtime_font_size_pt = clamped;
        // Re-derive cell metrics at the new size. `font_size_px` applies
        // the same 96/72 pt→px conversion `settings.font_size_px()` uses
        // at startup so the grid stays consistent with the WebView build.
        let new_px = clamped * crate::settings::PT_TO_PX;
        let (cell_w, cell_h) = crate::render::compute_cell_dims(
            self.font_rasterizer.as_ref(),
            self.font_fallback.as_ref(),
            new_px,
        );
        self.cell_w_logical = cell_w;
        self.cell_h_logical = cell_h;
        // Push the new point size into every tab's theme. Only
        // `font_size_pt` is touched so OSC-driven palette / cursor
        // mutations a tab accumulated are preserved.
        for tab in &self.tabs {
            tab.theme.lock().font_size_pt = clamped;
        }
        self.needs_full_redraw = true;
        true
    }

    /// Apply a settings draft committed by the in-app settings panel to
    /// the running app. Re-derives every startup-resolved state that
    /// the panel's categories can affect; settings that only bind at
    /// tab spawn time (`shell_path` / `shell_args` /
    /// `scrollback_lines`) intentionally reach new tabs only, matching
    /// the WebView build.
    ///
    /// Returns `true` when the caller (`window_host`) must reshape the
    /// window grid (cell metrics or padding changed).
    pub fn apply_settings(&mut self, mut new: Settings) -> bool {
        crate::settings_store::clamp_for_save(&mut new);
        let old = Arc::clone(&self.settings);

        // The profile selector / new-tab chooser renders its rows live
        // from `self.settings.profiles` and its highlight index is bound
        // to that list. A settings save (from the external WebView
        // settings window) can add / remove / reorder profiles while the
        // modal is open, leaving `selected` pointing past the new list or
        // at a different profile than the highlighted row. Close it so the
        // user never confirms against a stale list (the WebView rebuilt
        // the list on every open; closing is the equivalent invariant).
        if self.profile_selector.visible {
            self.profile_selector.close();
            self.needs_full_redraw = true;
        }

        // UI chrome palette: preset × brightness swaps live (the md3
        // slot is process-wide, so the next frame re-skins every
        // widget).
        crate::ui::md3::set_preset(new.ui_theme_preset, new.ui_theme);
        // Keybinds / locale resolve the same way as startup.
        self.keybinds = crate::ui::keybinds::KeybindTable::from_settings(&new.keybinds);
        self.locale = crate::i18n::resolve(new.language);

        // mux: rebuild the prefix latch (chord + action bindings) from the
        // new settings (FR11 dynamic apply). The tab group always renders its
        // windows as sub-tabs (WebView parity), so there is no expand
        // preference to push onto tabs.
        self.mux_latch = build_mux_latch(&new);

        let font_families_changed = new.font_family_fallback != old.font_family_fallback;
        let font_size_changed = (new.font_size - old.font_size).abs() >= f32::EPSILON;
        let padding_changed = new.padding != old.padding;

        self.settings = Arc::new(new);

        // Reflect the (possibly changed) SFTP concurrency cap onto the live
        // pool so reload takes effect without restarting in-flight uploads.
        self.sftp_service
            .set_max_concurrent(self.settings.sftp_max_concurrent_uploads);

        if font_families_changed {
            let (resolver, fallback, cache, rasterizer, base_id) =
                Self::build_font_stack(&self.settings);
            self.font_resolver = resolver;
            self.font_fallback = fallback;
            self.font_cache = cache;
            self.font_rasterizer = rasterizer;
            self.font_base_id = base_id;
        }

        // Re-derive cell metrics. A font_size change routes through
        // `set_font_size_pt` (which also pushes the size into every tab
        // theme); a pure family swap keeps the size but the new chain's
        // advance may differ, so recompute the dims in place.
        if font_size_changed {
            self.set_font_size_pt(self.settings.font_size);
        } else if font_families_changed {
            let px = self.runtime_font_size_pt * crate::settings::PT_TO_PX;
            let (w, h) = crate::render::compute_cell_dims(
                self.font_rasterizer.as_ref(),
                self.font_fallback.as_ref(),
                px,
            );
            self.cell_w_logical = w;
            self.cell_h_logical = h;
        }

        // Rebuild every tab's theme from the new settings (color scheme
        // / cursor style / bold-brighten), preserving the live zoom
        // level. OSC-driven palette mutations a tab accumulated are
        // reset — same outcome as the WebView's
        // `applyTerminalColorScheme` full remap.
        for tab in &mut self.tabs {
            let mut theme = crate::render::theme::Theme::from_settings(self.settings.as_ref());
            theme.font_size_pt = self.runtime_font_size_pt;
            {
                // FR5 (cursor-settings-fix task0004 AC-2/AC-3): an active
                // OSC 12 cursor-color override survives this rebuild.
                // `scheme_cursor_fg` above already reflects the NEW
                // settings' scheme (so a later OSC 112 restores THAT
                // color); only `cursor_fg` + the override flag carry
                // forward from the old theme.
                let old_theme = tab.theme.lock();
                if old_theme.cursor_fg_override_active {
                    theme.cursor_fg = old_theme.cursor_fg;
                    theme.cursor_fg_override_active = true;
                }
            }
            *tab.theme.lock() = theme;
            {
                let mut core = tab.core.lock();
                core.set_cursor_blink(self.settings.cursor_blink);
                core.set_cursor_style(self.settings.cursor_style.as_cursor_shape_u8());
                core.mark_all_dirty();
            }
            tab.set_fold_enabled(self.settings.fold_enabled);
        }

        self.needs_full_redraw = true;
        font_size_changed || font_families_changed || padding_changed
    }
}
