# Verification Document: stale-test-name-refs

> **Identifier notation used in this document**
> Spelling the old identifier as one contiguous string here would add an
> occurrence and break the very criterion TS-1 checks, so it appears only in
> concatenated form.
>
> - `OLD_ID` = `test_relocate_widened_base_via_wrap_` + `no_panic_when_column_one_does_not_exist`
> - `NEW_ID` = `test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`
>
> Every search command below therefore writes the `OLD_ID` pattern with an
> **empty-quote break** (`'…wrap_''no_panic…'`). Adjacent quoted strings
> concatenate in the shell, so the pattern the tool receives is the complete
> identifier while this file never contains it contiguously. **Do not "tidy"
> the quote break away** — doing so makes this document fail its own TS-1.

## Overview

**Feature**: stale-test-name-refs
**SPEC.md**: `feature-docs/stale-test-name-refs/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/stale-test-name-refs/IMPLEMENTATION.md`

Documentation-only identifier replacement across eight records, with three
carve-out records left verbatim. No source file is modified, so the build and
test verifications below are regression guards rather than checks of new
behavior; the substantive verification is the pair of occurrence-count searches
and the diff-scope inspection.

All commands are run from the repository root.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors. Since no Rust file is in the change set,
  this is expected to be a no-op relative to the base revision; a failure here
  means a source file was touched in violation of NFR1.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0, no new failures relative to the base revision.
- Coverage target: not applicable. No source line is added, removed or
  changed, so coverage is unchanged by construction. Introducing new tests to
  raise coverage is out of scope (NFR3).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Old-name sweep: repository-wide search for `OLD_ID` over git-tracked files | Exactly 6 occurrences, distributed 2 + 2 + 2 across exactly the three carve-out files; zero occurrences in any other file | Manual-command |
| TS-2 | New-name presence: per-file count of `NEW_ID` across the eight edited files | Counts 1, 1, 1, 1, 2, 2, 2, 2 (twelve total), and zero `OLD_ID` in those files | Manual-command |
| TS-3 | Identifier-filtered run of the `crates/term_core` library suite using `NEW_ID` as the filter | At least 1 test executed — not `0 passed; 0 failed; N filtered out` | Unit (filtered) |
| TS-4 | Full `crates/term_core` library suite | Passes with no new failures | Unit |
| TS-5 | Diff scope: file list and hunk content against the base revision, excluding the workflow-generated path families | Exactly the 8 expected paths; every hunk changes only an identifier segment | Manual-command |

#### TS-1 — Old-name sweep (verifies AC-2 → FR3, FR5, NFR2)

```
rg -c 'test_relocate_widened_base_via_wrap_''no_panic_when_column_one_does_not_exist' .
```

Search scope is git-tracked files: default ripgrep behavior, `.gitignore`
honored, `.git/` excluded. This is the scope in which the 18-occurrence /
11-file ground truth was established (assumption A-2), and pinning it is what
makes the expected count reproducible.

Expected output — exactly three lines, and no others:

| Path | Count |
|------|-------|
| `feature-docs/relocate-wrap-cursor-clamp/SPEC.md` | 2 |
| `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md` | 2 |
| `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml` | 2 |

A fourth line — from any path whatsoever, including this feature's own
documents — is a failure. Check the *distribution*, not only the total of 6: a
bulk replacement that ran too wide can produce a total of 6 while having
rewritten a carve-out file.

#### TS-2 — New-name presence (verifies AC-1, AC-2 → FR1, FR2, FR3, FR5)

```
rg -c 'test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist' \
  test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml \
  test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml \
  test-docs/relocate-wrap-ec1-scroll-test/task0002.tests.yaml \
  feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md \
  feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md \
  feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md \
  feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0001.md \
  feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0002.md
```

Expected: all eight paths present, with counts 1, 1, 1, 1, 2, 2, 2, 2 in the
order listed above (twelve occurrences total). A missing path means that file
kept the stale identifier.

The count is asserted per file rather than repository-wide on purpose: `NEW_ID`
legitimately also occurs in the `crates/term_core` test source, in the
relocate-wrap-cursor-clamp records, and in this feature's own documents, so a
repository-wide total is not a stable expectation.

#### TS-3 — Filtered run of the new identifier (verifies AC-3 → FR6)

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib \
  test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist
```

Expected: the summary line reports at least one test run (for example
`1 passed; 0 failed; N filtered out`). The failure mode this scenario exists to
catch is `0 passed; 0 failed; N filtered out` with exit code 0 — the silent
green that a stale identifier produces.

#### TS-4 — Full term_core suite (verifies AC-4 → FR7)

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib
```

Expected: exit code 0, no new failures relative to the base revision.

#### TS-5 — Diff scope (verifies AC-5 → FR4, NFR1)

```
git diff --stat <base-revision> -- . \
  ':(exclude)feature-docs/stale-test-name-refs' \
  ':(exclude)test-docs/stale-test-name-refs'
```

