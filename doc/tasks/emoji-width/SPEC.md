# Feature: Emoji Width Handling

## Overview

Implement accurate emoji character width calculation for the terminal emulator based on Unicode 17.0 / Emoji 17.0. This includes treating all `Emoji_Presentation=Yes` characters as width 2, handling ZWJ emoji sequences as single grapheme clusters, and correctly processing Variation Selectors, Regional Indicators, and skin tone modifiers.

## Objectives

- Fix emoji characters displaying as width 1 instead of width 2
- Support ZWJ emoji sequences as single width-2 grapheme clusters
- Handle Variation Selectors for dynamic width control (U+FE0F → width 2, U+FE0E → width 1)
- Fix zero-width characters (ZWJ, Variation Selectors) currently treated as width 1
- Track supported Unicode version for future manual updates

## User Stories

### US1: Single Emoji Width

As a terminal user, I want emojis like 📁 and 🔋 to occupy 2 cells, so that text after them aligns correctly.

**Acceptance Criteria:**
- [ ] All `Emoji_Presentation=Yes` characters are width 2
- [ ] Cursor advances by 2 after printing an emoji
- [ ] Placeholder cell (width=0) is created for the second cell

### US2: ZWJ Emoji Sequences

As a terminal user, I want compound emojis like 👨‍👩‍👧‍👦 to display as a single width-2 character, so that they don't take up excessive space.

**Acceptance Criteria:**
- [ ] ZWJ sequence codepoints are buffered into a single grapheme cluster
- [ ] The cluster occupies exactly 2 cells
- [ ] The full cluster string is stored in a single cell

### US3: Flag Emojis

As a terminal user, I want flag emojis like 🇯🇵 to display as width 2, so that they don't break line alignment.

**Acceptance Criteria:**
- [ ] Regional Indicator pairs are recognized as a single grapheme cluster
- [ ] The pair occupies 2 cells

### US4: Variation Selector Width Control

As a terminal user, I want emoji+U+FE0F to be width 2 and emoji+U+FE0E to be width 1.

**Acceptance Criteria:**
- [ ] Character followed by U+FE0F is width 2
- [ ] Character followed by U+FE0E is width 1

## Technical Requirements

### Functional Requirements

- **FR1:** `charWidth()` returns 2 for all `Emoji_Presentation=Yes` codepoints (Unicode 17.0)
- **FR2:** `charWidth()` returns 0 for ZWJ (U+200D), Variation Selectors (U+FE00-U+FE0F), ZWNJ (U+200C), ZWS (U+200B), Word Joiner (U+2060), BOM (U+FEFF), and Variation Selectors Supplement (U+E0100-U+E01EF)
- **FR3:** Print handler buffers emoji grapheme clusters (ZWJ sequences, modifier sequences, Regional Indicator pairs)
- **FR4:** Buffered grapheme cluster is stored as a single cell with width 2
- **FR5:** Variation Selector U+FE0F forces width 2; U+FE0E forces width 1
- **FR6:** ASCII fast path is preserved unchanged

### Non-Functional Requirements

- **NFR1 - Performance:** ASCII character processing must not be slower (fast path preserved)
- **NFR2 - Performance:** Emoji width lookup is O(1) using range checks
- **NFR3 - Maintainability:** Unicode version (17.0) is documented in source comments
- **NFR4 - Compatibility:** Existing CJK/Fullwidth width behavior is unchanged
- **NFR5 - Compatibility:** All existing tests pass

## Implementation Approach

### Architecture

The Rust parser remains unchanged (emits `Print(char)` per codepoint). All emoji handling is in TypeScript.

```
Rust Parser → Print(codepoint) → TypeScript Print Handler
                                      ↓
                              Grapheme Cluster Buffer
                                      ↓
                              Width Determination
                                      ↓
                              Cell Placement (Grid)
                                      ↓
                              Canvas Renderer
```

### Component Changes

#### 1. `src/terminal/unicode.ts` - Width Calculation

