# Feature: ring-push-blank-note-unconditional

## Overview

The explanatory NOTE above the survivor assertions in
`crates/term_core/src/ring_buffer/tests.rs` attributes the redundancy of the two
clearing sites inside `ring_push_blank` to the fixture's 2-row, zero-scrollback
dimensions. The true reason is unconditional: `evicted_abs` is captured before
the ring rotation and `new_bottom_abs` is computed after it, so both denote the
same ring slot for any `rows >= 1`. This feature rewrites that NOTE with the
unconditional reason and amends one YAML record field so all four sibling
records agree, changing no production code and no assertion.

Requirement content in this document is rendered from
`feature-docs/ring-push-blank-note-unconditional/REQUIREMENTS.md`.

## Objectives

- **OBJ-1:** Correct the NOTE above the survivor assertions in
  `crates/term_core/src/ring_buffer/tests.rs` so it gives the true,
  unconditional reason the two clearing sites inside `ring_push_blank` are
  redundant — the evaluation order of `evicted_abs` (captured pre-rotation)
  versus `new_bottom_abs` (computed post-rotation) — instead of attributing the
  redundancy to the fixture's 2-row, zero-scrollback dimensions.
- **OBJ-2:** Remove the documentation drift between that NOTE and the three
  sibling records that already state the fact unconditionally: SPEC.md FR6,
  VERIFICATION.md MT3, and (after this feature's amendment)
  `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` AC-5.
- **OBJ-3:** Keep the change purely textual — comment lines in the test module
  and record text in the tests.yaml — with
  `crates/term_core/src/ring_buffer.rs` and every assertion left
  byte-identical, so no runtime or test behavior changes.

## User Stories

Not applicable. The feature has no user-visible surface: it rewrites an English
comment block inside a Rust `#[cfg(test)]` module and one YAML record field.
There is no UI, no API, no data model and no architectural choice, so the design
step is skipped and no user story is defined. Acceptance is expressed directly
in Success Criteria below.

## Technical Requirements

### Functional Requirements

- **FR1 — NOTE states the unconditional evaluation-order reason:** The NOTE
  above the survivor assertions in `crates/term_core/src/ring_buffer/tests.rs`
  (currently lines 517-522) must state that `new_bottom_abs == evicted_abs`
  holds for EVERY `ring_push_blank` call with `rows >= 1`, independent of
  fixture dimensions and of `scrollback_capacity`, and must give the reason:
  `evicted_abs` is captured from `ring_head` BEFORE the rotation
  (`crates/term_core/src/ring_buffer.rs:129`), while
  `new_bottom_abs = (ring_head + rows - 1) % rows` is computed AFTER
  `ring_head` advances by one (`ring_buffer.rs:204`, `:207`), so the two
  expressions denote the same ring slot.
- **FR2 — Fixture-scoped qualifiers removed from the NOTE:** The rewritten NOTE
  must contain no fixture-scoped qualifier. Specifically the phrases
  `in this 2-row, zero-scrollback fixture` (tests.rs:520) and
  `for this fixture, not independently pinned by it` (tests.rs:522) must be
  gone, replaced by unconditional wording.
- **FR3 — The NOTE's existing factual claim is preserved:** The rewritten NOTE
  must keep the fact the current NOTE already carries: removing only ONE of the
  two clearing sites inside `ring_push_blank` (the eviction-time clear or the
  new-bottom-row clear) still leaves
  `test_ring_push_blank_clears_recycled_row_overflow_entries` green. Only the
  reason for that fact changes, never the fact itself.
- **FR4 — NOTE placement, language and form unchanged:** The NOTE stays an
  English comment block at its current position — inside the inline
  `#[cfg(test)]` module, immediately above the survivor assertion
  `assert!(core.overflow.contains_key(&(0u32, abs1)))` at tests.rs:523 — and
  remains a comment. It is not promoted to an assertion, a doc comment, or a
  separate document.
- **FR5 — VERIFICATION MT3 and MT5 remain satisfiable:** The rewritten NOTE must
  continue to satisfy VERIFICATION.md MT3 (present, in English, stating both the
  fact and the reason that the two sites target the same row within a single
  push), and must not disturb the separate explanatory block above the test that
  MT5 checks.
- **FR6 — tests.yaml AC-5 red_reason amended to read unconditionally:**
  `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` AC-5's
  `red_reason` must drop the trailing `in this fixture` qualifier (currently at
  line 54: `... coincide within a single push in this fixture.`) so it reads
  unconditionally and matches the corrected NOTE, SPEC.md FR6 and
  VERIFICATION.md MT3. AC-5's `red_confirmed: false` and its "Comment-only
  criterion, not test-observable" character are unchanged — only the qualifier
  wording inside `red_reason` is corrected; the rest of the entry's text stays
  as written. `test-docs/ring-push-blank-row-scope-test/**` is part of this
  feature's declared change set.
- **FR7 — NOTE records the no-op consequence:** The rewritten NOTE must also
  state the consequence of `new_bottom_abs == evicted_abs`: the new-bottom-row
  clear at `crates/term_core/src/ring_buffer.rs:221-224` is always a no-op
  within a single push, because whichever eviction-time clear branch ran
  (`ring_buffer.rs:146-149`, `:177-180`, or `:196-199`) has already emptied that
  same absolute row.

### Non-Functional Requirements

- **NFR1 - Change containment (comment and record text only):** The change set is
  exactly two files and contains only comment / record text: (a)
  `crates/term_core/src/ring_buffer/tests.rs` — comment lines inside the inline
  `#[cfg(test)]` module; (b)
  `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` — the AC-5
  `red_reason` text. `crates/term_core/src/ring_buffer.rs` stays unmodified. No
  assertion, no fixture dimension, no test name, no production code changes
  anywhere.
- **NFR2 - No behavior change:** The `term_core` test suite's outcome must be
  identical before and after: the same tests pass, the same counts (825 passed /
  0 failed / 13 ignored per the sibling task's AC-7 record), with no new or
  removed test.
- **NFR3 - Comment style consistency:** The NOTE stays in English, keeps the
  surrounding `// ` line-comment style, and wraps at the comment width already
  used in that file, so rustfmt leaves it untouched.
- **NFR4 - Documents the redundancy, does not remove it:** The redundant
  new-bottom-row clear in `ring_buffer.rs:221-224` is described, never deleted or
  refactored. Removing it is explicitly out of scope for this feature.
- **NFR5 - Formatting clean:**
  `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` produces no
  output after the change.
- **NFR6 - tests.yaml stays valid and structurally unchanged:**
  `task0001.tests.yaml` remains valid YAML that parses to the same structure —
  same keys, same block-scalar style and indentation for AC-5's `red_reason`,
  same `red_confirmed` and `tests` values. Only characters inside the AC-5
  `red_reason` scalar change.

## Implementation Approach

### Architecture

No architecture is introduced or altered. Two text edits in two existing files:

```
crates/term_core/
├── src/
│   ├── ring_buffer.rs                 # UNMODIFIED (evidence source only)
│   │     :129                         #   evicted_abs captured (pre-rotation)
│   │     :146-149 / :177-180 / :196-199  #   eviction-time clear (exactly one per push)
│   │     :204                         #   ring_head advances by one
│   │     :207                         #   new_bottom_abs = (ring_head + rows - 1) % rows
│   │     :221-224                     #   new-bottom-row clear (the redundant site)
│   └── ring_buffer/
│       └── tests.rs                   # EDITED — comment lines only
│             :517-522                 #   the NOTE rewritten (FR1, FR2, FR3, FR7)
│             :523                     #   survivor assertion — unchanged (FR4)
test-docs/ring-push-blank-row-scope-test/
└── task0001.tests.yaml                # EDITED — AC-5 red_reason scalar only
      :54                              #   trailing "in this fixture" dropped (FR6)
```

### Reasoning captured by the NOTE

```
ring_push_blank(rows >= 1)
  evicted_abs   := ring_head                       (ring_buffer.rs:129, PRE-rotation)
  ...
  eviction-time clear of evicted_abs               (:146-149 | :177-180 | :196-199)
  ...
  ring_head     := ring_head + 1                   (:204, rotation)
  new_bottom_abs := (ring_head + rows - 1) % rows  (:207, POST-rotation)
                 == pre-rotation ring_head
                 == evicted_abs                    (unconditional, any rows >= 1)
  new-bottom-row clear of new_bottom_abs           (:221-224)  -> always a no-op
```

### API Design

Not applicable. No API surface is added or changed.

### Database Schema

Not applicable. No persisted data is involved.

### Dependencies

**Internal Dependencies:**

- `crates/term_core/src/ring_buffer.rs` — read-only evidence for the line
  references the NOTE cites; must remain unmodified (NFR1, NFR4).
- `feature-docs/ring-push-blank-row-scope-test/SPEC.md` FR6 and
  `feature-docs/ring-push-blank-row-scope-test/VERIFICATION.md` MT3 / MT5 — the
  reference wording the NOTE and AC-5 are brought into line with; not edited by
  this feature (FR5).
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` — the sibling
  task's completed test record whose AC-5 `red_reason` this feature corrects
  (FR6).

**External Dependencies:**

None.

### Component and commands

Component: `term_core`.

| Purpose | Command |
| --- | --- |
| Build check | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml` |
| Test | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` |
| Format check | `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` |

License: MIT.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths are derived at create-plan from every task's `files`
entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths:

- `feature-docs/ring-push-blank-note-unconditional/**`
- `test-docs/ring-push-blank-note-unconditional/**`

In addition, and per FR6, this feature declares:

- `crates/term_core/src/ring_buffer/tests.rs`
- `test-docs/ring-push-blank-row-scope-test/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and
restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is a
deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it.

## Test Scenarios

### Unit Tests

- [ ] TS1 (NFR2, NFR5, AC8): Run the `term_core` library test suite before and
      after the change and compare pass/fail/ignored counts; they must be
      identical.

### Integration Tests

Not applicable. No integrating code path is touched.

### Inspection Scenarios

- [ ] TS2 (FR1, FR3, FR7, AC1, AC3, AC4): Read the rewritten NOTE and check it
      carries all three elements: the fact (one-site removal keeps the test
      green), the unconditional reason (pre-rotation vs post-rotation evaluation
      of the same slot), and the no-op consequence.
- [ ] TS3 (FR2, FR6, AC2, AC5): Grep the rewritten NOTE and the amended AC-5
      `red_reason` for fixture-scoped qualifiers (`this fixture`, `2-row`,
      `zero-scrollback`); none must remain.
- [ ] TS4 (NFR1, NFR4, NFR6, AC7): Inspect the integrated diff: exactly two files
      changed, no hunk in `ring_buffer.rs`, no assertion or fixture line altered,
      `task0001.tests.yaml` still parses as YAML with AC-5's
      `red_confirmed: false` intact.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

No E2E infrastructure exists in this project.

### Edge Cases

Not applicable. The change executes no code path.

### Performance Tests

Not applicable.

## Security Considerations

Not applicable. The change is confined to an English comment block inside a
`#[cfg(test)]` module and to one YAML record field; it introduces no input
handling, no authentication or authorization surface, and no data flow.

## Error Handling

Not applicable. No runtime code path is added or modified.

## Performance Optimization

Not applicable. `crates/term_core/src/ring_buffer.rs` is byte-identical before
and after, so runtime behavior and cost are unchanged.

## Success Criteria

- [ ] AC1: The NOTE in `crates/term_core/src/ring_buffer/tests.rs` states that
      `new_bottom_abs == evicted_abs` holds for every `ring_push_blank` call
      regardless of row count and scrollback capacity, and names the
      evaluation-order reason (`evicted_abs` captured before the `ring_head`
      rotation; `new_bottom_abs` computed after it).
- [ ] AC2: Neither `in this 2-row, zero-scrollback fixture` nor
      `for this fixture, not independently pinned by it` — nor any other
      fixture-scoped qualifier — appears in the rewritten NOTE.
- [ ] AC3: The NOTE still states that removing only one of the two clearing sites
      leaves the test green.
- [ ] AC4: The NOTE additionally states that the new-bottom-row clear is
      consequently always a no-op within a single push, because the eviction-time
      clear has already emptied that same absolute row.
- [ ] AC5: `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` AC-5's
      `red_reason` no longer contains the phrase `in this fixture` and reads as an
      unconditional statement, while AC-5's `red_confirmed` remains false, its
      `tests` list remains empty, and the rest of the entry's wording is intact.
- [ ] AC6: All four records now agree with no conditional qualifier: the tests.rs
      NOTE, SPEC.md FR6
      (`feature-docs/ring-push-blank-row-scope-test/SPEC.md:98-102`),
      VERIFICATION.md MT3 (`:103-105`), and `task0001.tests.yaml` AC-5.
- [ ] AC7: The final diff touches only
      `crates/term_core/src/ring_buffer/tests.rs` and
      `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` (plus
      workflow-generated entries). `crates/term_core/src/ring_buffer.rs` shows no
      changed hunk, every changed Rust line lies inside the inline `#[cfg(test)]`
      module and is a comment line, and every changed YAML line lies inside AC-5's
      `red_reason` scalar.
- [ ] AC8:
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      is green with counts matching the pre-change run, and
      `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is clean.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement (FR1-FR7, NFR1-NFR6) has `status: resolved`.

## Assumptions

- **A1:** The feature deliberately does NOT delete the redundant new-bottom-row
  clear in `crates/term_core/src/ring_buffer.rs:221-224`. It documents the
  redundancy; removing it would be a behavioral change requiring its own feature.
- **A2:** The redundancy is genuinely unconditional and not fixture-specific:
  `evicted_abs = self.ring_head` is read at `ring_buffer.rs:129` before the
  rotation at `:204`, and `new_bottom_abs = (ring_head + rows - 1) % rows` at
  `:207` evaluates to the pre-rotation `ring_head` for any `rows >= 1`, so the
  eviction-time clear (one of `:146-149` / `:177-180` / `:196-199`, exactly one
  per push) and the new-bottom clear (`:221-224`) always target the same absolute
  row.
- **A3:** SPEC.md FR6 and VERIFICATION.md MT3 of the sibling feature
  `ring-push-blank-row-scope-test` already state the fact unconditionally and are
  therefore not edited by this feature; they are the reference wording the NOTE
  and AC-5 are brought into line with.
- **A4:** Because no production code and no assertion changes, the appropriate
  verification is inspection of the diff plus an unchanged-outcome test run — no
  new test is added and no mutation experiment is repeated.
- **A5:** The tests.yaml edit is a correction to a completed task's record, not a
  re-run of that task: AC-5's `red_confirmed: false` / "not test-observable"
  classification remains factually correct and is preserved.

## Design Step

Skipped. The feature has no user-visible surface: it rewrites an English comment
block inside a Rust `#[cfg(test)]` module and one YAML record field. There is no
UI, no API, no data model and no architectural choice to design, and the answered
`create-spec.design-step` gate resolved to `decide_autonomously`, accepting the
skip recommendation without asking the user.

## References

- Requirements document (Japanese):
  `feature-docs/ring-push-blank-note-unconditional/REQUIREMENTS.md`
- NOTE and survivor assertion: `crates/term_core/src/ring_buffer/tests.rs`
  (NOTE at `:517-522`, survivor assertion at `:523`)
- Evidence source (unmodified): `crates/term_core/src/ring_buffer.rs`
  (`:129`, `:146-149`, `:177-180`, `:196-199`, `:204`, `:207`, `:221-224`)
- Sibling SPEC FR6: `feature-docs/ring-push-blank-row-scope-test/SPEC.md:98-102`
- Sibling VERIFICATION MT3:
  `feature-docs/ring-push-blank-row-scope-test/VERIFICATION.md:103-105` (MT5 in
  the same document)
- Sibling test record AC-5:
  `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml:54`
