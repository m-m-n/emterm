# Verification Document: COLRv1 Vector Emoji Rendering

## Overview

**Feature**: colrv1-emoji-vector-rendering
**SPEC.md**: `doc/tasks/colrv1-emoji-vector-rendering/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/colrv1-emoji-vector-rendering/IMPLEMENTATION.md`

This document is owned by sdd.2-create-plan. Subsequent SDD steps Edit
the result sections (build / test / format / E2E outcomes) only.

## Build Verification

### GUI build (default features)

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0; no warnings introduced by the change set.

### CLI-only build (gates that the change does not pull GUI-only crates into the CLI)

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0. `skrifa`, `tiny-skia`, and
  `render::font::colrv1_painter` must not be reachable from this
  configuration.

### Release build (user-initiated; required for binary-size scenario)

- Command (user-run only — agent never invokes this without explicit
  instruction):
  `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0; binary at
  `src-tauri/target-host/release/emterm` exists and is executable.

### Result (filled in by sdd.4-implement / sdd.6-verify)

- GUI check: PASS — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` exited 0 (no warnings introduced).
- CLI check: PASS — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` exited 0. `skrifa` and `tiny-skia` stay gated behind the `gui` feature (verified by inspecting the resolved feature set; the new `render::font::colrv1_painter` module is reachable only when `render` is included, and `render` is itself behind `feature = "gui"` via `lib.rs`).
- Release build: NOT RUN by sdd.4 (user-initiated only per VERIFICATION plan).

## Test Verification

### Command

- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`

### Coverage Target

- New module `render::font::colrv1_painter`: > 80 % statement coverage
  via the 16 unit tests + 3 integration tests below (informal —
  coverage is not measured by CI today).
- No regression in the existing 12 tests under
  `render::font::swash_adapter::tests`.

### Test Scenarios

