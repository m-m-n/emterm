# Feature: Color Scheme Settings

## Overview

Implement WezTerm-compatible color scheme as the default colors for eMterm terminal emulator. This initial implementation uses hardcoded values, with future phases planned for user customization through configuration files.

## Objectives

- Replace current hardcoded colors with WezTerm color scheme
- Ensure consistent color application across all terminal components
- Maintain compatibility with existing ANSI, 256-color, and True Color functionality
- Establish foundation for future color customization features

## User Stories

### US1: Display Terminal with WezTerm Colors
As a user, I want eMterm to display with WezTerm's color scheme, so that I have a familiar visual environment.

**Acceptance Criteria:**
- [ ] Foreground color displays as #40ff40 (bright green)
- [ ] Background color displays as #000000 (black)
- [ ] Cursor color displays as #008000 (dark green)
- [ ] All 16 ANSI colors match WezTerm configuration
- [ ] Terminal is visually consistent throughout the application

## Technical Requirements

### Functional Requirements
- **FR1:** Update PALETTE_16 constant in colors.ts with WezTerm colors
- **FR2:** Update DEFAULT_FOREGROUND to #40ff40
- **FR3:** Update DEFAULT_BACKGROUND to #000000
- **FR4:** Update cursor color to #008000 in renderer.ts
- **FR5:** Update CSS hardcoded colors in styles.css
- **FR6:** Ensure 256-color palette indices 0-15 reflect updated PALETTE_16

### Non-Functional Requirements
- **NFR1 - Performance:** No runtime overhead (colors are compile-time constants)
- **NFR2 - Compatibility:** Maintain backward compatibility with all SGR sequences
- **NFR3 - Maintainability:** Keep color definitions centralized in colors.ts where possible

## Implementation Approach

### Architecture

The color system consists of three layers:

```
┌─────────────────────────────────────────────┐
│              CSS (styles.css)               │
│  - Body background/foreground               │
│  - Terminal container colors                │
│  - IME composition view colors              │
├─────────────────────────────────────────────┤
│           TypeScript (colors.ts)            │
│  - PALETTE_16: ANSI 16 colors               │
│  - PALETTE_256: Full 256-color palette      │
│  - DEFAULT_FOREGROUND / DEFAULT_BACKGROUND  │
├─────────────────────────────────────────────┤
│          Renderer (renderer.ts)             │
│  - Cursor color (in addCursorStyles)        │
│  - Color application to cells               │
└─────────────────────────────────────────────┘
```

### Color Values Reference

WezTerm color scheme values to implement:

| Color | Hex | RGB |
|-------|-----|-----|
| Foreground | #40ff40 | rgb(64, 255, 64) |
| Background | #000000 | rgb(0, 0, 0) |
| Cursor | #008000 | rgb(0, 128, 0) |
| ANSI 0 (Black) | #000000 | rgb(0, 0, 0) |
| ANSI 1 (Red) | #ff0000 | rgb(255, 0, 0) |
| ANSI 2 (Green) | #00dd00 | rgb(0, 221, 0) |
| ANSI 3 (Yellow) | #eeee00 | rgb(238, 238, 0) |
| ANSI 4 (Blue) | #4040ff | rgb(64, 64, 255) |
| ANSI 5 (Magenta) | #ff00ff | rgb(255, 0, 255) |
| ANSI 6 (Cyan) | #00dddd | rgb(0, 221, 221) |
| ANSI 7 (White) | #dedacf | rgb(222, 218, 207) |
| ANSI 8 (Bright Black) | #555555 | rgb(85, 85, 85) |
| ANSI 9 (Bright Red) | #ff6060 | rgb(255, 96, 96) |
| ANSI 10 (Bright Green) | #60ff60 | rgb(96, 255, 96) |
| ANSI 11 (Bright Yellow) | #ffff60 | rgb(255, 255, 96) |
| ANSI 12 (Bright Blue) | #6060ff | rgb(96, 96, 255) |
| ANSI 13 (Bright Magenta) | #ff60ff | rgb(255, 96, 255) |
| ANSI 14 (Bright Cyan) | #60ffff | rgb(96, 255, 255) |
| ANSI 15 (Bright White) | #ffffff | rgb(255, 255, 255) |

