# Implementation Plan: Font Renderer Migration (ab_glyph → swash)

## Overview

Replace native-poc terminal grid's glyph rasterizer with swash + zeno, gaining CJK shaping and color emoji rendering while keeping ab_glyph as a fallback engine behind a `Settings::font_engine` flag. Scope is `native-poc/` only; Wry viewers are out of scope.

## Objectives

- Render CJK (Japanese / Chinese / Korean) glyphs in the native terminal grid (eliminate `U+FFFD` tofu).
- Render color emoji as bitmap/COLR glyphs (Noto Color Emoji bundled; Segoe UI Emoji via system lookup on Windows).
- Abstract the glyph cache behind a `Glyph` trait so future font work does not touch every renderer call site.
- Keep ab_glyph reachable as an opt-in escape hatch until Phase 7.
- Remove `#[allow(dead_code)]` from `Theme::font_family` / `Theme::font_size_pt` by routing them through the renderer.

## Prerequisites

### Development Environment

- Rust toolchain matching the workspace (see root `rust-toolchain.toml` / `Cargo.toml`).
- Docker (for the canonical build/test gates documented in `sdd.yaml.project.components.main`).
- Linux X11 + fcitx5 host for the primary manual gate; Windows host deferred-compatible.

### Dependencies

- Phase 4-G complete (HEAD = 5c54d1a, winit 0.30.9 IME bridge live).
- `term_core` crate (already provides grid + Unicode width).
- `native-poc` builds and runs against `GDK_BACKEND=x11`.

## Architecture Overview

### Technology Stack

- **Language**: Rust (workspace member `native-poc`).
- **Framework**: winit 0.30 + wgpu 22 + egui 0.29.
- **Key Libraries**:
  - `swash` (pinned `= 0.1.x`) — font parsing, shaping, scaling.
  - `zeno` — vector path rasterizer (companion to swash).
  - `fontdb` — system font enumeration.
  - `ab_glyph` (already transitive via egui) — retained as fallback adapter.

### Design Approach (revised 2026-05-15 — Option 3)

Build a new custom wgpu render pass `TerminalGridPass` from scratch (mirroring the existing `image/overlay.rs` pattern). egui's text path is **not** extended; per-cell drawing leaves the egui pipeline entirely. The new layering inside `native-poc/src/render/`:

```
window_host frame loop
  ├─ clear
  ├─ TerminalGridPass.draw     (this SDD, new)
  ├─ egui pass (LoadOp::Load)  (UI only: tabs / status / IME preedit / settings)
  └─ ImageOverlayPass (LoadOp::Load)   (unchanged, image/overlay.rs)

TerminalGridPass.prepare
        │ per-cell
        ▼
render/font/fallback.rs        chain resolve
        │
        ▼
render/font/cache.rs           glyph cache (key → atlas region)
        │  miss
        ▼
render/font/traits.rs          GlyphRasterizer trait
        │
   ┌────┴────┐
   ▼         ▼
swash       ab_glyph           (adapter pair)
        │
        ▼
render/font/atlas.rs           alpha (R8) + rgba (RGBA8) wgpu textures
```

Glyph resolution per grapheme cluster walks the fallback chain `[base_font, font_family_fallback..., emoji_font]`; the chosen (font_id, codepoint) is memoized. Settings selects the active rasterizer at startup (no runtime hot-swap).

### Component Interaction

- `app.rs` constructs the chosen rasterizer once based on `Settings::font_engine` and hands it to `TerminalGridPass`.
- `render/mod.rs::draw_grid` is gutted: `painter.text()`, decoration-line, and background-rect calls are removed. The cell loop moves into `TerminalGridPass::prepare`.
- `render/mod.rs` (or its successor in `TerminalGridPass`) reads `Theme::font_family` and `Theme::font_size_pt`; the `FONT_SIZE` constant + `FontFamily::Monospace` literal are deleted.
- The glyph cache and atlas are consumed by `TerminalGridPass`; egui no longer touches the new atlas textures.
- The cursor MAY remain on the egui side (Phase 4-G IME bridge already paints it there) or be promoted into `TerminalGridPass`. Implementation choice; SPEC FR12 does not mandate.

## Implementation Phases

### Phase 1: swash + zeno evaluation PoC (FR1)

**Goal**: Confirm that swash + zeno can read Noto Color Emoji and emit a recognizable RGBA bitmap. Produce a Go / pivot decision before refactoring the renderer.

**Files to Create**:
- `native-poc/examples/swash_emoji.rs` — stand-alone binary that loads Noto Color Emoji, rasterizes one emoji glyph, writes a PNG.
- `native-poc/assets/fonts/NotoColorEmoji.ttf` — bundled font for the PoC (placed early so the example can `include_bytes!` it).
- `native-poc/assets/fonts/LICENSE` — SIL OFL 1.1 text.
- `native-poc/assets/fonts/README.md` — bundled font versions + SHA-256 hashes (per security note in SPEC).

