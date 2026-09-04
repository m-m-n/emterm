# Implementation Plan: ac7-red-confirmed-unobserved

## Overview

One YAML documentation record — `test-docs/stale-test-name-refs/task0001.tests.yaml`
— is corrected so that its AC-7 entry stops claiming a red state that was never
observed, and so that the record's trailing `notes` block says so. The feature
adds no code, changes no runtime behaviour, and touches no other file.

## Technology Stack

- **Language / Framework**: none introduced. The single production-side
  artifact is a YAML documentation record; the project's Rust and TypeScript
  components are untouched (NFR2).
- **Key libraries**: no new dependency is introduced, so there is no new
  dependency license to record. `project.license` is MIT and stays MIT; no
  license-compatibility question arises for this feature.
- **Verification tooling**: a YAML parser already present in the developer
  environment (the workflow's own validation tooling requires one), `git`, and
  a fixed-string search. All three are pre-existing environment tools, not
  project dependencies.

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

**C1 — Split-identifier discipline.** The old identifier that the edited record
forbids is never written as a contiguous string anywhere this feature produces
text: not in the record, not in this feature's commit messages, not in its own
test record. It is referred to descriptively ("the old identifier"), or spelled
in the `prefix + suffix` split form the record's header comment declares. The
identifier is deliberately not reproduced in this document or in the task plan;
whoever needs it assembles it from the record's own header comment at the moment
of checking, and never writes the assembled form back into a file.

**C2 — Truthful red records.** A criterion that cannot fail before the change —
because it guards a writing discipline whose subject does not yet exist, or
because it already holds in the base revision — is recorded with
`red_confirmed: false` and a reason that says why no red was observable. This
convention is the substance of the feature, and it binds this feature's OWN
per-task test record (`test-docs/ac7-red-confirmed-unobserved/…`) exactly as it
binds the record being corrected. Shipping this feature with an unobserved red
claimed as `true` in its own record would reproduce the defect being fixed.

**C3 — Format fidelity.** Edits happen inside the record's existing structures:
the existing key order is kept, and the folded block scalars stay folded block
scalars with their existing indentation. No key is added, removed, or reordered,
and no scalar style is converted.

**C4 — Diff containment.** The change set for this feature contains exactly one
file outside `feature-docs/` and `test-docs/{feature}/`: the record itself.
Every hunk in that file lies inside the AC-7 entry or the trailing `notes`
block.

**C5 — Naming collision awareness.** Three different things in this feature are
called "AC-n", and two different files are called `task0001.tests.yaml`. Any
document, commit message, or report produced here disambiguates explicitly —
"the record's AC-7 entry" for the file's own criteria, "SPEC AC-n" for the
feature's acceptance criteria, "task AC-n" for the task plan's — and always
names the record by its full path rather than by its file name.

## Cross-task Design Decisions

### D1: One task, not several

The feature is a bounded edit to three regions of one file (the AC-7 entry's
boolean, that entry's reason scalar, and the trailing `notes` scalar), and the
regions are mutually dependent: the reason text and the `notes` line must agree
with the boolean, and all three are checked by the same containment and
identifier-count constraints. Splitting them across parallel tasks would create
three tasks writing one file with no way to keep the wording consistent.
Affected tasks: task0001.

### D2: The feature's own test record is held to the standard it is fixing

Per convention C2, the implementer's own `taskNNNN.tests.yaml` record must mark
every criterion whose pre-state cannot fail as `red_confirmed: false`. The task
plan enumerates which of its acceptance criteria are genuinely red-observable
and which are invariant guards, so this classification is decided at planning
time rather than improvised while writing the record. Affected tasks: task0001.

### D3: Verification is inspection-based, not build-based

Because the change produces no behavioural surface, the feature's evidence comes
from four inspections — content, parse, identifier count, diff scope — rather
than from the project's build and test suites. The suites remain available and
are expected to be unaffected; running them is regression confirmation, not the
primary evidence. Affected tasks: task0001, and the verify phase.

### D4: Locate the edit by content, not by line number

The line numbers cited in REQUIREMENTS.md and SPEC.md describe the base
revision. The edit is located by reading the record's `acceptance_tests`
mapping and matching the AC-7 key, and by matching the trailing `notes` key —
never by seeking a fixed line. This keeps the task correct if the record shifts
by a line between planning and implementation. Affected tasks: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The rewritten reason text reintroduces the old identifier as one contiguous string — the text being rewritten is precisely the text that documents that prohibition | Medium | High (falsifies the record's own repository-wide count claim) | Convention C1; the zero-count check is a task acceptance criterion and a verification scenario |
| An added line breaks the folded block scalar's indentation, silently running sentences together or changing the parsed shape | Medium | Medium | Convention C3; the parse check confirms the key set and the folded style after the edit |
| The rewrite drops the explanation of why the record's own AC-2 repository-wide count stays at 6 rather than 7 | Medium | Medium | The linkage is an explicit content requirement of the task's reason-text criterion |
| An edit escapes the AC-7 entry or the `notes` block and silently alters a neighbouring entry | Low | Medium | Convention C4; the diff-scope check compares the untouched regions against the base revision |
| The feature's own test record repeats the defect by claiming an unobserved red | Medium | High (ships the exact class of defect being fixed) | Conventions C2 and decision D2; the task plan pre-classifies each criterion |
| The two same-named `task0001.tests.yaml` files are confused and the wrong one is edited | Low | High | Convention C5; the task's file list names the record by full path only |

## Open Questions

- [ ] NFR3 (commit-message discipline) has no numbered test scenario, because
      SPEC.md's TS-1..TS-5 do not cover it. It is verified as a manual item in
      VERIFICATION.md instead, and its requirement mapping carries an empty
      `tests` list. If a numbered scenario is wanted, SPEC.md has to grow one
      first.
