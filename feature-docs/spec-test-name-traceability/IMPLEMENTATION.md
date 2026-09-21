# Implementation Plan: spec-test-name-traceability

## Overview

The three narrative documents of `feature-docs/mouse-report-held-callsite-test/`
are rewritten so that every identifier they present as a test name resolves to a
real function in `src-tauri/src/window_host/tests.rs`. The change set is Markdown
only; the implementation file is read-only for every task.

## Technology Stack

- **Language / Framework**: none — the deliverable is Markdown prose and tables
  in the form the target documents already use (CommonMark bullet lists and
  pipe tables).
- **Key libraries**: none. This feature introduces NO new dependency, so the
  dependency licence ledger is empty and `project.license` (`MIT`) is untouched.
- **Tooling**: the project's existing component commands (`cargo check`,
  `cargo test --lib`, `cargo fmt --check`, `bun test`, `bunx biome check .`) act
  as regression gates only — no tool reads the amended documents (NFR1).

## Layer Structure

Authority runs one way and is never inverted:

1. `src-tauri/src/window_host/tests.rs` — normative, **read-only for every
   task**. No rename, no body edit, no comment edit (FR5, NFR1).
2. The canonical assignment table in this document — the single transcription of
   that file's added test set. Every task copies identifiers from here.
3. The three amended documents — each owned by exactly ONE task.

Permitted dependency direction: 1 → 2 → 3. A document never becomes the side of
truth for a name, and no task edits a document owned by another task.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Canonical test-name assignment (section below) | Single source of the 25 identifiers and their target-document TS attribution | **pre**: `tests.rs` is unmodified at the base revision. **post**: the amended document lists exactly these 25 identifiers, character-for-character, under exactly this TS attribution, with per-TS counts TS-1=1, TS-2=1, TS-3=1, TS-4=18, TS-5=3, TS-6=1 | task0001, task0002, task0003 |
| Added-test-set definition (decision D2) | Defines which functions of `tests.rs` count as "the tests that feature added" | **pre**: the span from the `AC-3/TS-1` section marker to end of file. **post**: the set is exactly the functions carrying a `#[test]` attribute in that span (25); fixture/helper functions in the same span carry no such attribute and are excluded | task0001, task0002, task0003 |
| Identifier transcription rule (decision D6) | How an identifier is written into Markdown | **post**: one single-backtick span, copied character-for-character, never re-cased, never abbreviated, never wrapped inside the identifier — a long name overflows the line rather than breaking (NFR2) | task0001, task0002, task0003 |
| Scaffold role-note contract | What the TS-4 entry for the sanity test must convey | **post**: the entry identifies `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` as the sanity/scaffold anchor — it proves the synthetic callee pattern matches unquoted text, so TS-4's negative cases are not vacuous. Wording is in the host document's own language (AC-9) | task0001, task0002, task0003 |
| Change-set discipline rule (decision D3) | Bounds every task's diff | **post**: a task's diff contains exactly its own one document; nothing under `src-tauri/`, `crates/`, or the target feature's `reviews/`, `phase-state/`, `tasks/`, or `test-docs/mouse-report-held-callsite-test/` (FR5, NFR4) | task0001, task0002, task0003 |

## Canonical test-name assignment (25 identifiers)

Transcribed from `src-tauri/src/window_host/tests.rs` and confirmed present at
the listed lines. Line numbers are informational; the **names are normative**.
The TS column is the *target-document* TS ID (the scenario IDs that already
exist inside `feature-docs/mouse-report-held-callsite-test/`), never this
feature's own TS IDs.

### Single-test scenarios

| Target-document TS | Test function | tests.rs |
|---|---|---|
| TS-1 | `run_button_decision_calls_apply_outcome_with_held_once_with_live_held_as_fourth_arg` | 6007 |
| TS-2 | `handle_mouse_wheel_calls_apply_wheel_report_step_once_with_live_held_as_fifth_arg` | 6031 |
| TS-3 | `neither_call_site_body_contains_the_held_unaware_apply_path` | 6056 |
| TS-6 | `body_extractor_returns_exactly_the_two_named_bodies_excluding_the_motion_path` | 6082 |

### TS-4 — 18 tests (source section `AC-1/TS-4`, tests.rs:6118-6404)

| # | Test function | tests.rs | Note |
|---|---|---|---|
| 1 | `sanity_the_fake_callee_pattern_matches_a_real_unquoted_call` | 6151 | scaffold / sanity anchor — see the role-note contract above |
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

### TS-5 — 3 tests (source section `AC-6/TS-5`, tests.rs:6406-6775)

| # | Test function | tests.rs |
|---|---|---|
| 1 | `button_path_real_body_mutations_are_rejected_and_the_unmutated_body_accepted` | 6440 |
| 2 | `wheel_path_real_body_mutations_are_rejected_and_the_unmutated_body_accepted` | 6592 |
| 3 | `button_path_benign_edits_to_the_real_source_are_still_accepted` | 6717 |

### TS-M1

A manual scenario (`manual: none required`), not a Rust test function. It stays
exactly as written and is excluded from every name check (assumption A4).

## Conventions

- **Language per document** (NFR3): SPEC.md and REQUIREMENTS.md stay Japanese;
  VERIFICATION.md stays English. A task writes new prose in its document's
  existing language.
- **What may change**: only the name cells, VERIFICATION.md's count claim,
  VERIFICATION.md's AC-1 verification wording, and VERIFICATION.md's Code
  Quality naming inspection item. Section headings, TS IDs, requirement/AC
  columns, and every other table stay as they are (NFR3).
