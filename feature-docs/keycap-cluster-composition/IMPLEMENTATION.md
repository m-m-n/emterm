# Implementation Plan: Keycap Cluster Composition

## Overview

Two independent tasks: term_core gains retroactive merging of
standalone-arriving zero-width characters into the previously written cell
(FR1–FR4); the renderer strips VS16 from emoji-font-routed clusters before
shaping (FR5). They meet only through the grid-cell content contract below.

## Technology Stack

- **Language**: Rust (existing crates only; no new dependencies, no license
  impact — project license MIT)
- **Shaping**: swash 0.1.18 (existing) — its ligature matcher does not skip
  default-ignorable variation selectors, which is why FR5 exists

## Layer Structure

- `crates/term_core` (task0001): grid state authority. Decides cell content,
  cell width, cursor movement. Knows nothing about fonts.
- `src-tauri/src/render` (task0002): consumes grid cells. Decides font
  routing and shaping input. Never mutates grid content.

Dependency direction unchanged: render reads term_core output; term_core
never depends on render.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Grid cell cluster content | term_core stores the FULL composed cluster string in the base cell | Postcondition of task0001: after retroactive merge, the base cell's string contains every codepoint of the cluster IN ARRIVAL ORDER, INCLUDING VS16 (U+FE0F). VS16 is never dropped at the grid layer. Precondition of task0002: the cell string it receives may contain VS16 anywhere after the base character; presentation/fallback decisions use this full string, and only the shaping input has VS16 removed. | task0001 (producer), task0002 (consumer) |
| Cell width semantics | term_core sets `width` on the base cell and creates a spacer cell for width 2 | Unchanged existing semantics: base cell `width` ∈ {1, 2}; a width-2 base is followed by a zero-width placeholder (spacer) cell. task0001 reuses this exact shape when retroactively widening; task0002 relies on it unchanged. | task0001, task0002 |

## Conventions

- Tests live in the same file's `#[cfg(test)]` module, following each file's
  existing test style.
- Test commands come from workflow.yaml `project.components` (term_core and
  main entries) — run them verbatim.
- No behavior change to the grapheme-buffer path (`flush_grapheme_buffer`,
  ExtPict-base assembly): it is a regression boundary for both tasks (NFR1).

## Cross-task Design Decisions

### VS16 stays in the grid, is stripped only at shaping time

term_core keeps VS16 in the stored cluster (it is the presentation signal the
renderer's font selection needs); the renderer removes it only from the
codepoint sequence handed to the emoji-font shaper. Rationale: font selection
(presentation.rs / fallback.rs) is already implemented and tested against
clusters that include VS16 (NFR1), and the swash ligature limitation is a
shaping-layer concern. Affects: task0001, task0002.

### Retroactive merge model (Alacritty / xterm style), width follows VS16

A standalone-arriving zero-width character (VARIATION_SEL / COMBINING per
term_core's width tables) appends to the most recently written cell rather
than overwriting the cursor cell. Width follows presentation: cluster gains
VS16 → base cell retroactively widens to 2 (spacer + cursor advance + wrap
handling at end of line); no VS16 → width stays 1 and the cursor does not
move. This matches `flush_grapheme_buffer`'s existing VS16 → width-2 rule, so
the width policy is uniform across both composition paths. Affects: task0001
(implements), task0002 (consumes resulting cells; no special casing).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| wcwidth-based apps (zsh line editor, nvim) compute keycap as width 1 and drift from the width-2 grid | Medium | Medium | Accepted per spec (AI-first). Future `unicode_version`-style knob documented as out of scope. |
| Retroactive widening at the last column interacts badly with wrap_pending state | Medium | High | Dedicated acceptance criterion + boundary tests in task0001 (TS-6). |
| "Most recently written cell" tracking invalidation gaps (cursor moves, scroll, resize, screen clears between writes) | Medium | High | task0001 must define explicit invalidation rules; stale-base cases fall back to FR4 (drop). |
| Cell storage overflow for long combining runs (fixed 16-byte inline cell data) | Low | Medium | Existing overflow mechanism (`is_overflow` + overflow map) already handles long strings; covered by an edge-case test in task0001. |

## Open Questions

None.
