# Implementation Plan: Emoji Width Handling

## Overview

Implement accurate Unicode 17.0 emoji width calculation for eMterm. Fix emoji characters displaying as width 1, add zero-width character handling, and implement grapheme cluster buffering for ZWJ/modifier/flag emoji sequences.

## Objectives

- All `Emoji_Presentation=Yes` characters display as width 2
- ZWJ emoji sequences are buffered and displayed as single width-2 grapheme clusters
- Zero-width characters (ZWJ, Variation Selectors, etc.) return width 0
- No regression in ASCII or CJK character width

## Prerequisites

### Development Environment
- Bun (package manager and test runner)
- Docker (for test execution)

### Dependencies
- No new external dependencies

### Knowledge Requirements
- Unicode character properties (Emoji_Presentation, Extended_Pictographic)
- UAX #29 grapheme cluster segmentation rules (GB9, GB11, GB12/GB13)
- Existing eMterm terminal state and handler architecture

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (Vanilla)
- **Test Runner**: Bun
- **Renderer**: Canvas 2D API

### Design Approach

Rust parser is unchanged. All emoji handling lives in TypeScript:
1. `unicode.ts`: Width lookup tables (Emoji_Presentation, zero-width)
2. `print_handler.ts`: Grapheme cluster buffering before cell placement
3. `state.ts`: Buffer flush coordination on non-Print actions

### Component Interaction

```
processAction(Print)
  → handlePrint → graphemeBuffer check
    → if buffering emoji: accumulate codepoints
    → if buffer complete: flush → createCell → grid placement
processAction(non-Print)
  → flush pending grapheme buffer first
  → then proceed with normal action handling
```

## Implementation Phases

### Phase 1: Unicode Width Tables and Zero-Width Fixes

**Goal**: `charWidth()` returns correct width for all emoji and zero-width codepoints. Existing CJK/ASCII behavior unchanged.

**Files to Modify**:
- `src/terminal/unicode.ts`: Add emoji presentation lookup, zero-width ranges
- `src/terminal/unicode.test.ts`: Add emoji and zero-width test cases

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `isEmojiPresentation(cp)` | Determine if codepoint has Emoji_Presentation=Yes | Valid codepoint number | Returns true/false |
| `isZeroWidth(cp)` | Determine if codepoint is zero-width | Valid codepoint number | Returns true/false |
| `charWidth(char)` (modified) | Integrate emoji and zero-width checks into priority chain | Single character string | Returns 0, 1, or 2 |

**Processing Flow**:
```
charWidth(char) priority chain:
1. Empty string → 0
2. ASCII 0x20-0x7E → 1  (fast path, unchanged)
3. C0 control 0x00-0x1F → 0
4. DEL/C1 0x7F-0x9F → 0
5. Zero-width characters → 0  (NEW)
6. Emoji_Presentation=Yes → 2  (NEW)
7. Latin-1/Extended with combining check → 0 or 1
8. East Asian Wide/Fullwidth → 2  (existing)
9. Combining characters → 0  (existing)
10. Default → 1
```

**Implementation Steps**:

1. **Add `isEmojiPresentation()` function**
   - Lookup table with all Emoji_Presentation=Yes ranges from Unicode 17.0
   - BMP ranges (U+231A, U+23E9-23EC, etc.) and SMP ranges (U+1F004, U+1F0CF, U+1F18E, etc.)
   - Add Unicode version header comment for maintainability

2. **Add `isZeroWidth()` function**
   - Cover: ZWJ (U+200D), ZWNJ (U+200C), ZWS (U+200B), Word Joiner (U+2060), BOM (U+FEFF)
   - Cover: Variation Selectors (U+FE00-FE0F)
   - Cover: Variation Selectors Supplement (U+E0100-E01EF)

3. **Modify `charWidth()` priority chain**
   - Insert zero-width check before the Latin-1/Extended range (step 5)
   - Insert Emoji_Presentation check after zero-width (step 6)
   - Key considerations:
     - Zero-width check must come before Emoji_Presentation to ensure VS returns 0
     - Emoji check must come before the 0xA0-0x2E00 Latin range to catch BMP emojis

