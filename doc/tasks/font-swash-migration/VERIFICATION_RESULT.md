# Verification Result: Font Renderer Migration (ab_glyph → swash)

**Verified at**: 2026-05-15
**Verified at commit**: c6e64580f1efb726ccdcc8f2f231fec9ed471b3e
**SDD**: `doc/tasks/font-swash-migration/`
**Scope**: `native-poc/` only (Wry viewers / `src-tauri/` legacy out of scope).

## Summary

| Category | Status | Notes |
|---|---|---|
| File structure | PASS | All 16 created files + 6 modified files present. |
| FR1–FR11 (automated) | PASS | All implemented + unit-tested (32 new tests). |
| FR12 (Phase 4-H Option 3) | PASS (6/8 tasks; 2 deferred) | `TerminalGridPass` + WGSL + pipeline + integration done; final 2 cut-over tasks deferred behind G1+G2 manual gate. |
| NFR1–NFR2 (perf) | INSTRUMENTED | `EMTERM_FONT_PERF=1` log lines in `cache.rs` + `resolver.rs`. Actual host measurement deferred. |
| NFR3 (stability + pin + escape hatch) | PASS | `swash = "=0.1.18"` in `Cargo.toml:77`; `ab_glyph_adapter.rs` + `FontEngine::AbGlyph` live. |
| NFR4 (observable cache) | PASS | `CacheStats { hits, misses, missing }` updated in `cache.rs:115/129/158`. |
| NFR5 (license) | PASS | `assets/fonts/LICENSE` (Noto + Adobe SIL OFL); `README.md` records SHA-256 column. |
| NFR6 (platform) | DEFERRED | Linux X11 + Wayland + Windows host gates deferred. |
| Go/No-Go gates G1–G5 | PENDING | All 5 require manual host session. Instrumentation/scaffolding ready. |
| Build / Test / Format / Clippy | PASS (sdd.5-check) | 2017/2017 PASS, fmt clean, clippy clean for new code. Not re-run here. |

**Overall: Automated verification PASS. Manual host gates (G1–G5) and the two cut-over Phase 4-H tasks (migrate-draw-grid-to-new-pass / remove-painter-text-from-draw-grid) remain pending and are formally tracked as deferred in `tasks.yaml`.**

## File Structure Verification

### Files to Create (all present)

| Path | Status |
|---|---|
| `native-poc/Cargo.toml` (swash pinned `= 0.1.18`, zeno, fontdb, png dev-dep) | OK (line 77) |
| `native-poc/assets/fonts/NotoSansCJKjp-Regular.otf` | OK |
| `native-poc/assets/fonts/NotoColorEmoji.ttf` | OK |
| `native-poc/assets/fonts/LICENSE` | OK |
| `native-poc/assets/fonts/README.md` (SHA-256 column) | OK (line 9, 16) |
| `native-poc/examples/swash_emoji.rs` | OK |
| `native-poc/src/render/font/mod.rs` | OK |
| `native-poc/src/render/font/traits.rs` | OK |
| `native-poc/src/render/font/cache.rs` | OK |
| `native-poc/src/render/font/atlas.rs` | OK |
| `native-poc/src/render/font/ab_glyph_adapter.rs` | OK |
| `native-poc/src/render/font/swash_adapter.rs` | OK |
| `native-poc/src/render/font/resolver.rs` | OK |
| `native-poc/src/render/font/fallback.rs` | OK |
| `native-poc/src/render/terminal_grid_pass.rs` | OK |
| `native-poc/src/render/terminal_grid_pass.wgsl` | OK |

### Files to Modify (all confirmed)

| Path | Evidence |
|---|---|
| `native-poc/src/settings.rs` | `FontEngine` enum (line 110), `font_engine` / `font_family_fallback` / `emoji_font` / `variable_font_axes` fields (lines 204/209/214/219). |
| `native-poc/src/render/mod.rs` | `pub mod terminal_grid_pass;` (line 32). `painter.text()` still at line 223 — see SC-10. |
| `native-poc/src/render/theme.rs` | No `#[allow(dead_code)]` on `font_family` / `font_size_pt` (lines 45–46). |
| `native-poc/src/window_host.rs` | `use ...TerminalGridPass` (line 45); `grid_pass: Option<TerminalGridPass>` (line 141); `ensure_grid_pass` (line 263); frame order `clear → TerminalGridPass → egui (Load) → ImageOverlayPass (Load)` (line 575). |
| `native-poc/src/app.rs` | `build_font_stack` (line 180) branches on `settings.font_engine`: `FontEngine::Swash` → `SwashRasterizer`; `FontEngine::AbGlyph` → wraps bundled CJK bytes; parse failure falls back to Swash with warn-log. |
| `tmp/restruct.md` | NOT updated (restruct-status-update task deferred until after host gates). |

