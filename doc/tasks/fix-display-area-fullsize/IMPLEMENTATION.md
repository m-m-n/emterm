# Implementation Plan: Full-Size Terminal Display Area

## Overview
Remove all padding from the terminal container to maximize the display area, allowing the terminal to utilize the full window height from top to bottom. This eliminates unused space at the bottom of the window and aligns behavior with standard terminal emulators.

## Objectives
- Eliminate unused space at the bottom of the terminal window
- Maximize the number of displayable rows within a given window size
- Align with standard terminal emulator behavior (no padding)
- Maintain compatibility with existing features (Markdown, images, mouse tracking)
- Ensure all existing tests continue to pass

## Prerequisites

### Development Environment
- Bun package manager
- TypeScript compiler
- Modern browser with ResizeObserver API support

### Dependencies
- No new external dependencies required
- All changes use existing APIs and libraries

### Knowledge Requirements
- Basic CSS layout concepts (padding, box-sizing)
- Understanding of terminal size calculation (cols, rows)
- ResizeObserver API behavior
- eMterm codebase structure

## Architecture Overview

### Technology Stack
- **Frontend**: Vanilla TypeScript
- **Styling**: CSS
- **Build Tool**: Bun
- **Testing**: Bun test

### Design Approach
This is a straightforward bug fix that removes unnecessary padding from the terminal container and updates size calculation logic to match. The approach follows the principle of minimal change with maximum verification.

**Current Behavior:**
- Terminal container has 8px padding on all sides
- Size calculation subtracts 16px (8px × 2) from client dimensions
- Result: Several rows of unused space at the bottom of the window

**Target Behavior:**
- Terminal container has 0px padding
- Size calculation uses full client dimensions
- Result: Terminal fills window from top to bottom

### Component Interaction
1. CSS defines terminal container layout (padding value)
2. JavaScript measures container client dimensions
3. Size calculation function computes cols/rows based on character metrics
4. ResizeObserver monitors container size changes and triggers recalculation
5. PTY backend receives updated terminal dimensions

No changes to component interaction pattern - only the values being calculated.

## Implementation Phases

### Phase 1: Remove Padding and Update Size Calculations

**Goal**: Eliminate bottom gap by removing padding from terminal container and updating all size calculation logic to match.

**Files to Modify**:
- `src/styles.css`:
  - Change `#terminal` padding from `8px` to `0`
  - Add CSS custom properties for font metrics (single source of truth):
    - `--terminal-font-size: 14px`
    - `--terminal-line-height: 16px` (font-size + 2px for integer calculation)
  - Update `font-size` and `line-height` to use CSS variables
- `src/main.ts`:
  - Update initial size calculation to remove hardcoded padding offset (`- 16`)
  - Update `measureCharacterSize()` call to pass container element instead of font parameters
  - Remove inline padding style assignments (CSS is single source of truth)
- `src/pty/size.ts`:
  - **Update `measureCharacterSize()` to read from CSS via `getComputedStyle()`**
    - Change API: accept `container: HTMLElement` instead of `fontFamily, fontSize`
    - Read `fontSize`, `lineHeight`, `fontFamily` from computed styles
    - Return `height` as `lineHeight` (integer value from CSS)
  - Simplify `calculateTerminalSize()` to remove dynamic padding calculation
- `src/terminal/renderer.ts`:
  - Update `measureCharacterSize()` method to read `lineHeight` from container's computed style
  - Replace hardcoded `"1.2"` with value from `getComputedStyle()`
- `src/image/layer.ts`:
  - Update `ImageLayer` constructor to dynamically retrieve container padding instead of using hardcoded values
  - This ensures image positioning adapts correctly when container padding changes

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| CSS `#terminal` selector | Define terminal container layout | Padding is 8px | Padding is 0px |
| `initTerminal()` | Calculate initial terminal size | Uses `clientWidth - 16` formula | Uses `clientWidth` directly |
| `calculateTerminalSize()` | Compute cols/rows from container dimensions | Dynamically calculates padding offset | Returns dimensions based on full client area |
| `observeContainerResize()` | Detect container size changes | Calls `calculateTerminalSize()` | Continues to work correctly with new calculation |

