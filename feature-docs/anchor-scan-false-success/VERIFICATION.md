# Verification Document: anchor-scan-false-success

## Overview

**Feature**: anchor-scan-false-success / **SPEC.md**:
`feature-docs/anchor-scan-false-success/SPEC.md` / **IMPLEMENTATION.md**: not
produced (reduced tier; a single task, no file shared between tasks)

Scope: the `#[cfg(test)]` module `anchor_scan` and new unit tests in
`src-tauri/src/window_host/tests.rs`. The `web` component is not affected and
is not run.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every test passes, including the new
  `anchor_locate_call_*` tests
- Coverage target: no coverage tool is configured for this component.
  Completeness is judged by every scenario below mapping to at least one named,
  passing test.
- A failure in a test outside `anchor_scan` and the new tests is re-run once
  with the same command before it is attributed to this feature.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `fn probe_fn(a: i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d, 0); }`, expected_arg_count 4 | `Err` | Unit |
| TS-2 | `fn probe_fn(a: i32, b: i32, c: i32) { probe_call(a, b, c, "s"); }`, expected_arg_count 3 | `Err` | Unit |
| TS-3 | `fn probe_fn(a: i32, b: i32, c: i32) { probe_call(a, b, c, ')'); }`, expected_arg_count 4 | `Ok` with args text exactly `a, b, c, ')'` (SPEC also permits `Err`; the chosen handling yields `Ok`) | Unit |
| TS-4 | a `probe_call` whose arguments include the raw string `r#"contains a " quote"#` | `Ok` with args text equal to the full true argument list (SPEC also permits `Err`; the chosen handling yields `Ok`) | Unit |
| TS-5 | `fn probe_fn<'a>(a: &'a i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d); }`, expected_arg_count 4 | `Ok` with args text `a, b, c, d` | Unit |
| TS-6 | `impl Probe { fn probe_fn(&self, a: i32, b: i32, c: i32, d: i32) { probe_call(a, b, c, d); } } fn probe_fn(w: i32, x: i32, y: i32, z: i32) { probe_call(w, x, y, z); }`, expected_arg_count 4 | `Ok` with args text `w, x, y, z` (SPEC also permits `Err`; the chosen handling yields `Ok`); an `Ok` pointing at the `impl` call is a failure | Unit |
| TS-7 | Existing benign-edit tests (4), existing anchor negative tests (3), `anchor_locate_call_unbalanced_paren_inside_a_comment_still_returns_the_correct_range`, and the whole `--lib` suite | All pass | Unit (regression) |

TS-1, TS-2, TS-3 and TS-6 (SPEC AC-1, AC-2, AC-3, AC-6) must fail against the
pre-change implementation. The evidence is task0001's TDD red-phase record.

### Edge-Case Checks (SPEC edge cases, FR4)

| ID | Check | Expected Result | Test Type |
|----|-------|-----------------|-----------|
| EC-1 | Lifetimes and labels (`'a`, `'static`, `'outer:` with `break 'outer`) in an otherwise locatable source | `Ok` with the exact args text; never swallowed as a char literal | Unit |
| EC-2 | Raw identifier `r#type` as an argument | `Ok` with the exact args text; not taken for a raw string | Unit |
| EC-3 | `'\''`, `'"'`, `b'x'`, a string with an escaped `"`, byte / C strings, `br` / `cr` raw strings containing `)`, `,` or `"`, and a non-ASCII char literal as arguments | `Ok` with the exact args text; each literal counts as one argument; no panic | Unit |
| EC-4 | A raw string with no matching closing `#` count; a `'` that is neither a char literal nor a lifetime (`'-`) | `Err` | Unit |
| EC-5 | Two brace-depth-0 definitions of `probe_fn` (the first `fn /* c */ probe_fn`) | `Err` | Unit |
| EC-6 | A `<` or pipe (`\|`) token at the argument list's top level — a two-parameter closure, or the turbofish `convert::<X, Y>(b)` — where the naive comma count equals expected_arg_count | `Err` | Unit |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — exit code 0
- Static analysis: none configured beyond the build check

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR5 are implemented and tested | Functional Requirements Coverage below; every mapped scenario has a named, passing test |
| SC-2 | TS-1 to TS-7 all pass | Test command above |
| SC-3 | The diff is limited to `src-tauri/src/window_host/tests.rs`; `call_site_scan` and `Cargo.toml` are unchanged | Inspect the integration branch diff from `workflow.implement.base_commit`: apart from the SPEC's default `feature-docs/anchor-scan-false-success/**` and `test-docs/anchor-scan-false-success/**` entries, the only changed path is `src-tauri/src/window_host/tests.rs`; no hunk falls inside `mod call_site_scan`; `src-tauri/Cargo.toml` and `src-tauri/src/window_host/pointer_routing.rs` do not appear |

### SPEC Acceptance Criteria Mapping

| SPEC AC | Scenario | Verification |
|---------|----------|--------------|
| AC-1 | TS-1 | Test command |
| AC-2 | TS-2 | Test command |
| AC-3 | TS-3 | Test command |
| AC-4 | TS-4 | Test command |
| AC-5 | TS-5 | Test command |
| AC-6 | TS-6 | Test command |
| AC-7 | TS-7 | Test command |
| AC-8 | TS-7 | Test command + SC-3 diff inspection |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 (plus EC-3) |
| FR2 | task0001 | TS-3, TS-4, TS-5 (plus EC-1 to EC-4) |
| FR3 | task0001 | TS-6 (plus EC-5) |
| FR4 | task0001 | TS-3, TS-4, TS-6, TS-7 (plus EC-4 to EC-6) |
| FR5 | task0001 | TS-1 to TS-6 exist as named unit tests calling `anchor_scan::locate_call` |
| NFR1 | task0001 | TS-7 + SC-3 diff inspection |
| NFR2 | task0001 | TS-7 + SC-3 diff inspection (`mod call_site_scan` untouched) |
| NFR3 | task0001 | TS-7 + SC-3 diff inspection (`Cargo.toml` absent from the diff) |
| NFR4 | task0001 | No automated test; review confirms the scanner additions are limited to the forms listed in task0001's Design |

## Manual Testing (E2E Not Possible)

None. The change is confined to `#[cfg(test)]` code and has no runtime or UI
effect.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios (TS) | 7 | 7 | 0 | 0 |
| Edge-case checks (EC) | 6 | 6 | 0 | 0 |
| Code quality (format) | 1 | 1 | 0 | 0 |
| Diff scope (SC-3, NFR4 review) | 2 | 0 | 0 | 2 |