## SPEC.md Functional Requirements

| ID | Status | Implementation | Test |
|---|---|---|---|
| FR1 (swash PoC) | PASS | `native-poc/examples/swash_emoji.rs` | TS-font-int-1 (produced 24 087-byte PNG) |
| FR2 (Glyph trait) | PASS | `render/font/traits.rs` (`GlyphRasterizer`, `GlyphBitmap`, `AtlasFormat`) | TS-font-3, TS-font-6, TS-font-7 |
| FR3 (Atlas split) | PASS | `render/font/atlas.rs` (Alpha R8 + RGBA8 pages) | TS-font-6 |
| FR4 (swash adapter) | PASS | `render/font/swash_adapter.rs` (Shaper + Render + color-table routing) | TS-font-8, TS-font-9 |
| FR5 (ab_glyph adapter) | PASS | `render/font/ab_glyph_adapter.rs` (returns None for CJK/emoji) | TS-font-7, TS-font-int-3 |
| FR6 (Settings::font_engine) | PASS | `settings.rs:110` `FontEngine` enum, `:204` field, `:127` warn-on-unknown | TS-font-1, TS-font-2 |
| FR7 (Font resolution) | PASS | `render/font/resolver.rs` (fontdb scan + bundled registration + warn-log) | TS-font-10 |
| FR8 (Fallback chain) | PASS | `render/font/fallback.rs` (chain walk + memoization) | TS-font-4, TS-font-5 |
| FR9 (Settings schema additions) | PASS | `font_family_fallback` / `emoji_font` / `variable_font_axes` live on `Settings` | TS-font-2 + code inspection |
| FR10 (Theme dead_code resolution) | PASS | `render/theme.rs:45-46` no `#[allow(dead_code)]` on the two fields; `render/mod.rs::renderer_reads_theme_font_family_and_size` confirms live read | TS-font-11, TS-font-12 |
| FR11 (Bundled fonts) | PASS | `assets/fonts/` carries both fonts + LICENSE + SHA-256-tagged README. Windows Segoe UI Emoji scaffolded via `FontRole::Secondary` (deferred `#[cfg(windows)]` wiring). | TS-font-10 + file inspection |
| FR12 (TerminalGridPass — Option 3) | PASS (6/8 tasks) | `render/terminal_grid_pass.rs` + `.wgsl` (pipeline + bind group + instance buffer + prepare/draw); `window_host.rs:575` integrates the new frame draw order; `app.rs:180` selects rasterizer at startup. **Deferred (gated on G1+G2)**: `migrate-draw-grid-to-new-pass` + `remove-painter-text-from-draw-grid` per `tasks.yaml` notes. | TS-font-13, TS-font-14, TS-font-int-2, TS-font-int-4 |

## NFR Coverage

| ID | Status | Evidence |
|---|---|---|
| NFR1 (Startup < 500 ms) | INSTRUMENTED, host gate PENDING | `resolver.rs:116` reads `EMTERM_FONT_PERF`; `:128` emits `font scan total = {} ms`. Measurement deferred to TS-manual-font-startup-perf. |
| NFR2 (Glyph rasterize < 5 ms) | INSTRUMENTED, host gate PENDING | `cache.rs:87` env gate; `:143` emits `glyph rasterize: ... elapsed_us=`. |
| NFR3 (Stability + ab_glyph + pinned swash) | PASS | `Cargo.toml:77` `swash = "=0.1.18"`; `ab_glyph_adapter.rs` present; `FontEngine::AbGlyph` selectable via `Settings`. |
| NFR4 (Observable cache) | PASS | `CacheStats { hits, misses, missing }` updated at `cache.rs:115/129/158`; public accessors exposed. |
| NFR5 (License compliance) | PASS | `assets/fonts/LICENSE` (Noto + Adobe SIL OFL 1.1); `README.md:9` SHA-256 column; Segoe UI Emoji not bundled. |
| NFR6 (Platform coverage — Linux primary, Windows deferred) | DEFERRED | Linux X11 / Wayland / Windows manual gates not yet run (`tasks.yaml` phase-5). |

## Go/No-Go Gates (G1–G5)

All five gates require visual / perf measurement on a real host. **All PENDING.**

| Gate | Description | Status |
|---|---|---|
| G1 | Noto Color Emoji renders in color via `TerminalGridPass` RGBA atlas | PENDING (TS-manual-font-linux-x11) |
| G2 | Japanese CJK legible (no tofu) | PENDING (TS-manual-font-linux-x11) |
| G3 | Cell-width integrity (emoji = 2, CJK = 2, ASCII = 1) | PENDING (TS-manual-font-linux-x11) |
| G4 | Startup font scan < 500 ms (release Linux x86_64) | PENDING (TS-manual-font-startup-perf) |
| G5 | Glyph cache-miss rasterize < 5 ms / glyph (release) | PENDING (TS-font-perf-2 host run) |

