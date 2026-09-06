# Implementation Plan: ac4-ac6-stale-line-reference

## Overview

Two prose passages in the completed test record
`test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` assert in the
present tense that line 523 of `crates/term_core/src/ring_buffer/tests.rs`
holds the survivor-row assertion; that is no longer true. This feature
rewrites those two passages so the assertion is identifiable after future line
drift, while leaving the record's verbatim evidence and its YAML structure
untouched.

## Technology Stack

- **Language / Framework**: none. The change set is one YAML document edited as
  text; no compiled code is involved (FR7).
- **Key libraries**: none added. This feature introduces **no new dependency**,
  so the license-compatibility check has nothing to evaluate. Project license
  is `MIT`; new dependencies introduced by this plan: none.
- **Verification tooling**: whatever YAML parser and diff tool the environment
  already provides (see VERIFICATION.md); nothing is installed for this
  feature.

## Layer Structure

There is no runtime layering. The one structural boundary that matters is the
writable / read-only split, and it is a hard constraint (FR7):

| Path | Role | Allowed operation |
|------|------|-------------------|
| `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` | the record being corrected | read + write |
| `crates/term_core/src/ring_buffer/tests.rs` | source of the assertion expression and its current line number | read only |

Nothing in this feature may depend on, or cause, a change under `crates/`.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | This feature decomposes into a single task; no component contract crosses a task boundary. | — | — |

## Conventions

- **Naming for the two assertion sites.** The record and the plan use exactly
  two names: *post-scroll survival assertion* (the one the AC-4 / AC-6
  mutation makes fail) and *pre-scroll anti-vacuity guard* (the textually
  identical earlier one). Any prose that identifies an assertion by its
  expression carries one of these two qualifiers.
- **Naming collision warning.** The record's own acceptance criteria are named
  AC-1..AC-7, and the task plan's acceptance criteria are also named AC-n.
  Wherever the two could be confused, the record's entries are written as
  "the record's AC-4 entry" / "the record's AC-6 entry".
- **Tense policy for line numbers.** A line number of `tests.rs` is written in
  the past tense, explicitly bound to the run it came from, unless it was
  re-derived from the working tree at edit time (see D3).
- **Error-handling policy.** Not applicable: the change introduces no runtime
  error path. The single failure mode is an edit that escapes its intended
  region; the handling is to revert and re-confine the edit (SPEC Error Flow).

## Cross-task Design Decisions

### D1: Hybrid anchoring, not either/or

The correction keeps 523 in the text **and** identifies the assertion by its
expression. Rationale: NFR2 requires the historical failure location to
survive as evidence, and NFR1 requires the phrasing to withstand the next line
shift; a bare relabelled 523 satisfies only the first, a bare expression
reference only the second. Any current line number cited alongside them is
supporting detail, never the sole anchor.
Affected: task0001; VERIFICATION.md TS-3, TS-4, TS-5.

### D2: The protected transcript is identified by content, not by line position

FR3 names the transcript as "lines 69-75". Those numbers describe the
pre-change file. The record's AC-4 entry sits earlier in the file, so an edit
that changes AC-4's line count shifts the transcript's position while leaving
its bytes untouched. The binding requirement is therefore stated as: **the
transcript block's lines, and their order, are unchanged byte-for-byte**; its
absolute line numbers may shift. Keeping the record's AC-4 edit line-count
neutral (so the block also stays at 69-75 literally) is preferred but not
required, and must never be bought by degrading the corrected prose.
Rationale: a position-based reading would forbid an FR2-compliant edit that
legitimately needs one more line, which contradicts FR2. Verification is
content-based accordingly.
Affected: task0001 (record AC-4 / AC-6 edits); VERIFICATION.md TS-2.

### D3: Every current line number is re-derived at edit time

Any line number presented as current is read out of the working tree's
`crates/term_core/src/ring_buffer/tests.rs` during the edit, never copied from
the task description, the SPEC, or this document. FR5 mandates this, and the
feature's own history shows why: the originating bug report states the
assertion moved to line 531, the SPEC states 606, and the working tree at
planning time holds it at 606 — a number carried forward without re-derivation
is how this bug was created in the first place. The pre-scroll anti-vacuity
guard is the earlier of the two identical occurrences.
Affected: task0001; VERIFICATION.md TS-5.

### D4: Checks read the parsed scalar, not the raw lines

Both target passages are YAML folded scalars, so a sentence in the record is
wrapped across physical lines: the phrase FR1 targets is split across two
lines in the file and matches no single-line search. Any presence/absence
check on prose therefore runs against the **parsed value** of the record's
AC-4 / AC-6 `red_reason` (where folding has joined the wrapped lines), while
the byte-identity check of D2 runs against the **raw file**. Mixing the two up
produces a check that passes vacuously.
Affected: task0001 Test Notes; VERIFICATION.md TS-1, TS-3, TS-4.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A check greps raw lines for a sentence that folding wrapped, silently passes, and the stale prose survives | High | Medium | D4: prose checks run on the parsed scalar value |
| Re-indenting or reflowing a folded scalar spills the edit into the transcript block | Medium | High | D2 content-identity check; edit confined to the two `red_reason` scalars; 6-space continuation indentation preserved (FR6, NFR3) |
| A new current line number is copied from a document instead of re-derived, re-creating this very bug | Medium | High | D3; TS-5 re-reads the file at verification time |
| The expression-based reference lands on the pre-scroll guard, pointing readers at the wrong site | Medium | Medium | Mandatory post-scroll survival qualifier (FR4); TS-5 confirms both sites |
| Deleting 523 outright to "fix" the sentence destroys the historical evidence | Low | High | NFR2 is an explicit acceptance criterion; TS-4 requires 523 to remain, qualified |

## Open Questions

- [ ] D2 interprets FR3's "lines 69-75" as a content region rather than fixed
      line positions. If the intent was literally that the transcript stay at
      lines 69-75, the record's AC-4 edit must additionally be line-count
      neutral; raise this in review if the stricter reading is wanted.
- [ ] TS-1 needs some YAML parser present in the verification environment. No
      dependency is added for it (FR7), so the verifier uses whatever the
      environment already provides; if none is available, TS-1 degrades to a
      structural inspection of the diff and should be reported as such.
