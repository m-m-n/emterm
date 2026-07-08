# Feature: block-cursor-glyph-font

## Overview

The focused filled block cursor overlay currently redraws the covered
glyph using egui's built-in `FontId::monospace(...)`, not the terminal's
configured font. This produces a visible font mismatch — e.g. Inconsolata
renders `0` with a slash while egui's monospace renders it with a stem,
so moving the cursor onto an existing character looks like the font
suddenly changed. This feature makes the block-cursor overlay glyph
match the grid glyph pixel-for-pixel (same font resolver, same swash
rasterizer, same baseline/size).

## Objectives

- Eliminate the glyph font mismatch when the focused block cursor covers
  an existing cell.
- Keep the render-cpu-optimization invariant intact: grid instance data
  stays independent of cursor state; the fix lives in the overlay layer,
  not in the wgpu grid pass.
- No regression on other cursor styles (underline / bar / unfocused
  hollow-block) or on IME preedit.

## User Stories

### US1: Consistent glyph under block cursor

As an eMterm user with a custom monospace font (e.g. Inconsolata), I
want the character shown under the block cursor to have the same glyph
shape as the surrounding grid, so that moving the cursor doesn't look
like a font change.

**Acceptance Criteria:**
- [ ] With Inconsolata configured, an existing `0` under the block
      cursor renders with the same slashed-zero glyph as the grid.
- [ ] The behavior holds for arbitrary ASCII, CJK, and symbol code
      points that appear in the grid.
- [ ] The covered-glyph color still uses the fully-resolved cell
      background color path (`resolve_cell_style_from_packed`) — reverse
      video / selection / dim / hidden still apply exactly as before.

### US2: No regression on other cursor shapes

As an eMterm user, I want underline / bar / unfocused hollow-block
cursors to behave exactly as before.

**Acceptance Criteria:**
- [ ] Non-block cursor styles do not redraw the covered glyph (unchanged
      behavior).
- [ ] Unfocused hollow-block still uses the outline path only.

## Technical Requirements

### Functional Requirements

- **FR1:** In `draw_block_cursor` (`src-tauri/src/render/cursor.rs`),
  the covered glyph MUST be rasterized through the same font resolver /
  swash rasterizer chain that `terminal_grid_pass` uses. It MUST NOT go
  through egui's built-in `FontId::monospace(...)`.
- **FR2:** The covered glyph position, size, baseline, and advance MUST
  match the grid rendering of the same code point at the same cell
  (i.e. the overlay glyph aligns visually with the underlying cell as if
  the grid glyph itself changed color).
- **FR3:** The overlay glyph color MUST remain the value produced by
  `resolve_cell_style_from_packed(...).bg` for the covered cell — the
  same value the current implementation passes to `painter.text(...)`.
- **FR4:** Wide (2-cell) glyphs under the block cursor MUST behave as
  today: rect covers the full 2-cell footprint, glyph draws once at the
  leading column, `resolve_cursor_glyph_col` still snaps the width-0
  trailing half to the leading column.
- **FR5:** Glyphs for which `cursor_glyph_paintable(...)` returns false
  (color emoji etc.) MUST continue to be suppressed — no overlay glyph
  is drawn, only the rect fill. No new exception path for the overlay.
- **FR6:** The final fallback behavior MUST use the same chain as the
  grid (swash → ab_glyph → .notdef). No cursor-only fallback path.

### Non-Functional Requirements

- **NFR1 - Performance:** Per-frame overlay cost MUST NOT regress the
  gains landed by the render-cpu-optimization feature. Since only up to
  1-2 cells per frame are overlaid, this is expected to be
  imperceptible; profile only if a hot path is introduced.
- **NFR2 - Architectural invariant:** The wgpu grid instance data MUST
  remain independent of cursor state (position, blink phase, focus).
  The fix stays in the egui overlay layer, not by re-baking cursor
  state into the grid pass. This mirrors the render-cpu-optimization
  task0001 invariant referenced in `cursor.rs`.
- **NFR3 - Cross-platform:** Behavior is identical on Linux and Windows.
- **NFR4 - Documentation:** The comment block above `draw_block_cursor`
  MUST be updated to explain that the overlay glyph now goes through the
  swash rasterizer chain, and why the egui monospace path was wrong.

## Implementation Approach

### Architecture

Two rendering paths currently coexist:

```
┌───────────────────────────────────────────────────────────────┐
│                  Frame drawing pipeline                        │
├───────────────────────────────────────────────────────────────┤
│ wgpu terminal_grid_pass ── swash rasterizer ─→ grid glyphs    │
│ egui Painter overlay    ── (block cursor: rect + glyph)       │
│                            (preedit: underline)               │
└───────────────────────────────────────────────────────────────┘
```

Today the overlay glyph is drawn via `egui::Painter::text(...)` with
`FontId::monospace(font_px)`, which resolves to egui's bundled
Ubuntu-Mono-ish typeface — completely unrelated to the swash chain.

The fix must route the overlay glyph through the same swash-based
resolver. Two viable shapes (final decision belongs to create-plan;
this SPEC states the constraints they must satisfy, not the choice):

1. **Overlay via swash → egui texture**: rasterize the covered glyph via
   the existing swash cache, upload/reuse an egui `TextureHandle` for
   that raster, and blit it inside the cursor rect at the same cell
   metrics the grid pass uses.
2. **Cursor-aware second pass in terminal_grid_pass**: add an
   overlay/late pass in the wgpu grid pipeline that draws just the
   covered cell (or 2-cell footprint) on top of the base grid, using
   the cursor color for the rect and the covered glyph's swash raster
   for the character. The base grid instance data still ignores cursor
   state.

