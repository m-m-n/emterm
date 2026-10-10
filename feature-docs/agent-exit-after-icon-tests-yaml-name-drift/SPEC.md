# Feature: agent-exit-after-icon-tests-yaml-name-drift

## Overview

`test-docs/agent-exit-after-icon/task0006.tests.yaml` AC-2 lists a test name
that no longer exists in the cargo `--lib` test listing. This feature replaces
that entry with the current test name, as `.claude/rules/test-docs-records.md`
requires, and adds a Rust unit test that fails if the record lists a
nonexistent `mux::upgrade::tests` name again.

## Objectives

- Make `test-docs/agent-exit-after-icon/task0006.tests.yaml` AC-2 list only
  test names that exist in the cargo `--lib` test listing, as required by
  `.claude/rules/test-docs-records.md`.
- Add a test that fails if this record lists a test name that does not exist
  again.

## Acceptance Criteria

- [ ] **AC-1** (FR1, NFR1): Line 51 of
  `test-docs/agent-exit-after-icon/task0006.tests.yaml` reads
  `      - mux::upgrade::tests::rewrite_handoff_file_replaces_an_already_written_handoff_file_at_the_same_path`,
  and that name appears as a `: test` line in the `cargo test --lib -- --list`
  output.
- [ ] **AC-2** (FR2): `git diff` of the record against the base revision shows
  exactly one changed line: line 51.
- [ ] **AC-3** (FR3): The FR3 test passes against the fixed record. It fails if
  line 51 is changed back to the old name, and it fails if any
  `mux::upgrade::tests::*` entry in the record names a function that
  `src-tauri/src/mux/upgrade/tests.rs` does not define.
- [ ] **AC-4** (NFR2, NFR3): The full
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  run reports no new failures. The only `src-tauri/src` change is the added
  test.

## Technical Requirements

### Functional Requirements

- **FR1:** Replace the stale AC-2 entry. In
  `test-docs/agent-exit-after-icon/task0006.tests.yaml`, the AC-2 `tests` entry
  `mux::upgrade::tests::rewrite_handoff_file_overwrites_an_already_written_handoff_file_in_place`
  (line 51) is replaced by
  `mux::upgrade::tests::rewrite_handoff_file_replaces_an_already_written_handoff_file_at_the_same_path`.
  The entry stays in the same list position.
- **FR2:** Keep everything else in the record unchanged. AC-2's
  `red_confirmed`, its `red_reason` text (including its old-name mention split
  over lines 57-58 and its `upgrade.rs` line references), its existing
  supersede comment (lines 45-46, which names
  `feature-docs/mux-upgrade-exited-pane-revision/SPEC.md` FR1/FR2), its other
  three `tests` entries, and every other AC block and top-level key all stay
  byte-identical. No new supersede note is added.
- **FR3:** Test that catches the problem coming back. A Rust `#[test]` in
  `src-tauri/src/mux/upgrade/tests.rs` (module `mux::upgrade::tests`) reads
  `test-docs/agent-exit-after-icon/task0006.tests.yaml` from the repository
  root (via `env!("CARGO_MANIFEST_DIR")/..`, as the `repository_file` helper in
  `src-tauri/src/mux/ipc/pty_spawn/tests/strip_open_string_body_closure.rs`
  does) and asserts:
  - (a) AC-2's `tests` list contains
    `mux::upgrade::tests::rewrite_handoff_file_replaces_an_already_written_handoff_file_at_the_same_path`;
  - (b) no `tests` list entry equals the old name;
  - (c) every `mux::upgrade::tests::<name>` entry in the record's `tests` lists
    names a `fn <name>(` that is defined in
    `src-tauri/src/mux/upgrade/tests.rs`.

  Check (c) also catches a later rename of any of the record's
  `mux::upgrade` test functions that leaves the record behind.

### Non-Functional Requirements

