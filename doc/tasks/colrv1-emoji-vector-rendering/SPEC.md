# Feature: COLRv1 Vector Emoji Rendering

## Overview

Replace the current swash CBDT-bitmap-strike emoji rasterization path with a
vector-direct COLRv1 path that uses `skrifa` (Google fontations) for paint
graph traversal and `tiny-skia` for rasterization. The new path eliminates
the bitmap-downscale step that causes fractional-DPI blur on Windows and
shrinks the bundled emoji font from 10.7 MiB (CBDT) to ~5 MiB (COLRv1).

## Objectives

- Sharpen color emoji rendering at all DPI scales on Windows (1.25× / 1.5× / 2.0×)
- Replace the bundled `NotoColorEmoji.ttf` (CBDT) with `Noto-COLRv1.ttf` (COLRv1 + glyf) in the same change
- Avoid regressions for CJK / Latin / Symbols / monochrome-emoji paths (these stay on swash)
- Lay the groundwork for moving emoji rasterization off swash 0.1.18 onto `skrifa`

## User Stories

### US1: Crisp emoji at fractional DPI

As a Windows user running at 1.5× DPI, I want emoji in my terminal to render
without blur, so that 😀 🚀 ❤️ 🌍 👍🏽 look as crisp as text glyphs.

**Acceptance Criteria:**
- [ ] At target_px 26 (Windows 1.5×) the rendered glyph matches the C-variant
  reference output recorded in `tmp/verify-emoji/out/compare3_*_26px.png`
  (edges sharp, no soft blur, no color muddying)
- [ ] No visible regression at target_px 17 (Linux 1.0×) or 21 (Windows 1.25×)
- [ ] RDP 1.0× shows no regression

### US2: No regression for non-emoji glyphs

As any user, I want CJK / Latin / Symbol rendering to stay identical, so that
no other text is affected by the emoji-path change.

**Acceptance Criteria:**
- [ ] Existing swash unit tests pass unchanged
- [ ] Monochrome glyphs (Latin, CJK, Symbols, `NotoEmoji-Regular`) still flow
  through swash with no path divergence
- [ ] Subpixel-AA and faux-bold paths are untouched

### US3: Smaller bundle

As a packager / user, I want a smaller terminal binary, so that the deb /
nsis artifacts are lighter.

**Acceptance Criteria:**
- [ ] `ls -l src-tauri/target-host/release/emterm` shrinks by approximately 5 MiB
- [ ] The bundled emoji asset list contains `Noto-COLRv1.ttf` and no longer
  contains `NotoColorEmoji.ttf`

## Technical Requirements

### Functional Requirements

- **FR1 — COLRv1 paint graph rasterization:** Implement a new module
  `src-tauri/src/render/font/colrv1_painter.rs` that, given (font_bytes,
  glyph_id, size_px, target_cell_h_px), resolves the COLRv1 paint graph
  via `skrifa::color` and rasterizes it into a tiny-skia `Pixmap` sized
  to `ceil(target_cell_h_px)` (or `ceil(size_px)` when
  `target_cell_h_px <= 0.0` — the fallback used by isolated unit tests).
  See FR8 for the full pixmap-sizing rule (1 px padding + bbox-fit).
- **FR2 — Font path routing:** Add an `is_colrv1_emoji: bool` flag to
  `SwashFont` populated at registration time by probing the COLR table
  (version == 1). When `raster()` is called and the flag is set, dispatch
  to the new path; otherwise keep the existing swash code path verbatim.
- **FR3 — Premultiplied-to-straight alpha conversion:** Convert tiny-skia's
  premultiplied RGBA output to straight alpha before constructing the
  returned `GlyphBitmap`, matching the format expected by the existing
  atlas.
- **FR4 — Bundled font swap:** Replace
  `src-tauri/assets/fonts/NotoColorEmoji.ttf` with
  `src-tauri/assets/fonts/Noto-COLRv1.ttf` and update the `include_bytes!`
  / font-registration site accordingly. Update the chain so the
  monochrome `NotoEmoji-Regular` fallback still sits after the COLRv1 font.
