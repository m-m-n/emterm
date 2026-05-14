# Feature: Font Renderer Migration (ab_glyph → swash)

## Overview

Replace the native-poc terminal grid's font rasterizer with `swash` + `zeno`, gaining proper CJK shaping and color-emoji rendering (COLR v0/v1, CBDT/CBLC, SVG-in-OT) while keeping `ab_glyph` as a fall-back path behind a `Settings::font_engine` flag. The migration is scoped to `native-poc/` (terminal grid only); Wry viewers stay on their own WebKit font stack.

## Objectives

- Make Japanese and other CJK scripts render correctly in the native terminal grid (currently shown as `U+FFFD` boxes after Phase 4-G).
- Render color emoji (Noto Color Emoji on Linux, Segoe UI Emoji on Windows) as bitmap/COLR glyphs in their full color form.
- Abstract the glyph cache so future font work (variable fonts, ligatures) does not require touching every renderer call site.
- Keep `ab_glyph` reachable as an opt-in escape hatch until Phase 7 (legacy Tauri retirement).
- Remove the `#[allow(dead_code)]` annotations on `Theme::font_family` / `Theme::font_size_pt` by wiring them through the renderer.

## User Stories

### US1: Read Japanese terminal output

As a Japanese-speaking user, I want `echo こんにちは` to render as readable Japanese (not tofu boxes), so that I can use the native terminal as my daily driver.

**Acceptance Criteria:**
- [ ] Mixed ASCII + hiragana + kanji lines render with no `U+FFFD` glyphs.
- [ ] Each CJK glyph occupies 2 cells with no pixel bleed into adjacent cells.

### US2: Compose Japanese via IME

As a Japanese-speaking user, I want IME preedit and commit text to render with the same fonts as plain output, so that my composition feedback is legible.

**Acceptance Criteria:**
- [ ] `WindowEvent::Ime(Preedit(...))` text renders with CJK glyphs at the cursor area.
- [ ] After commit, the PTY echo back is rendered identically.

### US3: See color emoji

As a developer reading Claude Code / git output containing emoji, I want emoji to render in full color, so that my CLI output stays readable.

**Acceptance Criteria:**
- [ ] Color emoji render with their bitmap/COLR colors (not monochrome outlines).
- [ ] Multiple emoji per line do not overlap or clobber neighbors.

### US4: Fall back to ab_glyph

As a developer hitting a swash bug, I want to switch the font engine to `ab_glyph` via settings, so that I can recover terminal usability without rebuilding.

**Acceptance Criteria:**
- [ ] Setting `font_engine: AbGlyph` (settings.json or `Settings::default()` patch) routes glyph requests through the legacy adapter.
- [ ] The fallback path keeps compiling and passing the existing test suite.

### US5: Use a custom base font

As any user with a preferred monospace font installed, I want the renderer to honor `font_family`, so that the base ASCII font matches my taste while CJK/emoji fall back automatically.

**Acceptance Criteria:**
- [ ] `Theme::font_family` (or `Settings::font_family`) drives the base font lookup via fontdb.
- [ ] Glyphs missing from the base font are resolved through the fallback chain.

## Technical Requirements

### Functional Requirements

