# Implementation Plan: ac7-red-reason-scope

## Overview

One documentation-record correction: the AC-7 entry's folded `red_reason` scalar in
`test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` is rewritten so that it describes the
change set that was actually observed, while the red verdict and every other part of the
record stay untouched. No product source is involved.

Per-task detail lives in `tasks/task0001.md`; this document carries only the decisions that
reach beyond that single task — the record-text conventions this feature's OWN artifacts must
also satisfy (D4), and the decomposition/metadata rationale (D5, D6).

## Technology Stack

- **Language / Framework**: none. The deliverable is English prose inside a YAML folded block
  scalar. No Rust and no TypeScript source changes, and no rebuild (NFR5).
- **Verification tooling**: PyYAML — loads the record so assertions run against the parsed
  scalar rather than raw lines; git (`diff` / `status` / commit stat) — establishes change
  containment and enumerates change sets.
- **New dependency licenses** (project license: MIT):
  - PyYAML — MIT. Permissive, and verification-time only: it is not added to any project
    manifest and is not distributed in the built artifact
    (`references/license-compat.md`, permissive category and the dev-only-dependency rule).
    Compatible with MIT; no conflict.
  - No other dependency is introduced by this feature.

## Layer Structure

There is no executable layer. One artifact layer participates:

| Layer | Contents | Direction |
|---|---|---|
| Evidence records (`test-docs/{feature}/{T}.tests.yaml`) | Machine-read per-task test records that later tasks and automation read for failure attribution | The record text depends on observed git facts; nothing depends on the record at build or run time |

The record is data, not configuration: editing its text changes no tooling behaviour.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | The feature decomposes into exactly one task (D5), so no component is used across tasks | — | — |

No cross-task component use exists, therefore no contract needs pinning here. Should a later
re-plan split this feature, the record-text conventions below become the shared contract.

## Conventions

These apply to every artifact this feature produces — the edited record, the task plan,
VERIFICATION.md, the implement phase's generated `test-docs/ac7-red-reason-scope/task0001.tests.yaml`,
and any review or retrospect text — not to the edited record alone.

- **Language**: English, matching every other entry of the edited file (NFR3).
- **Scalar form**: a rewritten folded scalar keeps the `>-` indicator at the indentation it
  already has and the line-wrap style its neighbours use (NFR2). Line breaks inside a folded
  scalar carry no meaning, so no assertion is ever written against raw line content.
- **Observation discipline**: a record states only what an actual observation supports. A
  claim whose backing state nobody inspected is the defect class this feature exists to
  remove (NFR4).
- **Change-set scoping**: wherever a change set is described, its members are named. A
  statement about a whole change set is never narrowed to a single member (NFR6). The edited
  record's own change set, and this feature's change set, are both described as the set of
  paths they contain.
- **Error-handling policy**: no runtime error surface exists. The failure modes are
  verification failures, handled per SPEC.md's Error Handling table: out-of-scope key changed
  → revert it; text outruns the observation → bring the text back to the observation; shape
  or format lost → restore indicator, key order and entry count.

## Cross-task Design Decisions

### D1: Evidence-first ordering

The exact path list of the described task's commit is established from git BEFORE the
replacement text is written (A-3, NFR4). Planning did not independently re-derive it, so the
two paths named in FR1 are an expectation, not an observation, until the implementer confirms
them. If the observed list differs from FR1's two paths, the text follows the observation and
the divergence is reported as a plan deviation rather than being written into the record.

Affected: task0001 and every later reader of the record.

### D2: Locate the entry by content

The AC-7 entry is located through the `acceptance_tests` mapping's `AC-7` key, never through
the line number quoted in the task description (A-4). Line numbers shift when a folded scalar
re-wraps.

### D3: Containment discipline

