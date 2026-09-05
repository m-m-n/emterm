# Implementation Plan: ac2-red-reason-accuracy

## Overview

One folded block scalar — the `red_reason` of the AC-2 entry in
`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` — is rewritten so
that it describes only states the base revision actually supports: three of the
four required elements were missing before the edit, and the fourth was already
there. The red verdict, every other key of that file, and every other file in
the repository stay as they are.

## Technology Stack

- **Language / Framework**: none introduced. The only production-side artifact
  is a YAML documentation record; the project's Rust and TypeScript components
  are untouched (NFR5).
- **Key libraries**: no new dependency is introduced, so there is no new
  dependency license to record. `project.license` is MIT and stays MIT; no
  license-compatibility question arises for this feature.
- **Verification tooling**: a YAML parser already present in the developer
  environment (PyYAML, MIT — the same parser the record being corrected was
  already verified with) and `git`. Both are pre-existing environment tools,
  not project dependencies, and neither enters any manifest.

## Layer Structure

Not applicable. No module, layer, or component of the application is involved.
The edited artifact is a record read by humans and by later tasks — never
compiled, imported, or executed — so there is no dependency direction to
constrain.

## Shared Components

None. The feature decomposes into a single task (decision D1 below), so no
component contract is shared between parallel tasks.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

## Conventions

**C1 — Naming disambiguation, before anything else.** Four different things in
this feature are numbered "AC-n", and three different files are named
`task0001.tests.yaml`. Every document, commit message, and report produced here
uses these terms and never the bare file name:

| Term | Meaning |
|---|---|
| **target record** | `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` — the file this feature edits |
| **described record** | `test-docs/stale-test-name-refs/task0001.tests.yaml` — the file the target record's text talks about; never modified here |
| **base blob** | the described record as of revision `9eee6161` |
| **own record** | `test-docs/ac2-red-reason-accuracy/task0001.tests.yaml` — this feature's own per-task test record, produced by the implement phase |
| **target-record AC-2** | the target record's AC-2 entry, whose `red_reason` scalar this feature rewrites |
| **described-record AC-2** | the described record's AC-2 entry, whose repository-wide occurrence count is the subject of the "fourth element" |
| **described-record AC-7** | the described record's AC-7 entry, whose pre-edit and post-edit `red_reason` text the rewritten scalar describes |
| **SPEC AC-n** | a success criterion in SPEC.md |
| **task AC-n** | a criterion in a task plan's own Acceptance Criteria section |

The single most likely way to get this feature wrong is to conflate
target-record AC-2 (the thing being rewritten) with described-record AC-2 (the
count explanation the rewritten text must say was already present).

**C2 — Truthful red records.** A criterion that cannot fail before the change —
because it already holds in the base revision, or because its subject does not
yet exist — is recorded with `red_confirmed: false` and a reason that says why
no red was observable. This convention is the substance of the feature, and it
binds this feature's own record exactly as it binds the record being corrected.
Shipping this feature with an unobserved red claimed as `true` in its own
record would reproduce, in a third file, the exact defect being fixed.

**C3 — Format fidelity.** The edit happens inside the target record's existing
structure: the `>-` folded block-scalar indicator, the scalar's indentation and
the file's line-wrap style are kept, and no key is added, removed, reordered, or
converted to another scalar style. Only the one scalar's content changes.

**C4 — Never write the old identifier contiguously.** The described-record AC-2
entry claims a repository-wide occurrence count for an identifier that its own
header comment deliberately spells only as `prefix + suffix`. Introducing one
contiguous occurrence anywhere in this feature's artifacts — the rewritten
scalar, this feature's own record, its plans, or its commit messages — would
raise that count and falsify the described record's AC-2 claim. This feature
needs that identifier nowhere: the rewritten text names the four elements
descriptively, so the correct handling is simply never to write it.

**C5 — Diff containment.** The change set for this feature contains exactly one
file outside `feature-docs/{feature}/` and `test-docs/{feature}/`: the target
record. Every hunk in that file lies inside the target-record AC-2 entry's
`red_reason` scalar.

