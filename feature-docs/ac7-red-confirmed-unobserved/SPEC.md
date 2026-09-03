# Feature: ac7-red-confirmed-unobserved

## Overview

The AC-7 entry in `test-docs/stale-test-name-refs/task0001.tests.yaml`
records `red_confirmed: true` for a criterion that is an invariant guard on
writing discipline — a criterion with no observable pre-state, because the
task that the record documents is the very thing that creates the record.
This feature flips that value to `false`, rewrites AC-7's `red_reason` to
state the invariant-guard rationale the way AC-6 already does, and appends
one line to the record's trailing `notes` block noting AC-7 as an
unconfirmed red.

Requirements document: `feature-docs/ac7-red-confirmed-unobserved/REQUIREMENTS.md`.

## Objectives

- Keep `taskNNNN.tests.yaml` records truthful, so a later task that reads
  this record to attribute a failure is not handed a red state that was
  never observed (BO-1).
- Remove the internal contradiction inside a single record, where AC-6
  treats an invariant guard as `red_confirmed: false` while AC-7 treats the
  same class of criterion as `red_confirmed: true` (BO-2).
- Avoid shipping, in a new machine-readable record, the same class of defect
  that PR #46 itself exists to fix — a record whose text diverges from the
  observed reality and silently reads as green (BO-3).

## User Stories

### US1: A later task attributes a failure from the record

As a later task reading `test-docs/stale-test-name-refs/task0001.tests.yaml`,
I want each criterion's `red_confirmed` to reflect whether a red state was
actually observed, so that I do not attribute a failure to a red that never
existed.

**Acceptance Criteria:**
- [ ] AC-1: The AC-7 entry has `red_confirmed: false`.
- [ ] AC-2: AC-7's `red_reason` states the "invariant guard on writing
      discipline with no observable pre-state, same treatment as AC-6"
      rationale, and contains no claim of an observed red.
- [ ] AC-3: The trailing `notes` block carries one added line stating AC-7 is
      an unconfirmed red.

### US2: A reviewer verifies the record against reality

As a reviewer of this record, I want the edit confined to the AC-7 entry and
the `notes` block, with the file still parseable and the split-identifier
convention intact, so that I can accept the change by inspection alone.

**Acceptance Criteria:**
- [ ] AC-4: The file parses as YAML.
- [ ] AC-5: The file contains zero contiguous occurrences of the old
      identifier string.
- [ ] AC-6: AC-1 through AC-6 of the record, and its `task_id`,
      `baseline_failures`, and `final_failures` keys, are byte-identical to
      the base revision.

## Technical Requirements

### Functional Requirements

- **FR1 - AC-7 red_confirmed becomes false:** In
  `test-docs/stale-test-name-refs/task0001.tests.yaml`, the AC-7 entry's
  `red_confirmed` value is `false` (currently `true` at line 81).
- **FR2 - AC-7 red_reason states the invariant-guard rationale:** AC-7's
  `red_reason` (currently lines 82-88) is rewritten so it states that AC-7 is
  an invariant guard on writing discipline with no observable pre-state — the
  task itself creates the record, so no red state could exist before it — and
  receives the same treatment as AC-6. The rewritten text no longer asserts
  or implies an observed red.
- **FR3 - notes records AC-7 as an unconfirmed red:** The record's trailing
  `notes` block (currently lines 89-95) gains one line stating that AC-7 is
  an unconfirmed red.
- **FR4 - Split-identifier convention preserved:** After the edit, the file
  contains zero contiguous occurrences of the old identifier. Any reference
  to it stays in the split `prefix + suffix` form declared in the file's
  header comment (lines 1-4), and that header comment is left intact.
- **FR5 - No other acceptance entry is altered:** AC-1 through AC-6 keep
  their current `red_confirmed` values and `red_reason` text unchanged, as do
  `task_id`, `baseline_failures`, and `final_failures`.

### Non-Functional Requirements

- **NFR1 - Format fidelity:** The file remains parseable as YAML, and keeps
  its existing key ordering and folded block-scalar style (`>-`) for
  `red_reason` and `notes`.
- **NFR2 - No behavioral surface:** Documentation-record change only. No Rust
  or TypeScript source is touched, so no build, test, or runtime behavior
  changes and no rebuild is required.
- **NFR3 - Commit-message discipline:** The commit message for this change
  also spells the old identifier only in split form, per the discipline AC-7
  itself describes.
