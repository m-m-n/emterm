# Feature: Ambiguous Width Rendering

## Overview

Handle East Asian Width Ambiguous (EAW=A) characters by keeping grid width at 1 cell for TUI app compatibility, while using Canvas `measureText()` at render time to shrink oversized glyphs (e.g. ■, ○, △) to fit within a single cell. This replaces the previous "smart table" approach that selectively treated some EAW=A characters as width 2, which broke TUI applications like lazygit.

## Objectives

- Ensure all EAW=A characters occupy exactly 1 grid cell (matching `wcwidth()` behavior of TUI apps)
- Shrink-to-fit rendering for non-ASCII characters that overflow 1 cell width
- Remove the previous `ambiguous_width` setting and all associated code
- Maintain backward compatibility for saved config files containing the removed setting

## User Stories

### US1: TUI Application Compatibility
As a user running TUI applications (lazygit, htop, etc.), I want box-drawing characters, arrows, and other EAW=A symbols to occupy exactly 1 cell in the grid, so that TUI layouts render correctly.

**Acceptance Criteria:**
- [ ] lazygit borders, arrows, and status indicators display without misalignment
- [ ] Box-drawing characters (U+2500 etc.) render at 1-cell width
- [ ] All EAW=A characters are width 1 in the terminal grid

### US2: Oversized Glyph Rendering
As a user, I want characters like ■ (U+25A0), ○ (U+25CB), and △ (U+25B3) to render fully visible within a single cell, even when the font renders them wider than 1 cell.

**Acceptance Criteria:**
- [ ] Characters wider than 1 cell are scaled down to fit
- [ ] Characters that already fit in 1 cell render at normal size
- [ ] ASCII characters (U+0000-U+007F) bypass measurement for performance

### US3: Setting Removal
As a user who previously had the `ambiguous_width` setting, I want the application to gracefully ignore the old setting in my config file without errors.

**Acceptance Criteria:**
- [ ] Config files with `ambiguous_width` field load without errors
- [ ] The setting toggle is removed from the UI
- [ ] No runtime errors from removed code paths

## Technical Requirements

### Functional Requirements

- **FR1: Grid width = 1 for all EAW=A** - The WASM terminal core treats all East Asian Width Ambiguous characters as width 1. No `ambiguous_width_wide` flag or conditional width-2 override.
- **FR2: Shrink-to-fit rendering** - The Canvas renderer measures non-ASCII characters via `ctx.measureText()` and scales down any glyph wider than `this.charWidth` to fit within 1 cell.
- **FR3: ASCII fast path** - Characters with code point <= 0x7F skip measurement and render directly via `ctx.fillText()`, as monospace fonts guarantee they fit in 1 cell.
- **FR4: Glyph width cache** - A per-font `Map<string, number>` cache stores measured widths to avoid repeated `measureText()` calls. Cache is cleared on font change.
- **FR5: Setting removal** - Remove `ambiguous_width` from TypeScript `AppSettings`, settings UI, settings applier, and terminal state. In Rust config, use `serde(skip)` for backward-compatible deserialization.
- **FR6: Combining character table sync** - TypeScript `isCombiningChar()` uses the same comprehensive Unicode 17.0 table as the WASM `is_combining_char()`, ensuring cross-validation tests pass.

### Non-Functional Requirements

- **NFR1 - Performance:** ASCII characters (the majority of terminal output) must not incur `measureText()` overhead. The glyph width cache must minimize repeated measurements for non-ASCII characters.
- **NFR2 - Compatibility:** Grid behavior must match POSIX `wcwidth()` for EAW=A characters (width 1), ensuring correct layout in all TUI applications.
- **NFR3 - Backward Compatibility:** Existing config files containing `ambiguous_width: true/false` must deserialize without error. The field is silently ignored.

## Implementation Approach

### Architecture

```
PTY output → WASM parser → Grid (all EAW=A = width 1)
                                    ↓
                           Canvas Renderer
                                    ↓
                    ┌───────────────┴───────────────┐
                    │ ASCII (≤0x7F)                  │ Non-ASCII (>0x7F)
                    │ fillText() direct              │ drawFittedCharacter()
                    └───────────────┬───────────────┘
                                    │
                         measureText() + cache
                                    │
                    ┌───────────────┴───────────────┐
                    │ width ≤ charWidth              │ width > charWidth
                    │ fillText() normal              │ scale + fillText()
                    └───────────────────────────────┘
```

### Data Flow

The `drawFittedCharacter()` method:

1. Look up font key (`ctx.font` string) in outer cache map
2. Look up character in inner cache map
3. If not cached, call `ctx.measureText(char).width` and cache result
4. If measured width <= `this.charWidth`: render normally
5. If measured width > `this.charWidth`: apply `ctx.scale(charWidth / measured, ...)` transform and render centered

