# ANSI Control Sequence Parser and Renderer Implementation Plan

## Overview

This document describes the phased implementation plan for ANSI control sequence parsing and rendering in eMterm, derived from [SPEC.md](./SPEC.md).

---

## Phase Overview

```
Phase 1: Core Parser Infrastructure (Rust)
    │
    ▼
Phase 2: Basic Terminal State (TypeScript)
    │
    ▼
Phase 3: SGR and Color Rendering
    │
    ▼
Phase 4: Cursor and Screen Operations
    │
    ▼
Phase 5: Mode and Buffer Management
    │
    ▼
Phase 6: OSC and Device Status
    │
    ▼
Phase 7: Performance Optimization
```

---

## Phase 1: Core Parser Infrastructure (Rust)

### Goal

Implement the ANSI parser state machine in Rust that can parse PTY output and emit structured terminal actions.

### Deliverables

1. `src-tauri/src/ansi/mod.rs` - Module exports
2. `src-tauri/src/ansi/parser.rs` - State machine implementation
3. `src-tauri/src/ansi/sequence.rs` - TerminalAction enum and related types
4. `src-tauri/src/ansi/params.rs` - CSI parameter parsing
5. Integration with PTY reader thread

### Contracts

#### Parser API

```rust
pub struct Parser {
    // Internal state
}

impl Parser {
    /// Create a new parser
    pub fn new() -> Self;

    /// Parse input bytes, calling emit for each action
    pub fn parse<F>(&mut self, input: &[u8], emit: F)
    where
        F: FnMut(TerminalAction);

    /// Reset parser to initial state
    pub fn reset(&mut self);
}
```

#### TerminalAction Types

- `Print(char)` - Printable character
- `Execute(u8)` - C0 control character
- `Csi(CsiAction)` - CSI sequence
- `Esc(EscAction)` - Escape sequence
- `Osc(OscAction)` - OSC sequence

#### EscAction Types

```rust
#[derive(Debug, Clone, Serialize)]
pub enum EscAction {
    /// ESC 7 - Save cursor position and attributes
    SaveCursor,

    /// ESC 8 - Restore cursor position and attributes
    RestoreCursor,

    /// ESC D - Index (move cursor down, scroll if at bottom)
    Index,

    /// ESC E - Next Line (move to column 0 of next line, scroll if needed)
    NextLine,

    /// ESC H - Horizontal Tab Set (set tab stop at current column)
    HorizontalTabSet,

    /// ESC M - Reverse Index (move cursor up, scroll if at top)
    ReverseIndex,

    /// ESC c - Reset to Initial State (full terminal reset)
    ResetToInitialState,

    /// ESC ( C - Select G0 Character Set
    SetG0CharSet(CharSet),

    /// ESC ) C - Select G1 Character Set
    SetG1CharSet(CharSet),
}

/// Character set designations
#[derive(Debug, Clone, Copy, Serialize)]
pub enum CharSet {
    Ascii,           // B
    DecLineDrawing,  // 0
    Uk,              // A
}
```

#### IPC Event

```rust
struct TerminalActionsPayload {
    session_id: String,
    actions: Vec<TerminalAction>,
}
```

Event name: `terminal_actions`

### Dependencies

- None (foundational phase)

### Estimated Effort

**Large** - State machine design, comprehensive sequence parsing

### Test Criteria

- [ ] Parse printable ASCII characters
- [ ] Parse C0 control characters (BEL, BS, HT, LF, CR)
- [ ] Parse incomplete sequences across buffer boundaries
- [ ] Parse CSI sequences with parameters
- [ ] Parse multi-parameter CSI sequences (e.g., `CSI 1;31m`)
- [ ] Emit correct TerminalAction variants
- [ ] Unit tests for each sequence type

### Verification Checklist

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes
- [ ] Parser handles split sequences correctly
- [ ] Event emitted to frontend contains valid JSON
- [ ] `pty_output` event replaced with `terminal_actions` event

---

## Phase 2: Basic Terminal State (TypeScript)

### Goal

Implement the terminal state management system in TypeScript that receives parsed actions and maintains screen state.

