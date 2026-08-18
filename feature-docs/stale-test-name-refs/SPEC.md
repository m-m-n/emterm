# Feature: stale-test-name-refs

> **Identifier notation used in this document**
> The old test identifier is written as `OLD_ID`, the new one as `NEW_ID`.
> Spelling `OLD_ID` out in full inside this document would break FR5 / AC-2
> (a repository-wide grep for the old identifier must return exactly the 6
> carve-out occurrences), so `OLD_ID` is only ever shown in concatenated form.
>
> - `OLD_ID` = `test_relocate_widened_base_via_wrap_` + `no_panic_when_column_one_does_not_exist`
> - `NEW_ID` = `test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`

Requirements document: `feature-docs/stale-test-name-refs/REQUIREMENTS.md`.

## Overview

The relocate-wrap-cursor-clamp rename changed a `crates/term_core` test from
`OLD_ID` to `NEW_ID`, but eight documentation records still reference the old
identifier. This feature replaces the stale identifier in those eight records
while leaving the three relocate-wrap-cursor-clamp records that document the
rename itself verbatim. It is a documentation-only, string-only change: no
test body, no implementation, and no schema is touched.

## Objectives

- Restore the regression-checking value of the machine-readable
  `acceptance_tests[].tests` lists in `test-docs/*/taskNNNN.tests.yaml`: a stale
  identifier makes `cargo test <old name>` match 0 tests and exit 0, so a broken
  invariant passes as silently green.
- Keep the historical records of prior features (relocate-wrap-overflow-cleanup,
  relocate-wrap-ec1-scroll-test) pointing at the test that actually guards their
  invariants after the relocate-wrap-cursor-clamp rename.
- Preserve the audit trail of the rename itself: the three
  relocate-wrap-cursor-clamp records that document the old -> new rename keep the
  old identifier verbatim.

## User Stories

### US1: Running a recorded test identifier actually exercises the invariant

As a developer or agent, I want the identifiers in
`test-docs/*/taskNNNN.tests.yaml` to resolve to real tests, so that running them
reports the true state of the invariant instead of a 0-match filter that exits 0.

**Acceptance Criteria:**
- [ ] AC-1: the four occurrences named in the original report carry `NEW_ID`.
- [ ] AC-3: a filtered cargo test run of `NEW_ID` reports at least 1 test run.

### US2: The rename's own audit trail stays coherent

As a developer or agent reading the relocate-wrap-cursor-clamp records, I want
the before/after pair of the rename to stay readable, so that the review finding
that instructed the replacement does not contradict itself.

**Acceptance Criteria:**
- [ ] AC-2: a repository-wide grep for `OLD_ID` returns exactly the 6 carve-out
      occurrences in the three relocate-wrap-cursor-clamp files, and zero
      occurrences outside them.
- [ ] AC-5: `git diff --stat` lists exactly the 8 files of FR1..FR3 and no others.

## Technical Requirements

### Functional Requirements

- **FR1 — Update the machine-readable test lists in test-docs:** Replace `OLD_ID`
  with `NEW_ID` in each of:
  `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` (1 occurrence),
  `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` (1),
  `test-docs/relocate-wrap-ec1-scroll-test/task0002.tests.yaml` (1).
- **FR2 — Update the VERIFICATION.md regression reference:** Replace the same
  identifier at `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md`
  (1 occurrence, the TS-3 regression-confirmation target).
- **FR3 — Update the remaining relocate-wrap-ec1-scroll-test records:** Replace
  the same identifier in the four non-enumerated relocate-wrap-ec1-scroll-test
  documents that carry the same regression-reference class as FR2:
  `feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md` (2),
  `feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md` (2),
  `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0001.md` (2),
  `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0002.md` (2).
- **FR4 — String-only edit:** Each edit changes only the test-identifier string.
  Surrounding YAML structure, Markdown prose, line ordering, and every other
  identifier stay byte-identical. No test body, no implementation, and no schema
  is modified.
