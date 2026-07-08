# Implementation Plan: block-cursor-glyph-font

## Overview

Route the focused block cursor's overlay glyph through the same font
resolver / swash rasterizer chain used by the wgpu `terminal_grid_pass`,
so the glyph shape under the cursor matches the surrounding grid.

## Technology Stack

- **Language**: Rust (edition per workspace, no new crates).
- **Rendering**: egui immediate-mode overlay layer for the cursor rect
  and the covered-cell glyph; the wgpu `terminal_grid_pass` is the SSOT
  for font resolution and glyph rasterization (via `render::font::*`).
- **Key components (existing, no new library)**:
  - `render::font::cache` — rasterized glyph cache/atlas.
  - `render::font::swash_adapter` — swash-based rasterizer.
  - `render::font::fallback` — font fallback chain.

## Layer Structure

Rendering layers, dependency direction unchanged:

- egui overlay (`render::cursor`) — draws non-grid overlays (block
  cursor, preedit) via `egui::Painter` on top of the wgpu pass.
- wgpu grid pass (`render::terminal_grid_pass`) — draws every grid
  cell's glyph via the font module.
- font module (`render::font::*`) — resolver + rasterizer + cache,
  shared by whoever needs a glyph raster.

The change: the overlay layer now consults the font module (rather than
egui's built-in monospace) when it needs to draw a glyph. The overlay
layer remains the only writer of cursor-state-dependent pixels — the
grid pass stays cursor-state-independent (render-cpu-optimization
invariant).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `render::font` overlay-glyph entry point | Given a code point, cell metrics, font size, weight/style, return an egui-paintable raster (texture or image) sourced from the same cache/atlas the grid pass uses. | **Pre**: the font resolver is initialized (same lifecycle as grid pass). **Post**: for the same (code_point, size, weight, style) the raster is identical (bit-exact) to what the grid pass draws in a full cell — same baseline, same advance, same glyph identity. Missing glyphs fall through the standard chain (swash → ab_glyph → .notdef). Returns None (or an empty raster) for code points where `cursor_glyph_paintable` is false — callers still guard with `cursor_glyph_paintable`. | task0001 |

Only one task in this feature, so this table's cross-task role is
degenerate — but the contract still pins what the helper must look
like so review can check the boundary.

## Conventions

- Comments in `render::cursor` MUST call out the fact that the overlay
  glyph now shares the grid font path, and cite the render-cpu-
  optimization invariant that keeps grid instances independent of
  cursor state.
- No new public modules exposed outside `render::`.
- No new dependency added to `Cargo.toml`.
- Test approach: pure functions (glyph identity / paintability) get
  unit tests; the egui `Painter` call itself is not unit-tested (no
  practical way to assert on it in isolation), consistent with the
  existing `cursor.rs` test module.

## Cross-task Design Decisions

### D1: Fix stays in the egui overlay layer, not the wgpu grid pass

**Decision**: Do not add a "cursor cell" instance to the wgpu grid
pass. Keep the overlay in `render::cursor::draw_block_cursor`; only
change what it uses to paint the glyph.

**Rationale**: The render-cpu-optimization task0001 header comment in
`cursor.rs` states the invariant "grid instance data is now
independent of cursor state". Baking cursor state back into the wgpu
instance stream would resurrect the exact coupling that optimization
removed (position / blink phase / focus would re-enter the grid pass'
dirty set). SPEC NFR2 requires this invariant to hold.

**Affected tasks**: task0001.

### D2: Reuse the grid's glyph cache, do not build a parallel one

**Decision**: The overlay glyph path uses the same rasterizer cache
the grid pass populates. Cache keys for the overlay MUST match the
grid keys for the same `(code_point, size, weight, style)` so the
raster is shared.

**Rationale**: SPEC FR2 requires the overlay glyph to be visually
identical to the grid glyph; sharing the cache makes that free
(same bytes, same baseline, same advance). SPEC NFR1 requires no
regression; a parallel cache would double memory and warm-up cost.

**Affected tasks**: task0001.

### D3: `cursor_glyph_paintable` gate stays as-is

**Decision**: The overlay glyph is still gated by
`cursor_glyph_paintable(&ch)` (existing helper). No new exception
path for color emoji / control characters / etc.

**Rationale**: SPEC FR5 explicitly requires this behavior to
carry forward. Broadening the gate would be a separate feature.

**Affected tasks**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Overlay path introduces per-frame allocation (texture creation) | Medium | Medium | Reuse the existing rasterizer cache; avoid creating a new `egui::TextureHandle` per frame — either bind once into egui's texture manager keyed by glyph, or blit CPU-side into a shared image. Concrete choice at implementation time, but the "no new per-frame allocation" property is an Acceptance Criterion. |
| Baseline / advance mismatch vs. grid | Low | High | Read the grid's baseline / cell metrics computation and use the same source. The overlay is drawn inside `block_cursor_rect`, so a small misalignment will be visible; catch by manual visual check MT-1. |
| wgpu → egui texture handoff not supported by the current egui version | Low | Medium | Fall back to blitting a raster via `egui::ColorImage` + `Context::load_texture` if direct wgpu-texture sharing is not viable. |
| Regression on other cursor styles | Low | High | The change is scoped to `draw_block_cursor`. Existing tests in `cursor.rs` cover geometry for underline / bar / hollow-block and stay untouched. |
| Windows-specific font resolution difference | Low | Medium | Route through the same `render::font::fallback` chain the grid uses; that chain already handles cross-platform differences (bundled CBDT, etc.). Verify manually on Windows if a build is available; documented as MT-2. |

## Open Questions

- [ ] Whether the shared glyph raster is delivered to egui as a
      pre-registered `egui::TextureId` or blitted as a per-glyph
      `ColorImage`. Left to implementer — either is compliant with
      FR1/FR2 as long as the grid cache is the source. Chosen shape
      must not allocate per-frame for glyphs already in the cache.