### Deliverables

1. `src/terminal/index.ts` - Module exports
2. `src/terminal/state.ts` - TerminalState class
3. `src/terminal/grid.ts` - Grid and Line structures
4. `src/terminal/buffer.ts` - ScreenBuffer implementation
5. `src/terminal/cursor.ts` - CursorState management
6. `src/terminal/attributes.ts` - CellAttributes defaults
7. `src/terminal/renderer.ts` - Basic DOM renderer
8. Updated `src/main.ts` - Event handler integration

### Contracts

#### TerminalState API

```typescript
class TerminalState {
  constructor(cols: number, rows: number);
  processAction(action: TerminalAction): void;
  getActiveBuffer(): ScreenBuffer;
  getDirtyRows(): number[];
  clearDirty(): void;
  resize(cols: number, rows: number): void;
}
```

#### Cell Structure

```typescript
interface Cell {
  char: string;
  width: number;
  attrs: CellAttributes;
  dirty: boolean;
}
```

#### Renderer API

```typescript
class TerminalRenderer {
  constructor(container: HTMLElement, fontFamily: string, fontSize: number);
  scheduleRender(state: TerminalState): void;
  resize(cols: number, rows: number): void;
}
```

### Dependencies

- Phase 1 (parser provides actions)

### Estimated Effort

**Large** - Core data structures, grid management, basic rendering

### Test Criteria

- [ ] Initialize empty grid with correct dimensions
- [ ] Print character updates cell at cursor position
- [ ] Cursor advances after print
- [ ] Line wrap at column boundary
- [ ] Newline moves cursor to next row
- [ ] Carriage return moves cursor to column 0
- [ ] Backspace moves cursor left
- [ ] Tab moves cursor to next tab stop
- [ ] Dirty row tracking works correctly

### Verification Checklist

- [ ] `bun test` passes
- [ ] Text appears in terminal window
- [ ] Basic shell prompt displays
- [ ] Typing shows characters
- [ ] Enter key creates new line

---

## Phase 3: SGR and Color Rendering

### Goal

Implement SGR (Select Graphic Rendition) parsing in Rust and color/style rendering in TypeScript.

### Deliverables

1. `src-tauri/src/ansi/sgr.rs` - SGR attribute parsing
2. `src/terminal/colors.ts` - Color palette (16 + 256 + RGB)
3. Updated `src/terminal/attributes.ts` - Full attribute handling
4. Updated `src/terminal/renderer.ts` - Style application

### Contracts

#### SgrAttr Enum

As defined in SPEC.md Section 3.3:
- Reset, Bold, Dim, Italic, Underline, Blink, Reverse, Hidden, Strikethrough
- No* variants for attribute reset
- Foreground(Color), Background(Color)
- DefaultForeground, DefaultBackground

#### Color Type

```typescript
type Color =
  | { type: 'default' }
  | { type: 'indexed'; index: number }
  | { type: 'rgb'; r: number; g: number; b: number };
```

#### Color Palette

- Indices 0-7: Standard colors
- Indices 8-15: Bright colors
- Indices 16-231: 6x6x6 color cube
- Indices 232-255: Grayscale ramp

### Dependencies

- Phase 2 (terminal state and renderer)

### Estimated Effort

**Medium** - SGR parsing, color palette, CSS generation

### Test Criteria

- [ ] Parse `CSI 0 m` (reset)
- [ ] Parse `CSI 1 m` (bold)
- [ ] Parse `CSI 31 m` (red foreground)
- [ ] Parse `CSI 38;5;196 m` (256-color red)
- [ ] Parse `CSI 38;2;255;0;0 m` (RGB red)
- [ ] Parse combined attributes `CSI 1;4;31 m`
- [ ] Render bold text with font-weight
- [ ] Render colored text with correct CSS color
- [ ] Render background colors
- [ ] Render reverse video correctly

### Verification Checklist

- [ ] `cargo test` and `bun test` pass
- [ ] Colored ls output displays correctly
- [ ] `echo -e "\e[1;31mRed Bold\e[0m"` renders correctly
- [ ] 256-color test pattern displays correctly
- [ ] True color gradient displays correctly

