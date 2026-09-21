# Feature: spec-test-name-traceability

## Overview

The three narrative documents of `feature-docs/mouse-report-held-callsite-test/`
(SPEC.md, VERIFICATION.md, REQUIREMENTS.md) name six test functions that do not
exist in `src-tauri/src/window_host/tests.rs`. The implementation actually added
25 test functions (tests.rs:6006-6717). This feature rewrites the test-name cells
of those three documents to the real function names, corrects VERIFICATION.md's
"six new entries" count claim, and replaces VERIFICATION.md's unsatisfiable Code
Quality naming inspection item with a two-way check against the implementation
file. The change set is Markdown only; `src-tauri/src/window_host/tests.rs` is
not modified.

## Objectives

- Make the three narrative documents of `feature-docs/mouse-report-held-callsite-test/`
  name the test functions that actually exist in `src-tauri/src/window_host/tests.rs`,
  so their traceability tables can be checked mechanically instead of by reading
  prose.
- Remove the count claim that the feature added six tests; the implementation
  added 25 test functions (tests.rs:6006-6717).
- Restore the usefulness of VERIFICATION.md's Code Quality naming inspection
  item, which today demands character-for-character equality against six names
  that do not exist in the codebase and is therefore unsatisfiable as written.

## Terminology: two sets of TS IDs

Two unrelated scenario-ID spaces appear in this document. They are kept distinct
throughout:

| Term | Meaning |
|------|---------|
| **target-document TS IDs** (TS-1 … TS-6, TS-M1) | The scenario IDs already present inside `feature-docs/mouse-report-held-callsite-test/`. They are the subject matter of this feature and are never renumbered. |
| **this feature's TS IDs** (TS-1 … TS-8, under "Test Scenarios") | The verification scenarios for *this* feature's own change. |

## User Stories

### US1: Mechanically checkable traceability tables

As a developer verifying the `mouse-report-held-callsite-test` feature, I want
every test name in its three narrative documents to resolve to a real function
in `src-tauri/src/window_host/tests.rs`, so that I can check the traceability
tables with grep instead of by reading prose.

**Acceptance Criteria:**
- [ ] AC-1: no nonexistent name survives in the three amended documents
- [ ] AC-2: every documented name resolves to exactly one function
- [ ] AC-3: documented name set equals the implementation's added test set
- [ ] AC-10: TS-4 lists exactly 18 names and TS-5 exactly 3 in every document

### US2: A truthful test count and a satisfiable naming inspection

As a reviewer running VERIFICATION.md's checklist, I want the expected test count
and the Code Quality naming item to describe a check I can actually perform
against the implementation file, so that the checklist yields a real pass/fail
instead of an impossible comparison.

**Acceptance Criteria:**
- [ ] AC-4: VERIFICATION.md no longer claims six new test entries
- [ ] AC-5: the Code Quality naming item states the two-way check against
      `src-tauri/src/window_host/tests.rs`

### US3: Documentation-only correction that preserves the record

As a maintainer of the feature-docs tree, I want this correction to touch only
the three current-state Markdown documents, so that the implementation file and
the point-in-time records stay exactly as they were.

**Acceptance Criteria:**
- [ ] AC-6: the change set is Markdown only and leaves the implementation file
      untouched
- [ ] AC-7: no point-in-time record of the amended feature is in the change set
- [ ] AC-8 / AC-9: the two decision-carrying names are attributed as decided

## Technical Requirements

### Functional Requirements

