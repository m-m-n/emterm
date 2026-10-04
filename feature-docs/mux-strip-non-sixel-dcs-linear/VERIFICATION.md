# Verification Document: mux-strip-non-sixel-dcs-linear

## Overview
**Feature**: mux-strip-non-sixel-dcs-linear / **SPEC.md**: `feature-docs/mux-strip-non-sixel-dcs-linear/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-strip-non-sixel-dcs-linear/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/mux-strip-non-sixel-dcs-linear/THREAT-MODEL.md`

All commands run from the project root (the integration worktree root).

## Build Verification
- Command (workflow.yaml build_command): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (NFR3, CLI-only build): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command (workflow.yaml test_command): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, every test passes
- Coverage target: no coverage tool is configured; coverage is tracked per acceptance criterion (every SPEC.md AC maps to at least one test or check below)

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Shared strip entry points (strip_replayable_rich_content, strip_pty_output_for_scrollback_write, strip_rich_content_and_remap with watch offsets, strip_pty_output_for_scrollback_write_with_written_state from ground) on the 2 MiB input | Each call under 10 s; output equals the input | Unit |
| TS-2 | ScrollbackWriteFilter: one call with the chain just under the pending cap, pieces of at most 65,536 bytes with the chain just under the cap, past the cap (overflow flush) | Each case under 10 s; emitted bytes equal the input; pending empty after the trailing ST | Unit |
| TS-3 | build_snapshot_bytes / build_resume_snapshot_bytes on a 2 MiB scrollback of the input with dimension segments | Each under 10 s; scrollback part equals the input; mapped offsets non-decreasing and within the payload | Unit |
| TS-4 | run_reader_without_owner with the input in reads of at most 65,536 bytes over the pending-cap range | Under 10 s; ring equals the input | Integration |
| TS-5 | client_parity_scan::scan called directly on the 2 MiB input with and without excluded pieces | Each under 10 s; no item; no tail without excluded pieces | Unit |
| TS-6 | run_reader_with_suppressed_reads with one suppressed read while the chain is held and one carrying the trailing ST | Under 10 s; ring equals the input; the only empty PtyOutput chunk is the EOF marker | Integration |
| TS-7 | Full lib test run and the CLI-only cargo check; production diff check when TS-1 to TS-6 pass at HEAD | Both commands exit 0; no production source file changed unless FR7 applied | Integration (commands) |
| TS-8 | test-docs record content | Every new test listed under its AC with red_confirmed false and the FR8 red_reason | Document check |

## Code Quality Verification
- Format: none configured (workflow.yaml format_command is empty)
- Static analysis: none configured

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | AC-1 to AC-8 pass | TS-1 to TS-8 |
| SC-2 | No production code changes when AC-1 to AC-5 pass (AC-6) | The integration branch diff against the implement base commit touches, under `src-tauri/`, only the new test module and the module registration in `src-tauri/src/mux/ipc/pty_spawn/tests.rs`; when FR7 applied, the diff names the changed production file and TS-1 to TS-7 pass |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-4 |
| FR5 | task0001 | TS-5 |
| FR6 | task0001 | TS-6 |
| FR7 | task0001 | TS-7 (diff check and full run) |
| FR8 | task0001 | TS-8 |
| NFR1 | task0001 | TS-1 to TS-6 (sizes and the 10 s budget) |
| NFR2 | task0001 | TS-7 (full lib run; the new tests carry no ignore attribute; Cargo.toml unchanged) |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0001 | TS-1 (non-SIXEL DCS written unchanged), TS-7 (existing tests green) |

## Manual Testing (E2E Not Possible)
- None. Every scenario is automated or a document check.

## Performance / Security Verification (if applicable)
- NFR1: every budget assertion of TS-1 to TS-6 finishes under 10 s at the NFR1 sizes (2 MiB strip / snapshot / direct scan; at least the 512 KiB pending-cap range on the write path; reads of at most 65,536 bytes).
- TM-1: linear scans on the write path and the snapshot path, guarded by budget regression tests — checked by TS-1 to TS-6 passing in the full lib run.
- TM-2: a FR7 change keeps every strip-target decision — checked by TS-1 (the non-SIXEL input is written unchanged) and TS-7 (every existing strip-target test stays green); when FR7 applied, the production diff is also reviewed for an unchanged removal decision (non-SIXEL DCS kept, SIXEL DCS removed).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-8) | 8 | 7 | 0 | 1 |
| Success criteria | 2 | 2 | 0 | 0 |
| Performance (NFR1) | 1 | 1 | 0 | 0 |
| Security (TM-1) | 1 | 1 | 0 | 0 |
| Security (TM-2) | 1 | 1 | 0 | 0 |