---

## Phase 4: Cursor and Screen Operations

### Goal

Implement cursor movement, erase operations, and scroll operations.

### Deliverables

1. Extended CSI parsing in `src-tauri/src/ansi/parser.rs`
2. Cursor movement handling in `src/terminal/state.ts`
3. Erase operations in `src/terminal/buffer.ts`
4. Scroll operations in `src/terminal/buffer.ts`
5. Insert/delete operations in `src/terminal/buffer.ts`

### Contracts

#### Cursor Movement Actions

- MoveCursor { direction: Up|Down|Left|Right, count: u16 }
- SetCursorPosition { row: u16, col: u16 }

#### Erase Actions

- EraseDisplay { mode: Below|Above|All|Scrollback }
- EraseLine { mode: Below|Above|All }

#### Line Operations

- InsertLines { count: u16 }
- DeleteLines { count: u16 }
- InsertChars { count: u16 }
- DeleteChars { count: u16 }

#### Scroll Actions

- ScrollUp { count: u16 }
- ScrollDown { count: u16 }
- SetScrollRegion { top: u16, bottom: u16 }

### Dependencies

- Phase 3 (attributes for erase fill)

### Estimated Effort

**Medium** - Multiple CSI sequences, cursor edge cases

### Test Criteria

- [ ] CUU/CUD/CUF/CUB move cursor correctly
- [ ] CUP sets absolute position
- [ ] Cursor stays within bounds
- [ ] ED 0 erases from cursor to end
- [ ] ED 1 erases from start to cursor
- [ ] ED 2 erases entire screen
- [ ] EL modes work correctly
- [ ] SU/SD scroll content
- [ ] IL/DL insert/delete lines
- [ ] ICH/DCH insert/delete characters
- [ ] Scroll region limits scrolling

### Verification Checklist

- [ ] Tests pass
- [ ] `clear` command clears screen
- [ ] `less` paging works
- [ ] Cursor positioning in editors visible
- [ ] Scroll regions work (test with `top`)

---

## Phase 5: Mode and Buffer Management

### Goal

Implement terminal modes, alternate screen buffer, and cursor save/restore.

### Deliverables

1. DEC private mode parsing
2. `src/terminal/modes.ts` - TerminalModes management
3. Alternate buffer in `src/terminal/state.ts`
4. Cursor save/restore functionality
5. Cursor visibility and style

### Contracts

#### Terminal Modes (from SPEC.md Section 2.4)

- DECCKM (1): Cursor keys mode
- DECCOLM (3): Column mode
- DECSCNM (5): Screen mode (reverse)
- DECOM (6): Origin mode
- DECAWM (7): Auto wrap mode
- DECTCEM (25): Cursor visibility
- Alternate buffer (47, 1047, 1049)
- Mouse tracking (1000, 1002, 1003, 1006) - skeleton only
- Focus tracking (1004) - skeleton only
- Bracketed paste (2004)

#### Cursor Style

- Block, Underline, Bar
- Blink on/off

#### ESC Sequences

- ESC 7 / ESC 8: Save/restore cursor position and attributes
- ESC D: Index (move cursor down, scroll if at bottom)
- ESC E: Next Line (move to column 0 of next line, scroll if needed)
- ESC H: Horizontal Tab Set (set tab stop at current column)
- ESC M: Reverse Index (move cursor up, scroll if at top)
- ESC c: Reset to Initial State (full terminal reset)
- ESC ( / ESC ): Select G0/G1 Character Set (requires charset.rs)

### Dependencies

- Phase 4 (cursor operations)

### Estimated Effort

**Medium** - Mode management, buffer switching

### Test Criteria

- [ ] DECTCEM shows/hides cursor
- [ ] Alternate buffer switch preserves main buffer
- [ ] Return from alternate restores main buffer
- [ ] Cursor save/restore works (ESC 7 / ESC 8)
- [ ] Auto wrap mode controls line wrapping
- [ ] Bracketed paste mode flag toggles
- [ ] ESC c resets terminal state
- [ ] ESC D (Index) scrolls at bottom margin
- [ ] ESC E (Next Line) moves to column 0 of next line
- [ ] ESC H (Horizontal Tab Set) sets tab stop
- [ ] ESC M (Reverse Index) scrolls at top margin
- [ ] ESC ( / ESC ) character set switching works