Exactly one scalar changes. The top-level scalars, the AC-1 through AC-6 entries, the AC-7
entry's `tests` and `red_confirmed` values, and the trailing `notes` block stay byte-identical
(FR6). The `notes` block already describes the verification method for AC-6/AC-7 using
single-file wording about the file that task edited; it is deliberately left as it stands
(A-5). If the implementer judges that wording to be a second instance of the same scoping
defect, that judgement is surfaced as a plan deviation and is not fixed silently — fixing it
would break FR6.

### D4: Self-consistent scoping across this feature's own artifacts

This feature's own task commit is expected to contain two paths — the edited
`test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` and the implement phase's generated
`test-docs/ac7-red-reason-scope/task0001.tests.yaml` — alongside the orchestrator's own
`feature-docs/ac7-red-reason-scope/**` documents, all inside the SPEC's Declared Change Set
(A-2). Every artifact this feature produces therefore states its own change set as that set
of paths. Reproducing the single-member phrasing in a third record would recreate exactly the
defect under repair (NFR6, AC-8).

This is the one decision that spans artifacts beyond the single task — the generated test
record is written by the implement phase, and the verification text by this planning phase —
which is why it lives here rather than in the task plan. It is mechanically checkable: see
VERIFICATION.md TS-5.

### D5: Decomposition — one task

The feature is a rewrite of one folded scalar in one file. Splitting it would put two tasks
on the same scalar with no independent deliverable on either side, so it decomposes into
exactly one task.

`task0001` carries eight acceptance criteria, one more than the task-decomposition size
heuristic's soft ceiling. The deviation is deliberate: the criteria are kept in a 1:1
correspondence with SPEC.md AC-1 through AC-8 so traceability from the SPEC needs no mapping
table, and all eight constrain the same scalar (or this feature's own change set), which is
not divisible into independently implementable work.

### D6: Task metadata rationale

- `domains: [config-infra]` — the edited artifact is machine-read data belonging to the
  workflow tooling rather than to the product. No product surface (auth, input handling,
  persistence, external I/O, concurrency, API contract, UI) is touched; `config-infra` is
  declared as the closest true domain so the review floor is not computed from an empty
  domain set.
- `skills: []` — no entry in the implementation-skill registry matches: the task's primary
  output is neither visual design, nor client-side behaviour, nor server/core-domain logic,
  nor configuration/automation. The registry's explicit fallback (empty list) applies; a
  layer skill is not force-fitted.
- `complexity: low` — a localized text change in one file, no new component, no contract
  change, and an obvious verification approach (load the record, assert on the parsed
  scalar, diff the regions).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The described task's commit does not contain exactly the two paths FR1 names, so the replacement text would assert an uninspected state | Medium | High | D1: confirm the path list from the commit first; follow the observation and report the divergence (A-3, NFR4) |
| Raw-line assertions break because the folded scalar re-wraps on rewrite | High | Medium | Assert on the PyYAML-parsed value throughout (TS-1, TS-2) |
| An out-of-scope key or the `notes` block changes while editing | Low | High | D3 plus the region-level byte-identity check (TS-4, AC-6) |
| This feature's own record repeats the single-member change-set phrasing, recreating the defect in a third record | Medium | High | D4, made checkable by TS-5 and task AC-8 |
| The `>-` indicator, indentation or top-level key order is lost during the rewrite | Low | Medium | Shape and format re-inspection after the edit (TS-3, AC-7) |

## Open Questions

- [ ] A-3: the exact path list of the described task's commit was not re-derived from git
      during planning. The implementer confirms it before writing the text; a mismatch with
      the two paths FR1 names is a plan deviation to report, not a text to invent.
- [ ] A-5: whether the `notes` block's single-file wording is a second instance of the same
      scoping defect is left open on purpose. FR6 keeps `notes` byte-identical, so any such
      judgement is reported rather than applied.
- [ ] NFR4 has no test scenario in SPEC.md's own TS-1..TS-5 list. VERIFICATION.md adds TS-6
      to close that gap; the addition is planner-made and is flagged in the completion report.
