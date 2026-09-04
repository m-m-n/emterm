# Implementation Plan: ring-push-blank-note-unconditional

## Overview

Correct one explanatory NOTE in the `term_core` ring-buffer test module so it
states the *unconditional* reason the two clearing sites inside
`ring_push_blank` are mutually redundant, and amend one field of a sibling
feature's completed test record so all four records agree. The change is
text only: no production code, no assertion, no fixture, no test name.

## Technology Stack

- **Language**: Rust (crate `term_core`) — the edited lines are line comments
  inside an inline `#[cfg(test)]` module.
- **Record format**: YAML — one folded block scalar inside an existing
  per-task test record.
- **New dependencies**: none. No crate, tool or library is added, so no new
  license enters the project. `project.license` stays `MIT`; the license
  review perspective has no new dependency line to cross-check.

## Layer Structure

No layer is introduced or altered. The relevant structure for this feature is
a read/write boundary rather than a dependency direction:

| File | Role | Access |
|---|---|---|
| `crates/term_core/src/ring_buffer.rs` | Evidence source: the evaluation order the NOTE describes | READ ONLY — must stay byte-identical (NFR1, NFR4) |
| `crates/term_core/src/ring_buffer/tests.rs` | Carries the NOTE | WRITE — comment lines only |
| `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` | Sibling feature's completed test record | WRITE — AC-5 `red_reason` scalar only |
| `feature-docs/ring-push-blank-row-scope-test/SPEC.md`, `.../VERIFICATION.md` | Reference wording already stated unconditionally | READ ONLY — not edited (A3) |

Crossing that boundary in the write direction — editing `ring_buffer.rs`, or
editing any part of the sibling record other than AC-5's `red_reason` — is a
requirement violation, not a judgment call.

## Shared Components

The one artifact shared across the edited files (and with the two read-only
sibling records they are brought into line with) is the **canonical claim
set**: the four claims that together make the redundancy statement
unconditional. Every record below must be readable as asserting the claims
listed for it, with no fixture-scoped qualifier anywhere.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Canonical claim set C1–C4 | Single wording reference the NOTE and the amended record are both measured against | Precondition: the claims are stated for `ring_push_blank` in general, never for one fixture. Postcondition: the tests.rs NOTE expresses C1, C2, C3 and C4; the sibling record's AC-5 `red_reason` expresses C1 and C2 (its existing sentence, with the fixture qualifier removed) and asserts nothing that contradicts C3 or C4 | task0001 |

**C1 — the fact (preserved from the current NOTE, FR3).** Removing only ONE
of the two clearing sites inside `ring_push_blank` — the eviction-time clear
or the new-bottom-row clear — still leaves
`test_ring_push_blank_clears_recycled_row_overflow_entries` green.

**C2 — the scope (FR1).** The new bottom absolute row equals the evicted
absolute row on EVERY `ring_push_blank` call with at least one row. It does
not depend on the viewport height and it does not depend on the scrollback
capacity, so no fixture can pin the two sites independently.

**C3 — the reason (FR1).** The evicted absolute row is read from the ring
head BEFORE the rotation, while the new bottom absolute row is computed from
the ring head AFTER it has advanced by one; modulo the row count the two
expressions therefore denote the same ring slot.

**C4 — the consequence (FR7).** The new-bottom-row clear is therefore always
a no-op within a single push: whichever eviction-time clear branch ran has
already emptied that same absolute row.

## Conventions

- **Language and form**: the NOTE stays an English `// ` line-comment block at
  its current position inside the inline `#[cfg(test)]` module, immediately
  above the survivor assertions. It is never promoted to a doc comment, an
  assertion, or a separate document (FR4).
- **Wrap width**: match the wrap width already used by the surrounding
  comment block in that file. `rustfmt` does not reflow line comments, so the
  format check passes either way; the requirement is visual consistency with
  the neighbouring blocks (NFR3, NFR5).
- **Code references inside the NOTE**: identify the two clearing sites and the
  two absolute-row expressions by their identifier names and by the `Step`
  labels the production function's own comments already use — see decision D3
  for why line numbers are excluded.