- **FR1 (swash PoC):** Stand-alone example binary (`native-poc/examples/swash_emoji.rs` or similar) loading Noto Color Emoji via swash + zeno and dumping a single emoji RGBA buffer to disk. Gate: human-inspectable PNG of one emoji glyph.
- **FR2 (Glyph trait):** Introduce a `Glyph` trait (raster + shape interface) in `native-poc/src/render/font/` and refactor the per-cell drawing path so that it consumes `Glyph` results rather than addressing `ab_glyph` directly.
- **FR3 (Texture atlas split):** Provide an atlas layer supporting two region kinds: `Alpha` (R8) for monochrome glyphs and `Rgba` (RGBA8) for color glyphs. Coexistence is required; replacing the alpha atlas wholesale is out of scope.
- **FR4 (swash adapter):** Implement `Glyph` against swash's `Shaper` (shaping) and `Render` (rasterizing). Color-glyph detection (COLR/CBDT/SVG-in-OT bitmap tables) routes to the `Rgba` region; monochrome to `Alpha`.
- **FR5 (ab_glyph adapter):** Keep the existing ab_glyph code reachable as a parallel `Glyph` implementation. CJK and emoji gaps are accepted on this path (it is an escape hatch, not a full equivalent).
- **FR6 (Settings::font_engine):** Add `font_engine: FontEngine` enum (`Swash` default / `AbGlyph` fallback). Selection happens once at startup; runtime hot-swap is not in scope.
- **FR7 (Font resolution):** Use `fontdb` to enumerate system fonts at startup and register the bundled fonts (Noto Sans CJK JP, Noto Color Emoji) as primary fallbacks. Failed scans degrade to bundled-only with a `warn` log.
- **FR8 (Fallback chain):** Per grapheme cluster, walk `[base_font, font_family_fallback..., emoji_font]` until a font supplies the glyph, memoizing the (font_id, codepoint) decision. Final miss falls back to `U+FFFD` / replacement box (no regression vs. today).
- **FR9 (Settings schema additions):** Add `font_family_fallback: Vec<String>`, `emoji_font: Option<String>`, `variable_font_axes: HashMap<String, f32>`. Fields not consumed in Phase 4-H may carry `#[allow(dead_code)]` until Phase 7 wires `settings.json`, but `font_engine` / `font_family` / `font_family_fallback` must be live.
- **FR10 (Theme dead_code resolution):** `Theme::font_family` and `Theme::font_size_pt` are read by the renderer (replacing the hard-coded `FontFamily::Monospace` + `FONT_SIZE = 13.0` constants in `render/mod.rs`). If any `#[allow(dead_code)]` markers remain on those fields they are removed; the essential outcome is that both fields participate in the live render path.
- **FR11 (Bundled fonts):** Place Noto Sans CJK JP Regular and Noto Color Emoji under `native-poc/assets/fonts/` with their SIL OFL 1.1 license file, embed via `include_bytes!`, and register into fontdb at startup. On Windows, additionally probe Segoe UI Emoji as a secondary system fallback (not bundled).
- **FR12 (Terminal grid wgpu render pass — Option 3):** Build a new custom wgpu render pass `TerminalGridPass` (file `native-poc/src/render/terminal_grid_pass.rs` + WGSL shader) from scratch. The pass samples the two-region font atlas (Alpha R8 + RGBA8) and draws every terminal cell (foreground glyph + background fill + underline / strikethrough). Modeled after the existing `native-poc/src/image/overlay.rs` custom pass. Frame draw order: wgpu clear → `TerminalGridPass` → egui pass (`LoadOp::Load`, UI only: tab bar / status bar / IME preedit / settings panel; cursor MAY stay on the egui side) → `ImageOverlayPass` (`LoadOp::Load`, unchanged). Existing egui `painter.text()` / decoration-line / background-rect logic in `render/mod.rs::draw_grid` is removed once the new pass passes the Go / No-Go gates; until then the two paths MAY coexist behind a build-time switch.

### Non-Functional Requirements

- **NFR1 - Performance (startup):** Font enumeration + bundled-font registration completes in under 500 ms on a release build (Linux x86_64, warm CPU). **Hard gate**: violation is a Go / No-Go failure for the PoC.
- **NFR2 - Performance (glyph rasterize):** Glyph cache miss → swash rasterize completes in under 5 ms per glyph on release. Cache hit cost stays at parity with the ab_glyph path (no per-frame regression). **Hard gate**: violation is a Go / No-Go failure for the PoC.
- **NFR3 - Stability:** A failure on the swash path must not deadlock or crash the renderer to the point of losing the window; fallback is via `font_engine: AbGlyph` and a process restart. The `swash` crate version is pinned (`= 0.1.x`) to guard against single-maintainer churn.
- **NFR4 - Maintainability:** Glyph cache size / hit rate are observable through public accessors so a future diagnostics UI can read them.
- **NFR5 - License compliance:** Bundled fonts ship with SIL OFL 1.1 license files. Segoe UI Emoji is referenced via system lookup only (never embedded).
- **NFR6 - Platform coverage:** Linux X11 (primary host gate) + Windows (deferred host gate). macOS is out of scope.

