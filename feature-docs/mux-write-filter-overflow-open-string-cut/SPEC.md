# Feature: mux-write-filter-overflow-open-string-cut

Requirements document: `feature-docs/mux-write-filter-overflow-open-string-cut/REQUIREMENTS.md`

## Overview

The scrollback write filter's written end state distinguishes an open OSC
string body and an open ST-terminated string body (DCS / APC) from ground.
When an overflow flush leaves such a body open in the ring, a later cut writes
the closure `ESC` + CAN, so a snapshot replay matches what the client saw.

## Objectives

- When the scrollback write filter's overflow flush leaves an OSC / DCS / APC
  string body open in the ring, a later cut (removed 47 / 1047 / 1049 screen
  switch, or the reader's fallback closing) closes that string in the ring, so
  a snapshot replay matches what the client saw.
- Main-buffer text written after such a cut is displayed on replay and is
  never absorbed into the open string body; a later BEL never completes the
  string with that text (no title set from it).
- Resolve review finding ed366a655164f747 (medium) left open by
  mux-write-filter-overflow-lone-esc, and the pre-existing same-cause case of
  an overflow run that ends in plain body bytes followed by a cut.

## User Stories

### US1: Replay after a cut that follows an overflowed string
As a mux client, I want main-buffer text written after a screen switch that
follows an overflowed OSC / DCS / APC string to be displayed on snapshot
replay, so that the replay matches what I saw live.

**Acceptance Criteria:**
- [ ] AC-1 (FR2, FR6): Production reader (`run_reader_without_owner`): reads
  grow an `ESC]0;` OSC past the cap, the read that crosses the cap ends in
  `ESC`, the next read begins with a screen switch (`ESC[?1049h` ...
  `ESC[?1049l`) followed by main-buffer plain text. term_core replaying the
  pane's ring displays that text, and its view (rows, cursor, responses)
  equals the reference (term_core fed the raw stream).
- [ ] AC-2 (FR2, FR4, FR5): Held-ESC open-body ending (held OSC at the cap,
  call 1 = body bytes + `ESC`): the reader's fallback closing writes exactly
  `ESC` + CAN, leaves `pending` empty, the written state ground and no
  designator awaited; a second fallback closing writes nothing; for each
  47 / 1047 / 1049 pair the replay of the written bytes plus a later
  plain-text call equals view_after_a_cut of the raw stream.
- [ ] AC-3 (FR2): Same ending, then a call carrying a cut at fed 0 followed by
  plain text: the call writes `ESC` + CAN then the text; `pending` empty,
  state ground; the replay equals view_after_a_cut for each switch pair.
- [ ] AC-4 (FR2, FR3): An overflow run that ends in plain OSC body bytes (no
  trailing `ESC`), then a cut (next call at fed 0, and the fallback closing):
  the closure is `ESC` + CAN and the replay with later plain text equals
  view_after_a_cut.
- [ ] AC-5 (FR2, FR5): An overflow flush followed by a cut in the same call
  while the run ends inside an open body: the written bytes are the strip of
  the run followed by `ESC` + CAN, nothing is held, state ground.
- [ ] AC-6 (FR1, FR2): DCS and APC bodies (a non-strip-target DCS / APC and a
  Kitty APC / SIXEL DCS left open past the cap): a cut writes `ESC` + CAN and
  the replay equals view_after_a_cut; a BEL inside a DCS / APC body does not
  close it (a later cut still writes `ESC` + CAN).
- [ ] AC-7 (FR3): After an overflow leaves an open OSC body, a later call that
  writes BEL (or `ESC \`) returns the written state to ground and a following
  cut writes nothing; a later call of plain bytes keeps the open-body state.

### US2: Existing behavior and documentation stay consistent
As a mux client, I want every path that never leaves an open string body
written to keep its output unchanged, and the code documentation to state the
new model.

**Acceptance Criteria:**
- [ ] AC-8 (FR4, FR6): The predecessor's open-body ending is updated: after
  call 1 exactly one `ESC` is held, the written state is the open-body state,
  and the fallback closing writes `ESC` + CAN; the `\` continuation is still
  written as `ESC \` and leaves ground; every other overflow_lone_esc test
  passes unchanged.
- [ ] AC-9 (NFR1, NFR2, NFR3, NFR4): The full `--lib` suite passes (no other
  existing expectation changed), and the `--no-default-features` cargo check
  succeeds.
- [ ] AC-10 (FR7): The doc comments listed in FR7 state the open-body state and
  the `ESC` + CAN closure; none states that a string body counts as ground.

## Technical Requirements

### Functional Requirements
- **FR1:** Written state distinguishes an open string body. The end state of
  the written stream that the write filter carries (and that the
  state-reporting strip reports) distinguishes, from ground, the state of
  being inside an open OSC string body and inside an open ST-terminated string
  body (DCS `ESC P`, APC `ESC _`), following term_core's transitions:
  `ESC ]` enters the OSC body; `ESC P` / `ESC _` enter the ST-terminated body;
  inside the OSC body BEL returns to ground; inside either body an `ESC` leaves
  the stream in the existing Escape state (a following `\` completes ST and
  returns to ground; any other byte aborts the string and is processed as from
  the Escape state); every other byte keeps the body state. Bytes the strip
  removes advance nothing (unchanged). The representation (new WrittenState
  variants or a separate O(1) flag) is a plan-time decision.
- **FR2:** Closure of an open string body at a cut. When the written stream
  ends inside an open string body (FR1), a cut writes the closure `ESC` + CAN
  (0x1b 0x18), which term_core takes as an aborted (Unterminated) string
  followed by a completed escape, returning to ground. This applies on every
  cut path: a cut inside the call (state of the run before the cut, after the
  strip), a cut at an empty segment and the reader's fallback closing (empty
  fed range with a cut at 0; the carried state), a cut that follows an
  overflow flush in the same call (state of the flushed run), and a cut or
  fallback closing that drops the held live lone `ESC` that an overflow flush
  left after an open body (state of the bytes written before that `ESC`).
  After the closure the written state is ground and no designator is awaited.
- **FR3:** Open-body state carried across calls. The open-body state set by an
  overflow flush is carried into later calls and is advanced by the bytes
  those calls write: a written BEL (OSC body only), a written `ESC \` or a
  written `ESC` followed by another byte closes it per FR1; written plain bytes
  keep it. A cut that follows a closed body writes the closure of the state the
  stream is then in (nothing in ground).
- **FR4:** Overflow hold decision unchanged. The overflow flush's decision to
  hold a run's final live lone `ESC` is unchanged: an `ESC` that ends a run
  inside an open string body still leaves the stream in Escape and is held as a
  chain of one construct. The written state stored with it is the open-body
  state (no longer ground). What the next call does with the held `ESC` without
  a cut (strip together with its continuation, `ESC \` written as ST,
  non-strip continuation written whole) is unchanged.
- **FR5:** One closure per cut, never a strip target. A cut still writes
  exactly one closure, chosen by the written end state alone (DEL in a CSI,
  CAN after an `ESC`, the designator `ESC` in a designator wait, `ESC` + CAN in
  an open string body, nothing in ground). The closure is written after the
  strip, is attributed to the outcome's dims, never starts a strip target for
  the write-path or snapshot-time strip, and does not form ST. A second
  fallback closing right after the first writes nothing.
- **FR6:** Regression tests and updated predecessor expectations. Tests detect
  the regression: view_after_a_cut comparisons for the 'inside an open string
  body' ending (held `ESC` dropped by the fallback closing and by a cut at
  fed 0), for an overflow run ending in plain body bytes followed by a cut,
  and a production-reader reproduction. The predecessor tests that pin the old
  behavior for the open-body ending (the `endings()` entry 'inside an open
  string body': `state_before_esc` Ground and `closing` empty) are updated to
  the new state and to `ESC` + CAN.
- **FR7:** Doc comments follow the new model. Doc comments that state the old
  model are updated: the WrittenState doc in scrollback_filter.rs ('an OSC /
  DCS / APC body needs no state of its own'), the escape-state comment in
  WrittenState::advance ('a string introducer enters a body that, for this
  question, is ground'), closure_for's and ESCAPE_CLOSING's docs ('a cut
  writes at most one of the two'), and feed_with_cuts' 'Overflow' / 'Closure
  at a cut' paragraphs and the overflow-branch comments ('a string body counts
  as ground') in write_filter.rs.

### Non-Functional Requirements
- **NFR1 - Performance:** Extra state is O(1) and the strip stays a single
  pass; the overflow branch strips each flushed run once.
- **NFR2 - Strip output stability:** The strip's removal decisions and
  stripped bytes are unchanged for the same input, whatever state is carried
  in; only the reported state and the closure at a cut change.
- **NFR3 - Non-overflow output stability:** Bytes written by paths that never
  leave an open string body written (every non-overflow path) are unchanged:
  the full `--lib` suite passes with only the predecessor's open-body
  expectations (FR6) changed.
- **NFR4 - CLI build:** `CARGO_TARGET_DIR=src-tauri/target cargo check
  --manifest-path src-tauri/Cargo.toml --no-default-features` succeeds (the mux
  is part of the CLI build).
- **NFR5 - Unchanged surroundings:** No change to the cap value, the overflow
  warning log, live forwarding to the client, the snapshot-time strip, or the
  reader loop's cut derivation.

## Implementation Approach

### Architecture

**Components touched:**
```
write_filter.rs        feed_with_cuts: overflow branch, closure at a cut,
                       carried written state, held lone ESC
scrollback_filter.rs   WrittenState / WrittenState::advance, closure_for,
                       ESCAPE_CLOSING, state-reporting strip
tests                  overflow_lone_esc.rs (predecessor expectations, FR6),
                       new regression tests (FR6)
```

The representation of the open-body state (new WrittenState variants or a
separate O(1) flag) is decided at plan time (FR1).

### Written-state transitions for string bodies (FR1)

| From | Byte(s) | To |
|---|---|---|
| — | `ESC ]` | OSC body |
| — | `ESC P` / `ESC _` | ST-terminated body |
| OSC body | BEL | ground |
| OSC body / ST-terminated body | `ESC` | existing Escape state |
| Escape (after a body's `ESC`) | `\` | ground (ST completed) |
| Escape (after a body's `ESC`) | any other byte | string aborted; byte processed as from the Escape state |
| OSC body / ST-terminated body | any other byte | body state kept |

A BEL inside an ST-terminated body keeps the body state (AC-6). Bytes the strip
removes advance nothing.

### Closure at a cut (FR2, FR5)

| Written end state | Closure |
|---|---|
| in a CSI | DEL |
| after an `ESC` | CAN |
| designator wait | the designator `ESC` |
| open string body | `ESC` + CAN (0x1b 0x18) |
| ground | nothing |

| Cut path | State the closure is chosen from |
|---|---|
| cut inside the call | the run before the cut, after the strip |
| cut at an empty segment; reader's fallback closing (empty fed range, cut at 0) | the carried state |
| cut following an overflow flush in the same call | the flushed run |
| cut / fallback closing dropping the held live lone `ESC` left after an open body | the bytes written before that `ESC` |

After the closure the written state is ground and no designator is awaited.

### Data Flow

```
PTY read → reader loop (cut derivation, unchanged)
         → write filter feed_with_cuts (strip, overflow flush, closure at cut)
         → pane ring → snapshot replay (term_core)
