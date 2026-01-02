# ANSI Control Sequence Parser and Renderer Specification

## 1. Overview

### 1.1 Purpose

Implement ANSI control sequence parsing and rendering in eMterm to enable full terminal emulator functionality including color display, cursor control, and screen manipulation.

### 1.2 Scope

- VT100/VT220 compatible control sequences
- xterm extensions (256 color, true color, mouse tracking)
- Hybrid implementation: Rust parser + TypeScript renderer
- **Out of scope**: Kitty Graphics Protocol, SIXEL, custom OSC extensions (Markdown)

### 1.3 Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Rust Backend                             │
│  ┌──────────┐    ┌──────────────┐    ┌─────────────────────┐   │
│  │   PTY    │───>│    ANSI      │───>│   Event Emitter     │   │
│  │  Reader  │    │   Parser     │    │   (Tauri Events)    │   │
│  └──────────┘    └──────────────┘    └─────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ IPC Events (JSON)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     TypeScript Frontend                         │
│  ┌──────────────┐    ┌──────────────┐    ┌─────────────────┐   │
│  │    Event     │───>│   Terminal   │───>│    Renderer     │   │
│  │   Handler    │    │    State     │    │   (DOM/Canvas)  │   │
│  └──────────────┘    └──────────────┘    └─────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. Supported Control Sequences

### 2.1 C0 Control Characters

| Code | Name | Action |
|------|------|--------|
| 0x00 | NUL | Ignore |
| 0x07 | BEL | Bell (audio/visual alert) |
| 0x08 | BS | Backspace (move cursor left) |
| 0x09 | HT | Horizontal Tab |
| 0x0A | LF | Line Feed |
| 0x0B | VT | Vertical Tab (same as LF) |
| 0x0C | FF | Form Feed (same as LF) |
| 0x0D | CR | Carriage Return |
| 0x1B | ESC | Escape (start sequence) |

### 2.2 Escape Sequences

| Sequence | Name | Action |
|----------|------|--------|
| ESC 7 | DECSC | Save cursor position |
| ESC 8 | DECRC | Restore cursor position |
| ESC D | IND | Index (move down, scroll if needed) |
| ESC E | NEL | Next Line |
| ESC H | HTS | Horizontal Tab Set |
| ESC M | RI | Reverse Index (move up, scroll if needed) |
| ESC c | RIS | Reset to Initial State |
| ESC [ | CSI | Control Sequence Introducer |
| ESC ] | OSC | Operating System Command |
| ESC ( | SCS | Select Character Set (G0) |
| ESC ) | SCS | Select Character Set (G1) |

### 2.3 CSI Sequences

#### 2.3.1 Cursor Movement

| Sequence | Name | Description |
|----------|------|-------------|
| CSI n A | CUU | Cursor Up n rows |
| CSI n B | CUD | Cursor Down n rows |
| CSI n C | CUF | Cursor Forward n columns |
| CSI n D | CUB | Cursor Back n columns |
| CSI n E | CNL | Cursor Next Line n rows |
| CSI n F | CPL | Cursor Previous Line n rows |
| CSI n G | CHA | Cursor Horizontal Absolute column n |
| CSI n ; m H | CUP | Cursor Position (row n, column m) |
| CSI n ; m f | HVP | Horizontal Vertical Position |
| CSI n d | VPA | Vertical Position Absolute row n |

#### 2.3.2 Cursor Visibility and Shape

| Sequence | Description |
|----------|-------------|
| CSI ? 25 h | Show cursor (DECTCEM) |
| CSI ? 25 l | Hide cursor (DECTCEM) |
| CSI n SP q | Set cursor style (0-6) |

#### 2.3.3 Erase Operations

| Sequence | Name | Description |
|----------|------|-------------|
| CSI n J | ED | Erase in Display (0=below, 1=above, 2=all, 3=scrollback) |
| CSI n K | EL | Erase in Line (0=right, 1=left, 2=all) |

#### 2.3.4 Insert/Delete

