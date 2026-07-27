# Verification Document: emterm --version flag

## Overview

**Feature**: version-flag / **SPEC.md**: `feature-docs/version-flag/SPEC.md`
/ **IMPLEMENTATION.md**: `feature-docs/version-flag/IMPLEMENTATION.md`

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Expected: all tests pass (new `--version` integration tests included)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Run binary with `--version` | stdout is exactly the crate version + newline; exit 0 | Integration |
| TS-2 | `--version` side effects | stderr empty; no logger/GUI startup on this path | Integration |
| TS-3 | Default-features build | `cargo check` passes | Build gate |
| TS-4 | CLI-only build | `cargo check --no-default-features` passes | Build gate |
| TS-5 | Workflow structure | release.yml valid YAML; `sync-version` job exists; `create-release.needs` includes it | Static |
| TS-6 | sync-version semantics | version resolution matches `get-version`; commit/push only on diff; Cargo.toml + Cargo.lock both rewritten | Static |

## Code Quality Verification

- Format: none configured (rustfmt not enforced in this project)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1/FR2 implemented and tested | TS-1..TS-4 |
| SC-2 | FR3 implemented | TS-5..TS-6 |
| SC-3 | Existing tests keep passing | test command above |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-3, TS-4 |
| FR3 | task0002 | TS-5, TS-6 |
| NFR1 | task0001 | TS-2 |
| NFR2 | task0002 | TS-6 (existing stamping untouched; dispatch path resolution) |

## E2E Testing

Not applicable — no UI, and CI workflow runs cannot execute locally.

## Manual Testing (E2E Not Possible)

- [ ] MT-1: After a real tag push, confirm on GitHub that a version-bump
  commit lands on `main` before the release is created (post-merge,
  human-operated — outside this workflow run).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Binary flag | TS-1..TS-4 | 4 | 0 | 0 |
| CI workflow | TS-5..TS-6 | 2 (static) | 0 | 0 |
| Release ordering | MT-1 | 0 | 0 | 1 |
