# Feature: Full-Size Terminal Display Area

## Overview

The eMterm terminal emulator currently has unused space at the bottom of the window, reducing the effective number of displayable rows. This feature removes all padding from the terminal container to maximize the display area, allowing the terminal to utilize the full window height from top to bottom, consistent with standard terminal emulator behavior.

## Objectives

- Eliminate unused space at the bottom of the terminal window
- Maximize the number of displayable rows within a given window size
- Align with standard terminal emulator behavior (no padding)
- Maintain compatibility with existing features (Markdown, images, mouse tracking)

## User Stories

### US1: View Maximum Terminal Content
As a terminal user, I want the terminal to use the full window height, so that I can see more lines of output without scrolling.

**Acceptance Criteria:**
- [ ] Terminal content fills the window from top to bottom
- [ ] No unused black space exists at the bottom of the window
- [ ] The number of rows matches the calculated maximum based on window size

### US2: Resize Window Without Creating Gaps
As a terminal user, I want the terminal to remain full-size when I resize the window, so that no unused space appears.

**Acceptance Criteria:**
- [ ] Terminal automatically recalculates size on window resize
- [ ] No gaps appear at the bottom after resizing
- [ ] Terminal content fills the new window size completely

## Technical Requirements

### Functional Requirements
- **FR1:** Remove all padding from the terminal container element (`#terminal`)
- **FR2:** Update terminal size calculation to use full client dimensions without padding offset
- **FR3:** Ensure resize observer correctly recalculates terminal dimensions
- **FR4:** Maintain cursor positioning accuracy without padding offset
- **FR5:** Preserve existing functionality (Markdown, images, mouse events)
- **FR6:** Use integer pixel values for line-height to eliminate floating-point calculation errors

### Line Height Specification

To prevent floating-point calculation errors in terminal size calculations, all dimensions must use integer pixel values.

**Formula:**
```
line-height = font-size + 2px
```

**Rationale:**
- CSS `line-height` with decimal values (e.g., `1.2`) causes floating-point precision issues
- Example: `14px × 1.2 = 16.8px` leads to cumulative rounding errors in row calculations
- Integer pixels ensure deterministic row count: `rows = floor(containerHeight / lineHeight)`

**Line Height Structure:**
```
┌──────────────────────────┐
│  half-leading (1px)      │  ← (lineHeight - fontSize) ÷ 2
├──────────────────────────┤
│                          │
│   Character (14px)       │
│                          │
├──────────────────────────┤
│  half-leading (1px)      │  ← (lineHeight - fontSize) ÷ 2
└──────────────────────────┘
        Total: 16px
```

**Calculation Example:**
```
Container height: 980px
Font size: 14px
Line height: 16px (14 + 2)

Rows = floor(980 / 16) = 61
Used height = 61 × 16 = 976px
Remainder = 980 - 976 = 4px (< 1 row, acceptable)
```

### Single Source of Truth Architecture

To prevent drift between CSS and JavaScript calculations, use CSS custom properties as the single source of truth.

**Design Principle:**
- Define font-size and line-height as CSS variables in `#terminal`
- All JavaScript code retrieves values via `getComputedStyle()`
- No hardcoded values in JavaScript for font metrics

**CSS Variables:**
```css
#terminal {
  --terminal-font-size: 14px;
  --terminal-line-height: 16px;  /* font-size + 2px */
  font-size: var(--terminal-font-size);
  line-height: var(--terminal-line-height);
}
```

**JavaScript Access Pattern:**
```typescript
function getTerminalMetrics(container: HTMLElement): { fontSize: number; lineHeight: number } {
  const style = getComputedStyle(container);
  return {
    fontSize: parseFloat(style.fontSize),
    lineHeight: parseFloat(style.lineHeight),
  };
}
```

**Benefits:**
- Single definition point for all font metrics
- CSS changes automatically propagate to JavaScript calculations
- No risk of drift between CSS rendering and JS calculations
- Easier to maintain and modify

### Non-Functional Requirements
- **NFR1 - Performance:** Window resize recalculation must complete within 100ms
- **NFR2 - Compatibility:** All existing tests must continue to pass
- **NFR3 - Usability:** Text must remain readable with edge-to-edge display
- **NFR4 - Maintainability:** Changes must be minimal and well-documented

## Implementation Approach

