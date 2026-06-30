# Implementation Plan: COLRv1 Vector Emoji Rendering

## Overview

Replace the bundled CBDT bitmap emoji font with `Noto-COLRv1.ttf` and add a
new vector rasterization path (`skrifa` paint-graph traversal + `tiny-skia`
filling) that bypasses swash for COLRv1 glyphs. CJK / Latin / Symbols /
monochrome emoji stay on swash unchanged.

## Objectives

- Sharpen color emoji at fractional DPI on Windows (1.25× / 1.5× / 2.0×).
- Replace `NotoColorEmoji.ttf` (10.7 MiB, CBDT) with `Noto-COLRv1.ttf`
  (~5 MiB) atomically in the same change set.
- Preserve every non-emoji rasterization path bit-for-bit.
- Keep the monochrome `NotoEmoji-Regular` fallback reachable via the
  existing `FallbackChain` when the COLRv1 path returns `None`.

## Prerequisites

### Development Environment

- Rust toolchain pinned by `rust-toolchain.toml` (workspace root).
- `bash`, `curl` (or `wget`), `sha256sum` for `scripts/fetch-fonts.sh`.

### Dependencies

- `resvg 0.44` (already a GUI dep) — transitively links `skrifa 0.20.0`
  and `tiny-skia 0.11.4`. Promoting both crates to direct GUI deps does
  not increase the link footprint.
- The `gui` cargo feature must be on for everything in `src-tauri/src/render/`;
  the new module is automatically gated under that feature by placement.

## Architecture Overview

### Technology Stack

- **Language**: Rust (edition = "2024")
- **Existing font stack**: `swash` 0.1.18, `zeno` 0.2, `fontdb` 0.21,
  `ab_glyph` 0.2, `resvg` 0.44 (GUI-only)
- **New direct dependencies (GUI-only)**:
  - `skrifa` 0.20 — COLR table reader + `ColorPainter` callback engine
  - `tiny-skia` 0.11 — `Pixmap` rasterizer + gradient shaders + blend modes
- **Bundled font**: `Noto-COLRv1.ttf` from `googlefonts/noto-emoji v2.051`
  (the same tag the existing `NotoColorEmoji.ttf` ships from)

### Design Approach

The rasterization branch is added as a fast-path inside
`SwashRasterizer::raster`: when the registered font is flagged
`is_colrv1_emoji`, the call diverts to `colrv1_painter::rasterize`
before the existing swash code runs. All other engine traits
(`shape`, `font_metrics`, `has_color`, `has_codepoint`) keep their
existing swash backing — shaping a COLRv1 glyph still uses swash's
cmap + GSUB, and the `GlyphCache` / `Atlas` / `FallbackChain` see no
shape change.

