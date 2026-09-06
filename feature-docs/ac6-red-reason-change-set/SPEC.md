# Feature: ac6-red-reason-change-set

## Overview

The AC-6 entry of `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
carries a `red_reason` whose closing clause asserts that `git status --porcelain`
for the whole change set lists just that one YAML file. That statement does not
match the actual change set, and it is strictly stronger than what AC-6 requires.
This feature rewrites that clause so the record states only what AC-6 asks for —
that the change set contains no Rust and no TypeScript file — while naming the
YAML documentation records that make up the change set and the workflow-generated
carve-out.

Requirements source: `feature-docs/ac6-red-reason-change-set/REQUIREMENTS.md`.

## Objectives

- **BO-1:** `taskNNNN.tests.yaml` records are machine-readable evidence consumed
  by later tasks and reviews. Make the AC-6 change-set claim in
  `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` match the actual
  change set, so no downstream reader concludes from the record that the
  ac7-red-confirmed-unobserved feature touched only one file.
- **BO-2:** Keep each `red_reason`'s claim no stronger than the acceptance
  criterion it evidences. AC-6 requires only "the change set contains no Rust and
  no TypeScript file"; the record currently asserts the strictly stronger
  single-file claim, which is an over-claim independent of whether it is true.

## User Stories

### US1: Read the AC-6 change-set evidence without being misled

As a downstream task or reviewer reading `taskNNNN.tests.yaml` as machine-readable
evidence, I want the AC-6 `red_reason` to describe the change set accurately, so
that I do not conclude the ac7-red-confirmed-unobserved feature changed only one
file.

**Acceptance Criteria:**
- [ ] The AC-6 `red_reason` no longer states that the change set is that one YAML
      file alone, and asserts no change-set file count.
- [ ] The AC-6 `red_reason` states that the change set is YAML documentation only
      and contains no Rust and no TypeScript file.
- [ ] The AC-6 `red_reason` names the `feature-docs/` workflow-generated artifacts
      as an expected carve-out.

## Technical Requirements

### Functional Requirements

- **FR1 — Correct the AC-6 change-set statement:** In the AC-6 entry of
  `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`, replace the clause
  asserting that `git status --porcelain` for the whole change set lists just that
  one YAML file with a statement that the change set consists of YAML
  documentation records only — the record edited by that task and the per-task
  test record itself — and contains no Rust and no TypeScript file.
- **FR2 — State the workflow-artifact carve-out explicitly:** The rewritten
  `red_reason` names the workflow-generated documents under
  `feature-docs/ac7-red-confirmed-unobserved/**` (and the `test-docs/` record tree)
  as a carve-out that is expected in the change set and does not affect the
  "no Rust, no TypeScript" claim.
- **FR3 — Limit the claim to what AC-6 requires:** The rewritten text asserts only
  the absence of Rust and TypeScript source files in the change set. It does not
  assert a file count, and does not assert that any particular single file is the
  sole entry of `git status --porcelain`.
- **FR4 — Preserve the surrounding AC-6 evidence:** The rest of the AC-6 entry is
  preserved: `red_confirmed: false`, the "Invariant guard, not a red->green
  criterion" framing, the clean pre-state observation, the two-hunk `git diff`
  observation, and the list of untouched keys (header comment, `task_id`,
  `baseline_failures`, `final_failures`, record AC-1 through record AC-6).
- **FR5 — Change no other acceptance entry:** AC-1 through AC-5 and AC-7,
  `task_id`, `baseline_failures`, `final_failures` and the top-level key order are
  left byte-identical.

### Non-Functional Requirements

- **NFR1 — Parseability:** The file continues to parse as YAML and continues to
  expose the same key set and shape (`task_id`, `baseline_failures`,
  `final_failures`, an `acceptance_tests` mapping with exactly its seven entries,
  `notes`).
- **NFR2 — Formatting fidelity:** The rewritten `red_reason` keeps the `>-` folded
  block-scalar indicator and the file's existing 2-space indentation and line-wrap
  style, so the edit reads as a same-style revision rather than a reformat.
- **NFR3 — Documentation-only change set:** No file under `src-tauri/`, `crates/`,
  `scripts/` or any `.rs` / `.ts` / `.css` path is modified, so no build or bundle
  output changes.
- **NFR4 — After-the-fact verifiability:** The rewritten text stays verifiable
  after the fact — every claim it makes can be re-checked by reading the record and
  inspecting the ac7 task's own commit, with no dependency on a pre-edit working
  state.

## Implementation Approach

### Architecture

The change is confined to a single machine-readable YAML record. No application
layer, no runtime component and no build artifact participates.

```
test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml
└── acceptance_tests
    └── AC-6
        ├── tests            (unchanged)
        ├── red_confirmed    (unchanged: false)
        └── red_reason       (rewritten: final change-set clause only)
```

### Data Flow

```
Downstream reader → PyYAML load of the record → acceptance_tests["AC-6"]["red_reason"]
                  → reads: YAML documentation records only, no Rust, no TypeScript,
                    feature-docs/** workflow artifacts as an expected carve-out
```

### Edit shape

| Element of the AC-6 entry | Disposition |
|---|---|
| `red_confirmed: false` | Preserved (FR4) |
| "Invariant guard, not a red->green criterion" framing | Preserved (FR4) |
| Clean pre-state observation | Preserved (FR4) |
| Two-hunk `git diff` observation | Preserved (FR4) |
| Untouched-keys list (header comment, `task_id`, `baseline_failures`, `final_failures`, record AC-1..AC-6) | Preserved (FR4) |
| Final change-set clause | Rewritten (FR1, FR2, FR3) |
| `>-` folded block-scalar indicator, 2-space indentation, wrap style | Preserved (NFR2) |

### API Design

Not applicable — this feature exposes no interface.

### Database Schema

Not applicable — no persisted schema is involved.

### Dependencies

**Internal Dependencies:**
- `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`: the record whose
  AC-6 `red_reason` is rewritten.

**External Dependencies:**
- PyYAML (`python3 -c "import yaml; ..."`): used by the verification checks
  (AC-4, TS-1) to load the record.

### File Structure

```
test-docs/
└── ac7-red-confirmed-unobserved/
    └── task0001.tests.yaml     # AC-6 red_reason rewritten
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths above are derived at create-plan from every task's
`files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths above:

- `feature-docs/ac6-red-reason-change-set/**`
- `test-docs/ac6-red-reason-change-set/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and
restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is a
deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (NFR1): PyYAML load of
      `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` succeeds; assert
      the `acceptance_tests` key set equals {AC-1..AC-7} and `AC-6.red_confirmed`
      is `False`. Covers NFR1 / AC-4.
- [ ] **TS-2** (FR1, FR3): Fixed-string check on the loaded AC-6 `red_reason` — the
      occurrence count of the single-file phrase is 0. Before the edit it is 1.
      Covers FR1 / FR3 / AC-1 as a red->green criterion.
- [ ] **TS-3** (FR1): Substring check on the loaded AC-6 `red_reason` for the
      "no Rust" / "no TypeScript" claim and for both record paths. Covers FR1 / AC-2.
- [ ] **TS-4** (FR2): Substring check on the loaded AC-6 `red_reason` for
      `feature-docs/`. Before the edit the count is 0. Covers FR2 / AC-3 as a
      red->green criterion.
- [ ] **TS-6** (NFR2): Raw-text check that the AC-6 `red_reason` still uses the `>-`
      folded block-scalar indicator, and that the top-level key order in the raw
      file is unchanged. Covers NFR2.

### Integration Tests

- [ ] **TS-5** (FR5): Load the pre-edit and post-edit versions of the file and
      compare every acceptance entry other than AC-6 plus `task_id` /
      `baseline_failures` / `final_failures` for equality. Covers FR5 / AC-5.
- [ ] **TS-7** (NFR3): `git status --porcelain` / `git diff --stat` for this
      feature's change set lists no path ending in `.rs` or `.ts`. Covers NFR3 /
      AC-6 of this feature (an invariant guard with no observable pre-state).

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

The project commands (`bun test`, `bun run typecheck`,
`cargo test --manifest-path src-tauri/Cargo.toml --lib`) are not part of this
feature's acceptance: the change set is a YAML documentation edit that exercises
no compiled or bundled code. Verification is YAML parse plus text assertions.

### Edge Cases

- [ ] **TS-8** (FR4, NFR4): Read the rewritten AC-6 `red_reason` and confirm the
      preserved evidence is intact — `red_confirmed: false`, the invariant-guard
      framing, the clean pre-state observation, the two-hunk `git diff`
      observation, the untouched-keys list — and that every claim it makes is
      re-checkable from the record plus the ac7 task's own commit without a
      pre-edit working state. Covers FR4 / NFR4.

### Performance Tests

Not applicable — no runtime behaviour changes.

## Security Considerations

Not applicable. The change set is a prose revision inside one machine-readable
YAML record under `test-docs/`; no authentication, authorization, input handling
or data-protection surface is involved.

## Error Handling

Not applicable — no runtime code path is added or changed.

## Performance Optimization

Not applicable — no runtime behaviour changes.

## Success Criteria

- [ ] **AC-1:** The AC-6 `red_reason` no longer contains the single-file phrase
      naming that one YAML file as the whole change set, and contains no assertion
      of a change-set file count.
- [ ] **AC-2:** The AC-6 `red_reason` states that the change set is YAML
      documentation only — naming both the record the ac7 task edited and that
      task's own per-task test record — and that it contains no Rust and no
      TypeScript file.
- [ ] **AC-3:** The AC-6 `red_reason` mentions `feature-docs/` workflow-generated
      artifacts as an expected carve-out of the change set.
- [ ] **AC-4:** `python3 -c "import yaml; yaml.safe_load(open(PATH))"` succeeds, and
      the loaded mapping still has exactly the seven `acceptance_tests` entries AC-1
      through AC-7 with `AC-6.red_confirmed` still `false`.
- [ ] **AC-5:** `git diff` for the file shows hunks confined to the AC-6 entry's
      `red_reason`; every other acceptance entry and the top-level keys are
      untouched.
- [ ] **AC-6:** The change set for this feature contains no Rust and no TypeScript
      file (the change set is the edited YAML record plus this feature's own
      `feature-docs/ac6-red-reason-change-set/**` and
      `test-docs/ac6-red-reason-change-set/**` workflow artifacts).

## Design Step

Skipped. No user-visible surface is involved. The change set is a prose revision
inside one machine-readable YAML record under `test-docs/`; there is no UI, no
rendered output, no design-token consumer, and no interaction affected.

## Assumptions

- **A-1:** Scope is the AC-6 `red_reason` only. The record's trailing `notes` block
  also says acceptance was verified by "git diff/status inspection of the single
  changed file", which carries the same narrowness, but the stated definition of
  done covers acceptance entries only, so `notes` is left unchanged.
- **A-2:** `red_confirmed: false` for AC-6 stays false. The finding is about the
  text of the reason, not about whether the criterion was a red->green criterion;
  AC-6 remains an invariant guard.
- **A-3:** The task description's report that implementation commit fc7af5d6
  contains two or more files is taken as given. The claim is corroborated by the
  record's own content: AC-1 shows the task edited
  `test-docs/stale-test-name-refs/task0001.tests.yaml`, a different file from the
  record itself, so the change set is at least two files.
- **A-4:** The carve-out wording covers both `feature-docs/**` and `test-docs/**`
  workflow-generated artifacts, since both are produced by the workflow rather than
  being product source.
- **A-5:** The normative sources the task description cites — the ac7 feature's
  `task0001.md` AC-6 definition and its SPEC's "Declared Change Set" and NFR4 — were
  not available as inputs, so AC-6's original requirement wording is reconstructed
  from the task description alone.
- **A-6:** The project commands (`bun test`, `bun run typecheck`,
  `cargo test --manifest-path src-tauri/Cargo.toml --lib`) are not part of this
  feature's acceptance: the change set is a YAML documentation edit that exercises
  no compiled or bundled code. Verification is YAML parse plus text assertions.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. FR1–FR5 and NFR1–NFR4 are all `status: resolved`.

## References

- Requirements document: `feature-docs/ac6-red-reason-change-set/REQUIREMENTS.md`
- Record under correction: `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