4. **Add `isExtendedPictographic()` function**
   - Needed for Phase 2 grapheme buffering
   - Broader than Emoji_Presentation: includes BMP codepoints like ©®, ‼⁉, and U+1F000-1FFFD
   - Can be implemented as simplified range approximation

5. **Export new functions** for use by print_handler

**Dependencies**:
- Requires: None (standalone)
- Blocks: Phase 2

**Testing Approach**:

*Unit Tests (unicode.test.ts)*:
- Test `charWidth()` for Emoji_Presentation=Yes codepoints (📁, 🔋, 😀, 🚀, ⌚, ⏰, ☕, ⭐)
- Test `charWidth()` for zero-width characters (ZWJ, VS16, VS15, ZWS, ZWNJ, BOM)
- Test `charWidth()` for negative cases (☀ U+2600 is NOT Emoji_Presentation=Yes)
- Test unchanged ASCII behavior
- Test unchanged CJK behavior
- Test `isEmojiPresentation()` directly

**Acceptance Criteria**:
- [ ] `charWidth('📁')` returns 2
- [ ] `charWidth('\u200D')` returns 0
- [ ] `charWidth('\uFE0F')` returns 0
- [ ] `charWidth('A')` returns 1 (unchanged)
- [ ] `charWidth('漢')` returns 2 (unchanged)
- [ ] All existing unicode.test.ts tests pass

**Estimated Effort**: 小

---

### Phase 2: Grapheme Cluster Buffering

**Goal**: Print handler buffers emoji grapheme clusters (ZWJ sequences, skin tone modifiers, Regional Indicator pairs, Variation Selectors) and emits them as single width-2 cells.

**Files to Modify**:
- `src/terminal/handlers/types.ts`: Add grapheme buffer fields to TerminalStateAccessor
- `src/terminal/state.ts`: Implement grapheme buffer state and flush coordination
- `src/terminal/handlers/print_handler.ts`: Add buffering logic to print dispatch
- `src/terminal/handlers/print_handler.test.ts`: Add grapheme cluster tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| GraphemeBuffer (state field) | Hold pending codepoints for current emoji cluster | Terminal initialized | Buffer empty or holding partial cluster |
| `handlePrintDispatch` (modified) | Check buffer state, accumulate or flush | Valid character string | Character processed or buffered |
| `flushGraphemeBuffer` | Convert buffered codepoints to cell and place on grid | Buffer non-empty | Cell placed, buffer cleared |
| `processAction` (modified) | Flush grapheme buffer before non-Print actions | Action received | Buffer flushed if pending |

**Processing Flow**:
```
Print action arrives:
1. Is buffer empty?
   ├─ Yes: Is codepoint Extended_Pictographic or Regional_Indicator?
   │       ├─ Yes → Start buffering, return
   │       └─ No → Process normally (existing path)
   └─ No: Does codepoint extend the cluster?
          ├─ ZWJ → Buffer, return (waiting for next emoji)
          ├─ Variation Selector → Buffer, return
          ├─ Skin Tone Modifier → Buffer, return
          ├─ Combining Mark → Buffer, return
          ├─ Regional Indicator (2nd of pair) → Buffer, flush, return
          ├─ Extended_Pictographic after ZWJ → Buffer, return
          └─ Other → Flush buffer, then handle new codepoint
```

**Implementation Steps**:

1. **Extend TerminalStateAccessor interface**
   - Add grapheme buffer fields: codepoint array, method to check/flush buffer
   - Key considerations:
     - Buffer is per-terminal-instance, not per-action
     - Must be accessible from both handlePrint and processAction

2. **Implement grapheme buffer in TerminalState**
   - Array of codepoints for current cluster
   - Flush method that creates cell string and places on grid
   - Width determination: U+FE0E present → width 1, otherwise → width 2

3. **Modify print handler buffering logic**
   - At entry to handlePrintDispatch, check if current codepoint should be buffered
   - Codepoint classification helpers: isRegionalIndicator, isSkinToneModifier, isVariationSelector
   - Key considerations:
     - ASCII fast path MUST be preserved (buffer can only be non-empty for emoji sequences, ASCII codepoints always flush first)
     - At the top of handlePrintDispatch, before the ASCII fast path, add a buffer length check: if the grapheme buffer is non-empty and the current codepoint is ASCII, flush the buffer first, then fall through to the existing ASCII fast path
     - After flushing, the new codepoint must be re-evaluated (may start new buffer or process normally)

