# Feature: ac4-ac6-stale-line-reference

## Overview

`test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` describes the
AC-4 and AC-6 failures in terms of line 523 of
`crates/term_core/src/ring_buffer/tests.rs`, in the present tense; that line
no longer holds the survivor-row assertion. This feature corrects the two
prose passages so a reader re-running the AC-4 / AC-6 mutation is directed to
the survivor-row assertion rather than to unrelated fixture or comment lines,
while leaving the verbatim `cargo test` transcript byte-identical. It is a
documentation-only change: no Rust source and no test code is touched.

Requirements source: `feature-docs/ac4-ac6-stale-line-reference/REQUIREMENTS.md`.

## Objectives

- Restore the accuracy of the ring-push-blank-row-scope test record so that
  anyone re-running the AC-4 / AC-6 mutation is directed to the survivor-row
  assertion rather than to unrelated fixture or comment lines.
- Complete the consistency pass that feature
  `ring-push-blank-note-unconditional` began (it corrected AC-5 but left this
  reference drift behind).
- Choose a phrasing that cannot go stale again the next time
  `crates/term_core/src/ring_buffer/tests.rs` shifts.

## User Stories

### US1: Re-run the AC-4 / AC-6 mutation from the record

As a developer re-running the AC-4 / AC-6 mutation, I want the test record to
point at the survivor-row assertion, so that I do not land on an unrelated
fixture or comment line.

**Acceptance Criteria:**
- [ ] AC-6's closing sentence no longer claims in the present tense that line
      523 is a survivor-row assertion; it either labels 523 as the line at the
      time of that run, or names the assertion without a line number.
- [ ] AC-4's `crates/term_core/src/ring_buffer/tests.rs:523:5` reference is
      corrected the same way.
- [ ] Where the assertion is identified by expression, the text distinguishes
      the post-scroll survival assertion from the identical pre-scroll
      anti-vacuity assertion.
- [ ] Where a current line number is cited, it matches the file at the time of
      the edit (606 at base_revision 8c6e2e1d).

### US2: Keep the record trustworthy as historical evidence

As a reader of the completed test record, I want the correction to preserve
what actually happened and the surrounding structure, so that the record stays
usable as evidence of the past run.

**Acceptance Criteria:**
- [ ] Lines 69-75 of the file (the AC-6 `cargo test` transcript, including the
      `panicked at ...:523:5` line) are byte-identical to their pre-change
      content.
- [ ] The file parses as valid YAML, and `task_id`, `baseline_failures`,
      `final_failures`, AC-1, AC-2, AC-3, AC-5, AC-7 and every `tests:` /
      `red_confirmed:` field are unchanged.

## Technical Requirements

### Functional Requirements

- **FR1 - Correct AC-6's closing prose:** In
  `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`, AC-6's
  closing sentence (currently lines 77-78: `The failing line (523) is a
  survivor-row assertion, matching the requirement that the failure not land on
  a recycled-row assertion.`) must stop asserting in the present tense that
  line 523 of `crates/term_core/src/ring_buffer/tests.rs` is a survivor-row
  assertion. It must either mark 523 as the line number at the time of that
  run, or identify the assertion without a line number.
- **FR2 - Correct AC-4's prose:** AC-4's `red_reason` prose (currently lines
  35-44, with the location on line 42: `at
  crates/term_core/src/ring_buffer/tests.rs:523:5.`) must be corrected the same
  way as FR1. The task explicitly classifies this occurrence as prose to fix,
  not as protected transcript.
- **FR3 - Leave the verbatim transcript untouched:** The AC-6 `cargo test`
  transcript block (lines 69-75, including `thread '...' (2553229) panicked at
  crates/term_core/src/ring_buffer/tests.rs:523:5:` on line 72) must be
  byte-identical before and after the change.
- **FR4 - Disambiguate the assertion when identifying it by expression:** If
  the replacement prose identifies the assertion by its expression
  (`core.overflow.contains_key(&(0u32, abs1))`), it must qualify it as the
  post-scroll survival assertion. That exact expression appears twice in the
  file — line 558 (pre-scroll anti-vacuity guard) and line 606 (post-scroll
  survival).
- **FR5 - Any newly written line number must be verified against the file at
  edit time:** If the fix cites a current line number, that number must be
  re-derived from `crates/term_core/src/ring_buffer/tests.rs` at implementation
  time rather than taken from the task description. At base_revision 8c6e2e1d
  the post-scroll survivor assertion is at line 606.
- **FR6 - Preserve YAML validity and surrounding structure:**
  `task0001.tests.yaml` must remain valid YAML, with its key structure, its
  other acceptance-criteria entries (AC-1, AC-2, AC-3, AC-5, AC-7), `task_id`,
  `baseline_failures`, `final_failures`, and every `tests:` / `red_confirmed:`
  field unchanged. Edits are confined to the folded-scalar prose of AC-4 and
  AC-6, preserving the existing 6-space continuation indentation.
- **FR7 - Scope is documentation only:** No Rust source, no test code, and no
  other file is modified. `crates/term_core/src/ring_buffer/tests.rs` is
  read-only input for this feature.

### Non-Functional Requirements

- **NFR1 - Durability against future line drift:** The chosen phrasing must not
  require re-editing the next time `tests.rs` shifts. Anchoring on the
  assertion expression plus an explicitly historical line number satisfies
  this; a bare current line number does not.
- **NFR2 - Historical fidelity of the record:** The record is evidence of a
  past run. Corrections must not rewrite what happened — 523 was the true
  failure location at the time, so it stays present in the text, relabelled as
  historical rather than deleted.
- **NFR3 - Diff minimality:** The change touches only the two prose passages.
  Reflowing unrelated lines of the folded scalars, or reformatting the file, is
  out of scope.

