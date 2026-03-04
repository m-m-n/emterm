# Feature: Link Hover Underline

## Overview

Change URL and file path underline decoration from always-visible to hover-only. The underline appears only when the mouse cursor is over a detected link, and uses the actual character foreground color instead of the terminal's default foreground color.

## Objectives

- Replace always-on underline with hover-triggered underline for URLs and file paths
- Use each character's actual foreground color for the underline instead of the terminal default foreground
- Prevent stale underlines remaining after terminal content updates (e.g., Claude Code output refresh)

## User Stories

### US1: Hover to Reveal Link
As a developer, I want link underlines to appear only when I hover my mouse cursor over them, so that the terminal output remains clean and underlines don't persist after content updates.

**Acceptance Criteria:**
- [ ] No underlines are drawn when the mouse cursor is not over a link
- [ ] Underline appears when the mouse cursor moves over a detected URL or file path
- [ ] Underline disappears when the mouse cursor moves away from the link
- [ ] No stale underlines remain after PTY output updates the terminal content

### US2: Color-Matched Underline
As a developer, I want the link underline color to match the displayed text color, so that links with custom colors (e.g., ANSI-colored paths) have consistent visual styling.

**Acceptance Criteria:**
- [ ] Underline color matches the foreground color of the characters in the link
- [ ] Works correctly with ANSI-colored text (16-color, 256-color, RGB)
- [ ] Works correctly with default foreground color text

## Technical Requirements

### Functional Requirements

- **FR1:** Remove always-on underline rendering for detected URLs and file paths
  - `renderDetectionUnderlinesLogical()` must not draw underlines unconditionally
  - The detection cache mechanism may still be used for hover hit-testing

- **FR2:** Draw underline only for the link under the mouse cursor (hover state)
  - Track the current mouse position (row, col) in the renderer
  - On each render frame, check if the mouse is over a detected URL or file path
  - If hovering over a link: draw underline only for that specific link
  - If not hovering: draw no detection underlines
  - No Ctrl/Meta key required for hover underline display (Ctrl is only needed for click-to-open)

- **FR3:** Use the displayed character's foreground color for the underline
  - For each cell within the underlined range, determine the effective foreground color (considering ANSI attributes, bold, dim, reverse, etc.)
  - Use `getEffectiveForeground()` to resolve the actual color
  - If characters in the link have different colors, the underline segments should match each character's color

- **FR4:** Trigger re-render when hover state changes
  - When the mouse enters or leaves a link region, schedule a re-render to update the underline
  - Minimize unnecessary re-renders (only when hover state actually changes)

### Non-Functional Requirements

- **NFR1 - Performance:** Hover detection must not introduce perceptible lag. Mousemove event handling should be lightweight (detection only when position changes cells).
- **NFR2 - Compatibility:** Existing Ctrl+click behavior for opening URLs and file paths must remain unchanged.
- **NFR3 - Consistency:** The pointer cursor behavior on Ctrl+hover must continue to work as before.

## Implementation Approach

### Architecture

```
┌──────────────────────────────────────────────────┐
│              Frontend (TypeScript)                │
│                                                   │
│  terminal-app/index.ts                            │
│    ├── handleHover() ─── track mouse position     │
│    └── updateHoverCursor() ─── pointer cursor     │
│                                                   │
│  canvas-renderer.ts                               │
│    ├── setHoverPosition(row, col) ← from app      │
│    ├── renderDetectionUnderlinesLogical()          │
│    │     └── only draw if hover matches a link     │
│    └── drawClippedUnderline() ─── per-char color   │
│                                                   │
│  url-detector.ts (unchanged)                      │
│    ├── detectUrls()                               │
│    └── detectFilePaths()                          │
└──────────────────────────────────────────────────┘
```

### Data Flow

```
Mouse Move Event
    │
    ▼
terminal-app/index.ts :: handleHover()
    │
    ├─ Calculate (row, col) from mouse coordinates
    │
    ├─ renderer.setHoverPosition(row, col)
    │     │
    │     ├─ Compare with previous hover position
    │     │
    │     ├─ If cell changed: check if entering/leaving a link
    │     │     │
    │     │     └─ If hover state changed: scheduleRender()
    │     │
    │     └─ Store current hover (row, col)
    │
    ▼
canvas-renderer.ts :: renderDetectionUnderlinesLogical()
    │
    ├─ Get logical line for hover row
    │
    ├─ Detect URLs and file paths (using existing cache)
    │
    ├─ Find which link (if any) contains hover column
    │
    ├─ If hovering a link:
    │     │
    │     ├─ For each cell in link range:
    │     │     └─ Get cell's effective foreground color
    │     │
    │     └─ Draw underline segments with per-cell colors
    │
    └─ If not hovering: skip (no underline drawn)
```

### Key Changes

#### 1. `canvas-renderer.ts` - Add hover position tracking

```typescript
// New state
private hoverRow: number = -1;
private hoverCol: number = -1;

// New method on ITerminalRenderer (or direct on CanvasRenderer)
setHoverPosition(row: number, col: number): void {
  if (row === this.hoverRow && col === this.hoverCol) return;
  const oldRow = this.hoverRow;
  this.hoverRow = row;
  this.hoverCol = col;
  // Schedule re-render if hover state changed
  if (this.pendingState) {
    this.scheduleRender(this.pendingState);
  }
}
```

#### 2. `canvas-renderer.ts` - Modify `renderDetectionUnderlinesLogical()`

Current: draws underlines for ALL detected URLs and file paths on every row.

New: only draws underline for the link under the hover position.