### Architecture

**Current Architecture (with padding):**
```
┌─────────────────────────────────────┐
│ Window                              │
│ ┌─────────────────────────────────┐ │
│ │ #terminal (padding: 8px)        │ │
│ │ ┌─────────────────────────────┐ │ │
│ │ │ Terminal Content            │ │ │
│ │ │                             │ │ │
│ │ └─────────────────────────────┘ │ │
│ │ [8px padding]                   │ │
│ └─────────────────────────────────┘ │
│ [Unused space - several rows]       │
└─────────────────────────────────────┘
```

**Target Architecture (no padding):**
```
┌─────────────────────────────────────┐
│ Window = #terminal                  │
│ ┌─────────────────────────────────┐ │
│ │ Terminal Content (edge-to-edge) │ │
│ │                                 │ │
│ │                                 │ │
│ │                                 │ │
│ └─────────────────────────────────┘ │
└─────────────────────────────────────┘
```

### Component Changes

#### Component 1: CSS Styles (`src/styles.css`)

**Current Implementation:**
```css
#terminal {
  width: 100%;
  height: 100%;
  padding: 8px;
  font-size: 14px;
  line-height: 1.2;
}
```

**Target Implementation:**
```css
#terminal {
  /* CSS Variables - Single Source of Truth */
  --terminal-font-size: 14px;
  --terminal-line-height: 16px;  /* font-size + 2px */

  width: 100%;
  height: 100%;
  padding: 0;
  font-size: var(--terminal-font-size);
  line-height: var(--terminal-line-height);
}
```

**Rationale:**
- Remove padding to allow terminal content to fill the entire window
- Use CSS custom properties as single source of truth for font metrics
- Use integer pixel value for line-height (16px = 14px font + 2px margin) to eliminate floating-point errors
- JavaScript code will read these values via `getComputedStyle()`

#### Component 2: Initial Size Calculation (`src/main.ts`)

**Current Implementation:**
```typescript
const initialSize = {
  cols: Math.floor((terminal.clientWidth - 16) / charSize.width),
  rows: Math.floor((terminal.clientHeight - 16) / charSize.height),
};
```

**Target Implementation:**
```typescript
const initialSize = {
  cols: Math.floor(terminal.clientWidth / charSize.width),
  rows: Math.floor(terminal.clientHeight / charSize.height),
};
```

**Rationale:** Since padding is now 0, subtract no offset from client dimensions.

#### Component 3: Size Calculation Utility (`src/pty/size.ts`)

**Current Implementation:**
```typescript
export function calculateTerminalSize(
  container: HTMLElement,
  charWidth: number,
  charHeight: number
): TerminalSize {
  const { clientWidth, clientHeight } = container;

  // Account for padding/margins
  const style = getComputedStyle(container);
  const paddingX =
    parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
  const paddingY =
    parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);

  const availableWidth = clientWidth - paddingX;
  const availableHeight = clientHeight - paddingY;

  const cols = Math.max(1, Math.floor(availableWidth / charWidth));
  const rows = Math.max(1, Math.floor(availableHeight / charHeight));

  return { cols, rows };
}
```

**Target Implementation (Option 1 - Simplify):**
```typescript
export function calculateTerminalSize(
  container: HTMLElement,
  charWidth: number,
  charHeight: number
): TerminalSize {
  const { clientWidth, clientHeight } = container;

  // clientWidth/clientHeight already exclude padding
  // Since padding is 0, we can use dimensions directly
  const cols = Math.max(1, Math.floor(clientWidth / charWidth));
  const rows = Math.max(1, Math.floor(clientHeight / charHeight));

  return { cols, rows };
}
```

**Target Implementation (Option 2 - Keep dynamic calculation):**
Keep the existing implementation as-is. Since `clientWidth` and `clientHeight` already exclude padding, and we're computing `paddingX` and `paddingY` which will be 0, the calculation will naturally yield the full dimensions.

**Recommendation:** Use Option 1 for code clarity and simplification.

#### Component 4: Character Size Measurement (`src/pty/size.ts`)

**Current Implementation:**
```typescript
export function measureCharacterSize(
  fontFamily: string,
  fontSize: number
): CharacterSize {
  // ... canvas-based width measurement ...
  return {
    width: metrics.width,
    height: fontSize * 1.2,  // Floating-point result (e.g., 16.8)
  };
}
```

