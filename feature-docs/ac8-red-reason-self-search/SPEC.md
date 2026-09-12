# Feature: ac8-red-reason-self-search

## Overview

The AC-8 entry of the machine-readable evidence record
`test-docs/ac7-red-reason-scope/task0001.tests.yaml` records the outcome of a
fixed-string search of the record's own text while quoting the search needles
inline, so the recorded outcome does not hold when the same search is re-run
against the committed file. This feature rewrites that one folded YAML scalar
so it drops the quoted needles and states only the property verified directly
from the task commit's change set. Requirement sources and acceptance criteria
are in `feature-docs/ac8-red-reason-self-search/REQUIREMENTS.md`.

## Objectives

- Make the AC-8 entry record only checks that still hold when re-run, so later
  tasks that read it to attribute failures are not misled by a self-falsifying
  claim (BO1).
- Keep the record's independently confirmed findings — the two-path change-set
  enumeration and the no-Rust / no-TypeScript determination — unchanged, so no
  accurate evidence is lost while fixing the defective clause (BO2).

## User Stories

### US1: A later task reads the AC-8 entry to attribute a failure
As a later task that reads `test-docs/` evidence records, I want the AC-8
`red_reason` to contain only re-runnable claims, so that re-running a check it
describes confirms rather than contradicts the record.

**Acceptance Criteria:**
- [ ] AC-1: A parser load of `test-docs/ac7-red-reason-scope/task0001.tests.yaml`
  yields an AC-8 `red_reason` that contains no report of the outcome of
  searching the record's own text, and no inline quoted needle for such a
  search. Checkable from outside the record, e.g. by a scripted search of the
  loaded AC-8 scalar driven from a different file (see EC1).
- [ ] AC-4: Every check the rewritten AC-8 `red_reason` describes, when
  re-executed against the committed tree, produces the outcome the scalar
  records — no remaining clause is contradicted by re-running it.

### US2: The accurate findings survive the correction
As a later task that reads `test-docs/` evidence records, I want the change-set
enumeration and the language determination to be preserved verbatim in
substance, so that fixing the defective clause loses no accurate evidence.

