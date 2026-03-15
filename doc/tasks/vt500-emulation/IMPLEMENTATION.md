# IMPLEMENTATION: VT500 Emulation Level Migration

## Overview

Single-phase implementation to update DA1/DA2 response values and align capability flags with eMterm's actual feature set.

## Phase 1: Update Device Attribute Responses

### Objective

Update DA1 and DA2 responses from VT420 to VT500 level and align capability flags.

### Files to Modify

| File | Changes |
|------|---------|
| `wasm/src/csi_device.rs` | Update DA1 and DA2 response byte strings |

### Tasks

#### Task 1.1: Update DA1 Response (FR1, FR3)

In `handle_primary_device_attributes()`:
- Change response from `\x1b[?64;1;2;6;22c` to `\x1b[?65;1;4;22c`
- Conformance level: 64 → 65
- Remove flags: 2 (printer), 6 (selective erase)
- Add flag: 4 (Sixel)
- Keep flags: 1 (132-col), 22 (ANSI color)

#### Task 1.2: Update DA2 Response (FR2)

In `handle_secondary_device_attributes()`:
- Change response from `\x1b[>41;1;0c` to `\x1b[>65;1;0c`
- Terminal type: 41 → 65

#### Task 1.3: Update Tests (FR4)

In the `tests` module:
- `test_da1`: Assert `b"\x1b[?65;1;4;22c"`
- `test_da2`: Assert `b"\x1b[>65;1;0c"`

### Verification

- All WASM tests pass (`cargo test`)
- TypeScript typecheck passes (`bun run typecheck`)
- Manual: vim search wrap (N) message persists in eMterm
- Manual: tmux, less, shell operate normally
