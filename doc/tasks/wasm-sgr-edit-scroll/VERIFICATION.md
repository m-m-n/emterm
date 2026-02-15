# Verification Document: WASM SGR + Edit/Scroll CSI + Modes + Device Response (Sprint 4)

## Overview
**Feature**: WASM SGR + Edit/Scroll CSI + Modes + Device Response
**SPEC.md**: `doc/tasks/wasm-sgr-edit-scroll/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wasm-sgr-edit-scroll/IMPLEMENTATION.md`

## Build Verification

### WASM Build
```bash
cd wasm && wasm-pack build --target web --out-dir pkg
```

### Rust Build
```bash
cargo test --manifest-path wasm/Cargo.toml --no-run
```

### TypeScript Build
```bash
bun run build
```

### TypeScript Type Check
```bash
bun run typecheck
```

### Expected Result
- All commands: exit code 0, no error messages

## Test Verification

### Rust Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path wasm/Cargo.toml"
```

### TypeScript Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90%

### Test Scenarios from SPEC.md

#### SGR Tests (Rust Unit)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-SGR-01 | handle_sgr: empty params | Reset (default attrs) | Unit |
| TS-SGR-02 | handle_sgr: param 0 | Reset | Unit |
| TS-SGR-03 | handle_sgr: param 1 | Bold flag set | Unit |
| TS-SGR-04 | handle_sgr: param 2 | Dim flag set | Unit |
| TS-SGR-05 | handle_sgr: param 3 | Italic flag set | Unit |
| TS-SGR-06 | handle_sgr: param 4 | Underline flag set | Unit |
| TS-SGR-07 | handle_sgr: param 5 | Blink flag set | Unit |
| TS-SGR-08 | handle_sgr: param 7 | Reverse flag set | Unit |
| TS-SGR-09 | handle_sgr: param 8 | Hidden flag set | Unit |
| TS-SGR-10 | handle_sgr: param 9 | Strikethrough flag set | Unit |
| TS-SGR-11 | handle_sgr: param 22 | NormalIntensity (bold=false, dim=false) | Unit |
| TS-SGR-12 | handle_sgr: params 23-29 | Not* resets | Unit |
| TS-SGR-13 | handle_sgr: params 30-37 | Standard foreground (indexed 0-7) | Unit |
| TS-SGR-14 | handle_sgr: params 40-47 | Standard background (indexed 0-7) | Unit |
| TS-SGR-15 | handle_sgr: params 90-97 | Bright foreground (indexed 8-15) | Unit |
| TS-SGR-16 | handle_sgr: params 100-107 | Bright background (indexed 8-15) | Unit |
| TS-SGR-17 | handle_sgr: [38, 5, 196] | Indexed foreground 196 | Unit |
| TS-SGR-18 | handle_sgr: [48, 5, 21] | Indexed background 21 | Unit |
| TS-SGR-19 | handle_sgr: [38, 2, 255, 128, 0] | RGB foreground | Unit |
| TS-SGR-20 | handle_sgr: [48, 2, 0, 128, 255] | RGB background | Unit |
| TS-SGR-21 | handle_sgr: param 39 | Default foreground | Unit |
| TS-SGR-22 | handle_sgr: param 49 | Default background | Unit |
| TS-SGR-23 | handle_sgr: [1, 31, 42] | Bold + Red FG + Green BG | Unit |
| TS-SGR-24 | handle_sgr: [38] with missing sub-params | No crash, ignore | Unit |
| TS-SGR-25 | handle_sgr: unknown param (99) | Ignored | Unit |
| TS-SGR-26 | handle_sgr: 20+ params | Handles without overflow | Unit |

#### Edit Tests (Rust Unit)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-EDIT-01 | IL: count=2 at row=5 in region (0,23) | Rows 5-21 shift to 7-23, rows 5-6 cleared | Unit |
| TS-EDIT-02 | IL: cursor outside scroll region | No-op | Unit |
| TS-EDIT-03 | IL: count exceeds region | Clamped to region height | Unit |
| TS-EDIT-04 | IL: dirty rows marked | All rows in region marked | Unit |
| TS-EDIT-05 | DL: count=2 at row=5 in region (0,23) | Rows 7-23 shift to 5-21, rows 22-23 cleared | Unit |
| TS-EDIT-06 | DL: cursor outside scroll region | No-op | Unit |
| TS-EDIT-07 | DL: count exceeds region | Clamped | Unit |
| TS-EDIT-08 | DL: dirty rows marked | All rows in region marked | Unit |
| TS-EDIT-09 | ICH: count=3 at col=5 | Cells 5-76 shift to 8-79, cells 5-7 cleared | Unit |
| TS-EDIT-10 | ICH: count exceeds remaining cols | Clamped | Unit |
| TS-EDIT-11 | ICH: dirty row marked | Row marked dirty | Unit |
| TS-EDIT-12 | DCH: count=3 at col=5 | Cells 8-79 shift to 5-76, cells 77-79 cleared | Unit |
| TS-EDIT-13 | DCH: count exceeds remaining cols | Clamped | Unit |
| TS-EDIT-14 | DCH: dirty row marked | Row marked dirty | Unit |

#### Scroll Tests (Rust Unit)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-SCR-01 | SU: count=2 with scroll region (5,20) | Rows shifted, returns 0 | Unit |
| TS-SCR-02 | SU: count=2 with full screen (0,23) | Returns 2 (TS scrollback) | Unit |
| TS-SCR-03 | SU: count exceeds region height | Clamped | Unit |
| TS-SCR-04 | SD: count=2 with scroll region (5,20) | Rows 5-18 shift to 7-20, rows 5-6 cleared | Unit |
| TS-SCR-05 | SD: count=2 with full screen | Rows shifted, top rows cleared | Unit |
| TS-SCR-06 | SD: count exceeds region height | Clamped | Unit |
| TS-SCR-07 | DECSTBM: top=5, bottom=20 | Region set (4,19) 0-indexed | Unit |
| TS-SCR-08 | DECSTBM: top=0, bottom=0 | Full screen | Unit |
| TS-SCR-09 | DECSTBM: cursor moved to (0,0) | Cursor at home | Unit |
| TS-SCR-10 | DECSTBM: wrapPending cleared | wrap_pending = false | Unit |
| TS-SCR-11 | DECSTBM: top > bottom (invalid) | Ignored or reset to full screen | Unit |

#### Mode Tests (Rust Unit)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-MOD-01 | mode=7 (DECAWM), enable=true | autoWrap set, returns 0 | Unit |
| TS-MOD-02 | mode=7 (DECAWM), enable=false | autoWrap cleared, returns 0 | Unit |
| TS-MOD-03 | mode=25 (DECTCEM), enable=true | cursorVisible set, returns 0 | Unit |
| TS-MOD-04 | mode=6 (DECOM), enable=true | originMode set, returns 0 | Unit |
| TS-MOD-05 | mode=47, enable=true | Returns 1 (switchToAlt) | Unit |
| TS-MOD-06 | mode=47, enable=false | Returns 3 (switchToMain) | Unit |
| TS-MOD-07 | mode=1049, enable=true | Returns 2 (saveAndSwitchToAlt) | Unit |
| TS-MOD-08 | mode=1049, enable=false | Returns 3 (switchToMain) | Unit |
| TS-MOD-09 | mode=1048, enable=true | Returns 4 (saveCursor) | Unit |
| TS-MOD-10 | mode=1048, enable=false | Returns 5 (restoreCursor) | Unit |
| TS-MOD-11 | mode=1000 (mouse) | Returns 0xFF (TS fallback) | Unit |
| TS-MOD-12 | mode=2004 (bracketed paste) | Returns 0xFF (TS fallback) | Unit |
| TS-MOD-13 | unknown mode | Returns 0 (no-op) | Unit |

#### Device Tests (Rust Unit)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-DEV-01 | DSR ps=5 | Response "ESC[0n" (4 bytes), returns 4 | Unit |
| TS-DEV-02 | DSR ps=6 at cursor (0,0) | Response "ESC[1;1R" (6 bytes) | Unit |
| TS-DEV-03 | DSR ps=6 at cursor (23,79) | Response "ESC[24;80R" | Unit |
| TS-DEV-04 | DSR ps=0 | Returns 0 (no response) | Unit |
| TS-DEV-05 | DA1 | Response "ESC[?64;1;2;6;22c" | Unit |
| TS-DEV-06 | DA2 | Response "ESC[>41;1;0c" | Unit |
| TS-DEV-07 | get_response_ptr/len | Valid pointer and length | Unit |

#### Integration Tests (TypeScript)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-INT-01 | SGR Reset via WASM | Cursor attrs cleared | Integration |
| TS-INT-02 | SGR Bold + Red FG via WASM | Correct flags and color | Integration |
| TS-INT-03 | SGR 256-color via WASM | Indexed color applied | Integration |
| TS-INT-04 | SGR TrueColor via WASM | RGB color applied | Integration |
| TS-INT-05 | SGR followed by Print | Printed cell has correct attrs | Integration |
| TS-INT-06 | InsertLines via WASM | Rows shifted correctly | Integration |
| TS-INT-07 | DeleteLines via WASM | Rows shifted correctly | Integration |
| TS-INT-08 | InsertCharacters via WASM | Cells shifted right | Integration |
| TS-INT-09 | DeleteCharacters via WASM | Cells shifted left | Integration |
| TS-INT-10 | ScrollUp in scroll region | Handled internally | Integration |
| TS-INT-11 | ScrollUp full screen | Triggers scrollback | Integration |
| TS-INT-12 | ScrollDown via WASM | Rows shifted correctly | Integration |
| TS-INT-13 | DECSTBM via WASM | Region set, cursor home | Integration |
| TS-INT-14 | SetMode DECAWM via WASM | Mode set, no action | Integration |
| TS-INT-15 | SetMode 1049 via WASM | Buffer switch triggered | Integration |
| TS-INT-16 | ResetMode 1049 via WASM | Switch to main triggered | Integration |
| TS-INT-17 | SetMode mouse (1000) via WASM | TS fallback handles it | Integration |
| TS-INT-18 | DSR CPR via WASM | Cursor position response sent | Integration |
| TS-INT-19 | DA1 via WASM | VT420 response sent | Integration |
| TS-INT-20 | DA2 via WASM | VT420 identification sent | Integration |

#### Regression Tests (TypeScript)

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-REG-01 | All existing SGR tests | Pass | Regression |
| TS-REG-02 | All existing edit tests | Pass | Regression |
| TS-REG-03 | All existing scroll tests | Pass | Regression |
| TS-REG-04 | All existing mode tests | Pass | Regression |
| TS-REG-05 | All existing device tests | Pass | Regression |
| TS-REG-06 | All Sprint 1-3 tests | Pass (no regression) | Regression |

#### Edge Case Tests

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-EDGE-01 | SGR with 20+ params | No overflow | Unit |
| TS-EDGE-02 | SGR 38 with truncated sub-params [38, 5] | Graceful handling | Unit |
| TS-EDGE-03 | IL with count=0 | Treat as 1 (ANSI default, TS normalizes) | Integration |
| TS-EDGE-04 | DL at last row of scroll region | Rows cleared | Unit |
| TS-EDGE-05 | ICH at last column | No-op or minimal shift | Unit |
| TS-EDGE-06 | DCH at last column | Clear last cell | Unit |
| TS-EDGE-07 | SU count > rows | Clamped to region height | Unit |
| TS-EDGE-08 | SD count > rows | Clamped to region height | Unit |
| TS-EDGE-09 | DECSTBM top > bottom | Ignore (invalid region) | Unit |
| TS-EDGE-10 | DECSTBM top=1, bottom=rows | Equivalent to reset | Unit |
| TS-EDGE-11 | SetMode multiple modes in one CSI | Each processed, actions collected | Integration |
| TS-EDGE-12 | SetMode 1049 then ResetMode 1049 | Save+switch, then switch back | Integration |
| TS-EDGE-13 | DSR at large cursor (999,999) | Multi-digit response correct | Unit |

## Code Quality Verification

### Format Check (Rust)
```bash
cd wasm && cargo fmt -- --check
```

### Static Analysis (Rust)
```bash
cd wasm && cargo clippy -- -D warnings
```

### TypeScript Type Check
```bash
bun run typecheck
```

## File Structure Verification

### Files to Modify
- `wasm/src/terminal_core.rs` - Add Sprint 4 handlers (SGR, Edit, Scroll, Mode, Device, response buffer)
- `src/terminal/state.ts` - Extend handleCsiWasm, add mode/device helpers, reduce syncCursorAttrsToWasm

### Files That Must NOT Change
- `wasm/src/lib.rs` - Unchanged
- `wasm/src/cell.rs` - Unchanged
- `wasm/src/unicode.rs` - Unchanged
- `src/terminal/wasm/terminal-core.ts` - Unchanged
- `src/terminal/wasm/loader.ts` - Unchanged
- `src/terminal/handlers/csi_char_attrs.ts` - Unchanged (JS fallback)
- `src/terminal/handlers/csi_edit.ts` - Unchanged (JS fallback)
- `src/terminal/handlers/csi_scrolling.ts` - Unchanged (JS fallback)
- `src/terminal/handlers/csi_modes.ts` - Unchanged (JS fallback)
- `src/terminal/handlers/csi_device.ts` - Unchanged (JS fallback)
- `src/terminal/handlers/index.ts` - Unchanged

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-01 | `wasm-pack build` succeeds with all Sprint 4 additions | Run build command |
| SC-02 | WASM binary size < 56KB total | Check `.wasm` file size after build |
| SC-03 | All Rust unit tests pass (Sprint 1-3 + Sprint 4) | `cargo test --manifest-path wasm/Cargo.toml` |
| SC-04 | All existing TypeScript tests pass (1824+) | `bun test` |
| SC-05 | SGR operations produce identical results to TS handlers | Cross-validation in integration tests |
| SC-06 | `syncCursorAttrsToWasm()` removed from all non-restore call sites | Code review / grep |
| SC-07 | All CSI actions routed through handleCsiWasm | Code review: default case returns false only for Unknown |
| SC-08 | `bun tauri dev` shows working terminal | Manual smoke test |
| SC-09 | vttest basic tests unchanged | Manual vttest run |
| SC-10 | Color display test: 256-color and TrueColor correct | Manual visual check |
| SC-11 | Alternate screen test: vim/top/htop switch correctly | Manual application test |
| SC-12 | Scroll region test: scroll within region works | Manual less/man test |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: handle_sgr parses SGR params | Phase 1 | Rust unit tests (TS-SGR-01 through TS-SGR-26) |
| FR2: SGR Reset | Phase 1 | Rust unit test (TS-SGR-01, TS-SGR-02) |
| FR3: SGR extended color | Phase 1 | Rust unit tests (TS-SGR-17 through TS-SGR-20) |
| FR4: syncCursorAttrsToWasm removed from non-restore sites | Phase 5 | Code review + grep verification |
| FR5: handle_insert_lines | Phase 2 | Rust unit tests (TS-EDIT-01 through TS-EDIT-04) |
| FR6: handle_delete_lines | Phase 2 | Rust unit tests (TS-EDIT-05 through TS-EDIT-08) |
| FR7: handle_insert_characters | Phase 2 | Rust unit tests (TS-EDIT-09 through TS-EDIT-11) |
| FR8: handle_delete_characters | Phase 2 | Rust unit tests (TS-EDIT-12 through TS-EDIT-14) |
| FR9: handle_scroll_up with bridge | Phase 3 | Rust unit tests (TS-SCR-01, TS-SCR-02) |
| FR10: handle_scroll_down | Phase 3 | Rust unit tests (TS-SCR-04, TS-SCR-05) |
| FR11: handle_decstbm | Phase 3 | Rust unit tests (TS-SCR-07 through TS-SCR-11) |
| FR12: handle_set_mode with action codes | Phase 4 | Rust unit tests (TS-MOD-01 through TS-MOD-13) |
| FR13: Boolean modes in WASM bitfield | Phase 4 | Rust unit tests (TS-MOD-01 through TS-MOD-04) |
| FR14: handle_device_status_report | Phase 4 | Rust unit tests (TS-DEV-01 through TS-DEV-04) |
| FR15: handle_primary_device_attributes | Phase 4 | Rust unit test (TS-DEV-05) |
| FR16: handle_secondary_device_attributes | Phase 4 | Rust unit test (TS-DEV-06) |
| FR17: get_response_ptr/len | Phase 4 | Rust unit test (TS-DEV-07) |
| FR18: handleCsiWasm all CSI actions | Phase 5 | Integration tests (TS-INT-01 through TS-INT-20) |
| FR19: JS fallback unchanged | Phase 5 | Regression tests (TS-REG-01 through TS-REG-06) |
| FR20: syncCursorAttrsToWasm removal | Phase 5 | Code review + grep verification |

### Non-Functional Requirements Coverage

| Requirement | Verification |
|-------------|--------------|
| NFR1: SGR batch 1 WASM call | Code review: single handle_sgr call in handleCsiWasm |
| NFR2: IL/DL/ICH/DCH 1 WASM call each | Code review: single call per operation in handleCsiWasm |
| NFR3: All TS tests pass (1824+) | `bun test` output |
| NFR4: WASM binary < 56KB | File size check after `wasm-pack build` |
| NFR5: JS fallback unchanged | Regression tests pass |
| NFR6: vttest basic tests unchanged | Manual vttest verification |

## E2E Testing (Docker)

Docker environment ref: CLAUDE.md testing instructions

### Setup
```bash
# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path wasm/Cargo.toml"

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# TypeScript type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Basic Functionality
- [ ] WASM build succeeds
- [ ] All Rust unit tests pass
- [ ] All TypeScript tests pass (1824+)
- [ ] TypeScript type check passes

