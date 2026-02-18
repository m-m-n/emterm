# Verification Document: WASM Implementation Optimization

## Overview
**Feature**: wasm-optimization
**SPEC.md**: `doc/tasks/wasm-optimization/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wasm-optimization/IMPLEMENTATION.md`

## Build Verification

- Rust WASM build: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cd wasm && wasm-pack build --release --target web"`
- Rust tests: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path wasm/Cargo.toml"`
- TypeScript build: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run build"`
- TypeScript typecheck: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Expected: All exit code 0, no errors

## Test Verification

- Rust command: `cargo test --manifest-path wasm/Cargo.toml`
- TypeScript command: `bun test`
- Coverage target: minimum 80%, target 90% on changed code

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | process_pty_data with mixed PTY data (print, CSI, OSC, ESC) | All actions dispatched correctly | Unit |
| TS-02 | Parser state preserved after std::mem::take pattern | Parser continues from correct state across calls | Unit |
| TS-03 | Overflow cell survives reflow on width change | Overflow string present at new position after resize | Unit |
| TS-04 | ZWJ family emoji (25 bytes) displays after resize | char_len=0xFF cell with correct overflow string | Unit |
| TS-05 | CSI with 0 params dispatches correctly | CsiDispatch with param_count=0 | Unit |
| TS-06 | CSI with 8 params (max) dispatches correctly | All 8 params accessible | Unit |
| TS-07 | CSI with >8 params truncates silently | Only first 8 params retained | Unit |
| TS-08 | Cell underline_style/color round-trip | Values set and read back correctly | Unit |
| TS-09 | Overflow with row index > 65535 | Stores and retrieves correctly with u32 key | Unit |
| TS-10 | Reverse index consistent after shift_rows_up/down | Index matches overflow table state | Unit |
| TS-11 | scroll_up_internal(1) marks only last row dirty | Only viewport last row in dirty bitset | Unit |
| TS-12 | scroll_up_internal(1) in scroll region marks all dirty | Fallback behavior unchanged | Unit |
| TS-13 | Full ANSI processing with fixed-length arrays through direct dispatch | FR1+FR3 integration | Integration |
| TS-14 | Reflow with overflow using u32 keys | FR2+FR5 integration | Integration |
| TS-15 | Empty PTY data (0 bytes) | No dispatch, parser state unchanged | Edge |
| TS-16 | Reflow when all cells are overflow | All overflow cells survive | Edge |
| TS-17 | CSI with >2 intermediates | Truncated to 2 | Edge |
| TS-18 | scrollback_lines=0 with overflow operations | Empty table, no crash | Edge |
| TS-19 | Rapid successive full-screen scrolls | Scroll events handled correctly | Edge |
| TS-20 | Benchmark process_pty_data with 1MB ANSI data | Measurable allocation reduction | Performance |
| TS-21 | WASM binary size comparison | Size reduced after FR7 | Performance |

## Code Quality Verification

- Rust format: `cargo fmt --manifest-path wasm/Cargo.toml -- --check`
- Rust lint: `cargo clippy --manifest-path wasm/Cargo.toml -- -D warnings` (if configured)
- TypeScript typecheck: `bun run typecheck`

## File Structure Verification

### Files to Modify
- `wasm/Cargo.toml` — FR7: codegen-units=1, strip="symbols"
- `wasm/src/parser_types.rs` — FR3: CsiDispatch fixed-length arrays
- `wasm/src/parser.rs` — FR3: emit fixed-len, FR9: buffer pre-alloc
- `wasm/src/cell.rs` — FR4: underline fields, FR5: key type, FR6: index type
- `wasm/src/terminal_core.rs` — FR1: take pattern, FR5/FR6: overflow ops, FR8: scroll event API
- `wasm/src/ring_buffer.rs` — FR2: reflow overflow, FR6: reverse index, FR8: scroll event
- `wasm/src/csi_dispatch.rs` — FR3: param slicing, FR4: SGR dispatch
- `wasm/src/print_handler.rs` — FR4: Cell underline init
- `src/terminal/canvas-renderer.ts` — FR8: differential scroll
- `src/terminal/wasm/terminal-core.ts` — FR8: scroll event bridge

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All existing tests pass | Run full Rust + TS test suites |
| SC-02 | New tests for each FR pass | Run Rust tests, check FR-specific test functions |
| SC-03 | WASM binary size reduced (FR7) | Compare wasm binary size before/after |
| SC-04 | process_pty_data allocation reduction (FR1, FR3) | TS-20 benchmark or code inspection (no Vec::new in hot path) |
| SC-05 | ZWJ family emoji survives resize (FR2) | TS-04 test |
| SC-06 | Full-screen scroll renders only 1 new row (FR8) | TS-11 test + manual visual verification |
| SC-07 | Cell struct exactly 32 bytes (NFR2) | Static assertion in Rust test |
| SC-08 | Code review completed | Manual review |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Direct dispatch via std::mem::take | Phase 3 | TS-01, TS-02, TS-15, SC-04 |
| FR2: Reflow overflow preservation | Phase 4 | TS-03, TS-04, TS-14, TS-16, SC-05 |
| FR3: ParsedAction fixed-length arrays | Phase 2 | TS-05, TS-06, TS-07, TS-17 |
| FR4: Cell underline fields | Phase 2 | TS-08, SC-07 |
| FR5: OverflowTable u32 keys | Phase 2 | TS-09, TS-18 |
| FR6: Overflow reverse index | Phase 4 | TS-10 |
| FR7: Cargo.toml optimization | Phase 1 | TS-21, SC-03 |
| FR8: Differential scroll rendering | Phase 5 | TS-11, TS-12, TS-19, SC-06 |
| FR9: APC/DCS buffer pre-allocation | Phase 1 | Code inspection (with_capacity call) |

## Manual Testing

- [ ] FR7: Record WASM binary size before changes, compare after Phase 1
- [ ] FR8: Visual verification — rapid `yes` or `seq 100000` output shows smooth scrolling without artifacts
- [ ] FR8: Canvas drawImage compatibility on target platform (WebKitGTK on Linux)
- [ ] FR2: Display ZWJ family emoji, resize terminal, verify emoji still visible
- [ ] FR4: Verify underline_style/color in packed format renders correctly (when renderer support is added)

## Performance Verification

- FR1+FR3: process_pty_data with 1MB ANSI data — no intermediate Vec allocation visible in code
- FR7: WASM binary size — expected reduction from codegen-units=1 + strip=symbols
- FR8: Full-screen scroll — dirty bitset shows only 1 row dirty after scroll(1)

## Security Verification

- [ ] FR3: CSI with >8 params does not cause buffer overflow (truncation verified in TS-07)
- [ ] FR1: Parser restoration after dispatch panic (drop guard pattern verified)
- [ ] APC/DCS buffer caps (MAX_APC_LEN, MAX_DCS_LEN) remain unchanged

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 4 | 4 | 0 |
| Unit Tests | 12 | 12 | 0 |
| Integration Tests | 3 | 3 | 0 |
| Edge Cases | 5 | 5 | 0 |
| Performance | 2 | 1 | 1 |
| Security | 3 | 2 | 1 |
| Visual/UX | 4 | 0 | 4 |
| **Total** | **33** | **27** | **6** |