Either shape satisfies FR1-FR6. The plan phase chooses based on
integration cost and consistency with the existing rasterizer cache.

### Data Flow

```
draw_block_cursor()
  ├─ cursor rect (unchanged: painter.rect_filled with cursor color)
  └─ covered glyph
       (new)   → resolve font via same chain as grid
                → swash raster (via existing cache)
                → paint at cell metrics with resolved bg color
       (old)   → FontId::monospace + painter.text (REMOVED)
```

### API Design

No public API change. All modifications are in the render layer:

- `src-tauri/src/render/cursor.rs::draw_block_cursor` — glyph path swap.
- Possibly a small helper in `src-tauri/src/render/font/` or
  `src-tauri/src/render/` exposing a "rasterize glyph for overlay"
  entry point that wraps the existing swash cache. Naming and location
  are chosen at plan time.

### File Structure

Only render-layer touches expected:

```
src-tauri/src/render/
├── cursor.rs              # draw_block_cursor: glyph path swap
├── terminal_grid_pass.rs  # possibly: shared helper or overlay pass
├── font/
│   ├── cache.rs           # possibly: expose a lookup for overlay use
│   └── swash_adapter.rs   # possibly: overlay entry point wrapper
└── mod.rs                 # possibly: re-export helper
```

## Test Scenarios

### Unit Tests

- [ ] `cursor.rs` existing block-cursor rect tests keep passing
      unchanged (geometry logic is untouched).
- [ ] New: helper that resolves the overlay glyph for a given cell
      routes through the swash resolver (mock resolver returns the same
      identifier as grid-side calls for the same code point + font
      size + weight).
- [ ] New: `cursor_glyph_paintable == false` code points (e.g. a color
      emoji sentinel) skip glyph rasterization entirely.

### Integration Tests

- [ ] Wide (2-cell) glyph under block cursor: cell width is 2, glyph
      raster is looked up once at the leading column, no glyph raster
      lookup for the trailing column.
- [ ] Font atlas / cache is reused between grid and overlay for the
      same code point (no duplicate raster).

### E2E Tests

**Existing E2E tests**: Rust cargo test suite + `bun test` for the
web bundles. No project-wide E2E framework configured today.
**Run command**: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml` / `bun test`
- [ ] Existing tests pass without regression.
- [ ] Manual visual check: eMterm launched with Inconsolata,
      `0O1lI` printed to the shell, cursor stepped through each cell
      with arrow keys. Glyphs under the block cursor match the grid.

### Edge Cases

- [ ] Empty cell under block cursor: only rect is drawn (unchanged).
- [ ] Cursor on width-0 trailing half of a wide glyph:
      `resolve_cursor_glyph_col` still resolves to the leading column,
      overlay glyph draws there (unchanged behavior).
- [ ] Reverse video / selection / dim / hidden on the covered cell: the
      overlay glyph color is the fully-resolved bg from
      `resolve_cell_style_from_packed`, matching current behavior.
- [ ] Focus lost mid-frame: block cursor branch is not entered — no
      glyph re-raster path runs.

### Performance Tests

- [ ] Manual: idle-frame CPU with cursor blinking on top of a filled
      cell does not regress against the pre-fix baseline (spot-check
      via `top` or the render-cpu-optimization benchmarks — no strict
      threshold; qualitative).

## Security Considerations

Not applicable — pure rendering-layer change with no I/O, no
untrusted input, no privilege boundaries.

## Error Handling

- Font resolution failure for the covered glyph: fall back through the
  existing chain (swash → ab_glyph → .notdef). No cursor-specific
  fallback logic.
- Swash rasterization returns empty (e.g. control char that
  `cursor_glyph_paintable` slipped through): draw the rect only, skip
  glyph — mirroring the current guard.

## Performance Optimization

### Performance Goals

- No perceptible regression versus current cursor rendering.
- Reuse the existing swash cache; do not create a parallel cache for
  the overlay path.

### Caching Strategy

Overlay glyph rasters share the same cache/atlas the grid pass uses.
Cache key equivalence for `(code_point, font_size, weight, style)`
between grid and overlay is required (this is what makes FR2 achievable
for free).

## Success Criteria

- [ ] FR1-FR6 met.
- [ ] All existing unit tests pass; new tests for FR1 / FR4 / FR5
      pass.
- [ ] Manual visual verification: block cursor on `0` (Inconsolata) shows
      the slashed-zero form matching the grid.
- [ ] `cargo check --no-default-features` still passes (CLI-only build
      unaffected).
- [ ] No regression in `bun test` / `bun run typecheck` (should be
      unrelated but verified as a smoke check).
- [ ] Code comments above `draw_block_cursor` updated to reflect the
      new glyph path.

## Open Questions

None outstanding for this SPEC. Plan-phase decisions include:
- Overlay-via-egui-texture vs. cursor-aware second pass in wgpu.
- Where the shared "rasterize glyph for overlay" helper lives.

## References

- `src-tauri/src/render/cursor.rs:300-360` — `draw_block_cursor`
  current implementation with the offending `FontId::monospace(...)`
  call at line 356.
- `src-tauri/src/render/terminal_grid_pass.rs` — grid swash pipeline.
- `src-tauri/src/render/font/` — resolver + swash adapter + fallback
  chain.
- render-cpu-optimization task0001 — the invariant `cursor.rs`'s
  header comment cites: grid instances stay independent of cursor
  state.
