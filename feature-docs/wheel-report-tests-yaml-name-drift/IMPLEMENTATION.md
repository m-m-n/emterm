# Implementation Plan: wheel-report-tests-yaml-name-drift

## Overview

Repair four enumerated broken references inside a single YAML evidence record
(`test-docs/wheel-report-fraction-accum/task0001.tests.yaml`) so that every test
name it cites resolves to a real definition and every identifier in its AC-6 /
AC-7 `red_reason` bodies names a symbol that exists at HEAD — without
re-judging any recorded red verdict.

## Technology Stack

- **Language / Framework**: none. No production source file is touched; the
  only edited artifact is a YAML evidence record under `test-docs/`.
- **Key libraries**: none added. This feature introduces **no new dependency**,
  so the license check (`references/license-compat.md`) has nothing to evaluate
  against `project.license: MIT`; no dependency license needs recording here.
- **Tooling used for evidence only** (already present in the project): the
  Rust test runner invoked through the `main` component's `test_command`, plain
  text search, and `git diff`. None of them is introduced by this feature.

## Layer Structure

Not applicable in the usual sense: no production layer, module boundary or
dependency direction is involved. For completeness, the artifact layers this
feature relates to are:

| Layer | Role in this feature | May this feature write it? |
|---|---|---|
| `test-docs/wheel-report-fraction-accum/` | Holds the evidence record under repair | Yes — exactly one file |
| `test-docs/wheel-report-tests-yaml-name-drift/` | Holds this feature's own task evidence record | Yes — created by the implement phase |
| `src-tauri/src/`, `crates/` | Hold the test definitions the record must resolve against | **No** — read for evidence only (NFR1) |

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| — | This feature decomposes into a single task (see D1), so no component contract crosses a task boundary. | — | — |

No placeholder-and-later-wiring situation exists, so no integration-wiring
owner needs naming.

## Conventions

These are the editing rules every change in this feature obeys. They are
recorded here rather than inside the task plan because they also bind any
rework task appended to this feature later.

### C1 — Line-local edit, no re-flow

Each repair is confined to the line range the requirement enumerates. Removing
text shortens a line; the shortened line is left short rather than re-wrapped
against its neighbours. Pulling words up from the following line, or pushing
words down, would alter bytes outside the enumerated locations and break AC-4.

### C2 — Indentation is load-bearing

The repaired bodies are YAML folded block scalars. Folding joins lines with a
single space, so a shorter line is harmless — but a line indented more deeply
than its siblings stops folding and is emitted literally. Every edited line
keeps exactly the indentation it already has.

### C3 — Quoting structure stays balanced

Inside the quoted compiler-error text the record uses a backslash-escaped
backtick convention nested inside an outer code span. When a fragment is
removed from such a quotation, the surrounding escaped-backtick and code-span
delimiters are re-checked so that every opener still has its closer. An
unbalanced remnant is a defect even though YAML still parses.

### C4 — Historical voice preserved

A `red_reason` narrates a failure observed **before** the implementation
existed. Repairs only delete identifiers that name no symbol at HEAD and
minimally re-join the surrounding words. A body is never rewritten to describe
today's passing code, never re-dated, and no `red_confirmed` flag is
re-evaluated.

### C5 — Two different files are both named `task0001.tests.yaml`

`test-docs/wheel-report-fraction-accum/task0001.tests.yaml` is the record under
repair. `test-docs/wheel-report-tests-yaml-name-drift/task0001.tests.yaml` is
this feature's own task evidence record. They are different files with the same
basename; every reference states the directory.

### C6 — Error-handling policy

There is no runtime error handling to define. The only failure mode is
evidence-level: if a cited name still fails to resolve after the repair, the
manual evidence for it cannot be produced and the task is not complete. A
missing or zero-count run is reported as a failure, never worked around by
weakening the criterion.

## Cross-task Design Decisions

### D1 — Single task

**Decision**: the feature is implemented as one task (`task0001`).