**Processing Flow**:
```
Application Startup
  ├─ Load CSS (padding: 0 applied to #terminal)
  ├─ Measure character size (width, height)
  ├─ Calculate initial terminal size
  │   ├─ Get container.clientWidth and container.clientHeight
  │   ├─ Divide by character dimensions
  │   └─ Return (cols, rows)
  ├─ Initialize terminal state with (cols, rows)
  ├─ Spawn PTY with (cols, rows)
  └─ Attach ResizeObserver
      └─ On container resize
          ├─ Recalculate (cols, rows) using calculateTerminalSize()
          ├─ Notify PTY of new size
          └─ Update terminal state and renderer
```

**Implementation Steps**:

1. **Update CSS Padding**
   - Modify `src/styles.css`
   - Change `#terminal` selector's `padding` property from `8px` to `0`
   - Consideration: This change affects terminal container layout globally

2. **Update Initial Size Calculation in main.ts**
   - Modify `src/main.ts` in the `initTerminal()` function
   - Remove hardcoded padding offset from calculation:
     - Before: `cols: Math.floor((terminal.clientWidth - 16) / charSize.width)`
     - After: `cols: Math.floor(terminal.clientWidth / charSize.width)`
     - Before: `rows: Math.floor((terminal.clientHeight - 16) / charSize.height)`
     - After: `rows: Math.floor(terminal.clientHeight / charSize.height)`

3. **Remove Redundant Inline Padding Styles in main.ts**
   - Modify `src/main.ts` in `initNewTerminal()` function
   - Remove: `container.style.padding = "8px"` (line ~135)
   - Modify `src/main.ts` in `initLegacyTerminal()` function
   - Remove: `container.style.padding = "8px"` (line ~153)
   - Rationale: CSS already defines `#terminal` padding; inline styles are redundant and override CSS changes
   - Benefit: Single source of truth (CSS only), easier to maintain, no CSS/JS inconsistency risk

4. **Simplify Size Calculation Utility (Optional)**
   - Modify `src/pty/size.ts` in `calculateTerminalSize()` function
   - Option 1 (Recommended): Remove dynamic padding calculation since padding is guaranteed to be 0
     - Simplify to: `cols = Math.max(1, Math.floor(clientWidth / charWidth))`
     - Simplify to: `rows = Math.max(1, Math.floor(clientHeight / charHeight))`
   - Option 2 (Conservative): Keep existing code unchanged (padding calculation will compute 0)
   - Rationale: Option 1 improves code clarity, Option 2 maintains robustness if padding is reintroduced

5. **Update Image Layer Padding Handling**
   - Modify `src/image/layer.ts` in `ImageLayer` constructor
   - Replace hardcoded padding values with dynamic computation from container:
     ```typescript
     const computedStyle = getComputedStyle(container);
     this.paddingX = parseFloat(computedStyle.paddingLeft) || 0;
     this.paddingY = parseFloat(computedStyle.paddingTop) || 0;
     ```
   - Rationale: Ensures image positioning automatically adapts to CSS changes, eliminating hardcoded value inconsistencies
   - Impact: Images will correctly position at edges when container padding is 0

6. **Update Test Fixtures**
   - Run `bun test` after code changes
   - Identify failing tests related to terminal size calculations
   - Update expected values in test fixtures to reflect padding=0:
     - Example: For 800x600px container with 8x16px characters:
       - Old expectation (with 16px padding offset): `cols: 98, rows: 35`
       - New expectation (without padding): `cols: 100, rows: 37`
   - Verify all tests pass after updating expectations
   - Rationale: Padding change affects calculated terminal dimensions; tests need to match new behavior

7. **Update TerminalRenderer Padding Offset Recalculation**
   - Modify `src/terminal/renderer.ts` in the `resize()` method
   - Add padding offset recalculation when renderer is resized:
     ```typescript
     // Recalculate padding offset in case CSS changed
     const computedStyle = window.getComputedStyle(this.container);
     this.paddingOffset = parseFloat(computedStyle.paddingLeft) || 0;
     ```
   - Location: Insert after line 776 (`this.rows = rows;`)
   - Rationale: The renderer caches `paddingOffset` at construction time (line 113); if CSS padding changes afterward, the cached value becomes stale, causing cursor and Markdown block positioning errors
   - Impact: Ensures cursor positioning (lines 737-738) and Markdown block positioning (lines 911, 936) use current padding values