### Scaling Algorithm

```typescript
const scale = this.charWidth / measured;
ctx.save();
ctx.translate(x + charWidth / 2, textY);
ctx.scale(scale, scale);
ctx.fillText(char, -measured / 2, 0);
ctx.restore();
```

This is the same approach used by `drawWideCharacter()` for CJK characters spanning 2 cells.

### Cache Structure

```typescript
// Outer: ctx.font string → Inner: character → measured width
private glyphWidthCache: Map<string, Map<string, number>>
```

Cleared in `measureCharacterSize()` when font configuration changes.

### Removed Code

| Component | Removed |
|-----------|---------|
| WASM `terminal_core.rs` | `ambiguous_width_wide` field, `set_ambiguous_width_wide()` |
| WASM `print_handler.rs` | Width-2 override for ambiguous+non-narrow characters |
| WASM `c0_handler.rs` | BS ambiguous spacer compensation |
| WASM `csi_cursor.rs` | CSI C/D ambiguous width adjustment |
| WASM `unicode.rs` | `AMBIGUOUS_NARROW_RANGES`, `is_ambiguous_narrow()` |
| WASM `lib.rs` | `is_ambiguous_narrow` wasm_bindgen export |
| TS `unicode.ts` | `AMBIGUOUS_NARROW_RANGES`, `isAmbiguousNarrow()` |
| TS `wasm/unicode.ts` | `isAmbiguousNarrow` wrapper |
| TS `settings/types.ts` | `ambiguous_width` field |
| TS `settings-sections.ts` | Toggle UI for ambiguous width |
| TS `settings-applier.ts` | `applyAmbiguousWidth()`, `RendererSettings.ambiguousWidth` |
| TS `terminal-app/index.ts` | Setting handler and cached init |
| TS `state.ts` | `ambiguousWidthWide` field, `setAmbiguousWidthWide()` |
| Rust `config.rs` | `ambiguous_width` changed to `serde(skip)` |
| i18n `en.json`, `ja.json` | `ambiguousWidth`, `ambiguousWidthDesc` keys |
| Tests | "Ambiguous Width via WASM", "isAmbiguousNarrow" test blocks |
| E2E | `ambiguous-width.e2e.js` |

### File Structure

```
wasm/src/
├── print_handler.rs      # Removed ambiguous width-2 override
├── c0_handler.rs         # Removed BS ambiguous compensation
├── csi_cursor.rs         # Removed CSI C/D ambiguous adjustment
├── unicode.rs            # Removed AMBIGUOUS_NARROW_RANGES, is_ambiguous_narrow()
├── lib.rs                # Removed is_ambiguous_narrow export
└── terminal_core.rs      # Removed ambiguous_width_wide field

src/terminal/
├── canvas-renderer.ts    # Added glyphWidthCache, drawFittedCharacter()
├── unicode.ts            # Removed ambiguous narrow; synced combining table
├── state.ts              # Removed ambiguousWidthWide
└── wasm/unicode.ts       # Removed isAmbiguousNarrow wrapper

src/settings/
├── types.ts              # Removed ambiguous_width field
├── settings-sections.ts  # Removed toggle UI
└── settings-applier.ts   # Removed applyAmbiguousWidth()

src-tauri/src/commands/
└── config.rs             # ambiguous_width: serde(skip)
```

## Test Scenarios

### Unit Tests
- [ ] WASM: EAW=A characters return width 1 from `char_width()`
- [ ] WASM: `is_ambiguous_width()` correctly identifies EAW=A code points
- [ ] TS: `charWidth()` returns 1 for EAW=A characters
- [ ] TS: `isCombiningChar()` matches WASM `is_combining_char()` for full BMP

### Cross-Validation Tests
- [ ] TS vs WASM: `charWidth` matches for entire BMP (U+0000..U+FFFF)
- [ ] TS vs WASM: `isCombiningChar` matches for entire BMP

### Manual Verification
- [ ] `printf '\u25a0ABC'` - ■ renders scaled down in 1 cell, ABC follows immediately
- [ ] `printf '\u03b1ABC'` - α renders at normal size in 1 cell
- [ ] `printf '\u2500\u2500\u2500'` - box-drawing lines render consecutively at 1-cell width
- [ ] lazygit borders and status indicators display correctly

## Success Criteria

- [ ] All EAW=A characters occupy 1 grid cell
- [ ] Oversized glyphs render scaled down within 1 cell
- [ ] ASCII characters have zero `measureText()` overhead
- [ ] WASM tests pass (474 tests)
- [ ] Rust backend tests pass (450 tests)
- [ ] TypeScript tests pass (1920 tests)
- [ ] TypeScript typecheck passes
- [ ] No regression in TUI application rendering

## References

- Unicode Standard Annex #11: East Asian Width
- POSIX `wcwidth()` specification