**Rationale**: all four repairs land in the same file and the same two
acceptance entries, so splitting them would guarantee an overlapping edit
region with no independence gained. The evidence requirement (FR6) can only be
produced against the already-repaired file, and tasks run fully in parallel
with no ordering mechanism — so it must live in the same task as the repair.

**Affected tasks**: task0001.

### D2 — Repair, not re-judgement

**Decision**: the scope of every body edit is the removal of identifiers that
resolve to no symbol at HEAD, plus the minimum re-joining needed for the
sentence to read. Still-accurate identifiers (`report_accum`,
`RecordUpdates.tracking_active`, `accumulate_wheel_report_lines`,
`MAX_WHEEL_REPORT_NOTCHES`, the `wheel_report_notches_*` family), the quoted
occurrence counts, and every `red_confirmed` value are untouched.

**Rationale**: SPEC assumption A1 — a `red_reason` is a historical record, so
rewriting it to describe current code would change what the evidence means.
The sibling feature that established this principle in this repository is cited
in SPEC.md References.

**Affected tasks**: task0001.

### D3 — Evidence is manual, and no tooling is added

**Decision**: name resolution is proven by recorded evidence — the definition's
file and line for each cited name, plus a name-filtered test run reporting a
non-zero count for the re-pointed name. No automated name-drift check, linter
or script is introduced.

**Rationale**: SPEC assumption A3 and the `requirement.ac1-verification-method`
answer (`manual_evidence`); adding tests or tooling is out of scope.

**Affected tasks**: task0001.

### D4 — Scope containment is an acceptance criterion, not a side effect

**Decision**: "no file outside the single repaired record is modified" is
verified explicitly against the base revision rather than assumed from the
absence of intent.

**Rationale**: NFR1 states the suite's pass/fail counts are unchanged by this
work; an accidental touch of `src-tauri/src` or of the sibling record
`task0002.tests.yaml` would invalidate that claim silently.

**Affected tasks**: task0001.

### D5 — The AC-9 `wheel_report_notches_*` reference is a pattern, not a citation

**Decision**: the `window_host::tests::wheel_report_notches_*` mention inside
the AC-9 `red_reason` is prose describing a family of pre-existing tests. It is
not one of the cited test names, contributes nothing to the name-resolution
evidence, and is not edited.

**Rationale**: AC-9's `tests` list is empty by design, and the
`requirement.stale-statement-scope` answer (`enumerated_only`) keeps the whole
AC-9 body out of scope.

**Affected tasks**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Removing a fragment from a quoted error leaves an unbalanced escaped backtick or code span | medium | medium | C3 — re-check delimiter balance after each removal; AC-5's YAML parse plus a read-through of both bodies |
| Re-wrapping a shortened line silently changes bytes outside the enumerated locations | medium | high | C1 — no re-flow; verified by a diff review confined to the enumerated line ranges (TS-4) |
| An editing pass touches the sibling record `task0002.tests.yaml`, which already carries the current name and needs no repair | low | medium | D4 — the change set is verified against the base revision by path |
| The successor name does not in fact cover the scenario AC-7 was written for | low | high | SPEC assumption A4 — the sibling record's AC-4 names it as re-pointed from the task0001 reactivation test, and its definition exists in the test module |
| `MouseReportRecords.last_tracking_active` is assumed absent from production without the production struct file having been scanned | low | medium | SPEC assumption A5 — the repair only removes the identifier from prose, so a mistaken absence claim would not corrupt any code; the evidence run would surface a contradiction |
| The implementer adds an automated drift check because the task has no new test | medium | low | D3 and the task's Out of Scope section state the prohibition explicitly |

## Open Questions

- [ ] NFR3 (work starts only after the upstream PR is merged) has no verifying
      test ID of its own. It is treated as a precondition of the base revision
      and is observed indirectly: if the upstream merge were absent, the cited
      definitions would not exist and TS-1 would fail.
- [ ] NFR2's "valid YAML, unchanged shape" half is verified through success
      criterion AC-5 rather than through a TS-numbered scenario, because SPEC.md
      defines no TS ID for it and this plan does not invent test IDs that SPEC.md
      does not carry.