```typescript
private renderDetectionUnderlinesLogical(rowIndex: number): void {
  // ... existing detection/cache logic ...

  // Only draw if this row is the hover row (or part of a multi-row hover link)
  // Find the link under hover position
  const hoverLogicalCol = physicalToLogicalCol(this.hoverRow, this.hoverCol, logical);

  let hoveredMatch: { startCol: number; endCol: number } | null = null;
  for (const match of cached.urls) {
    if (hoverLogicalCol >= match.startCol && hoverLogicalCol < match.endCol) {
      hoveredMatch = match;
      break;
    }
  }
  if (!hoveredMatch) {
    for (const match of cached.fps) {
      if (hoverLogicalCol >= match.startCol && hoverLogicalCol < match.endCol) {
        hoveredMatch = match;
        break;
      }
    }
  }

  if (!hoveredMatch) return;

  // Draw underline with per-cell foreground color
  this.drawClippedUnderlineWithCellColors(
    hoveredMatch.startCol, hoveredMatch.endCol,
    rowIndex, logical
  );
}
```

#### 3. `canvas-renderer.ts` - Per-cell color underline

```typescript
private drawClippedUnderlineWithCellColors(
  matchStart: number, matchEnd: number,
  rowIndex: number, logical: LogicalLine,
): void {
  const rowStartLogical = (rowIndex - logical.startRow) * logical.cols;
  const rowEndLogical = rowStartLogical + logical.cols;
  const clippedStart = Math.max(matchStart, rowStartLogical);
  const clippedEnd = Math.min(matchEnd, rowEndLogical);
  if (clippedStart >= clippedEnd) return;

  const y = Math.floor(rowIndex * this.charHeight);
  const line = /* get line accessor for rowIndex */;

  // Draw underline per-cell to match character colors
  for (let logCol = clippedStart; logCol < clippedEnd; logCol++) {
    const physCol = logCol - rowStartLogical;
    const cell = line.getCell(physCol);
    const fg = getEffectiveForeground(cell.attrs, this.currentForeground, this.currentBackground, this.currentPalette256, this.boldBrightensAnsiColors);
    const x = physCol * this.charWidth;
    this.drawUnderline(x, y, this.charWidth, fg);
  }
}
```

#### 4. `terminal-app/index.ts` - Pass hover position to renderer

```typescript
private handleHover(e: MouseEvent): void {
  this.lastMouseEvent = e;
  this.updateHoverCursor();

  // Pass hover position to renderer for underline drawing
  if (this.renderer && this.terminalRoot) {
    const rect = this.terminalRoot.getBoundingClientRect();
    const row = Math.floor((e.clientY - rect.top) / this.charSize.height);
    const col = Math.floor((e.clientX - rect.left) / this.charSize.width);
    (this.renderer as any).setHoverPosition(row, col);
  }
}
```

#### 5. Clear hover on mouse leave

```typescript
// In terminal-app setup, add mouseleave handler
this.terminalRoot.addEventListener("mouseleave", () => {
  (this.renderer as any).setHoverPosition(-1, -1);
});
```

### File Structure

```
src/
├── terminal/
│   ├── canvas-renderer.ts     # Modified: hover state, per-cell color underline
│   └── renderer-interface.ts  # Modified: add setHoverPosition() to interface
├── terminal-app/
│   └── index.ts               # Modified: pass hover position to renderer
```

### Dependencies

**Internal Dependencies:**
- `url-detector.ts`: Unchanged (detection logic stays the same)
- `canvas-renderer.ts`: Core changes for hover-based underline rendering
- `terminal-app/index.ts`: Bridge between mouse events and renderer
- `attributes.ts`: `getEffectiveForeground()` for per-cell color resolution

**External Dependencies:**
- None (no new dependencies)

## Test Scenarios

### Unit Tests

- [ ] `setHoverPosition()` triggers re-render only when cell position changes
- [ ] `setHoverPosition(-1, -1)` clears hover state
- [ ] Underline is not drawn when hover position is outside any link
- [ ] Underline is drawn only for the link containing the hover position
- [ ] Multiple links on the same row: only the hovered one is underlined
- [ ] Per-cell color: underline color matches each cell's foreground color
- [ ] Soft-wrapped links: hover underline works across physical rows

### Integration Tests

- [ ] Ctrl+click still opens URLs (behavior unchanged)
- [ ] Ctrl+click still opens file paths (behavior unchanged)
- [ ] Pointer cursor still appears on Ctrl+hover over links

### Edge Cases

- [ ] Mouse moves rapidly across multiple links: no stale underlines
- [ ] Terminal content updates while hovering: underline updates correctly
- [ ] Hovering over a link that spans a soft-wrap boundary
- [ ] Hovering at the exact boundary between two adjacent links
- [ ] Mouse leaves the terminal area: underline disappears

## Error Handling

| Error | Condition | Handling |
|-------|-----------|---------|
| Invalid hover position | Row/col out of bounds | Treat as no hover (no underline) |
| Missing line data | Line accessor returns null | Skip underline for that row |

## Success Criteria

- [ ] No underlines appear without hovering
- [ ] Hovering over a URL shows underline with character-matching colors
- [ ] Hovering over a file path shows underline with character-matching colors
- [ ] Moving mouse away removes the underline
- [ ] Terminal content refresh clears stale underlines
- [ ] Ctrl+click to open links works as before
- [ ] No perceptible performance impact on mousemove handling
- [ ] All existing URL/file path detection tests continue to pass

## References

- Existing URL detection: `src/terminal/url-detector.ts`
- Canvas renderer: `src/terminal/canvas-renderer.ts`
- File path click spec: `doc/tasks/file-path-click/SPEC.md` (FR2 references "underline decoration same as URLs")