- **FR5 — `fetch-fonts.sh` update:** Remove the `NotoColorEmoji.ttf` entry
  from `scripts/fetch-fonts.sh` and add a pinned entry for
  `Noto-COLRv1.ttf`. URL = `https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/Noto-COLRv1.ttf`、
  SHA256 = `0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2`
  (`googlefonts/noto-emoji v2.051` — 既存 `NotoColorEmoji.ttf` と同 tag)。
- **FR6 — Monochrome fallback preserved:** If the COLRv1 path cannot
  produce a paint graph for a given glyph_id (e.g. font does not cover the
  codepoint), return `None` so the existing `FallbackChain` descends to
  `NotoEmoji-Regular` exactly as it does today.
- **FR7 — Path-selection logging:** Emit `info` log on fallback events
  (paint graph missing for a non-zero glyph_id). COLRv1 hit-path logging
  stays at `debug` to avoid noise. Release builds drop `debug`, so this
  only matters during local diagnosis. Degraded-paint events
  (sweep gradient fallback, radial gradient with `r0 > 0`, unsupported
  composite mode, paint-stack allocation failure) emit a `warn` log
  exactly once per process via `OnceLock`-based debouncers.

- **FR8 — Pixmap sizing with base cell height + 1 px padding + bbox-fit:**
  `colrv1_painter::rasterize` accepts a fourth parameter
  `target_cell_h_px` (= `ascent + descent` of the renderer's base text
  font at `size_px`). When `target_cell_h_px > 0`, the output pixmap is
  square with side `dim = ceil(target_cell_h_px)` and the emoji renders
  into the inner `(dim - 2) × (dim - 2)` area (1 px padding on each
  side); for tiny dims (`dim < 4`) padding is skipped so the inner area
  stays positive. The glyph is fitted to the inner area via
  bbox-aware uniform scaling: the COLRv1 `ColorGlyph::bounding_box` is
  read for the actual glyph extents and a single `scale` is chosen so
  the bbox fits inside the inner area (centered). When the font has no
  ClipBox entry for the glyph, the EM box `(0, 0, upem, upem)` is used
  as the fallback bbox. The returned `advance` is pinned to `dim`
  so the wide-cell renderer reserves exactly the square's footprint.
  `target_cell_h_px <= 0.0` selects the legacy "render at full
  `size_px`" fallback (used by isolated unit tests).

### Non-Functional Requirements

- **NFR1 — Performance:** First-time rasterization for a single glyph at a
  given size_px must complete within 10 ms on the target hardware
  (Windows x86_64, ~2025 era laptop). Subsequent renders hit
  `GlyphCache` and skip both skrifa and tiny-skia entirely.
- **NFR2 — Binary size:** Bundled fonts shrink by ~5 MiB
  (NotoColorEmoji 10.7 MiB → Noto-COLRv1 ~5 MiB).
- **NFR3 — Safety:** No new `unsafe` blocks. Both `skrifa` and `tiny-skia`
  are safe-Rust libraries (skrifa is the renderer Chrome uses in
  production).
- **NFR4 — Reproducibility:** Bundled font is SHA256-pinned in
  `scripts/fetch-fonts.sh` so CI and developer machines fetch byte-identical
  data.
- **NFR5 — Maintainability:** New module ships with unit tests covering
  premultiplied conversion and end-to-end emoji rasterization on bundled
  bytes. Cargo dependencies are pinned to a stable minor.

## Implementation Approach

### Architecture

