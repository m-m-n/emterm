# Implementation Plan: mux-write-filter-overflow-open-string-cut

## Overview

The written end state of the scrollback write filter gains two open-string-body
states (an open OSC body and an open ST-terminated DCS / APC body), and a cut
that finds the written stream in one of them writes the closure `ESC` + CAN.
The whole change is one task (task0001): the state model, the closure, the
updated predecessor expectations and the new regression tests only pass
together, so no split yields tasks that pass independently in their own
worktrees.

## Technology Stack

- **Language**: Rust, `src-tauri` crate, `mux` module (built by both the GUI
  build and the `--no-default-features` CLI build).
- **Replay engine / test oracle**: `term_core` (workspace crate), unchanged.
- **New dependencies**: none. No license entry to record against
  `project.license` (MIT).

## Layer Structure

| Layer | File | Responsibility |
|-------|------|----------------|
| Shared strip module | `src-tauri/src/mux/scrollback_filter.rs` | The written end-state type, its per-byte transition, the state-reporting strip |
| IPC write filter | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | The carried written state, the cut paths, the overflow branch, the closure decision |

Allowed dependency direction: the write filter depends on the shared strip
module; the shared strip module never depends on the IPC layer (existing
rule, unchanged). No new dependency edge.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| WrittenState (written end state) | Where a parser replaying the bytes written so far stands, as the closure at a cut needs it | Existing states unchanged (Ground, Escape, Designator, CSI entry / parameter). Two new states: open OSC body, open ST-terminated body (DCS / APC). Transitions follow SPEC.md "Written-state transitions for string bodies": from Escape, `]` enters the OSC body and `P` / `_` enter the ST-terminated body; in the OSC body BEL returns to ground; in either body `ESC` moves to the existing Escape state; every other byte keeps the body. Every other existing transition is unchanged. Extra state stays O(1) | task0001 |
| State-reporting strip (strip_pty_output_for_scrollback_write_with_written_state) | One strip pass that also reports the end state of the carried-in state followed by the written bytes | Pre: unchanged (a pending designator is given only with Designator or Ground carried in). Post: the written bytes are identical, for the same input and designator flag, whatever state is carried in, the two body states included (NFR2); the reported state advances once per written byte and never for a removed byte | task0001 |
| Closure decision (closure_for) and the string-body closure | The one write a cut makes, chosen from the written end state alone | CSI → DEL; Escape → CAN; Designator → one `ESC`; either open body → the string-body closure, the two bytes `ESC` then CAN; Ground → nothing. Post: after the closure the written state is Ground and no designator is awaited. The closure is written after the strip, belongs to the outcome's dims, is never the opener of a strip target for either strip, and never forms ST | task0001 |
| End-state oracle (client_written_state, test support) | Classifies the end state of a stream by term_core's observable behavior | Returns the two body states as well; every existing reference stream keeps its classification; no probe provokes a response | task0001 |

## Conventions

- Test module docs and test doc comments name the feature and the FR / AC /
  TM IDs they cover, as the predecessor test modules do.
- Test names in the new regression module start with `overflow_open_string_cut_`.
- Replay oracle: `view_after_a_cut` (term_core fed the raw stream with a
  47 / 1047 / 1049 `h` / `l` pair in place of the cut, the removed
  construct's own effect kept out); exact-bytes checks compare against the
  write-path strip.
- Existing test names are kept. With no rename, no predecessor test-docs
  record changes (`.claude/rules/test-docs-records.md`).

## Cross-task Design Decisions

### D1: Body states are new WrittenState states, not a separate flag

Decision: the open OSC body and the open ST-terminated body are two new states
of the existing written end-state type. Rationale: the state-reporting strip
reports the body state (FR1) and a cut chooses its closure from the end state
alone (FR5); one state advanced per written byte keeps one transition function
and one pass (NFR1). Two states are needed because BEL ends an OSC body but is
body data in a DCS / APC body. Plan-time confirmation of SPEC A2 against
`crates/term_core/src/parser/{dcs,apc,escape}.rs`: the DCS and APC string
handlers leave the body only on `ESC` (BEL is stored as data), and `ESC X` /
`ESC ^` are unknown escape finals that return to ground. A body's `ESC` reuses
the existing Escape state: term_core's string-escape handlers complete ST on
`\` and otherwise reprocess the byte as from Escape (FR1).
Affected tasks: task0001.

### D2: The string-body closure is `ESC` followed by CAN

Decision: one named closure of two bytes, `ESC` then CAN (the existing Escape
closure after an `ESC`), next to the other three. A cut still writes exactly
one closure. term_core takes the `ESC` into the string's escape state and CAN as
an aborted (Unterminated) string followed by a completed escape, returning to
ground (SPEC A1). Affected tasks: task0001.

### D3: The model covers every written byte, not only overflow output

Decision: the body states follow the written bytes on every path; they are not
gated on the overflow branch. Consequence beyond SPEC A3: a non-overflow run
whose written bytes end inside an open body, because the strip removed the
construct whose `ESC` aborted that body or because removals spliced a string
introducer, also carries the body state, and a cut after it writes `ESC` + CAN.
The replay then matches the client, which also saw an Unterminated dispatch.
The cut-free case stays out of scope (SPEC A4). Affected tasks: task0001.

### D4: Predecessor expectations that change

- `overflow_lone_esc.rs`, the `endings()` entry 'inside an open string body':
  the open OSC body state and the `ESC` + CAN closing (FR6).
- `round4_designator_cut.rs`, the third case of
  `round4_overflow_flush_then_a_cut_without_a_wait_writes_nothing_extra`
  ("Ends inside the still-open OSC"): it pins the old empty closure for an
  overflow flush followed by a cut in the same call whose run ends inside an
  open OSC body, which is SPEC AC-5's case. The case moves to the new
  regression module with the new expectation; the predecessor test keeps its
  name and its two ground-ending cases.

Affected tasks: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| An existing test outside D4 pins the old empty closure for written bytes that end inside an open body (D3) | Low | Low | task0001's rule for such a failure: confirm parity with the oracle / view_after_a_cut, update the expectation, keep the name, report the file as a plan deviation |
| The extended end-state oracle reclassifies an existing reference stream | Low | Medium | The oracle's reference-stream table keeps its rows and gains body rows (task0001 AC-1) |
| A Kitty APC / SIXEL DCS left open past the cap is dispatched on replay at the closure | Medium | Low | The client dispatched the same payload at the switch's `ESC` (parity); tests use payloads that neither answer nor place an image |
| The snapshot-time strip's ST search from a Kitty APC / SIXEL DCS left open still spans over the closure up to a later ST in the ring | Low | Low | Pre-existing behavior; the closure adds no ST; the snapshot-time strip is unchanged (NFR5) |
| A body state coexists with an awaited designator and trips the strip's debug precondition | Low | Medium | An awaited designator needs a run ending in a written `ESC (` / `ESC )`, which leaves Designator, never a body; covered by the full suite (task0001 AC-9) |

## Open Questions

- [ ] D3 widens the `ESC` + CAN closure beyond SPEC A3 / NFR3's parenthetical "every non-overflow path": a non-overflow cut after written bytes that end inside an open body (removed aborting construct, string-introducer splice) now writes it. Confirm at review that this is accepted as within FR1 / FR2.
- [ ] D4's second item changes a predecessor expectation that FR6 does not name (mux-suppressed-output-round4-fixes' open-OSC case). Confirm at review.