**Files to Modify**:
- `native-poc/Cargo.toml` — add `swash` (pinned), `zeno`, and a dev-only PNG encoder (e.g. existing `image` crate if available, otherwise minimal raw PNG via existing dependency) for the example only.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|---|---|---|---|
| `swash_emoji` example | Load font, shape one cluster, rasterize, write PNG | Bundled font present, swash/zeno linkable | PNG of one emoji glyph exists on disk |

**Processing Flow**:
1. Load Noto Color Emoji bytes (`include_bytes!`).
2. Open via swash font reference.
3. Look up a fixed emoji codepoint (e.g. `U+1F600`).
4. Rasterize with zeno; obtain RGBA buffer.
5. Encode and write PNG to a target path.

**Implementation Steps**:
1. **Bundle minimum font set** — Place Noto Color Emoji + LICENSE under `assets/fonts/`. Record SHA-256 + upstream version.
2. **Add swash + zeno dependencies** — Pin swash at `= 0.1.x`. Verify workspace compiles.
3. **Write the PoC example** — Loads the bundled font, rasterizes one emoji, writes PNG.
4. **Decision note** — Append a short PoC outcome paragraph (proceed-with-swash / pivot-to-cosmic-text) to this IMPLEMENTATION.md (Phase 1 epilogue).

**Dependencies**: None upstream. Blocks Phase 2+.

**Testing Approach**:
- Unit: not applicable (example binary).
- Integration: TS-font-int-1 (`cargo run --example swash_emoji` produces non-empty PNG).
- Manual: Open the PNG and confirm the glyph is recognizable.

**Acceptance Criteria**:
- [ ] `cargo build --example swash_emoji` succeeds.
- [ ] Running the example produces a non-empty PNG.
- [ ] Decision note appended to IMPLEMENTATION.md.

**Estimated Effort**: small

#### Phase 1 PoC Outcome (2026-05-14)

**Decision: proceed with swash + zeno.** No pivot to cosmic-text.

- `cargo run -p emterm-native-poc --example swash_emoji` produced
  `native-poc/target/swash_emoji.png` (159 × 150 RGBA8, 24 087 bytes) on
  the canonical Docker build environment.
- swash returned `Content::Color` from `Render::render` for `U+1F600`
  using the `Source::ColorBitmap(StrikeWith::BestFit)` source, confirming
  Noto Color Emoji's CBDT strikes round-trip through swash + zeno end to
  end without auxiliary plumbing.