## Implementation Approach

### Architecture

**Layering inside `native-poc/src/render/` (Option 3 — custom wgpu pass, built from scratch):**

```
┌──────────────────────────────────────────────────────────────┐
│  window_host frame loop                                       │
│  order: wgpu clear → TerminalGridPass → egui pass → image    │
│  overlay pass (all subsequent passes use LoadOp::Load)        │
└──────────────────────────────────────────────────────────────┘
              │
              ▼
┌──────────────────────────────────────────────────────────────┐
│  render/terminal_grid_pass.rs   custom wgpu render pass       │
│   - WGSL shader + pipeline + bind group                       │
│   - per-cell instanced quads (fg glyph + bg fill +            │
│     underline / strikethrough)                                │
│   - samples Alpha R8 / RGBA8 atlas pages                      │
└──────────────────────────────────────────────────────────────┘
              │ per-cell lookup
              ▼
┌──────────────────────────────────────────────────────────────┐
│  render/font/fallback.rs   chain resolve (base→CJK→emoji)     │
└──────────────────────────────────────────────────────────────┘
              │
              ▼
┌──────────────────────────────────────────────────────────────┐
│  render/font/cache.rs    glyph cache (key → atlas region)     │
└──────────────────────────────────────────────────────────────┘
              │ on miss, calls
              ▼
┌──────────────────────────────────────────────────────────────┐
│  render/font/traits.rs   GlyphRasterizer trait                │
└──────────────────────────────────────────────────────────────┘
              │
   ┌──────────┴──────────┐
   ▼                     ▼
┌──────────────┐   ┌──────────────┐
│ swash adapter│   │ ab_glyph     │  (legacy fallback)
└──────────────┘   └──────────────┘
              │
              ▼ uploads RGBA / alpha bitmap to
┌──────────────────────────────────────────────────────────────┐
│  render/font/atlas.rs    {alpha, rgba} wgpu textures          │
└──────────────────────────────────────────────────────────────┘
```

The existing `render/mod.rs::draw_grid` `painter.text()` / decoration-line / background-rect code is removed once `TerminalGridPass` clears the Go / No-Go gates. egui retains the UI layer only (tab bar / status bar / IME preedit / settings panel). The cursor MAY stay on the egui side or migrate into the new pass — implementation detail, not required by SPEC.

**Trait sketch (illustrative — exact signatures land in IMPLEMENTATION.md):**

```
pub enum AtlasFormat { Alpha, Rgba }

pub struct GlyphBitmap {
    pub format: AtlasFormat,
    pub width: u32,
    pub height: u32,
    pub bearing: (i32, i32),
    pub advance: f32,
    pub pixels: Vec<u8>,
}

pub trait GlyphRasterizer: Send + Sync {
    fn shape(&self, cluster: &str, font_id: FontId) -> Vec<ShapedGlyph>;
    fn raster(&self, font_id: FontId, glyph_id: u32, size_px: f32) -> Option<GlyphBitmap>;
}
```

### Data Flow

```
PTY → term_core grid update
  → window_host.redraw
      → TerminalGridPass.prepare (per-cell loop)
          → fallback_chain.resolve(grapheme) → (font_id, glyph_id)
              → glyph_cache.get_or_rasterize(...)
                  ├── hit  → atlas UV
                  └── miss → GlyphRasterizer (swash or ab_glyph)
                              → atlas.upload(...) → UV
          → push instance (cell_xy, atlas_uv, fg/bg color, decoration flags)
      → TerminalGridPass.draw (one instanced draw call)
      → egui pass (LoadOp::Load) for UI overlay
      → ImageOverlayPass (LoadOp::Load) for inline images
```

