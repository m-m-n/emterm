# Implementation Plan: Mux Inband Protocol

## Overview

Replace the direct Unix socket + Tauri IPC bridge between GUI and mux daemon with APC (Application Program Command) escape sequences over the PTY stream. The `emterm mux` command becomes a long-running bridge process that translates between APC on stdin/stdout and MuxMessage frames on a Unix socket to the daemon.

## Objectives

- Enable mux communication over PTY stream using APC escape sequences, making SSH-transparent mux sessions possible
- Remove `bridge.rs` Tauri IPC commands and simplify GUI-side mux code to use PTY writes instead of Tauri invoke
- Remove `feature = "gui"` gate from mux module so CLI-only builds include daemon and bridge

## Prerequisites

### Development Environment

- Rust 1.85+, Bun, Tauri CLI
- Docker for test execution

### Dependencies

- `base64` crate (already in Cargo.toml) for APC payload encoding
- `bincode` crate must be available without `feature = "gui"` gate (currently optional)
- Existing WASM APC callback infrastructure (`set_apc_callback`, `fire_apc_callback`)

## Architecture Overview

### Technology Stack

- **Rust backend**: Bridge process (stdin/stdout APC), protocol helpers, feature gate changes
- **TypeScript frontend**: MuxClient rewrite (PTY APC instead of Tauri invoke)
- **Rust/WASM**: APC parser (existing, verify callback path)

### Design Approach

The bridge process (`emterm mux`) becomes a long-running stdin/stdout translator instead of exiting immediately after emitting an OSC. The GUI communicates with it via APC escape sequences written to the PTY, and reads APC messages from the PTY output stream. The daemon-side Unix socket protocol remains completely unchanged.

### Component Interaction

```
GUI ─── APC write to PTY stdin ──> Bridge stdin parser ─── MuxMessage ──> Daemon (Unix socket)
GUI <── APC in PTY output stream ── Bridge stdout writer <── MuxMessage ── Daemon (Unix socket)
```

Key principle: Normal keyboard input goes directly to PTY stdin (no APC wrapping). Only mux control messages (handshake, attach, resize, split, etc.) and routed PtyInput use APC encoding. The bridge's stdin parser distinguishes APC sequences from passthrough data.

## Implementation Phases

### Phase 1: Protocol Layer (APC Encode/Decode)

**Goal**: Add APC encode/decode helpers to protocol.rs and make `bincode` available to non-GUI builds.

**Files to Modify**:
- `src-tauri/Cargo.toml` - Move `bincode` from optional/gui to always-on dependency
- `src-tauri/src/mux/ipc/protocol.rs` - Add APC encode/decode helpers

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `to_apc(msg)` | Encode MuxMessage to APC string | Valid MuxMessage | Returns `ESC _ emterm-mux;<base64> ST` string |
| `from_apc(payload)` | Decode APC payload to MuxMessage | APC payload string (with `emterm-mux;` prefix); validates and strips prefix internally | Returns MuxMessage or error |
| `APC_PREFIX` constant | Identify emterm mux APC sequences | - | `"emterm-mux;"` literal |

**Processing Flow**:
1. Encode: MuxMessage -> to_frame_body() -> Base64 encode -> prepend prefix -> wrap in APC delimiters
2. Decode: Strip APC prefix -> Base64 decode -> from_frame_body() -> MuxMessage

**Implementation Steps**:
1. **Move bincode dependency** - Make bincode non-optional in Cargo.toml so bridge process can serialize/deserialize control messages
2. **Add APC constants** - Define APC introducer, string terminator, and mux prefix as module constants
3. **Implement encode helper** - Convert MuxMessage to APC-wrapped Base64 string
4. **Implement decode helper** - Parse APC payload string back to MuxMessage with validation
5. **Add unit tests** - Round-trip, invalid input, edge cases (empty payload, max size)

**Dependencies**: None (foundational)

**Testing Approach**:
- Unit: Round-trip encode/decode for all MessageType variants
- Unit: Reject invalid Base64, missing prefix, truncated frame body

**Acceptance Criteria**:
- [ ] to_apc produces correctly formatted APC string
- [ ] from_apc round-trips all MuxMessage types
- [ ] Invalid inputs return errors (not panics)

**Estimated Effort**: small

---

### Phase 2: Bridge Process (stdin/stdout APC Translation)

**Goal**: Transform `emterm mux` from an OSC-emitting command that exits immediately into a long-running bridge process that translates APC on stdin/stdout to MuxMessage on Unix socket.

**Files to Modify**:
- `src-tauri/src/mux/cli.rs` - Rewrite `execute_mux` and `execute_attach` as long-running bridge

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Bridge main loop | Orchestrate stdin reader, stdout writer, socket client | Daemon running, stdin/stdout available | Exits on stdin EOF or socket close |
| Stdin APC parser | Separate APC sequences from passthrough data | Raw stdin byte stream | APC -> decode to MuxMessage; other data -> PtyInput for active pane |
| Stdout APC writer | Encode daemon responses as APC to stdout | MuxMessage from daemon | APC-encoded data written to stdout |
| Socket client | Bidirectional MuxMessage communication with daemon | Daemon socket path | Connected Unix socket with frame codec |

