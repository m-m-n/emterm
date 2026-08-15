# Implementation Plan: relocate-wrap-ec1-scroll-test

## Overview

A test-coverage correction inside `crates/term_core`: the EC1 scroll-path
test stops claiming an overflow-cleanup property it cannot observe, a new
test genuinely pins `ring_push_blank`'s eviction-time overflow clearing, and
the two test-docs records are reconciled. No production code changes (FR8).

## Technology Stack

- **Language**: Rust — `crates/term_core` only. All Rust edits live inside
  `#[cfg(test)]` modules.
- **Test framework**: the crate's built-in unit-test harness, driven by the
  `term_core` component's test command in `workflow.yaml`. No new test
  framework, no new dev-dependency (NFR2).
- **Records**: YAML test-docs records under `test-docs/{feature}/`, following
  the existing per-task record convention.
- **New dependencies**: none. Because no dependency is introduced, the
  project license (MIT, `workflow.yaml` `project.license`) is unaffected and
  no license-compatibility decision arises for this feature.

## Layer Structure

Three layers, with a one-way dependency direction:

| Layer | Content | Status in this feature |
|-------|---------|------------------------|
| Production | every non-`#[cfg(test)]` path of `term_core` (`print_handler.rs`, `ring_buffer.rs`, `terminal_rows.rs`, …) | **Frozen** — byte-identical before and after (FR8) |
| Test | `print_handler/tests.rs`, `ring_buffer/tests.rs` | Modified — the whole code change lives here |
| Records | `test-docs/{feature}/taskNNNN.tests.yaml` | One created, one corrected in place |

Allowed direction: Records describe Test; Test observes Production.
Production never depends on either, and is never edited — a temporary
production mutation exists only inside the red-confirmation procedure (D3)
and must be reverted before the task is complete.

## Shared Components

Not applicable. This feature is a single task (D1), so no component contract
crosses a task boundary and nothing here needs a cross-task contract pin.

## Conventions

- **Test naming**: `test_<subject>_<behavior>`, matching the surrounding
  style in both test modules (NFR4). The two names this feature fixes or
  introduces are pinned by SPEC (FR1, FR4) and must be used verbatim,
  because both test-docs records reference them as strings.
- **Test placement by subject**: a test lives in the module of the unit whose
  behavior it pins. The new eviction-clearing test therefore belongs beside
  `test_ring_push_blank_clears_ridx` in `ring_buffer/tests.rs`, not in
  `print_handler/tests.rs` (SPEC assumption a3).
- **Leading comments**: every test carries a leading comment naming the AC /
  requirement IDs it covers, and that comment must claim only what the test's
  assertions can observe. A comment claiming a property the test cannot
  observe is the exact defect this feature corrects.
- **No vacuous assertions**: an assertion whose truth does not depend on the
  code under test is prohibited. Where such an assertion is removed, whatever
  it was meant to guard is either genuinely pinned elsewhere or explicitly
  recorded as unpinned.
- **Production freeze**: no non-`#[cfg(test)]` line changes. Verified by
  inspecting the final diff, not merely asserted.
- **Format**: touched Rust files satisfy the `term_core` component's format
  command (NFR5).

## Cross-task Design Decisions

### D1 — Single task

The whole change is one coherent edit inside one crate plus the two records
that describe it, and the records cannot be written until the red
confirmation (D3) has produced its observed failure message. Splitting would
put two tasks in the same two Rust test files and in interdependent record
files, buying no worktree independence while adding merge conflict surface.

The task carries eight acceptance criteria, at the upper edge of the
one-session guideline. That count reflects SPEC's fine granularity rather
than volume: three of the eight (SPEC AC-5, AC-6, AC-7) are confirmation /
records criteria rather than code work, and two more (AC-1, AC-2) are edits
to a single existing test. A split would not reduce the implementer session's
size, only fragment it.

### D2 — The relocation deletion branches are unreachable on the scroll path

