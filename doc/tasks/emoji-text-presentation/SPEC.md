# Feature: Emoji vs Text Presentation Rendering

## Overview

Force text presentation (monochrome glyph) for Extended_Pictographic characters that have `Emoji_Presentation=No` and no variation selector. Currently, these characters (e.g., `✳ ☀ © ® ™`) are rendered as color emoji due to system font fallback, even though they should display as monochrome text symbols by default.

## Background

Unicode defines two presentation modes for characters with both text and emoji forms:

| Presentation | Trigger | Glyph | Example |
|---|---|---|---|
| Text (default for EP=No) | No VS, or VS15 (U+FE0E) | Monochrome | ✳︎ ☀︎ ©︎ |
| Emoji (default for EP=Yes) | VS16 (U+FE0F), or EP=Yes default | Color | ✳️ ☀️ 😀 |

The canvas renderer currently passes characters directly to `ctx.fillText()` without indicating presentation preference. The system font fallback often selects color emoji glyphs for Extended_Pictographic characters, regardless of their default presentation.

## Objectives

- Extended_Pictographic characters with Emoji_Presentation=No render as monochrome text symbols
- Characters with VS16 (U+FE0F) or Emoji_Presentation=Yes continue to render as color emoji
- Characters with VS15 (U+FE0E) continue to render as monochrome text
- No changes to cell structure, width calculation, or WASM code

## Technical Requirements

### Functional Requirements

- **FR1:** In `drawFittedCharacter()`, append U+FE0E to Extended_Pictographic characters that do not already contain a variation selector (U+FE0E or U+FE0F)
- **FR2:** `drawWideCharacter()` is NOT modified — width 2 characters (Emoji_Presentation=Yes, VS16, CJK) render as before
- **FR3:** Characters that already contain VS15 or VS16 in the cluster string are not modified
- **FR4:** The glyph width cache correctly uses the VS15-appended string as the cache key

### Non-Functional Requirements

- **NFR1:** No performance regression — `isExtendedPictographic()` is a simple range check, and the VS15 append only affects a small subset of characters
- **NFR2:** No changes to cell structure (`Cell`), WASM code, or width calculation logic

## Implementation

### Change 1: `src/terminal/canvas-renderer.ts` — `drawFittedCharacter()`

In `drawFittedCharacter()`, before measuring and drawing:

```typescript
private drawFittedCharacter(char: string, x: number, textY: number): void {
    // Force text presentation for Extended_Pictographic without VS
    const cp = char.codePointAt(0)!;
    if (isExtendedPictographic(cp) && !hasVariationSelector(char)) {
        char = char + '\uFE0E';
    }

    // ... existing measurement and drawing logic (unchanged)
}
```

### Change 2: Helper function — `hasVariationSelector()`

Add a local helper (in canvas-renderer.ts or unicode.ts):

```typescript
function hasVariationSelector(s: string): boolean {
    for (let i = 0; i < s.length; i++) {
        const c = s.charCodeAt(i);
        if (c === 0xFE0E || c === 0xFE0F) return true;
    }
    return false;
}
```

### Import

Add `isExtendedPictographic` to the existing imports from unicode utilities in canvas-renderer.ts.

### Not Changed

- `drawWideCharacter()` — emoji/CJK at width >= 2
- `Cell` struct in `wasm/src/cell.rs`
- `flush_grapheme_buffer()` in Rust or TypeScript
- `charWidth()` / `char_width()`
- Width calculation logic

## Affected Characters

Characters that change from color emoji to monochrome text rendering (Extended_Pictographic, Emoji_Presentation=No, no VS):

| Range | Examples |
|-------|----------|
| U+00A9, U+00AE | ©, ® |
| U+203C, U+2049 | ‼, ⁉ |
| U+2122, U+2139 | ™, ℹ |
| U+2194..U+2199 | ↔, ↕, ↖, ↗, ↘, ↙ |
| U+2328 | ⌨ |
| U+2600..U+26FF | ☀, ☁, ☂, ⚙, ✳, etc. |
| U+2702..U+27BF | ✂, ✈, ✉, etc. |
| U+2764 | ❤ |

These characters revert to color emoji when followed by VS16 (U+FE0F), which is existing behavior.

## Rendering Path Summary

```
renderSpanText() cell loop:
  ├─ cellWidth >= 2 → drawWideCharacter()    [no change]
  ├─ non-ASCII, width 1 → drawFittedCharacter()  [append VS15 here]
  └─ ASCII → ctx.fillText()                  [no change]
```

## Test Scenarios

- [ ] `✳` (U+2733) renders as monochrome text symbol at width 1
- [ ] `☀` (U+2600) renders as monochrome text symbol at width 1
- [ ] `©` (U+00A9) renders as monochrome text symbol at width 1
- [ ] `✳️` (U+2733 + U+FE0F) renders as color emoji at width 2
- [ ] `☀️` (U+2600 + U+FE0F) renders as color emoji at width 2
- [ ] `😀` (Emoji_Presentation=Yes) renders as color emoji at width 2 (unchanged)
- [ ] `😀︎` (U+1F600 + U+FE0E) renders as monochrome at width 1 (unchanged)
- [ ] ASCII characters are not affected
- [ ] CJK characters are not affected
- [ ] Claude Code spinner (`✳` cycling) displays correctly without layout corruption

## Verification

```bash
# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# TypeScript typecheck
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

Manual: Verify `✳ ☀ © ® ™` display as monochrome in terminal, and `✳️ ☀️` display as color emoji.