**Processing Flow** (stdin parser):
1. Read bytes from stdin
   - If ESC _ detected -> start APC accumulation
   - If not inside APC -> buffer as passthrough data, send as PtyInput to daemon
2. Inside APC accumulation:
   - If ST (ESC \) detected -> check prefix
     - "emterm-mux;" prefix -> decode as MuxMessage, forward to daemon
     - Other prefix -> forward raw as PtyInput to daemon
   - Otherwise -> continue accumulating

**Processing Flow** (main loop):
1. Ensure daemon is running (existing logic)
2. Connect to daemon via Unix socket
3. Send Hello handshake, receive Welcome
4. Write Welcome as APC to stdout (GUI reads it)
5. Concurrently: stdin -> parse APC -> daemon socket; daemon socket -> APC encode -> stdout
6. On stdin EOF or socket error -> exit cleanly

**Implementation Steps**:
1. **Implement stdin APC parser** - State machine that separates APC sequences from passthrough bytes, handling partial reads across buffer boundaries
2. **Implement stdout APC writer** - Buffered writer that encodes MuxMessage to APC and flushes to stdout
3. **Implement bridge main loop** - Async event loop with concurrent stdin/socket reading using select/join
4. **Rewrite execute_mux** - Replace OSC emit + exit with long-running bridge process
5. **Rewrite execute_attach** - Same as execute_mux but with attach semantics
6. **Handle lifecycle** - Clean exit on stdin EOF, socket close, or signal; daemon survives bridge death

**Dependencies**: Phase 1 (APC encode/decode)

**Testing Approach**:
- Unit: Stdin parser correctly separates APC from passthrough data
- Unit: Stdin parser handles partial APC sequences across read boundaries
- Unit: Bridge exits cleanly on stdin EOF
- Integration: Bridge connects to daemon, completes handshake, forwards messages

**Acceptance Criteria**:
- [ ] Bridge process runs as long-running stdin/stdout translator
- [ ] APC messages on stdin are decoded and forwarded to daemon
- [ ] Passthrough data on stdin is forwarded as PtyInput
- [ ] Daemon responses are APC-encoded to stdout
- [ ] stdin EOF causes clean exit; daemon survives

**Estimated Effort**: large

---

### Phase 3: GUI Integration (Replace Tauri IPC with PTY APC)

**Goal**: Replace all Tauri invoke mux commands with PTY stdin APC writes and handle APC responses from the PTY output stream via the existing WASM parser APC callback.

**Files to Modify**:
- `src/terminal/mux/mux-client.ts` - Rewrite to use PTY APC instead of Tauri invoke
- `src/terminal-app/mux/mux-session.ts` - Update enterMuxMode to launch bridge as shell command and handle APC flow
- `src/terminal/handlers/apc_handlers.ts` - Add mux APC dispatch (detect "emterm-mux;" prefix)
- `src/terminal-app/osc-handler.ts` - Wire mux APC callback (or handle via existing APC callback path)

**Files to Delete**:
- `src-tauri/src/mux/bridge.rs` - Remove Tauri IPC bridge

**Files to Modify (backend)**:
- `src-tauri/src/mux/mod.rs` - Remove `bridge` module declaration
- `src-tauri/src/lib.rs` - Remove `feature = "gui"` gate from `mux` module
- `src-tauri/src/app.rs` - Remove bridge Tauri command registrations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| MuxClient (rewritten) | Send mux control messages as APC via PTY write | PTY session active with bridge process | APC messages written to PTY stdin |
| APC mux handler (TS) | Process incoming mux APC from PTY output | WASM APC callback registered | MuxMessage decoded and dispatched to MuxClient callbacks |
| enterMuxMode (modified) | Launch `emterm mux` as shell command instead of OSC-triggered daemon connect | PTY session active | Bridge process running, APC communication established |

**Processing Flow** (GUI -> daemon):
1. MuxClient creates MuxMessage
2. Encode to APC string using same format as Rust encode
3. Write APC bytes to PTY via PtyClient.write()
4. Bridge stdin receives APC, decodes, forwards to daemon

**Processing Flow** (daemon -> GUI):
1. Bridge receives MuxMessage from daemon
2. Encodes as APC to stdout
3. PTY stream delivers to GUI
4. WASM parser fires APC callback with payload bytes
5. TS APC handler checks "emterm-mux;" prefix
6. Base64 decode -> parse frame body -> dispatch to MuxClient callbacks

**Implementation Steps**:
1. **Add mux APC handler in TypeScript** - Detect "emterm-mux;" prefix in APC callback, Base64 decode, parse frame body, dispatch by message type. On decode failure (invalid Base64 or invalid frame body), console.warn log and discard the message
2. **Rewrite MuxClient** - Replace Tauri invoke calls with PTY stdin APC writes; receive responses via APC callback instead of Tauri events
3. **Update enterMuxMode** - Launch `emterm mux` as the shell command in PTY (or write the command to existing PTY), handle Welcome response via APC
4. **Delete bridge.rs and cleanup** - Remove bridge module, Tauri command registrations, MuxBridgeState
5. **Remove mux feature gate** - Move `pub mod mux` out of `#[cfg(feature = "gui")]` in lib.rs
6. **Update Cargo.toml feature flags** - Ensure mux dependencies (tokio, bincode, etc.) are available without gui feature

