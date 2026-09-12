# Implementation Plan: ac8-red-reason-self-search

## Overview

The trailing clause of one folded YAML scalar — the AC-8 `red_reason` of
`test-docs/ac7-red-reason-scope/task0001.tests.yaml` — reports the outcome of a
fixed-string search of the record's own text while quoting the search needles
inline, so re-running that search against the committed file contradicts the
record. The feature replaces that clause with a statement of the property
verified directly from the task commit's change set, and changes nothing else.

## Technology Stack

- **Data format**: YAML, as loaded by a plain parser. The artifact under change
  is a machine-readable evidence record, not source code; no runtime component
  reads it.
- **Verification tooling**: a YAML parser available in the environment (the
  predecessor feature used PyYAML) driven from an ad-hoc script, plus `git` for
  change-set enumeration and edit-containment inspection.
- **New dependencies and their licenses**: none. Nothing is added to
  `package.json`, any `Cargo.toml`, or any other package manifest. The YAML
  parser the check script uses is verification-only tooling that is never
  distributed with the artifact (`references/license-compat.md` rule 9); PyYAML
  is MIT, compatible with the project's `MIT` license either way. No license
  conflict exists for this feature.

## Layer Structure

No runtime layers are involved. The only structure that matters is the data
path the record sits in:

1. The implement phase of a past feature produced the evidence record.
2. The record is stored under `test-docs/{feature}/{task}.tests.yaml`.
3. A later task loads the record to attribute a failure.

This feature edits element 2 only; the producer and the consumer are untouched.
Nothing under `src-tauri/`, `crates/`, `scripts/`, or any build or packaging
input is in scope.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Evidence record `test-docs/ac7-red-reason-scope/task0001.tests.yaml` | Machine-readable record a later task reads to attribute failures | **Pre**: loads under a plain YAML parser; top-level keys in raw order `task_id`, `baseline_failures`, `final_failures`, `acceptance_tests`, `notes`; `acceptance_tests` holds exactly the eight entries AC-1..AC-8. **Post**: same keys in the same raw order, same eight entries; AC-8's `tests` is still the empty list and its `red_confirmed` still the boolean `false`; AC-8's `red_reason` is still introduced by the same folded block scalar introducer at the same indentation as the file's other folded scalars | task0001 |
| Re-runnable-claim rule (below) | Governs what an evidence record is allowed to assert | **Pre**: a claim is about to be written into a record. **Post**: the claim names a property observable from an artifact outside the record's own text, and re-executing the described check reproduces the recorded outcome | task0001 |
| This feature's own record `test-docs/ac8-red-reason-self-search/task0001.tests.yaml` | Evidence record the implement phase generates for this task | **Post**: obeys the same re-runnable-claim rule; its own change-set statement is a verified property, never a search result (SPEC EC2) | task0001 |

## Conventions

### Re-runnable-claim rule (the defect class this feature removes)

- An evidence record states properties verified from observable artifacts — the
  commit's change set against its merge base, a parser load, a command's exit
  status — never the outcome of searching the record's own text.
- Any check of the form "text T contains / does not contain S" is driven from a
  file other than T, and S is never written into T. Quoting a needle inside the
  text the needle is searched for is what made the AC-8 clause self-falsifying
  (SPEC EC1).
- The rule binds both the record being corrected and this feature's own
  generated record. This feature's task commit will again touch two paths, so
  any self-enumeration written into its own record states a verified property
  rather than a search result (SPEC EC2).

### Assertion scoping

Assertions about the corrected text run against the **loaded AC-8 `red_reason`
value**, not against raw file bytes and not against the whole file:

- Folded-scalar re-wrapping changes bytes without changing the loaded string,
  and can silently introduce literal newlines; running the assertions on the
  loaded value is what surfaces those artifacts (SPEC EC4).
- The file legitimately keeps `lists only` at lines 43 and 47 inside the AC-4
  `red_reason`, which describes a different record. A whole-file search would
  therefore contradict the edit-containment requirement; scoping to the AC-8
  value keeps both constraints satisfiable at once (SPEC EC3, A3).

### Edit technique

The scalar is replaced in place, addressed by its position inside the AC-8
entry. No file-wide search-and-replace is used — a blanket replacement would
reach the AC-4 occurrences and break edit containment (SPEC EC3).

### Style

English prose, folded block scalar, indentation and line width comparable to
the file's other `red_reason` scalars, no trailing whitespace.

## Cross-task Design Decisions

### D1: Correction form — drop the needles, state the property

The replacement clause makes no claim at all about the result of searching the
record's own text. It states only what the change set shows directly: exactly
the two enumerated paths, no Rust file, no TypeScript file. Alternative forms
(e.g. re-framing the existing occurrences as negative examples) keep a
self-referential claim alive and were rejected. Resolved at create-spec via the
`create-spec.requirement-clarification` gate (SPEC A1). Affects task0001.

### D2: Verification is scripted and git-based, not suite-based

No automated test suite covers `test-docs/` records (SPEC A5, pinned by
`test/README.md`). Acceptance is verified by an ad-hoc parse/assert script plus
git inspection. **The script is not committed**: it lives in the project's
gitignored `tmp/` or in the session scratchpad, both outside the declared change
set. The project's own build and test commands run only as a no-regression
guard. Affects task0001.

### D3: The no-observable-pre-state framing is retained

The scalar's opening — this task's own commit does not exist before the task
runs, so its change set cannot be enumerated beforehand, and the enumeration was
therefore verified after committing against the merge base — stays. It is the
stated justification for AC-8's `red_confirmed: false` and `tests: []`;
shortening the scalar past it would leave those two values unexplained (SPEC
EC5, FR5). The merge-base revision itself is neither re-derived nor restated
(SPEC A6). Affects task0001.

### D4: Edit containment is a hard boundary, not a best effort

`task_id`, `baseline_failures`, `final_failures`, the AC-1..AC-7 entries, AC-8's
`tests` and `red_confirmed`, and the trailing `notes` block stay byte-identical
to the base revision. The `notes` sentence about the task's own change set is
explicitly out of scope even though it is topically adjacent (SPEC A4). Affects
task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The same defect is reproduced one level up — an acceptance check phrased as "the record contains none of X, Y" is written into the very scalar it searches | Medium | High (the feature would fail its own purpose) | Re-runnable-claim rule above; needles live only in the check script or in this feature's own record (SPEC EC1) |
| A file-wide replacement removes the AC-4 occurrences at lines 43 and 47 | Medium | High (edit containment violated, accurate evidence destroyed) | Targeted in-place replacement of the AC-8 scalar only; region-wise byte comparison against the base revision (SPEC EC3) |
| Folded-scalar re-wrapping silently changes the loaded string (line-break folding, indentation-induced literal newlines) | Medium | Medium (machine readability degrades without a visible diff signal) | Assertions run on the loaded value, not on raw bytes; scalar form and indentation asserted explicitly (SPEC EC4) |
| Shortening the scalar drops the no-observable-pre-state framing | Low | Medium (`red_confirmed: false` and `tests: []` lose their justification) | D3; a dedicated acceptance criterion in task0001 |
| This feature's own generated record repeats the self-enumeration as a search result | Medium | Medium (defect re-appears in a new record) | Re-runnable-claim rule binds the generated record too (SPEC EC2) |

## Open Questions

- [ ] None. Every requirement in `workflow.yaml` is `ok`; no requirement is
      `tbd`, and every FR/NFR maps to task0001 and to at least one test
      scenario.