PTY read → live forwarding to the client (unchanged)
```

### API Design

N/A

### Database Schema

N/A

### Dependencies

**Internal Dependencies:**
- term_core: its string / escape transitions are the model FR1 follows, and it
  is the replay engine the tests compare against (view_after_a_cut).
- scrollback strip (scan_boundary / find_st): its removal decisions stay
  unchanged (NFR2).

**External Dependencies:**
- None

### File Structure

Feature-specific files are derived at create-plan from the tasks' `files`
entries (see Declared Change Set). FR6 / FR7 name write_filter.rs,
scrollback_filter.rs and overflow_lone_esc.rs.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-write-filter-overflow-open-string-cut/**`
- `test-docs/mux-write-filter-overflow-open-string-cut/**`

`feature-docs/mux-write-filter-overflow-open-string-cut/**` covers
`REQUIREMENTS.md`, `SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`,
`phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`,
`retrospect.yaml`, and the design artifacts the design step produces. These
are generated and owned by the phase documents and by
`references/phase-state.md`; this section cites them and restates none of
their rules.

`test-docs/mux-write-filter-overflow-open-string-cut/**` covers
`test-docs/mux-write-filter-overflow-open-string-cut/{T}.tests.yaml`, the
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

## Test Scenarios

### Unit Tests
- [ ] TS-2 (AC-2, AC-3): held-ESC open-body ending with the fallback closing
  and with a cut at fed 0, per 47 / 1047 / 1049 pair, view_after_a_cut oracle.
