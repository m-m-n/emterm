# Feature: Canvas 2D Renderer

## Overview

Implement a Canvas 2D-based rendering engine for eMterm terminal emulator to improve performance during fast scrolling. The new renderer will coexist with the existing DOM renderer, allowing gradual migration through feature flags.

## Objectives

- Improve rendering performance during high-speed scrolling
- Maintain visual parity with the existing DOM renderer
- Enable seamless switching between renderers via environment variables
- Support all existing terminal features including text attributes, cursor, and selection

## User Stories

### US1: High-Speed Scrolling
As a developer, I want smooth scrolling when viewing large log outputs, so that I can quickly navigate through terminal history.

**Acceptance Criteria:**
- [ ] Scrolling feels responsive even with rapid input
- [ ] No visible frame drops during fast scroll
- [ ] Content renders correctly at all scroll positions

### US2: Text Attribute Rendering
As a user, I want all text attributes (colors, bold, underline, etc.) to display correctly, so that I can use terminal applications with rich formatting.

**Acceptance Criteria:**
- [ ] All SGR attributes render correctly (bold, italic, underline, etc.)
- [ ] 256-color and RGB colors display accurately
- [ ] Reverse video and dim attributes work as expected

### US3: Renderer Selection (Temporary)
As a developer, I want to switch between Canvas and DOM renderers during migration, so that I can compare performance and behavior.

**Acceptance Criteria:**
- [ ] Environment variable `EMTERM_RENDERER` controls renderer selection
- [ ] Application uses Canvas renderer when set to "canvas"
- [ ] Application defaults to DOM renderer when not set or set to "dom"
- [ ] Feature flag will be removed after migration is complete (Canvas becomes default)

## Technical Requirements

### Functional Requirements
- **FR1:** Render terminal text using Canvas 2D API fillText
- **FR2:** Support all CellAttributes (bold, italic, underline, strikethrough, blink, reverse, hidden, dim)
- **FR3:** Support foreground and background colors (16, 256, and RGB)
- **FR4:** Render cursor within Canvas with block, underline, and bar styles
- **FR5:** Implement cursor blink using JavaScript setInterval
- **FR6:** Render selection highlight overlay
- **FR7:** Support wide characters (fullwidth) using existing charWidth function
- **FR8:** Display scrollback buffer content
- **FR9:** Select renderer via EMTERM_RENDERER environment variable

### Non-Functional Requirements
- **NFR1 - Performance:** Render time should be lower than DOM renderer
- **NFR2 - Compatibility:** Maintain same public API as TerminalRenderer
- **NFR3 - Visual Parity:** Output should be visually identical to DOM renderer
- **NFR4 - Maintainability:** Share common types and utilities with DOM renderer

## Implementation Approach

### Architecture

**Renderer Abstraction:**
```
┌─────────────────────────────────────┐
│        ITerminalRenderer            │
│  (Common Interface)                 │
├─────────────────────────────────────┤
│  scheduleRender(state)              │
│  forceRender(state)                 │
│  resize(cols, rows)                 │
│  renderSelection(selection)         │
│  clearSelectionHighlight()          │
│  getCharWidth() / getCharHeight()   │
│  getFontFamily() / getFontSize()    │
└─────────────────────────────────────┘
           ▲              ▲
           │              │
┌──────────┴───┐  ┌───────┴──────────┐
│TerminalRenderer │  │CanvasRenderer     │
│ (DOM-based)      │  │ (Canvas 2D-based) │
└──────────────────┘  └──────────────────┘
```

**Component Structure:**
```
CanvasRenderer
├── TextRenderer      - Text drawing with attributes
├── CursorRenderer    - Cursor drawing and blink
├── SelectionRenderer - Selection highlight
└── FontMetrics       - Character measurement
```

### Data Flow

```
TerminalState → CanvasRenderer.scheduleRender()
                       │
                       ▼
              requestAnimationFrame
                       │
                       ▼
              render() - Process dirty rows
                       │
           ┌───────────┼───────────┐
           ▼           ▼           ▼
      renderLine   updateCursor  renderSelection
           │           │           │
           ▼           ▼           ▼
        Canvas 2D Context Operations
```

### Class Design

#### CanvasRenderer

```typescript
class CanvasRenderer {
  // Canvas elements
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;

  // Font metrics
  private fontFamily: string;
  private fontSize: number;
  private charWidth: number;
  private charHeight: number;

  // Grid dimensions
  private cols: number;
  private rows: number;

  // Render state
  private renderPending: boolean;
  private pendingState: TerminalState | null;

  // Cursor blink
  private blinkInterval: ReturnType<typeof setInterval> | null;
  private cursorVisible: boolean;

  // Performance
  private renderTimer: RenderTimer;

  // Public API (same as TerminalRenderer)
  constructor(container: HTMLElement, fontFamily: string, fontSize: number);
  scheduleRender(state: TerminalState): void;
  forceRender(state: TerminalState): void;
  resize(cols: number, rows: number): void;
  renderSelection(selection: SelectionRange): void;
  clearSelectionHighlight(): void;
  getCharWidth(): number;
  getCharHeight(): number;
  getFontFamily(): string;
  getFontSize(): number;
}
```