Note: G1+G2 also gate the two deferred Phase 4-H tasks (`migrate-draw-grid-to-new-pass`, `remove-painter-text-from-draw-grid`) per the PoC failure-path policy.

## Success Criteria SC-1..10

| ID | Status | Notes |
|---|---|---|
| SC-1 | PASS | TS-font-int-1: `examples/swash_emoji.rs` produced 159×150 RGBA8 PNG (24 087 bytes). |
| SC-2 | PASS | TS-font-1..14 + TS-font-int-1..4 all in test list (sdd.5-check 2017/2017 PASS). |
| SC-3 | PASS | 2017 tests passing (+32 vs ~1985 baseline, 0 regressions). |
| SC-4 | PASS | `cargo fmt --all -- --check` clean in sdd.5-check. |
| SC-5 | PASS | `cargo clippy --workspace -- -D warnings` clean for new code in sdd.5-check (pre-existing `term_core` lints out of scope). |
| SC-6 | PASS | `theme.rs:45-46` carries no `#[allow(dead_code)]`; `render/mod.rs::renderer_reads_theme_font_family_and_size` confirms live read. |
| SC-7 | PENDING | TS-manual-font-linux-x11 deferred to host gate session. |
| SC-8 | DEFERRED | `tmp/restruct.md` Phase 4-H status row update tracked as `restruct-status-update` (runs after host gates). |
| SC-9 | DEFERRED | TS-manual-font-windows formally deferred per `tasks.yaml` (no Windows host available this session). |
| SC-10 | PARTIAL | `TerminalGridPass` is present, wired into `window_host` frame loop (line 575), and has its own clear. `painter.text()` removal from `render/mod.rs::draw_grid:223` is the deferred `remove-painter-text-from-draw-grid` task, gated on G1+G2. Today the new pass is fed an empty cell list; the painter.text() path continues to draw cells until manual gates clear. |

## E2E

Not applicable. native-poc uses winit + wgpu (no WebKit / tauri-driver). Project component config sets `e2e_test_command: ""`. No new E2E specs were introduced. The legacy `e2e-tests/` suite remains as the regression gate for the `src-tauri/` build, which this SDD did not touch.

## Outstanding / Manual Items

Tracked in `tasks.yaml` as `status: deferred`:

1. **TS-manual-font-linux-x11** (G1, G2, G3) — primary host gate. Linux X11 + fcitx5, screenshots under `doc/tasks/font-swash-migration/manual/`.
2. **TS-manual-font-linux-x11-fallback** — `font_engine = AbGlyph` escape hatch confirmed alive (CJK tofu acceptable).
3. **TS-manual-font-linux-wayland** — `GDK_BACKEND=x11` smoke.
4. **TS-manual-font-windows** — Segoe UI Emoji via system lookup. Formally defer-able with documented next-step.
5. **TS-manual-font-startup-perf** (G4) — confirm `font scan total < 500 ms`.
6. **G5 host perf run** — confirm per-glyph rasterize < 5 ms on cache miss.
7. **`migrate-draw-grid-to-new-pass`** — feed `TerminalGridPass::build_instances` from the live `term_core` grid. Gated on G1+G2.
8. **`remove-painter-text-from-draw-grid`** — delete `painter.text()` at `render/mod.rs:223` + `FONT_SIZE` constant + `FontFamily::Monospace` literal. Runs immediately after item 7.
9. **`restruct-status-update`** (SC-8) — flip Phase 4-H row in `tmp/restruct.md` to ✅.
10. **`windows-segoe-emoji-secondary`** — `#[cfg(windows)]` wiring of `FontRole::Secondary` (scaffold present).

## Conclusion

Automated verification of the font-swash-migration SDD is **PASS**. The foundation (Glyph trait, two-page atlas, glyph cache, swash + ab_glyph adapters, fontdb resolver, fallback chain, Settings::font_engine, Theme dead_code resolution, perf instrumentation) is complete and unit-tested with 32 new tests on top of the Phase 4-G baseline. The Phase 4-H custom wgpu pass (`TerminalGridPass`) is implemented and integrated into the `window_host` frame loop; the final cut-over (`painter.text()` removal + driving the new pass from the live grid) is intentionally deferred behind the G1+G2 manual host gates per the PoC failure-path policy. The five Go/No-Go gates (G1–G5) and SC-7, SC-8, SC-9, SC-10 (final substep), NFR6 are all pending a manual host session.