| Sequence | Name | Description |
|----------|------|-------------|
| CSI n @ | ICH | Insert n blank characters |
| CSI n P | DCH | Delete n characters |
| CSI n L | IL | Insert n lines |
| CSI n M | DL | Delete n lines |
| CSI n X | ECH | Erase n characters |

#### 2.3.5 Scrolling

| Sequence | Name | Description |
|----------|------|-------------|
| CSI n S | SU | Scroll Up n lines |
| CSI n T | SD | Scroll Down n lines |
| CSI t ; b r | DECSTBM | Set scroll region (top, bottom) |

#### 2.3.6 SGR (Select Graphic Rendition)

| Parameter | Description |
|-----------|-------------|
| 0 | Reset all attributes |
| 1 | Bold |
| 2 | Dim (faint) |
| 3 | Italic |
| 4 | Underline |
| 5 | Slow blink |
| 6 | Rapid blink |
| 7 | Reverse video |
| 8 | Conceal (hidden) |
| 9 | Strikethrough |
| 21 | Double underline |
| 22 | Normal intensity (not bold/dim) |
| 23 | Not italic |
| 24 | Not underlined |
| 25 | Not blinking |
| 27 | Not reversed |
| 28 | Not concealed |
| 29 | Not strikethrough |
| 30-37 | Foreground color (standard) |
| 38;5;n | Foreground 256 color |
| 38;2;r;g;b | Foreground RGB |
| 39 | Default foreground |
| 40-47 | Background color (standard) |
| 48;5;n | Background 256 color |
| 48;2;r;g;b | Background RGB |
| 49 | Default background |
| 90-97 | Bright foreground |
| 100-107 | Bright background |

#### 2.3.7 Mode Setting

| Sequence | Description |
|----------|-------------|
| CSI n h | Set Mode |
| CSI n l | Reset Mode |
| CSI ? n h | Set Private Mode |
| CSI ? n l | Reset Private Mode |

### 2.4 DEC Private Modes

| Mode | Name | Description |
|------|------|-------------|
| 1 | DECCKM | Cursor Keys Mode (application/normal) |
| 3 | DECCOLM | 132/80 Column Mode |
| 5 | DECSCNM | Screen Mode (reverse video) |
| 6 | DECOM | Origin Mode |
| 7 | DECAWM | Auto Wrap Mode |
| 12 | ATT160 | Cursor Blink |
| 25 | DECTCEM | Text Cursor Enable Mode |
| 47 | XTERM_ALTBUF | Use Alternate Screen Buffer |
| 1000 | X10_MOUSE | X10 Mouse Reporting |
| 1002 | BTN_EVENT_MOUSE | Button-Event Mouse Tracking |
| 1003 | ANY_EVENT_MOUSE | Any-Event Mouse Tracking |
| 1004 | FOCUS | Focus Tracking |
| 1005 | UTF8_MOUSE | UTF-8 Mouse Mode |
| 1006 | SGR_MOUSE | SGR Mouse Mode |
| 1047 | XTERM_ALTBUF | Alternate Screen Buffer |
| 1048 | XTERM_SAVE | Save Cursor |
| 1049 | XTERM_ALTBUF | Alternate Screen Buffer + Save Cursor |
| 2004 | BRACKETED_PASTE | Bracketed Paste Mode |

### 2.5 OSC Sequences

| OSC | Description |
|-----|-------------|
| OSC 0 ; text ST | Set icon name and window title |
| OSC 1 ; text ST | Set icon name |
| OSC 2 ; text ST | Set window title |
| OSC 4 ; c ; spec ST | Set/query color palette entry |
| OSC 7 ; url ST | Set current working directory |
| OSC 8 ; params ; uri ST | Hyperlink |
| OSC 10 ; color ST | Set/query foreground color |
| OSC 11 ; color ST | Set/query background color |
| OSC 777 ; emterm ; ... ST | eMterm extension (skeleton only) |

**Note**: String Terminator (ST) is ESC \ or BEL (0x07)

### 2.6 Device Status Reports

