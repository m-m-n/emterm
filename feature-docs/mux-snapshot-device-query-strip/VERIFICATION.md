# Verification Document: mux-snapshot-device-query-strip

## Overview

**Feature**: mux-snapshot-device-query-strip / **SPEC.md**: `feature-docs/mux-snapshot-device-query-strip/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-snapshot-device-query-strip/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors
- Additional (NFR2): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` — exit code 0

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: every SPEC test scenario below has at least one passing test

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `\x1b[c` (bare DA1) in scrollback | stripped; surrounding text preserved | Unit |
| TS-2 | `\x1b[0c` / `\x1b[?1;2c` (DA1 params / `?` prefix) | stripped | Unit |
| TS-3 | `\x1b[>c` / `\x1b[>0c` (DA2) | stripped | Unit |
| TS-4 | `\x1b[5n` / `\x1b[6n` (DSR/CPR) | stripped | Unit |
| TS-5 | `\x1b[14t` / `\x1b[16t` / `\x1b[18t` | stripped | Unit |
| TS-6 | `\x1b[?Ps$p` (DECRPM, known + unknown mode) | stripped | Unit |
| TS-7 | keep set: `\x1b[=c`, `\x1b[?6n`, `\x1b[0n`, `\x1b[22t`, `\x1b[23t`, `\x1b[8;24;80t`, `\x1b[!p`, `\x1b["p` | preserved byte-for-byte | Unit |
| TS-8 | incomplete CSI at buffer end (`\x1b[6`) | preserved | Unit |
| TS-9 | C0 inside stripped query (`\x1b[\x076n`) | query removed, `\x07` re-emitted | Unit |
| TS-10 | mixed payload (viewer OSC + queries + text + SGR) | only viewer OSC and queries removed | Unit |
| TS-11 | all pre-existing `strip_*` tests | pass unchanged | Unit |
| TS-12 | `build_snapshot_bytes` product from DA1-bearing scrollback | contains no removable device query | Integration |

## Code Quality Verification

- Format: none (project PostToolUse hook handles per-file formatting; crate-wide fmt is prohibited by project policy)
- Static analysis: covered by the build check

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements implemented and tested | TS-1 … TS-12 pass |
| SC-2 | Existing filter tests pass unchanged | TS-11 |
| SC-3 | CLI-only build compiles | `--no-default-features` check exits 0 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6 |
| FR2 | task0001 | TS-7, TS-10, TS-11 |
| FR3 | task0001 | TS-8 |
| FR4 | task0001 | TS-9 |
| NFR1 | task0001 | TS-13 (bench, below) |
| NFR2 | task0001 | build verification (`--no-default-features`) |

## Performance Verification

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-13 | `strip_replayable_rich_content_bench_2mib_plain` (`#[ignore]` bench, run explicitly with `--include-ignored` filter per its doc comment) | per-call < 30ms threshold assertion passes | Bench (manual invocation) |

## E2E Testing

Out of scope per user decision (unit tests only). The Docker E2E suite is not run for this feature.

## Manual Testing (E2E Not Possible)

- [ ] MT-1 (deferred to user, not part of the verify phase): on a real mux session, detach → attach a zsh-prompt tab that previously showed `65;1;4;22c`; the prompt stays clean

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit / Integration | TS-1 … TS-12 | 12 | 0 | 0 |
| Performance | TS-13 | 1 (explicit invocation) | 0 | 0 |
| Build | 2 (default + no-default-features) | 2 | 0 | 0 |
| Manual (deferred) | MT-1 | 0 | 0 | 1 |
