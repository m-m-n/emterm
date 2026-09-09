# Feature: ac7-red-reason-scope

## Overview

The AC-7 entry's `red_reason` in `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml`
states, of the whole change set, that `git status --porcelain` lists only
`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`. That task's change set in
fact contained two paths: the record it edited and its own workflow-generated per-task
test record. This feature rewrites that one folded scalar so it describes the change set
that was actually observed, while leaving the red verdict and every other part of the
record untouched.

Requirements source: `feature-docs/ac7-red-reason-scope/REQUIREMENTS.md`.

## Objectives

- Keep `taskNNNN.tests.yaml` records usable as machine-readable evidence for later tasks,
  by making the AC-7 entry of `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml`
  describe the inspection that was actually performed and the scope it actually covered
  (BO-1).
- Remove the record/reality gap in that entry without weakening the inspection it records:
  the change set contained no Rust and no TypeScript file, and that conclusion stays
  stated (BO-2).
- Leave the red verdict (`red_confirmed: false`, invariant-guard classification) and every
  other part of the record untouched, so the correction reads as a text fix rather than a
  re-judgement (BO-3).

## User Stories

### US1: A change-set description that matches the change set

As a later task that reads `taskNNNN.tests.yaml` as machine-readable evidence, I want the
AC-7 `red_reason` to describe the change set that was actually observed, so that I am not
handed a claim about a state nobody inspected.

