# Feature: WASM ANSI Parser + Binary IPC (Sprint 6)

## Overview

Move the ANSI parser from the Rust backend (`src-tauri/src/ansi/`) to the WASM crate (`wasm/src/`), and replace JSON-based IPC with binary transfer via Tauri Channel API. This creates an end-to-end WASM processing pipeline: raw PTY bytes flow directly to `process_pty_data(&[u8])`, which parses and handles all terminal actions internally. OSC/APC/DCS actions that require DOM operations are delivered via closure callbacks (multi-tab safe). Image processing (Kitty/SIXEL) uses a WASM→JS→Backend round-trip for decoding. Also adds `TerminalState.dispose()` for WASM resource cleanup.

## Objectives

- Move ANSI parser from backend to WASM, achieving end-to-end WASM processing
- Replace JSON IPC (`app.emit("terminal_actions")`) with binary transfer via Tauri `Channel<Vec<u8>>`
- Implement `process_pty_data(&[u8])` as the single entry point for all PTY data
- Use closure callbacks for OSC/APC/DCS (multi-tab safe, no `globalThis` pollution)
- Rebuild image pipeline: WASM detects APC/DCS → JS callback → `invoke()` to backend for decoding
- Simplify backend: remove ANSI parser, remove `TerminalAction` type, PTY reader sends raw bytes only
- Add `TerminalState.dispose()` for WASM resource cleanup (carry-over from Sprint 1)
- Remove `terminal_actions` and `pty_output` events

## User Stories

### US1: End-to-End WASM Processing

As the terminal data pipeline, I want all PTY bytes to be processed entirely within WASM, so that JSON serialization/deserialization overhead is eliminated and the full parse-to-state-update path is WASM-internal.

**Acceptance Criteria:**
- [ ] `core.process_pty_data(data: &[u8])` parses ANSI sequences and dispatches to internal handlers
- [ ] Print, C0, CSI, ESC actions are handled internally (no WASM-TS boundary crossing)
- [ ] Parser maintains state between calls (handles sequences split across PTY read chunks)
- [ ] Same parsing results as the existing Rust backend parser for all supported sequences

### US2: Binary IPC

As the PTY communication layer, I want raw bytes transferred from the backend to the frontend via Tauri Channel API, so that JSON serialization overhead is eliminated.

**Acceptance Criteria:**
- [ ] Backend `pty_spawn` command accepts a `Channel<Vec<u8>>` parameter
- [ ] PTY reader thread sends raw bytes via `channel.send(bytes)` (no JSON serialization)
- [ ] Frontend receives `Uint8Array` via channel `onmessage` callback
- [ ] Frontend passes received bytes directly to `core.process_pty_data(data)`
- [ ] `terminal_actions` event is completely removed
- [ ] `pty_output` event is completely removed

### US3: Closure Callbacks for DOM Actions

As a multi-tab terminal, I want OSC/APC/DCS actions delivered via instance-scoped closure callbacks, so that each tab's WASM instance calls its own JS handlers without `globalThis` pollution.

**Acceptance Criteria:**
- [ ] `TerminalCore` accepts closure callbacks via setter methods (e.g., `set_osc_callback(callback)`)
- [ ] OSC sequences (title, working directory, hyperlink, markdown, semantic prompt, color palette) trigger the `on_osc` callback
- [ ] APC sequences (Kitty Graphics) trigger the `on_apc` callback with raw protocol data
- [ ] DCS sequences (SIXEL) trigger the `on_dcs` callback with raw protocol data
- [ ] BEL character triggers the `on_bell` callback
- [ ] Device responses (DSR/DA) trigger the `on_device_response` callback with response bytes
- [ ] Callbacks are per-instance (not global), safe for multi-tab

### US4: Image Processing Pipeline

As the image display system, I want Kitty/SIXEL image data to flow from WASM to the backend for decoding, so that CPU-intensive image processing stays in the backend thread.

**Acceptance Criteria:**
- [ ] WASM parser detects APC (Kitty) sequences and calls `on_apc` callback with raw protocol data
- [ ] WASM parser detects DCS (SIXEL) sequences and calls `on_dcs` callback with raw protocol data
- [ ] JS callback sends image data to backend via `invoke("process_image", { data })` or similar
- [ ] Backend decodes image and emits `image_event` as before
- [ ] Cursor position for image placement obtained from WASM `get_cursor_row()`/`get_cursor_col()` at callback time
- [ ] Image display works correctly for both Kitty and SIXEL protocols

### US5: Backend Simplification

As the Rust backend, I want to be simplified to just PTY management and image processing, with the ANSI parser removed entirely.

**Acceptance Criteria:**
- [ ] `src-tauri/src/ansi/` directory is removed (parser moved to WASM)
- [ ] `TerminalAction`, `CsiAction`, `EscAction`, `OscAction` types removed from backend
- [ ] `TerminalActionsPayload` struct removed from backend
- [ ] PTY reader thread sends raw bytes via Channel (no parsing)
- [ ] Backend retains image processing capability via new `invoke` command
- [ ] Backend retains `pty_spawn`, `pty_write`, `pty_resize`, `pty_kill` commands

### US6: TerminalState Dispose

As the terminal lifecycle manager, I want `TerminalState.dispose()` to properly clean up WASM resources when a tab is closed.

