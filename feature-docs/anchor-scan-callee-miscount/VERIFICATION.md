# Verification Document: anchor-scan-callee-miscount

## Overview

- **Feature**: anchor-scan-callee-miscount
- **SPEC.md**: `feature-docs/anchor-scan-callee-miscount/SPEC.md`
- **IMPLEMENTATION.md**: not produced. The tier is `reduced`, and the single
  task shares no file with another task.
- **Task plan**: `feature-docs/anchor-scan-callee-miscount/tasks/task0001.md`

The change is confined to the `#[cfg(test)]` `anchor_scan` module in
`src-tauri/src/window_host/tests.rs` and its tests. The verification is the
Rust `main` component's test-target check plus the scenarios below. The
`web` component is not touched.

## Build Verification

- Command (from the project root):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Additionally, compile the test target:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run`
  The changed code is under `#[cfg(test)]`, so `cargo check` alone does not
  compile it.
- Expected: exit code 0, no errors, and no new warnings from the
  `anchor_scan` module.

## Test Verification

- Full suite:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Focused runs (the same command plus a test-name filter):
  - `anchor_locate_call`: all anchor_scan unit tests, both existing and new
  - `button_path_`: the benign-edit tests that anchor against the embedded
    `pointer_routing` source
- Tests live in the library target (`--lib`).
- Some tests unrelated to this feature are known to be flaky when run in
  parallel: the `tabs.rs` replay tests and the `tmux_sockets` discovery
  tests. If one of them fails in the full run, re-run it alone or with
  `--test-threads=1` before judging the result.
- Coverage target: not measured, because no coverage tooling is configured
  for this component. Completeness is judged by every scenario below
  mapping to a named, passing test.

### Test Scenarios from SPEC.md

Every scenario calls `locate_call(src, "probe_fn", "probe_call", 4)`. The
inputs are the ones given in SPEC.md Test Scenarios, Edge Cases and
Assumptions.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Body with `r#probe_call(w, x, y, z)` and `probe_call(a, b, c, d)` | `Err` | Unit |
| TS-2 | Body with `let r = 0..probe_call(k);` and `probe_call(a, b, c, d);` | `Err` | Unit |
| TS-3 | Body with `self.0.probe_call(x);` and `probe_call(a, b, c, d);` | `Err` | Unit |
| TS-4 | Body with `probe_call::<u8>(w, x, y, z);` and `probe_call(a, b, c, d);` | `Err` | Unit |
| TS-5 | Depth-0 definitions `fn r#probe_fn(...)` and `fn probe_fn(...)` | `Err` | Unit |
| TS-6 | Only call is `r#probe_call(a, b, c, d)` | `Ok`. Argument text is `a, b, c, d`, and the source up to `callee_end` ends with `r#probe_call` | Unit |
| TS-7 | Only call is `probe_call(1.5, 1.0f32, 1e10, 0x1F)` | `Ok`. Argument text is `1.5, 1.0f32, 1e10, 0x1F` | Unit |
| TS-8 | Existing suite: `anchor_locate_call` filter plus the `button_path_*` benign-edit tests (including `button_path_rewrapped_argument_list_edit_is_accepted` and `button_path_call_with_inserted_comment_edit_is_accepted`) | All green | Unit (regression) |
| EC-1 | Body with `let r = 0..=probe_call(k);` and `probe_call(a, b, c, d);` | `Err` (the literal ends at the first `.`) | Unit |
| EC-2 | Only call has the tuple-index argument `x.0.1` plus three plain arguments | `Ok`, 4 arguments, argument text exact | Unit |
| EC-3 | Body with `probe_call::<Vec<u8>>(w, x, y, z);` and `probe_call(a, b, c, d);` | `Err` (nested `<` / `>` balanced) | Unit |
| EC-4 | Body with `let f = probe_call::<u8>;` and `probe_call(a, b, c, d);` | `Ok`, anchored on the plain call (the non-call turbofish is not counted) | Unit |
| EC-5 | Turbofish call with block comments between its pieces, plus `probe_call(a, b, c, d);` | `Err` | Unit |
| as-1 | Depth-0 `r#fn probe_fn ...` followed by `fn probe_fn(a, b, c, d) { probe_call(a, b, c, d); }` | `Ok` with argument text `a, b, c, d` (`r#fn` is not the keyword) | Unit |
| as-2 | Only call is `probe_call::<u8>(a, b, c, d)` | `Err` (turbofish calls are counted but never anchored) | Unit |
| as-3 | Only depth-0 definition is `fn r#probe_fn(...) { probe_call(a, b, c, d); }` | `Ok` | Unit |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`.
  Expected: no formatting diff reported for
  `src-tauri/src/window_host/tests.rs`.
- Static analysis: none is configured for this component. The Build
  Verification commands must report no new warnings from `anchor_scan`.
- Header-comment check (NFR1; task owner's ticket comment):
  - `src-tauri/src/window_host/tests.rs` no longer contains
    `doc/tasks/anchor-scan-false-success/`.
  - The comment block right before `mod anchor_scan` contains
    `feature-docs/anchor-scan-false-success/tasks/task0001.md`.
- Change-scope check (NFR1). In the integrated diff against the base branch:
  - Outside `feature-docs/anchor-scan-callee-miscount/` and
    `test-docs/anchor-scan-callee-miscount/`, only
    `src-tauri/src/window_host/tests.rs` changes.
  - Within that file, the changes are limited to the `anchor_scan` module,
    its header comment and `anchor_locate_call_*` tests.
  - The `call_site_scan` module is unchanged.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 to FR5 are all implemented and tested | TS-1 to TS-7, EC-1 to EC-5 and as-1 to as-3 each pass as named unit tests |
| SC-2 | TS-1 to TS-8 all pass | Full `--lib` suite plus the focused runs above |
| SC-3 | NFR1's change scope and NFR2's green existing tests are kept | Change-scope check, header-comment check and TS-8 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-6, TS-8 |
| FR2 | task0001 | TS-2, TS-3, TS-7, TS-8, EC-1, EC-2 |
| FR3 | task0001 | TS-4, EC-3, EC-4, EC-5, as-2 |
| FR4 | task0001 | TS-5, as-1, as-3 |
| FR5 | task0001 | TS-6 |
| NFR1 | task0001 | TS-8, change-scope check, header-comment check |
| NFR2 | task0001 | TS-8 |

## E2E Testing

Not applicable. SPEC.md lists no existing E2E tests, and the change is
limited to test-only code.

## Manual Testing (E2E Not Possible)

None. The change has no user-facing behavior, no UI and no runtime path in
the shipped binary.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests (TS-1 to TS-7, EC-1 to EC-5, as-1 to as-3) | 15 | 15 | 0 | 0 |
| Regression (TS-8) | 1 | 1 | 0 | 0 |
| Code quality (format, header comment, change scope) | 3 | 3 | 0 | 0 |
