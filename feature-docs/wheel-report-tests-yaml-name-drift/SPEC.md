# Feature: wheel-report-tests-yaml-name-drift

## Overview

`test-docs/wheel-report-fraction-accum/task0001.tests.yaml` cites a test name
that no longer resolves to any definition, and its AC-6 / AC-7 `red_reason`
bodies reference `MouseReportRecords.last_tracking_active`, a field that names
no symbol at HEAD. This feature repairs those four enumerated locations so the
record's executable evidence runs again and its narrative rests only on symbols
that exist. The repair is a text fix of broken references, never a
re-judgement of the recorded red verdicts.

Requirements document: `feature-docs/wheel-report-tests-yaml-name-drift/REQUIREMENTS.md`.

## Objectives

- Restore the executable acceptance evidence of task0001: every test name the
  record cites must resolve to a real definition, so re-running the evidence by
  name cannot silently report `0 tests run`.
- Remove from task0001's `red_reason` bodies the identifiers that name no
  symbol at HEAD, so a reader reconstructing the pre-implementation failure
  state is not sent after a field that never existed or no longer exists.
- Keep the historical meaning of the record intact: a `red_reason` documents a
  failure observed before the implementation existed, so the repair never
  re-judges a red verdict.

## User Stories

### US1: Re-run the cited acceptance evidence by name
As a developer re-running the recorded evidence, I want every test name cited
by `task0001.tests.yaml` to resolve to a real definition, so that a
name-filtered `cargo test` run reports a non-zero test count instead of
silently reporting `0 tests run`.

**Acceptance Criteria:**
- [ ] Every test name referenced by the record resolves to a definition that
      exists in `src-tauri/src` (AC-1).
- [ ] The AC-7 entry formerly reading the old name now reads
      `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`,
      and the old name appears nowhere in the file (AC-2).

### US2: Reconstruct the pre-implementation failure state without dangling identifiers
As a reader reconstructing the pre-implementation failure state, I want the
AC-6 / AC-7 `red_reason` bodies to reference only symbols that exist at HEAD,
so that I am not sent after a field that never existed or no longer exists —
while every recorded red verdict keeps the meaning it had when recorded.

**Acceptance Criteria:**
- [ ] `last_tracking_active` appears nowhere in `task0001.tests.yaml`, and the
      AC-6 / AC-7 `red_reason` bodies read coherently without it (AC-3).
- [ ] Every `red_confirmed` value, every acceptance-criterion key, and all text
      outside the four enumerated locations are byte-identical to the
      pre-change file (AC-4).
- [ ] The repaired file parses as valid YAML with the same top-level structure
      (AC-5).
- [ ] No file outside
      `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` is modified
      (AC-6).

## Technical Requirements

### Functional Requirements

- **FR1 — Re-point the unresolvable AC-7 test name:** In
  `test-docs/wheel-report-fraction-accum/task0001.tests.yaml`, replace the AC-7
  `tests` entry at line 62,
  `window_host::tests::decide_wheel_event_detects_reactivation_observed_only_by_an_owner_recorded_release`,
  with the test that superseded it,
  `window_host::tests::decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`.
  The other two AC-7 entries (lines 61 and 63) already resolve and stay
  unchanged.
- **FR2 — Repair the AC-6 red_reason body (line 57):** In the AC-6 `red_reason`
  (lines 54-58), remove the reference to the non-existent field
  `MouseReportRecords.last_tracking_active` at line 57 while keeping the
  `report_accum` half of the quoted `error[E0560]` and the rest of the
  sentence. The AC-6 `red_confirmed: true` flag and the remainder of the
  narrative are not touched.
- **FR3 — Repair the AC-7 red_reason error quotation (line 67):** In the AC-7
  `red_reason` (lines 65-75), remove `last_tracking_active` from the quoted
  no-field error text at line 67, keeping `report_accum` and
  `tracking_active`, which both still name real symbols. The quoted occurrence
  count for `report_accum` and the `red_confirmed: true` flag are not touched.