### Rendering Algorithm

#### Text Rendering

```typescript
private renderLine(rowIndex: number, line: Line): void {
  const y = rowIndex * this.charHeight;
  let x = 0;

  // Group consecutive cells with same attributes
  const spans = this.groupCellsIntoSpans(line);

  for (const span of spans) {
    const width = span.text.length * this.charWidth;

    // Draw background if not default
    const bg = getEffectiveBackground(span.attrs);
    if (bg !== null) {
      this.ctx.fillStyle = rgbToCanvasColor(bg);
      this.ctx.fillRect(x, y, width, this.charHeight);
    }

    // Set text style
    this.ctx.fillStyle = rgbToCanvasColor(getEffectiveForeground(span.attrs));
    this.ctx.font = this.buildFont(span.attrs);

    // Apply dim
    if (span.attrs.dim) {
      this.ctx.globalAlpha = 0.5;
    }

    // Draw text
    if (!span.attrs.hidden) {
      this.ctx.fillText(span.text, x, y + this.charHeight - baseline);
    }

    // Draw decorations
    if (span.attrs.underline) {
      this.drawUnderline(x, y, width);
    }
    if (span.attrs.strikethrough) {
      this.drawStrikethrough(x, y, width);
    }

    // Reset alpha
    this.ctx.globalAlpha = 1.0;

    x += width;
  }
}
```

#### Cursor Rendering

```typescript
private renderCursor(col: number, row: number, style: CursorStyle, visible: boolean): void {
  if (!visible || !this.cursorVisible) return;

  const x = col * this.charWidth;
  const y = row * this.charHeight;

  this.ctx.fillStyle = '#008000'; // Cursor color

  switch (style) {
    case 'block':
      this.ctx.fillRect(x, y, this.charWidth, this.charHeight);
      break;
    case 'underline':
      this.ctx.fillRect(x, y + this.charHeight - 2, this.charWidth, 2);
      break;
    case 'bar':
      this.ctx.fillRect(x, y, 2, this.charHeight);
      break;
  }
}

private startCursorBlink(): void {
  this.blinkInterval = setInterval(() => {
    this.cursorVisible = !this.cursorVisible;
    // Re-render cursor area only
    this.renderCursorArea();
  }, 500);
}
```

#### Selection Rendering

```typescript
private renderSelection(selection: SelectionRange): void {
  const { start, end } = this.normalizeSelection(selection);

  this.ctx.fillStyle = 'rgba(50, 150, 250, 0.3)';

  for (let row = start.row; row <= end.row; row++) {
    const colStart = row === start.row ? start.col : 0;
    const colEnd = row === end.row ? end.col : this.cols - 1;

    const x = colStart * this.charWidth;
    const y = row * this.charHeight;
    const width = (colEnd - colStart + 1) * this.charWidth;

    this.ctx.fillRect(x, y, width, this.charHeight);
  }
}
```

### Font Handling

```typescript
private measureCharacterSize(): void {
  // Use same approach as DOM renderer
  const computedStyle = window.getComputedStyle(this.container);
  const lineHeight = computedStyle.lineHeight || '1.2';

  // Create offscreen measurement
  this.ctx.font = `${this.fontSize}px ${this.fontFamily}`;
  const metrics = this.ctx.measureText('W');

  this.charWidth = metrics.width;
  this.charHeight = this.fontSize * parseFloat(lineHeight);
}

private buildFont(attrs: CellAttributes): string {
  const style = attrs.italic ? 'italic' : 'normal';
  const weight = attrs.bold ? 'bold' : 'normal';
  return `${style} ${weight} ${this.fontSize}px ${this.fontFamily}`;
}
```

### High DPI Support

```typescript
private setupCanvas(): void {
  const dpr = window.devicePixelRatio || 1;
  const rect = this.container.getBoundingClientRect();

  // Set display size
  this.canvas.style.width = `${rect.width}px`;
  this.canvas.style.height = `${rect.height}px`;

  // Set actual size in memory (scaled for DPI)
  this.canvas.width = rect.width * dpr;
  this.canvas.height = rect.height * dpr;

  // Scale context to match
  this.ctx.scale(dpr, dpr);
}
```

### Renderer Factory

```typescript
// src/terminal/renderer-factory.ts

export type RendererType = 'canvas' | 'dom';

export function createRenderer(
  container: HTMLElement,
  fontFamily: string,
  fontSize: number
): TerminalRenderer | CanvasRenderer {
  const rendererType = getRendererType();

  if (rendererType === 'canvas') {
    return new CanvasRenderer(container, fontFamily, fontSize);
  }
  return new TerminalRenderer(container, fontFamily, fontSize);
}

function getRendererType(): RendererType {
  // Check environment variable via Tauri
  const envValue = import.meta.env.EMTERM_RENDERER;
  if (envValue === 'canvas') {
    return 'canvas';
  }
  return 'dom';
}
```