- **NFR1 - Resolution check:** Every name the record's AC-2 lists appears as a
  `<name>: test` line in the output of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`,
  run from the project root.
- **NFR2 - No production code change:** Nothing under `src-tauri/src` changes
  apart from adding the FR3 test to `src-tauri/src/mux/upgrade/tests.rs`. No
  existing test function is renamed or changed.
- **NFR3 - No new failures:** The full `--lib` suite reports no failures beyond
  the known flaky `tabs::tests` ts* / `welcome_without_windows_leaves_group_none`
  set (the `baseline_failures` in the record).

## Implementation Approach

### Guard test (FR3)

- Location: `src-tauri/src/mux/upgrade/tests.rs`, the module that owns the
  record's `mux::upgrade` test names.
- Reads the record through `CARGO_MANIFEST_DIR/..` and uses string checks; no
  YAML parser.
- Isolates the AC-2 block by slicing from `\n  AC-2:\n` up to the next
  `\n  AC-`.
- Checks only `tests` list entries (lines starting with `      - `), not the
  free text in `red_reason`.
- Check (c) matches record names against `fn <name>(` definitions in the
  source of `src-tauri/src/mux/upgrade/tests.rs`. It does not query the
  libtest listing at test time; the cargo `--list` resolution check (NFR1) is a
  manual or verify-phase step (TS-3).
- Runs only on Linux: `src-tauri/src/mux/upgrade/tests.rs` imports
  `std::os::unix` and builds only on Unix.

### File Structure

```
test-docs/agent-exit-after-icon/
└── task0006.tests.yaml      # FR1: line 51 replaced
src-tauri/src/mux/upgrade/
└── tests.rs                 # FR3: guard test added
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/agent-exit-after-icon-tests-yaml-name-drift/**`
- `test-docs/agent-exit-after-icon-tests-yaml-name-drift/**`

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

- [ ] **TS-1** (AC-3, unit): Run the FR3 test against the unfixed record (old
  name on line 51). It fails on the (a)/(b)/(c) checks. This is the red check.
- [ ] **TS-2** (AC-1, AC-3, unit): After FR1 is applied, the FR3 test passes.

### Command Checks

- [ ] **TS-3** (AC-1, command): Run
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
  and check that
  `mux::upgrade::tests::rewrite_handoff_file_replaces_an_already_written_handoff_file_at_the_same_path: test`
  appears, along with every other `mux::upgrade::tests` name the record lists.
- [ ] **TS-4** (AC-2, command):
  `git diff <base_revision> -- test-docs/agent-exit-after-icon/task0006.tests.yaml`
  shows only line 51 changed.
- [ ] **TS-5** (AC-4, command): Run the full `--lib` suite. Failures stay
  within the known flaky `tabs::tests` set (re-run with `--test-threads=1` if
  they flake).

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] The old name also appears in AC-2's folded `red_reason`, split across a
  line break. The guard restricts its checks to `tests` list entries so that a
  `red_reason` re-flow cannot cause a false failure or hide a real one.
- [ ] The AC-2 block is isolated by slicing from `\n  AC-2:\n` up to the next
  `\n  AC-`.
- [ ] The guard's own source contains the old name as a string literal. Check
  (c) scans `tests.rs` for `fn <name>(` definitions, so that literal does not
  create a false match.

## Assumptions

- **A1:** No supersede note is added. Commit ccfc1d65 only renamed the
  function, and the renamed test still checks that a rewrite replaces the
  handoff file at the same path. That does not reverse AC-2's expectation, so
  the supersede-note clause of `test-docs-records.md` does not apply.
- **A2:** The `red_reason` mention of the old name (lines 57-58) stays
  unchanged. `test-docs-records.md` says `red_reason` is left unchanged, and its
  scope covers only the `tests` lists. The guard test's checks therefore look
  only at `tests` list entries (lines starting with `      - `), not at the
  free text in `red_reason`.
- **A3:** The guard test goes in `src-tauri/src/mux/upgrade/tests.rs`, the
  module that owns the record's `mux::upgrade` test names. It follows the
  pattern of the `strip_open_string_body_closure.rs` precedent (reads a
  repository file through `CARGO_MANIFEST_DIR/..`, uses string checks and no
  YAML parser).
- **A4:** Check (c) matches record names against `fn <name>(` definitions in
  the source of `src-tauri/src/mux/upgrade/tests.rs`. It does not query the
  libtest listing at test time. The cargo `--list` resolution check stays a
  manual or verify-phase step (TS-3).
- **A5:** Only `test-docs/agent-exit-after-icon/task0006.tests.yaml` is in
  scope. Other test-docs records are not covered by this task.
- **A6:** `src-tauri/src/mux/upgrade/tests.rs` imports `std::os::unix` and so
  builds only on Unix. The guard test therefore runs only on Linux.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC-1 through AC-4 are satisfied

## Open Questions

None.

## References

- Test-docs record rule: `.claude/rules/test-docs-records.md`
- Record under change: `test-docs/agent-exit-after-icon/task0006.tests.yaml`
- Guard test precedent:
  `src-tauri/src/mux/ipc/pty_spawn/tests/strip_open_string_body_closure.rs`