- **FR4 — Repair the AC-7 red_reason sentence that depends on the removed
  field (lines 73-75):** Rewrite only the clause at lines 73-75 that grounds the
  scenario's inexpressibility in `MouseReportRecords.last_tracking_active`, so
  the sentence rests solely on symbols that exist at HEAD
  (`RecordUpdates.tracking_active`). The surrounding historical narrative —
  including the reference to the coordinator's second-opinion clarification and
  the D10 gap — is preserved.
- **FR5 — Preserve every red verdict and all still-accurate text:** No
  `red_confirmed` flag in `task0001.tests.yaml` changes value, no
  acceptance-criterion entry is added or removed, and no still-accurate
  identifier (`report_accum`, `RecordUpdates.tracking_active`,
  `accumulate_wheel_report_lines`, `MAX_WHEEL_REPORT_NOTCHES`, the
  `wheel_report_notches_*` family) is altered. Edits are confined to the four
  enumerated locations (line 62, and the `red_reason` bodies at 57, 67 and
  73-75).
- **FR6 — Record manual name-resolution evidence for AC-1:** For each test name
  referenced by the repaired `task0001.tests.yaml`, record the file and line of
  its `fn` definition plus a `cargo test` run filtered by that name reporting a
  non-zero test count, and attach that evidence to the task's own verification
  record. No new test and no automated name-drift check is added.

### Non-Functional Requirements

- **NFR1 — Maintainability (documentation-only change):** The change touches
  `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` only. No file
  under `src-tauri/src` or `crates/` is modified, no test is added, renamed or
  deleted, and the `--lib` suite's pass/fail counts are unchanged by this task.