- **NFR4 - Diff containment:** The git diff for this change is confined to
  `test-docs/stale-test-name-refs/task0001.tests.yaml` (plus this feature's
  own feature-docs, which are outside the record's scope table).

## Implementation Approach

### Architecture

No architectural surface. The change edits a single YAML documentation
record; no module, layer, or component of the application is involved.

**Affected artifact:**

```
test-docs/stale-test-name-refs/task0001.tests.yaml
├── (lines 1-4)   header comment — declares the split prefix + suffix form   [unchanged, FR4]
├── task_id, baseline_failures, final_failures                               [unchanged, FR5]
├── acceptance_tests
│   ├── AC-1 .. AC-5                                                          [unchanged, FR5]
│   ├── AC-6  (red_reason at lines 74-78 — the wording AC-7 is aligned to)    [unchanged, FR5]
│   └── AC-7
│       ├── red_confirmed  (line 81)   true → false                           [FR1]
│       └── red_reason     (lines 82-88) rewritten                            [FR2]
└── notes (lines 89-95) — one line added inside the folded scalar             [FR3]
```

### Data Flow

```
implementer → edits AC-7 entry + notes → YAML parse check → grep count check → git diff scope check
```

### API Design

Not applicable. The feature exposes no API surface.

### Database Schema

Not applicable. The feature touches no persistent store.

The edited record's key shape, which AC-4 asserts is preserved:

| Key | Type | Changed |
|-----|------|---------|
| `task_id` | scalar | No (FR5) |
| `baseline_failures` | scalar | No (FR5) |
| `final_failures` | scalar | No (FR5) |
| `acceptance_tests[AC-1..AC-6]` | mapping | No (FR5) |
| `acceptance_tests[AC-7].red_confirmed` | boolean | Yes → `false` (FR1) |
| `acceptance_tests[AC-7].red_reason` | folded scalar (`>-`) | Yes, rewritten (FR2) |
| `notes` | folded scalar (`>-`) | Yes, one line added (FR3) |

### Dependencies

**Internal Dependencies:**
- `test-docs/stale-test-name-refs/task0001.tests.yaml`: the record being
  edited; its AC-6 `red_reason` (lines 74-78) is the wording AC-7 is aligned
  to, and its header comment (lines 1-4) declares the split form FR4
  preserves.

**External Dependencies:**
- A YAML parser, for the AC-4 parse check only. No runtime dependency is
  added or changed.

### File Structure

```
test-docs/
└── stale-test-name-refs/
    └── task0001.tests.yaml    # the only production-side file this feature edits
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from every
task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries
in addition to the feature-specific paths above:

- `feature-docs/ac7-red-confirmed-unobserved/**`
- `test-docs/ac7-red-confirmed-unobserved/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal
is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it.

Note that the edited artifact `test-docs/stale-test-name-refs/task0001.tests.yaml`
belongs to a different feature's test-docs directory and is therefore a
feature-specific path that create-plan derives from the task's `files`, not a
default member.

## Test Scenarios

### Unit Tests

Not applicable. The feature adds no code.

### Integration Tests

- [ ] TS-1 (record-inspection, covers FR1 / FR2): Open
  `test-docs/stale-test-name-refs/task0001.tests.yaml`, read the AC-7 entry,
  and confirm `red_confirmed: false` with a `red_reason` that mirrors AC-6's
  invariant-guard wording.
- [ ] TS-2 (parse-check, covers NFR1): Load the file with a YAML parser and
  confirm the seven `acceptance_tests` keys and the `notes` key are all still
  present and well-formed.
- [ ] TS-3 (grep-count, covers FR4): Assemble the old identifier from the
  header comment's two fragments and grep the file for the contiguous string;
  expect 0 matches.
- [ ] TS-4 (diff-scope, covers FR3 / FR5 / NFR4): `git diff` the file and
  confirm every hunk lies within the AC-7 entry or the `notes` block, and
  that the header comment (lines 1-4) is untouched.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] TS-5 (no-regression, covers NFR2): Confirm no Rust or TypeScript file
  changed, so the project's existing suites need no re-run to accept this
  change. If a run is wanted anyway, the established commands are
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  and `bun test`.

### Edge Cases

- [ ] EC-1: The rewritten `red_reason` must not itself introduce a contiguous
  occurrence of the old identifier — the text being replaced is precisely the
  text that documents that constraint. Handled by the TS-3 grep count.
- [ ] EC-2: `notes` is a `>-` folded block scalar. An added line must keep the
  block's indentation so folding stays valid and the surrounding sentences
  are not run together incorrectly. Handled by the TS-2 parse check.
- [ ] EC-3: The record's AC-2 asserts a repository-wide count of exactly 6
  occurrences. Writing the old identifier contiguously anywhere in this file
  would falsify that record's own count claim — the same class of
  record/reality divergence this feature fixes. Handled by FR4's zero-count
  constraint.
- [ ] EC-4: AC-7's original text also binds commit messages. The commit that
  makes this change is itself subject to the split-identifier discipline.
  Handled by NFR3.
- [ ] EC-5: AC-7 currently reads as the justification for AC-2's count staying
  at 6. Rewriting it must not delete that linkage in a way that leaves AC-2's
  "6 instead of 7" reasoning unexplained. Handled by FR2's rewrite retaining
  the linkage.

### Performance Tests

Not applicable (NFR2 — no runtime behavior changes).

## Security Considerations

- **SC-1:** None applicable. The change edits a documentation record
  containing no executable content, no credentials, and no network or
  filesystem behavior.

## Error Handling

Not applicable. The feature introduces no runtime error paths. The failure
modes that matter are verification failures, each caught by a listed
scenario:

| Failure mode | Caught by |
|---|---|
| Old identifier written contiguously | TS-3 / AC-5 |
| Folded scalar indentation broken | TS-2 / AC-4 |
| Edit escaped the AC-7 entry or `notes` block | TS-4 / AC-6 |

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1-FR5) are implemented.
- [ ] All non-functional requirements (NFR1-NFR4) hold.
- [ ] All test scenarios (TS-1 through TS-5) pass.
- [ ] All acceptance criteria (AC-1 through AC-6) are satisfied.
- [ ] Security requirements are satisfied (SC-1: not applicable).
- [ ] Code review is completed.

## Assumptions

Every assumption below is carried over from the resolved requirements; none
originates here.

- **A-1** (impact low, reversible): The factual after-state evidence
  currently in AC-7's `red_reason` (that grepping this file for the
  contiguous old identifier after writing it returned zero matches) may be
  kept as verification evidence, provided the rewritten text no longer
  presents it as a confirmed red observation. Reason: the definition of done
  constrains the claim (`red_confirmed`) and the rationale, not the retention
  of a true measurement.
- **A-2** (impact low, reversible): "One line appended to `notes`" means one
  added line inside the existing `notes` folded block scalar, not a new
  top-level key and not a new `notes` entry list. Reason: `notes` is a `>-`
  folded scalar at lines 89-95, and the definition of done says
  「末尾の notes に … 1 行追記」.
- **A-3** (impact medium, reversible): Nothing among the supplied inputs
  mechanically validates or consumes `red_confirmed` values, so flipping
  `true` to `false` requires no corresponding change to any tool, schema, or
  fixture. Reason: no validator or schema for `taskNNNN.tests.yaml` appeared
  in `fixed_path_inputs` or `resolved_input_paths`; the only scan target is
  the record itself.
- **A-4** (impact low, reversible): The zero-contiguous-occurrence constraint
  applies to this file alone. The three carve-out files named in the record's
  AC-2 and AC-6
  (`feature-docs/relocate-wrap-cursor-clamp/{SPEC.md,REQUIREMENTS.md,reviews/round1.yaml}`)
  keep their 6 occurrences and are not touched. Reason: stated by the
  record's own AC-2 and AC-6; those paths are outside this dispatch's
  `reference_scan_targets`.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional requirement is `status: resolved`.

## Design Step

Skipped. The feature edits one YAML documentation record
(`test-docs/stale-test-name-refs/task0001.tests.yaml`). It produces no UI
surface, no visual output, and no user-facing screen, and touches none of the
project's design-system layers.

## References

- Requirements document: `feature-docs/ac7-red-confirmed-unobserved/REQUIREMENTS.md`
- Record under edit: `test-docs/stale-test-name-refs/task0001.tests.yaml`
- Origin of the defect: PR #46 review round 1, comprehensive perspective
  (stable_id `41b6ef707d9d6692`, severity medium, confidence 65, left
  `unresolved` because the auto-fix gate only covers critical/high)
- Carve-out files that keep their occurrences and are not touched:
  `feature-docs/relocate-wrap-cursor-clamp/SPEC.md`,
  `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md`,
  `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml`
