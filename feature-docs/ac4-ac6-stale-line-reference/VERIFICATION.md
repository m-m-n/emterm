# Verification Document: ac4-ac6-stale-line-reference

## Overview

**Feature**: ac4-ac6-stale-line-reference /
**SPEC.md**: `feature-docs/ac4-ac6-stale-line-reference/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/ac4-ac6-stale-line-reference/IMPLEMENTATION.md`

This feature changes one YAML documentation record and no compiled code (FR7),
so the integrated verification is document-level: parse, diff-region, and
reference-accuracy checks. The project's build and test commands are recorded
below and act as unchanged-baseline guards, not as evidence of the change.

Two reading rules apply to every check below and come from IMPLEMENTATION.md:

- **D4**: checks about prose run against the **YAML-parsed** value of the
  record's `red_reason` fields. Both passages are folded scalars, so a
  sentence is wrapped across physical lines and a raw line-oriented search for
  it matches nothing even when it is present.
- **D2**: the protected transcript is a **content region**, not fixed line
  positions. Its lines and their order must be byte-identical; its absolute
  line numbers may shift if the record's AC-4 entry changes line count.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors — and identical to the pre-change result,
  since no compiled file is touched. Running it is optional evidence
  (see TS-6); a failure here indicates the change escaped its declared scope.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Coverage target: not applicable. No compiled code changes, so this feature
  neither adds nor moves test coverage; the target is "unchanged from the
  pre-change baseline".

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Load `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` with a YAML parser available in the environment and compare its key structure against the pre-change version | Parses without error; top-level keys, the AC-1..AC-7 key set, `task_id`, `baseline_failures`, `final_failures`, and every `tests:` / `red_confirmed:` field are unchanged | Unit |
| TS-2 | Diff the changed file against its pre-change version (the content at the implement step's base commit) | Every hunk falls inside the record's AC-4 or AC-6 `red_reason` folded scalar; the `cargo test` transcript block's lines are unchanged byte-for-byte, in order; the 6-space continuation indentation is preserved; no unrelated line is reflowed | Integration |
| TS-3 | Search the parsed value of the record's AC-6 `red_reason` for the bare present-tense claim that the failing line (523) is a survivor-row assertion | The bare present-tense form is absent | Integration |
| TS-4 | Enumerate every occurrence of 523 in the changed file and classify each one | Every occurrence is either inside the protected transcript or explicitly qualified as the value observed at the time of that run; no occurrence remains an unqualified present-tense claim about the file's current state; at least one occurrence survives (the record keeps its historical evidence) | Integration |
| TS-5 | Open `crates/term_core/src/ring_buffer/tests.rs` in the working tree and locate both occurrences of the survivor assertion expression | The post-scroll survival assertion is where the corrected prose says it is (line 606 at base_revision 8c6e2e1d — re-derive rather than assume), and the earlier identical occurrence (line 558 at that revision) is the pre-scroll anti-vacuity guard; the corrected prose's qualifier picks out the post-scroll one | Integration |
| TS-6 | Confirm no compiled code changed | The change set contains no Rust source or test file; a Rust build or test run is therefore not required as evidence, and if run must be unchanged from baseline | Integration |

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` —
  expected clean, and expected unaffected: no Rust file is in the change set.
- Static analysis: none applies to this change. The document-level equivalent
  is TS-1 (the file remains valid YAML).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | The record's AC-6 closing sentence no longer claims in the present tense that line 523 is a survivor-row assertion | TS-3, TS-4 |
| SC-2 | The record's AC-4 `…/ring_buffer/tests.rs:523:5` reference is corrected the same way | TS-4 |
| SC-3 | The `cargo test` transcript block is byte-identical to its pre-change content | TS-2 |
| SC-4 | The file parses as valid YAML with `task_id`, `baseline_failures`, `final_failures`, the record's AC-1, AC-2, AC-3, AC-5, AC-7 entries and every `tests:` / `red_confirmed:` field unchanged | TS-1 |
| SC-5 | Where the assertion is identified by expression, the post-scroll survival assertion is distinguished from the identical pre-scroll anti-vacuity guard | TS-5 |
| SC-6 | Where a current line number is cited, it matches the file at edit time | TS-5 |
| SC-7 | No Rust source, no test code and no other file is modified | TS-2, TS-6 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-3, TS-4 |
| FR2 | task0001 | TS-4 |
| FR3 | task0001 | TS-2 |
| FR4 | task0001 | TS-5 |
| FR5 | task0001 | TS-5 |
| FR6 | task0001 | TS-1, TS-2 |
| FR7 | task0001 | TS-6 |
| NFR1 | task0001 | TS-5 |
| NFR2 | task0001 | TS-4 |
| NFR3 | task0001 | TS-2 |

## E2E Testing

The project registers no E2E command for the `term_core` component
(`e2e_test_command` is empty), and a documentation record has no end-to-end
surface. No E2E scenario applies to this feature.

## Manual Testing (E2E Not Possible)

- [ ] Read the corrected AC-4 and AC-6 passages end to end and confirm they
      read as a coherent account of the past run rather than as a patched
      sentence — the record's value is that a later reader trusts it.
- [ ] Follow the corrected prose as a first-time reader would: locate the
      post-scroll survival assertion in
      `crates/term_core/src/ring_buffer/tests.rs` using only what the prose
      says, and confirm arrival at that assertion and not at the pre-scroll
      guard, a fixture line, or a comment line.
- [ ] Confirm the phrasing would still be correct if the assertion moved
      again — the NFR1 durability claim is a judgment about wording that no
      mechanical check can make.

## Performance / Security Verification (if applicable)

Not applicable. This feature changes no compiled code, adds no dependency,
introduces no input-processing path, and alters no runtime behavior. The only
input-validation concern is that the edited file remains parseable, covered by
TS-1.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios (TS-1..TS-6) | 6 | 6 | 0 | 0 |
| Success criteria (SC-1..SC-7) | 7 | 7 | 0 | 0 |
| Build / format guards | 3 | 3 | 0 | 0 |
| Judgment checks | 3 | 0 | 0 | 3 |
