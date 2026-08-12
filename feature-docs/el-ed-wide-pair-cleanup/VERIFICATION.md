# Verification Document: el-ed-wide-pair-cleanup

## Overview

**Feature**: el-ed-wide-pair-cleanup / **SPEC.md**: `feature-docs/el-ed-wide-pair-cleanup/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/el-ed-wide-pair-cleanup/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0; every pre-existing test passes unmodified; all TS-mapped tests below pass
- Coverage target: no numeric coverage gate is configured for this project; coverage is scenario-based — every TS row below is implemented and passing

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Wide char base at col−1 / spacer at col; cursor on the spacer; EL 0 (`ESC[K`) | Base at col−1 is a width-1 space with its original attributes; `[col, cols)` is BCE | Unit |
| TS-2 | Cursor on a wide char base at col (spacer at col+1); EL 1 (`ESC[1K`) | Spacer at col+1 is a width-1 space; `[0, col+1)` is BCE | Unit |
| TS-3 | TS-1 / TS-2 repeated through ED 0 (`ESC[J`) and ED 1 (`ESC[1J`) | Cursor-row results identical to TS-1/TS-2; rows below/above fully cleared | Unit |
| TS-4 | Negative: cursor on the base with EL 0 (both halves inside the range); cursor on the spacer with EL 1 (base also inside) | No orphan is created; no extra blanking outside the range | Unit |
| TS-5 | Boundary: cursor at col 0 with EL 0 (no left partner); col+1 == cols with EL 1 (right partner check out of bounds) | The cleanup step is a safe no-op | Unit |
| TS-6 | No-op: EL 2 / ED 2 over a row containing wide pairs | Fully BCE-cleared row with no writes attributable to partner cleanup | Unit |

## Code Quality Verification

- Format: `cargo fmt -p term_core`
- Static analysis: none configured beyond the build command above

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | EL 0 blanks the width-2 base at col−1 when the cursor is on a width-0 spacer (FR1) | TS-1 test passes |
| AC2 | EL 1 blanks the width-0 spacer at col+1 when the cursor is on a width-2 base (FR2) | TS-2 test passes |
| AC3 | ED 0 / ED 1 apply the same rule on the cursor row (FR3) | TS-3 test passes |
| AC4 | Full-line clears (EL 2 / ED 2) are no-ops for edge cleanup, no behavioral change (FR4) | TS-6 test passes |
| AC5 | The reproduction paths exist as term_core unit tests and pass; the full `--lib` suite passes (FR6) | Test command above exits 0 with the new tests included |
| AC6 | Remaining out-of-scope items recorded in SPEC / IMPLEMENTATION as known remaining work (FR7) | Manual document inspection (MV-1 below) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-4 |
| FR2 | task0001 | TS-2, TS-4 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-6 |
| FR5 | task0001 | TS-1, TS-2, TS-3 (attribute-preservation assertions) |
| FR6 | task0001 | TS-1, TS-2, TS-3, TS-6 exist as inline term_core unit tests and pass |
| FR7 | — (satisfied by planning artifacts) | MV-1: SPEC.md "Out of Scope / Known Remaining Work" + IMPLEMENTATION.md "Known Remaining Work" record the items |
| NFR1 | task0001 | MV-2: `crates/term_core/Cargo.toml` unchanged (no new dependency) |
| NFR2 | task0001 | TS-1, TS-2 (BCE fill inside the range unchanged); pre-existing EL/ED tests pass unmodified; MV-3 |
| NFR3 | task0001 | MV-4: per-call-site cost bounded to two width lookups + two conditional single-cell writes (ECH-equivalent profile) |
| NFR4 | task0001 | TS-5 |

## E2E Testing

None — this project has no E2E infrastructure (workflow.yaml `e2e_test_command` is empty; no E2E suite exists per `test/README.md`).

## Manual Testing (E2E Not Possible)

The design step was skipped (no UI surface), so no mockup visual comparison applies. The manual items are document/diff inspections:

- [ ] MV-1 (AC6 / FR7): confirm SPEC.md and IMPLEMENTATION.md record the known remaining work (partner-cleanup chokepoint refactor, overflow-path tests, PR #30-covered ECH/DCH/ICH/print paths).
- [ ] MV-2 (NFR1): confirm `crates/term_core/Cargo.toml` gained no new dependency (diff inspection).
- [ ] MV-3 (NFR2): confirm the diff touches only `crates/term_core/src/csi_screen.rs` and no primitive (`clear_line_range` / `clear_line` / `blank_wide_pair_split`) changed.
- [ ] MV-4 (NFR3): confirm the added per-call-site work is bounded to two width lookups and two conditional single-cell writes.

## Performance / Security Verification

- Performance (NFR3): verified structurally via MV-4; no separate benchmark (SPEC.md declares performance tests not applicable).
- Security: not applicable — in-crate grid-state fix; no new input surface, auth, or data-protection boundary.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit test scenarios (TS-1..TS-6) | 6 | 6 | 0 | 0 |
| Format | 1 | 1 | 0 | 0 |
| Success criteria (AC1..AC6) | 6 | 5 | 0 | 1 (AC6 = MV-1) |
| NFR checks (NFR1..NFR4) | 4 | 2 (NFR2 via TS-1/TS-2 + suite, NFR4 via TS-5) | 0 | 3 (MV-2, MV-3, MV-4; NFR2 is verified both ways) |