- **FR5 — Repository-wide sweep with an explicit rename-record carve-out:** After
  the change, the only remaining repository-wide occurrences of `OLD_ID` are the
  6 occurrences inside the three records that document the rename itself, which
  stay verbatim: `feature-docs/relocate-wrap-cursor-clamp/SPEC.md` (2),
  `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md` (2),
  `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml` (2). Every
  occurrence outside those three files is replaced. Total at the base revision:
  18 occurrences across 11 files — 8 files to edit (12 occurrences) plus 3
  carve-out files (6).
- **FR6 — New identifier resolves to a real test:** `NEW_ID` matches at least one
  test when run through the project's cargo test invocation — never a 0-match
  filter that exits 0.
- **FR7 — Existing test suite stays green:** The existing `crates/term_core` test
  suite continues to pass unchanged after the edits.

### Non-Functional Requirements

- **NFR1 — Scope confinement to documentation records:** No file under `crates/`,
  `src-tauri/src/`, `src-tauri/tests/`, or any build configuration is modified.
  The change set is limited to the 8 files listed in FR1 through FR3.
- **NFR2 — Audit-trail integrity:** The rename's own record set stays readable as
  a before/after pair. In particular
  `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml` is the finding
  that instructs this very replacement; rewriting its "before" side would make
  the record self-contradictory.
- **NFR3 — Out of scope:** Mechanizing schema validation of
  `test-docs/*/taskNNNN.tests.yaml`, and any change to test logic, are explicitly
  out of scope.

## Implementation Approach

### Architecture

No runtime component is involved. The change is a textual replacement applied to
eight documentation records. The design step is skipped: this is a
documentation-only identifier replacement across 8 records, with no user-facing
surface, no UI, no new module, no data model, and no interface between
components — there is nothing for a design step to decide.

### Edit map

| # | File | Occurrences | Requirement |
|---|------|-------------|-------------|
| 1 | `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` | 1 | FR1 |
| 2 | `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` | 1 | FR1 |
| 3 | `test-docs/relocate-wrap-ec1-scroll-test/task0002.tests.yaml` | 1 | FR1 |
| 4 | `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md` | 1 | FR2 |
| 5 | `feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md` | 2 | FR3 |
| 6 | `feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md` | 2 | FR3 |
| 7 | `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0001.md` | 2 | FR3 |
| 8 | `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0002.md` | 2 | FR3 |

### Carve-out map (left verbatim)

| File | Occurrences | Requirement |
|------|-------------|-------------|
| `feature-docs/relocate-wrap-cursor-clamp/SPEC.md` | 2 | FR5, NFR2 |
| `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md` | 2 | FR5, NFR2 |
| `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml` | 2 | FR5, NFR2 |

### API Design / Database Schema

Not applicable — no API surface and no data store is involved.

### Dependencies

**Internal Dependencies:**
- `crates/term_core` test source: already contains `NEW_ID` at the base revision
  (assumption A-3); this feature reads it only as the resolution target of FR6.
- relocate-wrap-cursor-clamp records: carve-out inputs that must stay unchanged.

**External Dependencies:**
- None.

### File Structure

```
test-docs/
├── relocate-wrap-overflow-cleanup/
│   └── task0001.tests.yaml
└── relocate-wrap-ec1-scroll-test/
    ├── task0001.tests.yaml
    └── task0002.tests.yaml
feature-docs/
└── relocate-wrap-ec1-scroll-test/
    ├── VERIFICATION.md
    ├── REQUIREMENTS.md
    ├── SPEC.md
    └── tasks/
        ├── task0001.md
        └── task0002.md
```

## Declared Change Set

Feature-specific paths:

- `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml`
- `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml`
- `test-docs/relocate-wrap-ec1-scroll-test/task0002.tests.yaml`
- `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0001.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0002.md`

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths above:

- `feature-docs/stale-test-name-refs/**`
- `test-docs/stale-test-name-refs/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`, `workflow.yaml`,
`phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`,
`retrospect.yaml`, and the design artifacts the design step produces. These are
generated and owned by the phase documents and by `references/phase-state.md`;
this section cites them and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is a
deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it.