### File Structure

```
src/terminal/
├── renderer.ts              # Existing DOM renderer (unchanged)
├── canvas-renderer.ts       # New Canvas 2D renderer
├── renderer-factory.ts      # Factory for creating renderers
├── renderer-interface.ts    # Common interface definition
├── performance.ts           # Existing performance utilities
├── unicode.ts               # Existing charWidth (shared)
├── attributes.ts            # Existing attributes (shared)
├── colors.ts                # Existing colors (shared)
└── grid.ts                  # Existing grid types (shared)
```

## Test Scenarios

### Unit Tests
- [ ] renderLine correctly draws text with default attributes
- [ ] renderLine applies foreground color correctly
- [ ] renderLine applies background color correctly
- [ ] renderLine handles bold font weight
- [ ] renderLine handles italic font style
- [ ] renderLine draws underline decoration
- [ ] renderLine draws strikethrough decoration
- [ ] renderLine handles dim attribute (opacity)
- [ ] renderLine skips hidden text
- [ ] renderCursor draws block cursor
- [ ] renderCursor draws underline cursor
- [ ] renderCursor draws bar cursor
- [ ] measureCharacterSize returns correct dimensions
- [ ] Wide characters occupy 2 cells width

### Integration Tests
- [ ] Full screen render produces correct output
- [ ] Dirty row optimization only renders changed rows
- [ ] Resize correctly updates canvas dimensions
- [ ] Selection highlight renders across multiple lines
- [ ] Cursor blink toggles visibility

### E2E Tests
- [ ] Terminal displays text correctly with Canvas renderer
- [ ] Scrolling works smoothly with large output
- [ ] Text selection and copy works
- [ ] Cursor appears and blinks at correct position
- [ ] Environment variable switches renderer

### Performance Tests
- [ ] Render time is lower than DOM renderer for full screen
- [ ] Frame drops are reduced during rapid scroll
- [ ] Memory usage is comparable to DOM renderer

## Security Considerations

- **Input Validation:** Text content is drawn via fillText, which does not interpret HTML
- **No XSS Risk:** Canvas API does not execute scripts from content
- **Resource Limits:** Canvas size is bounded by terminal dimensions

## Error Handling

### Error Cases

| Condition | Handling |
|-----------|----------|
| Canvas context unavailable | Application fails to start (no fallback) |
| Invalid font | Use fallback monospace font |
| Out of bounds cursor | Clamp to valid range |

**Note:** Canvas 2D is required. No fallback to DOM renderer is provided for unsupported environments.

## Performance Optimization

### Strategies
- **Dirty Row Tracking:** Only redraw rows that changed (existing pattern)
- **Batched Rendering:** Use requestAnimationFrame for render scheduling
- **Attribute Grouping:** Group consecutive cells with same attributes into single draw calls
- **Canvas State Caching:** Minimize context state changes

### Caching
- Font string cache per attribute combination
- Color string cache for RGB values

## Success Criteria

- [ ] All functional requirements implemented and tested
- [ ] Visual output matches DOM renderer
- [ ] Performance improved vs DOM renderer (measured by PerformanceMonitor)
- [ ] Feature flag correctly switches renderers
- [ ] All existing terminal features work correctly
- [ ] Code review completed

## Implementation Phases

### Phase 1: Core Rendering
**Goals:** Basic text rendering with Canvas 2D
**Deliverables:**
- CanvasRenderer class with basic structure
- Text rendering without attributes
- Character measurement

### Phase 2: Attributes and Styling
**Goals:** Full attribute support
**Deliverables:**
- Color rendering (foreground/background)
- Text decorations (bold, italic, underline, strikethrough)
- Dim and hidden attributes

### Phase 3: Cursor and Selection
**Goals:** Interactive features
**Deliverables:**
- Cursor rendering (block, underline, bar)
- Cursor blink
- Selection highlight

### Phase 4: Integration
**Goals:** Production readiness
**Deliverables:**
- Renderer factory with feature flag (temporary for comparison)
- Performance optimization
- Documentation

### Phase 5: Migration Complete
**Goals:** Full transition to Canvas 2D
**Deliverables:**
- Remove feature flag (Canvas becomes default and only renderer)
- Remove DOM renderer code
- Update documentation

## References

- Existing DOM renderer: `src/terminal/renderer.ts`
- Performance utilities: `src/terminal/performance.ts`
- Unicode width calculation: `src/terminal/unicode.ts`
- Cell attributes: `src/terminal/attributes.ts`
- Color palette: `src/terminal/colors.ts`