Resolution: Pixmap dimensions are `ceil(target_cell_h_px)` (the base
text font's cell height = `ascent + descent` at `size_px`) when the
caller passes a positive value, otherwise the legacy `ceil(size_px)`
fallback. The emoji renders into the inner `(dim - 2) × (dim - 2)`
square (1 px padding on each side; skipped when `dim < 4`) and is
fitted via bbox-aware uniform scaling: `ColorGlyph::bounding_box`
provides the actual font-unit bbox and `scale = inner / max(bbox_w, bbox_h)`
preserves aspect ratio. The font_units → pixel transform combines this
`scale` with a Y-flip and centering translation so the bbox lands
centered inside the inner area. When the font has no ClipBox for the
glyph, the EM box `(0, 0, upem, upem)` is used as the fallback bbox.

### Component Interaction

```
codepoint
  → FallbackChain selects FontId               (unchanged)
  → SwashRasterizer::shape (swash shaper)      (unchanged)
  → GlyphCache lookup                          (unchanged)
        miss
         ▼
  → SwashRasterizer::raster
        ├─ font.is_colrv1_emoji = true ──► colrv1_painter::rasterize
        │                                       skrifa → ColorPainter callbacks
        │                                       tiny_skia Pixmap fill
        │                                       un-premultiply → Vec<u8>
        │                                  ──► Option<RasterizedColorGlyph>
        │
        └─ else ─────────────────────────────► existing swash Render path
  → GlyphBitmap (same shape as today)          (unchanged)
  → Atlas upload                               (unchanged)
  → wgpu sampling                              (unchanged)
```

When the COLRv1 path returns `None` (no paint graph for the requested
`glyph_id`), the cache stores `Slot::Missing` and the `FallbackChain`
descends to the next font in the chain — that lands on
`NotoEmoji-Regular` (monochrome outline), exactly as it does today
when CBDT misses.

## Implementation Phases

The SPEC asks for a single atomic PR / commit set so that the bundled
font swap and the new rasterization path never ship apart. The
implementation is sequenced internally as four sub-phases for
review-ability and incremental local testing, but every phase below
must land together.

---

### Phase 1: Dependencies & Font Asset Swap

**Goal**: Reach a buildable state with `Noto-COLRv1.ttf` bundled and the
new direct deps declared. No new code paths yet — only data + manifest
changes. After this phase the GUI build links the new font through the
existing swash path (which still routes via `Source::ColorOutline(0)` for
COLR fonts), so emoji render but not yet through the new painter.

**Files to Create**:

- `src-tauri/assets/fonts/Noto-COLRv1.ttf` — fetched via
  `scripts/fetch-fonts.sh` (gitignored asset; not committed).

**Files to Modify**:

- `src-tauri/Cargo.toml`
  - Add `skrifa = { version = "0.20", optional = true }` to
    `[dependencies]`.
  - Add `tiny-skia = { version = "0.11", optional = true }`
    to `[dependencies]`.
  - Append both deps to the `gui` feature list so they only link in
    GUI builds (mirrors `swash`, `zeno`, `fontdb`).
- `scripts/fetch-fonts.sh`
  - Remove the `NotoColorEmoji.ttf` `fetch_one` block.
  - Add a `fetch_one` block for `Noto-COLRv1.ttf` with:
    - URL: `https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/Noto-COLRv1.ttf`
    - SHA256: `0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2`
- `src-tauri/assets/fonts/README.md`
  - Replace the `NotoColorEmoji.ttf` inventory row with a
    `Noto-COLRv1.ttf` row carrying the new SHA256.
- `src-tauri/src/render/font/resolver.rs`
  - Update the `BUNDLED_EMOJI_COLOR_FONT` `include_bytes!` path from
    `NotoColorEmoji.ttf` to `Noto-COLRv1.ttf`. Keep the constant name
    and its doc-comment language tweaked from "CBDT / COLR" to
    "COLRv1 + glyf".
  - Update the `(bundled)` family-name registration comment.
- `src-tauri/assets/fonts/.gitignore`
  - Ensure `Noto-COLRv1.ttf` is covered (existing pattern matches all
    .ttf already; verify).

**Files to Delete**:

- `src-tauri/assets/fonts/NotoColorEmoji.ttf` (after fetch verifies
  the new file is present; the file is .gitignored so this is a
  local-FS deletion only).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `fetch-fonts.sh` | Idempotent HTTPS download with SHA256 pin | `curl`/`wget` + `sha256sum` available | `assets/fonts/Noto-COLRv1.ttf` exists; CBDT file removed |
| `Cargo.toml [features].gui` | Gate `skrifa`/`tiny-skia` to GUI builds | `gui` feature is the default-on profile | CLI build does not link skrifa/tiny-skia directly |
| `BUNDLED_EMOJI_COLOR_FONT` | `&[u8]` reference to the bundled emoji TTF | `Noto-COLRv1.ttf` is present at build time | Linked into the binary; family name resolves to "Noto Color Emoji" |

**Implementation Steps**:

1. **Pin the font source** — update `fetch-fonts.sh` with the v2.051
   Noto-COLRv1 URL and SHA256.
2. **Refresh local copy** — run `bash scripts/fetch-fonts.sh` to fetch
   the new font and verify the pin succeeds.
3. **Remove the old asset** — delete `NotoColorEmoji.ttf` from the
   working tree.
4. **Re-point the `include_bytes!` constant** — change
   `BUNDLED_EMOJI_COLOR_FONT` in `resolver.rs` to the new file name.
5. **Add direct cargo deps** — append `skrifa` and `tiny-skia` to
   `[dependencies]` and to `features.gui`.
6. **Update the bundled-fonts README** — swap the inventory row.

**Dependencies**: Independent (no upstream phase).

**Testing Approach**:

- Build: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  (GUI build) must compile with the new constant pointing at the new file.
- Build: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  (CLI) must still compile; this phase touches no CLI code.
- Fetch round-trip: re-run `bash scripts/fetch-fonts.sh` after the
  file is present — must report `Noto-COLRv1.ttf up-to-date`.

**Acceptance Criteria**:

- [ ] `bash scripts/fetch-fonts.sh` succeeds against a clean
  `assets/fonts/` directory and produces a file whose `sha256sum`
  matches the pinned value.
- [ ] `cargo check` (GUI) passes after the constant re-point.
- [ ] `cargo check --no-default-features` (CLI) passes.
- [ ] `NotoColorEmoji.ttf` is removed from the local working tree.

**Estimated Effort**: small (1 short session).

---

### Phase 2: `colrv1_painter` Module

**Goal**: Land the pure rasterizer — `is_colrv1_emoji` probe +
`rasterize` entry point + a `ColorPainter` implementation backed by
`tiny-skia`. No `swash_adapter` change yet; module compiles and unit
tests pass standalone.

**Files to Create**:

- `src-tauri/src/render/font/colrv1_painter.rs`

**Files to Modify**:

- `src-tauri/src/render/font/mod.rs`
  - Add `pub mod colrv1_painter;` underneath the existing `pub mod`
    block.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `is_colrv1_emoji(bytes) -> bool` | Probe: parses bytes as a font, checks COLR table is version 1, and verifies cmap covers a canonical emoji codepoint | `bytes` is a candidate font buffer (any byte slice; may be malformed) | `true` only when the font is a usable COLRv1 emoji font; `false` for CBDT-only, monochrome outline, or non-fonts |
| `rasterize(font_bytes, glyph_id, size_px, target_cell_h_px) -> Option<RasterizedColorGlyph>` | Drive `skrifa::color::ColorGlyph::paint` with a `TinySkiaPainter`; size the Pixmap to `ceil(target_cell_h_px)` (or `ceil(size_px)` when `target_cell_h_px <= 0`); fit the glyph to the inner padded area via bbox-aware scaling; return packed RGBA + bearings + advance | `glyph_id > 0`, `size_px >= 1.0` (the single guard `!(size_px >= 1.0)` rejects NaN / zero / negative / sub-1.0), `font_bytes` parses as a font | `Some(_)` when paint graph exists and Pixmap allocation succeeded; `None` otherwise |
| `RasterizedColorGlyph` (struct) | Owned output: width, height, bearing_left, bearing_top, advance, pixels (RGBA straight alpha) | None | `pixels.len() == width * height * 4`; `advance == width as f32` (pinned to pixmap dim so the wide-cell renderer's `sx = cell_w / advance` resolves to 1.0) |
| `TinySkiaPainter` (private struct) | `ColorPainter` impl that materializes paint callbacks into one `tiny-skia::Pixmap` | Constructed with a target Pixmap + font_units → pixel scale | Pixmap ends populated with the glyph after `ColorGlyph::paint` returns `Ok` |
| `un_premultiply(rgba_premul)` (private fn) | Convert premultiplied RGBA to straight alpha in-place | Input is `&mut [u8]` of length `4N`, premultiplied | Each pixel is divided-and-clamped to straight alpha; `a == 0` zeroes RGB |
| Composite-mode debouncer | Log at most once per unsupported `CompositeMode` | `OnceLock<Mutex<HashSet>>` (or equivalent) | No log spam from emoji glyphs that reuse the same unsupported mode every frame |

**Processing Flow** (`rasterize`, diagram-convertible):

1. Validate inputs.
   - `glyph_id == 0` → return `None`.
   - `!(size_px >= 1.0)` (rejects NaN, zero, negative, and sub-1.0 in
     one branch) → return `None`.
2. Parse the font.
   - Skrifa FontRef from `font_bytes`; failure → `None`.
   - Read `upem = face.head().units_per_em()`; `upem == 0` → `None`.
3. Look up the paint graph.
   - `MetadataProvider::color_glyphs().get_with_format(glyph_id, ColrV1)`.
   - Missing → return `None` (the caller — `swash_adapter::raster` —
     emits the `info` fallback log). The cache stores the miss and the
     `FallbackChain` descends to the next font.
4. Compute pixmap dimensions (FR8).
   - If `target_cell_h_px > 0`: `dim = max(1, ceil(target_cell_h_px))`.
   - Else (legacy fallback used by isolated unit tests):
     `dim = max(1, ceil(size_px))`.
   - 1 px padding on every side; the emoji renders into the inner
     `(dim - 2) × (dim - 2)` square. For tiny dims (`dim < 4`) padding
     is skipped so the inner stays positive.
   - `Pixmap::new(dim, dim)` on failure → `warn_once` log and return `None`.
5. Compute the font→pixel base transform with bbox-fit scaling (FR8).
   - Read `bbox_units = color_glyph.bounding_box(...)`; fall back to
     `BoundingBox { 0, 0, upem, upem }` (EM box) when absent.
   - `scale = inner / max(bbox_w, bbox_h)` (uniform — preserves aspect).
   - Center the scaled bbox inside the inner padded area; build a Y-flip
     `TsTransform::from_row(scale, 0, 0, -scale, tx, ty)` where
     `(tx, ty)` translate font-unit `(x_min, y_max)` to pixmap
     `(offset_x, offset_y_top)`.
6. Construct the `TinySkiaPainter` with:
   - A mutable borrow of the owned Pixmap.
   - The base transform from step 5.
   - An empty transform stack, empty clip stack, empty layer stack,
     and a `PaletteCache` built from the font's CPAL table.
7. Run the traversal.
   - `color_glyph.paint(LocationRef::default(), &mut painter)`.
   - On `PaintError` → log `warn` and return `None`.
8. Materialize the bitmap.
   - Take ownership of the Pixmap pixel buffer (`pixmap.take()`).
   - Convert premultiplied → straight alpha in place.
9. Report bearings + advance.
   - `bearing_left = 0` (the bbox-fit centering already places the glyph
     correctly inside the square).
   - `bearing_top = baseline_y.ceil() as i32` where `baseline_y = ty`
     from step 5 (`swash_adapter::raster` overrides this to the base
     font's ascent so the bitmap top aligns with the cell top).
   - `advance = dim as f32` (pinned to pixmap dim so the wide-cell
     renderer's `sx = cell_w / advance` resolves to 1.0 — no
     re-scaling, no re-centering, no asymmetric padding).
10. Return `Some(RasterizedColorGlyph { width: dim, height: dim,
    bearing_left, bearing_top, advance, pixels })`.

**Processing Flow** (`is_colrv1_emoji`):

1. Try to parse as a font; failure → `false`.
2. Open the COLR table; missing → `false`.
3. Read COLR version; `!= 1` → `false`.
4. Probe cmap for U+1F600; if not covered → `false`.
5. Otherwise → `true`.

**ColorPainter callback responsibilities** (each callback maps one
skrifa primitive to one tiny-skia operation):

| skrifa callback | tiny-skia translation |
|-----------------|-----------------------|
| `push_transform(Transform)` | Multiply onto the top of the painter's transform stack |
| `pop_transform()` | Pop the top transform |
| `push_clip_glyph(GlyphId)` | Build a `tiny_skia::Path` from skrifa outline pen output for the inner glyph, push onto the clip stack |
| `push_clip_box(BoundingBox<f32>)` | Convert to `Rect`, push onto the clip stack |
| `pop_clip()` | Pop the top clip |
| `fill(Brush::Solid)` | `Paint` with solid color, `fill_path` of the current clip path with `BlendMode::SourceOver` |
| `fill(Brush::LinearGradient)` | Build `LinearGradient` shader from `ColorStop` list + matrix, fill clip path |
| `fill(Brush::RadialGradient)` | Build `RadialGradient` shader from `c0` (focal) + `c1` (center) + `r1` (outer radius); when `r0 > 0` emit `warn_once` and drop `r0` (tiny-skia 0.11 has no two-circle radial form), fill clip path. Empty stops or zero `r1` → fall back to first-stop solid color |
| `fill(Brush::SweepGradient)` | tiny-skia 0.11 has no sweep shader. Fall back to first-stop `Solid` + `warn_once` log (acceptable because `Noto-COLRv1.ttf` uses sweep gradients on a small minority of glyphs) |
| `push_layer(CompositeMode)` | Allocate an offscreen `Pixmap` of the same size, redirect subsequent fills to it |
| `pop_layer()` | Composite the offscreen onto the parent Pixmap using `BlendMode` mapped from `CompositeMode`; unmapped mode → `SourceOver` + once-per-mode `warn` log |
| `paint_cached_color_glyph` | Leave as the default (`PaintCachedColorGlyph::Unimplemented`); skrifa will reissue the nested paint graph through the normal callbacks |

**CompositeMode → BlendMode mapping** (subset that tiny-skia 0.11
supports):

- `Clear`, `Src`, `Dest`, `SrcOver`, `DestOver`, `SrcIn`, `DestIn`,
  `SrcOut`, `DestOut`, `SrcAtop`, `DestAtop`, `Xor`, `Plus`,
  `Multiply`, `Screen`, `Overlay`, `Darken`, `Lighten`, `ColorDodge`,
  `ColorBurn`, `HardLight`, `SoftLight`, `Difference`, `Exclusion` →
  map 1:1 to `tiny_skia::BlendMode`.
- Modes tiny-skia cannot express (notably the HSL family: `HslHue`,
  `HslSaturation`, `HslColor`, `HslLuminosity`) → fall back to
  `BlendMode::SourceOver` and log `warn` once per mode.

**Implementation Steps** (5–7 max):

1. **Module skeleton + public surface** — declare `RasterizedColorGlyph`,
   the two `pub fn` entry points, and the `pub mod` registration in
   `mod.rs`. Stub bodies returning `None` / `false`.
2. **Implement `is_colrv1_emoji`** — open the font, inspect the COLR
   table version, probe cmap for U+1F600. Add the first three unit
   tests (accept Noto-COLRv1, reject mono emoji, reject CBDT fixture).
3. **Implement `un_premultiply`** — port the arithmetic from
   `emoji_resample.rs` (premultiplied → straight, clamped). Add the
   three premultiply unit tests.
4. **Implement `TinySkiaPainter`** — wire the transform / clip / layer
   stacks (with `Option<Layer>` / `Option<Mask>` sentinels for
   allocation failures), the `PaletteCache` (CPAL → straight RGBA), and
   the solid + linear + radial + sweep gradient fills. Radial drops
   `r0 > 0` with `warn_once`; sweep falls back to first-stop solid with
   `warn_once`. `push_layer` / `pop_layer` use the CompositeMode mapping
   table with debounced `warn_once` for HSL family modes.
5. **Implement `rasterize`** — input validation
   (`glyph_id != 0`, `size_px >= 1.0`), paint-graph lookup,
   bbox-fit Pixmap sizing per FR8 (`dim = ceil(target_cell_h_px)` or
   legacy `ceil(size_px)`; 1 px padding; `ColorGlyph::bounding_box`
   centered fit; EM box fallback), traversal, un-premultiply, build
   `RasterizedColorGlyph` with `advance = dim`. Add the unit tests
   listed in SPEC §Unit Tests (premultiply ×3, probe ×3, rasterize
   success ×4, rasterize rejection ×3, target_pxs grid ×1,
   target_cell_h padding ×1, tiny-dim no-padding ×1).
6. **Audit for `unsafe` and `unwrap`** — confirm no new `unsafe`
   blocks and that every external API (`FontRef::new`,
   `Pixmap::new`, paint-graph traversal) is consumed via `?`,
   `match`, or `unwrap_or`.

**Dependencies**: Requires Phase 1 (Cargo deps + bundled font).
Blocks Phase 3.

**Testing Approach**:

- Unit: 16 cases listed in SPEC §Unit Tests (premultiply ×3, probe ×3,
  rasterize success ×4, rasterize rejection ×3, target_pxs grid ×1,
  target_cell_h padding/centering ×1, tiny-dim no-padding ×1).
- Integration: deferred to Phase 3.

**Acceptance Criteria**:

- [ ] All 16 unit tests in `colrv1_painter::tests` pass.
- [ ] `cargo check` (GUI) passes.
- [ ] No new `unsafe` blocks anywhere in the module.
- [ ] No `unwrap()` on font / paint-graph operations.

**Estimated Effort**: large (the ColorPainter implementation is the
bulk of this feature; sweep-gradient handling and the layer-composite
mapping are the highest-risk sub-tasks).

---

### Phase 3: `swash_adapter` Integration

**Goal**: Wire the new path into `SwashRasterizer::raster` so COLRv1
emoji glyphs flow through `colrv1_painter` and everything else stays
on swash.

**Files to Modify**:

- `src-tauri/src/render/font/traits.rs`
  - Add a new method `fn set_base_font(&self, _font: FontId) {}` on the
    `GlyphRasterizer` trait with a default no-op body so adapters that
    do not need the hint (e.g. `ab_glyph_adapter`) keep compiling
    untouched.
- `src-tauri/src/render/font/swash_adapter.rs`
  - Add `is_colrv1_emoji: bool` field to the private `SwashFont`
    struct (next to `has_color`).
  - Add `base_font: Option<FontId>` to `Inner` so the COLRv1 path can
    look up the base text font's ascent / cell height.
  - Implement `set_base_font` on `SwashRasterizer`:
    `self.inner.lock().base_font = Some(font)`.
  - Populate the `is_colrv1_emoji` flag in `ingest_font` via
    `super::colrv1_painter::is_colrv1_emoji(&bytes)`.
  - OR `has_color` with `is_colrv1_emoji` so the chain builder still
    sees Noto-COLRv1 as a colour source (swash's own `probe_color_support`
    returns `false` for it because the COLRv1 path bypasses swash).
  - In `raster`, branch on `swash_font.is_colrv1_emoji` before the
    existing swash code. Resolve `(base_ascent_px, base_cell_h_px)`
    from `Inner.base_font` and `size_px` while still holding the lock,
    then drop the `Inner` mutex guard before entering tiny-skia
    (the painter does no `Inner` access; holding the lock across
    rasterization would needlessly serialize all rasterize calls).
  - Call `colrv1_painter::rasterize(bytes, gid, size_px, base_cell_h_px)`.
  - Map `RasterizedColorGlyph` → `GlyphBitmap { format: AtlasFormat::Rgba, … }`,
    overriding `bearing.1` with `base_ascent_px.round() as i32` (when
    `base_ascent_px > 0`) so the bitmap top aligns with cell top — without
    this override the painter's `bearing_top` centers the emoji on the
    baseline, which bleeds above the line.
  - Add `log::info!` on fallback (`colrv1: fallback for gid={gid}, size_px={size_px}`).
    Hit-path `log::debug!` is acceptable but not required.
- `src-tauri/src/app.rs`
  - In `App::build_font_stack`, immediately before returning, call
    `rasterizer.set_base_font(base_id)` so subsequent `raster` calls
    can resolve the base text font's metrics.
- (No change required to `cache.rs`, `atlas.rs`, `fallback.rs`, or
  `resolver.rs` outside the `include_bytes!` swap already done in Phase 1.)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `SwashFont.is_colrv1_emoji` | Cached probe result | Set once at ingest by `colrv1_painter::is_colrv1_emoji` | Read-only during rasterize; `true` → divert to new path |
| `Inner.base_font` | Optional FontId of the renderer's base text font | Set by `set_base_font` from `App::build_font_stack` once at startup | COLRv1 dispatch reads it to compute `(base_ascent_px, base_cell_h_px)` at the call's `size_px` |
| `GlyphRasterizer::set_base_font` (default no-op + Swash impl) | Renderer-side hook for caching the base FontId | `App::build_font_stack` finished resolving the base id | `ab_glyph_adapter` keeps the default no-op; `SwashRasterizer` writes to `Inner.base_font` |
| `SwashRasterizer::raster` branch | Dispatch to colrv1_painter when the flag is set, passing the resolved `base_cell_h_px` | `Inner` lock held to look up the font + base metrics; released before calling the painter | Either returns the new path's `Option<GlyphBitmap>` (with `bearing.1` overridden to `base_ascent_px`) or falls through to the existing swash code |
| Fallback contract | `colrv1_painter::rasterize` returning `None` propagates as the rasterize call's `None` | `is_colrv1_emoji == true` for the font | `GlyphCache` stores `Slot::Missing`; `FallbackChain::resolve` walks to the next font |

**Processing Flow** (`raster`, after change):

1. Acquire `Inner` lock; clone the `SwashFont` entry for `font`.
   - Missing entry → return `None`.
2. Reject `glyph_id == 0`. (`size_px <= 0.0` is rejected by the painter
   via its stricter `!(size_px >= 1.0)` guard — for the legacy path
   below, swash's `Render` produces an empty image and `None` flows
   naturally.)
3. If `swash_font.is_colrv1_emoji`:
   - While still holding the lock, resolve
     `(base_ascent_px, base_cell_h_px)` from `Inner.base_font`:
     - `bf = inner.fonts.get(base_font_id)`; if missing or `upem == 0`,
       fall back to `(0.0, 0.0)` (painter's legacy `ceil(size_px)` path).
     - Otherwise compute `scale = size_px / upem`,
       `base_ascent_px = metrics.ascent * scale`,
       `base_cell_h_px = (metrics.ascent + metrics.descent) * scale`.
   - Drop the lock.
   - Call `colrv1_painter::rasterize(&swash_font.bytes, glyph_id, size_px, base_cell_h_px)`.
   - On `Some(r)` → build `GlyphBitmap { format: Rgba, … }` with
     `bearing.1 = base_ascent_px.round() as i32` when `base_ascent_px > 0`
     (else `r.bearing_top`); return `Some`.
   - On `None` → log `info` fallback, return `None`.
4. Else: execute the existing swash code path verbatim.

**Implementation Steps**:

1. **Extend the `GlyphRasterizer` trait** — add
   `fn set_base_font(&self, _font: FontId) {}` with a default no-op
   so unaffected adapters keep compiling.
2. **Extend `SwashFont`** — add `is_colrv1_emoji: bool` and update the
   one struct literal in `ingest_font`. Add `base_font: Option<FontId>`
   to `Inner`.
3. **Override `set_base_font` on `SwashRasterizer`** — write `font` into
   `self.inner.lock().base_font`.
4. **Wire `App::build_font_stack`** — call `rasterizer.set_base_font(base_id)`
   right before returning the constructed stack (single call site
   so the dependency is centralized).
5. **Probe at ingest** — call `colrv1_painter::is_colrv1_emoji(&bytes)`
   alongside `probe_color_support(&bytes)`; OR `has_color()` with
   `is_colrv1_emoji` so chain builders still see Noto-COLRv1 as colour.
6. **Add raster branch** — resolve `(base_ascent_px, base_cell_h_px)`
   from `Inner.base_font` while holding the lock; drop the lock;
   call `colrv1_painter::rasterize` with the 4-arg signature; map the
   result with `bearing.1` overridden to `base_ascent_px`.
7. **Add structured log** — `info` on fallback (`colrv1: fallback for
   gid={gid}, size_px={size_px} (no paint graph)`).
8. **Adjust the existing emoji-rgba unit test** — the test currently
   exercises swash's color path on `BUNDLED_EMOJI_COLOR_FONT`. After
   the swap that constant points at Noto-COLRv1, so the call routes
   through the new painter. Rename / re-comment the test so its
   intent is clear ("emoji bytes route through colrv1 path and return
   non-empty RGBA"); the assertions (`AtlasFormat::Rgba`, non-zero
   RGB) hold unchanged.

**Dependencies**: Requires Phase 2. Blocks Phase 4.

**Testing Approach**:

- Existing unit + integration tests in `swash_adapter::tests` must
  pass (CJK / ASCII / subpixel / `has_color` / `has_codepoint`).
- New integration tests added in Phase 4 exercise the routing.

**Acceptance Criteria**:

- [ ] `cargo test --lib` on `src-tauri/` passes.
- [ ] Emoji rasterization through `SwashRasterizer::raster` now yields
  `AtlasFormat::Rgba` from the colrv1 path (verified by Phase 4
  integration test).
- [ ] Non-emoji rasterization (`A`, CJK, subpixel) is bit-identical
  to pre-change output (covered by existing tests staying green).
- [ ] No `unsafe`, no new `unwrap()` calls in the branch.

**Estimated Effort**: small (one struct field + one branch + log
calls; the heavy lifting is in Phase 2).

---

### Phase 4: Tests & Verification

**Goal**: Add the SPEC-mandated integration tests, exercise the
`--no-default-features` and full GUI builds, and capture the manual
verification scenarios that gate sdd.6.

**Files to Create**: none (tests live next to the code they cover).

**Files to Modify**:

- `src-tauri/src/render/font/swash_adapter.rs::tests` — add three
  integration tests:
  - `emoji_routes_through_colrv1_path` — register the bundled
    Noto-COLRv1 bytes, call `raster()` for the smiley glyph, assert
    `AtlasFormat::Rgba`, non-zero RGB pixels, advance > 0.
  - `cjk_unchanged_after_colrv1_addition` — register CJK + emoji,
    rasterize `'A'` from CJK at 32 px, byte-compare to a pre-change
    fixture vector or assert against the unchanged `swash_rasters_ascii_alpha`
    invariants (format Alpha, non-empty, advance > 0).
  - `unknown_glyph_falls_back_to_chain` — request a codepoint in a
    PUA range (e.g. U+E000) on Noto-COLRv1; expect `raster()` to
    return `None` so the FallbackChain in production would descend to
    `NotoEmoji-Regular`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Integration test fixtures | Use `BUNDLED_EMOJI_COLOR_FONT` (now Noto-COLRv1) + `BUNDLED_CJK_FONT` | Phase 1–3 landed | Round-trip COLRv1 routing verified via the public `GlyphRasterizer` trait |
| Manual scenario script | Per-OS / per-DPI checklist captured in VERIFICATION.md | A release build available on the target machine | Manual sign-off recorded by sdd.6 |
| Binary-size measurement | `ls -l src-tauri/target-host/release/emterm` before / after | A release build is reproducible on the same host with identical features | Difference ≈ 5 MiB reported in VERIFICATION_RESULT.md |

**Implementation Steps**:

1. **Add the three integration tests** in `swash_adapter::tests`.
2. **Re-run the test suite** — `cargo test --lib` (full GUI) and
   `cargo check --no-default-features` (CLI).
3. **Local release build (manual; user-initiated only)** — the user
   runs `make build` to produce `src-tauri/target-host/release/emterm`.
   The agent reads the resulting binary size and computes the delta
   versus the pre-change baseline (~5 MiB target).
4. **Capture manual scenarios** in VERIFICATION.md (Windows 1.5× DPI,
   Linux 1.0×, RDP 1.0×, binary size).

**Dependencies**: Requires Phases 1–3.

**Testing Approach**:

- Unit: phases 2 & 3 cover everything in-tree.
- Integration: three tests added above.
- Manual: four scenarios (see VERIFICATION.md).

**Acceptance Criteria**:

- [ ] All three new integration tests pass.
- [ ] `cargo test --lib` on the full GUI build passes (no
  regressions in existing `swash_adapter::tests`).
- [ ] `cargo check --no-default-features` passes.
- [ ] Manual scenarios documented in VERIFICATION.md with explicit
  before/after artifacts (binary size delta + glyph screenshots
  versus reference PNGs under `tmp/verify-emoji/out/`).

**Estimated Effort**: small.

---

## Complete File Structure

Only files touched by this feature are listed; everything else under
`src-tauri/` is unchanged.

```
src-tauri/
├── Cargo.toml                                    # +skrifa, +tiny-skia (gui-only optional)
├── build.rs                                      # failsafe list updated to Noto-COLRv1.ttf
├── assets/
│   └── fonts/
│       ├── Noto-COLRv1.ttf                       # NEW (~5 MiB)
│       ├── NotoColorEmoji.ttf                    # DELETED (local FS only; was .gitignored)
│       ├── README.md                             # inventory row swapped
│       └── …other fonts unchanged
├── src/
│   ├── app.rs                                    # build_font_stack calls rasterizer.set_base_font(base_id)
│   └── render/
│       └── font/
│           ├── colrv1_painter.rs                 # NEW (skrifa + tiny-skia painter, 4-arg rasterize)
│           ├── mod.rs                            # +pub mod colrv1_painter
│           ├── resolver.rs                       # include_bytes! path updated, doc-comment refreshed
│           ├── swash_adapter.rs                  # +SwashFont.is_colrv1_emoji, +Inner.base_font,
│           │                                       set_base_font impl, raster branch with cell-h
│           │                                       lookup + ascent override
│           └── traits.rs                         # +GlyphRasterizer::set_base_font default no-op
scripts/
└── fetch-fonts.sh                                # CBDT entry removed, COLRv1 entry added
doc/tasks/colrv1-emoji-vector-rendering/
├── SPEC.md                                       # (already exists)
├── 要件定義書.md                                  # (already exists)
├── IMPLEMENTATION.md                             # this file
├── VERIFICATION.md                               # NEW
├── VERIFICATION_RESULT.md                        # NEW (sdd.6)
├── tasks.yaml                                    # NEW
└── sdd.yaml                                      # requirements.tasks / tests populated
```

## Testing Strategy

- **Unit (Phase 2)**: 16 tests in `colrv1_painter::tests` — premultiply
  arithmetic (×3), COLR-table probe (×3), rasterize success (×4),
  rasterize rejection (×3), target_px grid (×1), target_cell_h padding
  + centering (×1), tiny-dim no-padding (×1). Target coverage for new
  module: > 80 % statement coverage (informal, not measured).
- **Integration (Phase 4)**: 3 tests in `swash_adapter::tests` —
  end-to-end routing, CJK invariance, unknown-glyph fallback.
- **Build verification**: `cargo check` (GUI, default features) and
  `cargo check --no-default-features` (CLI) — neither path may regress.
- **E2E**: not applicable (project has no E2E framework today; the
  SPEC explicitly notes "Run command: Not detected").
- **Manual**: Windows 1.5× DPI sharpness; Linux 1.0× regression check;
  RDP 1.0× regression check; binary-size delta — captured in
  VERIFICATION.md.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `skrifa` | `0.20` | COLR table reader; `ColorGlyphCollection::get_with_format(_, ColrV1)`; `ColorGlyph::paint(LocationRef, &mut ColorPainter)` |
| `tiny-skia` | `0.11` | `Pixmap`, `Paint`, `Path`/`PathBuilder`, `LinearGradient`/`RadialGradient`, `BlendMode` |
| (already linked) `resvg` | `0.44` | Brings both crates in transitively today — direct adds do not change link footprint |
| (no change) `swash` | `=0.1.18` | Continues to drive CJK / Latin / Symbols / monochrome emoji |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `tiny-skia 0.11` does not support sweep gradients natively | High | Medium — small visual regression on a minority of emoji using sweep fills | Implement a sampled sweep fill on the Pixmap; if cost exceeds budget, fall back to first-stop solid + warn log (still better than CBDT blur at fractional DPI) |
| HSL composite modes have no tiny-skia equivalent | Medium | Low — affects a few Unicode 14+ flag / clock glyphs | Map unsupported modes to `SourceOver` with a once-per-mode `warn` log; render still produces a coherent glyph |
| Deep `PaintColrGlyph` nesting causes traversal stack growth | Low | Low | skrifa's own cycle detection (`VisitedSet`) terminates loops; an `info` log fires if we hit a depth ceiling we add for defense (cap at 32) |
| `Noto-COLRv1.ttf` family-name change vs. CBDT version | Low (verified `"Noto Color Emoji"` is identical) | Low | Confirmed via OT name-table inspection during planning; existing `by_family` tests stay valid |
| Sweep-gradient implementation slows first-rasterize beyond 10 ms | Low | Low — only affects miss path; subsequent frames hit the cache | Performance budget is informal (NFR1 is a guideline); GlyphCache absorbs the cost on every subsequent draw |
| User-installed COLRv1 emoji on Windows (24 MB Noto Color Emoji) accidentally activates new path | Low (host font is NOT in our chain by default) | Low | The bundled font wins via `register_bundled` ordering; if the user explicitly points at the host font and it is COLRv1, the new path renders it (intended behavior, not a regression) |

## Open Questions

- [ ] Sweep-gradient fallback choice — implement sampled sweep on
  Pixmap, or accept first-stop solid with warn log? Decision deferred
  to Phase 2 implementation when actual emoji-glyph impact is
  measurable. The SPEC accepts either as long as it does not silently
  drop the glyph.
- [ ] Whether the `is_colrv1_emoji` probe's cmap check should accept
  emojis beyond U+1F600 to be robust to future Noto-COLRv1 builds
  that might drop that codepoint. Decision: ship with the
  single-codepoint probe (matches `probe_color_support`'s pattern);
  revisit if a future font update breaks it.
- [ ] Whether to add a feature-flag rollback path (env var to force
  the old swash COLR route). Decision: no — the SPEC says the swap is
  atomic, and the bundled font no longer has CBDT for swash to read.

## Success Metrics

- [ ] All 14 unit tests + 3 integration tests pass.
- [ ] `cargo check` (GUI) and `cargo check --no-default-features`
  (CLI) both pass.
- [ ] Release binary shrinks by ~5 MiB on Linux x86_64.
- [ ] No new `unsafe` blocks anywhere in the change set.
- [ ] No `unwrap()` on font / paint-graph operations in
  `colrv1_painter` or the new branch in `swash_adapter`.
- [ ] Windows 1.5× DPI rendering visually matches the C-variant
  reference PNGs under `tmp/verify-emoji/out/`.
- [ ] Linux 1.0× and RDP 1.0× show no regression versus current main.
