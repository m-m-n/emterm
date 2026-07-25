# Feature: Keycap Cluster Composition (Retroactive Combining-Character Merge)

## Overview

Keycap emoji clusters (`5️⃣` = U+0035 U+FE0F U+20E3) currently render in eMterm as
two cells: "5" followed by an empty keycap glyph (□). The root cause is that
term_core's print path never merges a standalone-arriving zero-width character
(VARIATION SELECTOR / COMBINING) into the previously written cell — it overwrites
the cell at the cursor instead, where it is later clobbered by subsequent text.
This also breaks all combining characters (e.g. `e` + U+0301 loses the accent).
This feature adds retroactive merging in term_core and strips VS16 before emoji
font shaping in the renderer so the keycap GSUB ligature can match.

## Objectives

- Render keycap clusters (`<digit/*/#> [VS16] U+20E3`) as a single composed glyph.
- Fix retroactive composition for all standalone-arriving combining characters.
- Keep the existing grapheme-buffer path (ExtPict-base clusters) unchanged.

## User Stories

### US1: Keycap emoji display
As a terminal user, I want keycap emoji output by applications (e.g. Claude Code)
to render as a single keycap glyph, so that emoji-rich output is readable.

**Acceptance Criteria:**
- [ ] `5 FE0F 20E3` occupies one base cell (width 2 + spacer) in the grid.
- [ ] `5 20E3` (no VS16) occupies one width-1 cell.
- [ ] On screen, 5️⃣ renders as one keycap emoji glyph (human verification).

### US2: Combining character display
As a terminal user, I want combining accents that arrive as separate writes to
merge into the preceding character, so that text like é is not corrupted.

**Acceptance Criteria:**
- [ ] `e` then U+0301 composes into one cell and survives subsequent text.

## Technical Requirements

### Functional Requirements

- **FR1: Retroactive merge of zero-width characters.** In term_core's print path
  (`write_grapheme_to_grid`, `crates/term_core/src/print_handler.rs`), a
  standalone-arriving zero-width character (`char_width` = 0: VARIATION_SEL or
  COMBINING per `unicode_width.rs`) is appended to the cell most recently written,
  instead of overwriting the cell at the cursor. The cursor does not advance
  (except per FR2). The same mechanism handles keycap (`5 [FE0F] 20E3`) and
  combining accents (`e 0301`).
- **FR2: VS16-driven retroactive width-2 expansion.** When the merged cluster
  contains VS16 (U+FE0F), the base cell is expanded to width 2: the adjacent cell
  becomes a wide-spacer and the cursor advances by 1. At the last column, line
  wrap handling applies. Without VS16 the cell stays width 1 and the cursor does
  not move. This matches `flush_grapheme_buffer`'s existing rule (VS16 → width 2).
- **FR3: Wide-cell spacer traversal.** When the previously written cell is the
  spacer of a wide (width-2) cell, the zero-width character is appended to the
  base cell, not the spacer.
- **FR4: Drop when no base exists.** When there is no previously written base
  cell (e.g. start of line / after a state that invalidates it), the zero-width
  character is discarded: not written to the grid, cursor unchanged.
- **FR5: Strip VS16 before emoji font shaping.** In the renderer's shaping path
  for emoji-font-routed clusters, VS16 is removed from the codepoint sequence
  before shaping, because swash does not skip default-ignorable variation
  selectors during ligature matching (the keycap GSUB ligature is defined as
  `base + 20E3`). Presentation/font-selection decisions (VS16 → color emoji font,
  `src-tauri/src/render/font/presentation.rs`, `fallback.rs`) keep using the
  cluster BEFORE stripping.

### Non-Functional Requirements

- **NFR1 - Compatibility:** The existing grapheme-buffer path (ExtPict-base
  cluster assembly, `flush_grapheme_buffer`) is behaviorally unchanged; all
  existing term_core and renderer tests keep passing.
- **NFR2 - Performance:** No measurable regression in the print hot path (the
  ASCII fast path is untouched).

## Implementation Approach

### Affected Code