**Dependencies**:
- Requires: None (self-contained changes)
- Blocks: None (no dependent features)

**Testing Approach**:

*Unit Tests*:
- Test `calculateTerminalSize()` returns correct dimensions with zero padding
- Test `calculateTerminalSize()` handles minimum size (1x1) correctly
- Test `calculateTerminalSize()` handles fractional pixels (floor behavior)
- Test `measureCharacterSize()` continues to work correctly (no changes needed)

*Integration Tests*:
- Verify initial terminal spawn uses full window height
- Verify window resize updates terminal size without creating gaps
- Verify Markdown rendering works correctly with padding removed
- Verify image display works correctly with padding removed

*Manual Testing*:
- [ ] Launch eMterm and visually inspect for bottom gap
- [ ] Verify terminal content fills window from top to bottom
- [ ] Resize window smaller and verify no gap appears
- [ ] Resize window larger and verify terminal fills new space
- [ ] Run sample commands (ls, cat, etc.) and verify display is correct
- [ ] Display Markdown content via OSC sequence and verify positioning
- [ ] Display image via Kitty protocol and verify positioning
- [ ] Test on different window sizes (small, medium, large, 4K)

*Regression Testing*:
- [ ] Run full test suite: `bun test`
- [ ] Verify all existing tests pass
- [ ] Verify no new warnings or errors in console

**Acceptance Criteria**:
- [ ] CSS `#terminal` padding is set to `0`
- [ ] CSS custom properties `--terminal-font-size` and `--terminal-line-height` are defined
- [ ] CSS `line-height` uses integer pixel value (`16px`) instead of ratio (`1.2`)
- [ ] Initial size calculation in `main.ts` uses full client dimensions (no `-16` offset)
- [ ] Inline padding styles in `main.ts` are removed (CSS is single source of truth)
- [ ] `measureCharacterSize()` in `size.ts` reads from CSS via `getComputedStyle()`
- [ ] `measureCharacterSize()` in `renderer.ts` reads `lineHeight` from container's computed style
- [ ] Visual inspection confirms no bottom gap in terminal window
- [ ] Terminal rows match calculated maximum based on window height (integer division)
- [ ] Window resize maintains full-size display without gaps
- [ ] All existing unit tests pass (updated for new API)
- [ ] All existing integration tests pass
- [ ] Markdown display feature works correctly
- [ ] Image display feature works correctly
- [ ] No performance degradation (resize < 100ms)

**Estimated Effort**: 小 (1-2 hours)
- Code changes: 30 minutes
- Testing: 1 hour
- Code review and documentation: 30 minutes

**Risks and Mitigation**:
- **Risk**: Text at screen edges may be less readable
  - **Mitigation**: Standard terminal behavior; users expect this; can be addressed in future if feedback received
- **Risk**: Markdown or image positioning may be affected
  - **Mitigation**: Test both features thoroughly; their positioning is relative to cursor, not container padding
- **Risk**: Existing tests may fail due to hardcoded size expectations
  - **Mitigation**: Review and update test fixtures if needed

## Complete File Structure

Files modified in this implementation:

```
src/
├── styles.css              # CSS padding removal (line 22: padding: 8px → padding: 0)
├── main.ts                 # Initial size calculation update (lines 41-42: remove - 16 offset)
│                          # Inline padding style update (lines 135, 153: padding: "8px" → padding: "0")
├── terminal/
│   └── renderer.ts        # Padding offset recalculation in resize() method
├── image/
│   └── layer.ts           # Dynamic padding retrieval in ImageLayer constructor
└── pty/
    └── size.ts             # (Optional) Simplify size calculation logic
```

**File Descriptions**:
- `src/styles.css`: Defines global terminal container styles including padding
- `src/main.ts`: Application entry point; initializes terminal with initial size calculation
- `src/terminal/renderer.ts`: Terminal DOM renderer; handles cursor and content positioning with padding awareness
- `src/image/layer.ts`: Image overlay layer management; handles positioning relative to terminal container
- `src/pty/size.ts`: Terminal size calculation utilities used during initialization and resize

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner
- Test size calculation functions in isolation
- Mock container dimensions using test fixtures