| Sequence | Response | Description |
|----------|----------|-------------|
| CSI 5 n | CSI 0 n | Device Status Report (OK) |
| CSI 6 n | CSI row ; col R | Cursor Position Report |
| CSI c | CSI ? 64 ; 1 ; 2 ; ... c | Primary Device Attributes |
| CSI > c | CSI > 41 ; version ; 0 c | Secondary Device Attributes |

---

## 3. Rust Parser Design

### 3.1 State Machine

```
                    ┌───────────────────────────────────┐
                    │                                   │
                    ▼                                   │
┌─────────┐  ESC  ┌─────────┐  [   ┌─────────┐        │
│ Ground  │──────>│ Escape  │─────>│   CSI   │        │
│         │       │         │      │  Entry  │        │
└─────────┘       └─────────┘      └─────────┘        │
     │                 │                │              │
     │ printable       │ ]             │ param        │
     ▼                 ▼               ▼              │
┌─────────┐      ┌─────────┐      ┌─────────┐        │
│ Print   │      │   OSC   │      │   CSI   │        │
│  Char   │      │  String │      │  Param  │        │
└─────────┘      └─────────┘      └─────────┘        │
     │                 │                │              │
     └─────────────────┴────────────────┴──────────────┘
                       final char / ST
```

### 3.2 Parser Module Structure

```
src-tauri/src/ansi/
├── mod.rs           # Module exports
├── parser.rs        # State machine implementation
├── sequence.rs      # Sequence type definitions
├── params.rs        # CSI parameter parsing
├── sgr.rs           # SGR attribute parsing
├── osc.rs           # OSC command parsing
└── charset.rs       # Character set handling
```

### 3.3 Core Data Structures

```rust
/// Parsed terminal action
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum TerminalAction {
    /// Print a character at current position
    Print(char),

    /// Execute a C0 control character
    Execute(u8),

    /// CSI sequence
    Csi(CsiAction),

    /// ESC sequence
    Esc(EscAction),

    /// OSC sequence
    Osc(OscAction),

    /// Hook for DCS sequences (future use)
    Hook { params: Vec<u16>, intermediates: Vec<u8>, ignore: bool },

    /// Put character in DCS
    Put(u8),

    /// Unhook DCS
    Unhook,
}

/// CSI action types
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "action")]
pub enum CsiAction {
    /// Cursor movement
    MoveCursor { direction: Direction, count: u16 },

    /// Cursor position (absolute)
    SetCursorPosition { row: u16, col: u16 },

    /// Erase display
    EraseDisplay { mode: EraseMode },

    /// Erase line
    EraseLine { mode: EraseMode },

    /// Insert/delete characters
    InsertChars { count: u16 },
    DeleteChars { count: u16 },

    /// Insert/delete lines
    InsertLines { count: u16 },
    DeleteLines { count: u16 },

    /// Scroll
    ScrollUp { count: u16 },
    ScrollDown { count: u16 },

    /// Set scroll region
    SetScrollRegion { top: u16, bottom: u16 },

    /// SGR attributes
    SetGraphicsRendition { attrs: Vec<SgrAttr> },

    /// Mode setting
    SetMode { mode: u16, value: bool, private: bool },

    /// Device status report
    DeviceStatus { report: DeviceStatusReport },

    /// Cursor style
    SetCursorStyle { style: CursorStyle },
}

/// SGR attribute
#[derive(Debug, Clone, Serialize)]
pub enum SgrAttr {
    Reset,
    Bold,
    Dim,
    Italic,
    Underline,
    Blink,
    Reverse,
    Hidden,
    Strikethrough,
    NoBold,
    NoItalic,
    NoUnderline,
    NoBlink,
    NoReverse,
    NoHidden,
    NoStrikethrough,
    Foreground(Color),
    Background(Color),
    DefaultForeground,
    DefaultBackground,
}

/// Color representation
#[derive(Debug, Clone, Serialize)]
pub enum Color {
    Indexed(u8),           // 0-255
    Rgb { r: u8, g: u8, b: u8 },
}

/// OSC action types
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "command")]
pub enum OscAction {
    SetTitle { title: String },
    SetIconName { name: String },
    SetTitleAndIcon { text: String },
    SetColorPalette { index: u8, color: String },
    SetWorkingDirectory { url: String },
    Hyperlink { params: String, uri: String },
    EmtermExtension { verb: String, params: Vec<String> },
}

/// Direction for cursor movement
#[derive(Debug, Clone, Copy, Serialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Erase mode
#[derive(Debug, Clone, Copy, Serialize)]
pub enum EraseMode {
    Below,      // 0
    Above,      // 1
    All,        // 2
    Scrollback, // 3
}
```

