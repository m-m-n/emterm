# Feature: Mux OSC Handshake

## Overview

Replace the `TERM_PROGRAM` environment variable check in `emterm mux` with an OSC-based live handshake. When `emterm mux` starts, it sends an OSC 777 query to the terminal. If the terminal is eMterm, it responds with an ACK via PTY input. This enables mux to work over SSH where environment variables are not forwarded.

## Objectives

- Enable `emterm mux` to work on remote servers accessed via SSH
- Replace static environment variable check with live terminal capability detection
- Maintain security by refusing to start mux in non-eMterm terminals

## User Stories

### US1: SSH Mux Usage
As a developer, I want to run `emterm mux` on a remote SSH server, so that I can use the terminal multiplexer over SSH connections.

**Acceptance Criteria:**
- [ ] `emterm mux` succeeds when run inside eMterm (local or via SSH)
- [ ] `emterm mux` fails with an error when run in a non-eMterm terminal

## Technical Requirements

### Functional Requirements

- **FR1: OSC Query** - `emterm mux` sends `ESC ] 777 ; emterm ; mux ; query ST` to stdout before starting the bridge
- **FR2: eMterm ACK Response** - eMterm recognizes the mux query OSC and writes `ESC ] 777 ; emterm ; mux ; ack ST` back to the PTY input stream
- **FR3: Stdin Response Reading** - `emterm mux` reads stdin with a timeout to receive the ACK response
- **FR4: Timeout Error** - If no ACK is received within the timeout period, `emterm mux` exits with an error message
- **FR5: Remove Environment Variable Check** - Remove `check_emterm_environment()` which checks `TERM_PROGRAM=emterm`
- **FR6: Nesting Check Preserved** - The existing `EMTERM_MUX=1` nesting check remains unchanged

### Non-Functional Requirements

- **NFR1 - Timeout:** Handshake timeout is 2 seconds (sufficient for SSH round-trip latency)
- **NFR2 - Terminal Safety:** The OSC query must not produce visible output or side effects in non-eMterm terminals. OSC 777 is ignored by terminals that don't support it.

## Implementation Approach

### Sequence Diagram

```
emterm mux (CLI)                    eMterm (Terminal Emulator)
     |                                        |
     |-- stdout: ESC]777;emterm;mux;queryST ->|
     |                                        | (recognize mux query)
     |<- PTY input: ESC]777;emterm;mux;ackST -|
     |                                        |
     | (ACK received, proceed with bridge)    |
```

### Protocol Details

**Query (CLI → Terminal via stdout):**
```
\x1b]777;emterm;mux;query\x07
```

**ACK (Terminal → CLI via PTY input):**
```
\x1b]777;emterm;mux;ack\x07
```

Both use BEL (`\x07`) as ST (String Terminator) for simplicity, consistent with existing OSC 777 usage in the project.

### Changes Required

**Rust (`src-tauri/src/mux/cli.rs`):**
- Remove `check_emterm_environment()` function
- Add `handshake_emterm()` function:
  1. Set stdin to raw mode (to read escape sequences)
  2. Write OSC query to stdout
  3. Read stdin with 2-second timeout, parse for ACK OSC
  4. Restore stdin mode
  5. Return Ok(()) on ACK, Err on timeout

**TypeScript (`src/terminal-app/osc-handler.ts`):**
- Add handler in `handleMuxOsc()` for `action === "query"`:
  - Write ACK sequence `\x1b]777;emterm;mux;ack\x07` to PTY input via `pty_write` Tauri command

### File Structure

```
src-tauri/src/mux/cli.rs          # Handshake logic (query + stdin read)
src/terminal-app/osc-handler.ts    # ACK response handler
```

## Test Scenarios

### Unit Tests
- [ ] `handshake_emterm()` returns error on timeout (no response)
- [ ] ACK parsing correctly identifies `ESC]777;emterm;mux;ack` in stdin data

### Integration Tests
- [ ] OSC handler routes mux query to ACK response writer

### Edge Cases
- [ ] Partial OSC response (data arrives in chunks)
- [ ] Non-eMterm terminal: OSC query produces no visible output, timeout triggers error
- [ ] SSH latency: 2-second timeout accommodates typical SSH round-trip

## Success Criteria

- [ ] `emterm mux` works when invoked over SSH from eMterm client
- [ ] `emterm mux` fails with clear error in non-eMterm terminals
- [ ] No regression in local (non-SSH) mux usage
- [ ] `emterm mux --daemon` and `emterm mux ls/kill` are unaffected (no handshake needed)

## References

- Existing OSC 777 extension: `wasm/src/osc_handler.rs` (line 88)
- Mux OSC handler: `src/terminal-app/osc-handler.ts` (line 205)
- Current environment check: `src-tauri/src/mux/cli.rs` (line 14)