**Acceptance Criteria:**
- [ ] `TerminalState.dispose()` method added
- [ ] Dispose calls `WasmGrid.free()` on both primary and alternate grids
- [ ] Dispose clears closure callbacks to prevent memory leaks
- [ ] `TerminalApp.dispose()` calls `state.dispose()` during cleanup

## Technical Requirements

### Functional Requirements

#### WASM ANSI Parser
- **FR1:** `TerminalCore.process_pty_data(data: &[u8])` processes raw PTY bytes end-to-end. Internally calls the ANSI parser, dispatches recognized actions to the appropriate WASM handler (handle_print, handle_execute, handle_csi, handle_esc), and triggers callbacks for OSC/APC/DCS.
- **FR2:** The ANSI parser in WASM is a direct port of `src-tauri/src/ansi/parser.rs`. It maintains the same state machine and produces the same actions for all supported sequences.
- **FR3:** The parser handles streaming input: sequences split across multiple `process_pty_data()` calls are handled correctly by maintaining parser state between calls.
- **FR4:** CSI dispatch within WASM is internal: `process_pty_data` calls `handle_csi_internal()` which routes to the existing CSI handlers (cursor, screen, edit, scroll, sgr, modes, device) without crossing the WASM-TS boundary.
- **FR5:** ESC dispatch within WASM is internal: `process_pty_data` calls `handle_esc_internal()` which routes to the existing ESC handlers.

#### Closure Callbacks
- **FR6:** `TerminalCore.set_osc_callback(callback: JsValue)` registers a JS closure called for OSC sequences. The callback receives `(action_type: u8, data: &str)` where `action_type` encodes the OSC variant.
- **FR7:** `TerminalCore.set_apc_callback(callback: JsValue)` registers a JS closure called for APC sequences. The callback receives the raw APC payload as `&[u8]`.
- **FR8:** `TerminalCore.set_dcs_callback(callback: JsValue)` registers a JS closure called for DCS sequences. The callback receives the raw DCS payload as `&[u8]`.
- **FR9:** `TerminalCore.set_bell_callback(callback: JsValue)` registers a JS closure called when BEL (0x07) is encountered. No parameters.
- **FR10:** `TerminalCore.set_device_response_callback(callback: JsValue)` registers a JS closure called when a device response (DSR/DA) is generated. The callback receives response bytes as `&[u8]`.
- **FR11:** Callbacks are stored as `Option<js_sys::Function>` in the `TerminalCore` struct. When set to `None`, the action is silently ignored.
- **FR12:** BEL notification switches from sentinel return value (0xFE) to callback. `handle_execute()` no longer returns a sentinel for BEL.
- **FR13:** Device responses switch from `get_response_bytes()` polling to callback. The response buffer is cleared after callback invocation.

#### Binary IPC
- **FR14:** `pty_spawn` Tauri command accepts a `channel: Channel<Vec<u8>>` parameter for binary data transfer.
- **FR15:** PTY reader thread sends raw PTY bytes via `channel.send(buf[..n].to_vec())` without any ANSI parsing.
- **FR16:** The `terminal_actions` event and `TerminalActionsPayload` struct are removed from the backend.
- **FR17:** The `pty_output` event is removed from the backend.
- **FR18:** `PtyClient` is refactored: `onTerminalActions()` and `onOutput()` methods are removed, replaced by a Channel-based binary data flow.

#### Image Processing Pipeline
- **FR19:** When WASM parser encounters an APC sequence containing Kitty Graphics data, it calls the `on_apc` callback with the raw Kitty protocol payload.
- **FR20:** When WASM parser encounters a DCS sequence containing SIXEL data, it calls the `on_dcs` callback with the raw SIXEL payload.
- **FR21:** JS APC/DCS callback handler obtains cursor position from WASM `get_cursor_row()`/`get_cursor_col()` and sends image data to backend via `invoke("process_image_data", { protocol, data, cursor_row, cursor_col })`.
- **FR22:** Backend exposes a new `process_image_data` Tauri command that accepts image protocol data and cursor position, processes the image using the existing `ImageProcessor`, and emits `image_event` as before.
- **FR23:** The image event listener in `TerminalApp` remains unchanged (listening to `image_event`).

#### Frontend Refactoring
- **FR24:** `PtyClient.spawn()` creates a Tauri Channel and passes it to the `pty_spawn` command. The channel's `onmessage` callback receives binary data.
- **FR25:** `TerminalApp.setupPtyHandlers()` is refactored: instead of listening to `terminal_actions` events, it registers callbacks on the WASM `TerminalCore` instance and processes binary data from the Channel.
- **FR26:** The `TerminalActionsPayload` TypeScript type is removed.
- **FR27:** The `processAction()` method in `TerminalState` is no longer the main entry point. Instead, `process_pty_data()` on the WASM core is called directly.

#### TerminalState.dispose()
- **FR28:** `TerminalState` class gains a `dispose()` method that frees WASM resources.
- **FR29:** `dispose()` calls `WasmGrid.free()` on both primary and alternate buffer grids.
- **FR30:** `dispose()` sets callback references to null to release JS closures.
- **FR31:** `TerminalApp.dispose()` calls `this.state.dispose()` before setting state to null.