### 3.4 Parser API

```rust
/// ANSI sequence parser
pub struct Parser {
    state: State,
    params: Vec<u16>,
    intermediates: Vec<u8>,
    osc_buffer: Vec<u8>,
}

impl Parser {
    /// Create a new parser
    pub fn new() -> Self;

    /// Parse input bytes and emit actions
    pub fn parse<F>(&mut self, input: &[u8], mut emit: F)
    where
        F: FnMut(TerminalAction);

    /// Reset parser state
    pub fn reset(&mut self);
}
```

### 3.5 Integration with PTY Reader

```rust
// In lib.rs spawn_reader_thread modification

fn spawn_reader_thread(app: AppHandle, manager: PtyManager, session_id: String) {
    std::thread::spawn(move || {
        // ... existing setup code ...

        let mut parser = Parser::new();
        let mut buf = [0u8; 4096];
        let mut pending_actions: Vec<TerminalAction> = Vec::new();

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    parser.parse(&buf[..n], |action| {
                        pending_actions.push(action);
                    });

                    // Batch emit actions
                    if !pending_actions.is_empty() {
                        let payload = TerminalActionsPayload {
                            session_id: session_id.clone(),
                            actions: std::mem::take(&mut pending_actions),
                        };
                        let _ = app.emit("terminal_actions", payload);
                    }
                }
                Err(e) => { /* ... */ }
            }
        }
        // ... rest of cleanup ...
    });
}
```

---

## 4. TypeScript Renderer Design

### 4.1 Module Structure

```
src/
├── terminal/
│   ├── index.ts          # Module exports
│   ├── state.ts          # Terminal state management
│   ├── grid.ts           # Grid and cell structures
│   ├── buffer.ts         # Screen buffer (main + alternate)
│   ├── cursor.ts         # Cursor state
│   ├── attributes.ts     # Cell attributes
│   ├── renderer.ts       # DOM rendering
│   ├── colors.ts         # Color palette
│   └── unicode.ts        # Character width calculation
```

### 4.2 Core Data Structures

```typescript
/**
 * Cell attributes
 */
interface CellAttributes {
  fg: Color;
  bg: Color;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  underline: boolean;
  blink: boolean;
  reverse: boolean;
  hidden: boolean;
  strikethrough: boolean;
}

/**
 * Color representation
 */
type Color =
  | { type: 'default' }
  | { type: 'indexed'; index: number }
  | { type: 'rgb'; r: number; g: number; b: number };

/**
 * Terminal cell
 */
interface Cell {
  char: string;        // Unicode character (may be empty for wide char continuation)
  width: number;       // Character display width (0, 1, or 2)
  attrs: CellAttributes;
  dirty: boolean;      // Needs re-render
}

/**
 * Terminal line
 */
interface Line {
  cells: Cell[];
  dirty: boolean;
}

/**
 * Cursor state
 */
interface CursorState {
  row: number;
  col: number;
  visible: boolean;
  style: 'block' | 'underline' | 'bar';
  blink: boolean;
  savedPosition: { row: number; col: number } | null;
  savedAttributes: CellAttributes | null;
}

/**
 * Screen buffer
 */
interface ScreenBuffer {
  lines: Line[];
  scrollTop: number;
  scrollBottom: number;
  cursorState: CursorState;
}

/**
 * Terminal state
 */
interface TerminalState {
  cols: number;
  rows: number;
  mainBuffer: ScreenBuffer;
  alternateBuffer: ScreenBuffer;
  activeBuffer: 'main' | 'alternate';
  scrollback: Line[];
  maxScrollback: number;
  modes: TerminalModes;
  currentAttributes: CellAttributes;
  title: string;
  colorPalette: string[];
}

/**
 * Terminal modes
 */
interface TerminalModes {
  cursorKeys: 'application' | 'normal';
  autoWrap: boolean;
  originMode: boolean;
  reverseVideo: boolean;
  bracketedPaste: boolean;
  mouseTracking: MouseTrackingMode;
  focusTracking: boolean;
}
```