### API Design

This feature exposes no public Tauri commands or IPC endpoints. The new boundary is the `GlyphRasterizer` trait inside `native-poc`.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `term_core`: provides the grid being rendered. No new term_core changes required.
- `term_images`: independent layer (already RGBA atlas). Coexists; no merge with the font atlas in this SDD. `TerminalGridPass` draws before the existing `ImageOverlayPass`, both using their own pipelines.
- `native-poc::settings`: gains `font_engine` + font-related fields.
- `native-poc/src/image/overlay.rs`: reference implementation for the custom wgpu pass pattern (Option 3 follows the same shape: pipeline + bind group + instance buffer + `prepare` / `draw` API).

**External Dependencies (added):**
- `swash` (= 0.1.x, pinned, MIT/Apache 2.0): shaping + scaling.
- `zeno` (= 0.2.x or compatible, MIT/Apache 2.0): vector path → bitmap.
- `fontdb` (latest compatible with swash, MIT/Apache 2.0): system font enumeration.

**External Dependencies (retained):**
- `egui = "0.29"`, `egui-wgpu = "0.29"`: layout pipeline kept; only the glyph cache layer is swapped.
- `ab_glyph` (already transitive via egui): kept as the fallback rasterizer.

### File Structure

```
native-poc/
├── Cargo.toml                       # add swash, zeno, fontdb; pin versions
├── assets/
│   └── fonts/
│       ├── NotoSansCJKjp-Regular.otf    # SIL OFL 1.1
│       ├── NotoColorEmoji.ttf           # SIL OFL 1.1
│       └── LICENSE                       # SIL OFL 1.1 text
├── examples/
│   └── swash_emoji.rs                # PoC binary (FR1)
└── src/
    ├── settings.rs                       # add FontEngine + font fields
    └── render/
        ├── mod.rs                        # delete FONT_SIZE / FontFamily::Monospace; cell drawing moves to TerminalGridPass
        ├── theme.rs                      # drop #[allow(dead_code)] on font_family/font_size_pt
        ├── terminal_grid_pass.rs         # FR12 custom wgpu render pass (cells + decorations)
        ├── terminal_grid_pass.wgsl       # FR12 shader (or inline as include_str! inside terminal_grid_pass.rs)
        └── font/
            ├── mod.rs
            ├── traits.rs                 # GlyphRasterizer + GlyphBitmap + AtlasFormat
            ├── swash_adapter.rs          # FR4
            ├── ab_glyph_adapter.rs       # FR5
            ├── cache.rs                  # FR2 glyph cache
            ├── atlas.rs                  # FR3 alpha/rgba atlases
            ├── fallback.rs               # FR8 fallback chain memoization
            └── resolver.rs               # FR7 fontdb scan + bundled registration
```

## Test Scenarios

### Unit Tests