**Target Implementation:**
```typescript
export function measureCharacterSize(
  container: HTMLElement  // Changed: pass container instead of fontFamily/fontSize
): CharacterSize {
  const style = getComputedStyle(container);
  const fontSize = parseFloat(style.fontSize);
  const lineHeight = parseFloat(style.lineHeight);
  const fontFamily = style.fontFamily;

  // Measure character width using canvas
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return { width: fontSize * 0.6, height: lineHeight };
  }
  ctx.font = `${fontSize}px ${fontFamily}`;
  const metrics = ctx.measureText("M");

  return {
    width: metrics.width,
    height: lineHeight,  // Read from CSS (integer value)
  };
}
```

**Rationale:**
- Read font metrics from CSS via `getComputedStyle()` instead of hardcoded values
- CSS `--terminal-line-height: 16px` is the single source of truth
- Eliminates drift between CSS rendering and JavaScript calculations
- API change: pass `container` element instead of `fontFamily` and `fontSize`

#### Component 5: Terminal Renderer (`src/terminal/renderer.ts`)

**Current Implementation:**
```typescript
private measureCharacterSize(): void {
  const measureSpan = document.createElement("span");
  measureSpan.style.lineHeight = "1.2";  // Hardcoded value
  // ...
}
```

**Target Implementation:**
```typescript
private measureCharacterSize(): void {
  const style = getComputedStyle(this.container);
  const measureSpan = document.createElement("span");
  measureSpan.style.fontFamily = style.fontFamily;
  measureSpan.style.fontSize = style.fontSize;
  measureSpan.style.lineHeight = style.lineHeight;  // Read from CSS
  // ...
}
```

**Rationale:**
- Read line-height from CSS instead of hardcoding
- Ensures renderer uses the same values as PTY size calculation
- Prevents cursor positioning errors caused by metric mismatch

### Data Flow

```
User Opens Window
    ↓
Window Size Detection (clientWidth, clientHeight)
    ↓
Measure Character Size (charWidth, charHeight)
    ↓
Calculate Terminal Size
  cols = floor(clientWidth / charWidth)
  rows = floor(clientHeight / charHeight)
    ↓
Spawn PTY with (cols, rows)
    ↓
Render Terminal Content
    ↓
User Resizes Window
    ↓
ResizeObserver Triggered
    ↓
Recalculate Terminal Size
    ↓
Notify PTY of New Size
    ↓
Re-render Terminal Content
```

### File Structure

```
src/
├── styles.css              # CSS padding removal
├── main.ts                 # Initial size calculation update
└── pty/
    └── size.ts             # Size calculation utility update
```

### Dependencies

**Internal Dependencies:**
- `src/terminal/renderer.ts`: Renderer must handle edge-to-edge rendering
- `src/terminal/state.ts`: Terminal state must resize correctly
- `src/image/layer.ts`: Image overlay positioning may need verification
- `src/markdown/renderer.ts`: Markdown block positioning may need verification

**External Dependencies:**
- Tauri WebView API: Window size detection
- Browser ResizeObserver API: Resize detection

## Test Scenarios

### Unit Tests
- [ ] Test 1: `calculateTerminalSize()` returns correct dimensions with zero padding
  - Given: Container with `clientWidth=800px`, `clientHeight=600px`, `charWidth=8px`, `charHeight=16px`
  - Expected: `cols=100`, `rows=37` (floor(600/16))

- [ ] Test 2: `calculateTerminalSize()` handles minimum size
  - Given: Container with very small dimensions (e.g., 10x10px)
  - Expected: `cols=1`, `rows=1` (minimum guaranteed)

- [ ] Test 3: Size calculation handles fractional pixels
  - Given: Container with `clientWidth=805px`, `charWidth=8px`
  - Expected: `cols=100` (floor(805/8) = 100, not 101)

### Integration Tests
- [ ] Test 1: Initial terminal spawn uses full window height
  - Given: Window opened with specific size
  - Expected: Terminal rows = floor(windowHeight / charHeight)

- [ ] Test 2: Window resize updates terminal size correctly
  - Given: Terminal running with initial size
  - When: Window resized to new dimensions
  - Expected: Terminal rows/cols recalculated without gaps