#### Backend Cleanup
- **FR32:** `src-tauri/src/ansi/` directory is deleted (parser.rs, sequence.rs, params.rs, sgr.rs, mod.rs moved to WASM; apc.rs, dcs.rs types preserved in backend for image processing).
- **FR33:** APC/DCS type definitions needed for image processing are preserved in the backend (possibly in a new `src-tauri/src/image/types.rs` or kept as-is).
- **FR34:** `TerminalActionsPayload` struct is removed from `src-tauri/src/lib.rs`.
- **FR35:** PTY reader thread is simplified: read bytes → send via Channel. No parser, no `TerminalAction` processing.
- **FR36:** Cursor position tracking (for image placement) is removed from the PTY reader thread (now handled by WASM).

### Non-Functional Requirements

- **NFR1 - Performance:** JSON serialize/deserialize is completely eliminated from the PTY data path.
- **NFR2 - Performance:** Per PTY chunk, the WASM-TS boundary is crossed once for `process_pty_data()` plus N times for callbacks (N = number of OSC/APC/DCS sequences in the chunk, typically 0 for normal text output).
- **NFR3 - Performance:** Large output throughput (`cat large_file`) improves 3-5x over Sprint 5.
- **NFR4 - Binary size:** WASM binary < 80KB total (Sprint 5: 58.9KB, parser code estimate ~15KB).
- **NFR5 - Compatibility:** All existing TypeScript tests pass.
- **NFR6 - Compatibility:** vttest basic tests unchanged.
- **NFR7 - Multi-tab safety:** Closure callbacks are per-instance. No `globalThis` pollution.
- **NFR8 - Compatibility:** All existing image display functionality (Kitty/SIXEL) works correctly.
- **NFR9 - Resource cleanup:** `dispose()` properly releases all WASM resources and callbacks.

## Implementation Approach

### Architecture

**Before (Sprint 5):**
```
┌─────────────────────────────────────────────────┐
│ Rust Backend                                    │
│                                                  │
│  PTY (4KB buffer)                                │
│    ↓                                             │
│  ANSI Parser → Vec<TerminalAction>               │
│    ↓ serde_json::to_string()                     │
│  app.emit("terminal_actions", JSON)              │
│  app.emit("pty_output", raw_bytes)               │
│  Image processing (APC/DCS → image_event)        │
└──────────────────────┬──────────────────────────┘
                       │ JSON IPC + raw bytes
                       ↓
┌─────────────────────────────────────────────────┐
│ TypeScript Frontend                             │
│                                                  │
│  PtyClient.onTerminalActions()                   │
│    ↓ JSON.parse                                  │
│  state.processAction(action)                     │
│    ↓ switch(action.type)                         │
│  WASM handlers (handle_print, handle_csi, ...)   │
│                                                  │
│  PtyClient.onOutput() (unused)                   │
└─────────────────────────────────────────────────┘
```

**After (Sprint 6):**
```
┌─────────────────────────────────────────────────┐
│ Rust Backend (PTY + Image processing)           │
│                                                  │
│  PTY (4KB buffer)                                │
│    ↓ raw bytes                                   │
│  Channel.send(Vec<u8>)  [binary, no JSON]        │
│                                                  │
│  process_image_data() command (Kitty/SIXEL)      │
│    → image_event emission                        │
└──────────────────────┬──────────────────────────┘
                       │ Binary Channel
                       ↓
┌─────────────────────────────────────────────────┐
│ WASM TerminalCore                               │
│                                                  │
│  process_pty_data(&[u8])                         │
│    ↓                                             │
│  ANSI Parser (state machine)                     │
│    ↓                                             │
│  Internal dispatch:                              │
│    Print → handle_print()                        │
│    C0    → handle_execute()                      │
│    CSI   → handle_csi_internal()                 │
│    ESC   → handle_esc_internal()                 │
│    OSC   → on_osc callback → JS                  │
│    APC   → on_apc callback → JS → invoke()       │
│    DCS   → on_dcs callback → JS → invoke()       │
│    BEL   → on_bell callback → JS                 │
│    DSR   → on_device_response callback → JS      │
│                                                  │
│  Ring Buffer (scrollback + viewport)             │
│  Cursor, Modes, TabStops                         │
└──────────────────────┬──────────────────────────┘
                       │ Callbacks (OSC/APC/DCS only)
                       ↓
┌─────────────────────────────────────────────────┐
│ TypeScript (Rendering + DOM + UI)               │
│                                                  │
│  OSC callbacks → title, working dir, etc.        │
│  APC/DCS callbacks → invoke() for image decode   │
│  BEL callback → bell action                      │
│  Device response → PTY write                     │
│  Canvas Renderer (dirty cells from WASM)         │
│  Selection, Search, UI controls                  │
└─────────────────────────────────────────────────┘
```

### Data Flow

**Normal text output (most common path):**
```
PTY read(4KB) → Backend channel.send(bytes) → Channel → Frontend
  → core.process_pty_data(bytes)
  → [WASM internal: parse → handle_print × N, handle_execute × M]
  → return (0 WASM-TS boundary crossings beyond the initial call)
  → renderer.scheduleRender()
```

**OSC title change:**
```
PTY "\x1b]2;My Title\x07" → Backend → Channel → Frontend
  → core.process_pty_data(bytes)
  → [WASM: parser detects OSC 2 → on_osc callback(SET_TITLE, "My Title")]
  → JS: document.title = "My Title"
```