**Changes:**
- Add `isEmojiPresentation(cp)` function with full `Emoji_Presentation=Yes` lookup table
- Add zero-width character ranges to `charWidth()`
- Integrate `isEmojiPresentation` into `charWidth()` logic
- Add Unicode version comment header

**Width determination priority:**
```
1. ASCII fast path (0x20-0x7E) → 1
2. Control characters → 0
3. Zero-width characters (ZWJ, VS, ZWS, etc.) → 0
4. Emoji_Presentation=Yes → 2
5. East Asian Wide/Fullwidth → 2
6. Combining characters → 0
7. Default → 1
```

**Emoji_Presentation=Yes Codepoint Table (Unicode 17.0):**

BMP ranges:
```
0x231A..0x231B, 0x23E9..0x23EC, 0x23F0, 0x23F3,
0x25FD..0x25FE, 0x2614..0x2615, 0x2648..0x2653,
0x267F, 0x2693, 0x26A1, 0x26AA..0x26AB,
0x26BD..0x26BE, 0x26C4..0x26C5, 0x26CE, 0x26D4,
0x26EA, 0x26F2..0x26F3, 0x26F5, 0x26FA, 0x26FD,
0x2705, 0x270A..0x270B, 0x2728, 0x274C, 0x274E,
0x2753..0x2755, 0x2757, 0x2795..0x2797, 0x27B0,
0x27BF, 0x2B1B..0x2B1C, 0x2B50, 0x2B55
```

SMP ranges (U+1F000+):
```
0x1F004, 0x1F0CF,
0x1F18E, 0x1F191..0x1F19A,
0x1F1E6..0x1F1FF,
0x1F201, 0x1F21A, 0x1F22F,
0x1F232..0x1F236, 0x1F238..0x1F23A,
0x1F250..0x1F251,
0x1F300..0x1F320, 0x1F32D..0x1F335, 0x1F337..0x1F37C,
0x1F37E..0x1F393, 0x1F3A0..0x1F3CA, 0x1F3CF..0x1F3D3,
0x1F3E0..0x1F3F0, 0x1F3F4, 0x1F3F8..0x1F43E,
0x1F440,
0x1F442..0x1F4FC, 0x1F4FF..0x1F53D,
0x1F54B..0x1F54E, 0x1F550..0x1F567,
0x1F57A, 0x1F595..0x1F596, 0x1F5A4,
0x1F5FB..0x1F64F,
0x1F680..0x1F6C5, 0x1F6CC, 0x1F6D0..0x1F6D2,
0x1F6D5..0x1F6D8, 0x1F6DC..0x1F6DF,
0x1F6EB..0x1F6EC, 0x1F6F4..0x1F6FC,
0x1F7E0..0x1F7EB, 0x1F7F0,
0x1F90C..0x1F93A, 0x1F93C..0x1F945, 0x1F947..0x1F9FF,
0x1FA70..0x1FA77, 0x1FA78..0x1FA7C,
0x1FA80..0x1FA8A, 0x1FA8E..0x1FA8F,
0x1FA90..0x1FABD, 0x1FABE..0x1FABF,
0x1FAC0..0x1FAC6, 0x1FAC8, 0x1FACD..0x1FACF,
0x1FAD0..0x1FADC, 0x1FADF,
0x1FAE0..0x1FAEA, 0x1FAEF,
0x1FAF0..0x1FAF8
```

**Zero-width codepoints to add:**
```
0x200B          Zero Width Space
0x200C          Zero Width Non-Joiner
0x200D          Zero Width Joiner
0x2060          Word Joiner
0xFEFF          Zero Width No-Break Space / BOM
0xFE00..0xFE0F  Variation Selectors
0xE0100..0xE01EF  Variation Selectors Supplement
```

#### 2. `src/terminal/handlers/print_handler.ts` - Grapheme Cluster Buffering

**Changes:**
- Add grapheme cluster buffer state to terminal state
- Buffer codepoints that form emoji grapheme clusters
- Emit buffered cluster when boundary is detected

**Grapheme cluster buffering logic:**

