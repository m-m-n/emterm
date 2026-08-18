# Implementation Plan: stale-test-name-refs

> **Identifier notation used in this document (and in every document this
> feature writes)**
> Writing the old identifier as one contiguous string anywhere inside this
> feature's own documents would add occurrences and break FR5 / AC-2, so the
> old identifier is only ever shown in concatenated form.
>
> - `OLD_ID` = `test_relocate_widened_base_via_wrap_` + `no_panic_when_column_one_does_not_exist`
> - `NEW_ID` = `test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`
>
> `NEW_ID` may be written in full. `OLD_ID` may not.

## Overview

Replace the stale test identifier `OLD_ID` with `NEW_ID` in eight
documentation records, while leaving the three relocate-wrap-cursor-clamp
records that document the rename itself byte-identical. Documentation-only,
string-only: no test body, no implementation source, and no schema is touched.

## Technology Stack

- **Language / Framework**: none exercised. The change set contains only
  Markdown records and machine-readable YAML test records.
- **Key libraries**: none. **No new dependency is introduced**, so the
  `project.license: MIT` compatibility check has no candidate to evaluate and
  no license line to record.
- **Toolchain touched at verification time only**: the project's existing
  `crates/term_core` cargo build/test/format commands (see VERIFICATION.md)
  and any repository-wide text search tool.

## Layer Structure

No runtime layer participates. The only layer involved is the
**documentation-record layer**, which has two distinct populations that this
feature treats differently:

| Population | Members | Treatment |
|---|---|---|
| Historical invariant references | the 8 files of FR1–FR3 (`test-docs/relocate-wrap-*/…tests.yaml`, `feature-docs/relocate-wrap-ec1-scroll-test/…`) | point at the test that currently guards the invariant → replace with `NEW_ID` |
| Rename audit trail | the 3 carve-out files under `feature-docs/relocate-wrap-cursor-clamp/` | record the old → new rename itself → keep `OLD_ID` verbatim |

Dependency direction: the record layer references the `crates/term_core` test
source one-way. The test source is read-only for this feature (assumption A-3:
`NEW_ID` already exists there at the base revision).

## Shared Components

None. This feature builds no component, and its single task shares no
interface with any other task. The table below is intentionally empty; the
cross-task obligations that would otherwise live here are expressed as
Conventions and Cross-task Design Decisions, because they bind not only the
implement phase but also the review, verify and retrospect phases, which write
further records under `feature-docs/stale-test-name-refs/`.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

## Conventions

These bind every phase of this feature, not just the implementer.

1. **Split-notation discipline (feature-wide).** Every document this feature
   writes — task plans, the per-task test record under
   `test-docs/stale-test-name-refs/`, review records, retrospect records — refers
   to the old identifier only in concatenated form, never as one contiguous
   string. A single careless full spelling turns the AC-2 count from 6 into 7
   and fails verification. `NEW_ID` may always be written in full.
2. **Exact-full-identifier matching.** Every replacement matches the complete
   `OLD_ID` string and nothing shorter. The shared prefix
   `test_relocate_widened_base_via_wrap_` is common to several unrelated test
   identifiers in this repository (it occurs far more often than `OLD_ID`
   does, including inside `crates/term_core` source), so a prefix-scoped or
   fuzzy replacement would corrupt unrelated identifiers and violate FR4 and
   NFR1.
3. **Preserve surrounding qualification.** Several occurrences are written as
   a module-qualified path (`print_handler::tests::` immediately preceding the
   identifier). The qualifier is not part of `OLD_ID` and stays untouched;
   only the trailing identifier segment changes.
4. **One-line-multiple-identifier safety.** At least one occurrence shares its
   line with a different, unrelated test identifier that must survive
   unchanged. Replacement is per-occurrence of the full `OLD_ID`, never
   per-line.
5. **String-only edit, byte-identical remainder.** No reflow, no re-wrapping,
   no line reordering, no whitespace or trailing-newline change, no
   table-column realignment, no other identifier touched. A record whose only
   diff hunk is the identifier itself is the expected shape of every one of
   the twelve edits.
6. **Error-handling / logging policy**: not applicable — no runtime code path
   exists in the change set.

## Cross-task Design Decisions

### D1 — Single-task decomposition

**Decision**: the feature is delivered as one task (task0001) owning all eight
files.

**Rationale**: two of the five acceptance criteria (AC-2 repository-wide sweep,
AC-5 diff scope) are *global* assertions over the whole change set. A task
owning a subset of the eight files could not verify either one from inside its
own worktree, so any split would produce subtasks whose Acceptance Criteria are
unverifiable in isolation — the opposite of the worktree-independence rule.
The work itself is twelve single-identifier substitutions, well inside one
implementer session, and the eight files sit in two closely-coupled record
trees, so parallel worktrees would only add merge surface without adding
parallelism. **Affected tasks**: task0001.

