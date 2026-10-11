# Feature: program-status-ac8-block-comment-carry-test

## Overview

Add self-tests to the ac8 allowlist checker in `src-tauri/src/program_status/tests.rs`
that fail when `ac8_scan_attribute` drops the block-comment level carried to the next
line. Update the AC-3 entry of the predecessor test record
`test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml` so it names the
tests that actually guard against that regression.

## Objectives

- The ac8 allowlist checker self-tests in `src-tauri/src/program_status/tests.rs`
  include tests that fail when `ac8_scan_attribute` drops the block-comment level
  carried to the next line (mutant:
  `Ac8AttributeScan::Open(Ac8OpenAttribute { depth, in_string, block_comment: 0 })`).
- The AC-3 entry of `test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml`
  names the tests that actually guard against that regression.

## Technical Requirements

### Functional Requirements

- **FR1: Bracket carry test** — A self-test feeds the following source to
  `ac8_check_allowlist`:

  ```text
  #[cfg(all( /* ]
  ] */ unix))] use winit::event::Event;
  ```

  and requires a violation reporting use root `winit` that quotes the second line
  (`] */ unix))] use winit::event::Event;`). With the current implementation the
  source returns exactly that one violation; with the carry-dropping mutant the second
  line's leading `]` closes the attribute, the rest is not a use line, and no violation
  is returned.

- **FR2: Quote carry test** — A self-test feeds the following source to
  `ac8_check_allowlist`:

  ```text
  #[allow(x /*
  " */)]
  use winit::event::Event;
  ```

  and requires a violation reporting use root `winit` that quotes
  `use winit::event::Event;`, and no end-of-source violation. With the current
  implementation the source returns exactly that one violation; with the
  carry-dropping mutant the `"` on line 2 opens a string literal that never closes, so
  the only violation is the end-of-source violation for `#[allow(x /*` and no winit
  violation is returned.

