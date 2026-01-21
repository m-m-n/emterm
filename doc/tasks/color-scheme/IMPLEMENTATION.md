# Implementation Plan: Color Scheme Settings

## Overview

Update eMterm terminal emulator to use WezTerm-compatible color scheme as the default colors. This is a configuration-only change that replaces hardcoded color values in three source files.

## Objectives

- Replace current hardcoded colors with WezTerm color scheme
- Ensure consistent color application across all terminal components (CSS, TypeScript, renderer)
- Maintain backward compatibility with existing ANSI, 256-color, and True Color functionality

## Prerequisites

### Development Environment
- Node.js / Bun for TypeScript development
- Tauri development environment for testing

### Dependencies
- No new dependencies required
- All changes are to existing constants and hardcoded values

### Knowledge Requirements
- Understanding of terminal color palettes (16-color ANSI, 256-color, True Color)
- CSS color syntax
- TypeScript/JavaScript constants

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), CSS
- **Build**: Bun bundler

### Design Approach

The color system has three independent layers that must be synchronized:

```
+-------------------------------------------+
|              CSS (styles.css)              |
|  - Body background/foreground              |
|  - Terminal container colors               |
|  - IME composition view colors             |
+-------------------------------------------+
                    |
+-------------------------------------------+
|         TypeScript (colors.ts)             |
|  - PALETTE_16: ANSI 16 colors              |
|  - PALETTE_256: Full 256-color palette     |
|  - DEFAULT_FOREGROUND / DEFAULT_BACKGROUND |
+-------------------------------------------+
                    |
+-------------------------------------------+
|          Renderer (renderer.ts)            |
|  - Cursor color (in addCursorStyles)       |
|  - Color application to cells              |
+-------------------------------------------+
```

Each layer uses hardcoded values. This implementation updates all three layers to use the same WezTerm color scheme.

## Implementation Phases

### Phase 1: Update TypeScript Color Definitions

**Goal**: Update all color constants in colors.ts to WezTerm values

**Files to Modify**:
- `src/terminal/colors.ts`:
  - Update PALETTE_16 array with 16 WezTerm colors
  - Update DEFAULT_FOREGROUND to #40ff40
  - Update DEFAULT_BACKGROUND to #000000

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| PALETTE_16 | Define 16 ANSI colors | Contains current palette values | Contains WezTerm palette values |
| DEFAULT_FOREGROUND | Define default text color | { r: 229, g: 229, b: 229 } | { r: 0x40, g: 0xff, b: 0x40 } |
| DEFAULT_BACKGROUND | Define default background | { r: 0, g: 0, b: 0 } | { r: 0, g: 0, b: 0 } (unchanged) |

**Color Value Reference**:

| Index | Name | Current | WezTerm Target |
|-------|------|---------|----------------|
| 0 | Black | #000000 | #000000 |
| 1 | Red | #cd3131 | #ff0000 |
| 2 | Green | #0dbc79 | #00dd00 |
| 3 | Yellow | #e5e510 | #eeee00 |
| 4 | Blue | #2472c8 | #4040ff |
| 5 | Magenta | #bc3fbc | #ff00ff |
| 6 | Cyan | #11a8cd | #00dddd |
| 7 | White | #e5e5e5 | #dedacf |
| 8 | Bright Black | #666666 | #555555 |
| 9 | Bright Red | #f14c4c | #ff6060 |
| 10 | Bright Green | #23d18b | #60ff60 |
| 11 | Bright Yellow | #f5f543 | #ffff60 |
| 12 | Bright Blue | #3b8eea | #6060ff |
| 13 | Bright Magenta | #d670d6 | #ff60ff |
| 14 | Bright Cyan | #29b8db | #60ffff |
| 15 | Bright White | #ffffff | #ffffff |

**Implementation Steps**:

1. **Update PALETTE_16 constant**
   - Replace all 16 RGB values with WezTerm palette
   - Preserve Object.freeze() for immutability
   - Keep existing comments updated with new hex values

2. **Update DEFAULT_FOREGROUND**
   - Change from { r: 229, g: 229, b: 229 } to { r: 0x40, g: 0xff, b: 0x40 }
   - Update JSDoc comment to mention WezTerm bright green

3. **Verify PALETTE_256 auto-updates**
   - PALETTE_256 copies from PALETTE_16 (indices 0-15)
   - No manual change needed; verify generate256Palette() behavior

**Dependencies**:
- Requires: None (first phase)
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests*:
- Test PALETTE_16 contains exactly 16 entries
- Test PALETTE_16[0] equals { r: 0, g: 0, b: 0 }
- Test PALETTE_16[1] equals { r: 255, g: 0, b: 0 } (WezTerm red)
- Test DEFAULT_FOREGROUND equals { r: 64, g: 255, b: 64 }
- Test indexToRgb(0-15) returns correct WezTerm colors
- Test indexToRgb(16-255) still returns correct 6x6x6 cube and grayscale