## Implementation Approach

### Architecture

No runtime architecture is involved. The change set is a single YAML document
under `test-docs/`, edited as text.

**Component Diagram:**
```
test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml   (edited)
  ├── AC-4 red_reason folded scalar (lines 35-44)  -> FR2
  ├── AC-6 cargo test transcript    (lines 69-75)  -> FR3 (byte-identical)
  └── AC-6 closing sentence         (lines 77-78)  -> FR1

crates/term_core/src/ring_buffer/tests.rs                      (read-only, FR7)
  ├── line 558  core.overflow.contains_key(&(0u32, abs1))  pre-scroll anti-vacuity guard
  └── line 606  core.overflow.contains_key(&(0u32, abs1))  post-scroll survival
```

### Data Flow

```
crates/term_core/src/ring_buffer/tests.rs  → re-derive current line number (FR5)
                                           → qualify assertion as post-scroll survival (FR4)
                                           → rewrite AC-4 / AC-6 prose (FR1, FR2)
                                           → task0001.tests.yaml (valid YAML, FR6; minimal diff, NFR3)
```

### API Design

Not applicable. This feature exposes no API.

### Database Schema

Not applicable. This feature touches no database.

### Dependencies

**Internal Dependencies:**
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`: the file
  being corrected.
- `crates/term_core/src/ring_buffer/tests.rs`: read-only source of the
  assertion expression and its current line number (FR5, FR7).
- Feature `ring-push-blank-note-unconditional`: corrected AC-5 and left this
  reference drift behind; this feature completes that pass.

**External Dependencies:**
- A YAML parser, for the validity check in TS-1. No new runtime dependency is
  introduced.

### File Structure

```
test-docs/
└── ring-push-blank-row-scope-test/
    └── task0001.tests.yaml      # AC-4 and AC-6 prose corrected
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`). The only feature-specific path
this feature changes is
`test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`.

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/ac4-ac6-stale-line-reference/**`
- `test-docs/ac4-ac6-stale-line-reference/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (FR6): Parse
      `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` with a
      YAML parser and confirm it loads, and that the top-level keys and the
      AC-1..AC-7 key set are unchanged.

### Integration Tests

- [ ] **TS-2** (FR3, FR6, NFR3): Diff the changed file against its pre-change
      version and confirm the hunks fall only inside AC-4's and AC-6's
      `red_reason` prose, with the lines-69-75 transcript region untouched.
- [ ] **TS-3** (FR1): Grep the changed file for `The failing line (523) is a
      survivor-row assertion` and confirm the bare present-tense form is gone.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases

- [ ] **TS-4** (FR1, FR2, NFR2): Grep the changed file for 523 and confirm
      every surviving occurrence is either inside the protected transcript or
      explicitly qualified as historical.
- [ ] **TS-5** (FR4, FR5, NFR1): Open
      `crates/term_core/src/ring_buffer/tests.rs` and confirm the post-scroll
      survivor assertion is where the new prose says it is (line 606 at
      base_revision 8c6e2e1d), and that line 558 holds the identical pre-scroll
      assertion.

### Performance Tests

- [ ] **TS-6** (FR7): No Rust build or test run is required: this feature
      changes no compiled code.

## Security Considerations

- **Authentication:** Not applicable.
- **Authorization:** Not applicable.
- **Input Validation:** The edited file must remain valid YAML (FR6, verified
  by TS-1).
- **Data Protection:** Not applicable.
- **XSS Prevention:** Not applicable.
- **SQL Injection Prevention:** Not applicable.
- **CSRF Protection:** Not applicable.

## Error Handling

### Error Codes

Not applicable. This feature introduces no runtime error paths.

### Error Flow

```
Edit deviates from FR3 / FR6 (transcript or structure changed)
  → detected by TS-1 / TS-2 → revert and re-confine the edit to the
    AC-4 / AC-6 folded-scalar prose
```

## Performance Optimization

### Performance Goals

Not applicable. This feature changes no compiled code (FR7).

### Optimization Strategies

Not applicable.

### Caching Strategy

Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1-FR7) are implemented.
- [ ] All non-functional requirements (NFR1-NFR3) are satisfied.
- [ ] All test scenarios (TS-1 through TS-6) pass.
- [ ] AC-6's closing sentence no longer claims in the present tense that line
      523 is a survivor-row assertion; it either labels 523 as the line at the
      time of that run, or names the assertion without a line number.
- [ ] AC-4's `crates/term_core/src/ring_buffer/tests.rs:523:5` reference is
      corrected the same way.
- [ ] Lines 69-75 of the file are byte-identical to their pre-change content.
- [ ] The file parses as valid YAML, and `task_id`, `baseline_failures`,
      `final_failures`, AC-1, AC-2, AC-3, AC-5, AC-7 and every `tests:` /
      `red_confirmed:` field are unchanged.
- [ ] Where the assertion is identified by expression, the text distinguishes
      the post-scroll survival assertion from the identical pre-scroll
      anti-vacuity assertion.
- [ ] Where a current line number is cited, it matches the file at the time of
      the edit (606 at base_revision 8c6e2e1d).

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional and non-functional requirement (FR1-FR7, NFR1-NFR3) is
`resolved`.

## Implementation Phases (if applicable)

Not applicable. The change is a single documentation edit.

## References

- Requirements document: `feature-docs/ac4-ac6-stale-line-reference/REQUIREMENTS.md`
- Corrected file: `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`
- Read-only reference: `crates/term_core/src/ring_buffer/tests.rs` (line 558
  pre-scroll anti-vacuity guard, line 606 post-scroll survival, at
  base_revision 8c6e2e1d)
- Prior feature that corrected AC-5: `ring-push-blank-note-unconditional`