### 4.3 Terminal State Manager

```typescript
/**
 * Manages terminal state and processes actions
 */
class TerminalState {
  private state: TerminalStateData;
  private dirtyRows: Set<number> = new Set();

  constructor(cols: number, rows: number) {
    this.state = this.createInitialState(cols, rows);
  }

  /**
   * Process a terminal action from the parser
   */
  processAction(action: TerminalAction): void {
    switch (action.type) {
      case 'Print':
        this.printChar(action.data);
        break;
      case 'Execute':
        this.executeControl(action.data);
        break;
      case 'Csi':
        this.processCsi(action.data);
        break;
      case 'Esc':
        this.processEsc(action.data);
        break;
      case 'Osc':
        this.processOsc(action.data);
        break;
    }
  }

  /**
   * Get dirty rows for incremental rendering
   */
  getDirtyRows(): number[] {
    return Array.from(this.dirtyRows);
  }

  /**
   * Clear dirty flags after render
   */
  clearDirty(): void {
    this.dirtyRows.clear();
  }

  /**
   * Resize terminal
   */
  resize(cols: number, rows: number): void;

  /**
   * Get current buffer for rendering
   */
  getActiveBuffer(): ScreenBuffer;

  /**
   * Generate response for device status reports
   */
  generateResponse(report: DeviceStatusReport): Uint8Array | null;
}
```

### 4.4 Renderer

```typescript
/**
 * Renders terminal state to DOM
 */
class TerminalRenderer {
  private container: HTMLElement;
  private rowElements: HTMLElement[] = [];
  private cursorElement: HTMLElement;
  private charWidth: number;
  private charHeight: number;
  private pendingRender: boolean = false;

  constructor(container: HTMLElement, fontFamily: string, fontSize: number) {
    this.container = container;
    this.measureCharSize(fontFamily, fontSize);
    this.createCursor();
  }

  /**
   * Schedule a render on next animation frame
   */
  scheduleRender(state: TerminalState): void {
    if (this.pendingRender) return;
    this.pendingRender = true;

    requestAnimationFrame(() => {
      this.render(state);
      this.pendingRender = false;
    });
  }

  /**
   * Render terminal state
   */
  private render(state: TerminalState): void {
    const buffer = state.getActiveBuffer();
    const dirtyRows = state.getDirtyRows();

    // Update only dirty rows
    for (const rowIndex of dirtyRows) {
      this.renderRow(buffer.lines[rowIndex], rowIndex);
    }

    // Update cursor
    this.updateCursor(buffer.cursorState);

    state.clearDirty();
  }

  /**
   * Render a single row
   */
  private renderRow(line: Line, rowIndex: number): void {
    let html = '';
    let currentSpan = '';
    let currentAttrs: CellAttributes | null = null;

    for (const cell of line.cells) {
      if (cell.width === 0) continue; // Skip wide char continuation

      if (!this.attrsEqual(currentAttrs, cell.attrs)) {
        if (currentSpan) {
          html += this.closeSpan(currentSpan);
        }
        currentAttrs = cell.attrs;
        currentSpan = this.openSpan(currentAttrs);
      }

      currentSpan += this.escapeHtml(cell.char || ' ');
    }

    if (currentSpan) {
      html += this.closeSpan(currentSpan);
    }

    this.rowElements[rowIndex].innerHTML = html;
  }

  /**
   * Generate CSS for cell attributes
   */
  private getAttrStyle(attrs: CellAttributes): string;

  /**
   * Update cursor position and visibility
   */
  private updateCursor(cursor: CursorState): void;

  /**
   * Handle resize
   */
  resize(cols: number, rows: number): void;
}
```