| ID    | Scenario                                                                                | Expected Result                                                                                          | Test Type    |
|-------|-----------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------|--------------|
| TS-1  | Premultiplied `(0,0,0,0)` round-trips through `un_premultiply`                          | Output equals `(0,0,0,0)`                                                                                | Unit         |
| TS-2  | Premultiplied `(255,128,64,255)` (alpha = 255) round-trips                              | Output equals `(255,128,64,255)`                                                                         | Unit         |
| TS-3  | Premultiplied `(64,32,16,128)` un-premultiplies                                         | Output equals `(127,63,31,128)` within ±1 per channel                                                    | Unit         |
| TS-4  | `is_colrv1_emoji(BUNDLED_EMOJI_COLOR_FONT)` (now Noto-COLRv1)                           | Returns `true`                                                                                           | Unit         |
| TS-5  | `is_colrv1_emoji(BUNDLED_EMOJI_MONO_FONT)`                                              | Returns `false`                                                                                          | Unit         |
| TS-6  | `is_colrv1_emoji` on a CBDT fixture (e.g. a stashed copy of the old CBDT bytes)         | Returns `false`. Skipped with `eprintln!` if no fixture available — record the skip in the result section | Unit         |
| TS-7  | `rasterize(Noto-COLRv1 bytes, gid(U+1F600), 26.0, 0.0)`                                 | Returns `Some(_)` with `pixels.len() == width * height * 4` and at least one non-zero RGB byte           | Unit         |
| TS-8  | `rasterize(Noto-COLRv1 bytes, gid(U+1F680), 26.0, 0.0)`                                 | Returns `Some(_)` with non-empty RGBA                                                                    | Unit         |
| TS-9  | `rasterize(Noto-COLRv1 bytes, gid(U+2764), 26.0, 0.0)`                                  | Returns `Some(_)` with non-empty RGBA                                                                    | Unit         |
| TS-10 | `rasterize(Noto-COLRv1 bytes, gid(U+1F30D), 26.0, 0.0)`                                 | Returns `Some(_)` with non-empty RGBA                                                                    | Unit         |
| TS-11 | `rasterize(_, glyph_id=0, _, _)` rejects the notdef sentinel                            | Returns `None`                                                                                           | Unit         |
| TS-12 | `rasterize(_, _, size_px=0.0, _)`                                                       | Returns `None`                                                                                           | Unit         |
| TS-13 | `rasterize(_, _, size_px=-1.0, _)`                                                      | Returns `None`                                                                                           | Unit         |
| TS-14 | `rasterize(smiley, size_px, 0.0)` at size_px ∈ {17.0, 21.0, 26.0, 35.0} (legacy fallback) | Each returns `Some(_)` with `width == ceil(size_px)` (square Pixmap), non-empty pixels                   | Unit         |
| TS-14b | `rasterize(smiley, 17.33, 19.0)` (cell-h padding mode, FR8)                            | Returns `Some(_)` with `width == height == 19`, `advance == 19.0`, `bearing_top ∈ [1, 18]` (baseline inside inner padded area) | Unit |
| TS-14c | `rasterize(smiley, 3.0, 3.0)` (tiny-dim no-padding, FR8)                               | Returns `Some(_)` with `width == height == 3`, `bearing_top == 3`, `advance == 3.0` (padding skipped when `dim < 4`) | Unit |
| TS-15 | Register Noto-COLRv1 in `SwashRasterizer`, call `raster(smiley)`                        | Returns `Some(GlyphBitmap { format: AtlasFormat::Rgba, … })` with non-zero RGB; routing log shows COLRv1 hit | Integration |
| TS-16 | Register CJK + Noto-COLRv1; rasterize `'A'` from CJK at 32 px                           | Output unchanged from current main (`AtlasFormat::Alpha`, non-empty, advance > 0)                        | Integration  |
| TS-17 | `raster(font=Noto-COLRv1, gid=cmap(U+E000))` for a codepoint not in the COLRv1 cmap     | Returns `None` (cache stores Slot::Missing; in production FallbackChain descends to NotoEmoji-Regular)   | Integration  |
| TS-18 | `cargo check --no-default-features` on `src-tauri/`                                     | Exit code 0                                                                                              | Build        |
| TS-19 | `cargo fmt --check` (or `cargo fmt --manifest-path src-tauri/Cargo.toml`)                | No diff for `colrv1_painter.rs` / `swash_adapter.rs` / `resolver.rs` / `mod.rs`                          | Format       |
| TS-20 | `ls -l src-tauri/target-host/release/emterm` before / after the change                  | New binary is approximately 5 MiB smaller (CBDT 10.7 MiB → COLRv1 4.99 MiB ≈ 5.7 MiB delta)              | Manual       |
| TS-21 | Windows 1.5× DPI: run `echo 😀🚀❤️🌍👍🏽` in emterm                                          | Each glyph visually matches the C-variant reference under `tmp/verify-emoji/out/compare3_*_26px.png` (edges sharp, no blur) | Manual       |
| TS-22 | Linux 1.0× DPI: same input                                                              | No regression versus current main                                                                        | Manual       |
| TS-23 | Windows RDP 1.0× scaling: same input                                                    | No regression versus current main                                                                        | Manual       |
| TS-24 | `git grep -n 'unsafe' src-tauri/src/render/font/colrv1_painter.rs` and the diff hunks   | Zero matches in `colrv1_painter.rs`; no new `unsafe` block in the modified `swash_adapter.rs` branch     | Manual       |

### Result (filled in by sdd.4-implement / sdd.6-verify)

Run command (per session, with `--test-threads=1` per the MEMORY note that
`tabs.rs` replay tests are non-deterministic in parallel):

```
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path src-tauri/Cargo.toml \
  --lib render::font -- --test-threads=1
```

- Unit (TS-1 .. TS-14, TS-14b, TS-14c): PASS — all 16 colrv1_painter tests green on first run. TS-14b (`rasterize_target_cell_h_pads_and_centers`) and TS-14c (`rasterize_tiny_dim_skips_padding`) cover the FR8 cell-h sizing + padding logic.
- Integration (TS-15 .. TS-17): PASS — three new tests in `swash_adapter::tests` green; existing 12 swash_adapter tests still green (including `swash_rasters_emoji_rgba`, retitled to reflect COLRv1 routing).
- Build (TS-18): PASS — `cargo check --no-default-features` exits 0.
- Format (TS-19): PASS — `cargo fmt --check src-tauri/src/render/font/{colrv1_painter,swash_adapter,resolver,mod}.rs` exits 0 (no diff).
- Manual (TS-20 .. TS-24): see Manual Testing section below.

Full `render::font` slice: 91 passed, 0 failed, 0 ignored.

