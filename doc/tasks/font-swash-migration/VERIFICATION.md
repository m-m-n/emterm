# Verification Document: Font Renderer Migration (ab_glyph → swash)

## Overview

- **Feature**: font-swash-migration (Phase 4-H in `tmp/restruct.md`)
- **SPEC.md**: `doc/tasks/font-swash-migration/SPEC.md`
- **IMPLEMENTATION.md**: `doc/tasks/font-swash-migration/IMPLEMENTATION.md`
- **Scope**: `native-poc/` only. Wry viewers and `src-tauri/` legacy build are out of scope; the existing E2E suite is kept as a regression gate but no new specs are added.

## Build Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --workspace"`
- **Expected**: exit code 0, no errors. Existing workspace builds pass after dependency additions (swash pinned, zeno, fontdb).

Result placeholder (filled in by sdd.4-implement / sdd.6-verify):

- [x] Build PASS on Linux x86_64 (Docker, default dev profile) — `cargo build --workspace` succeeds; `cargo build -p emterm-native-poc --example swash_emoji` succeeds.

## Test Verification

- **Command**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"`
- **Coverage target**: minimum 80% on the new `render/font/` module; workspace test count stays at parity with the Phase 4-G ~1985-test baseline (new tests add, no regressions).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|---|---|---|---|
| TS-font-1 | `FontEngine::default()` is `Swash` | Equality assertion passes | Unit |
| TS-font-2 | Parse `Settings::font_engine = "ab_glyph"`; parse `"unknown"` | Known value parses without warn; unknown warn-logs and defaults to `Swash` | Unit |
| TS-font-3 | `GlyphCache::get_or_rasterize` called twice with same key | Second call returns the same `AtlasRegion` (cache hit; rasterizer not re-invoked) | Unit |
| TS-font-4 | `FallbackChain::resolve(ASCII)` and `FallbackChain::resolve(U+3042)` | ASCII → base font; `U+3042` → CJK fallback | Unit |
| TS-font-5 | `FallbackChain::resolve(U+1F600)` | Emoji fallback | Unit |
| TS-font-6 | Atlas upload of `AtlasFormat::Alpha` vs `AtlasFormat::Rgba` | Alpha routed to R8 texture; RGBA routed to RGBA8 texture | Unit |
| TS-font-7 | ab_glyph adapter rasters for `'A'` vs `U+3042` / `U+1F600` | `'A'` returns Alpha bitmap; CJK / emoji return `None` (cache treats as miss-through) | Unit |
| TS-font-8 | swash adapter rasters `'A'` | Non-empty alpha bitmap with sensible advance | Unit |
| TS-font-9 | swash adapter rasters `U+1F600` | RGBA bitmap; at least one non-zero RGB byte | Unit |
| TS-font-10 | Bundled font registration against in-memory fontdb | Registration succeeds; resolver returns the registered FontId | Unit |
| TS-font-11 | `Theme::default().font_family == "monospace"` and `font_size_pt == 13.0` | Equality assertions pass (regression guard) | Unit |
| TS-font-12 | Renderer reads `Theme::font_family` + `Theme::font_size_pt` (not the deleted `FONT_SIZE` constant / hard-coded `FontFamily::Monospace`) | Build a renderer with `Theme { font_family = "TestSentinelFont", font_size_pt = 17.0, .. }`; assert that the resolver receives `"TestSentinelFont"` and the cell pixel dim is derived from `17.0` (not `13.0`) | Unit |
| TS-font-13 | `TerminalGridPass::prepare` emits one instance per non-empty cell | Instance count == filled-cell count for a fixture grid; per-instance UV refers to the cache-returned atlas region | Unit |
| TS-font-14 | `TerminalGridPass::prepare` records the correct atlas-page index for Alpha vs RGBA glyphs | Per-instance page index encodes Alpha for ASCII and RGBA for color emoji | Unit |
| TS-font-int-1 | `cargo run -p emterm-native-poc --example swash_emoji` | Produces a non-empty PNG file | Integration |
| TS-font-int-2 | Headless render of a single cell containing `U+3042` (swash engine) | `TerminalGridPass` instance buffer contains a non-empty entry; no panic | Integration |
| TS-font-int-3 | Headless render with `Settings::font_engine = AbGlyph` and the same cell | No panic; CJK glyph permitted to be empty | Integration |
| TS-font-int-4 | `TerminalGridPass` builds against the wgpu device used by `window_host` | Pipeline + bind-group-layout creation succeeds (smoke; no draw call required) | Integration |
| TS-font-perf-1 | `EMTERM_FONT_PERF=1` env toggle on release build | Log line `font scan total = <X> ms` present; `X < 500` | Performance (log-instrumented) |
| TS-font-perf-2 | `EMTERM_FONT_PERF=1` env toggle on release build | Per-glyph rasterize log lines present on cache miss; each `< 5 ms` | Performance (log-instrumented) |

