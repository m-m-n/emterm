---
title: "spec-test-name-traceability"
created_date: 2026-09-21
status: draft
---

# spec-test-name-traceability - Requirements Document

## 1. Overview

### 1.1 Background

The three narrative documents of `feature-docs/mouse-report-held-callsite-test/`
— SPEC.md, VERIFICATION.md and REQUIREMENTS.md — name six test functions that do
not exist in `src-tauri/src/window_host/tests.rs`. The implementation of that
feature actually added 25 test functions (tests.rs:6006-6717). Because the
documented names resolve to nothing, the documents' traceability tables cannot be
checked mechanically, VERIFICATION.md's "six new entries" count claim is wrong,
and VERIFICATION.md's Code Quality naming inspection item — which demands
character-for-character equality against those six names — is unsatisfiable as
written.

### 1.2 Purpose

Align the three documents with the implementation file so that every test name
they list resolves to a real function, the count claim is correct, and the naming
inspection item states a check that can actually be performed.

### 1.3 Scope

**In scope:** editing exactly three Markdown files —
`feature-docs/mouse-report-held-callsite-test/{SPEC.md, VERIFICATION.md, REQUIREMENTS.md}`
— plus this feature's own workflow-generated
`feature-docs/spec-test-name-traceability/**` and
`test-docs/spec-test-name-traceability/**`.

**Out of scope:** any change to `src-tauri/src/window_host/tests.rs` (no rename,
no body edit, no comment edit), any other file under `src-tauri/` or `crates/`,
and the point-in-time records of the amended feature
(`reviews/roundN.yaml`, `phase-state/`, `tasks/`,
`test-docs/mouse-report-held-callsite-test/`).

## 2. Business Requirements

### 2.1 Business Objectives

- Make the three narrative documents of `feature-docs/mouse-report-held-callsite-test/`
  name the test functions that actually exist in `src-tauri/src/window_host/tests.rs`,
  so their traceability tables can be checked mechanically instead of by reading
  prose.
- Remove the count claim that the feature added six tests; the implementation
  added 25 test functions (tests.rs:6006-6717).
- Restore the usefulness of VERIFICATION.md's Code Quality naming inspection
  item, which today demands character-for-character equality against six names
  that do not exist in the codebase and is therefore unsatisfiable as written.

### 2.2 Target Users

| User type | Description |
|-----------|-------------|
| Developer verifying `mouse-report-held-callsite-test` | Needs the documents' test names to grep-resolve against the implementation file. |
| Reviewer running VERIFICATION.md's checklist | Needs the expected test count and the naming inspection item to describe a performable check. |
| Maintainer of the feature-docs tree | Needs the correction to leave the implementation file and the historical records untouched. |

### 2.3 Expected Effects

- The traceability tables of the three documents become mechanically checkable.
- VERIFICATION.md's test-count expectation matches the run.
- VERIFICATION.md's Code Quality naming item becomes a satisfiable two-way check
  against the implementation file.

## 3. Use Cases

### 3.1 Use Case List

| ID | Use case | Actor | Priority |
|----|----------|-------|----------|
| UC01 | Check a documented test name against the implementation file | Developer | High |
| UC02 | Run the VERIFICATION.md checklist for the amended feature | Reviewer | High |

### 3.2 Use Case Details

#### UC01: Check a documented test name against the implementation file

**Actor**: Developer verifying `mouse-report-held-callsite-test`.

**Preconditions**:
- The three amended documents are in their corrected state.

**Basic flow**:
1. Read a test name from a document's Test Scenarios / traceability table.
2. Grep for `fn <name>()` in `src-tauri/src/window_host/tests.rs`.
3. Get exactly one match.

**Alternative flow**:
- TS-M1's "manual: none required" is not a function name and is excluded from
  the check.

**Postconditions**:
- Every documented name is confirmed to exist in the implementation file.