**Module placement (delta only):**
```
src-tauri/src/render/font/
  swash_adapter.rs       (modified: branch in raster + new is_colrv1_emoji flag
                          + Inner.base_font + set_base_font impl)
  colrv1_painter.rs      (NEW: skrifa + tiny-skia)
  mod.rs                 (modified: pub mod colrv1_painter)
  resolver.rs            (modified: BUNDLED_EMOJI_COLOR_FONT path swap)
  traits.rs              (modified: + set_base_font default no-op method on
                          the GlyphRasterizer trait — GlyphBitmap shape stays)
  fallback.rs            (unchanged)
  cache.rs               (unchanged)
  atlas.rs               (unchanged)

src-tauri/src/
  app.rs                 (modified: build_font_stack calls
                          rasterizer.set_base_font(base_id) right before
                          returning the constructed stack)

src-tauri/assets/fonts/
  Noto-COLRv1.ttf        (NEW, ~5 MiB)
  NotoColorEmoji.ttf     (DELETED)
  NotoEmoji-Regular.ttf  (unchanged — monochrome fallback)
  …                      (CJK / Latin / Symbols unchanged)

scripts/fetch-fonts.sh   (modified: remove CBDT entry, add COLRv1 entry)
```

**System architecture (rasterization branch):**
```
GlyphCache miss
       │
       ▼
SwashRasterizer::raster(font, glyph_id, size_px)
       │   (Inner.base_font was set at startup by App::build_font_stack
       │    via the new GlyphRasterizer::set_base_font(base_id))
       │
       ├─ if font.is_colrv1_emoji ──► resolve (base_ascent_px, base_cell_h_px)
       │                              from Inner.base_font + size_px
       │                                   │
       │                                   ▼
       │                          colrv1_painter::rasterize(
       │                              bytes, gid, size_px, base_cell_h_px)
       │                                   │
       │                                   ▼
       │                          skrifa: FontRef → ColorGlyphs
       │                          skrifa: paint graph for glyph_id
       │                          skrifa: ColorGlyph::bounding_box → bbox-fit scale
       │                                   │
       │                                   ▼
       │                          ColorPainter impl → tiny-skia Pixmap
       │                            (dim × dim square, 1 px padding,
       │                             emoji centered in inner area)
       │                                   │
       │                                   ▼
       │                          un-premultiply → Vec<u8> (RGBA straight)
       │                                   │
       │                                   ▼
       │                          GlyphBitmap { format: Rgba,
       │                            bearing.1 overridden to base_ascent_px
       │                            so bitmap top aligns with cell top, … }
       │
       └─ else ─────────────────────► existing swash path (unchanged)
                                        ColorBitmap / ColorOutline / Outline
                                        sources via Render::new(...)
```

### Data Flow

```
codepoint
  → FallbackChain selects FontId (existing)
  → FontShaper produces glyph_id (existing — swash shaper, OK for COLRv1 too)
  → GlyphCache lookup (existing)
       miss
        ↓
  → SwashRasterizer::raster
       branch on font.is_colrv1_emoji
        ↓ true
  → colrv1_painter::rasterize
       skrifa::FontRef::from_index(bytes, 0)
       face.color_glyphs().get(glyph_id_with_format) → Paint
       ColorPainter walks paint graph, drawing into Pixmap
        ↓
  → un-premultiply Pixmap → Vec<u8>
  → GlyphBitmap returned
  → atlas upload (existing)
  → wgpu sampling (existing)
```

### API Design

This feature has no external API. Internal Rust API summary:

#### New module: `src-tauri/src/render/font/colrv1_painter.rs`

```rust
// PUBLIC
pub fn rasterize(
    font_bytes: &[u8],
    glyph_id: u32,
    size_px: f32,
    target_cell_h_px: f32, // base text font's (ascent + descent) at size_px;
                           // 0.0 selects the legacy "render at full size_px" fallback
) -> Option<RasterizedColorGlyph>;

pub struct RasterizedColorGlyph {
    pub width: u32,
    pub height: u32,
    pub bearing_left: i32,
    pub bearing_top: i32,
    pub advance: f32,
    pub pixels: Vec<u8>, // RGBA straight alpha, length = width * height * 4
}

// PUBLIC (probe)
pub fn is_colrv1_emoji(font_bytes: &[u8]) -> bool;
```

#### Internal: `GlyphRasterizer` trait extension

```rust
// src-tauri/src/render/font/traits.rs
pub trait GlyphRasterizer: Send + Sync {
    // ... existing methods ...

    /// Record which font the renderer treats as the base text font.
    /// Engines with a COLRv1 path use this to size emoji to the base
    /// font's cell height (`ascent + descent`). Default no-op for engines
    /// that do not need the hint (e.g. ab_glyph).
    fn set_base_font(&self, _font: FontId) {}
}
```

