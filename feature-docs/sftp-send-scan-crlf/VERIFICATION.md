# Verification Document: sftp-send-scan-crlf

## Overview

**Feature**: sftp-send-scan-crlf / **SPEC.md**: `feature-docs/sftp-send-scan-crlf/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/sftp-send-scan-crlf/IMPLEMENTATION.md`

All commands run from the project root (the integration worktree root during
verify), never after changing into `src-tauri/`.

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (main-cli): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (main-windows): `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests`
- Expected: exit code 0, no errors, for each command. The main-windows check
  compiles the test module for the platform where the regression surfaced.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Focused run: the same command with the test-name filter `send_site`, which
  selects both the real-source structural test and the red-case test.
- Coverage target: not applicable (test-only change); every scenario below
  passing is the criterion.
- Note: unrelated tests known to be flaky under parallel execution (tab replay
  tests, tmux socket discovery) are re-run with a single test thread before a
  failure is attributed to this feature.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | CRLF synthetic text — a `#[cfg(test)]` seam impl block holding a bare send, a blank line, then the `#[cfg(test)]` / `mod tests` boundary, every line break CRLF — fed to the detection routine inside `send_site_scan_detection_routine_red_cases` | `Found` (not `MissingBoundary`); the bare send is detected | Unit |
| TS-2 | Existing red cases (i)–(iv) and the real-source structural test `every_progress_and_result_send_site_routes_through_the_wake_helpers` on the LF working copy | All pass; the red cases' inputs and expectations are unchanged | Unit |
| TS-3 | Separate working copy checked out on Linux with `core.autocrlf=true` so that `service.rs` is CRLF; run the `--lib` tests there | The structural test passes | Manual |
| TS-4 | Full-CRLF and mixed LF/CRLF variants of the red-case texts (i)–(iii) (SPEC edge case) | Boundary found; progress and result flags identical to the LF original | Unit |
| TS-5 | CRLF text whose bare send sits on the line directly above the boundary (offset-sensitive position), and CRLF text with sends only inside the `mod tests` body | The former is detected; the latter reports nothing | Unit |
| TS-6 | Whole-file text search of `src-tauri/src/sftp/service.rs` for either sender name immediately followed by the send-call opener | Zero matches | Static |
| TS-7 | Diff of `src-tauri/src/sftp/service.rs` against the base revision | Every changed line lies inside the `#[cfg(test)] mod tests` item; boundary locator signature and anchor, production code, and the seam block are unchanged | Static |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` (check
  only; no write-mode crate-wide format run)
- Static analysis: none configured in workflow.yaml; TS-6 and TS-7 are this
  feature's static checks.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | In a working copy checked out with `core.autocrlf=true` (`service.rs` is CRLF), `cargo test --lib` passes `every_progress_and_result_send_site_routes_through_the_wake_helpers` | TS-3 (manual) |
| AC-2 | `send_site_scan_detection_routine_red_cases` has a CRLF synthetic case confirming the boundary is found and a bare send before it is detected | TS-1 (test run plus inspection that the case lives in that test) |
| AC-3 | On the LF working copy, the full `--lib` test command passes, including red cases (i)–(iv) | TS-2 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-3, TS-4 |
| FR2 | task0001 | TS-3, TS-4, TS-5 |
| FR3 | task0001 | TS-1 |
| FR4 | task0001 | TS-2 |
| NFR1 | task0001 | TS-2, TS-7 |
| NFR2 | task0001 | TS-6 |

## E2E Testing

Not applicable (SPEC: no E2E tests for this area).

## Manual Testing (E2E Not Possible)

- [ ] TS-3: real CRLF working copy
  1. Create a separate working copy of the feature's integration branch
     outside the main working tree, with `core.autocrlf=true` in effect at
     checkout time (for example a fresh clone configured with that setting).
  2. Confirm `src-tauri/src/sftp/service.rs` in that copy has CRLF line
     terminators (for example with a file-type inspection).
  3. A fresh copy has no `viewer/dist` / `settings/dist` bundles; build them
     with bun first if the GUI build script requires them.
  4. From that copy's root, run the `--lib` test command (using that copy's
     own `src-tauri/target`).
  5. Expected: `every_progress_and_result_send_site_routes_through_the_wake_helpers`
     and `send_site_scan_detection_routine_red_cases` pass.
  6. Optional control: the same procedure on the base revision reproduces the
     `MissingBoundary` panic.

No mockup comparison applies (design step skipped; no UI change).

## Performance / Security Verification

Not applicable.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 (main, main-cli, main-windows) | 3 | 0 | 0 |
| Unit tests | 4 (TS-1, TS-2, TS-4, TS-5) | 4 | 0 | 0 |
| Static checks | 2 (TS-6, TS-7) | 1 (TS-6) | 0 | 1 (TS-7 diff review) |
| Format | 1 | 1 | 0 | 0 |
| Manual scenarios | 1 (TS-3) | 0 | 0 | 1 |