**Test Coverage Goals**:
- Size calculation: 100% coverage (critical functionality)
- Modified functions: 100% coverage
- Overall project: Maintain existing coverage levels

**Key Test Areas**:

1. **Size Calculation (`src/pty/size.ts`)**
   - Test `calculateTerminalSize()` with padding=0
     - Container 800x600px, char 8x16px → expect 100x37
   - Test minimum size enforcement
     - Container 10x10px, char 8x16px → expect 1x1
   - Test fractional pixel handling
     - Container 805x605px, char 8x16px → expect 100x37 (floor behavior)
   - Test with large dimensions
     - Container 3840x2160px (4K), char 8x16px → expect 480x135

2. **Character Measurement (`src/pty/size.ts`)**
   - Test `measureCharacterSize()` returns positive values
   - Test fallback behavior when canvas unavailable

### Integration Testing

**Scenarios**:

1. **Initial Terminal Size**
   - Given: Window with specific dimensions
   - When: Terminal initializes
   - Then: Terminal size matches calculated cols/rows without bottom gap

2. **Window Resize**
   - Given: Running terminal
   - When: Window resized to larger size
   - Then: Terminal recalculates and fills new space without gap
   - When: Window resized to smaller size
   - Then: Terminal recalculates and fits new space without gap

3. **Markdown Display Compatibility**
   - Given: Terminal with padding=0
   - When: Markdown content displayed via OSC sequence
   - Then: Markdown renders correctly at cursor position

4. **Image Display Compatibility**
   - Given: Terminal with padding=0
   - When: Image displayed via Kitty protocol
   - Then: Image renders correctly at cursor position

### Manual Testing Checklist

Based on spec test scenarios:

**Normal Operations:**
- [ ] Launch eMterm and verify no bottom gap exists
- [ ] Verify terminal content fills window edge-to-edge
- [ ] Run `ls -la` and verify all output is visible
- [ ] Run `cat large_file.txt` and verify text displays correctly

**Window Resize:**
- [ ] Resize window to smaller size and verify no gap appears
- [ ] Resize window to larger size and verify terminal fills space
- [ ] Rapidly resize window multiple times and verify stability

**Feature Compatibility:**
- [ ] Display Markdown document and verify correct rendering
- [ ] Display image and verify correct positioning
- [ ] Test mouse tracking (if applicable) and verify coordinates are correct

**Edge Cases:**
- [ ] Minimize window to very small size (e.g., 200x200px)
- [ ] Maximize window to full screen (e.g., 4K display)
- [ ] Verify fractional pixels don't cause rendering issues

**Performance:**
- [ ] Resize window rapidly and verify smooth response (< 100ms)
- [ ] Verify no console errors or warnings during resize

## Dependencies

### External Dependencies

No new external dependencies required.

### Internal Dependencies

**Modified Files and Their Dependents:**

1. `src/styles.css`
   - Affects: All terminal rendering (global stylesheet)
   - Dependents: Entire application (styles loaded at startup)

2. `src/main.ts`
   - Affects: Terminal initialization
   - Dependents: None (entry point)

3. `src/pty/size.ts`
   - Affects: Size calculation during init and resize
   - Dependents: `src/main.ts` (imports `calculateTerminalSize`, `observeContainerResize`)

**Implementation Order:**
1. Update CSS (affects layout)
2. Update main.ts initial calculation (matches new layout)
3. Update main.ts inline styles (consistency with CSS)
4. (Optional) Simplify size.ts calculation (code clarity)

All changes can be made in a single commit as they are tightly coupled.

## Risk Assessment

### Technical Risks

1. **Layout Shifts During Resize**
   - **Risk**: Rapid resize may cause visual artifacts
   - **Likelihood**: Low
   - **Impact**: Low (temporary visual glitch)
   - **Mitigation**: ResizeObserver already implements debouncing; existing code handles this

2. **Test Failures Due to Size Changes**
   - **Risk**: Existing tests may have hardcoded size expectations
   - **Likelihood**: Medium
   - **Impact**: Medium (requires test updates)
   - **Mitigation**: Run full test suite early; update test fixtures as needed