**Acceptance Criteria:**
- [ ] AC-1: Loading `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and reading
      `acceptance_tests['AC-7']['red_reason']` yields text that names both
      `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` and
      `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` as members of that task's
      change set.
- [ ] AC-2: The same parsed scalar identifies the second path as the task's own
      workflow-generated per-task test record and ties it to the SPEC Declared Change Set
      entry `test-docs/{feature}/**`.
- [ ] AC-4: The same parsed scalar no longer claims that `git status --porcelain` for the
      whole change set lists only
      `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`, and makes no equivalent
      single-file assertion about the whole change set.

### US2: The inspection and the verdict survive the correction

As a reader of the same record, I want the recorded finding and the red verdict to stay as
they are, so that the correction is a text fix and not a re-judgement.

**Acceptance Criteria:**
- [ ] AC-3: The same parsed scalar still states that the listed change set contains no Rust
      file and no TypeScript file.
- [ ] AC-5: The same parsed scalar still carries the invariant-guard classification, the
      empty-pre-state statement, and the zero-contiguous-occurrence result of the
      fixed-string identifier search; `acceptance_tests['AC-7']['red_confirmed']` is still
      the boolean `false` and the entry's `tests` value is unchanged.
- [ ] AC-6: The file's diff against the base revision confines every hunk to the AC-7
      entry's `red_reason` scalar; `task_id`, `baseline_failures`, `final_failures`, the
      AC-1 through AC-6 entries, and the trailing `notes` block are byte-identical to the
      base revision.

### US3: A record that stays loadable, and a feature that does not repeat the defect

As the automation that consumes these records, I want the file to keep its shape and this
feature's own artifacts to describe their own change set correctly, so that the defect being
repaired is not reproduced in a third record.

**Acceptance Criteria:**
- [ ] AC-7: The file loads under PyYAML with `task_id`, `baseline_failures`,
      `final_failures`, an `acceptance_tests` mapping of exactly seven entries (AC-1..AC-7)
      and `notes`; the raw top-level key order is unchanged; the rewritten scalar is still
      introduced by `red_reason: >-` at the same indentation as the file's other folded
      scalars; and it is in English.
- [ ] AC-8: The change set of this feature's own task contains no Rust file and no
      TypeScript file. It is expected to contain two paths — the edited record
      `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and this feature's own
      generated record `test-docs/ac7-red-reason-scope/task0001.tests.yaml` — and this
      criterion is stated in those terms, never as "only one file".

## Technical Requirements

### Functional Requirements

- **FR1 - Describe the actual change set:** The AC-7 entry's `red_reason` in
  `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` describes the task's change set
  as the two paths it actually contained —
  `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` (the target record that task
  edited) and `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` (that task's own
  workflow-generated per-task test record) — instead of the current single-path claim.
- **FR2 - Attribute the second path to the declared change set:** The rewritten scalar
  states that the second path is the workflow-generated per-task test record the implement
  phase always produces, and that it falls under the SPEC's Declared Change Set entry
  `test-docs/{feature}/**`, so its presence is expected rather than a containment violation.
- **FR3 - Preserve the inspection intent:** The rewritten scalar still records the original
  finding: the listed change set contains no Rust file and no TypeScript file.
- **FR4 - Drop the whole-change-set "only one file" scoping:** The rewritten scalar no
  longer asserts, of the whole change set, that `git status --porcelain` lists only
  `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`; any statement it makes about
  the change set is scoped to what was in fact observed.
- **FR5 - Preserve the rest of the AC-7 justification:** The remaining substance of the AC-7
  `red_reason` is preserved: its invariant-guard classification, the statement that the
  pre-edit change set was empty so there was no observable pre-state to fail, and the
  fixed-string identifier search (assembled from the described record's own header-comment
  fragments) returning zero contiguous occurrences before and after the edit.
- **FR6 - Confine the edit to the one folded scalar:** Only the AC-7 entry's `red_reason`
  folded scalar changes. `task_id`, `baseline_failures`, `final_failures`, the AC-7 entry's
  `tests` and `red_confirmed` values, the AC-1 through AC-6 entries, and the trailing
  `notes` block are byte-identical after the change.

### Non-Functional Requirements

- **NFR1 - Parseability:** The file continues to load under PyYAML with its existing key
  set — `task_id`, `baseline_failures`, `final_failures`, an `acceptance_tests` mapping with
  exactly its seven entries AC-1..AC-7, and `notes` — and with unchanged top-level key order
  in the raw file.
- **NFR2 - Formatting fidelity:** The edited scalar keeps the `>-` folded block-scalar
  indicator at its existing indentation and the surrounding line-wrap style used by every
  other folded scalar in the file.
- **NFR3 - Language:** The rewritten scalar stays in English, matching every other entry in
  the file.
- **NFR4 - No unobserved assertion:** The rewritten text asserts only what the recorded
  observation supports: the two paths that were in the task's commit, and the absence of
  Rust and TypeScript files among them. It introduces no new claim about a state nobody
  inspected — this is the defect class the feature exists to remove.
- **NFR5 - No build impact:** No Rust and no TypeScript source changes, and no rebuild: the
  change is a documentation-text correction in one YAML record.
- **NFR6 - Self-consistent scoping in this feature's own record:** No acceptance criterion,
  test scenario, or record text produced by this feature phrases its own change set as a
  single file: this feature's own task commit will likewise contain both the edited record
  and its workflow-generated `test-docs/ac7-red-reason-scope/task0001.tests.yaml`.

## Implementation Approach

### Architecture

No architectural surface is involved. The change is a text edit to one folded block scalar
in one YAML record.

**Component Diagram:**
```
test-docs/ac2-red-reason-accuracy/task0001.tests.yaml
  └── acceptance_tests
        └── AC-7
              ├── red_reason      <- the only scalar this feature rewrites (FR1-FR5)
              ├── red_confirmed   <- stays boolean false (FR5, BO-3)
              └── tests           <- unchanged (FR5, FR6)
```

### Data Flow

```
the described task's commit (its actual path list)
   → evidence for what that task's change set contained (A-3, NFR4)
   → rewritten AC-7 red_reason text in
     test-docs/ac2-red-reason-accuracy/task0001.tests.yaml
```

### API Design

Not applicable — no API surface.

### Record Schema

Not applicable as a database. The record's structure is the YAML shape below.

| Key | Type | Changes | Description |
|--------|------|------|-------------|
| `task_id` | scalar | No | Unchanged (FR6) |
| `baseline_failures` | list | No | Unchanged (FR6) |
| `final_failures` | list | No | Unchanged (FR6) |
| `acceptance_tests` | mapping (7 entries) | Only `AC-7.red_reason` | Entry count and raw key order unchanged (NFR1) |
| `acceptance_tests['AC-7']['red_reason']` | string (`>-` folded) | Yes | The corrected justification (FR1–FR5) |
| `acceptance_tests['AC-7']['red_confirmed']` | boolean | No | Stays `false` (FR5) |
| `acceptance_tests['AC-7']['tests']` | list | No | Unchanged (FR5, FR6) |
| `notes` | block | No | Unchanged (FR6, A-5) |

### Dependencies

**Internal Dependencies:**
- `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`: its AC-6 entry already uses
  the correct two-path phrasing for the analogous situation and corroborates the path list
  (A-3). It is read as evidence and is not modified by this feature.

**External Dependencies:**
- PyYAML: used to load the record when verifying AC-1 through AC-5 and AC-7.
- `git diff` / `git status`: used to confirm change containment (AC-6) and to enumerate this
  task's own change set (AC-8).

### File Structure

```
test-docs/
├── ac2-red-reason-accuracy/
│   └── task0001.tests.yaml     # the record this feature edits
└── ac7-red-reason-scope/
    └── task0001.tests.yaml     # this feature's own workflow-generated per-task record
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from every
task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`). The feature-specific path this
feature edits is:

- `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml`

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/ac7-red-reason-scope/**`
- `test-docs/ac7-red-reason-scope/**`

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

This feature's own task commit is expected to contain two paths — the edited
`test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and the implement phase's
generated `test-docs/ac7-red-reason-scope/task0001.tests.yaml` — plus the
orchestrator's own `feature-docs/ac7-red-reason-scope/**` documents. All of them are
inside the declaration above (A-2). No criterion, scenario or record text produced by
this feature phrases that change set as "only one file"; doing so would reproduce, in a
third record, the exact defect being repaired (NFR6).

## Out of Scope

Repair is limited to the live machine-readable record
`test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` (its AC-7 folded scalar). The
already-merged planning documents of feature `ac2-red-reason-accuracy` are NOT rewritten
by this feature (A-1):

- `feature-docs/ac2-red-reason-accuracy/SPEC.md`, Success Criteria: "`git status
  --porcelain` lists only `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`".
- `feature-docs/ac2-red-reason-accuracy/tasks/task0001.md`, AC-7: "The task's whole change
  set lists only this one YAML file".

Both carry the same unsatisfiable "only one file" claim, and both contradict that same
SPEC's own Declared Change Set, which declares `feature-docs/{feature}/**` and
`test-docs/{feature}/**`. That contradiction is recorded here as a fact instead of being
repaired: `test-docs/` is a live input future automation reads for failure attribution,
while the feature-docs of a merged, completed feature are a historical audit record that
must not be retroactively made to look correct.

## Test Scenarios

### Unit Tests

- [ ] TS-1 (FR1, FR2, FR4): Load the record with PyYAML before and after the change and
      assert on the parsed `acceptance_tests['AC-7']['red_reason']` string (never on raw
      line content — the folded scalar re-wraps): before, it contains the single-path
      "lists only test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml" claim; after,
      it names both paths and no longer carries that claim.
- [ ] TS-2 (FR3, FR5): Assert the parsed scalar still contains the no-Rust / no-TypeScript
      finding, the invariant-guard classification, the empty-pre-state statement and the
      zero-occurrence identifier-search result, and that `red_confirmed` for AC-7 is `False`
      both before and after — an invariant guard with no observable pre-state.
- [ ] TS-3 (NFR1, NFR2, NFR3): Confirm the raw file still uses `>-` for the AC-7
      `red_reason` at its original indentation, that the top-level key order is unchanged,
      that the loaded `acceptance_tests` mapping has exactly seven entries, and that the
      rewritten scalar is ASCII/English throughout.

### Integration Tests

- [ ] TS-4 (FR6): Diff the file against the base revision and assert every hunk is inside
      the AC-7 `red_reason` scalar; assert the AC-1..AC-6 entries, the top-level scalars and
      the `notes` block are byte-identical.
- [ ] TS-5 (NFR5, NFR6): List this task's own change set and assert it contains no Rust and
      no TypeScript path, stating the expected membership as the edited record plus this
      feature's own `test-docs/ac7-red-reason-scope/task0001.tests.yaml` — the same two-path
      shape the corrected text describes, and the phrasing this feature's own record must
      use.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] The folded scalar re-wraps on rewrite, so raw-line assertions are unreliable;
      assertions run against the PyYAML-parsed value (TS-1).
- [ ] The described task's commit may not contain exactly the two `test-docs/` paths named
      in FR1; the implementer confirms the exact path list against the commit before writing
      the text, since NFR4 forbids asserting an unobserved state (A-3).
- [ ] The AC-7 entry is located by content (the `acceptance_tests` mapping's `AC-7` key),
      never by the line number 96 quoted in the task description (A-4).
- [ ] The trailing `notes` block already describes the verification method as "git
      diff/status inspection of the single changed file". That phrase describes the file the
      task edited, not the whole change set, and FR6 makes `notes` byte-identical; if the
      implementer judges it to be a second instance of the same scoping defect, that is a
      plan deviation to surface, not something to fix silently (A-5).

### Performance Tests

Not applicable — a documentation-text correction in one YAML record (NFR5).

## Security Considerations

Not applicable — no code path, no input handling, and no data surface is touched; the change
is a documentation-text correction in one YAML record (NFR5).

## Error Handling

No runtime error surface exists for this change. The failure modes are verification
failures:

| Condition | Detected by | Response |
|------|-------------|--------------|
| The rewritten text names a path list the commit does not support | TS-5, A-3 | Correct the text to the observed path list (NFR4) |
| A key other than AC-7's `red_reason` changed | TS-4, AC-6 | Revert the out-of-scope change (FR6) |
| File no longer parses or loses its shape | AC-7, TS-3 | Restore the `>-` scalar style, key order and entry count (NFR1, NFR2) |

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] FR1 through FR6 are implemented and verified.
- [ ] NFR1 through NFR6 hold after the change.
- [ ] AC-1: the parsed AC-7 `red_reason` names both
      `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` and
      `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` as members of that task's
      change set.
- [ ] AC-2: the same parsed scalar identifies the second path as that task's own
      workflow-generated per-task test record and ties it to `test-docs/{feature}/**`.
- [ ] AC-3: the same parsed scalar still states the listed change set contains no Rust file
      and no TypeScript file.
- [ ] AC-4: the same parsed scalar makes no single-file assertion about the whole change
      set.
- [ ] AC-5: the invariant-guard classification, the empty-pre-state statement and the
      zero-occurrence identifier-search result remain; `red_confirmed` is still `false` and
      `tests` is unchanged.
- [ ] AC-6: every diff hunk against the base revision is inside the AC-7 `red_reason`
      scalar; the top-level scalars, AC-1..AC-6 and `notes` are byte-identical.
- [ ] AC-7: the file loads under PyYAML with its seven-entry `acceptance_tests` mapping and
      unchanged raw top-level key order; the rewritten scalar is introduced by
      `red_reason: >-` at the same indentation and is in English.
- [ ] AC-8: this feature's own change set contains no Rust file and no TypeScript file, and
      is expected to contain the two paths
      `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and
      `test-docs/ac7-red-reason-scope/task0001.tests.yaml`.
- [ ] TS-1 through TS-5 pass.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is `status: resolved`.

## Assumptions

These are the assumptions the requirements analysis carried; they are recorded here
unchanged in substance.

- **A-1:** Out of scope (recorded, not repaired): the already-merged planning documents of
  feature `ac2-red-reason-accuracy` keep their "only one file" claim. See the Out of Scope
  section above for the two documents and the reason.
- **A-2:** This feature's own task commit will again contain two paths — the edited
  `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and the implement phase's
  generated `test-docs/ac7-red-reason-scope/task0001.tests.yaml` — plus the orchestrator's
  own `feature-docs/ac7-red-reason-scope/**` documents. No acceptance criterion, test
  scenario or record text produced by this feature may therefore be phrased as "only one
  file".
- **A-3:** The described task commit's stat contains exactly the two `test-docs/` paths
  named in FR1. This comes from the task description and from the sibling record
  `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` AC-6, which already uses the
  correct two-path phrasing for the analogous situation; it was not independently
  re-derived from git in the analyst dispatch. The implementer confirms the exact path list
  against the commit before writing the text, since NFR4 forbids asserting an unobserved
  state.
- **A-4:** The file to edit is `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` at
  the integration worktree HEAD; the AC-7 entry is located by content (the
  `acceptance_tests` mapping's `AC-7` key), never by the line number 96 quoted in the task
  description.
- **A-5:** The trailing `notes` block needs no change: it already describes the verification
  method for AC-6/AC-7 as "git diff/status inspection of the single changed file". That
  phrase describes the file the task edited, not the whole change set, and the completion
  definition constrains only the AC-7 folded scalar. If the implementer judges the phrase to
  be a second instance of the same scoping defect, that is a plan deviation to surface, not
  something to fix silently — FR6 makes `notes` byte-identical.
- **A-6:** The project commands are not required to exercise this change set (it touches no
  Rust and no TypeScript source), matching the rationale the record's own `notes` already
  documents. `bun test` / `bun run typecheck` may still be run as a no-regression check, as
  the previous feature did.
- **A-7:** The record stays in English; the whole file is English today.

## Design Step

Skipped. Gate `create-spec.design-step` was resolved in batch mode with option
`decide_autonomously`, accepting the analyst's recommendation to skip. The feature is a
single folded block-scalar text correction in one YAML record: no UI, no visual surface, no
new module boundary, no API change, and the three detected design-system candidates
(`doc/UI-DESIGN-GUIDELINES.yaml`, `src-tauri/src/ui/md3.rs`,
`src-tauri/web-shared/styles.css`) are untouched by it.

## References

- Requirements document: `feature-docs/ac7-red-reason-scope/REQUIREMENTS.md`
- Record under correction: `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml`
- Correctly-scoped precedent (AC-6): `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
- Not repaired (see Out of Scope): `feature-docs/ac2-red-reason-accuracy/SPEC.md`,
  `feature-docs/ac2-red-reason-accuracy/tasks/task0001.md`
- Project license: MIT