```typescript
// State: buffer holds pending codepoints for the current grapheme cluster
// When a Print arrives:

if (buffer is empty) {
  if (codepoint is Extended_Pictographic or Regional_Indicator) {
    // Start buffering
    buffer.push(codepoint)
    return  // wait for more
  }
  // Normal character - process immediately (existing logic)
  processCharacter(codepoint)
  return
}

// Buffer is not empty - check if codepoint extends the cluster
if (codepoint is ZWJ(0x200D)) {
  buffer.push(codepoint)
  return  // ZWJ must be followed by another emoji
}
if (codepoint is Variation_Selector(0xFE00-0xFE0F)) {
  buffer.push(codepoint)
  return
}
if (codepoint is Skin_Tone_Modifier(0x1F3FB-0x1F3FF)) {
  buffer.push(codepoint)
  return
}
if (codepoint is Combining_Mark) {
  buffer.push(codepoint)
  return
}
if (codepoint is Regional_Indicator && buffer has exactly 1 Regional_Indicator) {
  buffer.push(codepoint)
  // Regional Indicator pair is complete - flush
  flushBuffer()
  return
}
if (buffer.lastCodepoint is ZWJ && codepoint is Extended_Pictographic) {
  buffer.push(codepoint)
  return  // ZWJ + emoji continues the cluster
}

// Codepoint does not extend the cluster - flush buffer, then process new codepoint
flushBuffer()
// Process new codepoint (may start a new buffer or process immediately)
handlePrint(codepoint)  // recursive call
```

**Buffer flush logic:**
```typescript
function flushBuffer() {
  const clusterString = String.fromCodePoint(...buffer)
  const width = determineClusterWidth(buffer)
  // Create cell with cluster string and determined width
  const cell = createCell(clusterString, attrs)
  cell.width = width
  placeCell(cell)
  buffer.clear()
}

function determineClusterWidth(codepoints): number {
  // Check if cluster contains U+FE0E (text presentation) → width 1
  if (codepoints.includes(0xFE0E)) return 1
  // All other emoji clusters → width 2
  return 2
}
```

**Timeout/fallback:** If a non-emoji action (e.g., cursor move, escape sequence) arrives while buffer is non-empty, flush the buffer first before processing the action.

#### 3. `src/terminal/canvas-renderer.ts` - Rendering

**Changes:**
- `renderSpanText`: Refactor span text iteration to handle multi-codepoint cluster strings
- The current `for (const char of span.text)` loop iterates by codepoint. When `groupCellsIntoSpans` concatenates `cell.char` values into `span.text`, a ZWJ cluster string like "👨‍👩‍👧" is decomposed into individual codepoints (👨, ZWJ, 👩, ZWJ, 👧), causing incorrect column advancement
- Need to track cell boundaries during span construction or switch to cell-based rendering for spans containing emoji clusters

**Refactoring required:** The renderer needs to iterate by cell boundary rather than by codepoint for spans containing multi-codepoint cluster strings. For single-codepoint characters (ASCII, CJK), the existing `charWidth()` approach works. For cluster strings, the renderer must draw the full string at the current position and advance by the cell's stored width (2), skipping the placeholder cell.

#### 4. `src/terminal/grid.ts` - Cell Structure

**No structural changes needed.** The existing `Cell.char` field already supports multi-character strings (combining marks are stored this way). Grapheme cluster strings are stored the same way.

### Extended_Pictographic Detection

For grapheme cluster buffering, we need to identify `Extended_Pictographic` codepoints. This is a broader set than `Emoji_Presentation`. The key ranges:

```
0x00A9, 0x00AE,                          // ©®
0x203C, 0x2049,                          // ‼⁉
0x2122, 0x2139,                          // ™ℹ
0x2194..0x2199,                          // ↔↙
0x21A9..0x21AA,                          // ↩↪
0x231A..0x231B,                          // ⌚⌛
0x2328,                                  // ⌨
0x23CF,                                  // ⏏
0x23E9..0x23F3,                          // ⏩⏳
0x23F8..0x23FA,                          // ⏸⏺
0x24C2,                                  // Ⓜ
0x25AA..0x25AB,                          // ▪▫
0x25B6,                                  // ▶
0x25C0,                                  // ◀
0x25FB..0x25FE,                          // ◻◾
0x2600..0x27BF,                          // ☀➿ (broad range)
0x2934..0x2935,                          // ⤴⤵
0x2B05..0x2B07,                          // ⬅⬇
0x2B1B..0x2B1C,                          // ⬛⬜
0x2B50,                                  // ⭐
0x2B55,                                  // ⭕
0x3030,                                  // 〰
0x303D,                                  // 〽
0x3297,                                  // ㊗
0x3299,                                  // ㊙
0x1F000..0x1F0FF,                        // Mahjong, Domino, Playing Cards
0x1F10D..0x1F10F,                        // (unassigned)
0x1F12F,                                 // (unassigned)
0x1F16C..0x1F171,                        // (various)
0x1F17E..0x1F17F,                        // 🅾🅿
0x1F18E,                                 // 🆎
0x1F191..0x1F19A,                        // 🆑🆚
0x1F1AD..0x1F1FF,                        // (flags area)
0x1F201..0x1F20F,                        // 🈁
0x1F21A,                                 // 🈚
0x1F22F,                                 // 🈯
0x1F232..0x1F23A,                        // 🈲🈺
0x1F23C..0x1F23F,                        // (unassigned)
0x1F249..0x1F3FA,                        // (broad range)
0x1F400..0x1FFFD,                        // (main emoji range to end)
```

For a practical implementation, `Extended_Pictographic` can be approximated as:
- Specific BMP codepoints listed above
- U+1F000..U+1FFFD (covers all SMP emoji blocks)

### Dependencies

**Internal Dependencies:**
- `unicode.ts`: Core width calculation (modified)
- `print_handler.ts`: Character printing (modified)
- `canvas-renderer.ts`: Rendering (minimal changes)
- `grid.ts`: Cell structure (unchanged)
- `state.ts`: Terminal state (add buffer field)

**External Dependencies:**
- None (no new libraries)

### File Structure

```
src/terminal/
├── unicode.ts              # Modified: add emoji width tables, zero-width fixes
├── handlers/
│   └── print_handler.ts    # Modified: add grapheme cluster buffering
├── canvas-renderer.ts      # Potentially modified: cluster rendering
├── grid.ts                 # Unchanged
└── state.ts                # Modified: add grapheme buffer to state
```

## Test Scenarios

### Unit Tests

#### unicode.ts tests
- [ ] `charWidth('📁')` returns 2 (U+1F4C1, Emoji_Presentation=Yes)
- [ ] `charWidth('🔋')` returns 2 (U+1F50B, Emoji_Presentation=Yes)
- [ ] `charWidth('😀')` returns 2 (U+1F600, Emoji_Presentation=Yes)
- [ ] `charWidth('🚀')` returns 2 (U+1F680, Emoji_Presentation=Yes)
- [ ] `charWidth('⌚')` returns 2 (U+231A, BMP Emoji_Presentation=Yes)
- [ ] `charWidth('⏰')` returns 2 (U+23F0, BMP Emoji_Presentation=Yes)
- [ ] `charWidth('☕')` returns 2 (U+2615, BMP Emoji_Presentation=Yes)
- [ ] `charWidth('⭐')` returns 2 (U+2B50, BMP Emoji_Presentation=Yes)
- [ ] `charWidth('\u200D')` returns 0 (ZWJ)
- [ ] `charWidth('\uFE0F')` returns 0 (Variation Selector 16)
- [ ] `charWidth('\uFE0E')` returns 0 (Variation Selector 15)
- [ ] `charWidth('\u200B')` returns 0 (Zero Width Space)
- [ ] `charWidth('\u200C')` returns 0 (ZWNJ)
- [ ] `charWidth('\uFEFF')` returns 0 (BOM/ZWNBS)
- [ ] `charWidth('A')` returns 1 (ASCII unchanged)
- [ ] `charWidth('あ')` returns 2 (CJK unchanged)
- [ ] `charWidth('漢')` returns 2 (CJK unchanged)
- [ ] `isEmojiPresentation(0x1F4C1)` returns true
- [ ] `isEmojiPresentation(0x41)` returns false (ASCII 'A')
- [ ] `isEmojiPresentation(0x2600)` returns false (☀ is NOT Emoji_Presentation=Yes)