**C6 — Language.** The target record is English throughout and stays English
(NFR3). Japanese appears only in this feature's user-facing reports, never in
the record.

## Cross-task Design Decisions

### D1: One task, not several

The feature is a single content rewrite of one scalar whose parts are mutually
dependent — the count ("three of four"), the enumeration of the three absent
elements, and the statement about the fourth must agree with each other and with
one shared piece of evidence. Splitting them across parallel tasks would create
several tasks writing one scalar with no way to keep the wording consistent.
Affected tasks: task0001.

### D2: Evidence first, then rewrite

The claim the rewritten text rests on — that the base blob's described-record
AC-7 `red_reason` already carried the linkage to described-record AC-2's
repository-wide count — is SPEC.md assumption A-1 and was NOT independently
verified during requirements analysis. It is read out of git history and
confirmed BEFORE any text is written (SPEC.md TS-4). NFR4 forbids the corrected
text from asserting an observation that was not made, so a rewrite performed
ahead of that read would risk replacing one unobserved claim with another. If
the base blob does not support the linkage, the task stops and reports instead
of writing text the evidence contradicts — the premise of the whole feature has
failed and that is a spec-layer question, not something to paper over.
Affected tasks: task0001, and the verify phase.

### D3: Red observability is classified at planning time

Per convention C2, this feature's own record must mark every criterion whose
pre-state cannot fail as `red_confirmed: false`. Which of the task's acceptance
criteria are genuinely red-observable and which are invariant guards is decided
in the task plan's Test Notes rather than improvised while writing the record.
Affected tasks: task0001.

### D4: Verification is inspection-based, not build-based

The change produces no behavioural surface, so the evidence is four inspections
— parsed content, parse shape, raw formatting, diff scope — rather than the
project's build and test suites. Those suites are expected to be unaffected;
the change-set containment check is what establishes that they exercise nothing
this feature touched. Affected tasks: task0001, and the verify phase.

### D5: Locate the edit by content, not by line number

The line numbers cited in REQUIREMENTS.md and SPEC.md describe the record as it
stood when the defect was reported. The edit is located by reading the target
record's `acceptance_tests` mapping and matching the AC-2 key — never by seeking
a fixed line — so the task stays correct if the record shifts by a line between
planning and implementation. Affected tasks: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The rewrite conflates target-record AC-2 with described-record AC-2, producing text that says the entry explains its own count | Medium | High (a new record/reality gap of the same class) | Convention C1; the task's criteria name each entry with its qualifier |
| The base blob turns out not to support the linkage claim (assumption A-1), and the rewrite asserts it anyway | Low | High (NFR4 violation — one unobserved claim replaced by another) | Decision D2; the evidence read is a task acceptance criterion and precedes the rewrite |
| Fixing the count drops the enumeration of the three genuinely absent elements, or the accurate post-edit half | Medium | Medium (FR3 / FR4 regression) | Both are stated as a single invariant task criterion, checked before and after |
| This feature's own record repeats the defect by claiming an unobserved red for the invariant-guard criteria | Medium | High (ships the exact class of defect being fixed) | Conventions C2, decision D3; the task plan pre-classifies every criterion |
| The rewrite re-wraps or restyles neighbouring scalars, or converts the folded block scalar | Medium | Medium | Convention C3; the parse check and the raw-format check run after the edit |
| A contiguous occurrence of the old identifier enters this feature's artifacts, raising the described record's repository-wide count from 6 to 7 | Low | High (falsifies described-record AC-2) | Convention C4; the identifier is needed nowhere in this feature |
| The wrong `task0001.tests.yaml` is opened — three files share that name | Low | High | Convention C1; the task's file list names the record by full path only |

## Open Questions

- [ ] NFR3 (the record stays English) has no numbered test scenario, because
      SPEC.md's TS-1..TS-5 do not cover it. It is verified as a manual item in
      VERIFICATION.md instead, and its requirement mapping carries an empty
      `tests` list. A numbered scenario would require SPEC.md to grow one first.
- [ ] Assumption A-1 is still unverified at planning time. It is discharged by
      the task's evidence gate (decision D2) before any text is written; a
      failure there is a spec-layer outcome, not an implementation defect.
