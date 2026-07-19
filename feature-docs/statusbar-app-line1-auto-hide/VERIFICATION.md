# Verification Document: Status Bar App Line 1 Auto-Hide

## Overview

**Feature**: statusbar-app-line1-auto-hide / **SPEC.md**: `feature-docs/statusbar-app-line1-auto-hide/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/statusbar-app-line1-auto-hide/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Also: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` (NFR1)
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: all tests pass (known environment-dependent mux replay flakes recorded in prior features are evaluated against the base commit if they appear)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Enabled, all rows empty | visible row count 0, panel height 0 | Unit |
| TS-2 | App Line 1 empty, OSC row has content | count 1, only OSC row drawn | Unit |
| TS-3 | App Line 1 has content | row counted and drawn | Unit |
| TS-4 | App Line 1 empty, App Line 2 has content | Line 1 hidden, Line 2 shown | Unit |
| TS-5 | Pre-existing status-bar tests (Line 2 auto-hide, OSC rules, disabled short-circuit) | pass unchanged in intent | Unit |

## Code Quality Verification

- Format: not enforced project-wide (rustfmt non-mandatory) — no format command configured

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FRs implemented and tested | TS-1..TS-5 pass |
| SC-2 | Default-features build passes | build command exit 0 |
| SC-3 | CLI-only build passes | `--no-default-features` check exit 0 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2, TS-3, TS-4 |
| FR2 | task0001 | TS-1 |
| FR3 | task0001 | TS-5 |
| NFR1 | task0001 | SC-3 |

## Manual Testing (E2E Not Possible)

- [ ] MT-1: Release build with `statusbar_enabled: true` and empty app-line templates — no empty band is shown; attaching to a mux session shows only the OSC row (user-performed; requires release build).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests | 5 | 5 | 0 | 0 |
| Manual | 1 | 0 | 0 | 1 |