- [ ] TS-font-1: `FontEngine::default()` is `Swash`.
- [ ] TS-font-2: `Settings::font_engine = AbGlyph` parses from the textual form (`"ab_glyph"`) without warnings; unknown values fall back to default with a warn-log.
- [ ] TS-font-3: `GlyphCache::get_or_rasterize` returns the same atlas region on second call (cache hit).
- [ ] TS-font-4: `FallbackChain::resolve` returns base font for ASCII codepoints and the CJK fallback for `U+3042` (あ).
- [ ] TS-font-5: `FallbackChain::resolve` returns the emoji fallback for `U+1F600`.
- [ ] TS-font-6: Atlas upload routes `AtlasFormat::Alpha` bitmaps into the R8 texture and `Rgba` into RGBA8.
- [ ] TS-font-7: ab_glyph adapter implements `GlyphRasterizer::raster` returning `Alpha` bitmaps; CJK / emoji return `None` (cache treats this as a miss and falls through).
- [ ] TS-font-8: swash adapter rasterizes ASCII `'A'` and returns a non-empty alpha bitmap with sensible advance.
- [ ] TS-font-9: swash adapter rasterizes `U+1F600` and returns RGBA bitmap (asserts at least one non-zero RGB byte exists).
- [ ] TS-font-10: Bundled font registration succeeds against an in-memory fontdb (no real filesystem access).
- [ ] TS-font-11: `Theme::default().font_family` is `"monospace"` and `font_size_pt` is `13.0` (regression guard).
- [ ] TS-font-12: Renderer reads `Theme::font_family` and `Theme::font_size_pt` instead of the deleted `FONT_SIZE` constant / hard-coded `FontFamily::Monospace`. Assert by constructing a renderer with a `Theme` whose `font_family = "TestSentinelFont"` and `font_size_pt = 17.0`, then verifying the value reaches the resolver call (FontId lookup is invoked with `"TestSentinelFont"`) and the per-cell drawing emits at `17.0 pt` (cell pixel dim derived from 17.0, not 13.0).
- [ ] TS-font-13: `TerminalGridPass::prepare` emits one instance per non-empty cell (CPU-side state, GPU not required). Verify instance count = filled-cell count for a fixture grid; verify per-instance UV refers to the atlas region returned by the cache.
- [ ] TS-font-14: `TerminalGridPass::prepare` records the correct atlas-page index for Alpha vs RGBA glyphs (so the WGSL shader can branch on page kind).

### Integration Tests

- [ ] TS-font-int-1: `cargo run -p emterm-native-poc --example swash_emoji` produces a non-empty PNG file (PoC gate, FR1).
- [ ] TS-font-int-2: Headless render of a single cell containing `U+3042` produces a non-empty instance in the `TerminalGridPass` instance buffer (no panic on the swash path).
- [ ] TS-font-int-3: Headless render with `Settings::font_engine = AbGlyph` and the same cell completes without panicking; CJK glyph is permitted to be empty.
- [ ] TS-font-int-4: `TerminalGridPass` builds against the wgpu device used by `window_host` (smoke pipeline-build test; no draw call required — pipeline + bind-group-layout creation succeeds).

### E2E Tests

**Existing E2E tests**: Tauri-driver based suite at `e2e-tests/` (used by `src-tauri/` build). Not applicable to native-poc because tauri-driver cannot attach to a winit window without WebKit.
**Run command**: Not applicable to this SDD.

- [ ] No new E2E specs. Verification is via host manual gates (see below).

### Edge Cases

- [ ] Missing bundled font file at build time → build fails (compile-time `include_bytes!`); CI catches this.
- [ ] Corrupted bundled font at runtime → fontdb register fails with `warn` log; renderer falls back to base font only.
- [ ] fontdb panics during system scan → caught at the call site, downgraded to bundled-only with a `warn` log.
- [ ] Codepoint absent from every font in chain → renderer falls back to `U+FFFD` or empty quad (no panic).
- [ ] Atlas region exhaustion → re-allocate / grow atlas (or evict LRU); behavior documented in `atlas.rs`.
- [ ] swash returns a zero-size bitmap → cache stores it as a sentinel so subsequent calls do not retry.

### Performance Tests

- [ ] TS-font-perf-1: `EMTERM_FONT_PERF=1` env toggle prints startup font scan duration; release target < 500 ms (NFR1).
- [ ] TS-font-perf-2: `EMTERM_FONT_PERF=1` prints per-glyph rasterize duration on cache misses; release target < 5 ms/glyph (NFR2).

### Manual Host Gates

(Modeled on the Phase 4-G `TS-manual-ime-*` pattern. Gates are run on real hosts; tauri-driver is not used.)