- swash 0.1.18 was pinned (`= 0.1.18`) per NFR3; zeno is pulled in
  transitively as `swash::zeno` so a separate `zeno` dep was kept light
  (matches version with swash's transitive resolution).
- Open Question on RGBA-atlas-vs-egui-pipeline (SPEC.md): the PoC does
  not yet exercise the egui-wgpu sampler. Phase 2 will land a custom
  two-region atlas and upload path; if egui's existing pipeline cannot
  sample the RGBA region directly, Phase 2 introduces a small custom
  render pass beside the egui pass. Decision deferred to Phase 2.

---

### Phase 2: Glyph trait + atlas split (FR2, FR3, FR5)

**Goal**: Refactor the renderer so per-cell drawing goes through a new `GlyphRasterizer` trait + a two-region atlas (Alpha R8 + RGBA8). ab_glyph is the only adapter at this point; behavior is unchanged for ASCII.

**Files to Create**:
- `native-poc/src/render/font/mod.rs` — module aggregator.
- `native-poc/src/render/font/traits.rs` — `GlyphRasterizer` trait, `GlyphBitmap`, `AtlasFormat`, `FontId`, `ShapedGlyph` types.
- `native-poc/src/render/font/cache.rs` — glyph cache (key → atlas region; cache-hit / miss logic; sentinel for zero-size bitmaps).
- `native-poc/src/render/font/atlas.rs` — atlas with alpha + rgba regions; growth / eviction policy stub (start without eviction, log on cap hit).
- `native-poc/src/render/font/ab_glyph_adapter.rs` — `GlyphRasterizer` implementation backed by ab_glyph (Alpha-only; CJK / emoji return `None`).

**Files to Modify**:
- `native-poc/src/app.rs` — construct an `ab_glyph_adapter::AbGlyphRasterizer` at startup. (The renderer-side consumer lands in Phase 4-H; this phase only builds the foundation layer + unit tests.)

(`native-poc/src/render/mod.rs` is **not** modified in this phase. Direct `ab_glyph` usage there is left alone until Phase 4-H replaces the whole `draw_grid` cell loop with `TerminalGridPass`. The foundation layer is fully unit-tested without touching the live renderer.)

(`native-poc/src/render/theme.rs` is unchanged in this phase; Theme wiring lands in Phase 4.)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|---|---|---|---|
| `GlyphRasterizer` trait | Shape + rasterize per (font_id, glyph_id, size) | None | Returns `GlyphBitmap` (Alpha or Rgba) or `None` (miss) |
| `GlyphBitmap` | Carry pixel buffer + width/height/bearing/advance + format tag | Rasterizer returned data | Atlas-upload-ready |
| `AtlasFormat` | Enum `{Alpha, Rgba}` | — | Atlas dispatches to R8 or RGBA8 region |
| `GlyphCache` | Memoize (FontId, GlyphId, SizeBucket, SubpixelBucket) → AtlasRegion | Rasterizer + Atlas available | Cache hit returns region; miss triggers rasterize then upload |
| `Atlas` | Own R8 + RGBA8 wgpu textures; allocate regions | wgpu device available | Returns UV rect per upload |
| `AbGlyphRasterizer` | Adapter wrapping today's ab_glyph path | ab_glyph font loaded | Alpha bitmaps for ASCII; `None` for CJK / emoji |

**Processing Flow** (per-cell glyph emit):
1. Compute lookup key from (font_id, glyph_id, size_bucket, subpixel_bucket).
2. Cache hit → return UV.
3. Cache miss → call rasterizer.
   - Returns `Some(GlyphBitmap)` → atlas upload by format → cache + return UV.
   - Returns `None` → store sentinel; caller emits replacement glyph or empty quad.
4. Emit quad with UV + bearing + advance.

**Implementation Steps**:
1. **Introduce the font module skeleton** — Create `render/font/` with the trait, types, and module wiring. No behavior change yet.
2. **Add the two-region atlas** — Define `AtlasFormat`, allocate two wgpu textures, expose `upload(format, bitmap) → AtlasRegion`. Document growth strategy.
3. **Wrap ab_glyph in the trait** — Build `AbGlyphRasterizer` as a standalone implementation. Returns `None` for non-ASCII. (Existing `render/mod.rs` ab_glyph calls stay; the new adapter is consumed by `TerminalGridPass` in Phase 4-H.)
4. **Implement `GlyphCache`** — Cache + sentinel logic, unit-tested standalone. (Live wiring into the renderer happens in Phase 4-H via `TerminalGridPass`.)
5. **Expose cache observability** — Public accessors for cache size + hit count (NFR4).

**Dependencies**: Requires Phase 1 decision (Go on swash). Blocks Phase 3 (swash adapter) and Phase 4 (Settings wiring).

**Testing Approach**:
- Unit: TS-font-3 (cache hit), TS-font-6 (atlas format routing), TS-font-7 (ab_glyph adapter returns `None` for CJK / emoji).
- Integration: existing `cargo test --workspace` regression suite stays green.
- Manual: visually confirm ASCII rendering parity with HEAD = 5c54d1a.

**Acceptance Criteria**:
- [ ] Foundation layer (`render/font/`) compiles and unit-tests pass.
- [ ] No regression in `cargo test --workspace` (parity with ~1985 tests baseline).
- [ ] Cache size + hit count are readable from public accessors.

(Note: live integration with the renderer's draw path is intentionally **deferred to Phase 4-H** — Option 3 replaces the whole `draw_grid` cell loop, so partial wiring through Phase 2 would be wasted work.)

**Estimated Effort**: medium

---

### Phase 3: swash adapter + font resolution + fallback chain (FR4, FR7, FR8, FR11)

**Goal**: Add the swash adapter, fontdb-driven font resolution, the per-codepoint fallback chain, and bundle the CJK font alongside the emoji font from Phase 1. After this phase, Japanese + color emoji render under the default `Settings::font_engine = Swash`.

**Files to Create**:
- `native-poc/src/render/font/swash_adapter.rs` — `GlyphRasterizer` implementation using swash `Shaper` + `Render`; emits Alpha for monochrome, RGBA for color glyphs (COLR / CBDT / SVG-in-OT detection).
- `native-poc/src/render/font/resolver.rs` — fontdb scan + bundled font registration; produces a name → FontId table.
- `native-poc/src/render/font/fallback.rs` — fallback chain walk with `(BaseFontId, Codepoint) → FontId` memoization.
- `native-poc/assets/fonts/NotoSansCJKjp-Regular.otf` — bundled CJK base.

**Files to Modify**:
- `native-poc/assets/fonts/README.md` — record CJK font version + SHA-256.
- `native-poc/src/render/font/mod.rs` — re-export swash adapter, resolver, fallback.
- `native-poc/src/app.rs` — startup wiring: build resolver, register bundled fonts, build fallback chain, instantiate swash adapter by default (still gated by Settings in Phase 4).

(Live integration of the fallback chain into per-cell drawing is deferred to **Phase 4-H** — `TerminalGridPass::prepare` is its only consumer. `render/mod.rs` is **not** modified here.)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|---|---|---|---|
| `SwashRasterizer` | Shape and rasterize via swash; route color/mono | Font registered in resolver | `GlyphBitmap` with correct format |
| `Resolver` | Scan system fonts via fontdb; register bundled fonts | fontdb available; bundled bytes accessible | Returns FontId table; logs warn on scan failure |
| `FallbackChain` | For a grapheme cluster, walk `[base, fallback..., emoji]` until hit | Resolver populated | Returns `(FontId, GlyphId)` or `None` (renderer draws replacement) |
| Bundled fonts | Provide CJK + emoji coverage out of the box | Files committed under `assets/fonts/` | `include_bytes!` succeeds at build time |

**Processing Flow** (grapheme cluster → glyph):
1. FallbackChain receives a cluster + base font id.
2. Check memo table `(BaseFontId, Codepoint)` → hit returns cached FontId.
3. Miss → iterate chain: for each font, ask the rasterizer "do you have this codepoint?".
   - Hit → memoize + return.
   - All miss → return `None`; renderer emits replacement glyph (`U+FFFD` or empty box, behavior unchanged from today).

**Implementation Steps**:
1. **Add fontdb dependency + resolver** — Build a startup scan; register bundled bytes; failure paths warn + continue with bundled-only.
2. **Implement `SwashRasterizer`** — Shape via swash `Shaper`, raster via `Render`. Detect color tables; emit Alpha vs Rgba.
3. **Implement `FallbackChain` + memo** — Walk base → CJK → emoji, memoize result. Stands as a fully unit-tested module; live consumer lands in Phase 4-H.
4. **Windows secondary fallback** — On Windows, append `Segoe UI Emoji` (system lookup) as a secondary chain entry (scaffolding via `FontRole::Secondary`).

**Dependencies**: Requires Phase 2 (trait + atlas + cache). Blocks Phase 4 (Settings selects between swash/ab_glyph).

**Testing Approach**:
- Unit: TS-font-4 (CJK fallback), TS-font-5 (emoji fallback), TS-font-8 (swash rasterizes ASCII to non-empty alpha), TS-font-9 (swash rasterizes emoji to non-zero RGB), TS-font-10 (bundled registration on in-memory fontdb).
- Integration: TS-font-int-2 (headless render of `U+3042` produces a non-empty quad).
- Manual: TS-manual-font-linux-x11 (deferred to Phase 5 gate).

**Acceptance Criteria**:
- [ ] `U+3042` resolves to the CJK bundled font in unit tests.
- [ ] `U+1F600` resolves to the emoji font in unit tests.
- [ ] swash rasterize returns RGBA for at least one bundled color emoji.
- [ ] `cargo test --workspace` PASS.

**Estimated Effort**: large

---

### Phase 4: Settings integration + Theme dead_code resolution (FR6, FR9, FR10)

**Goal**: Wire `Settings::font_engine` and the new font-related fields. Make `Theme::font_family` and `Theme::font_size_pt` live by reading them from `render/mod.rs` instead of the hard-coded `FONT_SIZE = 13.0` and `FontFamily::Monospace`. Remove any residual `#[allow(dead_code)]` markers on those fields (current code at HEAD = 5c54d1a has none on the fields themselves, but earlier SDD notes documented them as preexisting dead code; the live-read wiring is the essential outcome).

**Files to Modify**:
- `native-poc/src/settings.rs` — add `font_engine: FontEngine`, `font_family_fallback: Vec<String>`, `emoji_font: Option<String>`, `variable_font_axes: HashMap<String, f32>`. Add `FontEngine::{Swash, AbGlyph}` enum with `Swash` default. Parse from textual form (`"swash"` / `"ab_glyph"`); unknown values warn-log + fall back to default.
- `native-poc/src/render/theme.rs` — remove `#[allow(dead_code)]` on `font_family` / `font_size_pt`.
- `native-poc/src/render/mod.rs` — read `Theme::font_family` and `Theme::font_size_pt`. The deletion of the `FONT_SIZE` constant and `FontFamily::Monospace` literal happens in **Phase 4-H** when `painter.text()` is removed; this phase only ensures the live read of `Theme` fields by the renderer (TS-font-12).

(Startup branching on `Settings::font_engine` to build `SwashRasterizer` vs `AbGlyphRasterizer` is moved to **Phase 4-H** — it lives next to the `TerminalGridPass` construction.)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|---|---|---|---|
| `FontEngine` enum | Selects rasterizer at startup | Settings parsed | Either `Swash` or `AbGlyph` is active for the process lifetime |
| Settings additions | Carry user font choices | Settings file parsed (or default) | Renderer + resolver consume these fields |
| Theme wiring | Provide base font name + size from Theme | Theme constructed | Renderer no longer references the removed `FONT_SIZE` constant |

**Processing Flow** (startup):
1. Load Settings (existing path).
2. Read `font_engine`.
   - `Swash` (default) → instantiate `SwashRasterizer`.
   - `AbGlyph` → instantiate `AbGlyphRasterizer` (CJK / emoji will tofu — escape hatch is alive).
3. Build resolver + fallback chain (same for both engines).
4. Construct renderer with `Theme::font_family` + `Theme::font_size_pt` as base font config.

**Implementation Steps**:
1. **Add `FontEngine` enum + Settings fields** — Default values per FR9. Add a parser that warn-logs on unknown values.
2. **Remove Theme `#[allow(dead_code)]`** — Make `font_family` + `font_size_pt` live by reading them from the renderer.
3. **Renderer reads Theme** — `render/mod.rs` reads `Theme::font_family` + `font_size_pt`. (Removing the `FONT_SIZE` constant + `FontFamily::Monospace` literal happens in Phase 4-H.)

(Step 4 "Startup selection — build the chosen rasterizer in `app.rs` based on Settings" is moved into **Phase 4-H**.)

**Dependencies**: Requires Phase 3 (swash adapter must compile). Blocks Phase 4-H (renderer pass construction).

**Testing Approach**:
- Unit: TS-font-1 (`FontEngine::default() == Swash`), TS-font-2 (parse `"ab_glyph"` OK / unknown warn + default), TS-font-11 (`Theme::default()` regression), TS-font-12 (renderer reads Theme), TS-font-int-3 (headless render with `AbGlyph` does not panic).
- Integration: `cargo test --workspace` PASS.
- Manual: deferred to Phase 5.

**Acceptance Criteria**:
- [ ] `Theme::font_family` and `Theme::font_size_pt` are read by the renderer; any residual `#[allow(dead_code)]` on those fields is removed.
- [ ] Settings parses both `font_engine` strings; unknown values warn + default to Swash.
- [ ] `cargo fmt --all` clean; `cargo clippy --workspace -- -D warnings` zero warnings.

(Deletion of the `FONT_SIZE` constant + startup `font_engine` branch land in Phase 4-H.)

**Estimated Effort**: small

---

### Phase 4-H: Terminal grid wgpu render pass — Option 3 (FR12)

**Goal**: Build `TerminalGridPass` from scratch and route the foundation (cache + fallback chain + atlas + startup rasterizer selection) through it. Replace `render/mod.rs::draw_grid`'s `painter.text()` / decoration-line / background-rect calls with the new pass. Headline outcome: the Go / No-Go gates G1 (Noto Color Emoji) + G2 (CJK) pass.

**PoC stance**: if G1 or G2 cannot be met with the new pass, the SDD is declared a PoC failure and the swash + own-pipeline approach is dropped (re-evaluate cosmic-text / Vello / drop color emoji + `egui::Context::set_fonts` for CJK only).

**Files to Create**:
- `native-poc/src/render/terminal_grid_pass.rs` — `TerminalGridPass` struct: wgpu pipeline + bind group layouts + uniform / instance buffer + `prepare(grid_snapshot, cache, fallback, atlas)` + `draw(render_pass)`.
- WGSL shader (file `native-poc/src/render/terminal_grid_pass.wgsl` or `include_str!` inside `terminal_grid_pass.rs`) — handles atlas-page (Alpha R8 vs RGBA8) branching, fg color modulation for Alpha glyphs, RGBA glyphs sampled directly, decoration lines (underline / strikethrough) as a separate sub-pipeline or via shader-side flags, background-rect fill.

**Files to Modify**:
- `native-poc/src/window_host.rs` (or wherever the frame loop lives) — insert `TerminalGridPass` into the frame draw order: `clear → TerminalGridPass → egui pass (LoadOp::Load) → ImageOverlayPass (LoadOp::Load)`.
- `native-poc/src/render/mod.rs::draw_grid` — remove `painter.text()` calls; remove decoration-line `painter.line_segment` calls; remove background-rect `painter.rect_filled` calls. Selection highlighting moves into the new pass (or stays on egui if the existing rect-based selection is preserved unchanged — implementation choice). Cursor MAY stay on the egui side.
- `native-poc/src/app.rs` — at startup, branch on `Settings::font_engine` to build either `SwashRasterizer` or `AbGlyphRasterizer`, then construct `TerminalGridPass` with the chosen rasterizer + resolver + fallback chain.
- `native-poc/src/render/mod.rs` — delete the `FONT_SIZE` constant + `FontFamily::Monospace` literal (no longer referenced after the cell loop migrates).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|---|---|---|---|
| `TerminalGridPass` | Custom wgpu pass that samples the font atlas and draws every cell | Pipeline + bind groups built; atlas + cache + fallback wired | One instanced draw call per frame producing the cell layer |
| WGSL shader | Per-instance vertex + fragment; branches on atlas-page kind | Bind group with both atlas pages + a sampler | Alpha glyphs get fg color modulation; RGBA glyphs pass through |
| Frame-order integration | Inject `TerminalGridPass` before egui pass | Egui pass uses `LoadOp::Load` after this SDD | Cells render below UI overlay; image overlay still draws last |
| `painter.text()` removal | Strip dead egui text calls | TerminalGridPass passes G1 + G2 | No `painter.text` calls remain in `draw_grid` |

**Processing Flow** (one frame):
1. `window_host` enters redraw.
2. `TerminalGridPass::prepare`:
   - Snapshot the grid (rows, cells, fg / bg / attrs).
   - For each cell, run `FallbackChain::resolve(cluster)` → `(FontId, GlyphId)`.
   - `GlyphCache::get_or_rasterize` → `AtlasRegion` (uploads on miss).
   - Push an instance: `cell_xy, atlas_uv, atlas_page_kind, fg_rgba, bg_rgba, decoration_flags`.
3. `TerminalGridPass::draw(render_pass)`:
   - One instanced draw call against a single 0..1 quad VBO + the instance buffer.
4. egui pass runs with `LoadOp::Load` (UI overlay).
5. `ImageOverlayPass` runs with `LoadOp::Load` (Kitty / SIXEL images).

**Implementation Steps**:
1. **Author the WGSL shader** — Atlas-page branch + fg modulation + UV math.
2. **Build the wgpu pipeline + bind group layout** — Bind group: `{ atlas_alpha_view, atlas_rgba_view, sampler, uniform_buffer }`; instance buffer for per-cell data.
3. **Implement `TerminalGridPass` struct** — `new(device, format)`, `prepare(cx)`, `draw(rpass)`.
4. **Integrate into `window_host`** — Frame draw order: clear → grid pass → egui (Load) → image overlay (Load).
5. **Migrate selection / underline / strikethrough** — Move out of `draw_grid` into the new pass.
6. **Remove `painter.text` from `draw_grid`** — After G1 + G2 pass, delete egui text calls + `FONT_SIZE` constant + `FontFamily::Monospace` literal.
7. **Startup rasterizer selection** — `app.rs` branches on `Settings::font_engine` and hands the chosen rasterizer to `TerminalGridPass`.

**Dependencies**: Requires Phase 2 (foundation), Phase 3 (swash + fallback), Phase 4 (Settings + Theme). Blocks Phase 5 (host gates).

**Testing Approach**:
- Unit: TS-font-13 (prepare emits one instance per non-empty cell, CPU-side), TS-font-14 (atlas-page index recorded correctly).
- Integration: TS-font-int-2 (headless render of `U+3042` produces a non-empty instance), TS-font-int-4 (pipeline-build smoke test against the wgpu device).
- Manual: Phase 5 hosts run TS-manual-font-linux-x11 (records G1 + G2 outcome with screenshots).

**Acceptance Criteria**:
- [ ] `TerminalGridPass` is constructed at startup with the chosen rasterizer + atlas + cache + fallback chain.
- [ ] `window_host` frame draw order is `clear → TerminalGridPass → egui (Load) → ImageOverlayPass (Load)`.
- [ ] `painter.text()` calls are removed from `render/mod.rs::draw_grid`.
- [ ] `FONT_SIZE` constant + `FontFamily::Monospace` literal deleted from `render/mod.rs`.
- [ ] TS-font-13 / TS-font-14 / TS-font-int-2 / TS-font-int-4 pass.
- [ ] Go / No-Go gates G1 (Noto Color Emoji) + G2 (CJK) are visually confirmed on the Linux X11 host (Phase 5 logs / screenshots).

**PoC Failure Path**: if G1 or G2 cannot be met, do NOT spend more cycles patching the pass. Stop, write a "Phase 4-H PoC failure" note in IMPLEMENTATION.md, revert the `TerminalGridPass` integration, and run a separate SDD to evaluate alternatives (cosmic-text / Vello / drop color emoji + `egui::Context::set_fonts` for CJK only).

**Estimated Effort**: large (1–2 weeks including host validation; multi-day for the shader + pipeline alone).

---

### Phase 5: Host manual gates + perf verification (NFR1, NFR2, NFR3, NFR5, NFR6)

**Goal**: Run host manual gates on Linux X11 (primary), validate NFR1 / NFR2 against `EMTERM_FONT_PERF=1`, and either run or formally defer the Windows host gate.

**Files to Modify**:
- `native-poc/src/render/font/cache.rs` (or a small `perf.rs` shim) — honor `EMTERM_FONT_PERF=1`: log startup scan duration and per-glyph rasterize duration on cache misses.
- `doc/tasks/font-swash-migration/VERIFICATION.md` — append manual gate outcome rows (this agent writes the planned scenarios; the operator records results).
- `tmp/restruct.md` — Phase 4-H status row flipped on completion.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|---|---|---|---|
| `EMTERM_FONT_PERF` env toggle | Print perf timings to log | Set in environment | `font scan total = <X> ms` and per-glyph timings present in `emterm.log` |
| Manual gate logs | Record host gate outcomes | Linux X11 / Windows host available | Pass / Defer recorded in VERIFICATION_RESULT.md (Phase 6) |

**Processing Flow** (manual gate, Linux X11):
1. `EMTERM_FONT_PERF=1 GDK_BACKEND=x11 cargo run --release -p emterm-native-poc` (release for NFR perf numbers).
2. Type `echo こんにちは` → confirm Japanese renders.
3. IME-compose `こんにちは` via fcitx5 → confirm preedit + commit render with the same font.
4. `echo 🎉🚀🤖` → confirm color emoji render in color, no overlap.
5. Restart with `Settings::font_engine = AbGlyph` → confirm tofu (escape hatch alive).
6. Read `emterm.log` → confirm `font scan total < 500 ms` and `glyph rasterize < 5 ms` lines.

**Implementation Steps**:
1. **Add perf instrumentation** — Time-gated logging behind `EMTERM_FONT_PERF=1`.
2. **Run Linux X11 + fcitx5 gate** — Operator captures screenshots / log excerpts under `doc/tasks/font-swash-migration/manual/`.
3. **Run Linux Wayland (`GDK_BACKEND=x11`) gate** — Same scenarios, deferred-compatible.
4. **Run Windows gate or formally defer** — If a Windows host is available, run; otherwise document the defer with a clear next-step.
5. **Flip Phase 4-H status in `tmp/restruct.md`** — Update the status row and risk annotations once gates pass.

**Dependencies**: Requires Phase 4 (Settings wired). Blocks SDD verify step.

**Testing Approach**:
- Unit: none new.
- Integration: TS-font-perf-1 + TS-font-perf-2 (via env toggle, recorded in log).
- E2E: not applicable (font rendering not observable via tauri-driver).
- Manual: TS-manual-font-linux-x11, TS-manual-font-linux-x11-fallback, TS-manual-font-wayland, TS-manual-font-windows, TS-manual-font-startup-perf.

**Acceptance Criteria**:
- [ ] Linux X11 + fcitx5 gate passes (JA + color emoji visible).
- [ ] AbGlyph fallback confirmed to be alive (tofu observed when selected).
- [ ] `EMTERM_FONT_PERF=1` log shows `font scan total < 500 ms` on release build.
- [ ] Per-glyph rasterize timings show < 5 ms / glyph on cache misses.
- [ ] Windows gate passes or has a documented defer with next-step.
- [ ] Phase 4-H status row in `tmp/restruct.md` updated.

**Estimated Effort**: medium

---

## Complete File Structure

```
native-poc/
├── Cargo.toml                                  # add swash (pinned), zeno, fontdb
├── assets/
│   └── fonts/
│       ├── NotoSansCJKjp-Regular.otf           # Phase 3 bundled
│       ├── NotoColorEmoji.ttf                  # Phase 1 bundled
│       ├── LICENSE                              # SIL OFL 1.1
│       └── README.md                            # versions + SHA-256
├── examples/
│   └── swash_emoji.rs                          # Phase 1 PoC
└── src/
    ├── app.rs                                  # Phase 4-H: startup rasterizer selection + grid pass construction
    ├── window_host.rs                          # Phase 4-H: insert TerminalGridPass into frame draw order
    ├── settings.rs                             # Phase 4: FontEngine + font fields
    └── render/
        ├── mod.rs                              # Phase 4: Theme wiring; Phase 4-H: delete FONT_SIZE / FontFamily literal + painter.text
        ├── theme.rs                            # Phase 4: drop #[allow(dead_code)]
        ├── terminal_grid_pass.rs               # Phase 4-H: custom wgpu render pass
        ├── terminal_grid_pass.wgsl             # Phase 4-H: shader (or include_str! in terminal_grid_pass.rs)
        └── font/
            ├── mod.rs                          # Phase 2
            ├── traits.rs                       # Phase 2: GlyphRasterizer, GlyphBitmap, AtlasFormat
            ├── cache.rs                        # Phase 2: glyph cache; Phase 5: perf log
            ├── atlas.rs                        # Phase 2: alpha + rgba regions
            ├── ab_glyph_adapter.rs             # Phase 2
            ├── swash_adapter.rs                # Phase 3
            ├── resolver.rs                     # Phase 3: fontdb + bundled
            └── fallback.rs                     # Phase 3: chain + memo
```

## Testing Strategy

- **Unit**: TS-font-1..12 (in the relevant module). New unit tests target ≥ 80% on the new `render/font/` module.
- **Integration**: TS-font-int-1..3 (example binary + headless render smoke).
- **E2E**: Not applicable. tauri-driver cannot attach to a winit window without WebKit; the existing E2E suite continues to run as a regression gate but no new specs are added.
- **Manual host gates**: TS-manual-font-* (Linux X11 primary; Windows deferred-compatible).
- **Workspace regression**: `cargo test --workspace` stays at parity with the Phase 4-G ~1985-test baseline.

## Dependencies

| Package | Version | Purpose |
|---|---|---|
| swash | `= 0.1.x` (pinned) | Font parsing + shaping + scaling |
| zeno | latest compatible | Vector path → bitmap rasterizer |
| fontdb | latest compatible with swash | System font enumeration |
| ab_glyph | already transitive via egui | Retained as fallback rasterizer |

Existing pinned: `egui = "0.29"`, `egui-wgpu = "0.29"`, `winit = "0.30.9"`, `wgpu = "22"`.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| swash migration breaks egui text layout | Low | High | egui's text path is no longer extended (Option 3); the new `TerminalGridPass` is independent of egui's font store. `font_engine: AbGlyph` stays as a live escape hatch |
| swash single-maintainer churn | Medium | Medium | Pin `swash = "= 0.1.x"`; keep cosmic-text route open as a fallback path |
| Bundled-font binary-size growth (~20 MB) | Medium | Low-medium | Subset evaluation deferred until after Phase 1 PoC sizes the impact (Open Question) |
| `TerminalGridPass` PoC fails on G1 / G2 | Medium | High | Declared PoC failure path is documented in Phase 4-H (revert the pass; re-evaluate cosmic-text / Vello / set_fonts CJK-only). Existing `painter.text()` path stays alive until G1 + G2 are met, so the PoC failure mode does not break the terminal |
| Custom shader path complexity (atlas-page branching, decoration rendering) | Medium | Medium | Mirror the proven `image/overlay.rs` pattern (pipeline + bind group + instance buffer + LoadOp::Load). Selection / decorations may stay on egui side for the initial PoC if shader-side wiring slows down G1 / G2 validation |
| Emoji cell-width drift (2-cell expectation) | Low | Medium | Trust `unicode-width`; add a width-integrity unit test when wiring the renderer (Phase 3) |
| Bundled font file missing at build | Low | High (build break) | `include_bytes!` fails compile; CI catches |
| Bundled font corrupted at runtime | Low | Low | fontdb register fails → warn-log + base-only fallback |
| Windows host not available | Medium | Low | Formally defer with a clear next-step; not a blocker for Linux X11 acceptance |

## Open Questions

- [ ] (NFR1 detail) Bundled font subset strategy — full distribution vs. subset by script. Deferred until after Phase 1 PoC sizes the binary impact. Not blocking.
- [x] (FR3 detail) Whether the RGBA atlas can be sampled by egui's existing pipeline or requires a custom wgpu render pass. **Resolved 2026-05-15**: a custom wgpu render pass (`TerminalGridPass`, FR12) is built from scratch. egui's pipeline is not extended.
- [ ] (Phase 5) Windows host availability for `TS-manual-font-windows`. If unavailable at verify time, document defer with next-step.
- [ ] (Phase 4-H detail) Whether selection / underline / strikethrough render inside `TerminalGridPass` or stay on the egui rect path during the PoC. Decided per-shader-iteration; not a blocker for the G1 / G2 gates.

## Success Metrics

- [ ] Go / No-Go gates G1–G5 (SPEC.md §Success Criteria) all met. G1 (Noto Color Emoji) and G2 (CJK) are the headline gates; G4 (< 500 ms scan) and G5 (< 5 ms / glyph) are hard perf gates.
- [ ] All FR1–FR12 acceptance criteria in SPEC.md satisfied.
- [ ] All NFR1–NFR6 acceptance criteria in SPEC.md satisfied.
- [ ] `cargo test --workspace` PASS (parity with the Phase 4-G ~1985-test baseline; new tests add to this count).
- [ ] `cargo fmt --all` clean; `cargo clippy --workspace -- -D warnings` zero warnings.
- [ ] Renderer reads `Theme::font_family` and `Theme::font_size_pt`; any residual `#[allow(dead_code)]` on those fields is removed.
- [ ] `painter.text()` calls removed from `render/mod.rs::draw_grid`; `FONT_SIZE` + `FontFamily::Monospace` deleted.
- [ ] Linux X11 manual gate confirms Japanese + color emoji rendering with screenshots.
- [ ] `EMTERM_FONT_PERF=1` confirms NFR1 (< 500 ms) and NFR2 (< 5 ms / glyph).
- [ ] `tmp/restruct.md` Phase 4-H row flipped on completion.