**Kitty Graphics image:**
```
PTY "\x1b_G...base64data...\x1b\\" → Backend → Channel → Frontend
  → core.process_pty_data(bytes)
  → [WASM: parser detects APC → on_apc callback(kitty_payload)]
  → JS: cursor = { row: core.get_cursor_row(), col: core.get_cursor_col() }
  → JS: invoke("process_image_data", { protocol: "kitty", data, cursor })
  → Backend: ImageProcessor.process_kitty_command() → image_event
  → Frontend: handleImageEvent() → ImageViewer.show()
```

**Binary IPC setup:**
```
TerminalApp.init():
  → ptyClient.spawn({ channel, cols, rows })
  → Backend: pty_spawn(channel, ...) → start reader thread
  → Reader thread: loop { read(buf) → channel.send(buf) }
  → Frontend: channel.onmessage = (data) => core.process_pty_data(data)
```

### WASM API Changes

```rust
#[wasm_bindgen]
impl TerminalCore {
    // ── Main Entry Point (NEW) ────────────────────────────
    /// Process raw PTY bytes end-to-end.
    /// Parses ANSI sequences and dispatches to internal handlers.
    /// OSC/APC/DCS trigger registered callbacks.
    pub fn process_pty_data(&mut self, data: &[u8]);

    // ── Callback Registration (NEW) ──────────────────────
    /// Register OSC callback: fn(action_type: u8, data: &str)
    pub fn set_osc_callback(&mut self, callback: JsValue);

    /// Register APC callback: fn(data: &[u8])
    pub fn set_apc_callback(&mut self, callback: JsValue);

    /// Register DCS callback: fn(data: &[u8])
    pub fn set_dcs_callback(&mut self, callback: JsValue);

    /// Register BEL callback: fn()
    pub fn set_bell_callback(&mut self, callback: JsValue);

    /// Register device response callback: fn(data: &[u8])
    pub fn set_device_response_callback(&mut self, callback: JsValue);

    /// Clear all callbacks (for dispose)
    pub fn clear_callbacks(&mut self);

    // ── Existing APIs (MODIFIED) ─────────────────────────
    // handle_print(cp)   → now called internally by process_pty_data
    // handle_execute(b)  → now called internally, BEL uses callback instead of sentinel
    // handle_esc(code)   → now called internally
    // handle_sgr(params) → now called internally
    // handle_scroll_up() → now called internally
    // (etc. - all handlers become internal, not pub for external dispatch)

    // ── Existing APIs (UNCHANGED) ────────────────────────
    // get_row_packed(row), get_scrollback_row_packed(index)
    // get_cursor_row(), get_cursor_col()
    // resize_reflow(), resize_no_reflow()
    // get_scrollback_length(), get_scrollback_text()
    // getDirtyRows(), clearDirty(), markAllDirty()
    // reset()

    // ── REMOVED from public API ──────────────────────────
    // get_response_bytes()  → replaced by device_response callback
    // get_response_ptr()    → removed
    // get_response_len()    → removed
}
```

**Note on handler visibility:** The individual handlers (`handle_print`, `handle_execute`, `handle_csi_*`, `handle_esc`, `handle_sgr`) should remain `pub` on the Rust side (for unit testing) but are no longer the primary external API for TS. The TS code calls only `process_pty_data()`. However, keeping them `#[wasm_bindgen] pub` is acceptable for backward compatibility during transition and for edge cases where individual action dispatch might be needed.

### Internal Parser Structure

```rust
// wasm/src/parser.rs (moved from src-tauri/src/ansi/parser.rs)

/// Parser state machine states
enum ParserState {
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    OscString,
    ApcString,
    DcsEntry,
    DcsParam,
    DcsString,
}

/// Internal action type (not exported via wasm_bindgen)
pub(crate) enum ParsedAction {
    Print(char),
    Execute(u8),
    CsiDispatch { params: Vec<u16>, intermediates: Vec<u8>, final_byte: u8 },
    EscDispatch { intermediate: Option<u8>, final_byte: u8 },
    OscDispatch(String),
    ApcDispatch(Vec<u8>),
    DcsDispatch(Vec<u8>),
}

pub(crate) struct Parser {
    state: ParserState,
    // ... intermediate buffers
}

impl Parser {
    pub(crate) fn new() -> Self { ... }

    /// Parse bytes, calling the handler for each recognized action.
    pub(crate) fn parse<F>(&mut self, data: &[u8], mut handler: F)
    where
        F: FnMut(ParsedAction),
    { ... }
}
```

### CSI Internal Dispatch