| Area | File | Current behavior |
| --- | --- | --- |
| term_core print | `crates/term_core/src/print_handler.rs` (`write_grapheme_to_grid` ~:62, `handle_print` ~:251, `flush_grapheme_buffer` ~:278) | width-0 chars overwrite the cursor cell; no retroactive merge |
| width tables | `crates/term_core/src/unicode_width.rs` | `char_width(0xFE0F)` = 0 (VARIATION_SEL), `char_width(0x20E3)` = 0 (COMBINING, 0x20D0–0x20F0) |
| font fallback | `src-tauri/src/render/font/fallback.rs` (~:128) | clusters containing U+20E3 route to the color emoji font (already implemented, tested) |
| presentation | `src-tauri/src/render/font/presentation.rs` (~:475) | keycap base + full-cluster emoji semantics (already implemented) |
| grid pass shaping | `src-tauri/src/render/terminal_grid_pass.rs` (`glyph_instance` ~:689) | shapes the cluster verbatim and uses `shaped.first()`; VS16 breaks the ligature |

### Data Flow

```
PTY bytes → term_core handle_print
  ├─ base char (e.g. '5') → cell[c], cursor advances          (unchanged)
  └─ standalone width-0 char (FE0F / 20E3 / 0301)
       → FR3: resolve base cell (skip wide spacer)
       → FR4: no base → drop
       → FR1: append to base cell's cluster
       → FR2: cluster now has VS16 → widen to 2, spacer, cursor +1, wrap at EOL
Renderer per cell
  → presentation/fallback decide font from the FULL cluster   (unchanged)
  → FR5: emoji-font shaping input = cluster minus VS16
  → keycap GSUB ligature matches → single glyph
```

### Reference Evidence (from the 2026-07-25 investigation)

- swash 0.1.18 shaping, bundled fonts: `5 20E3` → 1 glyph (ligature OK);
  `5 FE0F 20E3` → decomposed (VS16 blocks the match); Noto-COLRv1 has no cmap
  entry for FE0F (.notdef pollution).
- Benchmarks: Alacritty and xterm merge retroactively at width 1; WezTerm widths
  follow VS16 when `unicode_version >= 14`. eMterm adopts VS16 → width 2
  (AI-first: Claude Code measures keycap width with string-width semantics = 2).

### Dependencies

**Internal Dependencies:**
- `crates/term_core`: print path and width tables.
- `src-tauri/src/render`: font fallback / presentation / grid pass shaping.

**External Dependencies:**
- swash 0.1.18 (existing): ligature-matching behavior is the reason FR5 exists.

### Out of Scope

- The existing grapheme-buffer path (ExtPict-base) — no behavior change.
- Overflow/oversize glyph drawing knob (WezTerm-style 3-value option) — future.
- A `unicode_version`-style width negotiation knob — future escape hatch if
  wcwidth-based apps (zsh line editor, nvim) misalign with width 2.

## Test Scenarios

### Unit Tests (term_core)

- [ ] T1: `5 FE0F 20E3` then text → one base cell containing the full cluster,
      width 2, spacer cell, cursor advanced past the spacer.
- [ ] T2: `5 20E3` then text → one width-1 cell containing the cluster.
- [ ] T3: `e 0301` then text → é composed, accent survives subsequent writes.
- [ ] T4: width-0 char at start of line → dropped (grid unchanged, cursor at 0).
- [ ] T5: wide char (e.g. 全) then U+FE0F → appended to the wide base cell
      across the spacer.
- [ ] T6: base cell at last column, VS16 merge → width-2 expansion wraps
      correctly.
- [ ] T7 (regression): existing grapheme-buffer tests (ExtPict clusters, ZWJ
      sequences) pass unchanged.

### Unit Tests (renderer)

- [ ] T8: shaping input for an emoji-routed keycap cluster excludes VS16 and
      yields a single ligature glyph.
- [ ] T9: presentation/fallback still select the color emoji font from the
      cluster including VS16.

### E2E Tests

**Existing E2E tests**: None (no E2E infrastructure for the GUI terminal path)
**Run command**: Not detected
- [ ] Manual: 5️⃣ printed in a real eMterm session renders as one keycap glyph
      (human visual verification).

### Edge Cases

- [ ] Multiple consecutive combining chars appended to one base cell.
- [ ] Width-0 char arriving when the previous write was on another row (after
      wrap): merges to that cell only if it is the "most recently written" per
      FR1's tracking; otherwise FR4 applies.

## Success Criteria

- [ ] All FR1–FR5 implemented and covered by the tests above.
- [ ] NFR1: full existing test suite passes.
- [ ] Human visual verification of 5️⃣ on a release build.

## Open Questions

None.

## References

- 要件定義書: feature-docs/keycap-cluster-composition/REQUIREMENTS.md
- Investigation: tmp/keycap-cluster-investigation-2026-07-25.md
- Terminal policy research: tmp/keycap-terminal-policy-research-2026-07-25.md