### D2 — Carve-out is enforced by file allow-list, not by search-and-replace scope

**Decision**: the implementer edits exactly the eight files named in the task
plan's Scope section. No repository-wide bulk replacement is performed, even
one that excludes the carve-out directory.

**Rationale**: the carve-out (FR5, NFR2) exists because
`feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml` is the review
finding that *instructs* this very replacement; rewriting its "before" side
makes the record self-contradictory. An allow-list of eight concrete paths
fails safe — an accidental extra match cannot occur — whereas an exclusion
pattern fails open the moment the pattern is mistyped. The identified risk
"a bulk replacement rewrites the three carve-out files" is closed by
construction rather than by after-the-fact detection. **Affected tasks**:
task0001.

### D3 — Diff-scope semantics: feature-specific paths versus workflow-generated paths

**Decision**: AC-5 / TS-5 ("`git diff --stat` lists exactly the 8 files") is
evaluated over the *feature-specific* change set — that is, after excluding the
two workflow-generated path families that every feature declares by default,
`feature-docs/stale-test-name-refs/**` and `test-docs/stale-test-name-refs/**`.

**Rationale**: the implement phase necessarily writes this feature's own
per-task test record, and the plan/review/verify phases write under this
feature's own `feature-docs/` directory. Reading AC-5 as a literal whole-repo
file count would mark a correct implementation as failed. SPEC.md's Declared
Change Set already lists both families as default members and states that the
declaration is a superset assertion, so this decision records the reading
rather than widening the scope. Outside those two families, the count is
exactly eight paths and zero others. **Affected tasks**: task0001; also binds
the verify phase.

### D4 — Acceptance is command-verified, not test-code-verified

**Decision**: the acceptance criteria are discharged by (a) two
occurrence-count searches, (b) a diff-scope inspection, and (c) the project's
existing `crates/term_core` cargo runs — including one identifier-filtered run
whose purpose is to prove the filter is not a zero-match. No new automated test
is authored, and no schema validator for `test-docs/*/taskNNNN.tests.yaml` is
built.

**Rationale**: NFR3 places both schema-validation mechanization and test-logic
changes explicitly out of scope, and NFR1 forbids touching anything under
`crates/`, so there is no location in which a new automated test could legally
live. The red→green discipline is preserved by running the checks *before*
editing (they fail: the old-identifier count is 18 across 11 files, and each of
the eight target files carries `NEW_ID` zero times) and again after (they pass:
6 across 3 files; per-file `NEW_ID` counts 1,1,1,1,2,2,2,2). **Affected tasks**:
task0001.

### D5 — Search scope is git-tracked files

**Decision**: the repository-wide sweep of AC-2 / TS-1 is evaluated over
git-tracked files from the repository root, honoring `.gitignore` and excluding
`.git/` — the same scope in which the orchestrator established the 18/11 ground
truth (assumption A-2).

**Rationale**: without a pinned scope the expected count is not reproducible;
build artifacts, ignored scratch directories and packed git objects would each
shift the number and make AC-2 non-deterministic. **Affected tasks**: task0001;
also binds the verify phase.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A bulk / regex replacement rewrites the three carve-out files, destroying the rename audit trail | Medium | High | D2 file allow-list; TS-1 asserts exactly 6 occurrences across exactly the 3 carve-out paths |
| A prefix-scoped replacement corrupts unrelated identifiers sharing `test_relocate_widened_base_via_wrap_` | Medium | High | Convention 2 (exact-full-identifier match); TS-5 asserts the diff contains identifier-string changes only |
| The occurrence sharing a line with an unrelated identifier is replaced line-wise | Low | Medium | Convention 4; TS-2 per-file count check plus TS-5 diff inspection |
| This feature's own documents spell the old identifier in full, inflating the AC-2 count above 6 | Medium | High | Convention 1 applies to every phase's records, including the implement-phase test record and review records |
| AC-5 read literally as a whole-repository file count marks a correct change as failed | Medium | Medium | D3 pins the evaluation scope; VERIFICATION.md TS-5 states the exclusion inline |
| Incidental reformatting (whitespace, wrapping, table alignment) rides along with the edit | Low | Medium | Convention 5; TS-5 inspects diff content, not just the file list |

## Open Questions

- [ ] None. All ten requirements are `ok` or `assumed`; no requirement is
      `tbd`, no new dependency raises a license question, and no prior
      IMPLEMENTATION.md or task plan exists for this feature.