### SGR
- [ ] SGR Reset, style flags, standard colors pass
- [ ] SGR 256-color and TrueColor pass
- [ ] SGR multi-param batch pass

### Edit
- [ ] IL/DL within scroll region pass
- [ ] ICH/DCH within row pass
- [ ] Boundary/clamping cases pass

### Scroll
- [ ] SU scroll region (returns 0) passes
- [ ] SU full screen (returns count) passes
- [ ] SD passes
- [ ] DECSTBM passes

### Modes
- [ ] Boolean modes set correctly
- [ ] Buffer switch action codes correct
- [ ] TS fallback for mouse modes correct

### Device
- [ ] DSR ps=5 and ps=6 produce correct responses
- [ ] DA1 and DA2 produce correct responses
- [ ] Response buffer readable from TS

### Regression
- [ ] No Sprint 1-3 test regressions
- [ ] No existing TS handler regressions

## Manual Testing (E2E Not Possible)

Items that cannot be automated via Docker:

- [ ] `bun tauri dev` launches working terminal
- [ ] vttest basic screen operations pass visually
- [ ] 256-color palette renders correctly in terminal
- [ ] TrueColor gradients render correctly
- [ ] vim opens and closes cleanly (alternate screen)
- [ ] top/htop switches alternate screen correctly
- [ ] less/man scrolls within region correctly
- [ ] Cursor position reports work (applications that query cursor position)