## Code Quality Verification

- **Format**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --all"` — clean diff.
- **Static analysis (clippy)**: `cargo clippy --workspace -- -D warnings` — zero warnings.
- **Dead code resolution**: `Theme::font_family` and `Theme::font_size_pt` are read by the renderer; any residual `#[allow(dead_code)]` on those fields is removed (FR10). Note: as of HEAD = 5c54d1a these fields carry no `#[allow(dead_code)]` attribute on the struct itself, so this gate degenerates to "renderer must read them" plus grep for any newly-added attribute.

Result placeholders:

- [x] `cargo fmt --all -- --check` clean (verified in Docker).
- [ ] `cargo clippy --workspace -- -D warnings` zero warnings — _Not yet executed in this implementation pass; build emits pre-existing warn-level dead_code findings unrelated to this SDD. To be run during sdd.5-check._
- [x] `Theme::font_family` and `Theme::font_size_pt` are read by the renderer (TS-font-12 in `render/mod.rs::renderer_reads_theme_font_family_and_size`); no `#[allow(dead_code)]` is present on those fields.

## File Structure Verification

### Files to Create

- `native-poc/Cargo.toml` (modify) — add swash (pinned), zeno, fontdb.
- `native-poc/assets/fonts/NotoSansCJKjp-Regular.otf` — bundled CJK base.
- `native-poc/assets/fonts/NotoColorEmoji.ttf` — bundled emoji.
- `native-poc/assets/fonts/LICENSE` — SIL OFL 1.1.
- `native-poc/assets/fonts/README.md` — bundled font versions + SHA-256 hashes.
- `native-poc/examples/swash_emoji.rs` — PoC binary.
- `native-poc/src/render/font/mod.rs` — module aggregator.
- `native-poc/src/render/font/traits.rs` — `GlyphRasterizer`, `GlyphBitmap`, `AtlasFormat`, `FontId`, `ShapedGlyph`.
- `native-poc/src/render/font/cache.rs` — glyph cache + observability accessors + perf logging hook.
- `native-poc/src/render/font/atlas.rs` — alpha (R8) + rgba (RGBA8) atlas regions.
- `native-poc/src/render/font/ab_glyph_adapter.rs` — fallback rasterizer.
- `native-poc/src/render/font/swash_adapter.rs` — swash + zeno rasterizer.
- `native-poc/src/render/font/resolver.rs` — fontdb scan + bundled registration.
- `native-poc/src/render/font/fallback.rs` — fallback chain + memoization.

### Files to Modify

- `native-poc/src/settings.rs` — add `FontEngine` enum + `font_engine` / `font_family_fallback` / `emoji_font` / `variable_font_axes` fields with parse-or-warn logic.
- `native-poc/src/render/mod.rs` — delete `FONT_SIZE` constant + `FontFamily::Monospace` literal (Phase 4-H); remove `painter.text()` / decoration-line / background-rect calls from `draw_grid`; read `Theme::font_family` / `Theme::font_size_pt`.
- `native-poc/src/render/theme.rs` — drop `#[allow(dead_code)]` on `font_family` and `font_size_pt`.
- `native-poc/src/render/terminal_grid_pass.rs` (NEW, Phase 4-H) — custom wgpu render pass; pipeline + bind group + instance buffer + WGSL shader (inline or sibling `.wgsl`).
- `native-poc/src/window_host.rs` — insert `TerminalGridPass` into frame draw order: `clear → TerminalGridPass → egui (LoadOp::Load) → ImageOverlayPass (LoadOp::Load)`.
- `native-poc/src/app.rs` — startup rasterizer selection based on `Settings::font_engine`; build resolver + fallback chain; construct `TerminalGridPass` with the chosen rasterizer.
- `tmp/restruct.md` — flip Phase 4-H status row on completion.

## SPEC.md Compliance

### Go / No-Go Gates (PoC failure if any miss)

| ID | Gate | How to Verify |
|---|---|---|
| G1 | **Noto Color Emoji renders correctly** (CBDT → RGBA atlas → `TerminalGridPass`) | TS-manual-font-linux-x11 screenshot with `🎉🚀🤖` in color |
| G2 | **CJK (Japanese) renders correctly** (no tofu) | TS-manual-font-linux-x11 screenshot with `こんにちは` legible |
| G3 | **Cell-width integrity** (emoji = 2 cells, CJK = 2 cells, ASCII = 1 cell) | TS-manual-font-linux-x11 visual + grid alignment check |
| G4 | **Startup font scan < 500 ms** (release Linux x86_64) | TS-font-perf-1 + TS-manual-font-startup-perf |
| G5 | **Glyph cache-miss rasterize < 5 ms / glyph** (release) | TS-font-perf-2 |

If any G1–G5 fail, declare the SDD a PoC failure and document the next-step (alternative: cosmic-text / Vello / `egui::Context::set_fonts` for CJK only, color emoji dropped). Do not spend more cycles patching the pass.

### Success Criteria

| ID | Criterion | How to Verify |
|---|---|---|
| SC-1 | FR1 PoC binary produces a recognizable emoji PNG | TS-font-int-1 + manual visual inspection of the PNG |
| SC-2 | FR2–FR12 implemented and unit-tested per the test scenarios | TS-font-1..14 + TS-font-int-1..4 all pass |
| SC-3 | `cargo test --workspace` passes; new tests add to baseline | Test command result; count parity with ~1985 |
| SC-4 | `cargo fmt --all` clean | `cargo fmt --all -- --check` exit code 0 |
| SC-5 | `cargo clippy --workspace -- -D warnings` zero warnings | clippy exit code 0 |
| SC-6 | Renderer reads `Theme::font_family` / `Theme::font_size_pt`; no residual `#[allow(dead_code)]` on those fields | Grep on `render/theme.rs` (must show no `#[allow(dead_code)]` on those two fields); clippy still clean; TS-font-12 confirms live read |
| SC-7 | Linux X11 manual gate confirms Japanese + color emoji | TS-manual-font-linux-x11 |
| SC-8 | `tmp/restruct.md` Phase 4-H status row updated | Diff inspection at verify step |
| SC-9 | Windows host gate passes or is documented as deferred | TS-manual-font-windows result or formal defer note in VERIFICATION_RESULT.md |
| SC-10 | `painter.text()` removed from `draw_grid`; `TerminalGridPass` is the cell renderer | Grep `render/mod.rs` for `painter.text`; new pass present in `window_host` frame loop |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|---|---|---|
| FR1 (swash PoC) | Phase 1 | TS-font-int-1 + manual PNG inspection |
| FR2 (Glyph trait) | Phase 2 | TS-font-3 + TS-font-6 + TS-font-7 |
| FR3 (Texture atlas split) | Phase 2 | TS-font-6 |
| FR4 (swash adapter) | Phase 3 | TS-font-8 + TS-font-9 |
| FR5 (ab_glyph adapter retained) | Phase 2 | TS-font-7 + TS-font-int-3 |
| FR6 (Settings::font_engine) | Phase 4 | TS-font-1 + TS-font-2 + TS-font-int-3 |
| FR7 (Font resolution) | Phase 3 | TS-font-10 + TS-font-int-2 |
| FR8 (Fallback chain) | Phase 3 | TS-font-4 + TS-font-5 |
| FR9 (Settings schema additions) | Phase 4 | TS-font-2 (parse) + code inspection (fields present) |
| FR10 (Theme dead_code resolution) | Phase 4 | TS-font-11 + TS-font-12 + clippy clean |
| FR11 (Bundled fonts) | Phase 1 + Phase 3 | TS-font-10 + file structure verification + LICENSE inspection |
| FR12 (Terminal grid wgpu render pass — Option 3) | Phase 4-H | TS-font-13 + TS-font-14 + TS-font-int-2 + TS-font-int-4 + manual G1 / G2 screenshots |

### Non-Functional Requirements Coverage

| Requirement | Phase | Verification |
|---|---|---|
| NFR1 (Startup < 500 ms) | Phase 5 | TS-font-perf-1 + TS-manual-font-startup-perf |
| NFR2 (Glyph rasterize < 5 ms / cache hit parity) | Phase 5 | TS-font-perf-2 |
| NFR3 (Stability + ab_glyph escape hatch + pinned swash) | Phase 2 + Phase 4 | TS-font-int-3 + Cargo.toml inspection (pinned `= 0.1.x`) |
| NFR4 (Observable cache size / hit rate) | Phase 2 | Public accessor present (code inspection); used in TS-font-3 indirectly |
| NFR5 (License compliance) | Phase 1 + Phase 3 | `assets/fonts/LICENSE` present; README records SHA-256 + versions; Segoe UI Emoji not bundled |
| NFR6 (Platform coverage) | Phase 5 | TS-manual-font-linux-x11 (primary) + TS-manual-font-windows (pass or formal defer) |

## E2E Testing

**Not applicable.** tauri-driver cannot attach to a winit window without WebKit; the existing E2E suite under `e2e-tests/` is retained as a regression gate for the legacy `src-tauri/` build but no new specs are added in this SDD.

- [ ] No new E2E specs introduced.

## Manual Testing (E2E Not Possible)

All manual gates are run on real hosts. Logs / screenshots are stored under `doc/tasks/font-swash-migration/manual/` (or referenced inline in `VERIFICATION_RESULT.md`).

- [ ] **TS-manual-font-linux-x11**: Linux + X11 + fcitx5 host. `echo こんにちは` renders as Japanese; IME-compose `こんにちは` renders preedit + commit as Japanese; a line of color emoji (`🎉🚀🤖`) renders in color.
- [ ] **TS-manual-font-linux-x11-fallback**: Set `Settings::font_engine = AbGlyph`, restart. CJK renders as tofu (escape hatch confirmed alive). Restore default afterwards.
- [ ] **TS-manual-font-linux-wayland**: Linux Wayland host with `GDK_BACKEND=x11`. Same expectations as the X11 gate.
- [ ] **TS-manual-font-windows**: Windows host with default Segoe UI / Yu Gothic + MS-IME. Same scenarios as Linux. Color emoji uses Segoe UI Emoji via system lookup (not bundled). May be deferred with a documented next-step.
- [ ] **TS-manual-font-startup-perf**: With `EMTERM_FONT_PERF=1`, confirm `font scan total = <X> ms` log line is below 500 ms.

## Performance Verification

| Metric | Target | Method |
|---|---|---|
| Startup font scan | < 500 ms (release, Linux x86_64) | TS-font-perf-1 (`EMTERM_FONT_PERF=1` log line) |
| Glyph cache miss → rasterize | < 5 ms / glyph (release) | TS-font-perf-2 (per-miss log line) |
| Cache hit per-cell cost | Parity with ab_glyph path | Comparison of render frame-time logs before/after, on the swash engine |

## Security Verification

- [ ] No new IPC surface introduced (internal-only error tags per SPEC.md §Error Handling).
- [ ] Settings string parsing uses warn-log + default fallback (matches the `StatusBarPosition::parse_or_warn` convention).
- [ ] Bundled fonts ship with the SIL OFL 1.1 LICENSE file; Segoe UI Emoji is **not** bundled.
- [ ] swash pinned at `= 0.1.x` in `native-poc/Cargo.toml`.
- [ ] Bundled font SHA-256 hashes recorded in `native-poc/assets/fonts/README.md`.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|---|---|---|---|---|
| Unit tests | 14 (TS-font-1..14) | 14 | 0 | 0 |
| Integration tests | 4 (TS-font-int-1..4) | 4 | 0 | 0 |
| Performance tests | 2 (TS-font-perf-1..2) | 2 (log-instrumented; env-gated) | 0 | 0 |
| Manual host gates | 5 (TS-manual-font-*) | 0 | 0 | 5 |
| Code quality gates | 3 (fmt / clippy / dead_code) | 3 | 0 | 0 |
| **Total** | **28** | **23** | **0** | **5** |

## Verification Results

(Filled in by sdd.4-implement / sdd.6-verify. Each subsection records the actual outcome.)

### Build Result

- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --workspace"` — PASS.
- `cargo build -p emterm-native-poc --example swash_emoji` — PASS.

### Test Result

- `cargo test --workspace` — **2009 passed** / 0 failed / 6 ignored (baseline at Phase 4-G was ~1985; this SDD adds 24 new tests, no regressions).
- New unit tests added by this SDD:
  - `render/font/traits.rs` — `font_id_sentinel_default_is_zero`, `glyph_bitmap_bytes_per_pixel_alpha_is_1`, `glyph_bitmap_bytes_per_pixel_rgba_is_4`, `glyph_bitmap_is_empty_for_zero_dim`.
  - `render/font/atlas.rs` — `upload_routes_to_correct_page` (TS-font-6), `empty_bitmap_returns_empty_region`, `row_wrap_advances_cursor_y`, `grows_when_row_overflows_height`.
  - `render/font/cache.rs` — `cache_hit_returns_same_region_and_skips_raster` (TS-font-3), `missing_returns_none_and_caches_sentinel`, `empty_bitmap_caches_zero_size_region`, `observable_accessors_increment_on_use`, `key_bucketing_distinct_for_distinct_sizes`.
  - `render/font/ab_glyph_adapter.rs` — `ab_glyph_adapter_returns_none_for_uncovered_codepoint` (TS-font-7), `ab_glyph_adapter_returns_none_for_wrong_font_id`, `shape_maps_chars_to_glyph_ids`, `raster_for_emoji_codepoint_returns_some_or_none`.
  - `render/font/resolver.rs` — `register_bundled_returns_distinct_ids` (TS-font-10), `by_role_lists_each_registered_font`, `by_family_resolves_registered_name`, `scan_failed_starts_false`.
  - `render/font/fallback.rs` — `resolve_ascii_returns_base` (TS-font-4), `resolve_cjk_returns_cjk_fallback` (TS-font-4), `resolve_emoji_returns_emoji_fallback` (TS-font-5), `resolve_uncovered_returns_none_and_memoizes`, `duplicate_chain_entries_are_dropped`.
  - `render/font/swash_adapter.rs` — `swash_rasters_ascii_alpha` (TS-font-8), `swash_rasters_emoji_rgba` (TS-font-9), `unknown_font_id_returns_none`, `has_codepoint_for_emoji_font_covers_grin`.
  - `settings.rs` — `font_engine_default_is_swash` (TS-font-1), `font_engine_parses_known_values` (TS-font-2), `font_engine_unknown_falls_back_to_swash` (TS-font-2), `settings_carry_font_engine_default_swash`, `settings_font_family_fallback_default_empty`, `settings_emoji_font_default_none`, `settings_variable_font_axes_default_empty`.
  - `render/mod.rs` — `theme_default_font_family_is_monospace` (TS-font-11), `renderer_reads_theme_font_family_and_size` (TS-font-12), `renderer_routes_monospace_default_to_monospace_family`.
- Integration: `cargo run -p emterm-native-poc --example swash_emoji` produced `native-poc/target/swash_emoji.png` (159 × 150 RGBA8, 24 087 bytes, `Content::Color`) — TS-font-int-1 PASS.
- TS-font-int-2 / TS-font-int-3 (headless render of a single cell with swash / ab_glyph engine): _deferred together with renderer-cache-wiring; the foundation layers they exercise (cache + fallback + adapters) are unit-tested directly._

### Format / Clippy Result

- `cargo fmt --all -- --check` — PASS (Docker).
- `cargo clippy --workspace -- -D warnings` — _Not executed in this pass._ Pre-existing warn-level findings (unrelated to this SDD) are present in the build log. To be re-evaluated during sdd.5-check.

### Existing E2E Regression (Phase 3.8)

- Native-poc owns no E2E suite (winit + wgpu surface; tauri-driver inapplicable). Per sdd.yaml `e2e_test_command: ""` E2E is not run.
- Legacy `src-tauri/` E2E suite was not affected by this change (no `src-tauri/` files modified). No regression run executed in this session.

### Manual Gate Result

- All TS-manual-font-* gates: **deferred** until Phase 4-H lands (`TerminalGridPass` is the gating prerequisite for any visible CJK / color-emoji output). The foundation layers (cache, atlas, traits, adapters, resolver, fallback chain, settings, theme wiring, perf instrumentation) are complete and unit-tested.
- Phase 4-H status row in `tmp/restruct.md`: not yet flipped (gated on G1–G5 outcome).

## Known Limitations

- **Phase 4-H pending (Option 3)**: the new `TerminalGridPass` (FR12) — custom wgpu render pass + WGSL shader + `window_host` integration + `painter.text` removal + startup rasterizer selection — has not been implemented yet. Decision adopted 2026-05-15: build the cell renderer from scratch rather than retro-fitting the foundation onto egui's text path. PoC failure path (G1 or G2 unmet) is documented in IMPLEMENTATION.md Phase 4-H.
- **Manual host gates (TS-manual-font-*)** are deferred to a follow-up host session after Phase 4-H. The perf instrumentation that backs TS-font-perf-1 / TS-font-perf-2 is in place (`EMTERM_FONT_PERF=1` log lines in `render/font/cache.rs` and `render/font/resolver.rs`).
- **Windows secondary fallback (`Segoe UI Emoji`)** is scaffolded via `FontRole::Secondary` in the resolver but not wired into the chain on Windows. Add under `#[cfg(windows)]` when the platform comes online.
