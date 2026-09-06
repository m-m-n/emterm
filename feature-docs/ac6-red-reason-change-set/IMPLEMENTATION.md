# Implementation Plan: ac6-red-reason-change-set

## Overview

Rewrite the closing change-set clause of the AC-6 `red_reason` in
`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` so the recorded
claim matches the actual change set and is no stronger than the acceptance
criterion it evidences. Everything else in the record stays byte-identical.

## Technology Stack

- **Artifact under change**: a machine-readable YAML evidence record under
  `test-docs/`. No application layer, no runtime component, no build artifact
  participates.
- **Verification tooling**: the PyYAML parser invoked through `python3`, used
  at verification time only to load the record and assert its shape; `git`
  (`diff`, `show`, `status`) for the change-set and locality assertions.

### New dependency licenses

| Dependency | License | Note |
|---|---|---|
| PyYAML (invoked through `python3` at verification time) | MIT | Verification-time only; not added to any manifest and not distributed with any artifact (dev-only dependency, `references/license-compat.md` rule 9). Permissive, compatible with the project's `MIT`. |

No dependency is added to `Cargo.toml`, `package.json` or any other manifest,
so `project.license: MIT` is unaffected. No license conflict exists.

## Layer Structure

Not applicable in the usual sense — the feature has exactly one artifact layer:

| Layer | Responsibility | May depend on |
|---|---|---|
| Evidence record (`test-docs/{feature}/{task}.tests.yaml`) | Machine-readable statement of what was observed for each acceptance criterion | Nothing. It is a leaf document; no code reads it at build or run time. |

There is no allowed dependency direction to state because nothing depends on
anything here.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| — | — | — | — |

This feature decomposes into a single task, so no component is used across
tasks and no cross-task contract needs pinning. The record-shape invariant
below is stated as a convention rather than a shared component because it
constrains the one task's output rather than an interface between tasks.

## Conventions

### C-1: Record-shape invariance

Any edit to a `taskNNNN.tests.yaml` record preserves the record's parsed
shape: the top-level key set (`task_id`, `baseline_failures`,
`final_failures`, `acceptance_tests`, `notes`), the top-level key order in the
raw file, and the `acceptance_tests` entry set (here: exactly AC-1 through
AC-7). A change to a `red_reason` never becomes a change to the record's
schema.

### C-2: Claim strength matches the criterion

A `red_reason` states no claim stronger than the acceptance criterion it
evidences. When the criterion asks for the absence of a category of file, the
reason asserts that absence and does not additionally assert a file count, a
sole-entry claim, or any narrower enumeration of the change set. An
over-claim is a defect independent of whether it happens to be true.

### C-3: Carve-out naming

When a `red_reason` characterises a change set, workflow-generated artifacts
are named explicitly as an expected carve-out rather than left implicit. For
this feature the carve-out wording covers both the `feature-docs/{feature}/**`
tree and the `test-docs/` record tree, since both are produced by the workflow
rather than being product source.

### C-4: After-the-fact verifiability