## Binary Size Verification

### Command
```bash
cd wasm && wasm-pack build --target web --out-dir pkg && ls -la pkg/*.wasm
```

### Expected Result
- WASM binary size < 56KB (Sprint 3 baseline: 45.8KB + budget: <10KB)
- Estimated Sprint 4 addition: ~4.7KB

## syncCursorAttrsToWasm Removal Verification

### Grep Check
```bash
grep -n "syncCursorAttrsToWasm" src/terminal/state.ts
```

### Expected Result
After Sprint 4:
- Method definition: 1 occurrence (the method itself remains for cursor restore)
- Call sites: exactly 1 (after cursor restore action in mode handling)
- Removed from: `switchToAlternateBuffer`, `switchToPrimaryBuffer`, `reset`

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 4 | - | 4 | - |
| Rust Unit Tests | 64 | - | 64 | - |
| TS Integration Tests | 20 | - | 20 | - |
| TS Regression Tests | 6 | - | 6 | - |
| Edge Case Tests | 13 | - | 13 | - |
| Code Quality | 3 | - | 3 | - |
| File Structure | 13 | - | - | 1 |
| SPEC Compliance | 12 | - | 7 | 5 |
| NFR Compliance | 6 | - | 4 | 2 |
| Binary Size | 1 | - | 1 | - |
| syncCursorAttrsToWasm | 1 | - | 1 | - |
| Smoke Test | 8 | - | - | 8 |

**Total**: 0 host-automated, 123 E2E (Docker) items, 16 manual items