**Acceptance Criteria**:
- [ ] PALETTE_16 contains all 16 WezTerm colors
- [ ] DEFAULT_FOREGROUND is { r: 64, g: 255, b: 64 }
- [ ] DEFAULT_BACKGROUND is { r: 0, g: 0, b: 0 }
- [ ] indexToRgb(0-15) returns correct values
- [ ] Type check passes (`bun run typecheck`)
- [ ] Existing color tests pass

**Estimated Effort**: Small (1-2 hours)

---

### Phase 2: Update CSS Hardcoded Colors

**Goal**: Update all hardcoded colors in styles.css to match WezTerm theme

**Files to Modify**:
- `src/styles.css`:
  - Update html, body background-color from #1e1e1e to #000000
  - Update body color from #d4d4d4 to #40ff40
  - Update #terminal background-color from #1e1e1e to #000000
  - Update .ime-composition background and color

**Key Components**:

| Component | Responsibility | Current Value | Target Value |
|-----------|----------------|---------------|--------------|
| html, body background-color | Page background | #1e1e1e | #000000 |
| body color | Default text color | #d4d4d4 | #40ff40 |
| #terminal background-color | Terminal container background | #1e1e1e | #000000 |
| .ime-composition background | IME popup background | #1e1e1e | #000000 |
| .ime-composition color | IME popup text | #d4d4d4 | #40ff40 |

**Implementation Steps**:

1. **Update global colors**
   - Change `html, body { background-color: #1e1e1e; }` to `#000000`
   - Change `body { color: #d4d4d4; }` to `#40ff40`

2. **Update terminal container**
   - Change `#terminal { background-color: #1e1e1e; }` to `#000000`

3. **Update IME composition view**
   - Change `.ime-composition { background: #1e1e1e; }` to `#000000`
   - Change `.ime-composition { color: #d4d4d4; }` to `#40ff40`

**Dependencies**:
- Requires: Phase 1 (for consistency verification)
- Blocks: None

**Testing Approach**:

*Manual Testing*:
- Verify terminal displays black background
- Verify default text is bright green (#40ff40)
- Verify IME composition popup has correct colors

**Acceptance Criteria**:
- [ ] Terminal background is black (#000000)
- [ ] Body text color is bright green (#40ff40)
- [ ] IME composition view matches terminal colors
- [ ] No visual artifacts or color inconsistencies

**Estimated Effort**: Small (30 minutes - 1 hour)

---

### Phase 3: Update Cursor Color

**Goal**: Update cursor color in renderer.ts to WezTerm dark green (#008000)

**Files to Modify**:
- `src/terminal/renderer.ts`:
  - Update addCursorStyles() method cursor colors from #c0c0c0 to #008000

**Key Components**:

| Component | Responsibility | Current Value | Target Value |
|-----------|----------------|---------------|--------------|
| .terminal-cursor.block | Block cursor background | #c0c0c0 | #008000 |
| .terminal-cursor.underline | Underline cursor border | #c0c0c0 | #008000 |
| .terminal-cursor.bar | Bar cursor border | #c0c0c0 | #008000 |

**Implementation Steps**:

1. **Locate addCursorStyles() method**
   - Find the style.textContent assignment in renderer.ts

2. **Update cursor color values**
   - Change block cursor: `background-color: #c0c0c0` to `#008000`
   - Change underline cursor: `border-bottom: 2px solid #c0c0c0` to `#008000`
   - Change bar cursor: `border-left: 2px solid #c0c0c0` to `#008000`

**Dependencies**:
- Requires: None (independent of Phase 1 and 2)
- Blocks: None

**Testing Approach**:

*Manual Testing*:
- Verify block cursor displays as dark green (#008000)
- Verify underline cursor border is dark green
- Verify bar cursor border is dark green
- Verify cursor blink animation still works

**Acceptance Criteria**:
- [ ] Block cursor is dark green (#008000)
- [ ] Underline cursor border is dark green
- [ ] Bar cursor border is dark green
- [ ] Cursor blink animation functions correctly

**Estimated Effort**: Small (30 minutes)

---

## Complete File Structure

No new files are created. Modified files only:

```
src/
├── terminal/
│   ├── colors.ts           # Update PALETTE_16, DEFAULT_FOREGROUND/BACKGROUND
│   └── renderer.ts         # Update cursor color in addCursorStyles()
└── styles.css              # Update hardcoded colors
```

**File Descriptions**:

| File | Changes | Lines Affected |
|------|---------|----------------|
| colors.ts | PALETTE_16, DEFAULT_FOREGROUND | ~30 lines |
| styles.css | background-color, color values | ~5 locations |
| renderer.ts | Cursor color in CSS string | ~3 values |

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner
- Table-driven tests for color palette values
- Test color conversion functions

**Test Coverage Goals**:
- Color constants: 100% coverage (verify each value)
- Color functions: Existing coverage maintained

**Key Test Areas**:

1. **PALETTE_16 Validation**
   - Each of the 16 colors matches expected WezTerm value
   - Array length is exactly 16
   - All values are valid Rgb objects

2. **Default Colors**
   - DEFAULT_FOREGROUND equals { r: 64, g: 255, b: 64 }
   - DEFAULT_BACKGROUND equals { r: 0, g: 0, b: 0 }

3. **Color Functions**
   - indexToRgb(0-15) returns PALETTE_16 values
   - standardColorToRgb(0-7) returns correct subset
   - brightColorToRgb(0-7) returns indices 8-15

### Integration Testing

**Scenarios**:
1. SGR color codes (30-37, 90-97) produce correct colors
2. Background color codes (40-47, 100-107) produce correct colors
3. 256-color mode (38;5;n) works correctly
4. True Color mode (38;2;r;g;b) works correctly
5. Color reset (SGR 0) returns to WezTerm defaults

### Manual Testing Checklist

Based on specification test scenarios:

- [ ] Terminal displays with green foreground (#40ff40) on black background
- [ ] Cursor displays in dark green (#008000)
- [ ] ANSI standard colors (0-7) match WezTerm exactly
- [ ] ANSI bright colors (8-15) match WezTerm exactly
- [ ] IME composition view uses correct colors
- [ ] Color reset returns to default foreground/background
- [ ] Reverse video (SGR 7) swaps colors correctly
- [ ] Bold with standard colors uses bright variants
- [ ] 256-color palette indices 0-15 reflect WezTerm colors
- [ ] True Color (24-bit) still works correctly
- [ ] Markdown viewer has no visual regressions
- [ ] Image viewer has no visual regressions

## Dependencies

### External Dependencies

No new external dependencies required.

### Internal Dependencies

**Component Dependencies**:
- `PALETTE_256` depends on `PALETTE_16` (copies indices 0-15)
- `style-cache.ts` uses colors from `colors.ts`
- `renderer.ts` imports DEFAULT_FOREGROUND, DEFAULT_BACKGROUND from `colors.ts`

**Implementation Order**:
1. Phase 1 (colors.ts) - Foundation, no dependencies
2. Phase 2 (styles.css) - Independent, can parallel with Phase 3
3. Phase 3 (renderer.ts) - Independent, can parallel with Phase 2

All phases can be implemented in a single commit as they are tightly coupled.

## Risk Assessment

### Technical Risks

1. **CSS Variable Conflicts**
   - **Risk**: Markdown viewer uses CSS variables that might inherit changed colors
   - **Likelihood**: Low
   - **Impact**: Medium (visual regression in Markdown)
   - **Mitigation**: Markdown viewer CSS uses dedicated variables (--markdown-*, --md-*) independent of terminal colors

2. **256-Color Palette Integrity**
   - **Risk**: Changes to PALETTE_16 might affect 256-color rendering
   - **Likelihood**: Low (code structure is correct)
   - **Impact**: High (256-color apps would display wrong colors)
   - **Mitigation**: Verify generate256Palette() copies PALETTE_16 correctly; run 256-color tests

### Implementation Risks

1. **Incomplete Color Updates**
   - **Risk**: Missing some hardcoded color values
   - **Mitigation**: Search codebase for old color values (#1e1e1e, #d4d4d4, #c0c0c0) after changes

## Performance Considerations

- Zero runtime overhead (all values are compile-time constants)
- No change to rendering performance
- Object.freeze() ensures immutability with minimal overhead

## Security Considerations

- No security impact (hardcoded constants only)
- No external input or file reading in this phase

## Open Questions

### From Specification:
- None (all requirements are clearly defined)

### Implementation-Specific:
- None (straightforward constant replacement)

## Future Enhancements

Items deferred to later phases (from specification):

### Phase 2 (Future): Configuration File Support
- Read colors from ~/.config/emterm/colors.toml
- Fall back to hardcoded defaults if file not found
- Hot-reload colors when config file changes

### Phase 3 (Future): UI Color Picker
- Settings panel with color scheme selection
- Preset themes (WezTerm, Solarized, Dracula, etc.)
- Custom color scheme editor

## Success Metrics

### Functional Completeness
- [ ] All 16 ANSI colors match WezTerm configuration exactly
- [ ] Default foreground displays as #40ff40 (bright green)
- [ ] Default background displays as #000000 (black)
- [ ] Cursor displays as #008000 (dark green)

### Quality Metrics
- [ ] Type check passes (`bun run typecheck`)
- [ ] All tests pass (`bun test` and `cargo test`)
- [ ] No visual regressions

### User Experience
- [ ] Terminal visually consistent throughout application
- [ ] Colors match WezTerm reference

## References

- **Specification**: `doc/tasks/color-scheme/SPEC.md`
- **Requirements Document**: `doc/tasks/color-scheme/要件定義書.md`
- **WezTerm Configuration**: ~/.config/wezterm/wezterm.lua
- **ANSI SGR Codes**: https://en.wikipedia.org/wiki/ANSI_escape_code#SGR_parameters

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm color values match user's WezTerm configuration
   - Verify no additional files need updating

2. **Begin Implementation**
   - Start with Phase 1 (colors.ts)
   - Complete all phases in a single work session
   - Run verification after each phase

3. **Verification**
   - Run `bun run typecheck`
   - Run `bun test`
   - Run `cargo test`
   - Manual visual verification

4. **Commit**
   - Single commit for all color changes
   - Message: "feat: implement WezTerm color scheme as default"
