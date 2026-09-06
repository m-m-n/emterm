# Implementation Plan: ring-push-blank-note-scope

## Overview

Correct the scope of the redundancy NOTE attached to
`test_ring_push_blank_clears_recycled_row_overflow_entries` in the `term_core`
ring-buffer test module, so that "always a no-op" is attributed only to Step 3's
overflow clear pair while the cell fill and the wrapped-flag reset are recorded
as required work. The change is comment text only, in one file.

## Technology Stack

- **Language**: Rust — the existing `term_core` crate; toolchain, edition and
  crate manifest are untouched.
- **Framework**: none. The edit lives inside an existing test module of that
  crate; no new module, no new test harness.
- **Key libraries**: none.
- **New dependencies**: none. Because no dependency is added, no license
  compatibility check applies and `project.license` (MIT) is unaffected.

## Layer Structure

A single layer is in play: the `term_core` crate's ring-buffer test module.

| Element | Role in this feature | Modified |
|---|---|---|
| `crates/term_core/src/ring_buffer/tests.rs` | Holds the NOTE being corrected | Yes — comment text only |
| `crates/term_core/src/ring_buffer.rs` | The production function the NOTE describes; read as the reference for the corrected attribution | No |

Dependency direction is one-way and read-only: the test comment describes the
production source; nothing in the production source is adjusted to suit the
comment.

## Shared Components

None. The feature consists of a single task, so no component crosses a task
boundary and no cross-task contract needs to be pinned here. Every file this
feature touches has exactly one owning task.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

## Conventions

- **Comment language and register**: English, declarative, matching the
  surrounding comments in the same test module. Phases are referred to as
  Step 1 / Step 2 / Step 3 — the names the production function itself uses.
- **Wrap width**: new lines follow the surrounding NOTE's wrap width
  (approximately 72 columns inside the indented comment block).
- **Formatting gate**: the crate's format check must remain clean; no reflow of
  comment lines outside the NOTE being corrected.
- **Behavioural neutrality**: assertions, fixtures, test names, the test count
  and the production source are all left exactly as they are. The observable
  behaviour of the crate before and after the change is identical.
- **Locating the target**: the NOTE is located by the enclosing test's name, not
  by line number. Line numbers quoted in SPEC.md and REQUIREMENTS.md are a
  snapshot and may have drifted.

## Cross-task Design Decisions

### D1: One task, no decomposition

The entire change is one contiguous comment block in one file. Splitting it
would give two owners of the same paragraph and create a merge conflict with no
upside. Affected tasks: task0001.

### D2: The attribution model the corrected NOTE must express

This is the factual model the revised text has to reflect. It is recorded here
so the implementer does not have to re-derive it, and so the review phase can
check the result against a stated model rather than against an opinion.

1. Step 1 (eviction) clears, for the evicted absolute row, only the overflow and
   overflow-row-index entries. This holds in all three of its branches
   (scrollback bypassed, scrollback with capacity, scrollback disabled).
2. Step 2 rotates the ring head.
3. Step 3 derives the new bottom absolute row and performs three actions:
   clearing the overflow pair for that row, filling that row's cells, and
   resetting that row's `ring_wrapped` flag.
4. Whenever at least one row is pushed, the new bottom absolute row is the same
   ring slot as the evicted row. This premise is correct and is retained.
5. Of Step 3's three actions, only the overflow clear pair has a counterpart on
   the eviction side. The NOTE may therefore call only that pair redundant. The
   cell fill and the `ring_wrapped` reset have no eviction-side counterpart and
   are required work.

Affected tasks: task0001.

### D3: No new test is added

The task's expected behaviour is a comment correction. Even though the enclosing
test would stay green if the whole Step 3 block were deleted, this feature
deliberately does not add a test pinning the cell fill or the `ring_wrapped`
reset — that would exceed the declared scope and change the test file beyond
comment text. Affected tasks: task0001.

### D4: Content requirements, not wording, are specified

The plan states what the corrected NOTE must assert (D2) and leaves the exact
sentences to the implementer, subject to the register and wrap-width conventions
above. Verification of the wording is a read-through against the production
source (VERIFICATION.md TS-3), not a string match.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The rewrite overshoots and replaces the still-correct reasoning instead of narrowing only the conclusion | Medium | Low | FR3 keeps the existing reasoning as-is; TS-3 reads the result against the production source and TS-4 confirms the diff is comment-only |
| Stale line numbers in the source documents send the implementer to the wrong block | Medium | Low | Conventions above: locate by the enclosing test name, never by line number |
| New comment lines break the crate's format check or the surrounding wrap width | Low | Low | NFR1; TS-2 runs the crate's format check |
| The corrected NOTE still leaves "delete Step 3 wholesale" readable as licensed | Low | Medium | D2 item 5 states the attribution explicitly; TS-3 checks each Step 3 action is attributed |

## Open Questions

- [ ] None. FR1-FR4 and NFR1-NFR3 are all `ok` in workflow.yaml — no TBD
      requirement, no new dependency, and therefore no license decision.