Expected: exactly the eight paths of the edit map, and no others. The two
excluded families are this feature's own workflow-generated artifacts
(IMPLEMENTATION.md, VERIFICATION.md, task plans, phase state, review records,
per-task test record); they are declared members of the change set in SPEC.md
and are excluded here so that a correct implementation is not scored as a
scope violation. See IMPLEMENTATION.md D3.

Then inspect hunk content with the same pathspec and no `--stat`. Expected:
every hunk changes only an identifier segment. Specifically confirm that
surrounding YAML structure and indentation, Markdown prose, table alignment,
line ordering, whitespace and any `print_handler::tests::` module qualifier are
unchanged, and that no other identifier — in particular a different identifier
sharing the `test_relocate_widened_base_via_wrap_` prefix on the same line —
was rewritten.

Confirm as part of this scenario that no path under `crates/`,
`src-tauri/src/`, `src-tauri/tests/` or any build configuration appears
(NFR1), and that the three carve-out files do not appear at all (FR5, NFR2).

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
  — expected exit code 0. No Rust file is in the change set, so this is a
  guard against a stray source edit rather than a formatting check of new code.
- Static analysis: none configured for this change set. The edited files are
  Markdown records and machine-readable YAML test records; mechanizing schema
  validation of `test-docs/*/taskNNNN.tests.yaml` is explicitly out of scope
  (NFR3).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The four originally reported occurrences carry `NEW_ID` | TS-2 — rows 1–4 of the expected count table (the three `test-docs` records and `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md`) |
| AC-2 | Repository-wide search for `OLD_ID` returns exactly 6 matches, all inside the three carve-out files | TS-1 (distribution, not just total); TS-2 confirms the replaced side |
| AC-3 | The filtered cargo run of `NEW_ID` reports at least 1 test run | TS-3 |
| AC-4 | The full `crates/term_core --lib` suite passes with no new failures | TS-4 |
| AC-5 | The diff lists exactly the 8 files of FR1–FR3 and contains only identifier-string changes | TS-5 (`--stat` for the file list, hunk inspection for the content) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2 (rows 1–3: the three `test-docs/*/taskNNNN.tests.yaml` records at 1 occurrence each) |
| FR2 | task0001 | TS-2 (row 4: `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md` at 1 occurrence) |
| FR3 | task0001 | TS-1, TS-2 (rows 5–8: the four remaining relocate-wrap-ec1-scroll-test records at 2 occurrences each) |
| FR4 | task0001 | TS-5 (hunk-content inspection: identifier segment only) |
| FR5 | task0001 | TS-1 (exactly 6, exactly the 3 carve-out files), TS-2 (all other occurrences replaced) |
| FR6 | task0001 | TS-3 (at least 1 test executed, never a 0-match filter exiting 0) |
| FR7 | task0001 | TS-4 (full library suite green) |
| NFR1 | task0001 | TS-5 (no path under `crates/`, `src-tauri/src/`, `src-tauri/tests/` or build config in the diff); build and format guards |
| NFR2 | task0001 | TS-1 (the three carve-out files still hold their 6 occurrences); TS-5 (they do not appear in the diff) |
| NFR3 | task0001 | TS-5 (no schema-validation tooling and no test-logic change appears in the diff — the change set is 8 documentation records) |

## E2E Testing

Not applicable. The project has no E2E framework configured for this component
(`e2e_test_command` is empty for `term_core`), and this change set contains no
runtime behavior to exercise end to end.

## Manual Testing (E2E Not Possible)

TS-1, TS-2 and TS-5 are command-driven checks whose *judgment* is manual: the
commands print counts and a diff, and a human or agent compares them against the
expectations above. There is no automated assertion harness for them, because
building one would mean mechanizing schema/reference validation, which NFR3
places out of scope.

- [ ] TS-1: run the old-name sweep and confirm the output is exactly three
      lines with count 2 each, matching the three carve-out paths.
- [ ] TS-2: run the new-name count and confirm all eight paths appear with
      counts 1, 1, 1, 1, 2, 2, 2, 2.
- [ ] TS-5: run the diff-scope commands and confirm the eight-path file list
      and identifier-only hunks, including that the module qualifier and any
      second identifier sharing a line are untouched.
- [ ] Read one edited `test-docs/*/taskNNNN.tests.yaml` record end to end and
      confirm the `acceptance_tests[].tests` list is still valid YAML with
      unchanged structure and indentation.

No mockup visual comparison applies: the design step is `skipped` (there is no
user-facing surface in this feature), so no DESIGN.md or mockup exists to
compare against.

## Performance / Security Verification

Not applicable. SPEC.md declares no performance requirement, and the change set
has no runtime, authentication, input-handling or data-storage surface.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios (TS-1..TS-5) | 5 | 2 (TS-3, TS-4) | 0 | 3 (TS-1, TS-2, TS-5) |
| Code quality | 1 (format) | 1 | 0 | 0 |
| Success criteria (AC-1..AC-5) | 5 | 2 (AC-3, AC-4) | 0 | 3 (AC-1, AC-2, AC-5) |
| Requirements (FR1..FR7, NFR1..NFR3) | 10 | — | 0 | — |