`SwashRasterizer` overrides `set_base_font` to cache the FontId in
`Inner.base_font`; `App::build_font_stack` calls
`rasterizer.set_base_font(base_id)` once, right before returning the
constructed stack.

#### Internal: `SwashFont` field addition + `Inner.base_font`

```rust
struct SwashFont {
    bytes: Arc<[u8]>,
    offset: u32,
    coords: Vec<NormalizedCoord>,
    is_bold: bool,
    has_color: bool,
    is_colrv1_emoji: bool, // NEW
}

struct Inner {
    fonts: HashMap<FontId, SwashFont>,
    shape_ctx: ShapeContext,
    scale_ctx: ScaleContext,
    base_font: Option<FontId>, // NEW: set via set_base_font
}
```

#### Internal: `raster()` branch

```rust
fn raster(&self, font: FontId, glyph_id: u32, size_px: f32) -> Option<GlyphBitmap> {
    let mut inner = self.inner.lock();
    let swash_font = inner.fonts.get(&font).cloned()?;
    if glyph_id == 0 { return None; }

    if swash_font.is_colrv1_emoji {
        // Resolve base text font's ascent + cell_h at size_px so the
        // pixmap is sized to the cell (FR8). cell_h drives Pixmap dim;
        // ascent overrides bearing_top so the bitmap top aligns with
        // cell top (no above-line bleed).
        let (base_ascent_px, base_cell_h_px) = inner
            .base_font
            .and_then(|fid| inner.fonts.get(&fid))
            .and_then(|bf| /* compute (ascent, ascent + descent) * size_px / upem */)
            .unwrap_or((0.0, 0.0));
        drop(inner);
        return colrv1_painter::rasterize(
            &swash_font.bytes, glyph_id, size_px, base_cell_h_px,
        )
        .map(|r| GlyphBitmap {
            format: AtlasFormat::Rgba,
            width: r.width,
            height: r.height,
            bearing: (
                r.bearing_left,
                if base_ascent_px > 0.0 { base_ascent_px.round() as i32 } else { r.bearing_top },
            ),
            advance: r.advance,
            pixels: r.pixels,
        });
    }

    // ... existing swash path unchanged ...
}
```

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/render/font/traits.rs::GlyphBitmap` — output shape stays unchanged
- `src-tauri/src/render/font/swash_adapter.rs::SwashFont` — extended with one bool
- `src-tauri/src/render/font/resolver.rs` — font registration site adds the COLR probe
- `src-tauri/src/render/font/fallback.rs` — unaffected: monochrome fallback still descends naturally when COLRv1 path returns `None`

**External Dependencies:**

| Crate | Version | Status | Purpose |
|-------|---------|--------|---------|
| `skrifa` | 0.20 | **NEW direct dep** (already linked transitively via `resvg`) | Read COLRv1 tables, walk paint graph |
| `tiny-skia` | 0.11 | **NEW direct dep** (already linked transitively via `resvg`) | Rasterize paint graph to `Pixmap` |

Both are already in `Cargo.lock` because the feature-gated `resvg` (0.44) dependency pulls them in. Promoting them to direct dependencies of `src-tauri` does not increase the link footprint of the GUI build.

CLI-only build (`--no-default-features`) does not need this code path. New module is gated under `#[cfg(feature = "gui")]` via its placement under `src-tauri/src/render/`, which is already GUI-gated.

### File Structure

```
src-tauri/
├── Cargo.toml                                # add skrifa = "0.20", tiny-skia = "0.11" to [dependencies]
├── assets/fonts/
│   ├── Noto-COLRv1.ttf                        # NEW (~5 MiB)
│   └── NotoColorEmoji.ttf                     # DELETED
├── src/render/font/
│   ├── colrv1_painter.rs                      # NEW
│   ├── mod.rs                                 # pub mod colrv1_painter;
│   ├── swash_adapter.rs                       # branch + new field
│   └── resolver.rs                            # call is_colrv1_emoji() probe at registration
scripts/
└── fetch-fonts.sh                             # remove CBDT entry, add COLRv1 entry
```