**Acceptance Criteria:**
- [ ] AC-2: The loaded AC-8 `red_reason` names both
  `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and
  `test-docs/ac7-red-reason-scope/task0001.tests.yaml`, and states the change
  set contains exactly two paths.
- [ ] AC-3: The loaded AC-8 `red_reason` states the change set contains no Rust
  file and no TypeScript file.
- [ ] AC-5: The file loads under a YAML parser; top-level keys are `task_id`,
  `baseline_failures`, `final_failures`, `acceptance_tests`, `notes` in that raw
  order, and `acceptance_tests` has exactly the eight entries AC-1..AC-8.
- [ ] AC-6: `git diff` for the file shows exactly one hunk, confined to the AC-8
  entry's `red_reason` scalar; `task_id`, `baseline_failures`,
  `final_failures`, the AC-1..AC-7 entries, AC-8's `tests` and `red_confirmed`,
  and the `notes` block are byte-identical to the base revision — including the
  AC-4 `red_reason` occurrences of `lists only` at lines 43 and 47.
- [ ] AC-7: The AC-8 `red_reason` is still introduced by the same folded block
  scalar introducer at the same indentation as the file's other folded scalars,
  AC-8's `red_confirmed` is still the boolean `false` and its `tests` still the
  empty list.

## Technical Requirements

### Functional Requirements

- **FR1 — Remove the self-falsifying search clause:** The trailing clause of the
  AC-8 `red_reason` in `test-docs/ac7-red-reason-scope/task0001.tests.yaml` —
  the one reporting the outcome of a fixed-string search of the record's own
  text for a single-file phrasing, together with the two needles it quotes
  inline — is removed. The replacement text makes no claim about the result of
  searching the record's own text.
- **FR2 — State only the directly verified property:** The replacement clause
  states only the property verified directly from the task commit's change set
  against the merge base (`git diff --stat <merge-base>..HEAD`): the change set
  consists of exactly the two enumerated paths and contains no Rust file and no
  TypeScript file. No new claim is introduced that the record's own text cannot
  support.
- **FR3 — Preserve the two-path enumeration:** The rewritten AC-8 `red_reason`
  still names both `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` (the
  edited record) and `test-docs/ac7-red-reason-scope/task0001.tests.yaml` (that
  feature's own generated record), and still states the change set contains
  exactly two paths.
- **FR4 — Preserve the no-Rust / no-TypeScript determination:** The rewritten
  AC-8 `red_reason` still states that the enumerated change set contains no Rust
  file and no TypeScript file.
- **FR5 — Preserve the no-observable-pre-state framing:** The opening of the
  AC-8 `red_reason` — that this task's own commit does not exist before the task
  runs, so its change set cannot be enumerated beforehand, and that the
  enumeration was therefore verified after committing against the merge base —
  is retained, keeping `red_confirmed: false` and `tests: []` consistent with
  the stated reason.
- **FR6 — Confine the edit to the AC-8 red_reason scalar:** The change touches
  only the AC-8 entry's `red_reason` scalar. `task_id`, `baseline_failures`,
  `final_failures`, the AC-1 through AC-7 entries, the AC-8 entry's `tests` and
  `red_confirmed` values, and the trailing `notes` block are byte-identical to
  the base revision. In particular the two occurrences of the string
  `lists only` at lines 43 and 47, which belong to the AC-4 `red_reason` and
  describe a different record, are left untouched.
- **FR7 — Preserve the file's parse shape and scalar form:** After the edit the
  file still loads under a YAML parser with the same top-level keys in the same
  order (`task_id`, `baseline_failures`, `final_failures`, `acceptance_tests`,
  `notes`) and an `acceptance_tests` mapping of exactly eight entries
  AC-1..AC-8; the AC-8 `red_reason` is still introduced by a folded block scalar
  introducer at the same indentation as the file's other folded scalars, and is
  still written in English.

### Non-Functional Requirements

- **NFR1 — Compatibility:** No runtime behavior changes: the edit is confined to
  a documentation evidence record under `test-docs/` and touches no Rust,
  TypeScript, build, or packaging input.
- **NFR2 — Style consistency:** The rewritten scalar stays consistent in style
  with the file's other `red_reason` scalars — folded block scalar, English
  prose, comparable line width, no trailing whitespace.
- **NFR3 — Re-runnability:** Every assertion left in the AC-8 `red_reason` is
  re-runnable: re-executing the check it describes against the committed tree
  reproduces the stated outcome.
- **NFR4 — Machine readability:** The record remains machine-readable for
  downstream failure attribution — a plain parser load yields the same key
  structure consumers already rely on.

## Implementation Approach

### Architecture

No runtime architecture is involved. The unit of change is a single value inside
one YAML document:

```
test-docs/ac7-red-reason-scope/task0001.tests.yaml
├── task_id                     (unchanged)
├── baseline_failures           (unchanged)
├── final_failures              (unchanged)
├── acceptance_tests
│   ├── AC-1 .. AC-7            (unchanged, incl. AC-4 red_reason lines 43/47)
│   └── AC-8
│       ├── tests               (unchanged: [])
│       ├── red_confirmed       (unchanged: false)
│       └── red_reason          <-- the only edited scalar
└── notes                       (unchanged)
```

### Data Flow

```
implement phase → evidence record (YAML) → later task reading it for failure attribution
```

The correction operates on the middle element only; producer and consumer are
unchanged.

### API Design

Not applicable — the feature exposes no interface.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `test-docs/ac7-red-reason-scope/task0001.tests.yaml`: the record whose AC-8
  `red_reason` is corrected.
- `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml`: named by the AC-8
  scalar as the record the predecessor task edited; not modified here.

**External Dependencies:**
- A YAML parser for the scripted parse/assert check (TS1 follows the pattern the
  predecessor feature used with PyYAML).
- `git` for the change-set and containment checks (TS2, TS3).

### File Structure

```
test-docs/
└── ac7-red-reason-scope/
    └── task0001.tests.yaml     # single edited file
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/ac8-red-reason-self-search/**`
- `test-docs/ac8-red-reason-self-search/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

The feature-specific path this feature edits is
`test-docs/ac7-red-reason-scope/task0001.tests.yaml`.

These default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is
a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it.

## Test Scenarios

### Unit Tests
- [ ] TS1 (AC-1, AC-2, AC-3, AC-5, AC-7): Scripted parser-load assertion over
  the target record (the pattern the predecessor feature used with PyYAML): load
  the file, assert the top-level key order and the eight-entry
  `acceptance_tests` mapping, then assert on the AC-8 `red_reason` string — both
  required paths present, the no-Rust / no-TypeScript wording present, and no
  self-search report present. The needles for these assertions live in the
  script / this feature's own record, never inside the scalar being searched.

### Integration Tests
- [ ] TS2 (AC-4): Re-run the check the rewritten scalar describes —
  `git diff --stat <merge-base>..HEAD` for the predecessor task's commit — and
  confirm the enumerated two paths and the absence of Rust and TypeScript files
  match the recorded wording.
- [ ] TS3 (AC-6): `git diff` / `git diff --stat` inspection of the single
  changed file, plus a region-wise byte comparison of the untouched regions
  against the base revision.

### E2E Tests
**Existing E2E tests**: None — `test/README.md` documents Rust `cargo test` and
`bun test` as the only frameworks and records that no E2E infrastructure exists.
**Run command**: Not detected

### Regression Guard
- [ ] TS4 (NFR1): Run the project commands as a regression guard even though the
  change is YAML-only: `bun test` and `bun run typecheck` (matching the baseline
  the predecessor task recorded), and the Rust `--lib` suite if the plan calls
  for it. Neither exercises the edited YAML content directly.

### Edge Cases
- [ ] EC1: Re-introducing the same fault one level up: a criterion phrased as
  "the record contains none of the strings X, Y" must not be written into the
  same scalar it searches. Searching the target scalar from a different file (a
  script, or this feature's own generated record under
  `test-docs/ac8-red-reason-self-search/`) is sound; quoting the needle inside
  the searched scalar is not.
- [ ] EC2: This feature's own task commit will again touch two paths (the edited
  record plus its own generated test record), so any AC-8-style self-enumeration
  written for this feature must again state a verified property rather than a
  search result.
- [ ] EC3: Deleting the needle that occurs once removes its sole occurrence in
  the scanned target, while `lists only` must survive at lines 43 and 47 — a
  blanket search-and-replace over the file would violate AC-6.
- [ ] EC4: Folded-scalar rewrapping can silently change the loaded string
  (line-break folding, indentation-induced literal newlines). The assertions in
  TS1 run on the loaded value, not the raw bytes, so folding artifacts surface
  there.
- [ ] EC5: Shortening the scalar must not drop the "no observable pre-state"
  framing, or AC-8's `red_confirmed: false` / `tests: []` values lose their
  stated justification.

### Performance Tests
Not applicable.

## Security Considerations

- **SC1:** None applicable. The change is confined to a documentation evidence
  record; no input handling, no privilege boundary, and no network or filesystem
  surface is involved.

## Error Handling

Not applicable — the feature introduces no executable path.

## Performance Optimization

Not applicable.

## Assumptions

- **A1** (answered gate): The correction form is to drop the quoted needles from
  the AC-8 `red_reason` and state only the directly verified property (exactly
  two paths, no Rust file, no TypeScript file), making no self-referential
  fixed-string-search claim about the record's own text. Resolved via the
  `create-spec.requirement-clarification` gate in packet `create-spec-q0001`
  (answer `requirement.ac8-red-reason.correction-form`, option
  `drop_needle_describe_property`, recorded as an assumption per
  `batch-policies.yaml` `record_as_assumption`). Codex was consulted and
  concurred.
- **A2** (answered gate): The design step is skipped for this feature; the
  change is one folded YAML scalar in a generated evidence record under
  `test-docs/`, with no UI surface and no runtime code path. Resolved via the
  `create-spec.design-step` gate (option `decide_autonomously`).
- **A3** (analyst): The occurrences of `lists only` at lines 43 and 47 belong to
  the AC-4 `red_reason` and describe a different record
  (`test-docs/ac2-red-reason-accuracy/task0001.tests.yaml`); they are correct as
  written and out of scope. Preserved constraint pinned by AC-6.
- **A4** (analyst): The trailing `notes` block's sentence about this task's own
  change set ("never described as a single file") is out of scope: the defect is
  scoped to the AC-8 `red_reason` scalar and the edit-containment criterion
  forbids touching `notes`.
- **A5** (analyst): No automated test suite covers `test-docs/` evidence
  records. Acceptance is verified by scripted parse/assert checks and git
  inspection, with the project commands run only as a no-regression guard.
  Pinned by `test/README.md`, which documents Rust `cargo test` and `bun test`
  as the only frameworks and records that no E2E infrastructure exists.
- **A6** (analyst): The merge-base wording already in the scalar refers to the
  predecessor task's branch and stays accurate after the edit; the rewrite does
  not re-derive or restate the merge-base revision itself.

## Success Criteria

- [ ] All functional requirements (FR1–FR7) are implemented and verified.
- [ ] All non-functional requirements (NFR1–NFR4) are satisfied.
- [ ] All test scenarios (TS1–TS4) pass.
- [ ] All acceptance criteria (AC-1 – AC-7) hold.
- [ ] Code review is completed.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is `resolved`.

## References

- Requirements document: `feature-docs/ac8-red-reason-self-search/REQUIREMENTS.md`
- Correction target: `test-docs/ac7-red-reason-scope/task0001.tests.yaml`
- Record named by the AC-8 scalar: `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml`
- Test framework scope: `test/README.md`