Known limitation: outside the touched modules, `cargo test --lib` produces 5 pre-existing flaky failures in `tabs::tests` (`ts7`, `ts9`, `ts10`, `ts13` off-thread replay timeouts; `welcome_without_windows_leaves_group_none`). These failures reproduce with no diff in `tabs.rs` or its dependencies and are recorded in MEMORY (`feedback_tdd_scope` / project test notes) as known non-determinism. They do not exercise any code touched by this change set.

## Code Quality Verification

### Format

- Command: `cargo fmt --manifest-path src-tauri/Cargo.toml`
- Expected: no diff on the modified files
  (`colrv1_painter.rs`, `swash_adapter.rs`, `resolver.rs`, `mod.rs`).

### Static checks

- `cargo check` (GUI + CLI) — exit 0.
- (No project-wide clippy gate today; not adding one here.)
- Manual grep for new `unsafe` and `unwrap()` on font / paint-graph
  paths (TS-24).

### Result

- Format check on touched files: PASS (TS-19, command above).
- Static checks: PASS — both `cargo check` invocations exit 0.
- Manual `unsafe` audit (TS-24):
  - `grep -n "unsafe" src-tauri/src/render/font/colrv1_painter.rs` returns zero (only the file-header comment "`unsafe` is not used anywhere in this module (NFR3)" matches; no `unsafe { ... }` block).
  - `git diff src-tauri/src/render/font/swash_adapter.rs | grep '^+' | grep -i unsafe` returns nothing — no new `unsafe` in the COLRv1 branch.

## File Structure Verification

### Files to Create

- `src-tauri/src/render/font/colrv1_painter.rs` — skrifa-driven
  ColorPainter + tiny-skia rasterizer + premultiply helper + 14 unit
  tests.
- `src-tauri/assets/fonts/Noto-COLRv1.ttf` — bundled COLRv1 emoji font
  (fetched via `scripts/fetch-fonts.sh`; gitignored).

### Files to Modify

- `src-tauri/Cargo.toml` — `[dependencies]` adds optional
  `skrifa = "0.20"`, `tiny-skia = "0.11"`; `[features].gui` appends both.
- `src-tauri/build.rs` — failsafe bundled-font list points at
  `Noto-COLRv1.ttf` (mirror of the `include_bytes!` path).
- `src-tauri/src/render/font/mod.rs` — `+pub mod colrv1_painter;`.
- `src-tauri/src/render/font/traits.rs` — `GlyphRasterizer` gains
  `fn set_base_font(&self, _font: FontId) {}` (default no-op so
  `ab_glyph_adapter` keeps compiling without an override).
- `src-tauri/src/render/font/resolver.rs` — `include_bytes!` path
  changed from `NotoColorEmoji.ttf` to `Noto-COLRv1.ttf`; doc-comment
  refreshed from "CBDT / COLR" to "COLRv1 + glyf".
- `src-tauri/src/render/font/swash_adapter.rs` — `SwashFont.is_colrv1_emoji`
  field, populated in `ingest_font`; `Inner.base_font: Option<FontId>`;
  `SwashRasterizer::set_base_font` override; `raster()` divert branch
  resolves `(base_ascent_px, base_cell_h_px)`, calls
  `colrv1_painter::rasterize` with the 4-arg signature, overrides
  `bearing.1` with `base_ascent_px`; logs; the existing
  `swash_rasters_emoji_rgba` test is re-commented to reflect the new
  routing.
- `src-tauri/src/app.rs` — `build_font_stack` calls
  `rasterizer.set_base_font(base_id)` immediately before returning
  the constructed stack.
- `scripts/fetch-fonts.sh` — `NotoColorEmoji.ttf` `fetch_one` block
  removed; `Noto-COLRv1.ttf` block added (URL = `https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/Noto-COLRv1.ttf`,
  SHA256 = `0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2`).
- `src-tauri/assets/fonts/README.md` — inventory row swapped.

### Files to Delete (local FS only; gitignored)

- `src-tauri/assets/fonts/NotoColorEmoji.ttf`

### Result

Files Created:
- [x] `src-tauri/src/render/font/colrv1_painter.rs` (skrifa-driven painter + 14 unit tests).
- [x] `src-tauri/assets/fonts/Noto-COLRv1.ttf` (4,991,984 bytes; SHA256 `0ae57fe5…1e1a28d2` — matches pin).