- [ ] TS-manual-font-linux-x11: Linux + X11 + fcitx5 host. Type `echo こんにちは`, then IME-compose `こんにちは`. Both render as Japanese (G2). 1 line of mixed color emoji (`🎉🚀🤖`) renders in color (G1). Screenshot the result and store it under `doc/tasks/font-swash-migration/manual/`.
- [ ] TS-manual-font-linux-x11-fallback: Set `Settings::font_engine = AbGlyph`, restart. CJK renders as tofu (escape hatch confirmed to be alive). Restore default.
- [ ] TS-manual-font-windows: Windows host with default Segoe UI / Yu Gothic + MS-IME. Same scenarios as the Linux gate. Color emoji uses Segoe UI Emoji system fallback (not bundled).
- [ ] TS-manual-font-linux-wayland (defer-compatible): Linux Wayland host with `GDK_BACKEND=x11`. Same expectations as the X11 gate.
- [ ] TS-manual-font-startup-perf: With `EMTERM_FONT_PERF=1`, confirm `font scan total = <X> ms` log line is below 500 ms.

## Security Considerations

- **Authentication / Authorization:** Not applicable.
- **Input Validation:** Settings strings (`font_engine`, `font_family`, fallback list entries) are parsed with `warn-log + default fallback` on unknown values, mirroring the Phase 4-D `StatusBarPosition::parse_or_warn` convention.
- **Data Protection:** Fonts are read-only assets. No user data is written to disk by this feature.
- **Memory Safety:** swash and zeno are pure Rust (no `unsafe` C bindings on the hot path). The version pin guards against API breakage from a single-maintainer upstream.
- **Binary Provenance:** Bundled fonts are committed alongside their LICENSE; SHA-256 hashes recorded in `assets/fonts/README.md` or equivalent during IMPLEMENTATION.

## Error Handling

### Error Codes

Internal-only (no IPC surface). Errors are logged.

| Tag | Description | Log Level | Effect |
|---|---|---|---|
| `font.scan_failed` | fontdb panic / IO failure during system scan | warn | Continue with bundled-only registry |
| `font.bundled_missing` | A bundled font fails to register | warn | Continue; affected scripts may regress to base font |
| `font.unknown_engine` | settings.json has an unknown `font_engine` value | warn | Default to `Swash` |
| `font.glyph_miss_all` | All fonts in the chain miss a codepoint | debug | Render replacement glyph |
| `font.atlas_full` | Atlas region exhausted | warn | Evict / grow; document chosen strategy in IMPLEMENTATION.md |

### Error Flow

```
Error occurs → log at appropriate level → degrade gracefully (bundled-only / replacement glyph / default engine) → never panic the render thread
```

## Performance Optimization

### Performance Goals

- Startup font scan: < 500 ms (release, Linux x86_64).
- Glyph cache miss rasterize: < 5 ms / glyph (release).
- Cache hit per-cell cost: parity with ab_glyph path.

### Optimization Strategies

- **Cache key inclusive of subpixel offset:** prevents identical glyphs at different positions from triggering re-rasterize.
- **Per-(font, codepoint) fallback memoization:** the fallback chain walk happens once per (font_id, codepoint) pair.
- **Two-atlas split (alpha + rgba):** keeps the alpha path on R8 textures so existing performance characteristics for ASCII are preserved.
- **Lazy font loading:** bundled fonts are embedded but only parsed by fontdb when registration runs (once, at startup).

### Caching Strategy

- Glyph cache: `HashMap<(FontId, GlyphId, SizeBucket, SubpixelBucket), AtlasRegion>`. TTL: process lifetime. Eviction: not needed unless atlas grows beyond a configurable cap (Phase 4-H deferred — start without eviction, log if cap is hit, decide policy after PoC).
- Fallback resolution memo: `HashMap<(BaseFontId, Codepoint), FontId>`. TTL: process lifetime.
- fontdb result: built once at startup, never invalidated.

## Success Criteria

### Go / No-Go Gates (PoC failure if any of these miss)