```rust
// wasm/src/csi_dispatch.rs (NEW)

impl TerminalCore {
    /// Internal CSI dispatch - routes to existing handlers
    pub(crate) fn handle_csi_internal(
        &mut self,
        params: &[u16],
        intermediates: &[u8],
        final_byte: u8,
    ) {
        match (intermediates.first(), final_byte) {
            // Cursor movement
            (None, b'A') => self.handle_cursor_up(params.first().copied().unwrap_or(1)),
            (None, b'B') => self.handle_cursor_down(params.first().copied().unwrap_or(1)),
            (None, b'C') => self.handle_cursor_forward(params.first().copied().unwrap_or(1)),
            (None, b'D') => self.handle_cursor_back(params.first().copied().unwrap_or(1)),
            (None, b'E') => self.handle_cursor_next_line(params.first().copied().unwrap_or(1)),
            (None, b'F') => self.handle_cursor_previous_line(params.first().copied().unwrap_or(1)),
            (None, b'G') => self.handle_cursor_horizontal_absolute(params.first().copied().unwrap_or(1)),
            (None, b'H') | (None, b'f') => {
                let row = params.first().copied().unwrap_or(1);
                let col = params.get(1).copied().unwrap_or(1);
                self.handle_cursor_position(row, col);
            }
            (None, b'd') => self.handle_cursor_vertical_absolute(params.first().copied().unwrap_or(1)),

            // Screen erase
            (None, b'J') => { self.handle_erase_in_display(params.first().copied().unwrap_or(0) as u8); }
            (None, b'K') => self.handle_erase_in_line(params.first().copied().unwrap_or(0) as u8),
            (None, b'X') => self.handle_erase_characters(params.first().copied().unwrap_or(1)),

            // Edit
            (None, b'L') => self.handle_insert_lines(params.first().copied().unwrap_or(1)),
            (None, b'M') => self.handle_delete_lines(params.first().copied().unwrap_or(1)),
            (None, b'@') => self.handle_insert_characters(params.first().copied().unwrap_or(1)),
            (None, b'P') => self.handle_delete_characters(params.first().copied().unwrap_or(1)),

            // Scroll
            (None, b'S') => { self.handle_scroll_up(params.first().copied().unwrap_or(1)); }
            (None, b'T') => self.handle_scroll_down(params.first().copied().unwrap_or(1)),
            (None, b'r') => {
                let top = params.first().copied().unwrap_or(1);
                let bottom = params.get(1).copied().unwrap_or(0);
                self.handle_decstbm(top, bottom);
            }

            // SGR
            (None, b'm') => self.handle_sgr(params),

            // Modes
            (Some(b'?'), b'h') => { /* set mode */ for &p in params { self.handle_set_mode(p, true); } }
            (Some(b'?'), b'l') => { /* reset mode */ for &p in params { self.handle_set_mode(p, false); } }

            // Device
            (None, b'n') => {
                let ps = params.first().copied().unwrap_or(0) as u8;
                self.handle_device_status_report(ps);
            }
            (None, b'c') | (Some(b'?'), b'c') => { self.handle_primary_device_attributes(); }
            (Some(b'>'), b'c') => { self.handle_secondary_device_attributes(); }

            _ => { /* Unknown CSI - ignore */ }
        }
    }
}
```

### OSC Callback Protocol

```rust
/// OSC action type codes for callback
const OSC_SET_TITLE_AND_ICON: u8 = 0;
const OSC_SET_ICON_NAME: u8 = 1;
const OSC_SET_TITLE: u8 = 2;
const OSC_SET_COLOR_PALETTE: u8 = 4;
const OSC_SET_WORKING_DIRECTORY: u8 = 7;
const OSC_HYPERLINK: u8 = 8;
const OSC_SET_FOREGROUND_COLOR: u8 = 10;
const OSC_SET_BACKGROUND_COLOR: u8 = 11;
const OSC_EMTERM_EXTENSION: u8 = 100;
const OSC_SEMANTIC_PROMPT: u8 = 133;
const OSC_UNKNOWN: u8 = 255;
```

The `on_osc` callback receives `(action_type: u8, data: String)` where `data` is the OSC payload string. The JS handler parses it based on `action_type`.

### Backend Changes

```rust
// src-tauri/src/lib.rs

// REMOVED: TerminalActionsPayload struct
// REMOVED: ansi module import (parser moved to WASM)

use tauri::ipc::Channel;

#[tauri::command]
async fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyManager>,
    channel: Channel<Vec<u8>>,  // NEW: binary channel
    shell: Option<String>,
    args: Option<Vec<String>>,
    cols: u16,
    rows: u16,
) -> Result<SpawnResult, String> {
    // ... spawn session ...
    spawn_reader_thread(app, state.inner().clone(), session_id.clone(), channel);
    // ...
}

/// NEW: Process image data sent from WASM via JS
#[tauri::command]
async fn process_image_data(
    app: AppHandle,
    state: State<'_, PtyManager>,
    session_id: String,
    protocol: String,       // "kitty" or "sixel"
    data: Vec<u8>,          // raw protocol payload
    cursor_row: u32,
    cursor_col: u32,
) -> Result<(), String> {
    // Use existing ImageProcessor to decode and emit image_event
    // ...
}

/// Simplified reader thread - no parsing, just binary transfer
fn spawn_reader_thread(
    app: AppHandle,
    pty_manager: PtyManager,
    session_id: String,
    channel: Channel<Vec<u8>>,
) {
    std::thread::spawn(move || {
        // ... get reader ...
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    // Direct binary transfer - no parsing!
                    let _ = channel.send(buf[..n].to_vec());
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                Err(e) => {
                    let _ = app.emit("pty_error", PtyErrorPayload {
                        session_id: session_id.clone(),
                        message: e.to_string(),
                    });
                    break;
                }
            }
        }
        // ... handle exit ...
    });
}
```

### Frontend Changes

```typescript
// src/pty/client.ts - MODIFIED

export class PtyClient {
    private channel: Channel<number[]> | null = null;
    private dataCallback: ((data: Uint8Array) => void) | null = null;

    async spawn(options: PtySpawnOptions = {}): Promise<string> {
        // Create Tauri Channel for binary data
        this.channel = new Channel<number[]>();
        this.channel.onmessage = (data: number[]) => {
            if (this.dataCallback) {
                this.dataCallback(new Uint8Array(data));
            }
        };

        const result = await invoke<SpawnResult>("pty_spawn", {
            channel: this.channel,
            shell: options.shell,
            args: options.args,
            cols: options.cols ?? 80,
            rows: options.rows ?? 24,
        });

        this.sessionId = result.session_id;
        return this.sessionId;
    }

    /** Register callback for binary PTY data */
    onData(callback: (data: Uint8Array) => void): void {
        this.dataCallback = callback;
    }

    // REMOVED: onTerminalActions()
    // REMOVED: onOutput()
    // REMOVED: flushPendingTerminalActions()
    // REMOVED: pendingTerminalActions
}

// src/terminal-app/index.ts - MODIFIED

private async setupPtyHandlers(): Promise<void> {
    if (!this.ptyClient || !this.state) return;

    const core = this.state.getWasmCore();

    // Register WASM callbacks
    core.set_osc_callback((actionType: number, data: string) => {
        this.handleOscCallback(actionType, data);
    });

    core.set_apc_callback((data: Uint8Array) => {
        this.handleApcCallback(data);
    });

    core.set_dcs_callback((data: Uint8Array) => {
        this.handleDcsCallback(data);
    });

    core.set_bell_callback(() => {
        this.handleBell();
    });

    core.set_device_response_callback((data: Uint8Array) => {
        this.ptyClient?.write(data);
    });

    // Register binary data handler
    this.ptyClient.onData((data: Uint8Array) => {
        core.process_pty_data(data);

        // Schedule render
        this.renderer?.scheduleRender(this.state!);
        this.imeHandler?.updatePosition();
        this.outputActivityCallback?.();
    });

    // Handle exit
    await this.ptyClient.onExit(async (_code, _remaining) => {
        // ...
    });
}

private handleOscCallback(actionType: number, data: string): void {
    switch (actionType) {
        case 0: // SetTitleAndIcon
        case 2: // SetTitle
            this.updateWindowTitle(data);
            break;
        case 7: // SetWorkingDirectory
            if (this.state) this.state.workingDirectory = data;
            break;
        case 8: // Hyperlink
            // Parse params;uri format
            break;
        case 100: // EmtermExtension
            // Handle markdown etc.
            break;
        case 133: // SemanticPrompt
            // Handle semantic zones
            break;
        // ... other OSC types
    }
}

private async handleApcCallback(data: Uint8Array): Promise<void> {
    const core = this.state?.getWasmCore();
    if (!core) return;

    const cursorRow = core.get_cursor_row();
    const cursorCol = core.get_cursor_col();

    await invoke("process_image_data", {
        sessionId: this.ptyClient?.getSessionId(),
        protocol: "kitty",
        data: Array.from(data),
        cursorRow,
        cursorCol,
    });
}

private async handleDcsCallback(data: Uint8Array): Promise<void> {
    const core = this.state?.getWasmCore();
    if (!core) return;

    const cursorRow = core.get_cursor_row();
    const cursorCol = core.get_cursor_col();

    await invoke("process_image_data", {
        sessionId: this.ptyClient?.getSessionId(),
        protocol: "sixel",
        data: Array.from(data),
        cursorRow,
        cursorCol,
    });
}
```

### File Structure Changes

```
wasm/src/
├── lib.rs              # MODIFIED: add parser, csi_dispatch mods
├── parser.rs           # NEW: moved from src-tauri/src/ansi/parser.rs
├── parser_types.rs     # NEW: ParsedAction, internal types (from sequence.rs)
├── parser_params.rs    # NEW: moved from src-tauri/src/ansi/params.rs
├── parser_sgr.rs       # NEW: moved from src-tauri/src/ansi/sgr.rs
├── csi_dispatch.rs     # NEW: internal CSI routing
├── osc_handler.rs      # NEW: OSC parsing and callback dispatch
├── callbacks.rs        # NEW: callback storage and invocation
├── terminal_core.rs    # MODIFIED: add parser field, process_pty_data, callback fields
├── ring_buffer.rs      # UNCHANGED
├── esc_handler.rs      # MODIFIED: handle_esc becomes pub(crate), called internally
├── print_handler.rs    # MODIFIED: handle_print becomes pub(crate), called internally
├── c0_handler.rs       # MODIFIED: BEL uses callback instead of sentinel
├── csi_cursor.rs       # UNCHANGED (internal methods)
├── csi_screen.rs       # UNCHANGED
├── csi_edit.rs         # UNCHANGED
├── csi_scroll.rs       # UNCHANGED
├── csi_modes.rs        # MODIFIED: handle_set_mode callback for mode actions
├── csi_device.rs       # MODIFIED: response uses callback instead of buffer polling
├── sgr.rs              # UNCHANGED
├── cell.rs             # UNCHANGED
└── unicode.rs          # UNCHANGED

src-tauri/src/
├── lib.rs              # MODIFIED: Channel-based spawn, remove parser, add process_image_data
├── ansi/               # DELETED (moved to wasm/src/)
│   ├── mod.rs          # DELETED
│   ├── parser.rs       # → wasm/src/parser.rs
│   ├── sequence.rs     # → wasm/src/parser_types.rs
│   ├── params.rs       # → wasm/src/parser_params.rs
│   ├── sgr.rs          # → wasm/src/parser_sgr.rs
│   ├── apc.rs          # KEPT in backend (for image processing types)
│   └── dcs.rs          # KEPT in backend (for image processing types)
├── image/              # MODIFIED: new process_image_data command entry
└── (others)            # UNCHANGED

src/
├── pty/
│   ├── client.ts       # MODIFIED: Channel-based, remove onTerminalActions/onOutput
│   └── (others)        # UNCHANGED
├── terminal-app/
│   └── index.ts        # MODIFIED: WASM callback setup, binary data handling
├── terminal/
│   ├── state.ts        # MODIFIED: add dispose(), add getWasmCore()
│   ├── wasm/
│   │   └── terminal-core.ts  # MODIFIED: callback registration, dispose
│   └── (others)        # UNCHANGED
├── types/
│   ├── terminal.ts     # MODIFIED: remove TerminalActionsPayload
│   └── pty.ts          # MODIFIED: remove PtyOutputPayload
└── (others)            # UNCHANGED
```

### Dependencies

**No new crate dependencies** (parser code is moved, not added as external dep).

**No new npm dependencies.**

**Tauri Channel API** is already available in `@tauri-apps/api/core`.

### Mode Action Codes

With `process_pty_data()`, CSI mode handling needs to be reworked. Currently `handle_set_mode()` returns action codes (1=switchToAlt, etc.) that TS interprets. With internal dispatch, mode actions that require TS-side state changes (buffer switch, etc.) need a new mechanism.

**Approach:** Mode actions that need TS involvement are queued in an event buffer (similar to callbacks). After `process_pty_data()` returns, TS reads the mode action queue.

```rust
// Event buffer for mode actions
pub(crate) mode_actions: Vec<u8>,  // action codes: 1=switchToAlt, 2=saveAndSwitch, 3=switchToMain, etc.

// After process_pty_data, TS reads:
pub fn take_mode_actions(&mut self) -> Vec<u8>;
```

Alternatively, mode actions can be handled via a dedicated callback. The implementation plan will determine the optimal approach.

## Test Scenarios

### Unit Tests (Rust, `cargo test`)

#### Parser Migration
- [ ] Parser produces same actions as backend parser for all CSI sequences
- [ ] Parser produces same actions for all ESC sequences
- [ ] Parser produces same actions for all OSC sequences
- [ ] Parser handles streaming input (sequence split across chunks)
- [ ] Parser handles UTF-8 multi-byte characters correctly
- [ ] Parser handles malformed sequences gracefully
- [ ] Parser handles interleaved control sequences and text

#### process_pty_data Integration
- [ ] process_pty_data: plain text → correct cells in grid
- [ ] process_pty_data: CSI cursor movement → correct cursor position
- [ ] process_pty_data: CSI SGR → correct cell attributes
- [ ] process_pty_data: ESC sequence → correct state change
- [ ] process_pty_data: mixed content (text + CSI + ESC) → correct end state
- [ ] process_pty_data: streaming (split sequence across two calls) → correct result
- [ ] process_pty_data: large input (>4KB) → handles correctly

#### Callbacks
- [ ] OSC callback fires with correct action type and data
- [ ] APC callback fires with correct raw payload
- [ ] DCS callback fires with correct raw payload
- [ ] BEL callback fires on 0x07
- [ ] Device response callback fires with correct response bytes
- [ ] No callback = silent ignore (no panic)
- [ ] Callback registration/deregistration works correctly

#### CSI Internal Dispatch
- [ ] All cursor CSI commands route to correct handlers
- [ ] All screen erase CSI commands route correctly
- [ ] All edit CSI commands route correctly
- [ ] SGR parameters route to handle_sgr
- [ ] Mode set/reset route correctly
- [ ] Device queries route correctly
- [ ] Unknown CSI is silently ignored

### Integration Tests (TypeScript, `bun test`)

#### Binary IPC
- [ ] PtyClient.spawn creates Channel and receives binary data
- [ ] Binary data flows to WASM process_pty_data
- [ ] Terminal displays text correctly after binary IPC migration

#### Callback Integration
- [ ] OSC title change updates window title
- [ ] OSC working directory updates state
- [ ] BEL triggers bell action (visual/sound)
- [ ] Device response writes back to PTY

#### Image Pipeline
- [ ] Kitty Graphics image displays correctly via WASM→JS→Backend pipeline
- [ ] SIXEL image displays correctly via WASM→JS→Backend pipeline
- [ ] Cursor position is correct at image placement time

#### dispose()
- [ ] TerminalState.dispose() releases WASM resources
- [ ] No memory leaks after tab close (callback cleanup)

#### Regression
- [ ] All existing Sprint 1-5 tests pass
- [ ] All existing handler tests pass
- [ ] All existing buffer tests pass

### Edge Cases