### Integration Tests (print_handler)

- [ ] Single emoji prints at correct position with width 2
- [ ] ZWJ sequence "👨‍👩‍👧" is stored as single cell with width 2
- [ ] Regional Indicator pair "🇯🇵" is stored as single cell with width 2
- [ ] Skin tone modified emoji "👋🏻" is stored as single cell with width 2
- [ ] Emoji + U+FE0F is stored with width 2
- [ ] Emoji + U+FE0E is stored with width 1
- [ ] Buffer flushes correctly when non-emoji character follows
- [ ] Buffer flushes correctly when escape sequence arrives
- [ ] Mixed text "Hello📁World" positions correctly

### Edge Cases

- [ ] Lone ZWJ (not preceded by emoji) is handled gracefully (width 0, or ignored)
- [ ] Lone Regional Indicator (single, not pair) flushes after timeout/next char
- [ ] Very long ZWJ sequence (many joined emojis) doesn't crash
- [ ] Rapid alternation of emoji and ASCII characters works correctly
- [ ] Emoji at end of line triggers proper line wrap (wide char at col >= cols-1)

## Performance Considerations

- ASCII fast path in `handlePrintDispatch()` is **not modified** - ensures no regression for the common case
- Emoji width lookup uses a series of range checks (O(1) per character)
- Grapheme cluster buffer is typically very small (< 20 codepoints for the longest ZWJ sequences)
- Buffer is allocated once per terminal instance, not per character

## Unicode Version Management

Source file `unicode.ts` includes a version comment:
```typescript
/**
 * Unicode character width calculation.
 *
 * Based on Unicode 17.0 / Emoji 17.0 (2025-09-09)
 * Emoji_Presentation data from: https://www.unicode.org/Public/17.0.0/ucd/emoji/emoji-data.txt
 *
 * To update for a new Unicode version:
 * 1. Download the new emoji-data.txt
 * 2. Extract Emoji_Presentation=Yes entries
 * 3. Update isEmojiPresentation() ranges
 * 4. Update this version comment
 */
```

## Success Criteria

- [ ] All `Emoji_Presentation=Yes` codepoints return width 2
- [ ] ZWJ emoji sequences display as single width-2 characters
- [ ] Variation Selectors correctly control width (FE0F→2, FE0E→1)
- [ ] Zero-width characters (ZWJ, VS, ZWS, etc.) return width 0
- [ ] Regional Indicator pairs display as width 2
- [ ] Skin tone modified emojis display as width 2
- [ ] No regression in ASCII or CJK character width
- [ ] All existing tests pass
- [ ] Unicode version documented in source

## Open Questions

- [ ] Enclosing keycap sequences (#️⃣) — out of scope for this task
- [ ] Tag-based emoji (🏴󠁧󠁢󠁳󠁣󠁴󠁿) — out of scope for this task
- [ ] How should the renderer handle cluster strings that the font cannot render as a single glyph? (Fallback: display individual emojis)

## References

- Unicode 17.0 emoji-data.txt: https://www.unicode.org/Public/17.0.0/ucd/emoji/emoji-data.txt
- UAX #29 Unicode Text Segmentation: https://unicode.org/reports/tr29/
- UTS #51 Unicode Emoji: https://unicode.org/reports/tr51/
- UAX #11 East Asian Width: https://unicode.org/reports/tr11/
- Kitty Text Sizing Protocol: https://github.com/kovidgoyal/kitty/blob/master/docs/text-sizing-protocol.rst
- WezTerm widechar_width.rs: https://github.com/wez/wezterm/blob/main/termwiz/src/widechar_width.rs
- Alacritty unicode-width-16: https://github.com/alacritty/unicode-width-16
