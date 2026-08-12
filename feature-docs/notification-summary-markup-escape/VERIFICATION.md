# Verification Document: notification-summary-markup-escape

## Overview

**Feature**: notification-summary-markup-escape /
**SPEC.md**: `feature-docs/notification-summary-markup-escape/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/notification-summary-markup-escape/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, all tests pass
- Coverage target: no numeric threshold is defined for this project. The
  binding target: every task0001 Acceptance Criterion maps to at least one
  unit test (AC-1..AC-5 → TS1..TS5), and the full existing `--lib` suite
  stays green.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Title containing `<`, `>`, `&` through the escape transform (FR1) | Same entity output as the body path: `&` processed first; double-escape of pre-existing entities accepted (mirrors `src-tauri/src/callbacks/tests.rs:695`) | Unit |
| TS2 | Composed sink decision (FR1, FR3) | Confirmed capabilities escape the title; unconfirmed (list without `body-markup`, or fetch failure) leaves it byte-for-byte unchanged (mirrors `src-tauri/src/callbacks/tests.rs:751`) | Unit |
| TS3 | `sanitize_title`-truncated 100-char title ending in `<` (FR1) | Escapes to a complete trailing entity reference — escape after truncation (mirrors `src-tauri/src/callbacks/tests.rs:707`) | Unit |
| TS4 | OSC 9 fallback-title branch: empty title segment → tab title or `"emterm"` (FR2) | Fallback title flows through the same escaped summary decision | Unit |
| TS5 | Regression: existing PR #35 body-escape tests (FR3) | All pass unchanged — body escape order, gate, and fail-open byte-for-byte identical | Unit (existing suite) |
| TS6 | Single egress point (NFR1) | The summary escape exists only inside `NotifyRustSink::send`; no per-producer escaping introduced | Inspection |
| TS7 | Platform scope (NFR2) | The change is confined to the existing Unix-only conditional-compilation scope; the Windows toast path is unmodified | Inspection |

## Code Quality Verification

- Format: none — this project intentionally has no crate-wide format command
  (`format_command` is empty; a project hook formats individual edited files).
- Static analysis: covered by the build verification command above.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | A unit test fixes that a tag-bearing title is escaped in the summary when capabilities confirm body-markup, and left unchanged when unconfirmed | TS1, TS2 |
| SC2 | Existing PR #35 body-escape tests still pass unchanged | TS5 |
| SC3 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes | Run the Test Verification command; exit code 0 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2, TS3 |
| FR2 | task0001 | TS4 |
| FR3 | task0001 | TS2, TS5 |
| NFR1 | task0001 | TS4, TS6 |
| NFR2 | task0001 | TS7 |

## Manual Testing (E2E Not Possible)

This project has no E2E infrastructure (SPEC.md: no E2E inputs resolved).
The design step was skipped, so there is no mockup visual comparison.

- [ ] TS6 (inspection): search the diff and `src-tauri/src/` for summary
      escape application — it appears only inside `NotifyRustSink::send`; no
      producer (OSC 9 / tab activity / agent status / link-hover) escapes on
      its own, and internal copies (`pending_notifications`, rate-limiter
      keys) keep the raw title.
- [ ] TS7 (inspection): the diff in `src-tauri/src/callbacks.rs` stays
      inside the existing Unix-only conditional-compilation scope; the
      Windows toast path and the dispatch/logging behavior are unmodified.
- [ ] MT-1 (optional smoke test): on a Linux desktop running dunst with
      `markup=full`, emit an OSC 9 notification whose title contains a tag
      (e.g. an `<a href>` fragment) and confirm the popup shows the tag as
      literal text, not as rendered markup or a link.

## Performance / Security Verification

- Security (FR1, NFR1): markup meta characters in the summary are
  neutralized at the single D-Bus egress when the capability gate confirms
  body-markup — verified by TS1–TS4 (automated) and TS6 (inspection).
- Fail-open scope note: unconfirmed capability passes the title through
  unchanged by design (parity with the body path); fail-open on
  capability-retrieval failure itself is out of scope for this feature.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests (TS1–TS5) | 5 | 5 | 0 | 0 |
| Inspection (TS6, TS7) | 2 | 0 | 0 | 2 |
| Optional smoke (MT-1) | 1 | 0 | 0 | 1 |
