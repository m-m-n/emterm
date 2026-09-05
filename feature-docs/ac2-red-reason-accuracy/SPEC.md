# Feature: ac2-red-reason-accuracy

## Overview

The AC-2 entry's `red_reason` in `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
claims that none of the four required elements were present in the pre-edit AC-7 `red_reason`
of `test-docs/stale-test-name-refs/task0001.tests.yaml`. Three of the four were in fact
absent; the fourth — the linkage to the record's AC-2 repository-wide count explanation —
already existed in the base text. This feature rewrites that one scalar so it describes only
states that were actually observed, while leaving the red verdict itself untouched.

Requirements source: `feature-docs/ac2-red-reason-accuracy/REQUIREMENTS.md`.

## Objectives

- Keep `taskNNNN.tests.yaml` records usable as machine-readable evidence for later tasks by
  ensuring every `red_reason` describes only states that were actually observed (BO-1).
- Correct the AC-2 justification in `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
  without changing the (valid) red verdict, so a future reader is not handed a false premise
  about the base-revision text (BO-2).

## User Stories

### US1: Trustworthy red justification

As a reader of a later task who uses `taskNNNN.tests.yaml` as machine-readable evidence, I want
the AC-2 `red_reason` to state the correct count of missing elements, so that I am not handed a
false premise about the base-revision text.

**Acceptance Criteria:**
- [ ] AC-1: Parsing the file and reading `acceptance_tests['AC-2']['red_reason']` yields text
      stating that three of four required elements were missing before the edit.
- [ ] AC-2: The same parsed scalar states that the fourth element, the AC-2 repository-wide
      count linkage, already existed in the base text and was preserved.
- [ ] AC-3: The parsed scalar no longer contains a claim that none of the four elements were
      present.

### US2: Verdict and surrounding record left intact

As a reader of the same record, I want the red verdict and every other part of the file to stay
exactly as they are, so that the correction is a text fix and not a re-judgement.

**Acceptance Criteria:**
- [ ] AC-4: `acceptance_tests['AC-2']['red_confirmed']` is the boolean `true`.
- [ ] AC-5: The whole file loads as valid YAML with its original shape.
- [ ] AC-6: No acceptance entry other than AC-2, and no other top-level key, differs after the
      change.

## Technical Requirements

### Functional Requirements

- **FR1 - State the correct count of missing elements:** The AC-2 entry's `red_reason` in
  `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` states that three of the four
  required elements were absent from the pre-edit AC-7 `red_reason` of
  `test-docs/stale-test-name-refs/task0001.tests.yaml`, rather than claiming that none of the
  four were present.
- **FR2 - Record the fourth element as pre-existing and preserved:** The same `red_reason`
  states that the fourth element — the linkage to the record's AC-2 repository-wide count
  explanation — was already present in the base text and was kept by the edit, not newly
  introduced by it.
- **FR3 - Enumerate the three genuinely absent elements:** The rewritten text continues to name
  the three elements that were in fact absent before the edit: the "invariant guard" phrase, the
  mention of record AC-6, and the "no observable pre-state" phrasing.
- **FR4 - Preserve the accurate post-edit half:** The post-edit portion of the reason is
  preserved in substance: after the edit the scripted check finds all four elements present and
  finds no "confirmed by" / "observed" red-observation language in the rewritten AC-7 text.
- **FR5 - Keep the red verdict:** `acceptance_tests['AC-2']['red_confirmed']` remains the
  boolean `true` — three missing elements are sufficient to have made the criterion red, so the
  verdict itself is not revised.
- **FR6 - Confine the change to the AC-2 entry:** Only the AC-2 entry's `red_reason` scalar
  changes. `tests`, `red_confirmed`, every other acceptance entry (AC-1, AC-3 through AC-7),
  `task_id`, `baseline_failures`, `final_failures` and the trailing `notes` block are
  byte-identical after the change.

### Non-Functional Requirements

- **NFR1 - Parseability:** The file continues to parse under PyYAML with its existing key set,
  seven `acceptance_tests` entries, and unchanged top-level key order.
- **NFR2 - Formatting fidelity:** The edited scalar keeps the `>-` folded block-scalar indicator
  and the surrounding indentation / line-wrap style used throughout the file.
- **NFR3 - Language:** The record stays in English, matching every other entry in the file.
- **NFR4 - No unobserved assertion:** The corrected text must not itself assert an observation
  that was not made; it describes the pre-edit state only in terms the base blob actually
  supports.
- **NFR5 - No build impact:** No Rust or TypeScript source changes and no rebuild: this is a
  single-file YAML documentation correction.

## Implementation Approach

### Architecture

No architectural surface is involved. The change is a text edit to one folded block scalar in
one YAML record.

**Component Diagram:**
```
test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml
  └── acceptance_tests
        └── AC-2
              ├── red_reason      <- the only scalar this feature rewrites
              ├── red_confirmed   <- stays boolean true (FR5)
              └── tests           <- unchanged (FR6)
```

### Data Flow

```
base blob (9eee6161: test-docs/stale-test-name-refs/task0001.tests.yaml)
   → evidence for what the pre-edit AC-7 red_reason actually contained
   → rewritten AC-2 red_reason text in test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml
```

### API Design

Not applicable — no API surface.

### Database Schema

Not applicable — no database. The record's structure is the YAML shape below.

| Key | Type | Changes | Description |
|--------|------|------|-------------|
| `task_id` | scalar | No | Unchanged (FR6) |
| `baseline_failures` | scalar | No | Unchanged (FR6) |
| `final_failures` | scalar | No | Unchanged (FR6) |
| `acceptance_tests` | mapping (7 entries) | Only `AC-2.red_reason` | Entry count and order unchanged (NFR1) |
| `acceptance_tests['AC-2']['red_reason']` | string (`>-` folded) | Yes | The corrected justification (FR1–FR4) |
| `acceptance_tests['AC-2']['red_confirmed']` | boolean | No | Stays `true` (FR5) |
| `notes` | block | No | Unchanged (FR6) |

### Dependencies

**Internal Dependencies:**
- `test-docs/stale-test-name-refs/task0001.tests.yaml` at revision `9eee6161`: the base text the
  corrected justification describes. It is read as evidence and is not modified by this feature.

**External Dependencies:**
- PyYAML: used to load the record when verifying AC-1 through AC-5.
- `git show` / `git diff` / `git status`: used to read the base blob and to confirm change
  containment for AC-6.

### File Structure

```
test-docs/
└── ac7-red-confirmed-unobserved/
    └── task0001.tests.yaml     # the only file this feature edits
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/{feature}/**`
- `test-docs/{feature}/**`

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

- [ ] TS-1 (FR1, FR2, FR3, FR4): Load the record with PyYAML before and after the change; assert
      AC-2's `red_reason` moves from the "none of the four" claim to the "three of four missing,
      fourth pre-existing" claim, asserting on the parsed value rather than raw line content (the
      folded scalar re-wraps).
- [ ] TS-2 (FR5): Assert `red_confirmed` for AC-2 is `True` both before and after — an invariant
      guard, so no observable red pre-state exists for it.
- [ ] TS-5 (NFR1, NFR2): Confirm the raw file still uses `>-` for the AC-2 `red_reason` and that
      the header/top-level key order is unchanged.

### Integration Tests

- [ ] TS-3 (FR6, NFR5): Diff every acceptance entry except AC-2 and the `notes` block between
      base and result; assert equality.
- [ ] TS-4 (FR2, NFR4): Re-read the base blob
      (`git show 9eee6161:test-docs/stale-test-name-refs/task0001.tests.yaml`) and confirm the
      AC-7 `red_reason` there contains the AC-2 count linkage, so the rewritten justification
      matches the evidence it cites.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] The folded scalar re-wraps on rewrite, so raw-line assertions are unreliable; assertions
      run against the PyYAML-parsed value (TS-1).
- [ ] The base blob may not contain the AC-2 count linkage assumed by A-1; TS-4 checks this
      before the text is rewritten, since NFR4 forbids asserting an observation the base blob
      does not support.

### Performance Tests

Not applicable — single-file YAML documentation correction (NFR5).

## Security Considerations

Not applicable — no code path, no input handling, and no data surface is touched; the change is
a documentation-text correction in one YAML record (NFR5).

## Error Handling

No runtime error surface exists for this change. The failure modes are verification failures:

| Condition | Detected by | Response |
|------|-------------|--------------|
| Rewritten text contradicts the base blob | TS-4 | Correct the text to what the base blob supports (NFR4) |
| A key other than AC-2's `red_reason` changed | TS-3, AC-6 | Revert the out-of-scope change (FR6) |
| File no longer parses or loses its shape | AC-5, TS-5 | Restore the `>-` scalar style and key order (NFR1, NFR2) |

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] FR1 through FR6 are implemented and verified.
- [ ] NFR1 through NFR5 hold after the change.
- [ ] AC-1 through AC-6 pass.
- [ ] TS-1 through TS-5 pass.
- [ ] `git status --porcelain` lists only
      `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is `status: resolved`.

## Assumptions

These are the assumptions the requirements analysis carried; they are recorded here unchanged.

- **A-1:** The base revision `9eee6161` AC-7 `red_reason` contains "which is what keeps the AC-2
  repository-wide count at 6 instead of climbing to 7". Taken from task_description; NOT
  independently verified in the analyst dispatch — the implementer confirms it via the `git show`
  in TS-4 before rewriting the text.
- **A-2:** The three elements named in the current parenthetical ("invariant guard" phrase,
  mention of record AC-6, "no observable pre-state" phrasing) are exactly the three that were
  genuinely absent.
- **A-3:** The trailing `notes` block needs no change; the task's completion definition
  constrains only acceptance entries.
- **A-4:** The record stays in English; the whole file is English today.
- **A-5:** project_commands (bun test / bun run typecheck / cargo test) are not required for this
  change set, since it touches no Rust and no TypeScript source — the same rationale the record's
  own `notes` already documents.
- **A-6:** The file to edit is `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
  (present at the integration worktree HEAD); the record it *describes* is
  `test-docs/stale-test-name-refs/task0001.tests.yaml`, which is not modified by this feature.

## Design Step

Skipped. Single-file YAML documentation-text correction with no UI, no visual surface, no new
module boundary and no API change; the design-system candidates detected are untouched by it.

## References

- Requirements document: `feature-docs/ac2-red-reason-accuracy/REQUIREMENTS.md`
- Record under correction: `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
- Record it describes (not modified): `test-docs/stale-test-name-refs/task0001.tests.yaml`
- Base revision for the described text: `9eee6161`