### COLRv1 ColorPainter Coverage

The `ColorPainter` trait implementation in `colrv1_painter.rs` must handle
the full set of COLRv1 paint primitives that `Noto-COLRv1.ttf` exercises:

| Paint primitive | Required | Notes |
|-----------------|----------|-------|
| `PaintSolid` | Yes | Solid color fill |
| `PaintLinearGradient` | Yes | Two color stops + matrix |
| `PaintRadialGradient` | Partial | Focal point `c0` preserved (mapped to `tiny_skia::RadialGradient`'s `start` point) and outer radius `r1` honored; inner radius `r0 > 0` cannot be represented by `tiny_skia::RadialGradient` (only the two-point form is exposed) and is dropped. A `warn_once` log fires on the first occurrence of `r0 > 0` |
| `PaintSweepGradient` | Partial | tiny-skia 0.11 has no native sweep gradient shader. Falls back to the first color stop's solid color and emits a `warn_once` log. Noto-COLRv1 uses sweep gradients on a small minority of glyphs |
| `PaintGlyph` | Yes | Glyph outline as clip |
| `PaintColrGlyph` | Yes | Nested COLR glyph reference |
| `PaintTransform` / `PaintTranslate` / `PaintScale` / `PaintRotate` / `PaintSkew` | Yes | Affine transforms |
| `PaintComposite` (all 28 compositing modes) | Yes | Map to tiny-skia `BlendMode` where supported; fall back to `SourceOver` with a log warning for any mode tiny-skia cannot express |

Strategy: drive `skrifa::color::ColorPainter` callbacks (push/pop_transform,
push/pop_clip, fill, fill_glyph, push/pop_layer) and translate each into
tiny-skia paint operations on a single `Pixmap`. Aim for byte-equivalence
with the Chromium reference (`tmp/verify-emoji/out/*_C_vector.png`) at the
sample emoji set, not pixel-perfect identity across all glyphs.

### Premultiplied → Straight Alpha Conversion

```rust
// For each pixel (r_premul, g_premul, b_premul, a):
//   if a == 0: write (0, 0, 0, 0)
//   else:      write (r_premul * 255 / a, g_premul * 255 / a,
//                     b_premul * 255 / a, a)
// Clamp the divisions to [0, 255] before write to defend against
// tiny-skia rounding edge cases on near-saturated alpha.
```

Reference: see un-premultiply loop in `src-tauri/src/render/emoji_resample.rs`
(the same arithmetic).

### Font Registration / Probe

Add to `SwashRasterizer::register_bytes` (or the appropriate registration
site in `resolver.rs`):

```rust
let is_colrv1_emoji = colrv1_painter::is_colrv1_emoji(&bytes);
let has_color = probe_color_support(&bytes); // existing
```

`is_colrv1_emoji` returns `true` only when the COLR table exists, its
version field equals 1, and the font also has cmap coverage of at least
one canonical emoji codepoint (e.g. U+1F600). This matches the
`probe_color_support` defensive pattern and guards against malformed
fonts.

When both `is_colrv1_emoji` and the user-installed Windows
NotoColorEmoji (COLRv1+SVG, 24 MB) coexist, the existing
`probe_color_support` rejection logic (which checks that swash actually
produces non-empty pixels) is replaced for COLRv1 fonts by the new
probe: we trust `is_colrv1_emoji` to mean "rasterizable by our new
path".

## Test Scenarios

### Unit Tests

Located in `src-tauri/src/render/font/colrv1_painter.rs` under
`#[cfg(test)] mod tests { … }`.

- [ ] `un_premultiply_alpha_zero_emits_zeros` — premultiplied
  `(0,0,0,0)` stays `(0,0,0,0)`
- [ ] `un_premultiply_alpha_saturated_passthrough` — `(255,128,64,255)`
  stays `(255,128,64,255)`
- [ ] `un_premultiply_half_alpha_scales_up` —
  `(64,32,16, 128)` becomes `(127,63,31,128)` (within ±1 rounding)
- [ ] `is_colrv1_emoji_accepts_noto_colrv1` — bundled
  `Noto-COLRv1.ttf` bytes return `true`
- [ ] `is_colrv1_emoji_rejects_mono_emoji` — bundled
  `NotoEmoji-Regular.ttf` bytes return `false`
- [ ] `is_colrv1_emoji_rejects_cbdt` — synthetic / cached CBDT bytes
  return `false` (skipped if CBDT no longer bundled; use an in-test
  fixture)
- [ ] `rasterize_smiley_returns_non_empty_rgba` —
  rasterize(U+1F600 → glyph_id, size_px=26.0, target_cell_h_px=0.0)
  returns `Some(_)` with non-zero RGBA pixels
- [ ] `rasterize_rocket_returns_non_empty_rgba` — same for U+1F680
- [ ] `rasterize_heart_returns_non_empty_rgba` — same for U+2764
- [ ] `rasterize_globe_returns_non_empty_rgba` — same for U+1F30D
- [ ] `rasterize_glyph_id_zero_returns_none` — glyph_id=0 returns `None`
- [ ] `rasterize_size_px_zero_returns_none` — size_px=0.0 returns `None`
- [ ] `rasterize_size_px_negative_returns_none` — size_px=-1.0 returns `None`
- [ ] `rasterize_at_target_pxs` — call rasterize with
  size_px ∈ {17.0, 21.0, 26.0, 35.0} and `target_cell_h_px = 0.0`
  (legacy fallback path) and assert each result has
  `width == ceil(size_px) as u32` and `height == width` (square Pixmap)
- [ ] `rasterize_target_cell_h_pads_and_centers` — call rasterize with
  `target_cell_h_px > 0` (e.g. `size_px=17.33, target_cell_h_px=19.0`)
  and assert `width == height == ceil(target_cell_h_px)`,
  `advance == width`, and `bearing_top ∈ [pad, dim - pad]` (the glyph
  baseline sits inside the inner padded area)
- [ ] `rasterize_tiny_dim_skips_padding` — at `dim < 4`
  (e.g. `size_px=3.0, target_cell_h_px=3.0`) padding is skipped so the
  inner area stays positive (`baseline_y == dim`, `advance == dim`)

### Integration Tests

Located in `src-tauri/src/render/font/swash_adapter.rs` tests (extending
existing `swash_rasters_emoji_rgba` style).

- [ ] `emoji_routes_through_colrv1_path` — register the bundled
  `Noto-COLRv1.ttf`, call `raster()` for the smiley glyph, assert the
  returned bitmap has `format == AtlasFormat::Rgba` and non-zero pixels.
- [ ] `cjk_unchanged_after_colrv1_addition` — register CJK font alongside
  COLRv1 emoji font; assert the existing `swash_rasters_ascii_alpha` /
  CJK paths still produce identical output to before.
- [ ] `unknown_glyph_falls_back_to_chain` — request a codepoint not
  covered by Noto-COLRv1 (e.g. a custom PUA mark) and assert that
  `FallbackChain` lands on `NotoEmoji-Regular` (monochrome).

### E2E Tests

**Existing E2E tests**: None. There is no `docker-compose.e2e.yml`,
no `e2e-tests/` directory, no `tests/e2e/`. E2E behavior is validated
manually per `test/README.md`.

**Run command**: Not detected.

Manual scenarios for sdd.6 verification:
- [ ] Scenario 1: On Windows at 1.5× DPI, run `echo 😀🚀❤️🌍👍🏽` and
  visually compare to `tmp/verify-emoji/out/compare3_*_26px.png` C
  variant. Glyphs must look comparably sharp.
- [ ] Scenario 2: On Linux at 1.0× DPI, run the same and confirm no
  regression vs. current main.
- [ ] Scenario 3: On RDP at 1.0× scaling, confirm no regression.
- [ ] Scenario 4: Run `ls -l src-tauri/target-host/release/emterm`
  before / after and confirm ~5 MiB reduction.

### Edge Cases

- [ ] Skin-tone modifier sequence (👍🏽 = U+1F44D + U+1F3FD) is handled by
  the existing shaper (GSUB ligature → single glyph_id). The new path
  rasterizes that glyph_id without special-casing.
- [ ] VS16 variation selector (❤️ = U+2764 U+FE0F) is handled in shaping
  (U+FE0F maps to gid 0 / no advance) and the underlying glyph_id is
  rasterized.
- [ ] Glyph with deeply nested `PaintColrGlyph` (composite layered glyph)
  must terminate; cap nested depth at 32 to defend against malformed
  fonts (skrifa applies its own limit, but we add a defensive `info`
  log if reached).
- [ ] `size_px < 1.0` (and any value that is `NaN`, zero, or negative)
  returns `None`. The single guard `!(size_px >= 1.0)` rejects all of
  these in one branch — Pixmap allocation would otherwise fail or
  produce a degenerate 1×1 buffer the renderer cannot composite.
- [ ] **`PaintSweepGradient` degradation:** tiny-skia 0.11 has no sweep
  gradient shader, so the painter falls back to the first color stop's
  solid color and emits a `warn_once` log. Affected emoji render with
  a flat fill instead of an angular sweep; visual impact is small because
  Noto-COLRv1 uses sweep gradients on a small minority of glyphs.
- [ ] **`PaintRadialGradient` with `r0 > 0` degradation:** tiny-skia
  0.11's `RadialGradient::new` exposes a two-point (focal + center) form
  but not the two-circle form needed for non-zero inner radius. The
  focal point `c0` is preserved, but `r0` is dropped (treated as 0).
  A `warn_once` log fires on the first `r0 > 0` event.
- [ ] User-installed Windows "Noto Color Emoji" (the 24 MB COLRv1+SVG
  build) is not bundled and is not the candidate font during normal
  startup. If a user manually points the font picker at it, the new
  probe accepts it (it is COLRv1) and the new path renders it.

### Performance Tests

Not required as a gating test. NFR1 (10 ms per first rasterization) is
verified informally during manual scenarios; the GlyphCache amortizes the
cost so steady-state frame work is unaffected.

## Security Considerations

- **Authentication / Authorization:** Not applicable (local-only render path).
- **Input Validation:**
  - `glyph_id == 0` and `size_px <= 0.0` are rejected at the entry point.
  - Font bytes come exclusively from `include_bytes!` on the bundled font
    pinned by SHA256 in `scripts/fetch-fonts.sh`. No runtime ingestion of
    arbitrary fonts in this code path.
- **Data Protection:** Not applicable.
- **XSS / SQL / CSRF:** Not applicable.
- **Memory safety:** Both `skrifa` and `tiny-skia` are safe-Rust crates.
  No new `unsafe` is introduced.

## Error Handling

This feature uses `Option<T>` returns consistent with the surrounding
`GlyphRasterizer` trait. There are no user-visible error codes.

| Condition | Behavior |
|-----------|----------|
| `glyph_id == 0` | `None` — caller treats as "no glyph" (existing convention) |
| `!(size_px >= 1.0)` (NaN, zero, negative, or `< 1.0`) | `None` (single guard rejects all four cases) |
| skrifa cannot read COLR table | `None` — `FallbackChain` descends |
| skrifa returns no paint graph for glyph_id | `None` + `info` log (`colrv1: fallback for gid={gid}, size_px={size_px} (no paint graph)`) |
| tiny-skia `Pixmap::new` returns `None` (size too large) | `None` + `warn` log |
| `Pixmap`/`Mask` allocation inside `push_layer` / `push_clip_*` returns `None` | Push `None` sentinel onto the stack; matching `pop_*` unwinds normally and the sub-tree renders empty. One `warn_once` log per process via `OnceLock` |
| `PaintSweepGradient` brush | Fall back to first stop's solid color; one `warn_once` log per process |
| `PaintRadialGradient` brush with `r0 > 0` | Drop `r0` (keep focal `c0` + outer `r1`); one `warn_once` log per process |
| Composite mode unsupported by tiny-skia (HSL family) | Use `BlendMode::SourceOver` + `warn` log once per unique mode (debouncer via `OnceLock<Mutex<HashSet<u8>>>`) |

## Performance Optimization

### Performance Goals
- First rasterization of a single COLRv1 glyph at a given size_px:
  < 10 ms on Windows x86_64 reference hardware.
- Steady-state per-frame cost: unchanged (cache hit, no tiny-skia work).

### Optimization Strategies
- **GlyphCache** (existing): size_px-keyed cache absorbs every repeat
  rasterization. No code change required.
- **No per-frame allocation**: `colrv1_painter::rasterize` allocates one
  `Pixmap` per call; this happens only on cache miss.

### Caching Strategy
- Existing `src-tauri/src/render/font/cache.rs` `GlyphCache` is the sole
  cache layer. No new cache.

## Success Criteria

- [ ] All functional requirements (FR1–FR8) are implemented
- [ ] All listed unit + integration tests pass
- [ ] Manual scenarios 1–4 pass (Windows 1.5× crisp, no Linux / RDP
  regression, ~5 MiB binary reduction)
- [ ] `cargo check --no-default-features` still passes (CLI build
  untouched)
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
  src-tauri/Cargo.toml --lib` passes
- [ ] `bun run typecheck` passes (no TS surface change but sanity)
- [ ] No new `unsafe`; no `unwrap()` on font / paint-graph operations

## Open Questions

> **Note**: 解決済みの要件は sdd.yaml の `requirements.<id>.resolution` に
> 詳細が記録されている。本セクションは現時点で全項目解決済み。

- [x] **FR5** (resolved): Noto-COLRv1.ttf は `googlefonts/noto-emoji v2.051`
  tag から取得する (既存 `NotoColorEmoji.ttf` と同 tag)。
  URL = `https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/Noto-COLRv1.ttf`、
  SHA256 = `0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2`、
  size ≈ 4,991,984 bytes。詳細は `sdd.yaml` FR5 の resolution。
- [x] **FR2** (resolved): skrifa 0.20 の COLR API 形状を planning フェーズで
  確認済み。`MetadataProvider::color_glyphs() -> ColorGlyphCollection` →
  `get_with_format(GlyphId, ColorGlyphFormat::ColrV1) -> Option<ColorGlyph>` →
  `ColorGlyph::paint(LocationRef, &mut impl ColorPainter)` のチェーンで利用する。
  probe は (1) `FontRef::new(bytes)`, (2) COLR table version == 1,
  (3) cmap covers U+1F600 の3段ガードで `true`/`false` を返す。

## Implementation Phases

Single phase. All FRs land in one PR / commit set to keep the bundled
font swap atomic with the new rasterization path.

## References

- Plan document: `tmp/colrv1-emoji-vector-rendering-plan.md`
- Windows verification result: `tmp/emoji-check-result.md`
- Verification procedure: `tmp/emoji-rasterization-quality-verification.md`
- Ground-truth PNGs (do not embed in repo; reference for manual compare):
  `tmp/verify-emoji/out/*_C_vector.png`,
  `tmp/verify-emoji/out/compare3_*_*.png`
- Requirements document (Japanese): `doc/tasks/colrv1-emoji-vector-rendering/要件定義書.md`
- Existing code touched:
  - `src-tauri/src/render/font/swash_adapter.rs::raster` (and `SwashFont` struct)
  - `src-tauri/src/render/font/resolver.rs` (font registration probe site)
  - `src-tauri/src/render/font/mod.rs` (new `pub mod colrv1_painter`)
  - `scripts/fetch-fonts.sh`
- Existing code reused without modification:
  - `src-tauri/src/render/font/cache.rs` (GlyphCache)
  - `src-tauri/src/render/font/atlas.rs`
  - `src-tauri/src/render/font/fallback.rs`
  - `src-tauri/src/render/emoji_resample.rs` (un-premultiply reference only)