### 4.5 Unicode Width Calculation

```typescript
/**
 * Calculate display width of a Unicode character
 * Based on Unicode East Asian Width property
 */
function getCharWidth(char: string): number {
  const code = char.codePointAt(0);
  if (code === undefined) return 0;

  // Combining characters
  if (isCombining(code)) return 0;

  // Control characters
  if (code < 0x20 || (code >= 0x7F && code < 0xA0)) return 0;

  // East Asian Wide/Fullwidth
  if (isEastAsianWide(code)) return 2;

  // Default to single width
  return 1;
}

/**
 * Check if character is East Asian Wide
 */
function isEastAsianWide(code: number): boolean {
  // CJK ranges
  if (code >= 0x1100 && code <= 0x115F) return true;  // Hangul Jamo
  if (code >= 0x2E80 && code <= 0x9FFF) return true;  // CJK
  if (code >= 0xAC00 && code <= 0xD7A3) return true;  // Hangul Syllables
  if (code >= 0xF900 && code <= 0xFAFF) return true;  // CJK Compatibility
  if (code >= 0xFE10 && code <= 0xFE1F) return true;  // Vertical forms
  if (code >= 0xFE30 && code <= 0xFE6F) return true;  // CJK Compatibility Forms
  if (code >= 0xFF00 && code <= 0xFF60) return true;  // Fullwidth forms
  if (code >= 0xFFE0 && code <= 0xFFE6) return true;  // Fullwidth symbols
  if (code >= 0x20000 && code <= 0x2FFFF) return true; // CJK Extension B+
  if (code >= 0x30000 && code <= 0x3FFFF) return true; // CJK Extension G+
  return false;
}
```

---

## 5. IPC Event Design

### 5.1 Event Types

```typescript
/**
 * Payload for terminal actions event
 */
interface TerminalActionsPayload {
  session_id: string;
  actions: TerminalAction[];
}

/**
 * Terminal action from Rust parser
 */
type TerminalAction =
  | { type: 'Print'; data: string }
  | { type: 'Execute'; data: number }
  | { type: 'Csi'; data: CsiAction }
  | { type: 'Esc'; data: EscAction }
  | { type: 'Osc'; data: OscAction };

/**
 * CSI action
 */
type CsiAction =
  | { action: 'MoveCursor'; direction: 'Up' | 'Down' | 'Left' | 'Right'; count: number }
  | { action: 'SetCursorPosition'; row: number; col: number }
  | { action: 'EraseDisplay'; mode: 'Below' | 'Above' | 'All' | 'Scrollback' }
  | { action: 'EraseLine'; mode: 'Below' | 'Above' | 'All' }
  | { action: 'InsertChars'; count: number }
  | { action: 'DeleteChars'; count: number }
  | { action: 'InsertLines'; count: number }
  | { action: 'DeleteLines'; count: number }
  | { action: 'ScrollUp'; count: number }
  | { action: 'ScrollDown'; count: number }
  | { action: 'SetScrollRegion'; top: number; bottom: number }
  | { action: 'SetGraphicsRendition'; attrs: SgrAttr[] }
  | { action: 'SetMode'; mode: number; value: boolean; private: boolean }
  | { action: 'DeviceStatus'; report: DeviceStatusReport }
  | { action: 'SetCursorStyle'; style: CursorStyle };

/**
 * SGR attribute
 */
type SgrAttr =
  | 'Reset'
  | 'Bold'
  | 'Dim'
  | 'Italic'
  | 'Underline'
  | 'Blink'
  | 'Reverse'
  | 'Hidden'
  | 'Strikethrough'
  | 'NoBold'
  | 'NoItalic'
  | 'NoUnderline'
  | 'NoBlink'
  | 'NoReverse'
  | 'NoHidden'
  | 'NoStrikethrough'
  | { Foreground: Color }
  | { Background: Color }
  | 'DefaultForeground'
  | 'DefaultBackground';

/**
 * OSC action
 */
type OscAction =
  | { command: 'SetTitle'; title: string }
  | { command: 'SetIconName'; name: string }
  | { command: 'SetTitleAndIcon'; text: string }
  | { command: 'SetColorPalette'; index: number; color: string }
  | { command: 'SetWorkingDirectory'; url: string }
  | { command: 'Hyperlink'; params: string; uri: string }
  | { command: 'EmtermExtension'; verb: string; params: string[] };
```