### File Changes

#### 1. src/terminal/colors.ts

**Change PALETTE_16:**

```typescript
export const PALETTE_16: readonly Rgb[] = Object.freeze([
	// Standard colors (0-7) - WezTerm scheme
	{ r: 0x00, g: 0x00, b: 0x00 }, // 0: Black (#000000)
	{ r: 0xff, g: 0x00, b: 0x00 }, // 1: Red (#ff0000)
	{ r: 0x00, g: 0xdd, b: 0x00 }, // 2: Green (#00dd00)
	{ r: 0xee, g: 0xee, b: 0x00 }, // 3: Yellow (#eeee00)
	{ r: 0x40, g: 0x40, b: 0xff }, // 4: Blue (#4040ff)
	{ r: 0xff, g: 0x00, b: 0xff }, // 5: Magenta (#ff00ff)
	{ r: 0x00, g: 0xdd, b: 0xdd }, // 6: Cyan (#00dddd)
	{ r: 0xde, g: 0xda, b: 0xcf }, // 7: White (#dedacf)

	// Bright colors (8-15) - WezTerm scheme
	{ r: 0x55, g: 0x55, b: 0x55 }, // 8: Bright Black (#555555)
	{ r: 0xff, g: 0x60, b: 0x60 }, // 9: Bright Red (#ff6060)
	{ r: 0x60, g: 0xff, b: 0x60 }, // 10: Bright Green (#60ff60)
	{ r: 0xff, g: 0xff, b: 0x60 }, // 11: Bright Yellow (#ffff60)
	{ r: 0x60, g: 0x60, b: 0xff }, // 12: Bright Blue (#6060ff)
	{ r: 0xff, g: 0x60, b: 0xff }, // 13: Bright Magenta (#ff60ff)
	{ r: 0x60, g: 0xff, b: 0xff }, // 14: Bright Cyan (#60ffff)
	{ r: 0xff, g: 0xff, b: 0xff }, // 15: Bright White (#ffffff)
]);
```

**Change DEFAULT_FOREGROUND and DEFAULT_BACKGROUND:**

```typescript
/**
 * Default foreground color (WezTerm: bright green).
 */
export const DEFAULT_FOREGROUND: Rgb = { r: 0x40, g: 0xff, b: 0x40 };

/**
 * Default background color (WezTerm: black).
 */
export const DEFAULT_BACKGROUND: Rgb = { r: 0x00, g: 0x00, b: 0x00 };
```

#### 2. src/styles.css

**Update hardcoded colors:**

```css
html,
body {
  height: 100%;
  overflow: hidden;
  background-color: #000000;  /* Changed from #1e1e1e */
}

body {
  font-family: "Menlo", "Monaco", "Courier New", monospace;
  color: #40ff40;  /* Changed from #d4d4d4 */
}

#terminal {
  /* ... */
  background-color: #000000;  /* Changed from #1e1e1e */
  /* ... */
}

/* IME Composition View */
.ime-composition {
  /* ... */
  background: #000000;  /* Changed from #1e1e1e */
  color: #40ff40;  /* Changed from #d4d4d4 */
  /* ... */
}
```

#### 3. src/terminal/renderer.ts

**Update cursor styles in addCursorStyles():**

```typescript
private addCursorStyles(): void {
  // ...
  style.textContent = `
    @keyframes cursor-blink {
      0%, 50% { opacity: 1; }
      51%, 100% { opacity: 0; }
    }
    .terminal-cursor.blink {
      animation: cursor-blink 1s step-end infinite;
    }
    .terminal-cursor.block {
      background-color: #008000;  /* Changed from #c0c0c0 */
    }
    .terminal-cursor.underline {
      background-color: transparent;
      border-bottom: 2px solid #008000;  /* Changed from #c0c0c0 */
    }
    .terminal-cursor.bar {
      background-color: transparent;
      border-left: 2px solid #008000;  /* Changed from #c0c0c0 */
      width: 2px !important;
    }
  `;
  // ...
}
```