Files Modified:
- [x] `src-tauri/Cargo.toml` — added `skrifa = "0.20"` and `tiny-skia = "0.11"` to `[dependencies]` (both `optional = true`); appended `dep:skrifa` and `dep:tiny-skia` to `[features].gui`.
- [x] `src-tauri/build.rs` — failsafe `check_bundled_fonts` list points at `Noto-COLRv1.ttf` (mirror of the new `include_bytes!` path).
- [x] `src-tauri/src/render/font/mod.rs` — `+pub mod colrv1_painter;`.
- [x] `src-tauri/src/render/font/traits.rs` — `GlyphRasterizer` gains a default-no-op `fn set_base_font(&self, _font: FontId) {}` method.
- [x] `src-tauri/src/render/font/resolver.rs` — `BUNDLED_EMOJI_COLOR_FONT` `include_bytes!` points at `Noto-COLRv1.ttf`; doc-comment refreshed.
- [x] `src-tauri/src/render/font/swash_adapter.rs` — `SwashFont.is_colrv1_emoji` field; `Inner.base_font: Option<FontId>`; `SwashRasterizer::set_base_font` override; `ingest_font` populates via `colrv1_painter::is_colrv1_emoji`; `raster` resolves base-font ascent + cell_h, drops the lock, dispatches to `colrv1_painter::rasterize` with the 4-arg signature, and overrides `bearing.1` with `base_ascent_px`; `has_color` OR's the flag; three new integration tests; existing `swash_rasters_emoji_rgba` re-commented.
- [x] `src-tauri/src/app.rs` — `build_font_stack` calls `rasterizer.set_base_font(base_id)` once, right before returning the constructed stack.
- [x] `scripts/fetch-fonts.sh` — CBDT entry removed; pinned Noto-COLRv1 v2.051 entry added.
- [x] `src-tauri/assets/fonts/README.md` — inventory row swapped.

Files Deleted (local FS only; gitignored):
- [x] `src-tauri/assets/fonts/NotoColorEmoji.ttf` — removed during Phase 1 fetch.

## SPEC.md Compliance

### Success Criteria

| ID   | Criterion                                                                          | How to Verify                                                                |
|------|------------------------------------------------------------------------------------|------------------------------------------------------------------------------|
| SC-1 | All FR1–FR8 implemented                                                            | Cross-check the requirements coverage table below                            |
| SC-2 | All listed unit + integration tests pass                                           | `cargo test --lib` exit 0; TS-1 .. TS-17 results (including TS-14b / TS-14c) |
| SC-3 | Manual scenarios 1–4 (Windows 1.5× sharp, no Linux / RDP regression, ~5 MiB delta) | TS-20 .. TS-23 results                                                       |
| SC-4 | `cargo check --no-default-features` still passes                                   | TS-18                                                                        |
| SC-5 | `cargo test --lib` passes                                                          | Test command exit 0                                                          |
| SC-6 | `bun run typecheck` passes (sanity; no TS change expected)                         | `bun run typecheck` exit 0                                                   |
| SC-7 | No new `unsafe`; no `unwrap()` on font / paint-graph operations                    | TS-24                                                                        |

### Functional Requirements Coverage

| Requirement | Phase            | Verification                                                                              |
|-------------|------------------|-------------------------------------------------------------------------------------------|
| FR1         | Phase 2          | TS-7, TS-8, TS-9, TS-10, TS-14, TS-15 (rasterize returns non-empty RGBA across the bundled emoji set) |
| FR2         | Phase 2, Phase 3 | TS-4, TS-5, TS-6 (probe correctness); TS-15, TS-16 (routing dispatches correctly)         |
| FR3         | Phase 2          | TS-1, TS-2, TS-3 (premultiply arithmetic)                                                 |
| FR4         | Phase 1          | TS-15 (bundled bytes resolve as Noto-COLRv1); TS-20 (binary size delta)                   |
| FR5         | Phase 1          | TS-20 (binary built from SHA256-pinned font); rerun of `fetch-fonts.sh` reports up-to-date |
| FR6         | Phase 3          | TS-17 (raster returns None on uncovered codepoint; FallbackChain descends in production)  |
| FR7         | Phase 3          | Manual log inspection during TS-15 / TS-17 (info on fallback, debug on hit); TS-21 reproduces both paths. `warn_once` events for sweep gradient / radial `r0 > 0` / unsupported composite / Pixmap-or-Mask alloc failure are visible in the same emterm.log |
| FR8         | Phase 2, Phase 3 | TS-14 (legacy `target_cell_h_px = 0` fallback), TS-14b (cell-h padding + centering), TS-14c (tiny-dim no-padding); TS-15 verifies the end-to-end integration (raster routes through the cell-h sized pixmap when called via `swash_adapter::raster`) |
| NFR1        | Phase 4          | TS-21 (informal — first-rasterize latency observed at human-perception scale during the manual scenario) |
| NFR2        | Phase 1          | TS-20                                                                                     |
| NFR3        | All phases       | TS-24                                                                                     |
| NFR4        | Phase 1          | TS-20 (re-running `fetch-fonts.sh` on a clean dir produces a SHA256-matching file)        |
| NFR5        | Phases 1, 2      | TS-1 .. TS-14 (unit tests); pinned `skrifa = "0.20"` / `tiny-skia = "0.11"` in Cargo.toml |