### Verification Checklist

- [ ] Tests pass
- [ ] `vim` opens in alternate buffer
- [ ] Exiting `vim` restores shell output
- [ ] `htop` displays full screen
- [ ] Cursor visibility toggles work

---

## Phase 6: OSC and Device Status

### Goal

Implement OSC sequences and device status reports.

### Deliverables

1. `src-tauri/src/ansi/osc.rs` - OSC parsing
2. OSC handling in TypeScript
3. Device status report responses
4. Window title updates

### Contracts

#### OSC Actions (from SPEC.md Section 2.5)

- SetTitle { title: String }
- SetIconName { name: String }
- SetTitleAndIcon { text: String }
- SetColorPalette { index: u8, color: String }
- SetWorkingDirectory { url: String }
- Hyperlink { params: String, uri: String }
- EmtermExtension { verb: String, params: Vec<String> } - skeleton only

#### Device Status Reports

- CSI 5 n -> CSI 0 n (device OK)
- CSI 6 n -> CSI row;col R (cursor position)
- CSI c -> Primary device attributes
- CSI > c -> Secondary device attributes

### Dependencies

- Phase 5 (terminal state complete)

### Estimated Effort

**Small** - OSC parsing, simple response generation

### Test Criteria

- [ ] Parse OSC 0 (title + icon)
- [ ] Parse OSC 2 (title only)
- [ ] Parse OSC 7 (working directory)
- [ ] Parse OSC 8 (hyperlink)
- [ ] Detect OSC terminator (BEL or ESC \)
- [ ] Window title updates in Tauri
- [ ] DSR 5 returns OK response
- [ ] DSR 6 returns cursor position

### Verification Checklist

- [ ] Tests pass
- [ ] Shell title shows in window
- [ ] `printf '\e]0;My Title\a'` changes title
- [ ] Hyperlinks render (if enabled)
- [ ] Applications querying terminal work

---

## Phase 7: Performance Optimization

### Goal

Achieve performance targets: <16ms input latency, >10MB/s throughput.

### Deliverables

1. Parser hot path optimization
2. Renderer batching improvements
3. CSS class-based styling
4. Dirty region optimization
5. Performance measurement instrumentation

### Contracts

#### Performance Targets (from SPEC.md Section 6.1)

| Metric | Target |
|--------|--------|
| Input latency | < 16ms |
| Throughput | > 10MB/s |
| Memory (10K scrollback) | < 50MB |

#### Optimization Strategies

Rust Parser:
- Avoid allocations in hot paths
- Use slices instead of copying
- Batch action emission
- Inline small functions

TypeScript Renderer:
- Dirty row tracking
- requestAnimationFrame batching
- Reuse DOM elements
- CSS classes instead of inline styles

### Dependencies

- All previous phases

### Estimated Effort

**Medium** - Profiling, targeted optimization

### Test Criteria

- [ ] 10MB random data processed in < 1 second
- [ ] Full screen render in < 16ms
- [ ] Input response visually immediate
- [ ] No memory leaks over extended use
- [ ] Scrollback stays within memory budget

### Verification Checklist

- [ ] Performance tests pass
- [ ] `cat large_file` is responsive
- [ ] `yes | head -100000` handles high throughput
- [ ] Typing remains responsive during output
- [ ] Memory usage stable over time

---

## Component Design

### Rust Module Structure

```
src-tauri/src/
├── lib.rs              # Add ansi module, update reader thread
├── ansi/
│   ├── mod.rs          # pub use exports
│   ├── parser.rs       # State machine
│   ├── sequence.rs     # TerminalAction, CsiAction, EscAction, etc.
│   ├── params.rs       # Parameter parsing utilities
│   ├── sgr.rs          # SGR attribute parsing
│   ├── osc.rs          # OSC command parsing
│   └── charset.rs      # Character set handling (required for ESC ( / ESC ))
└── pty/
    └── ...             # Existing (unchanged)
```

