# Verification Plan: WASM ANSI Parser + Binary IPC (Sprint 6)

## Overview

This document defines the verification criteria for Sprint 6. Each section maps to specific functional and non-functional requirements from SPEC.md.

---

## 1. Build Verification

### 1.1 WASM Build

```bash
cd wasm && wasm-pack build --target web --out-dir pkg
```

- [ ] Build succeeds without errors
- [ ] `wasm/pkg/emterm_wasm_bg.wasm` exists
- [ ] WASM binary size < 80KB (NFR4)

### 1.2 Rust Backend Build

```bash
cargo build --manifest-path src-tauri/Cargo.toml
```

- [ ] Build succeeds without errors
- [ ] No warnings about unused imports from deleted modules

### 1.3 TypeScript Build

```bash
bun run typecheck
```

- [ ] Type check passes with no errors
- [ ] No references to removed types (`TerminalActionsPayload`, `PtyOutputPayload`)

### 1.4 Full Application Build

```bash
bun tauri build
```

- [ ] Full application builds successfully

---

## 2. Unit Tests (Rust)

### 2.1 Parser Tests (`wasm/src/parser.rs`)

**FR2: Parser port accuracy**

- [ ] Printable ASCII produces `Print(char)` actions
- [ ] UTF-8 multibyte characters decoded correctly
- [ ] C0 control characters produce `Execute(byte)` actions
- [ ] CSI sequences produce `CsiDispatch` with correct params/intermediates/final_byte
- [ ] ESC sequences produce `EscDispatch` with correct intermediate/final_byte
- [ ] OSC sequences produce `OscDispatch` with correct param and data
- [ ] APC sequences produce `ApcDispatch` with raw payload
- [ ] DCS sequences produce `DcsDispatch` with raw payload

**FR3: Streaming input**

- [ ] Sequence split across two `parse()` calls → correct result
- [ ] UTF-8 character split across chunks → correct character
- [ ] CSI with params split across chunks → correct params
- [ ] OSC data split across chunks → correct data
- [ ] APC/DCS split across chunks → correct payload

**Edge cases:**

- [ ] Empty input → no actions
- [ ] Malformed UTF-8 → replacement character (U+FFFD)
- [ ] Malformed CSI (missing final byte, interrupted by ESC) → graceful recovery
- [ ] OSC exceeding MAX_OSC_LEN → truncated
- [ ] APC exceeding MAX_APC_LEN → truncated
- [ ] DEL character (0x7F) → ignored
- [ ] C0 control in middle of CSI → executed immediately

### 2.2 CSI Dispatch Tests (`wasm/src/csi_dispatch.rs`)

**FR4: CSI internal dispatch**

- [ ] `CSI A` → `handle_cursor_up()` with correct param
- [ ] `CSI B` → `handle_cursor_down()` with correct param
- [ ] `CSI C` → `handle_cursor_forward()` with correct param
- [ ] `CSI D` → `handle_cursor_back()` with correct param
- [ ] `CSI E` → `handle_cursor_next_line()` with correct param
- [ ] `CSI F` → `handle_cursor_previous_line()` with correct param
- [ ] `CSI G` → `handle_cursor_horizontal_absolute()` with correct param
- [ ] `CSI H` / `CSI f` → `handle_cursor_position()` with correct row, col
- [ ] `CSI d` → `handle_cursor_vertical_absolute()` with correct param
- [ ] `CSI J` → `handle_erase_in_display()` with correct mode
- [ ] `CSI K` → `handle_erase_in_line()` with correct mode
- [ ] `CSI X` → `handle_erase_characters()` with correct count
- [ ] `CSI L` → `handle_insert_lines()` with correct count
- [ ] `CSI M` → `handle_delete_lines()` with correct count
- [ ] `CSI @` → `handle_insert_characters()` with correct count
- [ ] `CSI P` → `handle_delete_characters()` with correct count
- [ ] `CSI S` → `handle_scroll_up()` with correct count
- [ ] `CSI T` → `handle_scroll_down()` with correct count
- [ ] `CSI r` → `handle_decstbm()` with correct top, bottom
- [ ] `CSI m` → `handle_sgr()` with correct params
- [ ] `CSI ? h` → `handle_set_mode(p, true)` for each param
- [ ] `CSI ? l` → `handle_set_mode(p, false)` for each param
- [ ] `CSI n` → `handle_device_status_report()` with correct ps
- [ ] `CSI c` / `CSI ? c` → `handle_primary_device_attributes()`
- [ ] `CSI > c` → `handle_secondary_device_attributes()`
- [ ] `CSI = c` → silently ignored (TertiaryDeviceAttributes placeholder)
- [ ] Default param values: missing params default correctly (1 for movement, 0 for erase)
- [ ] Unknown CSI → silently ignored

### 2.3 ESC Dispatch Tests

**FR5: ESC internal dispatch**

- [ ] `ESC 7` → handle_esc(b'7') (save cursor)
- [ ] `ESC 8` → handle_esc(b'8') (restore cursor)
- [ ] `ESC D` → handle_esc(b'D') (index)
- [ ] `ESC E` → handle_esc(b'E') (next line)
- [ ] `ESC H` → handle_esc(b'H') (tab set)
- [ ] `ESC M` → handle_esc(b'M') (reverse index)
- [ ] `ESC c` → handle_esc(b'c') (reset)
- [ ] `ESC ( B` → handle_set_g0_charset(b'B') (ASCII)
- [ ] `ESC ( 0` → handle_set_g0_charset(b'0') (DEC Line Drawing)
- [ ] `ESC ) B` → handle_set_g1_charset(b'B')

### 2.4 process_pty_data Integration Tests

**FR1: End-to-end processing**

- [ ] Plain text "Hello" → 5 cells with correct characters
- [ ] `"\x1b[31mRed\x1b[0m"` → cells with red foreground, then reset
- [ ] `"\x1b[2;5HX"` → 'X' at row 1, col 4 (0-indexed)
- [ ] `"Line1\r\nLine2"` → text on two lines
- [ ] Large input (>4KB) → processes correctly without error
- [ ] Mixed text + CSI + ESC + OSC → all handled correctly
- [ ] Streaming: split sequence across two process_pty_data calls → correct result

### 2.5 Callback Tests

**FR6-FR13: Callback mechanism**

- [ ] `set_osc_callback(fn)` → callback stored
- [ ] OSC sequence in process_pty_data → `on_osc` callback fired with correct (action_type, data)
- [ ] `set_apc_callback(fn)` → callback stored
- [ ] APC sequence in process_pty_data → `on_apc` callback fired with raw payload
- [ ] `set_dcs_callback(fn)` → callback stored
- [ ] DCS sequence in process_pty_data → `on_dcs` callback fired with raw payload
- [ ] `set_bell_callback(fn)` → callback stored
- [ ] BEL (0x07) in process_pty_data → `on_bell` callback fired
- [ ] `set_device_response_callback(fn)` → callback stored
- [ ] DSR query in process_pty_data → `on_device_response` callback fired with response bytes
- [ ] No callback set → action silently ignored (no panic)
- [ ] Callback that throws → error logged via console.warn, processing continues (no panic)
- [ ] `clear_callbacks()` → all callbacks set to None

**FR12: BEL sentinel removal**

- [ ] `handle_execute_internal(0x07)` fires bell callback (not returning 0xFE)
- [ ] `handle_execute(0x07)` still returns 0xFE (backward compat for existing pub API)

**FR13: Device response callback**

- [ ] DSR 6 (cursor position) → callback receives correct `\x1b[row;colR` bytes
- [ ] DA1 → callback receives `\x1b[?64;1;2;6;22c`

### 2.6 Mode Action Queue Tests

- [ ] `handle_set_mode(47, true)` → mode_actions contains `[1]` (SWITCH_TO_ALT, 1 byte)
- [ ] `handle_set_mode(1049, true)` → mode_actions contains `[2]` (SAVE_AND_SWITCH_TO_ALT, 1 byte)
- [ ] `handle_set_mode(1000, true)` → mode_actions contains `[0xFF, 0xE8, 0x03]` (TS_FALLBACK set, mode=1000)
- [ ] `handle_set_mode(1000, false)` → mode_actions contains `[0xFE, 0xE8, 0x03]` (TS_FALLBACK reset, mode=1000)
- [ ] `handle_set_mode(1, true)` → mode_actions contains `[0xFF, 0x01, 0x00]` (TS_FALLBACK set, mode=1/DECCKM)
- [ ] `take_mode_actions()` returns and clears the queue
- [ ] Boolean modes (7, 25, etc.) → no mode action queued
- [ ] Multiple mode sets in one process_pty_data → all actions queued in order with correct encoding

### 2.7 ParamParser Tests

- [ ] All existing `parser_params.rs` tests pass (ported from backend)

---

## 3. Unit Tests (TypeScript)

### 3.1 Existing Test Suite

**NFR5: All existing TypeScript tests pass**

```bash
bun test
```

- [ ] All existing tests pass without modification
- [ ] No test references removed types or APIs

### 3.2 New Tests

- [ ] `TerminalState.dispose()` frees WASM grids
- [ ] `TerminalState.dispose()` clears callbacks
- [ ] `TerminalState.dispose()` called twice → no error
- [ ] `TerminalState.getWasmCore()` returns core reference
- [ ] `TerminalState.handleModeAction()` handles all action codes

---

## 4. Integration Verification

### 4.1 Binary IPC (FR14-FR18)

- [ ] `pty_spawn` command accepts `channel` parameter
- [ ] PTY reader sends raw bytes via channel (no JSON)
- [ ] Frontend receives `Uint8Array` from channel
- [ ] `terminal_actions` event no longer emitted (FR16)
- [ ] `pty_output` event no longer emitted (FR17)
- [ ] PtyClient has no `onTerminalActions` method (FR18)
- [ ] PtyClient has no `onOutput` method (FR18)

### 4.2 Image Pipeline (FR19-FR23)

- [ ] Kitty Graphics image displays correctly (FR19, FR21)
- [ ] SIXEL image displays correctly (FR20, FR21)
- [ ] `process_image_data` command works for "kitty" protocol (FR22)
- [ ] `process_image_data` command works for "sixel" protocol (FR22)
- [ ] Cursor position correct at image placement time (FR21)
- [ ] `image_event` listener unchanged (FR23)

### 4.3 Frontend Integration (FR24-FR27)

- [ ] PtyClient.spawn creates Channel and passes to pty_spawn (FR24)
- [ ] setupPtyHandlers registers WASM callbacks (FR25)
- [ ] Binary data flows: Channel → process_pty_data (FR25)
- [ ] OSC title change updates window title
- [ ] OSC working directory updates state
- [ ] BEL triggers bell action
- [ ] Device response writes back to PTY
- [ ] processAction still available (backward compat) (FR27)

### 4.4 dispose() (FR28-FR31)

- [ ] `TerminalState.dispose()` calls WasmGrid.free() on primary grid (FR29)
- [ ] `TerminalState.dispose()` calls WasmGrid.free() on alternate grid if exists (FR29)
- [ ] `TerminalState.dispose()` clears callbacks (FR30)
- [ ] `TerminalApp.dispose()` calls state.dispose() (FR31)

### 4.5 Backend Cleanup (FR32-FR36)

- [ ] `src-tauri/src/ansi/parser.rs` deleted (FR32)
- [ ] `src-tauri/src/ansi/sequence.rs` deleted (FR32)
- [ ] `src-tauri/src/ansi/params.rs` deleted (FR32)
- [ ] `src-tauri/src/ansi/sgr.rs` deleted (FR32)
- [ ] `src-tauri/src/ansi/apc.rs` preserved (FR33)
- [ ] `src-tauri/src/ansi/dcs.rs` preserved (FR33)
- [ ] `TerminalActionsPayload` removed from backend (FR34)
- [ ] PTY reader thread does no parsing (FR35)
- [ ] No cursor tracking in PTY reader (FR36)

---

## 5. Performance Verification

### 5.1 NFR1: No JSON Serialization

- [ ] grep for `serde_json::to_string` in PTY reader → not found
- [ ] grep for `JSON.parse` in terminal data path → not found
- [ ] grep for `app.emit("terminal_actions"` → not found
- [ ] grep for `app.emit("pty_output"` → not found

### 5.2 NFR2: Minimal Boundary Crossings

- [ ] Normal text: 1 WASM-TS crossing per PTY chunk (process_pty_data only)
- [ ] OSC: +1 crossing per OSC sequence (callback)
- [ ] Pure text processing: no callbacks fired

### 5.3 NFR3: Throughput Improvement

Manual test:
```bash
# Generate large file
dd if=/dev/urandom bs=1M count=10 | base64 > /tmp/large_test.txt

# Time display
time cat /tmp/large_test.txt
```

- [ ] Subjective improvement in large output display speed

### 5.4 NFR4: WASM Binary Size

```bash
ls -la wasm/pkg/emterm_wasm_bg.wasm
```

- [ ] File size < 80KB (81920 bytes)

---

## 6. Compatibility Verification

### 6.1 NFR5: TypeScript Tests

```bash
bun test
```

- [ ] All tests pass

### 6.2 NFR6: vttest

Manual verification:
- [ ] vttest screen operations: no regression
- [ ] vttest cursor movement: no regression
- [ ] vttest character sets: no regression
- [ ] vttest scrolling: no regression

### 6.3 NFR7: Multi-Tab Safety

- [ ] Open 2+ tabs simultaneously
- [ ] Each tab operates independently
- [ ] Close one tab → other tabs unaffected
- [ ] Each tab has its own callback set (no cross-contamination)

### 6.4 NFR8: Image Display

- [ ] Kitty Graphics image displays correctly via `emterm image`
- [ ] SIXEL image displays correctly (if SIXEL test available)

### 6.5 NFR9: Resource Cleanup

- [ ] Tab close → dispose() called
- [ ] After dispose: no WASM memory leaks (callback references released)
- [ ] Multiple tab open/close cycles → no growing memory

---

## 7. Smoke Tests

### 7.1 Basic Terminal Operations

- [ ] `bun tauri dev` starts without errors
- [ ] Terminal displays shell prompt
- [ ] Typing input echoes correctly
- [ ] `ls --color` shows colored output
- [ ] `cat` with ANSI escape sequences renders correctly
- [ ] Tab completion works
- [ ] History (arrow keys) works

### 7.2 Application Features

- [ ] vim opens and displays correctly
- [ ] less paging works correctly
- [ ] top displays and updates correctly
- [ ] nano editor works
- [ ] SSH connections work
- [ ] `clear` command clears screen

### 7.3 Special Features

- [ ] Markdown display works (`emterm markdown`)
- [ ] Image display works (`emterm image`)
- [ ] Window title updates from shell
- [ ] Working directory tracking works
- [ ] Scrollback works (mouse wheel)
- [ ] Selection and copy works
- [ ] Search works

---

## 8. Regression Check Commands

```bash
# Rust tests (Docker)
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c \
  "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript tests (Docker)
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c \
  "bun test"

# Type check (Docker)
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c \
  "bun run typecheck"

# WASM build + size check
cd wasm && wasm-pack build --target web --out-dir pkg && \
  ls -la pkg/emterm_wasm_bg.wasm

# Verify no JSON serialization in data path
grep -r "terminal_actions\|pty_output" src-tauri/src/lib.rs | \
  grep -v "//\|REMOVED\|test"
```

---

## Requirement Traceability

| Requirement | Verification Section |
|-------------|---------------------|
| FR1 | 2.4 |
| FR2 | 2.1 |
| FR3 | 2.1 (streaming) |
| FR4 | 2.2 |
| FR5 | 2.3 |
| FR6-FR10 | 2.5 |
| FR11 | 2.5 |
| FR12 | 2.5 (BEL) |
| FR13 | 2.5 (device response) |
| FR14-FR18 | 4.1 |
| FR19-FR23 | 4.2 |
| FR24-FR27 | 4.3 |
| FR28-FR31 | 4.4 |
| FR32-FR36 | 4.5 |
| NFR1 | 5.1 |
| NFR2 | 5.2 |
| NFR3 | 5.3 |
| NFR4 | 5.4 |
| NFR5 | 6.1 |
| NFR6 | 6.2 |
| NFR7 | 6.3 |
| NFR8 | 6.4 |
| NFR9 | 6.5 |
