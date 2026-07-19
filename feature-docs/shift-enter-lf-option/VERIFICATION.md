# Verification Document: Shift+Enter LF Option

## Overview

**Feature**: shift-enter-lf-option /
**SPEC.md**: `feature-docs/shift-enter-lf-option/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/shift-enter-lf-option/IMPLEMENTATION.md`

## Build Verification

- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (web): `bun run typecheck`
- Expected: exit code 0, no errors

## Test Verification

- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (web): `bun test`
- Coverage target: the new variant's decision row, both deserialization
  layers, and the UI option policy fully covered

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `lf`: bare Shift+Enter (both EncodeTargets); Ctrl/Alt combos | Exactly 0x0a; combos untouched | Unit |
| TS-2 | `"lf"` deserialization (native + app_settings); unknown fallback | Parses; unknown → default | Unit |
| TS-3 | `lf` persistence (settings-store + settings-window boundary) | Survives round-trips | Unit |
| TS-4 | Regression: three prior values' bytes, migration, null precedence, sentinel | All unchanged, pass | Unit |
| TS-5 | `bun run typecheck` | Passes with extended union | Integration |
| TS-6 | Section tests: 3-option order; 4-option grandfathering; saving `lf` | All pass | Integration |

## Code Quality Verification

- Format: none (project formats per-file via PostToolUse hook)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Existing values byte-identical to before | TS-4 |
| SC-2 | All FRs implemented and tested | Coverage table below |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3 |
| FR2 | task0002 | TS-6 |
| FR3 | task0002 | TS-6 |
| FR4 | task0001 | TS-4 |
| FR5 | task0002 | TS-5, TS-6 |
| NFR1 | task0001 | TS-4 |

## E2E Testing

No project E2E framework — omitted.

## Manual Testing (E2E Not Possible)

- [ ] M-1: In a running eMterm with `lf` selected: Shift+Enter inserts a
      newline in Claude Code, and behaves like Enter in a shell (no stray
      characters).
- [ ] M-2: With settings.json containing `kitty_csi_u`: behavior is
      unchanged and the settings panel shows it as the selected (fourth)
      option; switching to `lf` removes it from the list.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit/Integration | 6 | 6 | 0 | 0 |
| Manual | 2 | 0 | 0 | 2 |