- [ ] TS-3 (AC-4, AC-5): plain-body overflow ending followed by a cut in the
  next call, by the fallback closing, and by a cut in the same call.
- [ ] TS-4 (AC-6): DCS / APC bodies, including BEL inside a DCS / APC body.
- [ ] TS-5 (AC-7): state carry across calls (BEL / `ESC \` closes, plain bytes
  keep).
- [ ] TS-6 (AC-8): updated predecessor expectations in overflow_lone_esc.rs.

### Integration Tests
- [ ] TS-1 (AC-1): production-reader reproduction through
  run_reader_without_owner; ring replay vs raw-stream reference.

### Build and Suite
- [ ] TS-7 (AC-9): full `--lib` suite and `--no-default-features` cargo check.

### Documentation
- [ ] TS-8 (AC-10): read-through of the doc comments.

**Oracle note:** the term_core end-state oracle `client_written_state`
(scrollback_filter/tests.rs) classifies an open OSC body as Escape today (both
`m` and `[m` are absorbed); if the new states are asserted through it, it needs
probes that separate them (e.g. `\m` shows from Escape but not from a body;
BEL `m` shows from an OSC body but not from a DCS / APC body).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression (none detected)

### Edge Cases
- [ ] BEL inside a DCS / APC body does not close it (AC-6).
- [ ] A second fallback closing right after the first writes nothing (AC-2).
- [ ] An overflow run ending in plain body bytes with no trailing `ESC` (AC-4).
- [ ] An overflow flush and a cut in the same call (AC-5).

### Performance Tests
- N/A (NFR1 is a structural constraint: O(1) extra state, single-pass strip)

## Security Considerations

N/A

## Error Handling

N/A

## Performance Optimization

### Performance Goals
- NFR1: extra state is O(1); the strip stays a single pass; the overflow branch
  strips each flushed run once.

## Assumptions

- **A1** (reversible): `ESC` + CAN is the closure for an open string body, as
  proposed by the task and review finding ed366a655164f747. term_core's
  osc_escape dispatches the OSC as Unterminated on any byte other than `\` and
  reprocesses it as an escape; CAN then completes the escape without effect
  (ESCAPE_CLOSING's documented behavior). The client saw the same Unterminated
  dispatch at the switch's `ESC`.
- **A2** (reversible): DCS / APC bodies in term_core end only at `ESC` (ST or
  abort) and ignore BEL, and SOS / PM (`ESC X`, `ESC ^`) are not strings. This
  follows scan_boundary / find_st, which are documented to mirror term_core's
  dcs / apc handlers; dcs.rs / apc.rs were not among the supplied inputs and
  are confirmed at plan time.
- **A3** (reversible): Only an overflow flush can leave an open string body
  written: every non-overflow path holds an incomplete string in `pending`
  instead of writing it. So the new state changes no non-overflow output
  (NFR3).
- **A4** (reversible): Out of scope: a construct the strip removes (no cut)
  whose `ESC` aborted an open written body on the client side, e.g. an open
  body then `abc ESC[6n def` in a later call. The ring then keeps the body open
  and `def` is absorbed on replay. This is the same class as the accepted
  open-CSI-plus-removed-construct case, and fixing it would change strip output
  (NFR2).
- **A5** (reversible): The predecessor's AC-5 / TS-4 expectation 'nothing
  after the open-string-body ending' and SPEC FR4 / NFR2 prose are superseded
  by this feature. If an existing test listed in
  `test-docs/mux-write-filter-overflow-lone-esc/task0001.tests.yaml` is
  renamed, that record is updated and a supersede note naming this SPEC's
  FR2 / FR6 is added (`.claude/rules/test-docs-records.md`). If names are kept,
  the record is unchanged. The predecessor's feature-docs prose stays as
  written.
- **A6** (reversible): The task text explains the root cause by referring to
  the 'written-state model'; that is context and is not copied as a
  justification into the SPEC.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals (NFR1)
- [ ] Documentation is complete (FR7)
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None (no requirement has `status: tbd`)

## References

- Requirements: `feature-docs/mux-write-filter-overflow-open-string-cut/REQUIREMENTS.md`
- Predecessor feature: `feature-docs/mux-write-filter-overflow-lone-esc/`
- Test-docs record rule: `.claude/rules/test-docs-records.md`
