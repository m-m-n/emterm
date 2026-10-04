# Implementation Plan: mux-strip-open-string-body-closure

## Overview

When the shared strip removes a construct while the written stream is inside
an open OSC body or an open ST-terminated (DCS / APC) body, it first writes the
string-body closure, ESC then CAN. Later bytes then cannot join the body, and
the ring's replay stands where term_core stands on the raw stream, whose
construct `ESC` aborted the string.

The whole change is one task (task0001). The production fix, the inverted
predecessor expectations, the rename, the test-docs record update and the new
regression tests only pass together, so no split yields tasks that pass
independently in their own worktrees.

## Technology Stack

- **Language**: Rust, `src-tauri` crate, `mux` module. Both the GUI build and
  the `--no-default-features` CLI build compile it.
- **Replay engine / test oracle**: `term_core` (workspace crate), unchanged
  (NFR3).
- **New dependencies**: none, so there is no license entry to record against
  `project.license` (MIT).

## Layer Structure

| Layer | File | Responsibility |
|-------|------|----------------|
| Shared strip module | `src-tauri/src/mux/scrollback_filter.rs` | The single strip pass behind every entry point, the written-state model, the closure written before a removal, and the single definition of the string-body closure bytes |
| IPC write filter | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | The carried written state, the overflow flush, the cut paths and the cut closure decision. It re-exports the string-body closure |
| Snapshot builders | `src-tauri/src/mux/snapshot_bytes.rs` | The snapshot-time strip and segment remap through the shared strip. Production code here is unchanged; only its test module changes |

Allowed dependency direction: the write filter and the snapshot builders
depend on the shared strip module. The shared strip module never depends on
the IPC layer; this existing rule is unchanged. Moving the closure's
definition into the shared module keeps that direction. No new dependency edge
appears.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| STRING_BODY_CLOSING | The string-body closure bytes | Defined exactly once, in the shared strip module, with mux-module visibility (the same visibility CSI_CLOSING has). Its value is unchanged: the two bytes 0x1B (ESC) and 0x18 (CAN). The write filter re-exports it under the same name, as it already does for CSI_CLOSING. The cut closure decision and every existing test keep using that name | task0001 |
| Closure before a removal (Written::close_before_removal) | The bytes written in place of a removed construct's opening `ESC`, chosen from the current written state | Inside a CSI, or right after a written lone `ESC`: one CSI_CLOSING (DEL), unchanged. Inside an open OSC body or an open ST-terminated body: STRING_BODY_CLOSING, written through the per-byte writer, so the written state advances from the body through Escape to Ground. Ground and Designator: nothing, unchanged. Post: after the string-body closure the state is Ground, so a later removal in the same run writes nothing more unless later written bytes reopen a body or a CSI. The closure is always written before the removed construct's re-emitted C0 bytes | task0001 |
| State-reporting strip (the written-state entry point) | One strip pass that also reports the end state of the carried-in state followed by the written bytes | Post for a carried-in body state: the output equals the ground-started strip of the bytes that enter that state followed by the input, minus those entering bytes (D4). The reported state equals what the end-state oracle observes for the carried-in state followed by the output. Extra state stays O(1) and there is still one pass (NFR2) | task0001 |
| Remap (watch offsets and snapshot segments) | Maps input offsets to output offsets around removals | Unchanged mechanism, now covering the closure: the watch offset of a removed construct's first byte maps to the output position before the closure. An offset inside the construct, or right after it, maps past the closure and the re-emitted C0 bytes. Mapped offsets are non-decreasing and never exceed the output length (FR4) | task0001 |
| Cut closure decision (closure_for) | The one write a cut makes, chosen from the written end state alone | Unchanged (FR5): CSI gives CSI_CLOSING, Escape gives CAN, Designator gives one `ESC`, either open body gives STRING_BODY_CLOSING, and Ground gives nothing. After a strip-written string-body closure the state is Ground, so a cut writes nothing | task0001 |

## Conventions

- New tests start with `open_body_closure_`. Their doc comments name the
  feature and the FR / AC / TM IDs they cover, as the predecessor test modules
  do.
- Replay oracle (NFR3): term_core fed the raw stream with the removed
  construct's own effect excluded. Use view_after_a_cut, comparing rows,
  cursor and responses, and the end-state oracle client_written_state.
  Exact-bytes checks compare against the write-path strip.
- A test that asserts "no color response" replays through the themed client,
  which answers OSC 4/10/11/12 queries. A client without that responder
  answers no color query, so the assertion would pass vacuously. Each such
  test has a control that shows the themed client answers. Tests that use the
  themed client are compiled only with the `gui` feature, like the existing
  themed-client helpers.