### Dependencies

**Internal Dependencies:**
- `src/terminal/colors.ts`: Core color definitions
- `src/terminal/renderer.ts`: Uses colors for rendering
- `src/terminal/style-cache.ts`: Caches CSS classes using colors
- `src/styles.css`: Global CSS styles

**External Dependencies:**
- None (no new dependencies required)

### File Structure

No new files are created. Modified files:

```
src/
├── terminal/
│   ├── colors.ts           # Update PALETTE_16, DEFAULT_FOREGROUND/BACKGROUND
│   └── renderer.ts         # Update cursor color in addCursorStyles()
└── styles.css              # Update hardcoded colors
```

## Test Scenarios

### Unit Tests
- [ ] Test that PALETTE_16 contains exactly 16 colors
- [ ] Test that PALETTE_16[0] equals { r: 0, g: 0, b: 0 } (black)
- [ ] Test that DEFAULT_FOREGROUND equals { r: 64, g: 255, b: 64 }
- [ ] Test that DEFAULT_BACKGROUND equals { r: 0, g: 0, b: 0 }
- [ ] Test that indexToRgb(0-15) returns correct WezTerm colors

### Integration Tests
- [ ] Test that standard color SGR codes (30-37) produce correct colors
- [ ] Test that bright color SGR codes (90-97) produce correct colors
- [ ] Test that background color SGR codes (40-47, 100-107) produce correct colors
- [ ] Test that 256-color mode still works correctly
- [ ] Test that True Color mode (24-bit) still works correctly

### Visual Tests
- [ ] Verify terminal displays with green foreground on black background
- [ ] Verify cursor displays in dark green (#008000)
- [ ] Verify ANSI color output matches WezTerm exactly
- [ ] Verify IME composition view uses correct colors

### Edge Cases
- [ ] Color reset (SGR 0) returns to default foreground/background
- [ ] Reverse video (SGR 7) swaps foreground and background correctly
- [ ] Bold attribute with standard colors uses bright variants

## Security Considerations

- **Input Validation:** N/A - all values are hardcoded constants
- **No external input:** Colors are not read from user input or files in this phase

## Error Handling

No error handling required for this feature - all color values are compile-time constants.

## Performance Optimization

### Performance Goals
- Zero runtime overhead (constants are resolved at compile time)
- No change to rendering performance

### Optimization Strategies
- Use `Object.freeze()` for immutable color arrays
- Colors are resolved once during module initialization

## Success Criteria

- [ ] All ANSI 16 colors match WezTerm configuration exactly
- [ ] Default foreground displays as #40ff40 (bright green)
- [ ] Default background displays as #000000 (black)
- [ ] Cursor displays as #008000 (dark green)
- [ ] All existing tests pass without modification
- [ ] 256-color and True Color functionality unchanged
- [ ] No visual regressions in Markdown viewer or image viewer
- [ ] Type check passes (`bun run typecheck`)
- [ ] All tests pass (`bun test` and `cargo test`)

## Future Phases

### Phase 2: Configuration File Support
- Read colors from `~/.config/emterm/colors.toml`
- Fall back to hardcoded defaults if file not found
- Hot-reload colors when config file changes

### Phase 3: UI Color Picker
- Settings panel with color scheme selection
- Preset themes (WezTerm, Solarized, Dracula, etc.)
- Custom color scheme editor

## References

- WezTerm configuration: `~/.config/wezterm/wezterm.lua`
- Current implementation: `src/terminal/colors.ts`
- ANSI SGR codes: https://en.wikipedia.org/wiki/ANSI_escape_code#SGR_(Select_Graphic_Rendition)_parameters