**Dependencies**: Phase 2 (bridge process must be functional)

**Testing Approach**:
- Unit: MuxClient APC encode matches expected format
- Unit: APC handler correctly parses incoming mux messages
- Integration: GUI sends APC, bridge forwards, daemon responds, GUI receives
- E2E (Docker): Existing mux E2E tests pass with new protocol

**Acceptance Criteria**:
- [ ] bridge.rs deleted
- [ ] All Tauri mux commands removed from app.rs
- [ ] MuxClient uses PTY APC writes instead of Tauri invoke
- [ ] GUI receives mux responses via WASM APC callback
- [ ] `mux` module compiles without `feature = "gui"`

**Estimated Effort**: large

---

### Phase 4: Testing and Polish

**Goal**: Verify E2E tests, ensure no regressions, clean up dead code.

**Files to Modify**:
- None (testing and verification only)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| E2E test validation | Verify existing mux tests pass | Phase 3 complete | All E2E tests green |

**Implementation Steps**:
1. **Verify existing E2E tests** - Run mux.e2e.js, mux-multi-session.e2e.js, mux-reattach.e2e.js
2. **Add new E2E test scenarios** - Inband protocol session start, detach/reattach, pane split
3. **Performance verification** - Compare typing latency and throughput
4. **Clean up dead code** - Remove any remaining references to old bridge pattern

**Dependencies**: Phase 3 (GUI integration complete)

**Testing Approach**:
- E2E (Docker): All existing mux E2E tests pass
- E2E (Docker): New inband protocol specific tests
- Manual: SSH mux session test (requires real SSH connection)
- Manual: Performance comparison (typing latency, large file cat)

**Acceptance Criteria**:
- [ ] All existing E2E tests pass
- [ ] No perceivable performance degradation
- [ ] SSH mux session works (manual verification)

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/
  Cargo.toml                          # bincode non-optional, feature gate changes
  src/
    lib.rs                            # Remove feature="gui" gate from mux module
    app.rs                            # Remove bridge Tauri command registrations
    mux/
      mod.rs                          # Remove bridge module declaration
      cli.rs                          # Bridge process: stdin/stdout APC translation (rewritten)
      bridge.rs                       # DELETED
      ipc/
        protocol.rs                   # Add to_apc() / from_apc() helpers
        codec.rs                      # Unchanged
        connection.rs                 # Unchanged
        handlers.rs                   # Unchanged
      daemon/                         # Unchanged
      session/                        # Unchanged

src/
  terminal/
    mux/
      mux-client.ts                   # Rewritten: PTY APC instead of Tauri invoke
    handlers/
      apc_handlers.ts                 # Add mux APC dispatch
  terminal-app/
    mux/
      mux-session.ts                  # Updated: launch bridge as shell command
    osc-handler.ts                    # Minor: wire mux APC callback if needed

wasm/src/                             # Unchanged (APC callback already works)
```

## Testing Strategy

- **Unit**: APC encode/decode round-trips, stdin parser state machine, invalid input handling (target 90%+ for protocol layer)
- **Integration**: Bridge-to-daemon handshake, message forwarding, lifecycle management
- **E2E (Docker)**: Existing mux tests (mux.e2e.js, mux-multi-session.e2e.js, mux-reattach.e2e.js), new inband-specific scenarios
- **Manual**: SSH mux sessions, performance comparison, legacy mode verification

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| base64 | 0.22 | APC payload Base64 encode/decode (already present) |
| bincode | 1.3 | MuxMessage serialization (make non-optional) |
| tokio | 1 | Async I/O for bridge process (make non-optional for mux) |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| APC sequences corrupted by SSH or terminal intermediaries | Low | High | Base64 encoding ensures printable-only payload; test over SSH early |
| Bridge stdin parser mishandles partial reads | Medium | High | Thorough unit tests with split-boundary scenarios |
| Performance regression from Base64 overhead | Low | Medium | Only control messages use APC; bulk PTY output stays native |
| Feature gate removal breaks CLI-only builds | Low | Medium | CI tests both gui and no-default-features builds |
| WASM APC callback timing issues | Low | Medium | APC callback already proven for Kitty Graphics; same path reused |

## Open Questions

(None - all requirements resolved during specification dialogue)

## Success Metrics

- [ ] All FR1-FR9 implemented and tested
- [ ] All existing E2E tests pass without regression
- [ ] Local mux session works via inband protocol
- [ ] SSH mux session works (manual verification)
- [ ] Detach/reattach preserves screen state
- [ ] bridge.rs deleted, Tauri mux commands removed
- [ ] CLI-only build includes mux (no feature gate)
- [ ] No perceivable typing latency increase
