# Verification Result: Font Renderer Migration (ab_glyph → swash)

**Verified at**: 2026-05-15 (initial) / 2026-05-15 (post-migration update)
**Verified at commit**: c6e64580f1efb726ccdcc8f2f231fec9ed471b3e (initial) / current HEAD (post-migration)
**SDD**: `doc/tasks/font-swash-migration/`
**Scope**: `native-poc/` only (Wry viewers / `src-tauri/` legacy out of scope).

## Summary

| Category | Status | Notes |
|---|---|---|
| File structure | PASS | All 16 created files + 6 modified files present. |
| FR1–FR11 (automated) | PASS | All implemented + unit-tested. |
| FR12 (Phase 4-H Option 3) | PASS (8/8 tasks) | `TerminalGridPass` + WGSL + pipeline + integration + live cell-input feed + `painter.text()` removal all done. Manual host gate still pending. |
| NFR1–NFR2 (perf) | INSTRUMENTED | `EMTERM_FONT_PERF=1` log lines in `cache.rs` + `resolver.rs`. Actual host measurement deferred. |
| NFR3 (stability + pin + escape hatch) | PASS | `swash = "=0.1.18"` in `Cargo.toml:77`; `ab_glyph_adapter.rs` + `FontEngine::AbGlyph` live. |
| NFR4 (observable cache) | PASS | `CacheStats { hits, misses, missing }` updated in `cache.rs:115/129/158`. |
| NFR5 (license) | PASS | `assets/fonts/LICENSE` (Noto + Adobe SIL OFL); `README.md` records SHA-256 column. |
| NFR6 (platform) | DEFERRED | Linux X11 + Wayland + Windows host gates deferred. |
| Go/No-Go gates G1–G5 | PENDING | All 5 require manual host session. Instrumentation/scaffolding ready, and the chicken-and-egg deferral (G1+G2 blocked by painter.text() still drawing cells) is now dissolved. |
| Build / Test / Format / Clippy | PASS (sdd.5-check) | 2019/2019 PASS, fmt clean, clippy clean for Phase 4-H. Not re-run here. |

**Overall: Automated verification PASS. All Phase 4-H FR12 tasks are now complete (8/8). Manual host gates (G1–G5) plus `restruct-status-update` remain the only pending items, all formally tracked as `deferred` in `tasks.yaml`.**

## Post-Migration Update (2026-05-15)

This section records the second verify pass after the two previously-deferred Phase 4-H cut-over tasks landed.

### Tasks now completed (were `deferred` in initial verify)

- **`migrate-draw-grid-to-new-pass`** → `status: completed`
  - `render::collect_cell_inputs(core, theme, selection, width_mode) -> Vec<CellInput>` added at `native-poc/src/render/mod.rs:180`. Snapshots the live `term_core` grid into the input format `TerminalGridPass::prepare` consumes, preserving selection fg/bg swap via the existing `resolve_cell_style` path and skipping the trailing half of wide cells.
  - `WindowHost::render` (`native-poc/src/window_host.rs:584`) calls `crate::render::collect_cell_inputs(...)` per frame and passes the result to `TerminalGridPass::prepare` together with real `CellMetrics` (no more placeholder / empty list).
  - Cell metrics (`CELL_W`, `CELL_H`, `LEFT_PAD`, `TOP_PAD`) became `pub const` on `render` so `window_host` composes the grid origin with the empirical 36 px tab-bar offset.

- **`remove-painter-text-from-draw-grid`** → `status: completed`
  - `fn draw_grid`, the `cell_font_id` helper, the `FONT_SIZE` constant, and the `FontFamily::Monospace` literal are all removed from `native-poc/src/render/mod.rs`. Grep confirms zero hits for `painter.text(`, `fn draw_grid`, `FONT_SIZE`, or `FontFamily::Monospace` outside doc comments.
  - The `CentralPanel::frame` fill is now `Color32::TRANSPARENT` so the wgpu-rendered cells from `TerminalGridPass` are visible underneath the egui overlay layer.
  - **Kept on egui** (intentional, per SPEC): the text cursor + IME preedit overlays, and the selection rectangle `painter.rect_filled` (selection is also visible via the fg/bg swap in `resolve_cell_style` that `collect_cell_inputs` propagates to the pass).

### Test count delta

`2017 → 2019 PASS` (-2 retired + 4 new):