- [ ] Empty PTY read (0 bytes) → no crash
- [ ] Very large PTY chunk (>64KB) → handles correctly
- [ ] Rapid successive process_pty_data calls → state consistent
- [ ] process_pty_data during resize → no corruption
- [ ] Callback throws exception → WASM state not corrupted
- [ ] dispose() called twice → no crash
- [ ] Channel closed before all data sent → graceful handling
- [ ] Malformed APC/DCS (incomplete sequence) → no crash

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| WASM not initialized | WasmGrid is null | Panic with clear error message (no fallback per design decision) |
| Channel send failure | Backend → Frontend transfer fails | Log warning, continue (data loss acceptable for transient errors) |
| Callback exception | JS callback throws | Catch in WASM callback wrapper, log error, continue processing |
| Malformed sequence | Parser encounters invalid bytes | Ignore/skip as per ANSI parser conventions |
| Image invoke failure | Backend image processing fails | Log error, continue (image not displayed) |
| dispose() on active session | Tab closed while data flowing | Clear callbacks first, then free WASM resources |

## Performance Optimization

### Performance Goals

- Normal text: 1 WASM-TS boundary crossing per PTY chunk (process_pty_data only)
- OSC/APC/DCS: +1 boundary crossing per occurrence (via callback)
- No JSON serialization/deserialization
- IPC transfer: raw bytes only (no overhead)

### Memory Budget (Sprint 6 additions)

| Component | Estimated Size |
|-----------|---------------|
| Parser state machine | ~4.0 KB |
| Parser parameter handling | ~1.5 KB |
| SGR parsing | ~2.0 KB |
| CSI dispatch | ~2.0 KB |
| OSC handler | ~2.0 KB |
| Callback infrastructure | ~1.0 KB |
| process_pty_data | ~0.5 KB |
| **Total additional code** | **~13 KB** |

WASM binary estimate: 58.9KB + 13KB = ~72KB (under 80KB limit)

## Success Criteria

- [ ] `process_pty_data()` handles all ANSI sequences correctly
- [ ] Binary IPC via Tauri Channel works (no JSON)
- [ ] OSC/APC/DCS callbacks fire correctly
- [ ] Image display (Kitty/SIXEL) works via WASM→JS→Backend pipeline
- [ ] `TerminalState.dispose()` properly cleans up resources
- [ ] `terminal_actions` event completely removed
- [ ] `pty_output` event completely removed
- [ ] Backend ANSI parser removed (src-tauri/src/ansi/ deleted except apc.rs/dcs.rs)
- [ ] WASM binary < 80KB
- [ ] All Rust tests pass
- [ ] All TypeScript tests pass
- [ ] `bun tauri dev` shows working terminal
- [ ] vttest basic tests unchanged
- [ ] vim/less/top work correctly
- [ ] Image display works (Kitty and SIXEL)
- [ ] Multi-tab: each tab has independent callbacks

## Implementation Phases

### Phase 1: Parser Migration
**Goals:** Move ANSI parser from backend to WASM crate
**Deliverables:**
- `parser.rs`, `parser_types.rs`, `parser_params.rs`, `parser_sgr.rs` in `wasm/src/`
- Parser adapted for WASM (remove serde, adapt types)
- All existing parser tests pass in WASM crate

### Phase 2: Internal Dispatch
**Goals:** Wire parser output to existing WASM handlers
**Deliverables:**
- `csi_dispatch.rs`: CSI routing table
- `osc_handler.rs`: OSC parsing
- `process_pty_data()` implementation
- Unit tests for end-to-end processing

### Phase 3: Callback Infrastructure
**Goals:** Implement closure callback mechanism
**Deliverables:**
- `callbacks.rs`: callback storage and invocation
- OSC, APC, DCS, BEL, device response callbacks
- BEL switches from sentinel to callback
- Device response switches from buffer polling to callback

### Phase 4: Binary IPC
**Goals:** Replace JSON IPC with Tauri Channel
**Deliverables:**
- Backend `pty_spawn` with Channel parameter
- Simplified reader thread (no parsing)
- `PtyClient` refactored for Channel
- `terminal_actions` and `pty_output` events removed

### Phase 5: Image Pipeline
**Goals:** Rebuild image processing flow
**Deliverables:**
- Backend `process_image_data` command
- JS APC/DCS callback handlers with invoke()
- Cursor position from WASM for image placement
- Image display verification

### Phase 6: Frontend Integration
**Goals:** Wire everything together in TerminalApp
**Deliverables:**
- `setupPtyHandlers()` refactored for callbacks + Channel
- OSC callback handlers (title, working dir, etc.)
- `TerminalState.dispose()` implementation
- Backend cleanup (remove ansi module, TerminalActionsPayload)

### Phase 7: Verification and Regression
**Goals:** Full testing and verification
**Deliverables:**
- All existing tests pass
- Parser cross-validation (WASM vs old backend parser)
- Binary size verification
- Performance benchmarking
- `bun tauri dev` smoke test
- vttest verification
- Image display verification
- Multi-tab verification

## References

- WASM roadmap: `tmp/wasm.md`
- Sprint 5 SPEC: `doc/tasks/wasm-esc-ring-buffer/SPEC.md`
- Current implementations:
  - `src-tauri/src/ansi/` — ANSI parser (Rust backend, to be moved)
  - `src-tauri/src/lib.rs` — PTY reader thread, terminal_actions emission
  - `src/pty/client.ts` — PtyClient (terminal_actions listener)
  - `src/terminal-app/index.ts` — TerminalApp (setupPtyHandlers)
  - `src/terminal/state.ts` — TerminalState (processAction)
  - `wasm/src/terminal_core.rs` — TerminalCore (WASM handlers)
  - `src-tauri/src/ansi/apc.rs` — Kitty Graphics types
  - `src-tauri/src/ansi/dcs.rs` — SIXEL types
