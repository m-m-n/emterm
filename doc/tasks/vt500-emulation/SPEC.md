# SPEC: VT500 Emulation Level Migration

## Overview

Migrate eMterm's terminal emulation level from VT420 to VT500. Update DA1 (Primary Device Attributes) and DA2 (Secondary Device Attributes) responses and align capability flags with actual implementation.

## Current State

| Response | Current Value | Meaning |
|----------|--------------|---------|
| DA1 | `\x1b[?64;1;2;6;22c` | VT420 level 4, 132-col, printer, selective-erase, ANSI-color |
| DA2 | `\x1b[>41;1;0c` | VT420, firmware v1, ROM cartridge 0 |

## Target State

| Response | New Value | Meaning |
|----------|----------|---------|
| DA1 | `\x1b[?65;1;4;22c` | VT500 level 5, 132-col, Sixel, ANSI-color |
| DA2 | `\x1b[>65;1;0c` | VT500 series, firmware v1, ROM cartridge 0 |

## Functional Requirements

### FR1: DA1 Conformance Level

Change the first parameter of the DA1 response from `64` (VT420) to `65` (VT500).

### FR2: DA2 Terminal Type

Change the first parameter of the DA2 response from `41` (VT420) to `65` (VT500 series).

### FR3: DA1 Capability Flags

Update DA1 capability flags to reflect actual eMterm capabilities:

| Flag | Capability | Action | Reason |
|------|-----------|--------|--------|
| 1 | 132 columns (DECCOLM) | Keep | CSI ?3 h/l implemented |
| 2 | Printer port | Remove | Not implemented |
| 4 | Sixel graphics | Add | DCS parsing + backend rendering implemented |
| 6 | Selective erase (DECSED/DECSEL) | Remove | Not implemented |
| 22 | ANSI color | Keep | 256-color + 24-bit RGB implemented |

### FR4: Test Updates

Update existing unit tests to assert the new response values.

## Non-Functional Requirements

### NFR1: Backward Compatibility

- TERM environment variable (`xterm-256color`) unchanged
- Other responses (DSR, XTWINOPS) unchanged
- No regression in existing application behavior (shell, vim, tmux, less)

## Implementation

### File: `wasm/src/csi_device.rs`

**DA1** (line 27):
```rust
// Before
self.write_response(b"\x1b[?64;1;2;6;22c")
// After
self.write_response(b"\x1b[?65;1;4;22c")
```

**DA2** (line 33):
```rust
// Before
self.write_response(b"\x1b[>41;1;0c")
// After
self.write_response(b"\x1b[>65;1;0c")
```

**Tests**: Update `test_da1` and `test_da2` assertions to match new values.

## Out of Scope

- New VT500-specific feature implementation (DECRQM, DECRQSS, etc.)
- XTVERSION response
- Implementation of capabilities not already present