- **FR3: Predecessor test record AC-3** — In
  `test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml`, AC-3 `tests`
  lists the FR1 and FR2 test names in addition to its current three entries, and AC-3
  `red_reason` names the FR1 and FR2 tests as the regression guard against an
  implementation that does not carry the block-comment level, replacing the statement
  that the line-spanning test is that guard (that test passes under the mutant: line
  2's `]` closes the attribute and line 3 is still a winit use line). The rest of the
  `red_reason` text about the nested-comment and `/*/` tests and `red_confirmed` stay
  as they are.

- **FR4: Existing behavior unchanged** — The ac8 checker code (`Ac8OpenAttribute`,
  `ac8_scan_attribute`, `ac8_after_leading_attributes`, `ac8_check_allowlist`,
  `ac8_violation`, the allowlist constants) and `src-tauri/src/program_status.rs` are
  not changed. Every existing `ac8_*` test,
  `ac8_comments_ac3_a_bracket_in_a_block_comment_spanning_lines_does_not_hide_a_use_line`
  and
  `ac8_comments_ac5_a_source_ending_inside_a_block_comment_returns_one_end_of_source_violation`
  included, keeps its name, input and expected result.

### Non-Functional Requirements

- **NFR1: Change set limit** — Outside this feature's own feature-docs and test-docs,
  the change set is `src-tauri/src/program_status/tests.rs` and
  `test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml` only.
  `feature-docs/program-status-ac8-attribute-comments/tasks/task0001.md` (Test Notes)
  is not edited.
- **NFR2: Test name resolution** — Each test name added to the predecessor record
  appears as a `<name>: test` line in the output of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
  run from the project root (`.claude/rules/test-docs-records.md` resolution check).
- **NFR3: Cargo invocation** — Every cargo command runs from the project root with
  `CARGO_TARGET_DIR=src-tauri/target` and `--manifest-path src-tauri/Cargo.toml`.

## Acceptance Criteria

- [ ] **AC-1** (FR1): The FR1 test passes on the current implementation and fails when
  the end-of-text return of `ac8_scan_attribute` is temporarily changed to
  `Ac8AttributeScan::Open(Ac8OpenAttribute { depth, in_string, block_comment: 0 })`.
- [ ] **AC-2** (FR2): The FR2 test passes on the current implementation and fails under
  the same temporary mutant.
- [ ] **AC-3** (FR3, NFR2): AC-3 of
  `test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml` lists the FR1
  and FR2 test names (each resolving in the `--lib -- --list` output) and its
  `red_reason` names them as the guard against a dropped block-comment carry, without
  claiming the line-spanning test is that guard.
- [ ] **AC-4** (FR4, NFR1, NFR3):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  passes with every existing ac8 test unchanged in name, input and expected result; the
  mutant is not part of the final diff; the change set outside this feature's own docs
  is limited per NFR1.

## Implementation Approach

- The new tests go in a new section after the program-status-ac8-attribute-comments
  section, with a section header comment in the style of the existing ac8 sections and
  names carrying this feature's acceptance criterion (prefix such as `ac8_carry_acN_`);
  the existing assertion helpers are reused. (A1)
- The existing line-spanning AC-3 test and the AC-5 block-comment end-of-source test
  stay as they are and stay listed in the predecessor record. (A2)
- The mutant check (AC-1/AC-2) is done by a temporary local edit that is reverted
  before commit; it is not added as a permanent test mechanism. (A3)
- The predecessor's Test Notes prose
  (`feature-docs/program-status-ac8-attribute-comments/tasks/task0001.md`) is out of
  scope. (A4)

Design step: skipped (test-only change in `src-tauri/src/program_status/tests.rs` and a
test-docs record; no UI change).

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths:

- `feature-docs/program-status-ac8-block-comment-carry-test/**`
- `test-docs/program-status-ac8-block-comment-carry-test/**`

`feature-docs/program-status-ac8-block-comment-carry-test/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/program-status-ac8-block-comment-carry-test/**` covers
`test-docs/program-status-ac8-block-comment-carry-test/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/program-status-ac8-block-comment-carry-test/` directory at all; the declared
`test-docs/program-status-ac8-block-comment-carry-test/**` entry is still correct in
that case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (AC-1): Run the FR1 test on the current implementation (pass), then with
  the mutant applied temporarily (fail with no violation returned), then revert the
  mutant.
- [ ] **TS-2** (AC-2): Run the FR2 test on the current implementation (pass), then with
  the mutant applied temporarily (fail: only the end-of-source violation, no winit
  violation), then revert the mutant.

### Record Check

- [ ] **TS-3** (AC-3): Run the `--lib -- --list` command and confirm each name added to
  AC-3 appears as a test; read the AC-3 `red_reason`.

### Unit + Diff

- [ ] **TS-4** (AC-4): Full `--lib` run passes; the branch diff shows only `tests.rs`
  (new tests added, existing tests and checker code untouched) and the test-docs
  records.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

## Assumptions

- **A1** (reversible): The new tests go in a new section after the
  program-status-ac8-attribute-comments section, with a section header comment in the
  style of the existing ac8 sections and names carrying this feature's acceptance
  criterion (prefix such as `ac8_carry_acN_`); the existing assertion helpers are
  reused.
- **A2** (reversible): The existing line-spanning AC-3 test and the AC-5 block-comment
  end-of-source test stay as they are and stay listed in the predecessor record (they
  still verify their own criteria).
- **A3** (reversible): The mutant check (AC-1/AC-2) is done by a temporary local edit
  that is reverted before commit; it is not added as a permanent test mechanism.
- **A4** (reversible): The predecessor's Test Notes prose
  (`feature-docs/program-status-ac8-attribute-comments/tasks/task0001.md`) is out of
  scope: the task's constraint limits changes to `tests.rs` and its test-docs record,
  and `.claude/rules/test-docs-records.md` places predecessor feature-docs prose outside
  the record-update rule.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass

## Open Questions

None. No requirement has `status: tbd`.

## References

- `src-tauri/src/program_status/tests.rs`
- `test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml`
- `.claude/rules/test-docs-records.md`