4. **Add buffer flush on non-Print actions in processAction**
   - Before dispatching Execute, Csi, Esc, Osc, Apc, Dcs actions
   - Check if grapheme buffer has pending codepoints → flush first
   - Key considerations:
     - This ensures emoji sequence is committed before cursor moves, line feeds, etc.

**Dependencies**:
- Requires: Phase 1 (isExtendedPictographic, isEmojiPresentation, zero-width)
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests (print_handler.test.ts)*:
- Single emoji placed with width 2 and placeholder cell
- ZWJ sequence stored as single cell string with width 2
- Regional Indicator pair stored as single cell with width 2
- Skin tone modified emoji stored as single cell with width 2
- Emoji + U+FE0F stored with width 2
- Emoji + U+FE0E stored with width 1
- Buffer flushes correctly when ASCII follows emoji
- Mixed text "Hello📁World" cursor positions correctly
- Lone ZWJ (not after emoji) handled gracefully
- Lone Regional Indicator flushes when next non-RI character arrives

**Acceptance Criteria**:
- [ ] ZWJ sequence "👨‍👩‍👧" occupies 2 cells as one cluster
- [ ] "🇯🇵" (flag) occupies 2 cells
- [ ] "👋🏻" (skin tone) occupies 2 cells
- [ ] Buffer flushes on non-Print actions (cursor move, line feed)
- [ ] ASCII fast path not impacted (buffer always empty for ASCII)
- [ ] All existing print_handler tests pass

**Estimated Effort**: 中

**Risks and Mitigation**:
- **Risk**: Buffer not flushed on some edge-case action types
  - **Mitigation**: Centralized flush in processAction before all non-Print dispatches
- **Risk**: Performance regression from buffer check on every Print
  - **Mitigation**: ASCII fast path returns before buffer check; buffer check is a simple array length comparison

---

### Phase 3: Renderer Adjustments

**Goal**: Canvas renderer correctly draws multi-codepoint grapheme cluster strings stored in single cells.

**Files to Modify**:
- `src/terminal/canvas-renderer.ts`: Adjust span text iteration for cluster strings

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `renderSpanText` (modified) | Draw cluster strings and advance by cell width | Span with potential cluster chars | Correctly positioned emoji glyphs |
| `groupCellsIntoSpans` (review) | Verify cluster strings handled in span grouping | Line with emoji cells | Spans with correct cellCount |

**Processing Flow**:
```
Render span text:
1. For each character segment in span text
   ├─ Single codepoint (existing): charWidth() → advance
   └─ Multi-codepoint cluster (new):
       - Draw full string at current position via fillText
       - Advance by stored cell width (2)
       - Skip over codepoints already consumed
```

**Implementation Steps**:

1. **Review span text iteration**
   - Current loop uses `for (const char of span.text)` which iterates by codepoint
   - For a cluster string like "👨‍👩‍👧", this would iterate individual codepoints within the cluster
   - Need to detect when a "character" in the span is actually a multi-codepoint cluster
   - Key considerations:
     - `groupCellsIntoSpans` assembles span text from cell.char values
     - If a cell's char is a full cluster string ("👨‍👩‍👧"), it appears as one segment
     - The `for...of` loop treats each codepoint separately, which breaks rendering
     - Solution: track cell boundaries when building spans, or detect cluster strings during rendering

2. **Adjust column advancement for cluster strings**
   - Currently: `charWidth(char)` per codepoint → sums incorrectly for ZWJ sequences
   - Need: advance by cell width for the whole cluster, not per codepoint
   - Key considerations:
     - For single emojis, `charWidth()` returns 2 (correct after Phase 1)
     - For ZWJ sequences, iterating codepoints would advance too many columns
     - May need to refactor span text iteration to use cell-by-cell rendering

**Dependencies**:
- Requires: Phase 1 + Phase 2

**Testing Approach**:

*Manual Testing*:
- Visual verification that emoji, ZWJ sequences, and flags render at correct positions
- Verify text after emoji is aligned correctly
- Test with various terminal prompts containing emojis