A `red_reason` is phrased so every claim it makes can be re-checked later from
the record itself plus the referenced commit. Wording that depends on a
transient working-tree state at edit time (for example, "at the time of this
run the working tree showed …") is avoided, because a later reader has no way
to reconstruct that state.

### C-5: Formatting fidelity

An edit to an existing scalar keeps that scalar's existing block indicator
(here `>-`), the file's 2-space indentation, and the file's existing line-wrap
width, so the diff reads as a same-style revision rather than a reformat.

## Cross-task Design Decisions

### D-1: Scope is the final change-set clause of AC-6's `red_reason` only

**Decision**: The rewrite replaces the closing clause that asserts a
whole-change-set `git status --porcelain` listing only that one YAML file.
Every other element of the AC-6 entry is preserved: `red_confirmed: false`,
the invariant-guard framing, the clean pre-state observation, the two-hunk
`git diff` observation, and the untouched-keys list. `notes` is out of scope
even though it carries similar narrowness (assumption A-1).

**Rationale**: The defect is the strength and accuracy of one clause. A wider
rewrite would put FR4 (preserved evidence) and FR5 (other entries untouched)
at risk for no requirement gain.

**Affected**: task0001.

### D-2: The replacement asserts absence, names the records, and names the carve-out

**Decision**: The replacement clause carries exactly three assertions —
(a) the change set consists of YAML documentation records only, naming both
the record the ac7 task edited and that task's own per-task test record;
(b) it contains no Rust file and no TypeScript file; (c) the
workflow-generated documents under `feature-docs/ac7-red-confirmed-unobserved/**`
and the `test-docs/` record tree are an expected carve-out that does not
affect (b). It asserts no file count and no sole-entry claim.

**Rationale**: (a) and (b) satisfy FR1 under convention C-2; (c) satisfies FR2
under convention C-3; the exclusions satisfy FR3.

**Affected**: task0001.

### D-3: No test harness file is committed

**Decision**: The verification assertions for this feature are run as ad-hoc
commands at implement and verify time. No test script, fixture or harness file
is created or modified anywhere in the repository, and in particular nothing
under `src-tauri/`, `crates/` or `scripts/`, and no `.rs` / `.ts` / `.css`
path.

**Rationale**: NFR3 and the feature's own AC-6 require the change set to
contain no Rust and no TypeScript file; the project has no test runner that
would host assertions over a documentation record; and committing a harness
for a one-clause prose edit would violate the same claim the feature exists to
make accurate. Evidence of the assertions having run is recorded in the
implement phase's own test record under
`test-docs/ac6-red-reason-change-set/`, which is a workflow-generated artifact
of the declared change set.

**Affected**: task0001, and the verify phase.

### D-4: Project build/test commands are not an acceptance gate

**Decision**: `bun test`, `bun run typecheck` and
`cargo test --manifest-path src-tauri/Cargo.toml --lib` are run, if at all, as
an unchanged-baseline regression check only. They are not part of this
feature's acceptance (assumption A-6).

**Rationale**: The change set exercises no compiled or bundled code, so those
commands cannot observe the feature's effect. Treating them as a gate would
attribute unrelated pre-existing failures to this feature.

**Affected**: task0001, and the verify phase.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The target record `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` is absent from this feature's base branch. **Retired — the orchestrator verified the path is present in the integration worktree** (the ac7 work is on `main`). Retained as a record of the check, not as a live risk. | Retired | — | If the path is nevertheless missing at implementation time, the implementer treats it as a blocking plan deviation and reports it. It never creates the record, and never redirects the edit to a different path: creating it would fabricate evidence for a task that did not run here. |
| The rewrite silently reflows or re-indents the whole `red_reason`, producing a diff far larger than the changed clause and obscuring FR5 verification. | Medium | Medium | Convention C-5 plus the raw-text checks in VERIFICATION.md (TS-6) and the diff-locality check (TS-5, SPEC AC-5). |
| The replacement clause reintroduces an over-claim in different words (for example, enumerating the change set exhaustively, which is again a count claim). | Medium | Medium | Convention C-2, decision D-2's explicit three-assertion list, and the acceptance criteria of task0001 that assert the absence of count/sole-entry claims. |
| The rewritten text is phrased against the state of the working tree during this feature's run rather than against the ac7 task's own commit, breaking NFR4. | Low | Medium | Convention C-4 and TS-8. |
| The single-file phrase is removed from `red_reason` but the same narrowness remains in the record's trailing `notes`, so a downstream reader is still misled. | High (it is known to remain) | Low | Deliberately accepted and documented: assumption A-1 fixes scope to the acceptance entries. Recorded below as an open question rather than silently absorbed. |

## Open Questions

- [x] **Resolved by the orchestrator before implement.** The target record
      `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` is present
      in the integration worktree — the ac7 work is on `main`, so no
      re-planning against a different base is needed.
- [ ] The record's trailing `notes` block carries the same narrowness
      ("git diff/status inspection of the single changed file") and is left
      unchanged by assumption A-1. Should a follow-up feature correct it, or
      is the acceptance-entry correction sufficient?
- [x] **Resolved by the orchestrator before implement.** Assumption A-5's
      missing sources ARE available in the integration worktree:
      `feature-docs/ac7-red-confirmed-unobserved/tasks/task0001.md` (the AC-6
      definition) and `feature-docs/ac7-red-confirmed-unobserved/SPEC.md`
      (the "Declared Change Set" section and NFR4). The implementer reads
      these two files directly and matches the rewritten clause to the actual
      criterion wording, instead of relying on A-5's reconstruction from the
      task description. Neither file is edited by this task.