#### UC02: Run the VERIFICATION.md checklist for the amended feature

**Actor**: Reviewer.

**Preconditions**:
- VERIFICATION.md is in its corrected state.

**Basic flow**:
1. Run the test command and compare against the stated expectation of 25 entries.
2. Confirm the named test functions appear in the run (not the TS IDs, which
   never appear in cargo test output).
3. Perform the Code Quality naming item's two-way check against
   `src-tauri/src/window_host/tests.rs`.

**Postconditions**:
- Every checklist item yields a real pass/fail.

## 4. Functional Requirements

### 4.1 Function List

| ID | Name | Description | Priority |
|----|------|-------------|----------|
| FR1 | SPEC.md Test Scenarios revised to implementation test names | Replace the six nonexistent names in SPEC.md's Unit Tests bullets with the 25 real names. | High |
| FR2 | VERIFICATION.md Test Scenarios table revised, and its count claim corrected | Replace the six names in the Test Scenarios table, correct the count to 25, and rewrite the AC-1 verification wording. | High |
| FR3 | VERIFICATION.md Code Quality naming inspection item replaced | Replace the unsatisfiable naming item with a two-way check against the implementation file. | High |
| FR4 | REQUIREMENTS.md traceability table revised to implementation test names | Replace the test-name column of section 12.2's table with the 25 real names. | High |
| FR5 | Documentation-only change set; production and record files untouched | Limit the change set to Markdown; leave tests.rs and the point-in-time records untouched. | High |

### 4.2 Function Details

#### FR1: SPEC.md Test Scenarios revised to implementation test names

**Status**: resolved

**Description**: In `feature-docs/mouse-report-held-callsite-test/SPEC.md`, the
six nonexistent test names in the "Test Scenarios > Unit Tests" bullets
(SPEC.md:234, 240, 244, 248, 254, 259) are replaced by the real test function
names in `src-tauri/src/window_host/tests.rs`. TS-1/TS-2/TS-3/TS-6 each name
their single implementation test, TS-4 enumerates all 18 of its tests, and TS-5
enumerates all 3 of its tests — 25 names in total. The scaffold test
`sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` is listed under
TS-4 with its role noted (it is the sanity anchor proving the synthetic callee
pattern matches unquoted text, so TS-4's negative cases are not vacuous). The TS
IDs, their FR/AC mappings, and TS-M1 are unchanged.

**Input**: the 25 identifiers read verbatim from `src-tauri/src/window_host/tests.rs`.

**Output**: the amended SPEC.md of the target feature.

#### FR2: VERIFICATION.md Test Scenarios table revised, and its count claim corrected

**Status**: resolved

**Description**: In `feature-docs/mouse-report-held-callsite-test/VERIFICATION.md`,
the six nonexistent names in the "Test Scenarios from SPEC.md" table
(VERIFICATION.md:39-44) are replaced by the same 25 implementation test names
under the same TS IDs and the same per-row assignment used in FR1. The Test
Verification expectation at VERIFICATION.md:24-25, which currently reads "the six
new `window_host::tests` entries below appear in the run", is corrected to the
real count of 25. The AC-1 row at VERIFICATION.md:73 ("confirm exit 0 and that
TS-1 … TS-6 appear in the run") is rewritten so the thing to confirm in the run
is the named test functions rather than the TS IDs, which never appear in cargo
test output. The TS-M1 row (VERIFICATION.md:45), the Verification Summary table,
and every other table are otherwise unchanged.

#### FR3: VERIFICATION.md Code Quality naming inspection item replaced

**Status**: resolved

**Description**: The inspection item at VERIFICATION.md:55-56, currently "New
test names follow `<subject>_<scenario>_<expected>` and match the TS names in
this document character for character (NFR3)", is replaced by an item that states
a two-way (bijection) check against the real file. The revised item checks all
three of the following, and names `src-tauri/src/window_host/tests.rs` explicitly
as the side of truth:

- (a) every test name listed in that document's Test Scenarios table exists
  verbatim as a `fn <name>()` in `src-tauri/src/window_host/tests.rs`;
- (b) every test function that feature added to that file appears in exactly one
  TS row of the table — no added test is undocumented and no documented name is
  absent from the file;
- (c) each of those names follows the repository's
  `<subject>_<scenario>_<expected>` convention (NFR3, test/README.md "Test Naming
  Conventions").

The direction of authority is stated: the file is normative and the document
follows it.

#### FR4: REQUIREMENTS.md traceability table revised to implementation test names

**Status**: resolved

**Description**: In `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`,
the test-name column of the section 12.2 traceability table
(REQUIREMENTS.md:424-429) is replaced by the same 25 implementation test names
under the same TS IDs and the same per-row assignment used in FR1 and FR2. The
requirement / AC columns, the TS-M1 row (REQUIREMENTS.md:430), and the
per-scenario supplementary notes (REQUIREMENTS.md:434-455, which reference TS IDs
only and contain no test names) are unchanged.

#### FR5: Documentation-only change set; production and record files untouched

**Status**: resolved

**Description**: The change set is Markdown only, limited to exactly three files:
`feature-docs/mouse-report-held-callsite-test/{SPEC.md, VERIFICATION.md, REQUIREMENTS.md}`
(plus this feature's own workflow-generated
`feature-docs/spec-test-name-traceability/**` and
`test-docs/spec-test-name-traceability/**`).
`src-tauri/src/window_host/tests.rs` is NOT modified at all — no rename, no body
edit, no comment edit — and no other file under `src-tauri/` or `crates/` is
touched. The point-in-time records of `feature-docs/mouse-report-held-callsite-test/`
— `reviews/roundN.yaml`, `phase-state/`, `tasks/` — and
`test-docs/mouse-report-held-callsite-test/` are left byte-identical.

### 4.3 Target Test-Name Correspondence

The exact names the three documents must carry, with their target-document TS
assignment. Line numbers are informational; the names are normative.

| Target-document TS | Count | Test functions (tests.rs line) |
|---|---|---|
| TS-1 | 1 | `run_button_decision_calls_apply_outcome_with_held_once_with_live_held_as_fourth_arg` (6007) |
| TS-2 | 1 | `handle_mouse_wheel_calls_apply_wheel_report_step_once_with_live_held_as_fifth_arg` (6031) |
| TS-3 | 1 | `neither_call_site_body_contains_the_held_unaware_apply_path` (6056) |
| TS-6 | 1 | `body_extractor_returns_exactly_the_two_named_bodies_excluding_the_motion_path` (6082) |
| TS-4 | 18 | see the list below (source section `AC-1/TS-4`, tests.rs:6118-6404) |
| TS-5 | 3 | see the list below (source section `AC-6/TS-5`, tests.rs:6406-6775) |

TS-4 (18):

1. `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` (6151) — scaffold / sanity anchor
2. `scanner_treats_a_correct_call_written_inside_a_line_comment_as_absent` (6164)
3. `scanner_treats_a_correct_call_written_inside_a_string_literal_as_absent` (6175)
4. `scanner_ignores_call_shaped_text_inside_nested_block_comments` (6187)
5. `scanner_ignores_call_shaped_text_inside_a_raw_string_with_differing_hash_counts` (6204)
6. `scanner_tokenizes_byte_and_c_string_literals_as_opaque_literals` (6222)
7. `scanner_distinguishes_a_lifetime_marker_from_an_adjacent_character_literal` (6241)
8. `scanner_does_not_terminate_a_character_literal_on_an_escaped_quote` (6268)
9. `scanner_does_not_terminate_a_string_literal_on_an_escaped_quote` (6283)
10. `scanner_never_matches_an_identifier_as_a_prefix_of_a_longer_one` (6299)
11. `argument_splitter_ignores_a_trailing_comma` (6312)
12. `judge_call_tolerates_the_argument_list_rewrapped_across_lines` (6328)
13. `judge_call_tolerates_a_comment_between_the_callee_and_its_opening_paren` (6339)
14. `judge_call_rejects_a_let_bound_shadow_of_the_receiver_before_the_call` (6350)
15. `judge_call_rejects_a_closure_parameter_shadow_of_the_receiver_before_the_call` (6361)
16. `judge_call_rejects_a_match_arm_pattern_shadow_of_the_receiver_before_the_call` (6372)
17. `judge_call_rejects_an_if_let_pattern_shadow_of_the_receiver_before_the_call` (6383)
18. `judge_call_rejects_when_the_held_unaware_path_also_appears_elsewhere_in_the_body` (6397)

TS-5 (3):

1. `button_path_real_body_mutations_are_rejected_and_the_unmutated_body_accepted` (6440)
2. `wheel_path_real_body_mutations_are_rejected_and_the_unmutated_body_accepted` (6592)
3. `button_path_benign_edits_to_the_real_source_are_still_accepted` (6717)

TS-M1 is a manual scenario ("manual: none required"), not a Rust test function;
it is excluded from every name check.

## 5. Non-Functional Requirements

### NFR1: No source, manifest, or build-input change

**Status**: resolved

The feature changes no Rust source, no TypeScript source, no package manifest,
and no build input. Every build, test, format and typecheck command in the
project's components produces the same result before and after the change.

### NFR2: Names copied verbatim

**Status**: resolved

Each of the 25 names is transcribed character-for-character from
`src-tauri/src/window_host/tests.rs`. No name is paraphrased, abbreviated,
re-cased, line-wrapped inside the identifier, or reformatted. Identifiers are
written in backticks in the Markdown, as the surrounding documents already do.

### NFR3: Existing document structure preserved

**Status**: resolved

Section headings, TS IDs (TS-1 … TS-6, TS-M1), the FR/NFR-to-AC-to-TS mappings in
SPEC.md's Traceability tables (SPEC.md:328-355), VERIFICATION.md's Functional
Requirements Coverage table (VERIFICATION.md:84-101), and the documents' existing
language (Japanese in SPEC.md/REQUIREMENTS.md, English in VERIFICATION.md) are
all preserved. Only the name cells, the count claim, the AC-1 verification
wording, and the Code Quality naming item change.

### NFR4: Point-in-time records are immutable

**Status**: resolved

`reviews/roundN.yaml`, `phase-state/`, `tasks/` and
`test-docs/mouse-report-held-callsite-test/` are historical records. They are
never edited to match a later correction, even when they repeat a name this
feature is replacing.

## 6. UI/UX Requirements

None. The feature renders nothing and adds or alters no UI surface.

## 7. Data Requirements

None. No data model, no persisted state.

## 8. External Integrations

None.

## 9. Constraints

### 9.1 Technical Constraints

- `src-tauri/src/window_host/tests.rs` is read-only for this feature and is the
  normative side of every name comparison (FR5, FR3).
- Names must be transcribed verbatim (NFR2).

### 9.2 Business Constraints

- Point-in-time records must remain byte-identical (NFR4).

### 9.3 Schedule Constraints

None.

### 9.4 Declared Change Set

Feature-specific paths are not enumerated by hand here; they are derived at
create-plan from each task's `files` in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author
explicitly removes them):
- `feature-docs/spec-test-name-traceability/**`
- `test-docs/spec-test-name-traceability/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces; ownership is stated by the phase documents
and `references/phase-state.md` (cited, not restated).

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`; ownership is
stated by `implement-phase.md` (cited, not restated).

**Semantics**: the declaration is a SUPERSET assertion — the actual change set
must be CONTAINED IN the declared set. A declared path that never materializes is
not a violation.

Feature-specific paths in addition to the two defaults:
- `feature-docs/mouse-report-held-callsite-test/SPEC.md`
- `feature-docs/mouse-report-held-callsite-test/VERIFICATION.md`
- `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`

## 10. Anticipated Issues and Risks

### 10.1 Technical Issues

| Issue | Impact | Mitigation |
|-------|--------|------------|
| A name is transcribed inexactly, so it no longer grep-resolves | High | NFR2 requires verbatim transcription; AC-2 / TS-2 check each name resolves to exactly one `fn`. |
| A test is documented twice or omitted | Medium | AC-3 / TS-3 run a two-way set diff with per-TS counts 1/1/1/18/3/1. |
| The edit leaks into the implementation file or a historical record | High | AC-6 / AC-7, verified by `git diff --name-only` in TS-6 and TS-7. |

### 10.2 Business Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| The corrected documents drift again when tests change | Medium | Medium | FR3's two-way naming inspection item makes the drift detectable at verification time. |

## 11. Success Criteria

### 11.1 Acceptance Criteria

- [ ] **AC-1**: None of the six nonexistent names survives in the three amended
      documents. Check: grep for the six names over SPEC.md, VERIFICATION.md and
      REQUIREMENTS.md of `feature-docs/mouse-report-held-callsite-test/` exits
      non-zero (zero matches).
- [ ] **AC-2**: Every test name the three amended documents list resolves to
      exactly one function in the implementation file. Check: for each backticked
      test name N, `grep -c "fn ${N}()" src-tauri/src/window_host/tests.rs` is 1.
      TS-M1's "manual: none required" is excluded; it is not a function name.
- [ ] **AC-3**: The documented name set equals the implementation's added test
      set, in both directions — 25 names, no omission and no surplus. Check: a
      two-way set diff between the names extracted from the three documents and
      the 25 `fn` names in `src-tauri/src/window_host/tests.rs` between the
      `AC-3/TS-1` section marker (tests.rs:6000) and end of file (tests.rs:6776)
      is empty.
- [ ] **AC-4**: VERIFICATION.md no longer claims six new test entries. Check:
      grep for "six new" in that file returns zero matches, and the Test
      Verification "Expected" line names 25 entries.
- [ ] **AC-5**: VERIFICATION.md's Code Quality naming inspection item states the
      bijection check of FR3 rather than the one-way "match this document" claim.
      Check: grep for "character for character" returns zero matches; the naming
      item mentions the literal string `src-tauri/src/window_host/tests.rs` and
      states both directions.
- [ ] **AC-6**: The change set is Markdown only and leaves the implementation
      file untouched. Check: `git diff --name-only <base>..HEAD` lists only the
      three amended documents and paths under
      `feature-docs/spec-test-name-traceability/` or
      `test-docs/spec-test-name-traceability/`; no path starts with `src-tauri/`
      or `crates/`.
- [ ] **AC-7**: No point-in-time record of the amended feature is in the change
      set. Check: `git diff --name-only <base>..HEAD` matches nothing under
      `feature-docs/mouse-report-held-callsite-test/{reviews,phase-state,tasks}/`
      or `test-docs/mouse-report-held-callsite-test/`.
- [ ] **AC-8**: `button_path_benign_edits_to_the_real_source_are_still_accepted`
      is attributed to TS-5 in all three documents, following the source's own
      sectioning, and appears under no other TS ID.
- [ ] **AC-9**: `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` is
      listed under TS-4 in all three documents, with a role note identifying it
      as the sanity/scaffold anchor.
- [ ] **AC-10**: TS-4 lists exactly 18 names and TS-5 exactly 3, in every one of
      the three documents.

### 11.2 KPI

Not applicable.

## 12. Test Scenarios

The IDs in this section are **this feature's own** scenario IDs. They are
distinct from the target-document TS IDs (TS-1 … TS-6, TS-M1) that appear inside
`feature-docs/mouse-report-held-callsite-test/`.

### 12.1 Test Perspectives

- [ ] Positive: every documented name resolves to exactly one function in
      `src-tauri/src/window_host/tests.rs`.
- [ ] Negative: none of the six nonexistent names survives in the three amended
      documents.
- [ ] Boundary: the documented name set equals the added test set in both
      directions (25 names; per-TS counts 1/1/1/18/3/1).
- [ ] Scope: the change set is Markdown only and excludes the point-in-time
      records.
- [ ] Manual: the revised Code Quality naming item states both directions of the
      bijection.

### 12.2 Scenario Traceability

| ID | Scenario name | Type | Covers | AC |
|----|---------------|------|--------|-----|
| TS-1 | `docs_no_longer_reference_any_nonexistent_test_name` | scripted check | FR1, FR2, FR4 | AC-1 |
| TS-2 | `every_documented_test_name_resolves_to_a_function_in_tests_rs` | scripted check | FR1, FR2, FR4, NFR2 | AC-2 |
| TS-3 | `documented_name_set_equals_the_features_added_test_set` | scripted check | FR1, FR2, FR4 | AC-3, AC-10 |
| TS-4 | `verification_test_count_claim_matches_the_implementation` | scripted check | FR2 | AC-4 |
| TS-5 | `code_quality_naming_item_states_the_two_way_check` | manual inspection | FR3 | AC-5 |
| TS-6 | `change_set_is_markdown_only_and_leaves_tests_rs_untouched` | scripted check | FR5, NFR1 | AC-6 |
| TS-7 | `point_in_time_records_are_absent_from_the_change_set` | scripted check | FR5, NFR4 | AC-7 |
| TS-8 | `benign_edit_test_under_ts5_and_scaffold_test_under_ts4` | scripted check | FR1, FR2, FR4 | AC-8, AC-9 |

### 12.3 Scenario Descriptions

- **TS-1**: Run AC-1's grep over the three amended documents; assert zero matches
  for all six nonexistent names.
- **TS-2**: Extract every backticked test name from the three documents and
  assert each matches exactly one `fn <name>()` in
  `src-tauri/src/window_host/tests.rs`.
- **TS-3**: Two-way set diff between the documented names and the 25 test
  functions the amended feature added (tests.rs:6006-6717); assert both
  directions empty and the per-TS counts are 1/1/1/18/3/1 for target-document
  TS-1/TS-2/TS-3/TS-4/TS-5/TS-6.
- **TS-4**: Assert VERIFICATION.md's Test Verification expectation names 25
  entries and no longer contains the string "six new".
- **TS-5**: Read the revised Code Quality naming inspection item and confirm it
  names `src-tauri/src/window_host/tests.rs` as the side of truth and states both
  directions of the bijection plus the NFR3 convention check.
- **TS-6**: `git diff --name-only` against the base revision; assert no path
  under `src-tauri/` or `crates/` appears.
- **TS-7**: `git diff --name-only` against the base revision; assert no path
  under `feature-docs/mouse-report-held-callsite-test/{reviews,phase-state,tasks}/`
  or `test-docs/mouse-report-held-callsite-test/` appears.
- **TS-8**: Assert the TS attribution of the two decision-carrying names in all
  three documents, and that the scaffold test's entry notes its role.

## 13. Glossary

| Term | Definition |
|------|------------|
| Target-document TS ID | A scenario ID (TS-1 … TS-6, TS-M1) already present inside `feature-docs/mouse-report-held-callsite-test/`; the subject matter of this feature. |
| This feature's TS ID | A scenario ID (TS-1 … TS-8) defined in section 12 for verifying this feature's own change. |
| Scaffold / sanity anchor | `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call`, which proves the synthetic callee pattern matches unquoted text so TS-4's negative cases are not vacuous. |
| Point-in-time record | `reviews/roundN.yaml`, `phase-state/`, `tasks/`, and `test-docs/{feature}/` — historical artifacts that are never retro-edited. |
| Bijection check | The two-way naming check of FR3: every documented name exists in the file, and every added test appears in exactly one TS row. |

## 14. Confirmations

### 14.1 Confirmed Items

- [x] Remedy direction: revise the documents to the implementation names. The
      implemented names already satisfy the target SPEC's NFR3 naming pattern, so
      aligning the documents to the code resolves the mismatch without touching
      tests.rs, preserving behaviour, coverage and test granularity.
- [x] Amended-file scope: SPEC.md, VERIFICATION.md and REQUIREMENTS.md. All three
      carry the same six unresolvable names; leaving REQUIREMENTS.md out would
      keep a third document pointing at tests that do not exist. Point-in-time
      records stay untouched.
- [x] Benign-edit test TS assignment: follow the source's own sectioning, which
      places `button_path_benign_edits_to_the_real_source_are_still_accepted`
      under TS-5.
- [x] Scenario table granularity: list every test name, so the acceptance
      criterion can be the exact one ("every name in the table grep-resolves to a
      real test") rather than a weakened prefix match; the scaffold test is listed
      with its role noted.
- [x] AC-number alignment scope: out of scope. The AC-number mismatch in the
      tests' doc comments is independent of the test-name mismatch and would
      extend the change set into tests.rs comments. Recorded for separate
      follow-up.
- [x] Design step: skipped. The change set is three Markdown documents. Nothing is
      rendered, no UI surface is added or altered, and no design token, MD3
      constant or CSS variable is touched.

### 14.2 Assumptions

| ID | Assumption | Reversible |
|----|------------|------------|
| A1 | The 25 test functions between tests.rs:6006 and tests.rs:6717 are the complete set this feature must document. Verified by reading `src-tauri/src/window_host/tests.rs`: 1 (TS-1) + 1 (TS-2) + 1 (TS-3) + 1 (TS-6) + 18 (TS-4) + 3 (TS-5) = 25. | yes |
| A2 | The source file's own section comments ("AC-3/TS-1", "AC-4/TS-2", "AC-5/TS-3", "AC-2/TS-6", "AC-1/TS-4", "AC-6/TS-5") are the authoritative TS assignment for each implementation test, which places `button_path_benign_edits_to_the_real_source_are_still_accepted` under TS-5. | yes |
| A3 | `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam` is a real pre-existing test (tests.rs:1780). Every reference to it in the three documents (SPEC.md:52, 89, 411; VERIFICATION.md:80; REQUIREMENTS.md:247, 400) is correct and is NOT part of this feature's change. | yes |
| A4 | TS-M1 ("manual: none required") is a manual scenario, not a Rust test function. It stays as written and is excluded from every name check. | yes |
| A5 | The six documented names exist in no form in the implementation file — they are not stale renames of present functions but names that were never written. | yes |
| A6 | Rewriting these three Markdown files cannot break any build, test, lint or typecheck gate, since no tool reads them. | yes |
| A7 | The orchestrator verified that the six nonexistent names appear in exactly the three files in scope and nowhere else under `feature-docs/mouse-report-held-callsite-test/` or `test-docs/` — IMPLEMENTATION.md carries none of them. The amended-file scope is therefore complete. | yes |

### 14.3 Unresolved Items

None — every requirement is resolved.

## 15. References

- `src-tauri/src/window_host/tests.rs` — normative source of the 25 test names
- `feature-docs/mouse-report-held-callsite-test/SPEC.md` — amended (FR1)
- `feature-docs/mouse-report-held-callsite-test/VERIFICATION.md` — amended (FR2, FR3)
- `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md` — amended (FR4)
- test/README.md "Test Naming Conventions" — the `<subject>_<scenario>_<expected>` convention (NFR3)
- `feature-docs/spec-test-name-traceability/SPEC.md` — the specification derived from this document