- [ ] **G1 — Noto Color Emoji renders correctly**: emoji glyphs reach the screen as full-color bitmaps via the CBDT path through `TerminalGridPass` (RGBA atlas region sampled by the new wgpu pass). This is the headline gate for the Option 3 PoC.
- [ ] **G2 — CJK (Japanese hiragana / kanji) renders correctly**: no tofu / `U+FFFD` for typical Japanese terminal output.
- [ ] **G3 — Cell-width integrity**: emoji = 2 cells, CJK = 2 cells, ASCII = 1 cell. No bleed into adjacent cells.
- [ ] **G4 — Startup font scan < 500 ms** on release Linux x86_64 (NFR1).
- [ ] **G5 — Glyph cache-miss rasterize < 5 ms / glyph** on release (NFR2).

If any G1–G5 gate fails, this SDD is declared a PoC failure: discard the `swash + own wgpu pipeline` approach and re-evaluate alternatives (cosmic-text wrapper / Vello / drop color emoji and use `egui::Context::set_fonts` for bundled CJK only).

### Functional Completion

- [ ] FR1 PoC binary produces a recognizable emoji PNG.
- [ ] FR2–FR12 implemented and unit-tested per the test scenarios.
- [ ] `cargo test --workspace` passes (parity with the Phase 4-G baseline of ~1985 tests; new tests add to this count, no regressions).
- [ ] `cargo fmt --all` clean; `cargo clippy --workspace -- -D warnings` shows zero warnings.
- [ ] Renderer reads `Theme::font_family` and `Theme::font_size_pt` (any residual `#[allow(dead_code)]` on those fields is removed).
- [ ] Existing `painter.text()` cell drawing in `render/mod.rs::draw_grid` is removed (FR12) once G1–G5 pass.
- [ ] Linux X11 host manual gate confirms Japanese + color emoji rendering.
- [ ] Documentation updated: `tmp/restruct.md` Phase 4-H status flipped to ✅, font-related risk rows annotated.
- [ ] Windows host gate either passes or is documented as deferred with a clear next-step.

## Open Questions

> Note: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。`/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- [ ] (NFR1 detail) Bundled font subset strategy — full distribution vs. subset by script — deferred until after the PoC sizes the binary impact. Tracked as open, not blocking the start of implementation.
- [x] (FR3 detail) Whether the RGBA atlas can be sampled by egui's existing pipeline or requires a custom wgpu render pass — **resolved 2026-05-15**: a custom wgpu render pass (`TerminalGridPass`, FR12) is built from scratch. egui's text pipeline is not extended.

## Out-of-Scope (Future SDDs)

The following items are intentionally deferred and tracked here only so they are not lost. They are NOT part of this SDD's success criteria.

- **Emoji font auto-acquisition strategy**: at startup, probe system fonts; use system-side Noto Color Emoji when available, fall back to the bundled copy otherwise, or download on first run. This SDD ships with a fixed bundled emoji font and does not implement any auto-acquisition path.
- Ligature rendering (Fira Code / JetBrains Mono `==>` etc.; swash supports the OT features but terminal cell-width integrity is a separate design question).
- Vertical writing mode.
- Bidi (Arabic / Hebrew shaping is handled by swash; cursor remains LTR).
- TrueType bytecode hinting (swash does not implement it; pulling in a C dependency such as `freetype-rs` is rejected for this SDD).

## Implementation Phases

### Phase 1: swash + zeno evaluation PoC (FR1)

**Goals:** Confirm that swash + zeno can read Noto Color Emoji and emit a recognizable RGBA bitmap. Decide whether to proceed with direct swash or pivot to cosmic-text wrapping.

**Deliverables:**
- `native-poc/examples/swash_emoji.rs`
- PNG artifact of one emoji glyph
- Decision note (proceed-with-swash / pivot-to-cosmic-text) appended to IMPLEMENTATION.md

### Phase 2: Glyph trait + atlas split (FR2, FR3)

**Goals:** Refactor the renderer so per-cell drawing goes through the new `Glyph` trait and an atlas that can hold both alpha and RGBA regions. ab_glyph remains the only adapter at this point.

**Deliverables:**
- `render/font/{traits.rs, cache.rs, atlas.rs}`
- ab_glyph implementation behind the trait (FR5)
- Unit tests TS-font-3, TS-font-6, TS-font-7

### Phase 3: swash adapter + fallback chain (FR4, FR7, FR8)

**Goals:** Add the swash adapter, fontdb-driven font resolution, and the fallback chain.

**Deliverables:**
- `render/font/{swash_adapter.rs, resolver.rs, fallback.rs}`
- Unit tests TS-font-4, TS-font-5, TS-font-8, TS-font-9, TS-font-10
- Bundled fonts under `native-poc/assets/fonts/` (FR11)

### Phase 4: Settings integration + Theme dead_code resolution (FR6, FR9, FR10)

**Goals:** Wire `Settings::font_engine` and font-related fields. Remove `#[allow(dead_code)]` on Theme font fields by routing them through the renderer.