**Load-bearing fact.** The deletion branches at `print_handler.rs:493` and
`print_handler.rs:518` cannot fire on the scroll path. This is structural,
not conjecture, and rests on three mechanisms (SPEC "Unreachability of the
deletion branches on the scroll path"):

1. The row key the relocated writes use is a **ring slot index**, not a
   monotonic absolute line number (`viewport_abs`, `ring_buffer.rs:75-82`) —
   so it is exactly the slot `ring_push_blank` just recycled.
2. That slot's overflow / reverse-index keys are cleared at eviction time in
   all three scrollback branches (`ring_buffer.rs:147-148`, `178-179`,
   `197-198`) and again when the new viewport bottom is blanked
   (`ring_buffer.rs:222-223`).
3. The relocated writes run **after** the line feed returns
   (`print_handler.rs:464-521`), so by the time the deletion branches are
   evaluated their non-empty guard short-circuits or the key is simply
   absent.

Consequence for this feature: a test that scrolls through `ring_push_blank`
can never observe those branches removing anything. That is *why* the EC1
assertions were vacuous, and it is the reason the rewritten EC1 comment (FR2)
must state the mechanism instead of implying an overflow check happens there.

**Out of scope**: the DECSTBM scroll-region path via `shift_rows_up` is a
distinct clearing site (`terminal_rows.rs:125-126`, `164-165`, `189-190`,
`226-227`, `256-257`) and is not addressed by this feature.

### D3 — The two clearing sites are mutually redundant; red confirmation removes both

**Load-bearing fact.** `new_bottom_abs == evicted_abs` always holds, because
the new bottom is the slot the head just rotated past. The eviction-time
clear on the scrollback-disabled branch (`ring_buffer.rs:196-199`) and the
new-bottom clear (`ring_buffer.rs:221-224`) therefore target the **same ring
slot** and are mutually redundant for a single push.

Consequences the implementer must honor:

- The red confirmation removes **both** sites in one mutation. Removing only
  one leaves the new test green — that is not a failed red confirmation, it
  is the redundancy above.
- The test-docs record states this explicitly, so a future reader does not
  read a single-site green run as evidence that the clearing is unpinned.
- The mutation is temporary. Production is restored byte-identically before
  the task completes (FR8), and the restoration is verified against the diff.

### D4 — The property is not previously unpinned

`test_ring_push_blank_clears_ridx` (`ring_buffer/tests.rs:417`) already
partially covers the same clearing behavior. The D3 mutation will therefore
very likely turn **two** tests red, not one. Neither the new test's comment
nor either record may claim that the property was previously unpinned; the
honest claim is that the new test pins the property *directly and
observably*, on a fixture with no relocation involved.

### D5 — test-docs record numbering

FR7 defers the record's task number to this phase. The assignment is:

- `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` — created by
  this feature's single task, mapping every SPEC acceptance criterion to its
  tests (or recording, with a reason, that a criterion has no test
  projection).
- `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` — the AC-6
  entry (lines 59-74) is corrected in place. Every other entry in that file
  is left byte-identical.

Both paths live outside `feature-docs/`, and both are owned by the task, not
by this planning phase.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The temporary red-confirmation mutation leaks into the final diff, breaking the production freeze (FR8) | medium | high | The task's acceptance criteria require an explicit final diff inspection showing no non-`#[cfg(test)]` line changed; the record's red evidence is captured as text, never as a retained code change |
| The single-site removal is read as "red confirmation failed" and the implementer weakens the test until it fails | low | high | D3 states the redundancy up front and requires the record to carry it; the criterion is a both-sites removal, not "any removal fails" |
| The record overclaims that the clearing was previously unpinned | medium | medium | D4 forbids the claim and names the pre-existing partial cover; the record must mention the two-tests-red expectation |
| TS1 is edited while cleaning the neighboring EC1 test (both live in the same file, minutes apart) | medium | high | TS1 is frozen by an explicit acceptance criterion, and the surrounding tests are named in the task's Out of Scope section |
| The stale-record correction touches entries other than AC-6 | low | medium | The criterion requires every other entry of that file to stay byte-identical |
| The new fixture fails to produce overflow-bound cells (content under the inline cap), making the new test vacuous in the same way as the one being fixed | medium | high | The fixture mirrors the existing shape at `print_handler/tests.rs:1457-1472`, and the test pre-asserts the entries are present before scrolling — a vacuous fixture fails the pre-assertion |

## Open Questions

- [ ] None. Every SPEC requirement is `resolved`; no `tbd` requirement, no new
      dependency and therefore no license decision, and no pre-existing
      IMPLEMENTATION.md / task plan to reconcile.