### TypeScript Module Structure

```
src/
├── main.ts             # Update event handler
├── terminal/
│   ├── index.ts        # Module exports
│   ├── state.ts        # TerminalState class
│   ├── grid.ts         # Grid, Line, Cell
│   ├── buffer.ts       # ScreenBuffer
│   ├── cursor.ts       # CursorState
│   ├── attributes.ts   # CellAttributes
│   ├── modes.ts        # TerminalModes
│   ├── renderer.ts     # TerminalRenderer
│   ├── colors.ts       # Color palette
│   └── unicode.ts      # Character width
├── types/
│   ├── pty.ts          # Existing
│   └── terminal.ts     # TerminalAction types
└── pty/
    └── ...             # Existing (unchanged)
```

### IPC Event Flow

```
1. PTY read() returns bytes
2. Parser.parse() emits TerminalActions
3. Actions collected into Vec
4. Single "terminal_actions" event emitted
5. TypeScript listener receives payload
6. TerminalState.processAction() called for each
7. Dirty rows accumulated
8. requestAnimationFrame schedules render
9. Renderer updates only dirty rows
10. Cursor position updated
```

---

## Test Strategy

### Unit Tests

**Rust (cargo test)**
- Parser state transitions
- CSI parameter parsing
- SGR attribute parsing
- OSC parsing
- Edge cases (incomplete sequences, malformed input)

**TypeScript (bun test)**
- Grid operations
- Cursor movement boundaries
- Attribute merging
- Color palette lookup
- Character width calculation
- Buffer operations

### Integration Tests

- Parser to renderer pipeline
- Event payload serialization
- Resize handling
- Buffer switching

### Manual Compatibility Tests

After Phase 5:
- [ ] bash prompt displays correctly
- [ ] ls with colors works
- [ ] vim opens and edits files
- [ ] less pages through files
- [ ] htop displays and updates
- [ ] clear clears screen

After Phase 6:
- [ ] tmux/screen nesting works
- [ ] Shell title updates
- [ ] Working directory tracking

After Phase 7:
- [ ] Large file cat is responsive
- [ ] High-speed output (yes) handles well
- [ ] Typing responsive during output

---

## Risk Mitigation

### Parser Complexity

Risk: State machine becomes unwieldy
Mitigation: Reference vte crate architecture, incremental testing

### Performance Bottleneck

Risk: IPC overhead or rendering too slow
Mitigation: Early profiling, batch actions, dirty tracking

### Compatibility Issues

Risk: Applications render incorrectly
Mitigation: vttest validation, manual testing with common tools

---

## Acceptance Criteria Mapping

### Required (from SPEC.md Section 8.1)

| Criteria | Phase |
|----------|-------|
| SGR colors work (256 + true color) | Phase 3 |
| Cursor movement sequences work | Phase 4 |
| vim file editing works | Phase 5 |
| less page scrolling works | Phase 4-5 |
| htop displays correctly | Phase 5 |
| Alternate screen buffer switching | Phase 5 |
| Input latency < 16ms | Phase 7 |
| Throughput > 10MB/s | Phase 7 |

### Recommended (from SPEC.md Section 8.2)

| Criteria | Phase |
|----------|-------|
| tmux/screen nested mode | Phase 6 |
| Mouse tracking | Phase 5 (skeleton) |
| Hyperlinks (OSC 8) | Phase 6 |
| Bracketed Paste Mode | Phase 5 |
| Window title updates | Phase 6 |
| CJK character width | Phase 2 |

---

## Implementation Order Summary

1. **Phase 1**: Rust parser (foundation)
2. **Phase 2**: TypeScript terminal state + basic renderer
3. **Phase 3**: SGR/colors (visual feedback)
4. **Phase 4**: Cursor/screen ops (editor support)
5. **Phase 5**: Modes/buffers (vim/htop)
6. **Phase 6**: OSC/device status (completeness)
7. **Phase 7**: Performance (polish)

Each phase builds on the previous and provides testable, demonstrable progress.
