# Verification Document: spec-test-name-traceability

## Overview

**Feature**: spec-test-name-traceability
**SPEC.md**: `feature-docs/spec-test-name-traceability/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/spec-test-name-traceability/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `tasks/task0001.md`, `tasks/task0002.md` and
`tasks/task0003.md`.

The scenario IDs below (TS-1 … TS-8) are **this feature's own**. They are
distinct from the *target-document* TS IDs (TS-1 … TS-6, TS-M1) that live inside
`feature-docs/mouse-report-held-callsite-test/` and are this feature's subject
matter. Where a check is about the target documents, the text says
"target-document TS-n".

**Base revision** for every change-set check: the commit recorded as
`workflow.implement.base_commit` in `feature-docs/spec-test-name-traceability/workflow.yaml`
(`bf87e666` at planning time). Read it from `workflow.yaml` rather than
hardcoding it.

## Build Verification

The change set is Markdown; no build input is touched (NFR1). These commands are
regression gates — each must produce the same result as before the change.

| Component | Command | Expected |
|---|---|---|
| `main` | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| `cli_feature_gate` | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors |
| `web` | `bun run build:viewer && bun run build:settings` | exit code 0, bundles produced |

Run all three from the repository root.

## Test Verification

| Component | Command | Expected |
|---|---|---|
| `main` | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | exit code 0; the same set of tests passes as before the change — this feature adds, removes and renames no test |
| `web` | `bun test` | exit code 0, unchanged result |

- Coverage target: not applicable. The project has no coverage tooling
  configured and this feature adds no production line and no test line; a
  coverage number would measure nothing about the change.
- Known caveat (inherited, not caused by this feature): a red outside the
  changed area should be re-run with a single test thread before being
  attributed here — a small number of pre-existing tests in this suite are
  parallelism sensitive.

### Test Scenarios from SPEC.md

"The three documents" means
`feature-docs/mouse-report-held-callsite-test/{SPEC.md, VERIFICATION.md, REQUIREMENTS.md}`.
"The canonical table" means IMPLEMENTATION.md's "Canonical test-name assignment"
(25 identifiers with their target-document TS attribution).

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `docs_no_longer_reference_any_nonexistent_test_name` — for each identifier the base revision of a document presented as a test name, check whether a test function of that name exists in `src-tauri/src/window_host/tests.rs`; search the amended document for the ones that do not | Zero matches in all three documents — no unresolvable test name survives (the six removed names are derived from the base revision, never re-typed; IMPLEMENTATION.md decision D6) | Scripted (shell) |
| TS-2 | `every_documented_test_name_resolves_to_a_function_in_tests_rs` — extract every identifier the three documents present as a test name and search `src-tauri/src/window_host/tests.rs` for `fn <name>()` | Exactly one match per name, in every one of the three documents. Target-document TS-M1's `manual: none required` is excluded — it is not a function name | Scripted (shell) |
| TS-3 | `documented_name_set_equals_the_features_added_test_set` — two-way set diff between the names documented in each of the three documents and the feature's added test set (the test-attributed functions from the `AC-3/TS-1` section marker to end of file; IMPLEMENTATION.md decision D2), plus the per-TS counts | Both directions empty, 25 names, and per-TS counts 1 / 1 / 1 / 18 / 3 / 1 for target-document TS-1 / TS-2 / TS-3 / TS-4 / TS-5 / TS-6 — in each of the three documents | Scripted (shell) |
| TS-4 | `verification_test_count_claim_matches_the_implementation` — read the Test Verification "Expected" line of the target VERIFICATION.md and search that file for "six new" | The Expected line names 25 entries; "six new" returns zero matches | Scripted (shell) |
| TS-5 | `code_quality_naming_item_states_the_two_way_check` — read the revised Code Quality naming inspection item of the target VERIFICATION.md | The item names `src-tauri/src/window_host/tests.rs` as the side of truth, states both directions of the bijection and the naming-convention check, and "character for character" returns zero matches in the file | Manual inspection |
| TS-6 | `change_set_is_markdown_only_and_leaves_tests_rs_untouched` — `git diff --name-only <base>..HEAD` | Every listed path ends in `.md` or lies under `feature-docs/spec-test-name-traceability/` or `test-docs/spec-test-name-traceability/`; no path starts with `src-tauri/` or `crates/` | Scripted (shell) |
| TS-7 | `point_in_time_records_are_absent_from_the_change_set` — `git diff --name-only <base>..HEAD` | Nothing under `feature-docs/mouse-report-held-callsite-test/{reviews,phase-state,tasks}/` and nothing under `test-docs/mouse-report-held-callsite-test/` appears | Scripted (shell) |
| TS-8 | `benign_edit_test_under_ts5_and_scaffold_test_under_ts4` — locate the two decision-carrying names in each of the three documents | `button_path_benign_edits_to_the_real_source_are_still_accepted` appears under target-document TS-5 and under no other TS ID; `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` appears under target-document TS-4 with a note identifying it as the sanity/scaffold anchor | Scripted (shell) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` (`main`) and
  `bunx biome check .` (`web`). No Rust or TypeScript file is modified, so both
  are regression gates and must report exactly what they reported before.
- Static analysis: no separate lint command is configured; the two `cargo check`
  runs in Build Verification are the static gate.
- Inspection items:
  - Every identifier written into the three documents is a character-for-
    character copy of the source's own spelling — no paraphrase, abbreviation,
    re-casing, or line wrap inside an identifier (NFR2).
  - Each amended document keeps its own language: Japanese in SPEC.md and
    REQUIREMENTS.md, English in VERIFICATION.md (NFR3).
  - Section headings, TS IDs, requirement/AC columns and every table not named
    in FR1-FR4 are unchanged (NFR3).
  - No new file was created anywhere — in particular no checker script (FR5;
    IMPLEMENTATION.md decision D4).
  - No new dependency appears in any manifest; the feature's dependency licence
    ledger is empty and `project.license` (`MIT`) is untouched.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | None of the six nonexistent names survives in the three amended documents | TS-1 |
| AC-2 | Every documented test name resolves to exactly one function in the implementation file | TS-2 |
| AC-3 | The documented name set equals the implementation's added test set in both directions (25 names) | TS-3 |
| AC-4 | The target VERIFICATION.md no longer claims six new test entries | TS-4 |
| AC-5 | The target VERIFICATION.md's Code Quality naming item states the bijection check | TS-5 (manual read-back, M-1) |
| AC-6 | The change set is Markdown only and leaves the implementation file untouched | TS-6, plus the Build/Test regression gates above |
| AC-7 | No point-in-time record of the amended feature is in the change set | TS-7 |
| AC-8 | The benign-edit test is attributed to target-document TS-5 in all three documents and to no other TS ID | TS-8 |
| AC-9 | The scaffold test is listed under target-document TS-4 in all three documents with its role noted | TS-8 |
| AC-10 | Target-document TS-4 lists exactly 18 names and TS-5 exactly 3, in every one of the three documents | TS-3 (per-TS counts) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-8 |
| FR2 | task0002 | TS-1, TS-2, TS-3, TS-4, TS-8 |
| FR3 | task0002 | TS-5 |
| FR4 | task0003 | TS-1, TS-2, TS-3, TS-8 |
| FR5 | task0001, task0002, task0003 | TS-6, TS-7 |
| NFR1 | task0001, task0002, task0003 | TS-6, plus the Build and Test Verification regression gates |
| NFR2 | task0001, task0002, task0003 | TS-2, plus the Code Quality inspection item on verbatim transcription |
| NFR3 | task0001, task0002, task0003 | TS-5, plus the structure-preservation diff review (M-2) |
| NFR4 | task0001, task0002, task0003 | TS-7 |

## E2E Testing

No E2E framework exists in this project and none is introduced
(`e2e_test_command` is empty for every component). The feature changes no
runtime behaviour, so there is no end-to-end path to exercise.

## Manual Testing (E2E Not Possible)

- [ ] **M-1** (TS-5, AC-5): read the revised Code Quality naming inspection item
      in the target VERIFICATION.md and confirm it (a) names
      `src-tauri/src/window_host/tests.rs` as the side of truth, (b) states both
      directions of the bijection, and (c) keeps the
      `<subject>_<scenario>_<expected>` convention check. Confirm the item is a
      check a reviewer can actually perform.
- [ ] **M-2** (NFR3): review the full diff of the three amended documents and
      confirm only the name cells, the count claim, the AC-1 verification
      wording and the naming inspection item changed — headings, TS IDs,
      requirement/AC columns, other tables and each document's language are
      intact.
- [ ] **M-3** (NFR2): spot-read the longest identifiers in each rendered table
      and confirm none is broken across lines inside the identifier and that
      every table still parses with its original column set.

No mockup comparison applies: the design step is `skipped` for this feature and
no visual surface exists.

## Performance / Security Verification

Not applicable. The feature adds no runtime code path, no input handling, no
persistence and no external I/O.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build regression | 3 | 3 | 0 | 0 |
| Test regression | 2 | 2 | 0 | 0 |
| Scenarios (TS-1 … TS-8) | 8 | 7 | 0 | 1 |
| Code quality inspection | 5 | 2 | 0 | 3 |
| Manual items (M-1 … M-3) | 3 | 0 | 0 | 3 |