3. **Markdown/Image Positioning Issues**
   - **Risk**: Content may render incorrectly without padding
   - **Likelihood**: Low
   - **Impact**: Medium (affects rich content features)
   - **Mitigation**: Thoroughly test both features; positioning is cursor-relative

### Implementation Risks

1. **Incomplete Padding Removal**
   - **Risk**: Padding removed in CSS but not in JavaScript inline styles
   - **Mitigation**: Verify both `initNewTerminal()` and `initLegacyTerminal()` update padding

2. **Regression in Edge Cases**
   - **Risk**: Very small or very large windows may break
   - **Mitigation**: Test minimum size (1x1) and large displays (4K)

## Performance Considerations

### Expected Performance Impact
- **Positive**: Slightly faster size calculation (fewer operations)
- **Neutral**: No change to rendering performance
- **Neutral**: No change to resize performance (already optimized)

### Performance Goals
- Window resize recalculation: < 100ms (existing requirement)
- Initial terminal load: < 500ms (existing requirement)
- ResizeObserver callback: < 50ms (existing requirement)

### Optimization Notes
- Removing dynamic padding calculation reduces computational overhead (minimal impact)
- No additional DOM operations introduced
- No new event listeners or observers

## Security Considerations

### Input Validation
- `Math.max(1, ...)` in `calculateTerminalSize()` ensures minimum 1x1 size (prevents zero/negative)
- No new user input or data sources introduced

### Resource Protection
- No change to resource limits
- Large terminal sizes (e.g., 4K displays with 200+ rows) already handled by existing code

### XSS Prevention
- No changes to content sanitization or rendering
- Existing protections for Markdown and image content remain unchanged

## Open Questions

### From Specification:
None. All requirements have been clarified with the user.

### Implementation-Specific:
None. The implementation approach is straightforward.

### To Clarify with User:
None at this time.

## Future Enhancements

Items deferred to later phases or releases:

### Not in Current Spec:
- Configurable padding via user preferences (if user feedback requests it)
- Adaptive padding based on display characteristics
- Per-edge padding control (top/bottom/left/right)

These are not planned and should only be considered if user feedback indicates a need.

## Success Metrics

### Functional Completeness
- [ ] All functional requirements (FR1-FR6) implemented
- [ ] FR6: Integer pixel line-height with CSS variables as single source of truth
- [ ] All acceptance criteria met
- [ ] All test scenarios pass

### Quality Metrics
- [ ] All existing tests pass (100%)
- [ ] No new console errors or warnings
- [ ] Code follows project style conventions (TypeScript + CSS)

### Performance Metrics
- [ ] Window resize recalculation < 100ms
- [ ] Initial terminal load < 500ms
- [ ] ResizeObserver callback < 50ms

### User Experience
- [ ] Terminal fills window from top to bottom
- [ ] No visible gap at bottom of window
- [ ] Text remains readable edge-to-edge
- [ ] Markdown and image features work correctly

## References

- **Specification**: `doc/tasks/fix-display-area-fullsize/SPEC.md`
- **Requirements**: `doc/tasks/fix-display-area-fullsize/要件定義書.md`
- **Issue Screenshot**: `/dev/shm/work/output/image.png`
- **Current Implementation**:
  - `src/styles.css` (CSS padding definition)
  - `src/main.ts` (initial size calculation)
  - `src/pty/size.ts` (size calculation utilities)
- **ResizeObserver API**: https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver
- **CLAUDE.md**: Project structure and development workflow

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Review the implementation approach
   - Confirm acceptance criteria
   - Address any questions

2. **Environment Setup**
   - Ensure development environment is ready
   - Verify `bun install` completed successfully
   - Verify `bun tauri dev` runs without errors

3. **Begin Implementation**
   - Follow implementation steps in Phase 1
   - Make changes to CSS, main.ts, and optionally size.ts
   - Test incrementally (run tests after each change)

4. **Verification**
   - Run `/sdd.3-verify-plan` to validate plan integrity
   - Run full test suite: `bun test`
   - Perform manual testing checklist
   - Verify VERIFICATION.md criteria

5. **Commit and Push**
   - Create descriptive commit message
   - Reference issue/task in commit
   - Push to branch for review