## E2E Testing

Not applicable. The project ships no E2E framework today; the SPEC
explicitly records "Run command: Not detected". Manual scenarios cover
the on-screen verification this feature needs.

### Existing E2E Regression (Phase 3.8) — sdd.4 result

- Executed command: none. `sdd.yaml.project.components.main.e2e_test_command` is empty; no `e2e-tests/README.md`, no `docker-compose.e2e.yml`, no `scripts/*e2e*` helpers. Skipped per the implementation-executor's detection ladder.

## Manual Testing (E2E Not Possible)

- [ ] **TS-20**: Capture `ls -l src-tauri/target-host/release/emterm`
  before the change (baseline) and after (target). Record byte sizes
  and the delta in the result section. Expectation: delta ≈ 5 MiB
  reduction. **Not run by sdd.4**: release build is user-initiated only;
  the bundled-font delta (CBDT 10,673,480 B → Noto-COLRv1 4,991,984 B
  ≈ 5.42 MiB smaller asset) is confirmed at the asset level.
- [ ] **TS-21**: On Windows 1.5× DPI, run `echo 😀🚀❤️🌍👍🏽` in emterm.
  Compare the rendered cells to `tmp/verify-emoji/out/compare3_*_26px.png`
  (C variant). Pass criterion: edges visibly sharp; no blur; no color
  muddying. **Pending sdd.6 (user has Windows hardware).**
- [ ] **TS-22**: On Linux 1.0× DPI, run the same command and confirm no
  visible regression versus current main. **Pending sdd.6.**
- [ ] **TS-23**: On Windows via RDP at 1.0× scaling, run the same
  command and confirm no visible regression versus current main.
  **Pending sdd.6.**
- [x] **TS-24**: `grep -n 'unsafe' src-tauri/src/render/font/colrv1_painter.rs`
  returns only the doc-comment "`unsafe` is not used anywhere in this
  module (NFR3)"; no `unsafe { ... }` block exists.
  `git diff src-tauri/src/render/font/swash_adapter.rs | grep '^+' | grep -i unsafe`
  returns nothing — no new `unsafe` block in the COLRv1 dispatch branch.

## Performance Verification

- **NFR1 (first-time rasterize < 10 ms per glyph on Windows reference
  hardware)**: not gated by an automated test. Verified informally
  during TS-21 — if the first emoji draw causes a perceptible stall
  on the manual scenario, treat as a regression and re-investigate.
- Steady-state per-frame cost: unchanged by construction (GlyphCache
  is reused; only cache misses enter the new path).

## Security Verification

- [ ] **No new `unsafe`** — covered by TS-24.
- [ ] **No runtime ingestion of arbitrary fonts** — `colrv1_painter`
  only sees the bundled bytes resolved through `BUNDLED_EMOJI_COLOR_FONT`
  (and, optionally, user-dir overrides resolved through the existing
  `user_dir` scan which already validates byte length and skips
  symlinks). Verified by reading the call sites — the diff must not
  add any new file-read path inside `colrv1_painter`.
- [ ] **SHA256 pinning honored** — TS-20 (a clean fetch round-trip
  matches the pinned SHA256).

## Verification Summary

| Category    | Items | Automated | E2E | Manual |
|-------------|-------|-----------|-----|--------|
| Unit        | 16    | 16        | 0   | 0      |
| Integration | 3     | 3         | 0   | 0      |
| Build       | 1     | 1         | 0   | 0      |
| Format      | 1     | 1         | 0   | 0      |
| Manual      | 5     | 0         | 0   | 5      |
| **Total**   | 26    | 21        | 0   | 5      |
