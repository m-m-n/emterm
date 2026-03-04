# Verification Document: Background Color Erase (BCE)

## Overview
**Feature**: Background Color Erase (BCE)
**SPEC.md**: `doc/tasks/background-color-erase/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/background-color-erase/IMPLEMENTATION.md`

## Build Verification
- Command: `cargo test --manifest-path src-tauri/Cargo.toml` (Rust) and `bun test` (TypeScript)
- Expected: exit code 0, no errors

## Test Verification
- Command: `cargo test --manifest-path src-tauri/Cargo.toml` and `bun test`
- Coverage target: all modified erase/scroll code paths covered

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | EL 0 with green bg | Cells from cursor to end have green bg | Unit |
| TS-02 | EL 1 with green bg | Cells from start to cursor have green bg | Unit |
| TS-03 | EL 2 with green bg | All cells on line have green bg | Unit |
| TS-04 | ED 0 with green bg | Cells below cursor have green bg | Unit |
| TS-05 | ED 2 with green bg | All cells on screen have green bg | Unit |
| TS-06 | ECH with green bg | N erased cells have green bg | Unit |
| TS-07 | ICH with green bg | Inserted blank cells have green bg | Unit |
| TS-08 | DCH with green bg | Trailing blank cells have green bg | Unit |
| TS-09 | Scroll up with green bg | New bottom line has green bg | Unit |
| TS-10 | Scroll down with green bg | New top line has green bg | Unit |
| TS-11 | IL with green bg | Inserted blank lines have green bg | Unit |
| TS-12 | DL with green bg | New blank lines at bottom have green bg | Unit |
| TS-13 | Default bg erase | Erased cells have DEFAULT bg | Unit |
| TS-14 | SGR reset then EL | After ESC[0m, erased cells have DEFAULT bg | Unit |
| TS-15 | 256-color bg erase | BCE applies with indexed color | Unit |
| TS-16 | RGB bg erase | BCE applies with RGB color | Unit |

## Code Quality Verification
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: `cargo clippy --manifest-path src-tauri/Cargo.toml`

## File Structure Verification

### Files to Create
- (none)

### Files to Modify
- `wasm/src/terminal_core.rs` - Add bce_cell(), update clear_line/clear_line_range/shift_rows_up/shift_rows_down
- `wasm/src/csi_edit.rs` - Update handle_insert_characters/handle_delete_characters
- `wasm/src/ring_buffer.rs` - Update ring_push_blank signature, update scroll_up_internal

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All functional requirements FR1-FR4 implemented | Unit tests TS-01 through TS-16 pass |
| SC-02 | All existing tests pass | Run full test suite, zero failures |
| SC-03 | E2E tests pass without regression | Run `./scripts/run-e2e-docker.sh test` |
| SC-04 | No performance regression | No additional allocations or branches in hot path |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Erase operations inherit cursor bg | Phase 1 | TS-01 through TS-06, TS-13, TS-14 |
| FR2: Insert/delete inherit cursor bg | Phase 2 | TS-07, TS-08 |
| FR3: Scroll/line ops inherit cursor bg | Phase 3 | TS-09 through TS-12 |
| FR4: Reset/resize use default bg | Phase 1 | TS-13 (unchanged behavior), code review |

## E2E Testing (Docker)
- [ ] Existing E2E tests pass: `./scripts/run-e2e-docker.sh test`

## Manual Testing (E2E Not Possible)
- [ ] Visual: Run Claude Code diff display in eMterm, verify background colors fill entire line blocks
- [ ] Visual: Scroll within colored region, verify new lines have correct background

## Performance Verification
- NFR1: No measurable latency increase. bce_cell() copies one struct field; no heap allocation, no branching change.

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Erase operations (FR1) | 6 | 6 | 0 | 0 |
| Insert/delete (FR2) | 2 | 2 | 0 | 0 |
| Scroll/line ops (FR3) | 4 | 4 | 0 | 0 |
| Edge cases | 4 | 4 | 0 | 0 |
| Regression | 2 | 1 | 1 | 0 |
| Visual verification | 2 | 0 | 0 | 2 |
| **Total** | **20** | **17** | **1** | **2** |