**Deliverables:**
- Settings additions
- Renderer reads Theme font fields (no more hard-coded `FONT_SIZE = 13.0`)
- Unit tests TS-font-1, TS-font-2, TS-font-11, TS-font-12

### Phase 4-H: Terminal grid wgpu render pass — Option 3 (FR12)

**Goals:** Build `TerminalGridPass` from scratch and route the foundation (cache + fallback chain + atlas + startup rasterizer selection) through it. Existing `painter.text()` cell drawing in `render/mod.rs::draw_grid` is removed at the end of this phase, once G1–G5 gates pass.

**Stance:** PoC. If the new pass cannot satisfy G1 (Noto Color Emoji) and G2 (CJK), discard the swash + own-pipeline route and re-evaluate alternatives (cosmic-text / Vello / `egui::Context::set_fonts` for CJK only).

**Deliverables:**
- `native-poc/src/render/terminal_grid_pass.rs` (struct + pipeline + bind group + instance buffer + prepare/draw API)
- WGSL shader (file or `include_str!`) handling Alpha R8 vs RGBA8 atlas-page selection
- `window_host` integration: new frame draw order `clear → TerminalGridPass → egui pass (LoadOp::Load) → ImageOverlayPass (LoadOp::Load)`
- Selection / underline / strikethrough migrated from `draw_grid` into the new pass (cursor MAY stay on the egui side)
- `painter.text()` and decoration-line / background-rect calls removed from `render/mod.rs::draw_grid`
- `app.rs` constructs the chosen `GlyphRasterizer` (Swash / AbGlyph) at startup and hands it to `TerminalGridPass`

### Phase 5: Host manual gates + perf check

**Goals:** Run the host manual gates on Linux X11 (primary) and either run or defer Windows. Confirm Go / No-Go gates G1–G5.

**Deliverables:**
- Manual gate logs / screenshots stored under `doc/tasks/font-swash-migration/manual/` or referenced in VERIFICATION_RESULT.md
- Perf log under `EMTERM_FONT_PERF=1` confirming NFR1 / NFR2
- Recorded G1 / G2 outcome (color emoji + CJK on the screen) with screenshots

## References

- Phase 4-H section: `tmp/restruct.md`
- Phase 4-G IME integration (immediate predecessor): `doc/tasks/ime-native-integration/SPEC.md`
- Phase 1 PoC (ab_glyph origin): `doc/tasks/native-terminal-poc/SPEC.md`
- swash crate: <https://github.com/dfrg/swash>
- zeno crate: <https://github.com/dfrg/zeno>
- fontdb crate: <https://github.com/RazrFalcon/fontdb>
- Noto fonts: <https://github.com/googlefonts/noto-cjk>, <https://github.com/googlefonts/noto-emoji>