- **Identifiers presented as test names vs. other identifiers**: the documents
  also mention production identifiers (function names of the code under test,
  module paths) and one *pre-existing* test,
  `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam`
  (tests.rs:1780). Only identifiers presented as *test names* are in scope of
  the name checks; the pre-existing test reference is already correct and stays
  untouched (assumption A3).
- **No new files**: no checker script, no test file, no helper document. The
  declared change set admits only the three amended documents plus this
  feature's own `feature-docs/` and `test-docs/` trees, so creating a script
  anywhere else would itself fail AC-6.
- **Point-in-time records are never edited** (NFR4), even where they repeat a
  name this feature replaces.

## Cross-task Design Decisions

### D1 — `tests.rs` is normative and read-only

Affected tasks: all. The remedy direction settled at create-spec is "revise the
documents to the implementation names". No task edits `src-tauri/src/window_host/tests.rs`
for any reason, including the AC-number mismatch in its doc comments, which
create-spec placed explicitly out of scope.

### D2 — "The tests that feature added" = the `#[test]`-annotated functions only

Affected tasks: all, and the verify phase. Between the `AC-3/TS-1` section
marker and end of file the source also defines fixture/helper functions that
carry no `#[test]` attribute (they build inputs for the tests). A naive "every
function declaration in the span" extraction counts those and yields more than
25. The added-test set is therefore defined as **the functions carrying a
`#[test]` attribute in that span** — 25 of them, matching the canonical table
above. Every two-way set check (this feature's AC-3, and the bijection item FR3
introduces into the target document) uses this definition.

Rationale: it is the definition that makes AC-3's stated intent — "the
implementation's added test set" — mechanically decidable, and the one the
revised naming inspection item must state so a future reader reproduces the same
set.

### D3 — One document per task; tasks are parallel-safe by file partition

Affected tasks: all. The three tasks own disjoint files, so they can run fully
in parallel with no ordering and no merge conflicts. Their only coupling is the
canonical table above; because all three copy from the same pinned table, the
cross-document equality AC-3/AC-8/AC-9/AC-10 demand holds without any task
reading another task's plan or output.

### D4 — Verification without test code

Affected tasks: all. No test code can be added: FR5/NFR1 forbid touching
`src-tauri/` and `crates/`, and no other tree is in the declared change set. Each
task's acceptance criteria are discharged by shell checks (grep against
`tests.rs`, `git diff --name-only` against the task's base) run by the
implementer and recorded in that task's test record. The integrated form of the
same checks is this feature's VERIFICATION.md.

### D5 — Per-TS attribution follows the source's own section comments

Affected tasks: all. The section comments in `tests.rs` (`AC-3/TS-1`,
`AC-4/TS-2`, `AC-5/TS-3`, `AC-2/TS-6`, `AC-1/TS-4`, `AC-6/TS-5`) are the
authoritative TS assignment (assumption A2). This is what places
`button_path_benign_edits_to_the_real_source_are_still_accepted` under TS-5
although its subject matter overlaps TS-4 (AC-8), and what places the scaffold
test under TS-4 (AC-9). No task reassigns a test on its own judgment.

### D6 — The stale names are located positionally and never reproduced

Affected tasks: all. Each amended document carries exactly six stale names, one
per scenario entry, and they are replaced wholesale by the canonical list — so a
task locates them by TS ID and position, not by matching the old string. Neither
this document nor this feature's VERIFICATION.md reproduces a stale name, so the
repository is not re-seeded with identifiers that resolve to nothing. The
"no stale name survives" check is therefore expressed set-wise: an identifier
the base revision presented as a test name and that has no `#[test]` function in
`tests.rs` must have zero occurrences in the amended document.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| An identifier is transcribed inexactly (typo, re-casing, mid-identifier line wrap) and no longer grep-resolves | Medium | High | Transcription rule above; every task's AC requires each listed name to match exactly one `fn` in `tests.rs`; this feature's TS-2 re-checks it integrated |
| The three documents drift from each other (different name set or different TS attribution) | Medium | High | The canonical table is the single source all three tasks copy from; this feature's TS-3 runs a two-way set diff with the per-TS counts 1/1/1/18/3/1 |
| The added-test set is computed with a naive function scan and silently includes helper functions | Medium | Medium | Decision D2 fixes the definition; the revised naming inspection item states it, and this feature's TS-3 asserts 25 |
| An edit leaks into `tests.rs` or a point-in-time record | Low | High | Each task's file set is exactly one document; this feature's TS-6/TS-7 assert it with `git diff --name-only` |
| The revised naming inspection item restates a one-way check and stays unsatisfiable | Low | Medium | FR3 fixes the three required parts and the direction of authority; this feature's TS-5 is a manual read-back of that item |

## Open Questions

- [x] Resolved during create-plan, no longer open. AC-3 originally described its
      set check as "the 25 `fn` names ... between the `AC-3/TS-1` section marker
      and end of file". Taken literally, that span also contains non-`#[test]`
      fixture functions, so the literal reading yields more than 25 names.
      Decision D2 fixes the operative definition, and the orchestrator applied a
      matching post-validation correction to AC-3's Check wording in this
      feature's SPEC.md and REQUIREMENTS.md so the criterion is literally
      executable and agrees with its own stated count of 25. The requirement
      set, the count, and AC-3's intent are unchanged. Rationale and the Codex
      consultation behind it are recorded in
      `phase-state/batch-audit.yaml` and `phase-state/create-plan.yaml`.