### 5.2 Event Flow

```
1. PTY output arrives
2. Rust parser processes bytes into actions
3. Actions batched per read() call
4. Single "terminal_actions" event emitted
5. TypeScript receives event
6. TerminalState processes each action
7. Dirty rows tracked
8. Renderer scheduled via requestAnimationFrame
9. Only dirty rows re-rendered
```

### 5.3 Response Handling

For device status reports that require responses:

```typescript
// In TypeScript event handler
async function handleTerminalActions(payload: TerminalActionsPayload): Promise<void> {
  for (const action of payload.actions) {
    state.processAction(action);

    // Check for response-requiring actions
    if (action.type === 'Csi' && action.data.action === 'DeviceStatus') {
      const response = state.generateResponse(action.data.report);
      if (response) {
        await ptyClient.write(response);
      }
    }
  }

  renderer.scheduleRender(state);
}
```

---

## 6. Performance Requirements

### 6.1 Targets

| Metric | Target |
|--------|--------|
| Input latency | < 16ms (60fps) |
| Throughput | > 10MB/s |
| Memory (10K lines scrollback) | < 50MB |

### 6.2 Optimization Strategies

#### 6.2.1 Rust Parser
- Avoid allocations in hot paths
- Use `&[u8]` slices instead of copying
- Batch action emission
- Inline small functions

#### 6.2.2 TypeScript Renderer
- Dirty row tracking
- `requestAnimationFrame` batching
- Reuse DOM elements
- Minimize style recalculations
- Use CSS classes instead of inline styles

#### 6.2.3 IPC
- Batch actions per PTY read
- Avoid excessive JSON nesting
- Consider binary serialization if JSON becomes bottleneck

### 6.3 Measurement

```typescript
// Performance monitoring
const metrics = {
  parseTime: 0,
  renderTime: 0,
  actionCount: 0,
  lastFrameTime: 0,
};

function measureRender(callback: () => void): void {
  const start = performance.now();
  callback();
  metrics.renderTime = performance.now() - start;

  if (metrics.renderTime > 16) {
    console.warn(`Slow render: ${metrics.renderTime.toFixed(2)}ms`);
  }
}
```

---

## 7. Test Strategy

### 7.1 Unit Tests

#### 7.1.1 Rust Parser Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sgr_basic() {
        let mut parser = Parser::new();
        let mut actions = vec![];

        parser.parse(b"\x1b[1;31mHello", |a| actions.push(a));

        assert_eq!(actions.len(), 6); // SGR + 5 chars
        assert!(matches!(&actions[0],
            TerminalAction::Csi(CsiAction::SetGraphicsRendition { attrs })
            if attrs.contains(&SgrAttr::Bold)
        ));
    }

    #[test]
    fn test_parse_cursor_movement() {
        let mut parser = Parser::new();
        let mut actions = vec![];

        parser.parse(b"\x1b[5A", |a| actions.push(a));

        assert!(matches!(&actions[0],
            TerminalAction::Csi(CsiAction::MoveCursor {
                direction: Direction::Up,
                count: 5
            })
        ));
    }

    #[test]
    fn test_parse_incomplete_sequence() {
        let mut parser = Parser::new();
        let mut actions = vec![];

        // Split sequence across calls
        parser.parse(b"\x1b[31", |a| actions.push(a));
        assert!(actions.is_empty());

        parser.parse(b"m", |a| actions.push(a));
        assert_eq!(actions.len(), 1);
    }
}
```

#### 7.1.2 TypeScript Tests

```typescript
describe('TerminalState', () => {
  it('should print character at cursor position', () => {
    const state = new TerminalState(80, 24);
    state.processAction({ type: 'Print', data: 'A' });

    const buffer = state.getActiveBuffer();
    expect(buffer.lines[0].cells[0].char).toBe('A');
  });

  it('should handle line wrap', () => {
    const state = new TerminalState(3, 2);
    state.processAction({ type: 'Print', data: 'ABCD' });

    const buffer = state.getActiveBuffer();
    expect(buffer.lines[0].cells.map(c => c.char).join('')).toBe('ABC');
    expect(buffer.lines[1].cells[0].char).toBe('D');
  });

  it('should calculate CJK width correctly', () => {
    expect(getCharWidth('\u4e2d')).toBe(2); // Chinese
    expect(getCharWidth('A')).toBe(1);       // ASCII
  });
});
```

### 7.2 Integration Tests

```typescript
describe('Parser to Renderer Integration', () => {
  it('should render colored text', async () => {
    const { state, renderer } = setupTerminal();

    // Simulate PTY output
    const actions = parseSequence('\x1b[31mRed\x1b[0m');
    for (const action of actions) {
      state.processAction(action);
    }

    renderer.render(state);

    const firstCell = getRenderedCell(0, 0);
    expect(firstCell.style.color).toBe('rgb(255, 0, 0)');
  });
});
```

### 7.3 Compatibility Tests

```bash
# vttest for VT100 compatibility
vttest