- [ ] Test 3: Markdown blocks render correctly without padding
  - Given: Terminal with padding=0
  - When: Markdown content is displayed via OSC sequence
  - Expected: Markdown blocks render within terminal bounds

- [ ] Test 4: Image display works with edge-to-edge layout
  - Given: Terminal with padding=0
  - When: Image is displayed via Kitty protocol
  - Expected: Image renders correctly at cursor position

### E2E Tests
- [ ] Scenario 1: User opens terminal and sees full-height display
  1. Launch eMterm
  2. Verify terminal fills window from top to bottom
  3. Verify no black gap exists at bottom

- [ ] Scenario 2: User resizes window and terminal adjusts
  1. Launch eMterm
  2. Resize window to smaller size
  3. Verify terminal adjusts without bottom gap
  4. Resize window to larger size
  5. Verify terminal fills new space without gap

- [ ] Scenario 3: Terminal content remains readable edge-to-edge
  1. Launch eMterm
  2. Run command with long output (e.g., `ls -la`)
  3. Verify all content is readable
  4. Verify no text is cut off at edges

### Edge Cases
- [ ] Edge case 1: Window height not evenly divisible by char height
  - Expected: Unused pixels at bottom < charHeight (unavoidable, but minimal)

- [ ] Edge case 2: Extremely small window (e.g., 50x50px)
  - Expected: At least 1 row and 1 column displayed

- [ ] Edge case 3: Extremely large window (e.g., 4K display)
  - Expected: Terminal uses all available space without performance degradation

### Performance Tests
- [ ] Load test: Window resize performance
  - Scenario: Rapidly resize window 100 times
  - Acceptance: Each recalculation completes in < 100ms

- [ ] Stress test: Large terminal size
  - Scenario: 4K display with 200+ rows
  - Acceptance: Rendering remains smooth (60fps)

## Security Considerations

- **Input Validation:** Ensure `Math.max(1, ...)` prevents zero or negative row/col values
- **Resource Protection:** Large terminal sizes must not cause memory exhaustion
- **XSS Prevention:** No changes to content sanitization (existing protections maintained)

## Error Handling

### Error Codes

No new error codes are introduced. Existing error handling is sufficient:

| Scenario | Handling | User Impact |
|----------|----------|-------------|
| Container element not found | Log error, abort initialization | Terminal fails to start with error message |
| Invalid dimensions (0 or negative) | Clamp to minimum (1x1) | Terminal displays with minimal size |
| Resize calculation fails | Log error, keep previous size | Terminal continues with last known size |

### Error Flow

```
Calculate Terminal Size
    ↓
Dimensions Valid? ──No──> Clamp to 1x1 → Log Warning → Continue
    ↓ Yes
Return (cols, rows)
```

## Performance Optimization

### Performance Goals
- Window resize recalculation: < 100ms
- Initial terminal load: < 500ms
- ResizeObserver callback: < 50ms (to maintain 60fps during resize)

### Optimization Strategies
- **Remove unnecessary calculations:** Since padding is 0, skip dynamic padding computation
- **Debounce optimization:** ResizeObserver already filters duplicate events
- **DOM reflow minimization:** CSS change (padding removal) happens once at load time

### Caching Strategy
- Character size measurements are cached (already implemented in `main.ts`)
- No additional caching required for this feature

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Visual inspection confirms no bottom gap in various window sizes
- [ ] Performance meets specified goals (< 100ms resize)
- [ ] Existing features (Markdown, images) remain functional
- [ ] Code review is completed
- [ ] Documentation is updated

## Open Questions

None. All requirements have been clarified with the user.

## Implementation Phases

This is a simple bug fix that can be implemented in a single phase.

### Phase 1: Remove Padding and Update Calculations
**Goals:**
- Remove CSS padding
- Update size calculation logic
- Verify all existing tests pass

**Deliverables:**
- Modified `src/styles.css` with `padding: 0`
- Updated `src/main.ts` initial size calculation
- Simplified `src/pty/size.ts` calculation (optional, for clarity)
- All tests passing
- Visual verification of fix

**Estimated Effort:** 1-2 hours

## References

- Issue Screenshot: `/dev/shm/work/output/image.png`
- Current Implementation: `src/styles.css`, `src/main.ts`, `src/pty/size.ts`
- CLAUDE.md: Project structure and development workflow
- ResizeObserver API: https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver
