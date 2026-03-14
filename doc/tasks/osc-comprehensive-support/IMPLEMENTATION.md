# Implementation Plan: OSC Comprehensive Support

## Overview

Comprehensive implementation of OSC escape sequences for eMterm to achieve parity with major modern terminal emulators. Covers completing stub implementations (OSC 4/10/11), adding widely-used sequences (OSC 12/52/9/22/104/110-112), enhancing OSC 8 hyperlinks with per-cell storage, and supporting iTerm2 protocol extensions (OSC 1337;File, OSC 1337;SetUserVar).

## Objectives

- Achieve OSC feature parity with kitty, WezTerm, foot, and Alacritty
- Enable SSH-transparent clipboard operations via OSC 52
- Support dark/light mode auto-detection via color queries (OSC 10/11/12 `?`)
- Complete OSC 8 hyperlink support with proper per-cell storage and rendering
- Support iTerm2 inline image protocol for ecosystem compatibility

## Prerequisites

### Development Environment
- Rust toolchain with wasm32-unknown-unknown target
- Bun runtime
- Docker (for testing)

### Dependencies
- Existing WASM parser infrastructure (OSC parsing is generic, new numbers only need routing)
- Existing PTY write-back path (`pty_write` Tauri command)
- Existing image viewer infrastructure (for OSC 1337;File)
- Tauri `plugin-notification` (already included)
- Tauri clipboard API

## Architecture Overview

### Technology Stack
- **WASM (Rust)**: OSC number routing, Cell structure extension (hyperlink_id), color spec parsing
- **TypeScript (Frontend)**: OSC handler logic, color management, clipboard integration, notification, rendering
- **Rust (Backend)**: No changes needed (PTY write path and image processing already exist)

### Design Approach

The existing 3-layer architecture is preserved. New OSC numbers are routed through the existing WASM callback mechanism. All OSC handler logic lives in TypeScript since it requires Browser/Tauri API access (clipboard, notification, CSS cursor, PTY write-back via IPC).

Color spec parsing is implemented in WASM for reuse across OSC 4/10/11/12 and potential future use in SGR processing.

### action_type Mapping

WASM `osc_handler.rs` が OSC 番号を `action_type: u8` に変換して TS に渡す。

| OSC | action_type | Note |
|-----|-------------|------|
| 0   | 0           | existing |
| 1   | 1           | existing |
| 2   | 2           | existing |
| 4   | 4           | existing |
| 7   | 7           | existing |
| 8   | 8           | existing |
| 9   | 9           | new |
| 10  | 10          | existing |
| 11  | 11          | existing |
| 12  | 12          | new |
| 22  | 22          | new |
| 52  | 52          | new |
| 104 | 104         | new |
| 110 | 110         | new |
| 111 | 111         | new |
| 112 | 112         | new |
| 133 | 133         | existing |
| 777 | 100         | existing (remapped) |
| 1337| 101         | new (remapped, >255) |

### Component Interaction

```
PTY data → WASM Parser → osc_handler.rs (routing) → fire_osc_callback
    → pendingOscQueue → handleOscCallback (TS) → specific handler
        → PTY write-back (query responses)
        → Clipboard API (OSC 52)
        → Notification API (OSC 9)
        → CSS cursor property (OSC 22)
        → Canvas renderer (OSC 8 hover/underline)
        → Image viewer (OSC 1337;File)
```

ランタイムの OSC dispatch パスは `index.ts` の `handleOscCallback()` → 個別 `osc-*.ts` モジュール。`src/terminal/handlers/osc_handlers.ts` は既存機能のみで変更不要。

## Implementation Phases

### Phase 1: Color Infrastructure (OSC 4/10/11/12 + Reset)

**Goal**: Complete color set/query/reset foundation. After this phase, applications can detect dark/light mode via OSC 11 query and customize the palette via OSC 4.

**Files to Create**:
- `wasm/src/color_spec.rs` - Color specification parser (shared across all color OSCs)
- `src/terminal/osc-colors.ts` - Runtime palette overlay and color query/set/reset handlers

**Files to Modify**:
- `wasm/src/osc_handler.rs` - Add routing for OSC 12, 104, 110, 111, 112
- `wasm/src/lib.rs` - Register new module
- `src/terminal-app/index.ts` - Add cases for new OSC action types in handleOscCallback
- `src/terminal/colors.ts` - Export helper for building query response format
- `src/terminal/state.ts` - Add runtime palette overlay state and cursor color state

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Color spec parser (WASM) | Parse `rgb:r/g/b`, `#RGB`, `#RRGGBB`, `#RRRRGGGGBBBB` formats | Valid color spec string | Returns (r, g, b) as 8-bit values, or indicates query (`?`) |
| Runtime palette overlay | Maintain 256-entry nullable overlay array over theme palette | Theme palette loaded | OSC 4 set updates overlay; renderer uses overlay when present |
| Color query responder | Format and send `rgb:rrrr/gggg/bbbb` response via PTY write-back | Session ID available | Response written to PTY as raw bytes |
| OSC 10/11/12 handler | Set/query default fg, bg, cursor colors | Theme defaults known | Color applied to rendering; query returns current value |
| OSC 104/110/111/112 handler | Reset colors to theme-configured values | Overlay exists | Overlay entry cleared; rendering reverts to theme |
| OSC 10/11 chaining | Single sequence sets fg (10), bg (11), cursor (12) in order | Multiple params separated by `;` | Each param applied to successive OSC number |

**OSC 10/11 チェイニングの役割分担:**
- WASM: OSC 10 の data をそのまま1回の callback で TS に渡す（分割しない）
- TS (`osc-colors.ts`): セミコロンで分割し、index 0 = OSC N (自身), index 1 = OSC N+1, index 2 = OSC N+2 として順次処理

**Processing Flow**:
1. WASM parser emits OscDispatch with param and data
2. osc_handler.rs maps param to action_type, fires callback to TS
3. TS handleOscCallback dispatches to osc-colors handler
4. Handler parses color spec (or detects `?` query)
   - Set operation -> update overlay/state, notify renderer of dirty
   - Query operation -> format 16-bit response, write back to PTY
5. Reset operations clear overlay entries, restoring theme defaults

**Implementation Steps**:
1. **Color spec parser in WASM** - Parse all 4 color formats into (r, g, b); expose to TS via WASM binding; detect `?` query token
2. **WASM routing extension** - Add OSC 12, 104, 110, 111, 112 to osc_handler.rs dispatch table
3. **Runtime palette overlay** - Add nullable 256-entry array to TerminalState; integrate with renderer's palette lookup
4. **OSC 4 set/query handler** - Parse chained `c;spec` pairs; set overlay entries or respond to queries
5. **OSC 10/11/12 set/query with chaining** - Handle single and chained operations; integrate with default fg/bg/cursor color state
6. **OSC 104/110/111/112 reset handlers** - Clear overlay entries or default color overrides; revert to theme values

**Dependencies**: None (foundational phase)

**Testing Approach**:
- Unit: Color spec parsing for all formats, query response formatting, palette overlay operations
- Unit: Chaining behavior (OSC 10 with multiple params)
- Integration: Set color via OSC 4, query returns set value; set via OSC 10, reset via OSC 110, query returns theme default

**Acceptance Criteria**:
- [ ] `OSC 4;N;rgb:rr/gg/bb ST` sets palette entry N
- [ ] `OSC 4;N;? ST` responds with current color of entry N in 16-bit format
- [ ] `OSC 10;? ST` responds with current foreground color
- [ ] `OSC 11;? ST` responds with current background color (dark/light detection works)
- [ ] `OSC 12;? ST` responds with current cursor color
- [ ] `OSC 10;spec1;spec2 ST` sets foreground and background in one sequence
- [ ] `OSC 104 ST` resets all palette entries to theme defaults
- [ ] `OSC 110/111/112 ST` reset respective colors to theme defaults

**Estimated Effort**: medium

---

### Phase 2: Clipboard Operations (OSC 52)

**Goal**: Enable SSH-transparent clipboard read/write. After this phase, applications like vim and tmux can access the system clipboard through the terminal.

**Files to Create**:
- `src/terminal/osc-clipboard.ts` - Clipboard read/write handler with security controls

**Files to Modify**:
- `wasm/src/osc_handler.rs` - Add routing for OSC 52
- `src/terminal-app/index.ts` - Add OSC 52 case in handleOscCallback
- `src/settings/types.ts` - Add clipboard settings fields
- `src-tauri/src/settings.rs` - Add clipboard settings fields with defaults
- `src/settings/sections/terminal-behavior-section.ts` - Add clipboard setting UI controls

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OSC 52 write handler | Decode base64 payload, write to system clipboard | Payload within size limit | Text written to clipboard via Tauri API |
| OSC 52 read handler | Read clipboard, base64 encode, respond via PTY | Read permission enabled in settings | Response written to PTY |
| OSC 52 security gate | Check settings before allowing read | Settings loaded | Blocked read operations silently ignored |
| Clipboard settings | Persist read enable + size limit | Settings system functional | UI toggle and size limit config available |

**Processing Flow**:
1. WASM routes OSC 52 to TS callback with target + payload data
2. TS handler parses clipboard target (`c`, `p`, `cp`) and payload
   - Target resolution: `c` = system clipboard; `p`/`s` = primary selection on Linux (X11), fallback to system clipboard on Windows
   - Payload is `?` -> read operation (if permitted by setting)
   - Payload is base64 string -> write operation (if within size limit)
   - Payload is empty -> clear operation
3. Read: Tauri clipboard read -> base64 encode -> format response -> PTY write-back. If encoded response exceeds 1MB (`pty_write` payload limit), split into multiple `pty_write` calls
4. Write: base64 decode -> validate size -> Tauri clipboard write

**Implementation Steps**:
1. **WASM routing** - Add OSC 52 to dispatch table in osc_handler.rs
2. **Settings extension** - Add `clipboard_read_osc52`, `clipboard_max_size_osc52` fields
3. **Clipboard handler** - Implement read/write/clear with target parsing and security checks
4. **TS integration** - Wire up handleOscCallback case, connect to Tauri clipboard API
5. **Settings UI** - Add toggles and size limit field in terminal behavior section

**Dependencies**: Requires Phase 1 (PTY write-back pattern established)

**Testing Approach**:
- Unit: Base64 decode/encode, target parsing (`c`, `p`, `cp`), size limit validation
- Unit: Security gate (read disabled -> no response), size limit validation
- Integration: Write to clipboard via OSC 52, verify clipboard contents
- Integration: Read clipboard via OSC 52, verify PTY response

**Acceptance Criteria**:
- [ ] `OSC 52;c;base64data ST` writes decoded text to system clipboard
- [ ] `OSC 52;c;? ST` responds with base64-encoded clipboard contents
- [ ] `OSC 52;c; ST` clears the clipboard
- [ ] Oversized write data is rejected (configurable limit, default 10MB)
- [ ] Read queries are silently ignored when `clipboard_read_osc52` is disabled
- [ ] Settings UI provides toggle for read permission and size limit input

**Estimated Effort**: medium

---

### Phase 3: Hyperlinks (OSC 8) Per-Cell Storage

**Goal**: Store hyperlink associations in terminal cells with proper rendering. After this phase, `ls --hyperlink=auto` and other hyperlink-emitting tools display clickable underlined links.

**Files to Modify**:
- `wasm/src/cell.rs` - Add `hyperlink_id` field to Cell struct
- `wasm/src/terminal_core.rs` - Add hyperlink table and ID allocator (WASM owns state), expose accessors
- `wasm/src/osc_handler.rs` - Process OSC 8 inline: allocate ID in WASM table, set active hyperlink_id. Fire callback to TS for metadata mirroring only
- `wasm/src/ring_buffer.rs` - `pack_row_abs()` に hyperlink_id (2 bytes LE) 追加
- `src/terminal/wasm/terminal-core.ts` - Read hyperlink_id from WASM cells; expose WASM hyperlink table query API
- `src/terminal/attributes.ts` - Add `hyperlinkId` to CellAttributes
- `src/terminal/canvas-renderer.ts` - Integrate hyperlink hover detection and underline drawing
- `src/terminal/state.ts` - Mirror hyperlink table from WASM (read-only cache for TS rendering)
- `src/terminal-app/index.ts` - Wire OSC 8 callback for metadata mirroring (URI/params sync from WASM)
- `src/terminal-app/handlers/link.ts` - OSC 8 click handling integration (lookup via WASM table)
- `src/terminal/url-detector.ts` - OSC 8 links take priority over auto-detected URLs

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Cell hyperlink_id (WASM) | Store per-cell hyperlink reference as u16 | Cell struct extended | Each cell knows which hyperlink it belongs to |
| Hyperlink table (WASM) | Own hyperlink ID allocation and {params, uri} storage in `terminal_core.rs` | TerminalCore initialized | IDs allocated synchronously during `process_pty_data`; primary/alternate cores each have independent tables |
| Hyperlink table mirror (TS) | Read-only cache of WASM table for rendering | WASM table populated | TS queries WASM for URI lookup during hover/click |
| Hyperlink renderer | Draw underline on hover for cells with hyperlink_id | Renderer has access to hyperlink table | Hovered links underlined; click opens URI |
| Priority system | OSC 8 links override auto-detected URLs | Both detection systems active | OSC 8 takes precedence in overlap regions |

**Processing Flow**:
1. `OSC 8;params;URI ST` received in WASM during `process_pty_data`
   -> `osc_handler.rs` allocates ID in WASM hyperlink table, sets as active hyperlink_id
   -> Fires OSC callback to TS (queued) with params+URI for metadata mirroring
2. Subsequent printed characters (still within same `process_pty_data` call) get the active hyperlink_id written into their cell synchronously
3. `OSC 8;; ST` received -> WASM clears active hyperlink (new chars get hyperlink_id=0)
4. After `process_pty_data` returns, TS processes queued OSC 8 callback to update its mirror table
5. Renderer checks cell hyperlink_id during hover detection
   - hyperlink_id > 0 -> query WASM table for URI (priority over auto-detect)
   - hyperlink_id == 0 -> fall through to URL auto-detection
6. Ctrl+click on hyperlinked cell -> query WASM table by cell's hyperlink_id; open URI

**Implementation Steps**:
1. **Cell struct extension** - Add `hyperlink_id: u16` to Cell in WASM; update size assertion; expose via getter. `underline_color[3]` (offset 29-31) の後、offset 32 に `hyperlink_id: u16` を配置。パディング不要、合計 34 bytes
2. **WASM hyperlink table** - Add `Vec<Option<HyperlinkEntry>>` + monotonic counter to `TerminalCore`. Primary and alternate cores each have independent tables and independent ID counters, so clearing/resetting one buffer does not invalidate hyperlink references in the other. Expose `get_hyperlink(id: u16) -> Option<(params, uri)>` API for TS. Table reset occurs on RIS / full clear of the owning buffer only
3. **OSC 8 inline processing in WASM** - `osc_handler.rs` parses `params;uri`, allocates ID in WASM table, sets `active_hyperlink_id`. On close (`;;`), clears active. Callback to TS carries params+URI for mirror sync
4. **TS mirror table** - TerminalState maintains read-only `Map<number, {params, uri}>` synced from queued OSC 8 callbacks. Used for hover tooltip text; actual URI lookup can also query WASM directly
5. **Renderer integration** - Modify hover detection to check hyperlink_id first; draw underline for hyperlinked cells
6. **Click handler** - Ctrl+click queries WASM hyperlink table by cell's hyperlink_id; opens URI
7. **Packed binary format extension** - `pack_row_abs()` に hyperlink_id (2 bytes LE) 出力追加、`parsePackedRow()` で読み取り、`CellAttributes` に反映。WASM と TS は同一ビルドで同時に更新されるため、ワイヤフォーマットのバージョニングは不要。ただし互換性契約として: (a) `parsePackedRow()` のセルあたりバイト数定数を更新し WASM 側と一致させる、(b) WASM の `pack_row_abs()` 出力と TS の `parsePackedRow()` 入力が一致することを検証するラウンドトリップテスト (TS-47) を必須とする、(c) セルフォーマット変更時は両側を同一コミットで更新する

**Dependencies**: None (can proceed in parallel with Phase 2)

**Testing Approach**:
- Unit: Cell hyperlink_id field read/write, hyperlink table allocation/deallocation
- Unit: Priority system (OSC 8 link overrides auto-detected URL at same position)
- Integration: Print text within OSC 8 markers, verify cells have correct hyperlink_id
- E2E (Docker): Verify underline visible on hover, Ctrl+click opens URL

**Acceptance Criteria**:
- [ ] Cells between `OSC 8;params;URI ST` and `OSC 8;; ST` have non-zero hyperlink_id
- [ ] Hyperlink table correctly maps IDs to URIs
- [ ] Hover over hyperlinked cells shows underline
- [ ] Ctrl+click opens the URI
- [ ] OSC 8 links take priority over auto-detected URLs at the same position
- [ ] Multi-line hyperlinks (soft-wrapped) maintain the same hyperlink_id

**Estimated Effort**: large

---

### Phase 4: Notifications and UI (OSC 9, OSC 22)

**Goal**: Desktop notifications and mouse cursor shape control. After this phase, build tools can display progress bars and applications can change the cursor shape.

**Files to Create**:
- `src/terminal/osc-notification.ts` - Notification and progress bar handler
- `src/terminal/osc-cursor-shape.ts` - Mouse cursor shape handler with push/pop stack

**Files to Modify**:
- `wasm/src/osc_handler.rs` - Add routing for OSC 9, 22
- `src/terminal-app/index.ts` - Add cases for OSC 9, 22 in handleOscCallback
- `src/terminal/state.ts` - Add progress state and cursor shape stack

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Notification handler | Display OS-native notification via Tauri plugin | Notification permission granted | Notification shown to user |
| Progress bar handler | Parse state/percentage, update tab/title indicator | Tab bar accessible | Progress displayed in tab or title bar |
| Cursor shape handler | Map cursor names to CSS cursor values | Terminal root element accessible | Mouse cursor changes within terminal area |
| Cursor push/pop stack | Maintain stack of cursor shapes (max depth 10) | Stack initialized | Push saves + sets; pop restores previous |

**Processing Flow**:
1. OSC 9 notification:
   - Parse message text -> Tauri notification API -> OS notification
2. OSC 9;4 progress:
   - Parse state (0-4) and percentage (0-100)
   - Emit event to tab bar / title bar for visual display
   - State 0 removes progress indicator
3. OSC 22 cursor shape:
   - No prefix -> set cursor directly
   - `>` prefix -> push current to stack, set new
   - `<` -> pop from stack and restore
   - Empty -> reset to default

**Implementation Steps**:
1. **WASM routing** - Add OSC 9 and 22 to dispatch table
2. **Notification handler** - Parse notification text; invoke Tauri notification plugin
3. **Progress bar handler** - Parse state/percentage; emit event for tab/title display
4. **Cursor shape handler** - Map valid cursor names to CSS values; apply to terminal root element
5. **Push/pop stack** - Maintain bounded stack; handle edge cases (pop empty, stack overflow)

**Dependencies**: Phase 1 (WASM routing pattern established)

**Testing Approach**:
- Unit: OSC 9 message parsing, progress state/percentage parsing
- Unit: Cursor name validation (valid names accepted, unknown ignored)
- Unit: Push/pop stack operations (push, pop, pop-empty, overflow at depth 10)
- Manual: Notification appears in OS notification center
- Manual: Progress bar displays in tab title

**Acceptance Criteria**:
- [ ] `OSC 9;message ST` triggers OS-native desktop notification
- [ ] `OSC 9;4;1;50 ST` shows 50% progress indicator
- [ ] `OSC 9;4;0;0 ST` removes progress indicator
- [ ] `OSC 22;pointer ST` changes mouse cursor to pointer
- [ ] `OSC 22;>text ST` pushes current cursor and sets to text
- [ ] `OSC 22;< ST` pops and restores previous cursor
- [ ] Unknown cursor names are silently ignored

**Estimated Effort**: medium

---

### Phase 5: iTerm2 Protocol (OSC 1337)

**Goal**: iTerm2 ecosystem compatibility. After this phase, iTerm2 imgcat and shell integration tools work with eMterm.

**Files to Create**:
- `src/terminal/osc-iterm2.ts` - iTerm2 protocol handlers (File and SetUserVar)

**Files to Modify**:
- `wasm/src/osc_handler.rs` - Add routing for OSC 1337
- `src/terminal-app/index.ts` - Add OSC 1337 case in handleOscCallback
- `src/terminal/state.ts` - Add user variables storage

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OSC 1337;File parser | Parse args (name, size, width, height, inline, preserveAspectRatio) and base64 data | Image viewer infrastructure available | Image displayed or download triggered |
| OSC 1337;SetUserVar handler | Parse key=base64value, decode, store per-session | TerminalState initialized | Variable stored in session map |
| User variable storage | Per-session key-value map | TerminalState has map field | Accessible for future shell integration |

**Processing Flow**:
1. OSC 1337 routing:
   - WASM fires callback with full data string (e.g., `File=name=...:base64data` or `SetUserVar=key=base64val`)
   - TS handler dispatches based on subcommand prefix
2. File command:
   - Parse key=value args before the `:` separator
   - Extract base64 image data after `:`
   - `inline=1` -> decode image, display using existing fullscreen image viewer
   - `inline=0` (or omitted) -> trigger download flow using existing download infrastructure
3. SetUserVar:
   - Parse `key=base64value` format
   - Decode base64 value
   - Store in per-session map on TerminalState

**Implementation Steps**:
1. **WASM routing** - Add OSC 1337 to dispatch table in osc_handler.rs
2. **Subcommand dispatcher** - Parse `File=...` vs `SetUserVar=...` prefix in TS
3. **File handler** - Parse args, extract base64 data, route to image viewer or download manager
4. **SetUserVar handler** - Parse, decode, store in TerminalState map
5. **TS integration** - Wire up handleOscCallback case for OSC 1337

**Dependencies**: Phase 1 (WASM routing pattern)

**Testing Approach**:
- Unit: File args parsing (all supported args, missing optional args)
- Unit: SetUserVar key/value parsing and base64 decoding
- Unit: Subcommand dispatch (File vs SetUserVar vs unknown)
- Integration: Display image via OSC 1337;File with inline=1
- Integration: Store and retrieve user variable

**Acceptance Criteria**:
- [ ] `OSC 1337;File=inline=1:base64data ST` displays image using existing viewer
- [ ] `OSC 1337;File=inline=0:base64data ST` triggers download
- [ ] `OSC 1337;File` args parsing handles name, size, width, height, preserveAspectRatio
- [ ] `OSC 1337;SetUserVar=key=base64value ST` stores decoded value in session
- [ ] Invalid base64 data is logged and does not crash
- [ ] Unknown OSC 1337 subcommands are silently ignored

**Estimated Effort**: medium

---

## Complete File Structure

```
wasm/src/
  color_spec.rs          (NEW)  Color specification parser shared by all color OSCs
  osc_handler.rs         (MOD)  Add routing for OSC 9, 12, 22, 52, 104, 110-112, 1337
  cell.rs                (MOD)  Add hyperlink_id field to Cell
  terminal_core.rs       (MOD)  Hyperlink table, cell hyperlink_id accessors
  ring_buffer.rs         (MOD)  pack_row_abs() packed format extension (hyperlink_id)
  lib.rs                 (MOD)  Register color_spec module

src/terminal/
  osc-colors.ts          (NEW)  Runtime palette overlay, color set/query/reset handlers
  osc-clipboard.ts       (NEW)  OSC 52 clipboard handler with security controls
  osc-notification.ts    (NEW)  OSC 9 notification and progress bar handler
  osc-cursor-shape.ts    (NEW)  OSC 22 mouse cursor shape with push/pop stack
  osc-iterm2.ts          (NEW)  OSC 1337;File and SetUserVar handlers
  colors.ts              (MOD)  Query response format helper
  attributes.ts          (MOD)  Add hyperlinkId to CellAttributes
  state.ts               (MOD)  Palette overlay, hyperlink table, user vars, cursor shape stack, progress state
  canvas-renderer.ts     (MOD)  Hyperlink hover detection and underline rendering
  url-detector.ts        (MOD)  OSC 8 priority over auto-detected URLs
  wasm/terminal-core.ts  (MOD)  Read hyperlink_id from WASM cells

src/terminal-app/
  index.ts               (MOD)  Add all new OSC cases in handleOscCallback
  handlers/link.ts       (MOD)  OSC 8 click handling integration

src/settings/
  types.ts               (MOD)  Add OSC 52 clipboard settings
  sections/terminal-behavior-section.ts (MOD) Add clipboard setting UI

src-tauri/src/
  settings.rs            (MOD)  Add clipboard settings fields with defaults

src/types/
  terminal.ts            (MOD)  Add new OscAction variants if needed
```

## Error Handling

All new OSC handlers follow the error handling contract defined in SPEC.md (Security Considerations > Error Handling Contract). Key principles:
- Invalid/malformed OSC data: silently ignored, debug-level log only
- `pty_write` failures: error logged, no retry, no crash
- Permission-dependent operations (clipboard read, notifications): fail gracefully per contract
- No new handler should throw exceptions that propagate to the caller

## Testing Strategy

- **Unit tests**: Color spec parsing (WASM + TS), clipboard base64 handling, hyperlink table operations, cursor shape stack, progress parsing, iterm2 args parsing. Target 80%+ coverage for new modules.
- **Integration tests**: Color set/query roundtrip, clipboard write/read cycle, hyperlink cell storage verification, OSC 10 chaining.
- **E2E (Docker)**: Existing tests pass without regression. OSC 8 hyperlink visual verification. OSC 52 clipboard verification.
- **Manual**: OS notification appearance (OSC 9), progress bar display, dark/light mode detection by real applications (neovim, tmux).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| Tauri clipboard API | (bundled) | OSC 52 clipboard read/write |
| Tauri plugin-notification | (already included) | OSC 9 desktop notifications |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Cell size increase (hyperlink_id) breaks memory assumptions | Low | High | Validate with WASM Cell size assertion test; benchmark memory impact |
| Clipboard read security concerns | Medium | Medium | Default to enabled (matching other terminals); configurable toggle; document risk in settings UI |
| Color query response timing | Low | Low | Responses use existing async PTY write path; no synchronous blocking |
| iTerm2 File with large images | Low | Medium | Reuse existing image size limits and LRU cache (320MB quota) |
| WASM cell struct padding changes across compilers | Low | High | Use `#[repr(C)]` and explicit size assertion in tests |

## Open Questions

- [ ] OSC 99 (kitty extended notification): Deferred to future work per SPEC.md

## Success Metrics

- [ ] All 11 functional requirements (FR1-FR11) implemented and passing tests
- [ ] No regression in existing E2E test suite
- [ ] `neovim` dark/light mode detection works via OSC 11 query
- [ ] `tmux` clipboard integration works via OSC 52
- [ ] `ls --hyperlink=auto` shows clickable links
- [ ] Color query responses match xterm format (16-bit `rgb:rrrr/gggg/bbbb`)
