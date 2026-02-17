# Implementation Plan: WASM ANSI Parser + Binary IPC (Sprint 6)

## Overview

This plan covers the migration of the ANSI parser from the Rust backend to WASM, replacement of JSON IPC with Tauri Channel binary transfer, implementation of closure callbacks, image pipeline rebuild, and `dispose()` method addition.

## Dependencies

- **No new crate dependencies** in `wasm/Cargo.toml`
- **No new npm dependencies**
- Tauri `Channel` API already available via `@tauri-apps/api/core`

## Phase 1: Parser Migration to WASM

**Goal:** Move the ANSI parser state machine from `src-tauri/src/ansi/` to `wasm/src/`, adapting it for the WASM environment.

### Tasks

#### 1.1 Create `wasm/src/parser_types.rs`

Internal action types for the WASM parser (not exported via `wasm_bindgen`).

**Source:** Derived from `src-tauri/src/ansi/sequence.rs`

**Changes from backend:**
- Remove `serde::Serialize` derives
- Remove `#[serde(tag = "...", content = "...")]` attributes
- Use `pub(crate)` visibility (internal to WASM crate)
- Remove `CsiAction`, `EscAction`, `OscAction` enums (replaced by `ParsedAction`)
- APC/DCS actions use raw `Vec<u8>` (no structured parsing in WASM)

```rust
/// Internal action types for parser output.
/// Not exported via wasm_bindgen - used only within the crate.

pub(crate) enum ParsedAction {
    Print(char),
    Execute(u8),
    CsiDispatch {
        params: Vec<u16>,
        intermediates: Vec<u8>,
        final_byte: u8,
    },
    EscDispatch {
        intermediate: Option<u8>,  // '(' or ')' for charset
        final_byte: u8,
    },
    OscDispatch {
        param: u16,
        data: String,
    },
    ApcDispatch(Vec<u8>),
    DcsDispatch(Vec<u8>),
}
```

#### 1.2 Create `wasm/src/parser_params.rs`

**Source:** Direct port from `src-tauri/src/ansi/params.rs`

**Changes from backend:**
- Remove `use std::` imports (use `alloc::vec::Vec` if needed, but standard lib available in WASM)
- Remove doc examples referencing `app_lib::`
- Keep all logic identical

#### 1.3 Create `wasm/src/parser_sgr.rs`

**Source:** Direct port from `src-tauri/src/ansi/sgr.rs`

**Changes from backend:**
- Remove `serde::Serialize` derives from `SgrAttr` and `Color`
- Remove `#[serde(tag = "...", content = "...")]` attributes
- Keep `parse_sgr()` function and all parsing logic identical
- Note: The existing `wasm/src/sgr.rs` already has `handle_sgr` which applies parsed SGR attrs to cells. `parser_sgr.rs` provides the parsing function that produces `SgrAttr` values. However, since `handle_sgr` in `wasm/src/sgr.rs` already duplicates the SGR parameter parsing inline, we need to decide: either (a) `parser_sgr.rs` exports `parse_sgr()` used by both parser tests and `handle_sgr`, or (b) keep the existing inline SGR in `sgr.rs` and use `parser_sgr.rs` only for parser-level tests. **Decision: Option (b)** - keep the existing `sgr.rs` handler as-is (it already works), and `parser_sgr.rs` is for the parser's CSI dispatch to call `handle_sgr(params)` which is already implemented. The `parser_sgr.rs` file is NOT needed - SGR parsing is already handled by `handle_sgr(params)` in `wasm/src/sgr.rs`.

**Revised decision:** `parser_sgr.rs` is **not created**. CSI `m` dispatches directly to `self.handle_sgr(params)`.

#### 1.4 Create `wasm/src/parser.rs`

**Source:** Port from `src-tauri/src/ansi/parser.rs`

**Changes from backend:**
- Use `ParsedAction` from `parser_types.rs` instead of `TerminalAction`
- Use `ParamParser` from `parser_params.rs` instead of `crate::ansi::params::ParamParser`
- Remove APC structured parsing (`apc::parse_kitty_command`) - just pass raw bytes
- Remove DCS structured parsing (`dcs::parse_sixel_sequence`) - just pass raw bytes
- Remove `use crate::ansi::*` imports
- Remove OSC structured parsing into `OscAction` variants - pass `(param: u16, data: String)` directly
- CSI dispatch: emit `ParsedAction::CsiDispatch` with raw params/intermediates/final_byte
- ESC dispatch: emit `ParsedAction::EscDispatch` with intermediate + final_byte
- Keep UTF-8 handling, state machine, all parsing logic identical
- Keep `MAX_OSC_LEN` constant
- APC/DCS: define `MAX_APC_LEN` and `MAX_DCS_LEN` locally (or import from shared constants)

**Key structural difference:** The backend parser creates structured `CsiAction::CursorUp(n)` etc. The WASM parser emits raw `CsiDispatch { params, intermediates, final_byte }` and routing happens in `csi_dispatch.rs`.

**State machine states:** Same as backend (`Ground`, `Escape`, `EscapeCharset`, `CsiEntry`, `CsiParam`, `OscString`, `OscEscape`, `ApcString`, `ApcEscape`, `DcsString`, `DcsEscape`).

#### 1.5 Update `wasm/src/lib.rs`

Add module declarations:
```rust
mod parser;
mod parser_types;
mod parser_params;
mod csi_dispatch;
mod osc_handler;
mod callbacks;
```

#### 1.6 Parser Unit Tests

Port key tests from `src-tauri/src/ansi/parser.rs` `mod tests`:
- Printable ASCII, UTF-8 multibyte, C0 controls
- CSI sequences (cursor, SGR, erase, scroll, modes, device)
- ESC sequences (save/restore cursor, index, charset)
- OSC sequences (title, working directory, hyperlink, semantic prompt)
- APC/DCS (raw buffer capture)
- Streaming input (split sequences)
- Edge cases (malformed, interleaved)

Tests validate `ParsedAction` output, not `TerminalAction`.

**Files created:** `parser.rs`, `parser_types.rs`, `parser_params.rs`
**Files modified:** `lib.rs`
**Files NOT created:** `parser_sgr.rs` (not needed)

---

## Phase 2: Internal Dispatch

**Goal:** Wire the parser output to existing WASM handlers via internal dispatch functions. Implement `process_pty_data()`.

### Tasks

#### 2.1 Create `wasm/src/csi_dispatch.rs`

Routes `ParsedAction::CsiDispatch` to existing handler methods.

```rust
impl TerminalCore {
    pub(crate) fn handle_csi_internal(
        &mut self,
        params: &[u16],
        intermediates: &[u8],
        final_byte: u8,
    ) {
        match (intermediates.first(), final_byte) {
            // Cursor movement
            (None, b'A') => { self.handle_cursor_up(params.first().copied().unwrap_or(1)); }
            (None, b'B') => { self.handle_cursor_down(params.first().copied().unwrap_or(1)); }
            // ... (full routing table per SPEC.md)

            // SGR
            (None, b'm') => self.handle_sgr(params),

            // Modes
            (Some(b'?'), b'h') => { for &p in params { self.handle_set_mode(p, true); } }
            (Some(b'?'), b'l') => { for &p in params { self.handle_set_mode(p, false); } }

            // Device
            (None, b'n') => { self.handle_device_status_report(params.first().copied().unwrap_or(0) as u8); }
            (None, b'c') | (Some(b'?'), b'c') => { self.handle_primary_device_attributes(); }
            (Some(b'>'), b'c') => { self.handle_secondary_device_attributes(); }
            (Some(b'='), b'c') => { /* TertiaryDeviceAttributes - currently ignored */ }

            _ => { /* Unknown - ignore */ }
        }
    }
}
```

**Key change for modes:** Currently `handle_set_mode()` returns a `u8` action code. In internal dispatch, these action codes need to be collected. Add `mode_actions: Vec<u8>` field to `TerminalCore` and push non-zero action codes.

**Key change for device responses:** Currently `handle_device_status_report()` returns a `u8` length. In internal dispatch, the response is already in the response buffer. With callbacks (Phase 3), the response will be sent via callback.

#### 2.2 Create `wasm/src/osc_handler.rs`

Routes `ParsedAction::OscDispatch` to the OSC callback.

```rust
impl TerminalCore {
    pub(crate) fn handle_osc_internal(&mut self, param: u16, data: &str) {
        // Map OSC param to action_type code for callback
        let action_type = match param {
            0 => 0u8,    // SetTitleAndIcon
            1 => 1,      // SetIconName
            2 => 2,      // SetTitle
            4 => 4,      // SetColorPalette
            7 => 7,      // SetWorkingDirectory
            8 => 8,      // Hyperlink
            10 => 10,    // SetForegroundColor
            11 => 11,    // SetBackgroundColor
            133 => 133,  // SemanticPrompt
            777 => 100,  // EmtermExtension (mapped to 100)
            _ => 255,    // Unknown
        };

        // Fire OSC callback
        self.fire_osc_callback(action_type, data);
    }
}
```

#### 2.3 Implement `process_pty_data()` on `TerminalCore`

The main entry point. Lives in `terminal_core.rs` (added as a new method).

```rust
#[wasm_bindgen]
impl TerminalCore {
    pub fn process_pty_data(&mut self, data: &[u8]) {
        // Collect parsed actions and process them
        // Use a two-phase approach: parse into buffer, then dispatch
        // (can't borrow &mut self during parse callback)

        let mut actions = Vec::new();
        self.parser.parse(data, |action| {
            actions.push(action);
        });

        for action in actions {
            match action {
                ParsedAction::Print(ch) => {
                    self.handle_print(ch as u32);
                }
                ParsedAction::Execute(byte) => {
                    self.handle_execute_internal(byte);
                }
                ParsedAction::CsiDispatch { params, intermediates, final_byte } => {
                    self.handle_csi_internal(&params, &intermediates, final_byte);
                }
                ParsedAction::EscDispatch { intermediate, final_byte } => {
                    self.handle_esc_internal(intermediate, final_byte);
                }
                ParsedAction::OscDispatch { param, data } => {
                    self.handle_osc_internal(param, &data);
                }
                ParsedAction::ApcDispatch(payload) => {
                    self.fire_apc_callback(&payload);
                }
                ParsedAction::DcsDispatch(payload) => {
                    self.fire_dcs_callback(&payload);
                }
            }
        }
    }
}
```

**Note:** `handle_print` currently takes `u32` (codepoint). For `Print(char)`, pass `ch as u32`.

**Note:** `handle_execute_internal` is a new internal version that uses callback for BEL instead of sentinel return. See Phase 3.

#### 2.4 ESC Internal Dispatch

Add `handle_esc_internal()` method. The existing `handle_esc()` in `esc_handler.rs` takes a `code: u8` and handles simple ESC sequences. For charset selection (`ESC (` / `ESC )`), we need the intermediate byte.

```rust
impl TerminalCore {
    pub(crate) fn handle_esc_internal(
        &mut self,
        intermediate: Option<u8>,
        final_byte: u8,
    ) {
        match (intermediate, final_byte) {
            // Charset selection
            (Some(b'('), byte) => self.handle_set_g0_charset(byte),
            (Some(b')'), byte) => self.handle_set_g1_charset(byte),
            // Simple ESC sequences
            (None, byte) => { self.handle_esc(byte); }
            _ => { /* Unknown - ignore */ }
        }
    }
}
```

Note: The existing `handle_esc(code)` already handles `7`, `8`, `D`, `E`, `H`, `M`, `c`.

#### 2.5 Add `parser` Field to `TerminalCore`

```rust
pub struct TerminalCore {
    // ... existing fields ...
    pub(crate) parser: Parser,
    pub(crate) mode_actions: Vec<u8>,
}
```

Initialize in `new()`:
```rust
parser: Parser::new(),
mode_actions: Vec::new(),
```

#### 2.6 Mode Action Queue

After `process_pty_data()`, TS reads mode actions:

```rust
#[wasm_bindgen]
impl TerminalCore {
    pub fn take_mode_actions(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.mode_actions)
    }
}
```

**Encoding format:**
- Actions 1-5 (buffer switch, cursor save/restore): single byte `[action_code]`
- TS_FALLBACK set: 3 bytes `[0xFF, mode_lo, mode_hi]`
- TS_FALLBACK reset: 3 bytes `[0xFE, mode_lo, mode_hi]`

In `handle_csi_internal()` for modes:
```rust
(Some(b'?'), b'h') => {
    for &p in params {
        let action = self.handle_set_mode(p, true);
        if action != 0 {
            if action == MODE_ACTION_TS_FALLBACK {
                self.mode_actions.push(0xFF); // TS_FALLBACK set
                self.mode_actions.push((p & 0xFF) as u8);
                self.mode_actions.push(((p >> 8) & 0xFF) as u8);
            } else {
                self.mode_actions.push(action);
            }
        }
    }
}
(Some(b'?'), b'l') => {
    for &p in params {
        let action = self.handle_set_mode(p, false);
        if action != 0 {
            if action == MODE_ACTION_TS_FALLBACK {
                self.mode_actions.push(0xFE); // TS_FALLBACK reset
                self.mode_actions.push((p & 0xFF) as u8);
                self.mode_actions.push(((p >> 8) & 0xFF) as u8);
            } else {
                self.mode_actions.push(action);
            }
        }
    }
}
```

#### 2.7 Integration Tests

- `process_pty_data` with plain text → correct cells
- `process_pty_data` with CSI cursor movement → correct cursor position
- `process_pty_data` with CSI SGR → correct cell attributes
- `process_pty_data` with ESC sequences → correct state
- `process_pty_data` with mixed content → correct end state
- `process_pty_data` with streaming (split sequence) → correct result
- Mode action queue populated correctly

**Files created:** `csi_dispatch.rs`, `osc_handler.rs`
**Files modified:** `terminal_core.rs`, `lib.rs`, `csi_modes.rs` (mode action push)

---

## Phase 3: Callback Infrastructure

**Goal:** Implement closure callback mechanism for OSC/APC/DCS/BEL/device response.

### Tasks

#### 3.1 Create `wasm/src/callbacks.rs`

Callback storage and fire methods.

```rust
use js_sys::Function;
use wasm_bindgen::prelude::*;
use crate::terminal_core::TerminalCore;

#[wasm_bindgen]
impl TerminalCore {
    pub fn set_osc_callback(&mut self, callback: JsValue) {
        self.osc_callback = callback.dyn_into::<Function>().ok();
    }

    pub fn set_apc_callback(&mut self, callback: JsValue) {
        self.apc_callback = callback.dyn_into::<Function>().ok();
    }

    pub fn set_dcs_callback(&mut self, callback: JsValue) {
        self.dcs_callback = callback.dyn_into::<Function>().ok();
    }

    pub fn set_bell_callback(&mut self, callback: JsValue) {
        self.bell_callback = callback.dyn_into::<Function>().ok();
    }

    pub fn set_device_response_callback(&mut self, callback: JsValue) {
        self.device_response_callback = callback.dyn_into::<Function>().ok();
    }

    pub fn clear_callbacks(&mut self) {
        self.osc_callback = None;
        self.apc_callback = None;
        self.dcs_callback = None;
        self.bell_callback = None;
        self.device_response_callback = None;
    }
}

impl TerminalCore {
    pub(crate) fn fire_osc_callback(&self, action_type: u8, data: &str) {
        if let Some(ref cb) = self.osc_callback {
            if let Err(e) = cb.call2(
                &JsValue::NULL,
                &JsValue::from(action_type),
                &JsValue::from(data),
            ) {
                web_sys::console::warn_1(&e);
            }
        }
    }

    pub(crate) fn fire_apc_callback(&self, data: &[u8]) {
        if let Some(ref cb) = self.apc_callback {
            let array = js_sys::Uint8Array::from(data);
            if let Err(e) = cb.call1(&JsValue::NULL, &array) {
                web_sys::console::warn_1(&e);
            }
        }
    }

    pub(crate) fn fire_dcs_callback(&self, data: &[u8]) {
        if let Some(ref cb) = self.dcs_callback {
            let array = js_sys::Uint8Array::from(data);
            if let Err(e) = cb.call1(&JsValue::NULL, &array) {
                web_sys::console::warn_1(&e);
            }
        }
    }

    pub(crate) fn fire_bell_callback(&self) {
        if let Some(ref cb) = self.bell_callback {
            if let Err(e) = cb.call0(&JsValue::NULL) {
                web_sys::console::warn_1(&e);
            }
        }
    }

    pub(crate) fn fire_device_response_callback(&self) {
        if let Some(ref cb) = self.device_response_callback {
            let data = &self.response_buffer[..self.response_len as usize];
            let array = js_sys::Uint8Array::from(data);
            if let Err(e) = cb.call1(&JsValue::NULL, &array) {
                web_sys::console::warn_1(&e);
            }
        }
    }
}
```

#### 3.2 Add Callback Fields to `TerminalCore`

```rust
pub struct TerminalCore {
    // ... existing fields ...
    pub(crate) osc_callback: Option<js_sys::Function>,
    pub(crate) apc_callback: Option<js_sys::Function>,
    pub(crate) dcs_callback: Option<js_sys::Function>,
    pub(crate) bell_callback: Option<js_sys::Function>,
    pub(crate) device_response_callback: Option<js_sys::Function>,
}
```

Initialize all as `None` in `new()`.

**Note:** `js_sys::Function` requires `js_sys` dependency. Check `wasm/Cargo.toml` - `js-sys` should already be available through `wasm-bindgen`.

#### 3.3 Modify `handle_execute_internal()`

New internal version that uses BEL callback instead of sentinel:

```rust
impl TerminalCore {
    pub(crate) fn handle_execute_internal(&mut self, byte: u8) {
        match byte {
            0x07 => {
                // BEL: fire callback instead of returning sentinel
                self.fire_bell_callback();
            }
            _ => {
                // Delegate to existing handle_execute for other bytes
                self.handle_execute(byte);
            }
        }
    }
}
```

**Alternative:** Modify `handle_execute()` itself to use callback, but this changes the existing public API. Better to add internal version and keep `handle_execute()` backward compatible during transition.

#### 3.4 Modify Device Response to Use Callback

In `handle_csi_internal()`, after device response methods write to buffer:

```rust
// Device Status Report
(None, b'n') => {
    let ps = params.first().copied().unwrap_or(0) as u8;
    let len = self.handle_device_status_report(ps);
    if len > 0 {
        self.fire_device_response_callback();
    }
}
(None, b'c') | (Some(b'?'), b'c') => {
    let len = self.handle_primary_device_attributes();
    if len > 0 {
        self.fire_device_response_callback();
    }
}
(Some(b'>'), b'c') => {
    let len = self.handle_secondary_device_attributes();
    if len > 0 {
        self.fire_device_response_callback();
    }
}
```

#### 3.5 Add `js-sys` and `web-sys` Dependencies (if not already present)

Check `wasm/Cargo.toml` for `js-sys` and `web-sys`. If missing, add:
```toml
[dependencies]
js-sys = "0.3"
web-sys = { version = "0.3", features = ["console"] }
```

`web-sys` with `console` feature is needed for `web_sys::console::warn_1()` in callback error logging.

#### 3.6 Callback Tests

- Rust-side: test that `fire_*_callback()` with `None` callback does not panic
- Rust-side: test callback field set/clear
- (JS-side callback invocation tested in Phase 6 integration)

**Files created:** `callbacks.rs`
**Files modified:** `terminal_core.rs` (fields, new()), `csi_dispatch.rs` (device response callback), `lib.rs`

---

## Phase 4: Binary IPC (Tauri Channel)

**Goal:** Replace JSON IPC with Tauri Channel binary transfer.

### Tasks

#### 4.1 Modify `pty_spawn` Command in Backend

**File:** `src-tauri/src/lib.rs`

```rust
use tauri::ipc::Channel;

#[tauri::command]
async fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyManager>,
    channel: Channel<Vec<u8>>,  // NEW parameter
    shell: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<SpawnResult, String> {
    // ... existing session creation ...
    spawn_reader_thread(app, manager, session_id.clone(), channel);
    // ...
}
```

#### 4.2 Simplify `spawn_reader_thread()`

**File:** `src-tauri/src/lib.rs`

Remove:
- `Parser` creation and usage
- `TerminalAction` collection
- `image_processor` creation and usage (moved to `process_image_data` command)
- `cursor_row`/`cursor_col` tracking
- `app.emit("terminal_actions", ...)` calls
- `app.emit("pty_output", ...)` calls

Add:
- `channel: Channel<Vec<u8>>` parameter
- `channel.send(buf[..n].to_vec())` for binary transfer

```rust
fn spawn_reader_thread(
    app: AppHandle,
    manager: PtyManager,
    session_id: String,
    channel: Channel<Vec<u8>>,
) {
    // ... existing thread spawn ...
    // Reader loop simplified:
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = channel.send(buf[..n].to_vec());
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if process_exited.load(Ordering::SeqCst) {
                    // ... drain remaining data ...
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            Err(e) => {
                let _ = app.emit("pty_error", PtyErrorPayload { ... });
                break;
            }
        }
    }
    // ... exit handling unchanged ...
}
```

#### 4.3 Remove `TerminalActionsPayload` and `PtyOutputPayload`

**File:** `src-tauri/src/lib.rs`

Delete:
- `struct TerminalActionsPayload`
- `struct PtyOutputPayload`

#### 4.4 Refactor `PtyClient` for Channel

**File:** `src/pty/client.ts`

- Remove `onTerminalActions()` method
- Remove `onOutput()` method
- Remove `pendingTerminalActions` buffer
- Remove `flushPendingTerminalActions()` method
- Add `channel` field and `onData()` method
- Modify `spawn()` to create and pass Channel

```typescript
import { Channel } from "@tauri-apps/api/core";

export class PtyClient {
    private channel: Channel<number[]> | null = null;
    private dataCallback: ((data: Uint8Array) => void) | null = null;

    async spawn(options: PtySpawnOptions = {}): Promise<string> {
        this.channel = new Channel<number[]>();
        this.channel.onmessage = (data: number[]) => {
            if (this.dataCallback) {
                this.dataCallback(new Uint8Array(data));
            }
        };

        const result = await invoke<SpawnResult>("pty_spawn", {
            channel: this.channel,
            shell: options.shell,
            // ...
        });

        this.sessionId = result.session_id;
        return this.sessionId;
    }

    onData(callback: (data: Uint8Array) => void): void {
        this.dataCallback = callback;
    }

    // Keep: write(), resize(), kill(), onExit(), getSessionId()
}
```

#### 4.5 Remove Unused TypeScript Types

**File:** `src/types/terminal.ts`
- Remove `TerminalActionsPayload` type

**File:** `src/types/pty.ts`
- Remove `PtyOutputPayload` type (if exists)

#### 4.6 Verify `Channel` Import Works

Check that `@tauri-apps/api/core` exports `Channel`. Verify with existing Tauri version in `package.json`.

**Files modified:** `src-tauri/src/lib.rs`, `src/pty/client.ts`, `src/types/terminal.ts`

---

## Phase 5: Image Pipeline Rebuild

**Goal:** Create backend `process_image_data` command and wire APC/DCS callbacks to it.

### Tasks

#### 5.1 Create `process_image_data` Backend Command

**File:** `src-tauri/src/lib.rs`

```rust
#[tauri::command]
async fn process_image_data(
    app: AppHandle,
    state: State<'_, PtyManager>,
    session_id: String,
    protocol: String,
    data: Vec<u8>,
    cursor_row: u32,
    cursor_col: u32,
) -> Result<(), String> {
    let mut processor = image::ImageProcessor::new();

    match protocol.as_str() {
        "kitty" => {
            if let Some(cmd) = apc::parse_kitty_command(&data) {
                let events = processor.process_kitty_command(cmd, cursor_row as u16, cursor_col as u16);
                for event in events {
                    let payload = ImageEventPayload {
                        session_id: session_id.clone(),
                        event,
                    };
                    app.emit("image_event", payload).map_err(|e| e.to_string())?;
                }
            }
        }
        "sixel" => {
            if let Some(sixel) = dcs::parse_sixel_sequence(&data) {
                let events = processor.process_sixel(sixel, cursor_row as u16, cursor_col as u16);
                for event in events {
                    let payload = ImageEventPayload {
                        session_id: session_id.clone(),
                        event,
                    };
                    app.emit("image_event", payload).map_err(|e| e.to_string())?;
                }
            }
        }
        _ => {
            return Err(format!("Unknown image protocol: {}", protocol));
        }
    }

    Ok(())
}
```

Register in `.invoke_handler()`.

#### 5.2 Preserve APC/DCS Types in Backend

**File:** `src-tauri/src/ansi/apc.rs` and `src-tauri/src/ansi/dcs.rs`

These files are kept in the backend (not deleted) because `process_image_data` needs `parse_kitty_command()` and `parse_sixel_sequence()`.

Move them to `src-tauri/src/image/` or keep in `src-tauri/src/ansi/` but only retain `apc.rs` and `dcs.rs` (the rest of `ansi/` is deleted).

**Decision:** Keep `apc.rs` and `dcs.rs` under `src-tauri/src/ansi/` with a simplified `mod.rs` that only exports these two modules. This minimizes file moves.

```rust
// src-tauri/src/ansi/mod.rs (simplified)
pub mod apc;
pub mod dcs;

pub use apc::{ApcAction, KittyCommand, parse_kitty_command, /* ... */};
pub use dcs::{DcsAction, SixelData, parse_sixel_sequence, /* ... */};
```

#### 5.3 Wire APC/DCS Callbacks in Frontend

Handled in Phase 6 (Frontend Integration) as part of `setupPtyHandlers()` refactoring.

**Files modified:** `src-tauri/src/lib.rs`, `src-tauri/src/ansi/mod.rs`
**Files deleted:** `src-tauri/src/ansi/parser.rs`, `src-tauri/src/ansi/sequence.rs`, `src-tauri/src/ansi/params.rs`, `src-tauri/src/ansi/sgr.rs`

---

## Phase 6: Frontend Integration

**Goal:** Wire everything together in `TerminalApp`, add `dispose()`, clean up.

### Tasks

#### 6.1 Refactor `setupPtyHandlers()` in TerminalApp

**File:** `src/terminal-app/index.ts`

Replace the current `terminal_actions` event listener with:

```typescript
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
        this.state?.onBell?.();
    });

    core.set_device_response_callback((data: Uint8Array) => {
        this.ptyClient?.write(data);
    });

    // Register binary data handler
    this.ptyClient.onData((data: Uint8Array) => {
        core.process_pty_data(data);

        // Process mode actions (variable-length encoding)
        const modeActions = core.take_mode_actions();
        if (modeActions.length > 0) {
            let i = 0;
            while (i < modeActions.length) {
                const action = modeActions[i];
                if (action === 0xFF || action === 0xFE) {
                    // TS_FALLBACK: 3 bytes [marker, mode_lo, mode_hi]
                    const mode = modeActions[i + 1] | (modeActions[i + 2] << 8);
                    const isSet = action === 0xFF;
                    this.state?.setDecPrivateMode(mode, isSet);
                    i += 3;
                } else {
                    this.state?.handleModeAction(action);
                    i += 1;
                }
            }
        }

        // Schedule render
        this.renderer?.scheduleRender(this.state!);
        this.imeHandler?.updatePosition();
        this.outputActivityCallback?.();
    });

    // Handle exit (unchanged)
    await this.ptyClient.onExit(async (_code, _remaining) => { ... });
}
```

#### 6.2 Implement OSC Callback Handler

**File:** `src/terminal-app/index.ts`

```typescript
private handleOscCallback(actionType: number, data: string): void {
    switch (actionType) {
        case 0: // SetTitleAndIcon
        case 2: // SetTitle
            this.updateWindowTitle(data);
            break;
        case 1: // SetIconName
            if (this.state) this.state.iconName = data;
            break;
        case 4: // SetColorPalette
            // Parse index;color format
            break;
        case 7: // SetWorkingDirectory
            if (this.state) this.state.workingDirectory = data;
            break;
        case 8: // Hyperlink
            this.handleHyperlinkOsc(data);
            break;
        case 10: // SetForegroundColor
        case 11: // SetBackgroundColor
            // Color query/set
            break;
        case 100: // EmtermExtension (777)
            this.handleEmtermExtension(data);
            break;
        case 133: // SemanticPrompt
            this.handleSemanticPrompt(data);
            break;
    }
}
```

#### 6.3 Implement Image Callback Handlers

**File:** `src/terminal-app/index.ts`

```typescript
private async handleApcCallback(data: Uint8Array): Promise<void> {
    const core = this.state?.getWasmCore();
    if (!core) return;

    await invoke("process_image_data", {
        sessionId: this.ptyClient?.getSessionId(),
        protocol: "kitty",
        data: Array.from(data),
        cursorRow: core.get_cursor_row(),
        cursorCol: core.get_cursor_col(),
    });
}

private async handleDcsCallback(data: Uint8Array): Promise<void> {
    const core = this.state?.getWasmCore();
    if (!core) return;

    await invoke("process_image_data", {
        sessionId: this.ptyClient?.getSessionId(),
        protocol: "sixel",
        data: Array.from(data),
        cursorRow: core.get_cursor_row(),
        cursorCol: core.get_cursor_col(),
    });
}
```

#### 6.4 Add `getWasmCore()` to TerminalState

**File:** `src/terminal/state.ts`

```typescript
getWasmCore(): TerminalCore {
    const grid = this.getActiveWasmGrid();
    if (!grid) throw new Error("WASM not initialized");
    return grid.core;
}
```

#### 6.5 Add `handleModeAction()` to TerminalState

**File:** `src/terminal/state.ts`

Extract mode action handling from current `processAction` for `Csi` → `SetMode`/`ResetMode`:

```typescript
handleModeAction(actionCode: number): void {
    switch (actionCode) {
        case 1: // SWITCH_TO_ALT
            this.switchToAlternateBuffer();
            break;
        case 2: // SAVE_AND_SWITCH_TO_ALT
            this.saveCursor();
            this.switchToAlternateBuffer();
            break;
        case 3: // SWITCH_TO_MAIN
            this.switchToMainBuffer();
            break;
        case 4: // SAVE_CURSOR
            this.saveCursor();
            break;
        case 5: // RESTORE_CURSOR
            this.restoreCursor();
            break;
        // Note: TS_FALLBACK (0xFF/0xFE) is decoded in setupPtyHandlers
        // and calls setDecPrivateMode() directly, not handleModeAction()
    }
}
```

#### 6.6 Implement `TerminalState.dispose()`

**File:** `src/terminal/state.ts`

```typescript
dispose(): void {
    // Clear WASM callbacks
    const core = this.primaryWasmGrid?.core;
    if (core) {
        core.clear_callbacks();
    }

    // Free WASM grids
    this.primaryWasmGrid?.free();
    this.primaryWasmGrid = null;

    this.alternateWasmGrid?.free();
    this.alternateWasmGrid = null;
}
```

#### 6.7 Call `state.dispose()` from `TerminalApp.dispose()`

**File:** `src/terminal-app/index.ts`

Add `this.state?.dispose()` in the existing `dispose()` method.

#### 6.8 Remove `processAction()` Entry Point

The `processAction()` method in `TerminalState` is no longer the main entry point. It can be kept for backward compatibility (tests may use it) or removed. **Decision:** Keep it but mark as deprecated. The main path is now `core.process_pty_data()`.

#### 6.9 Update WasmGrid Wrapper

**File:** `src/terminal/wasm/terminal-core.ts`

Add wrapper methods for the new WASM APIs:
- `set_osc_callback`, `set_apc_callback`, etc.
- `process_pty_data`
- `clear_callbacks`
- `take_mode_actions`

#### 6.10 Remove Unused Imports and Types

- Remove `TerminalActionsPayload` from type imports
- Remove unused handler imports if `processAction` is deprecated
- Update `index.ts` imports

**Files modified:** `src/terminal-app/index.ts`, `src/terminal/state.ts`, `src/terminal/wasm/terminal-core.ts`, `src/pty/client.ts`

---

## Phase 7: Backend Cleanup and Verification

**Goal:** Delete moved files, verify everything works, run full test suite.

### Tasks

#### 7.1 Delete Backend Parser Files

Delete (already moved to WASM in Phase 1):
- `src-tauri/src/ansi/parser.rs`
- `src-tauri/src/ansi/sequence.rs`
- `src-tauri/src/ansi/params.rs`
- `src-tauri/src/ansi/sgr.rs`

Keep (needed for image processing):
- `src-tauri/src/ansi/apc.rs`
- `src-tauri/src/ansi/dcs.rs`
- `src-tauri/src/ansi/mod.rs` (simplified to only export apc/dcs)

#### 7.2 Update Backend `mod.rs`

Simplify `src-tauri/src/ansi/mod.rs`:
```rust
pub mod apc;
pub mod dcs;

pub use apc::{parse_kitty_command, KittyCommand, /* needed types */};
pub use dcs::{parse_sixel_sequence, SixelData, /* needed types */};
```

Remove re-exports of deleted types (`Parser`, `TerminalAction`, `CsiAction`, etc.).

#### 7.3 Fix Backend Compilation

After deleting parser files and removing types:
- Fix any remaining `use ansi::*` imports in `lib.rs`
- Ensure `process_image_data` command compiles
- Ensure all other commands (`pty_write`, `pty_resize`, `pty_kill`, etc.) still work

#### 7.4 Run Rust Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

- Backend tests should pass (image processing tests, etc.)
- WASM crate tests should pass (parser, integration)

#### 7.5 Run TypeScript Tests

```bash
bun test
bun run typecheck
```

- All existing TS tests should pass
- Type check should pass

#### 7.6 Build WASM and Check Size

```bash
cd wasm && wasm-pack build --target web --out-dir pkg
```

- Verify WASM binary < 80KB

#### 7.7 Smoke Test with `bun tauri dev`

Manual verification:
- Terminal starts and displays correctly
- Text input/output works
- Colors and attributes render correctly
- vim/less/top work correctly
- Scrollback works
- Image display works (Kitty/SIXEL)
- Window title changes
- Multiple tabs work independently
- Tab close doesn't leak resources

#### 7.8 vttest Verification

Run vttest basic tests and verify no regressions.

---

## File Change Summary

### New Files (WASM)
| File | Phase | Description |
|------|-------|-------------|
| `wasm/src/parser.rs` | 1 | ANSI parser state machine |
| `wasm/src/parser_types.rs` | 1 | Internal action types |
| `wasm/src/parser_params.rs` | 1 | CSI parameter parsing |
| `wasm/src/csi_dispatch.rs` | 2 | CSI routing table |
| `wasm/src/osc_handler.rs` | 2 | OSC callback dispatch |
| `wasm/src/callbacks.rs` | 3 | Callback storage and invocation |

### Modified Files (WASM)
| File | Phase | Changes |
|------|-------|---------|
| `wasm/src/lib.rs` | 1 | Add module declarations |
| `wasm/src/terminal_core.rs` | 2,3 | Add parser, callback fields; process_pty_data(); take_mode_actions() |
| `wasm/src/c0_handler.rs` | 3 | Add handle_execute_internal() with BEL callback |
| `wasm/src/csi_modes.rs` | 2 | Mode action push to queue |
| `wasm/src/esc_handler.rs` | 2 | Add handle_esc_internal() |

### Modified Files (Backend)
| File | Phase | Changes |
|------|-------|---------|
| `src-tauri/src/lib.rs` | 4,5 | Channel-based pty_spawn, simplified reader, process_image_data command, remove payloads |
| `src-tauri/src/ansi/mod.rs` | 7 | Simplified to only export apc/dcs |

### Deleted Files (Backend)
| File | Phase | Reason |
|------|-------|--------|
| `src-tauri/src/ansi/parser.rs` | 7 | Moved to WASM |
| `src-tauri/src/ansi/sequence.rs` | 7 | Replaced by parser_types.rs |
| `src-tauri/src/ansi/params.rs` | 7 | Moved to WASM |
| `src-tauri/src/ansi/sgr.rs` | 7 | Not needed (WASM sgr.rs handles SGR) |

### Modified Files (Frontend)
| File | Phase | Changes |
|------|-------|---------|
| `src/pty/client.ts` | 4 | Channel-based, remove onTerminalActions/onOutput |
| `src/terminal-app/index.ts` | 6 | Callback setup, binary data handling, dispose |
| `src/terminal/state.ts` | 6 | getWasmCore(), handleModeAction(), dispose() |
| `src/terminal/wasm/terminal-core.ts` | 6 | Wrapper methods for new WASM APIs |
| `src/types/terminal.ts` | 4 | Remove TerminalActionsPayload |

---

## Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Parser behavior differs between backend and WASM | Port parser tests; cross-validate with identical input |
| Tauri Channel API issues | Test binary transfer early in Phase 4; fallback: base64 encoding |
| Callback exception corrupts WASM state | Wrap all callback calls in try/catch at JS level |
| Mode actions lost during transition | Comprehensive mode action queue tests |
| Image round-trip latency | Images are already async; latency acceptable |
| WASM binary size exceeds 80KB | Monitor after each phase; optimize if needed |

## Build Commands

```bash
# WASM build
cd wasm && wasm-pack build --target web --out-dir pkg

# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# TypeScript tests
bun test

# Type check
bun run typecheck

# Full dev build
bun tauri dev
```