## Test Scenarios

### TS-1 — Old-name sweep (manual-command)

Grep for `OLD_ID` from the repository root and confirm the result is exactly the
6 carve-out occurrences in the three relocate-wrap-cursor-clamp files.
Verifies AC-2 → FR3, FR5.

### TS-2 — New-name presence (manual-command)

Grep for `NEW_ID` and confirm each of the 8 edited files carries it at the
expected occurrence count (1, 1, 1, 1, 2, 2, 2, 2).
Verifies AC-1, AC-2 → FR1, FR2, FR3, FR5.

### TS-3 — Filtered cargo run of the new identifier (cargo-test)

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib \
  test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist
```

reports at least 1 test executed (not `0 passed; 0 failed; N filtered out`).
Verifies AC-3 → FR6.

### TS-4 — Full term_core suite (cargo-test)

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib
```

passes with no new failures. Verifies AC-4 → FR7.

### TS-5 — Diff scope (manual-command)

`git diff --stat` shows exactly the 8 expected paths, and the diff contains only
identifier-string changes. Verifies AC-5 → FR4, NFR1.

### Edge Cases

- A bulk replacement must not touch the three carve-out files; TS-1 is the check
  that catches it.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Performance Tests

Not applicable.

## Security Considerations

Not applicable — documentation-only string replacement with no runtime,
authentication, input-handling, or data-storage surface.

## Error Handling

Not applicable — no runtime code path is added or changed.

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] AC-1 (FR1, FR2): the four originally reported occurrences carry `NEW_ID`.
- [ ] AC-2 (FR3, FR5): repository-wide grep for `OLD_ID` returns exactly 6
      matches, all inside the three relocate-wrap-cursor-clamp carve-out files.
- [ ] AC-3 (FR6): the filtered cargo test run of `NEW_ID` reports at least 1 test
      run.
- [ ] AC-4 (FR7): the full `crates/term_core --lib` suite passes with no new
      failures.
- [ ] AC-5 (FR4, NFR1): `git diff --stat` lists exactly the 8 files of FR1..FR3
      and no others; the diff contains only identifier-string changes.

## Assumptions

- **A-1**: The repository-wide sweep excludes the three relocate-wrap-cursor-clamp
  records that document the rename itself (SPEC.md, REQUIREMENTS.md,
  reviews/round1.yaml — 6 occurrences total), which stay verbatim. This resolves
  the task description's unqualified acceptance criterion
  「リポジトリ全体を grep して旧名の参照が 1 件も残っていない」, which is
  unsatisfiable as literally written: round1.yaml is the finding that instructs
  the old -> new replacement, so the old name must survive there for the record to
  stay coherent. (Source: answer to question `requirement.repo-wide-grep-scope`,
  option `sweep_except_history` — batch resolution; Codex suggested
  `four_files_only`, the orchestrator adopted `sweep_except_history` because
  `four_files_only` leaves AC-2 permanently unsatisfiable.)
- **A-2**: The occurrence set is the orchestrator-verified ground truth at base
  revision `688840b0a68f4d73cae34350089e23c437d86713`: 18 occurrences across 11
  files (8 edit + 3 carve-out). The task description's enumeration of 4
  occurrences is a subset; the delivered scope is the verified set. (Source:
  orchestrator-supplied ground truth.)
- **A-3**: PR #45 (relocate-wrap-cursor-clamp) is merged at the base revision, so
  the new identifier already exists in the `crates/term_core` test source and no
  test-source change is needed to make AC-3 pass. (Source: task description
  制約・前提 section, consistent with the base revision.)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every functional requirement is `resolved`.

## References

- Requirements document: `feature-docs/stale-test-name-refs/REQUIREMENTS.md`
- Rename finding: `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml`
- Rename records: `feature-docs/relocate-wrap-cursor-clamp/SPEC.md`,
  `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md`