- **Record edits**: the sibling record is amended by the smallest edit that
  achieves the requirement (removing the fixture qualifier), never by
  re-authoring the entry. Key set, block-scalar style, indentation,
  `red_confirmed` and `tests` all stay as written (FR6, NFR6).

## Cross-task Design Decisions

### D1 — One task, not two

The two edits are a single coherent statement split across two files: the
record amendment exists only to stop the record contradicting the NOTE. Both
files are small, the combined change is a handful of comment lines plus one
scalar, and the wording of both must be measured against the same claim set.
Splitting them into parallel tasks would duplicate the claim-set reasoning and
add a second worktree and merge for a one-line YAML edit. Affected: task0001.

### D2 — Document the redundancy, do not remove it

The new-bottom-row clear is genuinely dead within a single push (C4), which
invites deleting it. Deleting it is a production-code behavior change,
explicitly out of scope, and would require its own feature with its own
regression argument (the clear also guards the state a *later* push starts
from, which this feature makes no claim about). The NOTE records the
redundancy; `ring_buffer.rs` stays byte-identical. Affected: task0001 (NFR4,
A1).

### D3 — The NOTE cites identifiers and step labels, not line numbers

The SPEC's requirement text pins the evidence with `path:line` references so
the requirement is checkable today. The NOTE itself instead names the two
expressions and the production function's own `Step` labels. Rationale: line
numbers inside the same crate go stale silently on the first unrelated edit
above them, which is precisely the class of documentation drift this feature
exists to remove — encoding fresh drift into the fix would be
self-defeating. The success criteria (AC1, AC4) require the reason and the
consequence, not the line numbers, so this satisfies them as written.
Affected: task0001 (FR1, FR7).

### D4 — The sibling feature's SPEC and VERIFICATION are the reference, not a target

`feature-docs/ring-push-blank-row-scope-test/SPEC.md` FR6 and
`VERIFICATION.md` MT3 already state the fact unconditionally. They are read
to calibrate wording and are never edited by this feature; MT3 must still be
satisfiable by the rewritten NOTE, and the separate explanatory block above
the test — the one MT5 checks — must not be disturbed. Affected: task0001
(FR5, A3).

### D5 — Verification is inspection plus an unchanged-outcome run

No code path changes, so there is nothing new to assert. The evidence is (a)
reading the rewritten text against the claim set, (b) grepping for the
forbidden qualifiers, (c) inspecting the diff for containment, and (d)
re-running the existing suite and the format check and showing the outcome is
identical. No test is added, and the mutation experiment from the sibling
feature is not repeated. Affected: task0001 (A4, NFR2).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A longer NOTE shifts the survivor assertion's line number, making the historical line citations in the sibling record's AC-4 / AC-6 evidence point at the wrong line | High | Low | Keep the rewritten NOTE close to the current block's length. The historical entries are evidence of a past run and are deliberately NOT renumbered — NFR1 confines this feature's YAML edit to AC-5's `red_reason`. Note the drift in the completion report instead of editing more. |
| The edit lands in the wrong `task0001.tests.yaml` — this feature's own generated record has the same file name as the sibling record being amended | Medium | High | The task plan names both paths explicitly and marks which one is the edit target; the diff-containment criterion catches a wrong-file edit. |
| The rewritten NOTE drops the C1 fact while adding the new reason | Low | Medium | C1 is a separate acceptance criterion, verified independently of the reason and the consequence. |
| `ring_buffer.rs` gets "tidied" while being read as evidence | Low | High | It is declared read-only in the layer table, excluded from the task's file set, and a no-hunk check is an acceptance criterion. |
| A reviewer reads FR1's parenthetical line references as mandatory NOTE content | Medium | Low | D3 records the rationale so the trade-off is reviewable rather than looking like an omission. |

## Open Questions

- [ ] D3 (line references excluded from the NOTE text) is a planner judgment
      against a literal reading of FR1's parentheses. Cheap to overturn in
      review if the literal reading is preferred.
- [ ] The historical line citations in the sibling record's AC-4 / AC-6
      evidence will point one or more lines off after this change. Left
      as-is under NFR1; flagged here so a later feature can decide whether
      stale historical citations are worth a follow-up.
