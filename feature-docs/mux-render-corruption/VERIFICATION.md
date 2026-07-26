# Verification Document: mux Render Corruption Fix

## Overview

**Feature**: mux-render-corruption /
**SPEC.md**: `feature-docs/mux-render-corruption/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-render-corruption/IMPLEMENTATION.md`

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only gate, NFR2): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (web): `bun run build:viewer`
- Expected: exit code 0, no errors

## Test Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Command (web): `bun test`

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Replay of a resize-interleaved apt-style recording into a fixed-size core | No row mixes content from two distinct logical lines | Unit |
| TS-2 | Replay of a resize-interleaved TUI-style (cursor-addressed redraw) recording | No cross-line content mixing | Unit |
| TS-3 | Replay of a resize-free recording | Grid identical to pre-fix behavior | Unit |
| TS-4 | Marker-bearing recording through full pipeline (write filter → snapshot → replay) | Marker bytes never appear as visible cells | Unit |
| TS-5 | Existing Rust `--lib` suite (single-threaded) + `bun test` | All pass | Unit/Integration |
| TS-6 | CLI-only feature check (`--no-default-features`) | Compiles clean | Build |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Root cause identified; relationship to known coordinate-drift bug documented | task0001 completion report (AC-6) names the verdict and reproducing test |
| SC-2 | Regression tests added and passing | TS-1..TS-4 exist and pass |
| SC-3 | No regression in existing tests | TS-5 |
| SC-4 | User performs final on-device verification | MT-1 / MT-2 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | SC-1 (investigation verdict with test evidence) |
| FR2 | task0001 | TS-1, TS-2 |
| FR3 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| NFR1 | task0001 | MT-2 (manual latency feel check) |
| NFR2 | task0001 | TS-6 |
| NFR3 | task0001 | TS-5 |

## Manual Testing (E2E Not Possible)

- [ ] MT-1: On-device — run Claude Code in mux, repeat window/tab switches
      (including detach → attach); no line-content mixing appears
- [ ] MT-2: On-device — window switch / reattach latency feels unchanged
      from before the fix
- [ ] MT-3 (task0005 rework, review round-4 finding `6c650908ea8e95e9`):
      On-device — drag a mux window's edge to resize it repeatedly (a
      continuous drag, not discrete resizes), producing dozens of grid-size
      changes in quick succession against a pane with substantial
      scrollback (e.g. a long-running `seq`/`glances`/log-tailing pane),
      then immediately switch away to another window and back. The switch
      completes without a multi-second stall and the restored content is
      correct (no cross-phase-mixed rows).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Tests | 6 (TS-1..TS-6) | 6 | 0 | 0 |
| Manual | 3 (MT-1, MT-2, MT-3) | 0 | 0 | 3 |