- Time budget: the existing 10 s per-test budget.
- Reuse the existing helpers instead of adding parallel ones: the target
  tables and entry-point drivers in `strip_concat_query`, `post_strip_cut_csi`
  and `scrollback_filter::tests`, plus `round3_write_path`, `round4_cut_csi`
  and `strip_join_escape_closure`. Widening a sibling helper's visibility is
  allowed.
- Only a test whose name becomes inaccurate is renamed (SPEC A2). A rename of
  a test listed in a predecessor test-docs record updates that record
  (`.claude/rules/test-docs-records.md`).

## Cross-task Design Decisions

### D1: The closure at a removal is the string-body closure, written through the per-byte writer

Decision: a removal made inside an open OSC or ST-terminated body writes ESC
then CAN, the same bytes a cut already writes for an open body. It is written
through the same per-byte writer as every other written byte, so the written
state moves from the body to Escape and then to Ground, and the reported state
and every later decision follow without a special case.

Rationale: term_core aborts an OSC / DCS / APC body at any `ESC` not followed
by `\`. It dispatches the partial string, as Unterminated for OSC, and
reprocesses the next byte from Escape. CAN in Escape returns to ground. The
replay therefore aborts the body exactly where the raw stream's construct
`ESC` aborted it, and the partial-string dispatch is the same on both sides
(SPEC A3).

The closure is written before the construct's re-emitted C0 bytes. So a
re-emitted BEL can no longer terminate the OSC as a complete string, which was
the join that produced a complete color query.

Affected tasks: task0001.

### D2: One definition of the closure bytes, in the shared strip module

Decision: STRING_BODY_CLOSING moves from the write filter to the shared strip
module, and the write filter re-exports it, mirroring CSI_CLOSING (FR2). The
shared module's doc comments describe the byte values. They do not link to the
write filter's ESCAPE_CLOSING, because the shared module must not depend on
the IPC layer.

Affected tasks: task0001.

### D3: Cut path unchanged; the closure is never doubled

Decision: the cut closure decision and the cut paths stay as they are (FR5).
Because the strip-written closure leaves the state in Ground, a cut at the end
of the same call, a cut at fed offset 0 of the next call and the reader's
fallback closing all choose nothing. Both strips keep the closure unchanged
and still remove a target written right after it; the state is then Ground,
so no closure is added. The vt100 replay copy keeps the closure, because it
rewrites only DEL (FR6).

Affected tasks: task0001.

### D4: Pinned expectations that change (FR8)

The changes are those listed in SPEC FR8 items 1 to 6:

- the AC-11 rename with its four inversions;
- the AC-11 test-docs record update;
- the body rows that move out of the two Ground-closure tests;
- the state-form row inversions;
- the state-form identity.

Each is recorded with its before and after values in this feature's
DECISIONS.md (FR9).

Plan-time confirmation of the FR8 item 6 identity: for a carried OSC body,
prefix the input with `ESC ]0;`; for a carried ST-terminated body, prefix it
with `ESC _x`. These entering bytes never form or complete a strip target
together with any input, and they never start an ST search. So the
ground-started strip of entering bytes plus input always begins with the
entering bytes unchanged, and the rest equals the state-form output. Any other
existing expectation that changes is handled by task0001's rule for an
unforeseen failing test and reported as a plan deviation.

Affected tasks: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| An existing test outside SPEC FR8 items 1 to 6 pins the old "nothing written inside a body" expectation | Medium | Low | task0001's rule for an unforeseen failing test: confirm the replay with the oracle, update the expectation, keep the name, record it in DECISIONS.md, and report it as a plan deviation |
| A "no color response" test passes vacuously because it replays through a client with no color responder | Medium | High | Themed client plus a control in every such test (Conventions); task0001 AC-3 and AC-4 state the controls |
| The partial-string dispatch at the closure produces a side effect | Low | Low | It equals the raw stream's dispatch at the construct's `ESC` (SPEC A3); test heads abort with no answer |
| An open target-shaped DCS / APC body still makes the ST search span to a later ST | Low | Low | Pre-existing and out of scope (SPEC A4); test heads avoid target shapes |
| A doc-comment edit drops a phrase the predecessor doc-contract test requires, or reintroduces a stale one | Low | Low | The predecessor contract test keeps running; the new contract test pins the FR10 phrases (task0001 AC-7) |
| A themed-client test is left ungated and references the `gui`-only helpers, so a test build without `gui` fails to compile | Low | Low | Gate themed-client tests on `gui`, as the existing themed-client helpers are (Conventions) |

## Open Questions

- None.