- Retired: TS-font-12 (`cell_font_id` assertion — the helper no longer exists), and the `draw_grid_skips_wide_trailing_half` integration assertion that referenced the old painter.text() codepath.
- New unit tests for `collect_cell_inputs` in `render/mod.rs`:
  - `collect_cell_inputs_emits_one_entry_per_cell`
  - `collect_cell_inputs_handles_wide_cells`
  - `collect_cell_inputs_propagates_decoration_flags`
  - `collect_cell_inputs_draw_background_only_when_non_default`

### Status flips on this verify pass

- **SC-2**: PASS → PASS (unchanged, but FR12 is now fully implemented; TS-font-13 / TS-font-14 plus the four new `collect_cell_inputs` tests all green).
- **SC-10**: **PARTIAL → PASS** — `painter.text()` is removed from `render/mod.rs`; `TerminalGridPass` is the sole cell renderer. egui retains only the cursor + IME preedit + selection rectangle, all of which are explicitly out-of-scope for `TerminalGridPass` per SPEC.
- **FR12**: 6/8 → 8/8 tasks complete (excluding the manual host gate, which is the user's responsibility).
- **Chicken-and-egg deferral**: dissolved. Earlier the G1+G2 manual gate could not pass because `painter.text()` was still the only glyph source and the new pass was fed an empty cell list; the host gate the user just ran confirmed CJK tofu + missing emoji from that exact path. With this migration landed, G1+G2 are now physically achievable on the user's next host gate run.

### Cleanup follow-ups recorded (non-blocking)

- `Theme::font_family`: re-marked `#[allow(dead_code)]` with a `// TODO: wire into Resolver` note. The renderer no longer reads the field directly because font selection is now driven by the `FallbackChain` handed to `TerminalGridPass` at startup via `App::build_font_stack`. Wiring `Theme::font_family` into the resolver as a "primary preferred family" override is a future polish item.
- `CellStyle::bold` and `CellStyle::italic`: marked `#[allow(dead_code)]` for the same reason. The current `TerminalGridPass` shader pulls a single weight/style per fallback step; supporting bold/italic variants requires either separate atlases per face or a runtime variation-axis tweak, both deferred.
- `Theme::font_size_pt`: still live (read via `CellMetrics::font_size_px` in `WindowHost::render`), no `#[allow(dead_code)]` regression.
- **HiDPI scaling**: deferred. Today everything runs at 1.0× on the operator's host. `CellMetrics` carries a `scale: f32` field that is currently always 1.0; multi-DPI sweeps are a separate manual gate.
- **Tab-bar offset**: the 36 px constant in `WindowHost::render` is empirical (matches the current egui `TabBar` height). When the tab bar gains dynamic height it must move onto a measured layout query — currently noted as a TODO inline.

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
| `native-poc/src/render/mod.rs` | `pub mod terminal_grid_pass;` (line 32). **`painter.text()` removed** (post-migration); `pub fn collect_cell_inputs` added at line 180; cell metrics promoted to `pub const`. |
| `native-poc/src/render/theme.rs` | `font_size_pt` is live (no `#[allow(dead_code)]`); `font_family` re-marked `#[allow(dead_code)]` with a Resolver-wiring TODO (cleanup item, not a regression). |
| `native-poc/src/window_host.rs` | `use ...TerminalGridPass` (line 45); `grid_pass: Option<TerminalGridPass>` (line 141); `ensure_grid_pass` (line 263); frame order `clear → TerminalGridPass → egui (Load) → ImageOverlayPass (Load)`. **Live cell-input feed**: `collect_cell_inputs(...)` called at line 584 and handed to `TerminalGridPass::prepare` at line 612 with real `CellMetrics` (no placeholder). |
| `native-poc/src/app.rs` | `build_font_stack` (line 180) branches on `settings.font_engine`: `FontEngine::Swash` → `SwashRasterizer`; `FontEngine::AbGlyph` → wraps bundled CJK bytes; parse failure falls back to Swash with warn-log. |
| `tmp/restruct.md` | NOT updated (`restruct-status-update` task deferred until after manual host gates). |

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
| FR10 (Theme dead_code resolution) | PASS | `theme.rs::font_size_pt` carries no `#[allow(dead_code)]` (live via `CellMetrics::font_size_px`); `font_family` re-marked with a Resolver-wiring TODO (cleanup item — see Post-Migration Update). | TS-font-11 |
| FR11 (Bundled fonts) | PASS | `assets/fonts/` carries both fonts + LICENSE + SHA-256-tagged README. Windows Segoe UI Emoji scaffolded via `FontRole::Secondary` (deferred `#[cfg(windows)]` wiring). | TS-font-10 + file inspection |
| FR12 (TerminalGridPass — Option 3) | **PASS (8/8 tasks)** | `render/terminal_grid_pass.rs` + `.wgsl` (pipeline + bind group + instance buffer + prepare/draw); `render/mod.rs::collect_cell_inputs` snapshots the live grid; `window_host.rs` feeds the pass per frame with real cell metrics and `painter.text()` is gone. `app.rs:180` selects rasterizer at startup. | TS-font-13, TS-font-14, TS-font-int-2, TS-font-int-4, plus the four new `collect_cell_inputs_*` unit tests. |

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

The previous chicken-and-egg deferral (G1+G2 blocked because `painter.text()` was still the only glyph source) is now dissolved — `TerminalGridPass` is the sole cell renderer, fed from the live grid via `collect_cell_inputs`.

| Gate | Description | Status |
|---|---|---|
| G1 | Noto Color Emoji renders in color via `TerminalGridPass` RGBA atlas | PENDING (TS-manual-font-linux-x11) |
| G2 | Japanese CJK legible (no tofu) | PENDING (TS-manual-font-linux-x11) |
| G3 | Cell-width integrity (emoji = 2, CJK = 2, ASCII = 1) | PENDING (TS-manual-font-linux-x11) |
| G4 | Startup font scan < 500 ms (release Linux x86_64) | PENDING (TS-manual-font-startup-perf) |
| G5 | Glyph cache-miss rasterize < 5 ms / glyph (release) | PENDING (TS-font-perf-2 host run) |

## Success Criteria SC-1..10

| ID | Status | Notes |
|---|---|---|
| SC-1 | PASS | TS-font-int-1: `examples/swash_emoji.rs` produced 159×150 RGBA8 PNG (24 087 bytes). |
| SC-2 | PASS | TS-font-1..14 (minus retired TS-font-12) + TS-font-int-1..4 + the four new `collect_cell_inputs_*` tests all in test list (sdd.5-check 2019/2019 PASS). |
| SC-3 | PASS | 2019 tests passing (post-migration delta: -2 retired + 4 new vs prior 2017). |
| SC-4 | PASS | `cargo fmt --all -- --check` clean in sdd.5-check. |
| SC-5 | PASS | `cargo clippy --workspace -- -D warnings` clean for Phase 4-H in sdd.5-check (pre-existing `term_core` lints out of scope). |
| SC-6 | PASS | `theme.rs::font_size_pt` carries no `#[allow(dead_code)]`. `font_family` is re-marked with a Resolver-wiring TODO (cleanup item, not a regression — the renderer now drives font selection through `FallbackChain` instead). |
| SC-7 | PENDING | TS-manual-font-linux-x11 deferred to host gate session. |
| SC-8 | DEFERRED | `tmp/restruct.md` Phase 4-H status row update tracked as `restruct-status-update` (runs after host gates). |
| SC-9 | DEFERRED | TS-manual-font-windows formally deferred per `tasks.yaml` (no Windows host available this session). |
| SC-10 | **PASS** | `painter.text()` is removed from `render/mod.rs`; `TerminalGridPass` is the sole cell renderer. egui retains only cursor + IME preedit + selection rectangle, all explicitly out-of-scope for `TerminalGridPass` per SPEC. |

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
7. **`restruct-status-update`** (SC-8) — flip Phase 4-H row in `tmp/restruct.md` to ✅ after host gates pass.
8. **`windows-segoe-emoji-secondary`** — `#[cfg(windows)]` wiring of `FontRole::Secondary` (scaffold present).

> Items previously listed as #7/#8 (`migrate-draw-grid-to-new-pass` / `remove-painter-text-from-draw-grid`) are now **completed** — see the Post-Migration Update section above. They are removed from this outstanding list.

## Conclusion

Automated verification of the font-swash-migration SDD is **PASS**, including the Phase 4-H Option 3 cut-over.

The full pipeline is now in place end-to-end: the Glyph trait, two-page atlas, observable glyph cache, swash + ab_glyph rasterizers, fontdb resolver, fallback chain, `Settings::font_engine`, perf instrumentation, the custom wgpu `TerminalGridPass`, **and** the live cell-input feed from `term_core` into the pass with `painter.text()` retired. The only items still pending are the manual host gates (G1–G5 / SC-7 / SC-8 / SC-9 / NFR6), all of which require a real Linux X11 + fcitx5 / Wayland / Windows host session by the operator. The chicken-and-egg blocker — that `painter.text()` was still drawing cells while the new pass was fed an empty list — is dissolved as of this verify pass.