**Acceptance Criteria**:
- [ ] Single emoji renders in 2-cell-wide space
- [ ] ZWJ emoji sequence renders as single glyph in 2-cell-wide space
- [ ] Text following emoji is correctly positioned
- [ ] No visual regression in CJK or ASCII rendering

**Estimated Effort**: 小

---

## Complete File Structure

```
src/terminal/
├── unicode.ts                    # Modified: emoji presentation table, zero-width ranges
├── unicode.test.ts               # Modified: emoji and zero-width test cases
├── handlers/
│   ├── types.ts                  # Modified: grapheme buffer fields on TerminalStateAccessor
│   ├── print_handler.ts          # Modified: grapheme cluster buffering logic
│   ├── print_handler.test.ts     # Modified: grapheme cluster test cases
│   └── index.ts                  # Unchanged
├── state.ts                      # Modified: grapheme buffer state, flush in processAction
├── canvas-renderer.ts            # Modified: cluster string rendering adjustments
├── canvas-renderer.test.ts       # Unchanged (or minimal additions)
└── grid.ts                       # Unchanged
```

## Testing Strategy

### Unit Testing

**Approach**: Bun test runner (`bun test`)

**Test Coverage Goals**:
- unicode.ts: 90%+ (critical lookup logic)
- print_handler.ts buffering: 80%+ (cluster assembly and flush)

**Key Test Areas**:

1. **Width Calculation** (`unicode.test.ts`)
   - Emoji_Presentation=Yes codepoints return 2
   - Zero-width characters return 0
   - ASCII and CJK unchanged
   - Negative cases (non-emoji codepoints)

2. **Grapheme Buffering** (`print_handler.test.ts`)
   - Single emoji → width 2 cell
   - ZWJ sequences → single cluster cell
   - Regional Indicator pairs → single cluster cell
   - Skin tone modifiers → single cluster cell
   - Variation Selector behavior (FE0F → 2, FE0E → 1)
   - Buffer flush on non-emoji character
   - Buffer flush on non-Print action
   - Mixed emoji/ASCII text positioning

### E2E Testing (Docker)

```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Manual Testing

- [ ] Visual verification of emoji display in terminal prompt
- [ ] ZWJ emoji sequence rendering (requires font with ZWJ support)
- [ ] Flag emoji rendering

## Dependencies

### External Dependencies
None (no new libraries)

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: unicode.ts (standalone)
2. Phase 2: print_handler.ts + state.ts + types.ts (depends on Phase 1)
3. Phase 3: canvas-renderer.ts (depends on Phase 1 + 2)

## Risk Assessment

### Technical Risks

1. **Renderer iteration for cluster strings**
   - **Risk**: `for (const char of span.text)` iterates by codepoint, not grapheme cluster
   - **Likelihood**: High (known behavior of JavaScript string iteration)
   - **Impact**: Medium (rendering position offset)
   - **Mitigation**: Track cell boundaries in span or use cell-based rendering for emoji spans

2. **Buffer flush timing**
   - **Risk**: Non-Print action processed before buffer flushed
   - **Likelihood**: Low (centralized in processAction)
   - **Impact**: High (partial emoji committed as garbage)
   - **Mitigation**: Single flush point in processAction before dispatch switch

## Performance Considerations

1. **ASCII Fast Path**: Preserved unchanged in handlePrintDispatch
2. **Emoji Width Lookup**: O(1) range checks per codepoint
3. **Grapheme Buffer**: Allocated once per terminal, typically < 20 codepoints
4. **Buffer Check Overhead**: Single array length comparison per non-ASCII character

## Open Questions

### From Specification
- [ ] Enclosing keycap sequences (#️⃣) — out of scope
- [ ] Tag-based emoji (🏴󠁧󠁢󠁳󠁣󠁴󠁿) — out of scope
- [ ] Font fallback for unsupported cluster glyphs

## References

- **Specification**: `doc/tasks/emoji-width/SPEC.md`
- Unicode 17.0 emoji-data.txt: https://www.unicode.org/Public/17.0.0/ucd/emoji/emoji-data.txt
- UAX #29 Unicode Text Segmentation: https://unicode.org/reports/tr29/
- UTS #51 Unicode Emoji: https://unicode.org/reports/tr51/