- **NFR2 — Maintainability (preserve the file's YAML form):** The file remains
  valid YAML and keeps its existing shape: the folded block scalars, the
  backslash-escaped backtick convention used inside the quoted compiler errors,
  the key order, and the surrounding line wrapping style.
- **NFR3 — Compatibility (sequencing constraint):** Work starts only after
  PR #73 (`em-workflow/wheel-report-fraction-accum/integration`) is merged,
  since the repaired file and the test definitions it must resolve against both
  come from that branch.

## Implementation Approach

### Architecture

No architecture is involved. The change is a text repair inside a single YAML
evidence record; no production code, no user-visible surface, no data shape and
no architectural choice is touched. The design step was skipped for this
reason.

**Affected artifact:**

```
test-docs/wheel-report-fraction-accum/task0001.tests.yaml
├── acceptance_tests AC-6
│   └── red_reason (lines 54-58)   → FR2: line 57
└── acceptance_tests AC-7
    ├── tests (lines 61-63)        → FR1: line 62 only
    └── red_reason (lines 65-75)   → FR3: line 67
                                   → FR4: lines 73-75
```

### Data Flow

```
task0001.tests.yaml (AC-n.tests[])  →  `cargo test --lib <name>`  →  non-zero test count
task0001.tests.yaml (AC-n.red_reason) →  reader                   →  symbols resolvable at HEAD
```

Before the repair, the first path terminates in `0 tests run` for the AC-7
entry at line 62, and the second path terminates in an identifier that names no
symbol.

### API Design

Not applicable: no API surface is involved.

### Database Schema

Not applicable: no persisted data shape is involved. The repaired file keeps
its existing top-level structure (`task_id`, `baseline_failures`,
`final_failures`, `acceptance_tests` AC-1..AC-10, `notes`), which AC-5 verifies.

### Dependencies

**Internal Dependencies:**

- `wheel-report-fraction-accum` (PR #73,
  `em-workflow/wheel-report-fraction-accum/integration`): supplies both the file
  to repair and the test definitions it must resolve against. Work starts only
  after that PR is merged (NFR3, A6).
- `src-tauri/src/window_host/tests.rs`: holds the definitions every cited test
  name must resolve to, including
  `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`
  at line 3372. It is read for evidence only and never modified.
- `test-docs/wheel-report-fraction-accum/task0002.tests.yaml`: its AC-4 names
  the successor test as re-pointed from the task0001 reactivation test. It
  already references the current name and is not edited.

**External Dependencies:**

None.

### File Structure

```
test-docs/wheel-report-fraction-accum/
└── task0001.tests.yaml      # the only file modified by this feature
```

## Declared Change Set

Feature-specific paths are derived at create-plan from every task's `files`
entries in `workflow.yaml` (`references/phases/create-plan-phase.md`), not
hand-authored here.

Beyond those, this SPEC declares:

- `feature-docs/wheel-report-tests-yaml-name-drift/**` (default member)
- `test-docs/wheel-report-tests-yaml-name-drift/**` (default member)
- `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` — the single file
  this feature repairs

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md` and `retrospect.yaml`; these are
generated and owned by the phase documents and by `references/phase-state.md`.
`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, owned by
`implement-phase.md`. This section cites both and restates none of their rules.

The two default entries are part of the declaration unless explicitly removed;
their absence is never assumed by silence.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it. A
declared path that never materializes is not a violation.

## Test Scenarios

No test is added, renamed or deleted by this feature (NFR1, FR6). The scenarios
below are verification activities performed against the repaired file and the
existing suite.

### Unit Tests

- [ ] **TS-1** (FR1, FR6): Each of the 13 distinct test names referenced across
      AC-1..AC-10 of the repaired `task0001.tests.yaml` is looked up in
      `src-tauri/src/window_host/tests.rs` and its defining line recorded.
      Method: manual evidence (grep for `fn <name>(`), no automation added.
- [ ] **TS-2** (FR1, FR6): `cargo test --lib` filtered by the repaired AC-7 name
      reports a non-zero test count (the pre-change name reports 0).
      Method:
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`.

### Integration Tests

- [ ] **TS-5** (NFR1): Regression guard — the `--lib` suite still passes with the
      same counts, confirming the change is documentation-only.
      Method:
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

No E2E surface is involved; the change is documentation-only.

### Edge Cases

- [ ] **TS-3** (FR1, FR2, FR3, FR4): The repaired file contains no occurrence of
      the old test name and no occurrence of `last_tracking_active`.
      Method: text search of the single changed file.
- [ ] **TS-4** (FR5): The diff is confined to the enumerated locations and leaves
      every `red_confirmed` flag and the AC-9 body untouched.
      Method: `git diff` review.

### Performance Tests

Not applicable: no runtime code path changes.

## Security Considerations

Not applicable. The change is documentation-only: it introduces no
authentication, authorization, input-handling, data-protection, XSS, SQL
injection or CSRF surface.

## Error Handling

No runtime error handling changes. The failure modes this feature addresses are
evidence-level:

| Condition | Pre-change behavior | Post-change behavior |
|------|-------------|--------------|
| A cited AC-7 test name is run by name | `0 tests run`, reported silently | Non-zero test count |
| A reader follows an identifier in an AC-6 / AC-7 `red_reason` | Lands on `MouseReportRecords.last_tracking_active`, which names no symbol at HEAD | Every identifier names a real symbol at HEAD |

## Performance Optimization

Not applicable: the change is documentation-only.

## Success Criteria

- [ ] AC-1: Every test name referenced by `task0001.tests.yaml` resolves to a
      definition that exists in `src-tauri/src` — proven by recorded definition
      file+line plus a name-filtered `cargo test` run with a non-zero count.
- [ ] AC-2: The AC-7 entry reads
      `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`,
      and the old name appears nowhere in the file.
- [ ] AC-3: `last_tracking_active` appears nowhere in the file, and the AC-6 /
      AC-7 `red_reason` bodies read coherently without it.
- [ ] AC-4: Every `red_confirmed` value, every acceptance-criterion key, and all
      text outside the four enumerated locations are byte-identical to the
      pre-change file.
- [ ] AC-5: The repaired file parses as valid YAML with the same top-level
      structure (`task_id`, `baseline_failures`, `final_failures`,
      `acceptance_tests` AC-1..AC-10, `notes`).
- [ ] AC-6: No file outside
      `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` is modified
      (plus the feature's own workflow documents).
- [ ] All functional requirements FR1-FR6 and non-functional requirements
      NFR1-NFR3 are satisfied.
- [ ] All test scenarios TS-1..TS-5 pass.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every FR and NFR is `resolved`; the four open decisions
(`requirement.red-reason-update-style`, `requirement.stale-statement-scope`,
`requirement.ac1-verification-method`, `design-step.recommendation`) were
answered before this document was written.

## Out of Scope

- Adding or deleting any test (stated by the task description).
- Editing the AC-9 `#[allow(dead_code)]` statement at `task0001.tests.yaml`
  lines 95-97 — explicitly excluded by the `requirement.stale-statement-scope`
  answer (`enumerated_only`), since it is an accurate historical record of
  task0001's own state rather than drift.
- Adding an automated test-name drift check or any other new tooling —
  excluded by the `requirement.ac1-verification-method` answer
  (`manual_evidence`).
- Any edit to `test-docs/wheel-report-fraction-accum/task0002.tests.yaml`,
  which already references the current name and needs no repair.
- Any change to `src-tauri/src/window_host/tests.rs` or to the production
  wheel-report code.

## Assumptions

| ID | Assumption | Impact | Reversible |
|----|------------|--------|------------|
| A1 | A `red_reason` records a failure state observed before the implementation existed, so the repair only removes identifiers that name no symbol at HEAD; it never rewrites the body to describe today's green code and never re-judges a `red_confirmed` verdict. Basis: the `requirement.red-reason-update-style` answer (`rewrite_minimal`), the principle already established in this repository by `feature-docs/ac7-red-reason-scope/SPEC.md`. | medium | yes |
| A2 | The AC-9 `#[allow(dead_code)]` statement at lines 95-97 stays as written: a later task changing the implementation does not make an earlier historical record false, and that statement described what task0001 itself did at the time. Basis: the `requirement.stale-statement-scope` answer (`enumerated_only`), consistent with A1. | medium | yes |
| A3 | AC-1 is proven by recorded manual evidence (definition file+line plus a name-filtered `cargo test` run with a non-zero count); no automated name-drift check is introduced. Basis: the `requirement.ac1-verification-method` answer (`manual_evidence`); the task states that adding or removing tests is out of scope. | low | yes |
| A4 | `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero` is the correct successor of the removed AC-7 name, and it still covers the D10 owner-recorded-release-observing-tracking-inactive scenario AC-7 was written for. Basis: `task0002.tests.yaml` AC-4 names it as re-pointed from the task0001 reactivation test, and its definition exists at `src-tauri/src/window_host/tests.rs:3372`. | high | yes |
| A5 | `report_accum` and `RecordUpdates.tracking_active` still name real symbols at HEAD and are therefore left untouched, while `MouseReportRecords.last_tracking_active` does not. Basis: `src-tauri/src/window_host/tests.rs` has 39 occurrences of the `report_accum` / `tracking_active` family and zero occurrences of `last_tracking_active`. `mouse_report.rs` was not among the scan targets, so the field's absence from the production struct was not independently re-verified. | high | yes |
| A6 | Work begins only after PR #73 is merged, and the file repaired is the one on that merged base. Basis: stated as a constraint in the task description. | medium | yes |

## Design Step

Skipped. The change is a text repair inside a single YAML evidence record: four
enumerated locations in `test-docs/wheel-report-fraction-accum/task0001.tests.yaml`,
with no production code, no user-visible surface, no UI or visual element, no
data shape and no architectural choice involved. Every open decision was a
wording/scope question, all four of which are answered, leaving nothing for a
design step to decide.

## References

- Requirements document:
  `feature-docs/wheel-report-tests-yaml-name-drift/REQUIREMENTS.md`
- Repaired file: `test-docs/wheel-report-fraction-accum/task0001.tests.yaml`
- Successor test definition: `src-tauri/src/window_host/tests.rs:3372`
- Successor name provenance:
  `test-docs/wheel-report-fraction-accum/task0002.tests.yaml` AC-4
- Prior art for the repair principle:
  `feature-docs/ac7-red-reason-scope/SPEC.md`
- Upstream merge this work is sequenced after: PR #73
  (`em-workflow/wheel-report-fraction-accum/integration`)
