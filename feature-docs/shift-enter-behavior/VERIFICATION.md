# Verification Document: Shift+Enter Behavior Setting

## Overview

**Feature**: shift-enter-behavior /
**SPEC.md**: `feature-docs/shift-enter-behavior/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/shift-enter-behavior/IMPLEMENTATION.md`

## Build Verification

- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (web): `bun run typecheck`
- Expected: exit code 0, no errors

## Test Verification

- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (web): `bun test`
- Coverage target: the decision table and deserialization matrix fully
  covered (no percentage target; this feature is small and enumerable)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `none`: bare Shift+Enter | Plain Enter encoding (`\r`) | Unit |
| TS-2 | `alt_enter`: bare Shift+Enter | Alt+Enter encoding | Unit |
| TS-3 | `kitty_csi_u`: bare Shift+Enter | Exact bytes `\x1b[13;2u` on HostPty and PosixPty targets | Unit |
| TS-4 | Ctrl+Enter / Alt+Enter / Ctrl+Shift+Enter under all three values | Not rewritten | Unit |
| TS-5 | Deserialization matrix (new key values + null, legacy true/false, both keys, neither) | Values per FR5 | Unit |
| TS-6 | settings_store round-trip | `shift_enter_behavior` persisted and reloaded | Unit |
| TS-7 | `bun run typecheck` | Passes with union type and select UI | Integration |
| TS-8 | `bun test` | Passes including the new section test | Integration |

## Code Quality Verification

- Format: none (project formats per-file via PostToolUse hook; crate-wide
  fmt is prohibited)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Default behavior byte-identical to current `shift_enter_as_alt_enter: true` | TS-2 with default settings |
| SC-2 | All FRs implemented and tested | Coverage table below |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-5, TS-6 |
| FR2 | task0001 | TS-1 |
| FR3 | task0001 | TS-2 |
| FR4 | task0001 | TS-3 |
| FR5 | task0001 | TS-5 |
| FR6 | task0002 | TS-8 (select rendering/save), M-1 |
| FR7 | task0002 | TS-7 |
| NFR1 | task0001 | TS-4 |

## E2E Testing

No project E2E framework — omitted.

## Manual Testing (E2E Not Possible)

- [ ] M-1: In a running eMterm, switch the setting between the three
      values in the settings panel and confirm in Claude Code:
      `alt_enter` and `kitty_csi_u` insert a newline on Shift+Enter;
      `none` submits.
- [ ] M-2: In a mux session, `kitty_csi_u` behaves the same as in a plain
      host tab.
- [ ] M-3: Search bar open: Shift+Enter still navigates to the previous
      match regardless of the setting value.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit/Integration | 8 | 8 | 0 | 0 |
| Manual | 3 | 0 | 0 | 3 |