- **FR1 — SPEC.md Test Scenarios revised to implementation test names:**
  In `feature-docs/mouse-report-held-callsite-test/SPEC.md`, the six nonexistent
  test names in the "Test Scenarios > Unit Tests" bullets (SPEC.md:234, 240, 244,
  248, 254, 259) are replaced by the real test function names in
  `src-tauri/src/window_host/tests.rs`. TS-1/TS-2/TS-3/TS-6 each name their single
  implementation test, TS-4 enumerates all 18 of its tests, and TS-5 enumerates all
  3 of its tests — 25 names in total. The scaffold test
  `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` is listed under TS-4
  with its role noted (it is the sanity anchor proving the synthetic callee pattern
  matches unquoted text, so TS-4's negative cases are not vacuous). The TS IDs,
  their FR/AC mappings, and TS-M1 are unchanged.

- **FR2 — VERIFICATION.md Test Scenarios table revised, and its count claim corrected:**
  In `feature-docs/mouse-report-held-callsite-test/VERIFICATION.md`, the six
  nonexistent names in the "Test Scenarios from SPEC.md" table (VERIFICATION.md:39-44)
  are replaced by the same 25 implementation test names under the same TS IDs and the
  same per-row assignment used in FR1. The Test Verification expectation at
  VERIFICATION.md:24-25, which currently reads "the six new `window_host::tests`
  entries below appear in the run", is corrected to the real count of 25. The AC-1 row
  at VERIFICATION.md:73 ("confirm exit 0 and that TS-1 … TS-6 appear in the run") is
  rewritten so the thing to confirm in the run is the named test functions rather than
  the TS IDs, which never appear in cargo test output. The TS-M1 row
  (VERIFICATION.md:45), the Verification Summary table, and every other table are
  otherwise unchanged.

- **FR3 — VERIFICATION.md Code Quality naming inspection item replaced:**
  The inspection item at VERIFICATION.md:55-56, currently "New test names follow
  `<subject>_<scenario>_<expected>` and match the TS names in this document character
  for character (NFR3)", is replaced by an item that states a two-way (bijection)
  check against the real file. The revised item checks all three of the following, and
  names `src-tauri/src/window_host/tests.rs` explicitly as the side of truth:
  (a) every test name listed in that document's Test Scenarios table exists verbatim
  as a `fn <name>()` in `src-tauri/src/window_host/tests.rs`;
  (b) every test function that feature added to that file appears in exactly one TS
  row of the table — no added test is undocumented and no documented name is absent
  from the file;
  (c) each of those names follows the repository's `<subject>_<scenario>_<expected>`
  convention (NFR3, test/README.md "Test Naming Conventions").
  The direction of authority is stated: the file is normative and the document follows
  it.

- **FR4 — REQUIREMENTS.md traceability table revised to implementation test names:**
  In `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`, the test-name
  column of the section 12.2 traceability table (REQUIREMENTS.md:424-429) is replaced
  by the same 25 implementation test names under the same TS IDs and the same per-row
  assignment used in FR1 and FR2. The requirement / AC columns, the TS-M1 row
  (REQUIREMENTS.md:430), and the per-scenario supplementary notes
  (REQUIREMENTS.md:434-455, which reference TS IDs only and contain no test names) are
  unchanged.

- **FR5 — Documentation-only change set; production and record files untouched:**
  The change set is Markdown only, limited to exactly three files:
  `feature-docs/mouse-report-held-callsite-test/{SPEC.md, VERIFICATION.md, REQUIREMENTS.md}`
  (plus this feature's own workflow-generated `feature-docs/spec-test-name-traceability/**`
  and `test-docs/spec-test-name-traceability/**`).
  `src-tauri/src/window_host/tests.rs` is NOT modified at all — no rename, no body edit,
  no comment edit — and no other file under `src-tauri/` or `crates/` is touched. The
  point-in-time records of `feature-docs/mouse-report-held-callsite-test/` —
  `reviews/roundN.yaml`, `phase-state/`, `tasks/` — and
  `test-docs/mouse-report-held-callsite-test/` are left byte-identical.

### Non-Functional Requirements

- **NFR1 — No source, manifest, or build-input change:**
  The feature changes no Rust source, no TypeScript source, no package manifest, and
  no build input. Every build, test, format and typecheck command in the project's
  components produces the same result before and after the change.

- **NFR2 — Names copied verbatim:**
  Each of the 25 names is transcribed character-for-character from
  `src-tauri/src/window_host/tests.rs`. No name is paraphrased, abbreviated, re-cased,
  line-wrapped inside the identifier, or reformatted. Identifiers are written in
  backticks in the Markdown, as the surrounding documents already do.

- **NFR3 — Existing document structure preserved:**
  Section headings, TS IDs (TS-1 … TS-6, TS-M1), the FR/NFR-to-AC-to-TS mappings in
  SPEC.md's Traceability tables (SPEC.md:328-355), VERIFICATION.md's Functional
  Requirements Coverage table (VERIFICATION.md:84-101), and the documents' existing
  language (Japanese in SPEC.md/REQUIREMENTS.md, English in VERIFICATION.md) are all
  preserved. Only the name cells, the count claim, the AC-1 verification wording, and
  the Code Quality naming item change.

- **NFR4 — Point-in-time records are immutable:**
  `reviews/roundN.yaml`, `phase-state/`, `tasks/` and
  `test-docs/mouse-report-held-callsite-test/` are historical records. They are never
  edited to match a later correction, even when they repeat a name this feature is
  replacing.

## Acceptance Criteria

| ID | Criterion |
|----|-----------|
| AC-1 | None of the six nonexistent names survives in the three amended documents. Check: grep for the six names over SPEC.md, VERIFICATION.md and REQUIREMENTS.md of `feature-docs/mouse-report-held-callsite-test/` exits non-zero (zero matches). |
| AC-2 | Every test name the three amended documents list resolves to exactly one function in the implementation file. Check: for each backticked test name N, `grep -c "fn ${N}()" src-tauri/src/window_host/tests.rs` is 1. TS-M1's "manual: none required" is excluded; it is not a function name. |
| AC-3 | The documented name set equals the implementation's added test set, in both directions — 25 names, no omission and no surplus. Check: a two-way set diff between the names extracted from the three documents and the 25 `fn` names in `src-tauri/src/window_host/tests.rs` between the `AC-3/TS-1` section marker (tests.rs:6000) and end of file (tests.rs:6776) is empty. |
| AC-4 | VERIFICATION.md no longer claims six new test entries. Check: grep for "six new" in that file returns zero matches, and the Test Verification "Expected" line names 25 entries. |
| AC-5 | VERIFICATION.md's Code Quality naming inspection item states the bijection check of FR3 rather than the one-way "match this document" claim. Check: grep for "character for character" returns zero matches; the naming item mentions the literal string `src-tauri/src/window_host/tests.rs` and states both directions. |
| AC-6 | The change set is Markdown only and leaves the implementation file untouched. Check: `git diff --name-only <base>..HEAD` lists only the three amended documents and paths under `feature-docs/spec-test-name-traceability/` or `test-docs/spec-test-name-traceability/`; no path starts with `src-tauri/` or `crates/`. |
| AC-7 | No point-in-time record of the amended feature is in the change set. Check: `git diff --name-only <base>..HEAD` matches nothing under `feature-docs/mouse-report-held-callsite-test/{reviews,phase-state,tasks}/` or `test-docs/mouse-report-held-callsite-test/`. |
| AC-8 | `button_path_benign_edits_to_the_real_source_are_still_accepted` is attributed to TS-5 in all three documents, following the source's own sectioning, and appears under no other TS ID. |
| AC-9 | `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` is listed under TS-4 in all three documents, with a role note identifying it as the sanity/scaffold anchor. |
| AC-10 | TS-4 lists exactly 18 names and TS-5 exactly 3, in every one of the three documents. |

## Implementation Approach

### Architecture

No runtime architecture is involved. The change is a text substitution inside
three Markdown documents, driven by one authoritative list of identifiers read
from the implementation file:

```
src-tauri/src/window_host/tests.rs   (normative, read-only)
            │
            ├──► feature-docs/mouse-report-held-callsite-test/SPEC.md          (FR1)
            ├──► feature-docs/mouse-report-held-callsite-test/VERIFICATION.md  (FR2, FR3)
            └──► feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md  (FR4)
```

The direction of authority is one-way: the file is normative and the three
documents follow it.

### Target test-name correspondence (the exact 25 names)

This is the target list for FR1, FR2 and FR4. The same per-TS assignment is used
in all three amended documents. Line numbers are as of the base revision and are
informational; the names are normative.

#### target-document TS-1 (1 name)

| Test function | tests.rs |
|---|---|
| `run_button_decision_calls_apply_outcome_with_held_once_with_live_held_as_fourth_arg` | 6007 |

#### target-document TS-2 (1 name)

| Test function | tests.rs |
|---|---|
| `handle_mouse_wheel_calls_apply_wheel_report_step_once_with_live_held_as_fifth_arg` | 6031 |

#### target-document TS-3 (1 name)

| Test function | tests.rs |
|---|---|
| `neither_call_site_body_contains_the_held_unaware_apply_path` | 6056 |

#### target-document TS-6 (1 name)

| Test function | tests.rs |
|---|---|
| `body_extractor_returns_exactly_the_two_named_bodies_excluding_the_motion_path` | 6082 |

#### target-document TS-4 (18 names) — source section `AC-1/TS-4`, tests.rs:6118-6404

| # | Test function | tests.rs | Note |
|---|---|---|---|
| 1 | `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` | 6151 | scaffold / sanity anchor: proves the synthetic callee pattern matches unquoted text, so TS-4's negative cases are not vacuous |
| 2 | `scanner_treats_a_correct_call_written_inside_a_line_comment_as_absent` | 6164 | |
| 3 | `scanner_treats_a_correct_call_written_inside_a_string_literal_as_absent` | 6175 | |
| 4 | `scanner_ignores_call_shaped_text_inside_nested_block_comments` | 6187 | |
| 5 | `scanner_ignores_call_shaped_text_inside_a_raw_string_with_differing_hash_counts` | 6204 | |
| 6 | `scanner_tokenizes_byte_and_c_string_literals_as_opaque_literals` | 6222 | |
| 7 | `scanner_distinguishes_a_lifetime_marker_from_an_adjacent_character_literal` | 6241 | |
| 8 | `scanner_does_not_terminate_a_character_literal_on_an_escaped_quote` | 6268 | |
| 9 | `scanner_does_not_terminate_a_string_literal_on_an_escaped_quote` | 6283 | |
| 10 | `scanner_never_matches_an_identifier_as_a_prefix_of_a_longer_one` | 6299 | |
| 11 | `argument_splitter_ignores_a_trailing_comma` | 6312 | |
| 12 | `judge_call_tolerates_the_argument_list_rewrapped_across_lines` | 6328 | |
| 13 | `judge_call_tolerates_a_comment_between_the_callee_and_its_opening_paren` | 6339 | |
| 14 | `judge_call_rejects_a_let_bound_shadow_of_the_receiver_before_the_call` | 6350 | |
| 15 | `judge_call_rejects_a_closure_parameter_shadow_of_the_receiver_before_the_call` | 6361 | |
| 16 | `judge_call_rejects_a_match_arm_pattern_shadow_of_the_receiver_before_the_call` | 6372 | |
| 17 | `judge_call_rejects_an_if_let_pattern_shadow_of_the_receiver_before_the_call` | 6383 | |
| 18 | `judge_call_rejects_when_the_held_unaware_path_also_appears_elsewhere_in_the_body` | 6397 | |

#### target-document TS-5 (3 names) — source section `AC-6/TS-5`, tests.rs:6406-6775

| # | Test function | tests.rs |
|---|---|---|
| 1 | `button_path_real_body_mutations_are_rejected_and_the_unmutated_body_accepted` | 6440 |
| 2 | `wheel_path_real_body_mutations_are_rejected_and_the_unmutated_body_accepted` | 6592 |
| 3 | `button_path_benign_edits_to_the_real_source_are_still_accepted` | 6717 |

#### target-document TS-M1

TS-M1 is a manual scenario ("manual: none required"), not a Rust test function.
It stays as written and is excluded from every name check.

Per-TS counts: TS-1 = 1, TS-2 = 1, TS-3 = 1, TS-4 = 18, TS-5 = 3, TS-6 = 1;
total 25.

### Data Flow

```
tests.rs  ──read──►  25 verbatim identifiers  ──write──►  SPEC.md / VERIFICATION.md / REQUIREMENTS.md
                                              (name cells, count claim,
                                               AC-1 wording, naming item)
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/window_host/tests.rs`: read-only source of the 25 identifiers.
- `feature-docs/mouse-report-held-callsite-test/{SPEC.md, VERIFICATION.md, REQUIREMENTS.md}`:
  the three amended documents.

**External Dependencies:**
- None.

### File Structure

```
feature-docs/mouse-report-held-callsite-test/
├── SPEC.md           # amended (FR1)
├── VERIFICATION.md   # amended (FR2, FR3)
├── REQUIREMENTS.md   # amended (FR4)
├── reviews/          # untouched (NFR4)
├── phase-state/      # untouched (NFR4)
└── tasks/            # untouched (NFR4)

src-tauri/src/window_host/tests.rs   # read-only, not modified (FR5, NFR1)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/spec-test-name-traceability/**`
- `test-docs/spec-test-name-traceability/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

Feature-specific paths in addition to the two defaults:

- `feature-docs/mouse-report-held-callsite-test/SPEC.md`
- `feature-docs/mouse-report-held-callsite-test/VERIFICATION.md`
- `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`

## Test Scenarios

The IDs below are **this feature's** scenario IDs (see "Terminology: two sets of
TS IDs").

### Scripted Checks

- [ ] **TS-1** `docs_no_longer_reference_any_nonexistent_test_name` — covers FR1,
      FR2, FR4 (AC-1). Run AC-1's grep over the three amended documents; assert
      zero matches for all six nonexistent names.
- [ ] **TS-2** `every_documented_test_name_resolves_to_a_function_in_tests_rs` —
      covers FR1, FR2, FR4, NFR2 (AC-2). Extract every backticked test name from
      the three documents and assert each matches exactly one `fn <name>()` in
      `src-tauri/src/window_host/tests.rs`.
- [ ] **TS-3** `documented_name_set_equals_the_features_added_test_set` — covers
      FR1, FR2, FR4 (AC-3, AC-10). Two-way set diff between the documented names
      and the 25 test functions the amended feature added (tests.rs:6006-6717);
      assert both directions empty and the per-TS counts are 1/1/1/18/3/1 for
      target-document TS-1/TS-2/TS-3/TS-4/TS-5/TS-6.
- [ ] **TS-4** `verification_test_count_claim_matches_the_implementation` —
      covers FR2 (AC-4). Assert VERIFICATION.md's Test Verification expectation
      names 25 entries and no longer contains the string "six new".
- [ ] **TS-6** `change_set_is_markdown_only_and_leaves_tests_rs_untouched` —
      covers FR5, NFR1 (AC-6). `git diff --name-only` against the base revision;
      assert no path under `src-tauri/` or `crates/` appears.
- [ ] **TS-7** `point_in_time_records_are_absent_from_the_change_set` — covers
      FR5, NFR4 (AC-7). `git diff --name-only` against the base revision; assert
      no path under `feature-docs/mouse-report-held-callsite-test/{reviews,phase-state,tasks}/`
      or `test-docs/mouse-report-held-callsite-test/` appears.
- [ ] **TS-8** `benign_edit_test_under_ts5_and_scaffold_test_under_ts4` — covers
      FR1, FR2, FR4 (AC-8, AC-9). Assert the TS attribution of the two
      decision-carrying names in all three documents, and that the scaffold
      test's entry notes its role.

### Manual Inspection

- [ ] **TS-5** `code_quality_naming_item_states_the_two_way_check` — covers FR3
      (AC-5). Read the revised Code Quality naming inspection item and confirm it
      names `src-tauri/src/window_host/tests.rs` as the side of truth and states
      both directions of the bijection plus the NFR3 convention check.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] TS-M1 of the amended documents is the string "manual: none required", not a
      function name; every name check excludes it (assumption A4).
- [ ] `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam` is a
      real pre-existing test (tests.rs:1780); its references in the three
      documents (SPEC.md:52, 89, 411; VERIFICATION.md:80; REQUIREMENTS.md:247,
      400) are correct and are not part of this change (assumption A3).
- [ ] `button_path_benign_edits_to_the_real_source_are_still_accepted` overlaps
      TS-4's subject matter but belongs to TS-5 by the source's own sectioning
      (AC-8).

## Traceability

### Requirement → AC → Scenario

| Requirement | AC | This feature's scenarios |
|---|---|---|
| FR1 | AC-1, AC-2, AC-3, AC-8, AC-9, AC-10 | TS-1, TS-2, TS-3, TS-8 |
| FR2 | AC-1, AC-2, AC-3, AC-4, AC-8, AC-9, AC-10 | TS-1, TS-2, TS-3, TS-4, TS-8 |
| FR3 | AC-5 | TS-5 |
| FR4 | AC-1, AC-2, AC-3, AC-8, AC-9, AC-10 | TS-1, TS-2, TS-3, TS-8 |
| FR5 | AC-6, AC-7 | TS-6, TS-7 |
| NFR1 | AC-6 | TS-6 |
| NFR2 | AC-2 | TS-2 |
| NFR3 | AC-5 | TS-5 |
| NFR4 | AC-7 | TS-7 |

### Scenario → Requirement

| Scenario | Requirements | AC |
|---|---|---|
| TS-1 | FR1, FR2, FR4 | AC-1 |
| TS-2 | FR1, FR2, FR4, NFR2 | AC-2 |
| TS-3 | FR1, FR2, FR4 | AC-3, AC-10 |
| TS-4 | FR2 | AC-4 |
| TS-5 | FR3 | AC-5 |
| TS-6 | FR5, NFR1 | AC-6 |
| TS-7 | FR5, NFR4 | AC-7 |
| TS-8 | FR1, FR2, FR4 | AC-8, AC-9 |

## Assumptions

| ID | Assumption | Reversible |
|---|---|---|
| A1 | The 25 test functions between tests.rs:6006 and tests.rs:6717 are the complete set this feature must document. Verified by reading `src-tauri/src/window_host/tests.rs`: 1 (TS-1) + 1 (TS-2) + 1 (TS-3) + 1 (TS-6) + 18 (TS-4) + 3 (TS-5) = 25. | yes |
| A2 | The source file's own section comments ("AC-3/TS-1", "AC-4/TS-2", "AC-5/TS-3", "AC-2/TS-6", "AC-1/TS-4", "AC-6/TS-5") are the authoritative TS assignment for each implementation test, which places `button_path_benign_edits_to_the_real_source_are_still_accepted` under TS-5. | yes |
| A3 | `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam` is a real pre-existing test (tests.rs:1780). Every reference to it in the three documents (SPEC.md:52, 89, 411; VERIFICATION.md:80; REQUIREMENTS.md:247, 400) is correct and is NOT part of this feature's change. | yes |
| A4 | TS-M1 ("manual: none required") is a manual scenario, not a Rust test function. It stays as written and is excluded from every name check. | yes |
| A5 | The six documented names exist in no form in the implementation file — they are not stale renames of present functions but names that were never written. | yes |
| A6 | Rewriting these three Markdown files cannot break any build, test, lint or typecheck gate, since no tool reads them. | yes |
| A7 | The orchestrator verified that the six nonexistent names appear in exactly the three files in scope and nowhere else under `feature-docs/mouse-report-held-callsite-test/` or `test-docs/` — IMPLEMENTATION.md carries none of them. The amended-file scope is therefore complete. | yes |

## Design Step

**Status:** skipped.

**Reason:** The change set is three Markdown documents. Nothing is rendered, no
UI surface is added or altered, and no design token, MD3 constant or CSS variable
is touched.

## Success Criteria

- [ ] All functional requirements (FR1-FR5) are implemented
- [ ] All non-functional requirements (NFR1-NFR4) hold
- [ ] All acceptance criteria (AC-1 … AC-10) pass
- [ ] All test scenarios (TS-1 … TS-8) pass

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is resolved.

## References

- Implementation file (normative source of the 25 names):
  `src-tauri/src/window_host/tests.rs`
- Amended documents: `feature-docs/mouse-report-held-callsite-test/SPEC.md`,
  `feature-docs/mouse-report-held-callsite-test/VERIFICATION.md`,
  `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`
- Test naming convention (NFR3): test/README.md "Test Naming Conventions"
- Requirements document: `feature-docs/spec-test-name-traceability/REQUIREMENTS.md`