# Applications to test manually:
# - vim: Full screen editing, syntax highlighting
# - less: Paging, search highlighting
# - htop: Full screen, colors, updates
# - tmux: Multiplexing, nested terminals
# - fish: Autosuggestions, syntax highlighting
```

### 7.4 Performance Tests

```typescript
describe('Performance', () => {
  it('should handle 10MB/s throughput', async () => {
    const state = new TerminalState(80, 24);
    const data = generateRandomOutput(10 * 1024 * 1024); // 10MB

    const start = performance.now();
    const actions = parseSequence(data);
    for (const action of actions) {
      state.processAction(action);
    }
    const elapsed = performance.now() - start;

    expect(elapsed).toBeLessThan(1000); // < 1 second
  });

  it('should maintain 16ms frame budget', async () => {
    const { state, renderer } = setupTerminal();

    // Fill screen with content
    fillScreen(state);

    const start = performance.now();
    renderer.render(state);
    const elapsed = performance.now() - start;

    expect(elapsed).toBeLessThan(16);
  });
});
```

---

## 8. Acceptance Criteria

### 8.1 Required

1. [ ] SGR colors work (256 and true color)
2. [ ] Cursor movement sequences work correctly
3. [ ] vim file editing works normally
4. [ ] less page scrolling works normally
5. [ ] htop displays correctly
6. [ ] Alternate screen buffer switching works
7. [ ] Input latency is under 16ms
8. [ ] Throughput exceeds 10MB/s

### 8.2 Recommended

1. [ ] tmux/screen works in nested mode
2. [ ] Mouse tracking works
3. [ ] Hyperlinks (OSC 8) work
4. [ ] Bracketed Paste Mode works
5. [ ] Window title updates work
6. [ ] CJK characters display with correct width

---

## 9. Migration Path

### 9.1 Current State

```typescript
// Current: Simple text append
await ptyClient.onOutput((data) => {
  const text = new TextDecoder().decode(data);
  terminal.textContent += text;
});
```

### 9.2 Target State

```typescript
// Target: Full terminal emulation
await listen<TerminalActionsPayload>('terminal_actions', (event) => {
  for (const action of event.payload.actions) {
    terminalState.processAction(action);
  }
  renderer.scheduleRender(terminalState);
});
```

### 9.3 Backward Compatibility

- Replace `pty_output` event with `terminal_actions` event
- Keep raw output available for debugging
- Graceful fallback if parser encounters unknown sequences

---

## 10. References

- [XTerm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- [ECMA-48 Standard](https://www.ecma-international.org/publications-and-standards/standards/ecma-48/)
- [VT100 User Guide](https://vt100.net/docs/vt100-ug/)
- [Unicode East Asian Width](https://www.unicode.org/reports/tr11/)
- [vte crate (reference implementation)](https://github.com/alacritty/vte)
