# Implementation Plan: mux-osc-double-esc-abort

## Overview

The mux daemon's feed scanner ends an OSC string the way term_core does when
an in-body `ESC` is followed by a byte other than `\`: the string is cut
(unterminated) and that byte is handled as the byte after an `ESC`. Cut OSC
7501 strings are dropped; cut OSC 777 agent-status and OSC 133 strings are
applied as term_core applies them. Two tasks: the mux scanner change with its
unit and daemon-level tests (task0001), and the plain-tab half of the parity
corpus (task0002, tests only).

## Technology Stack

- **Language**: Rust (edition and toolchain as already pinned by the project)
- **Crates touched**: `emterm` (src-tauri) only. term_core is not changed (NFR5).
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Location | Role in this feature |
|---|---|---|
| Plain-tab path (reference) | `crates/term_core` (`osc_escape`), `src-tauri/src/callbacks.rs`, `src-tauri/src/tabs/` | The behavior the mux path is aligned to. Not changed (A2, NFR5). |
| Mux ingestion | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` (`AgentStatusFeedScanner`) | The only production change: the cut rule (FR1–FR6). |
| Mux reader and daemon task | `src-tauri/src/mux/ipc/pty_spawn/mod.rs` (`pty_reader_loop`), `src-tauri/src/mux/daemon/tasks.rs` | Unchanged consumers. The reader still passes each read's live spans to the scanner; the daemon task still applies items in receive order. |

Dependency direction is unchanged. The scanner's item type and the
program-status terminator type keep their shapes, so no consumer changes.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| SC-1: cut parity corpus | The fixed expectations both paths are tested against (FR7, FR9) | See "Cut parity corpus (SC-1)" below. Each path starts from the row's seed, delivers the row's stream as the row says, and asserts the row's final OSC 7501 table, final OSC 777 state and, where given, the table-change count. The mux side drives the real `pty_reader_loop` and the daemon agent-status task; the plain side drives `process_combined` on a plain tab. | task0001 (mux half), task0002 (plain half) |

### Cut parity corpus (SC-1)

Notation:

- `ESC` is 0x1B, `BEL` is 0x07, `\` is 0x5C.
- **EE** (cut by `ESC ESC`): the bytes `ESC ESC \` placed right after the
  string's own bytes. The second `ESC` cuts the string; the `\` that follows
  ends nothing.
- **EX** (cut by another byte): the bytes `ESC [0m` placed right after the
  string's own bytes. The `[` cuts the string and starts a harmless SGR reset.
- **R777**: `ESC ] 777;emterm;agent-status;v=1;state=working BEL`.
- **Final OSC 777 state**: the pane's / tab's OSC 777 agent state after every
  item or event of the stream is applied, the OSC 777 inferred clear (D→A
  latch) included. "none" means no state.

Seeds (the table and state before the stream; how a seed is produced is each
path's own choice):

- **S-W**: one root `working` record (what `ESC ] 7501;state=working BEL` makes). No OSC 777 state.
- **S-ALL**: records `w` working, `b` blocked, `i` idle, `d` done, `e` error. No OSC 777 state.
- **S-0**: no record, no OSC 777 state.

Deliveries:

- **one**: the whole stream in one read (mux) or one `process_combined` call (plain).
- **between ESCs**: two reads / calls; the first ends with the first `ESC` of the EE form, the second starts with the second `ESC`.
- **cut points**: one read, and each two-read / two-call split at an offset from right after the string's last own byte through right after the cutting byte (that is: after the last body byte, after the in-body `ESC`, and after the cutting byte).

| Row | Seed | Stream | Delivery | Final OSC 7501 table | Final OSC 777 state | Table changes |
|---|---|---|---|---|---|---|
| K1 | S-W | `ESC ] 07501;state=clear` EE | one | root `working` | none | 0 |
| K2 | S-W | `ESC ] 7501;state=clear` EE | one | root `working` | none | 0 |
| K3 | S-W | K1's stream | between ESCs | root `working` | none | 0 |
| K4 | S-W | K2's stream | between ESCs | root `working` | none | 0 |
| K5 | S-W | `ESC ] 7501;state=error ESC ESC ] 7501;state=clear BEL` | one | empty | none | 1 |
| K6 | S-0 | `ESC ] 777;emterm;agent-status;v=1;state=working` EE | one | empty | working | — |
| K7 | S-0 | `ESC ] 777;emterm;agent-status;v=1;state=working` EX | one | empty | working | — |
| K8 | S-0 | R777, `ESC ] 133;D` EE, `ESC ] 133;A` EE | one | empty | none (inferred clear fired) | — |
| K9 | S-0 | R777, `ESC ] 133;D` EX, `ESC ] 133;A` EX | one | empty | none (inferred clear fired) | — |
| K10 | S-0 | R777, `ESC [?1049h`, `ESC ] 133;D` EE, `ESC ] 133;A` EE, `ESC [?1049l` | one | empty | working (alternate-screen marks fire nothing) | — |
| K11 | S-ALL | `ESC ] 133;A` EE | one | `d` done, `e` error | none | — |
| K12 | S-ALL | `ESC ] 133;A` EX | one | `d` done, `e` error | none | — |
| K13 | S-ALL | `ESC ] 0133;A` EE | one | `d` done, `e` error | none | — |
| K14 | S-ALL | `ESC [?1049h`, `ESC ] 133;A` EE, `ESC [?1049l` | cut points | all five records | none | — |
| K15 | S-ALL | `ESC ] 133;A ESC [?1049h`, `ESC [?1049l` | cut points | `d` done, `e` error | none | — |

"Table changes" is the number of OSC 7501 table changes the stream causes:
on the mux side the number of `AgentStatusUpdate` broadcasts and the revision
delta, on the plain side the number of pending summary changes. "—" means the
row does not assert it.

K15 is the FR4 / A6 case: the `ESC` that cuts the string also starts the
switch to the alternate screen, and the mark still counts as a main-screen
mark because the string's own bytes ended on the main screen. Its cut points
fall inside that switch sequence, which is intended.

Not a corpus row: whether a non-canonical prompt start (`0133;A`) reaches the
OSC 777 inferred-clear latch. The mux path keeps it away from the latch by
design (osc7501-leading-zero-length FR6 / A4) while the plain tab's latch sees
term_core's canonical mark; FR7 asks for parity only for canonical D→A pairs.
The mux side of that rule is pinned at the scanner (task0001 AC-3).

## Conventions

- **Test names**: existing tests keep their names and their assertions; no
  test is renamed, so no predecessor test-docs record changes. New tests carry
  this feature's acceptance-criterion labels in their names or doc comments.
- **Logging**: a cut, discarded or rejected string produces no log line
  (NFR3). The scanner's one-shot carry-overflow warning stays as it is.
- **Feature gate**: the production change stays inside mux code built with
  `--no-default-features` and gains no dependency on a `gui`-gated item (NFR2).
- **Platform**: no platform-gated code is added (NFR4).
- **term_core**: no file under `crates/term_core/` changes, and 7501 is not
  embedded there (NFR5).

## Cross-task Design Decisions

### D1: The plain tab is the oracle

term_core and the plain-tab callbacks define the expected behavior (A2, A5,
A6); the feature changes only the mux scanner. task0002 changes no production
file. If a corpus row's plain-tab outcome differs from SC-1, task0002 reports
the row as a plan deviation instead of changing term_core or the callbacks,
and task0001 keeps asserting SC-1 as written until the corpus is corrected.
Affects: task0001, task0002.

### D2: Parity through one shared corpus, asserted on each path

The mux half needs only mux code (it builds without `gui`); the plain half
needs the GUI tab. The two halves therefore live in different test modules and
are written in parallel; parity holds because both assert the same SC-1
expectations (the precedent is the osc7501-leading-zero-length SC-4 corpus).
Affects: task0001, task0002.

### D3: Split delivery

On the mux side a split delivery means one read per part, handed out by a
test-controlled multi-read reader into the real `pty_reader_loop`, so live
spans come from the real reader. On the plain side it means one
`process_combined` call per part. No corpus delivery splits inside a switch to
the alternate screen that comes before the row's cut string (see Open
Questions). Affects: task0001, task0002.

### D4: File-overlap map

No file is declared by both tasks.

| File | Task |
|---|---|
| `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | task0001 |
| `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | task0001 |
| `src-tauri/src/mux/daemon/tests.rs` | task0001 |
| `src-tauri/src/tabs/tests/output_pipeline.rs` | task0002 |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The scanner advances its RIS tracking before its OSC decode state for each byte, so a cut item could be appended after the Reset of the same `ESC c` | High | Medium | task0001 AC-4 pins Report-then-Reset; the task plan states the ordering contract |
| The liveness of a cut OSC 133 string is judged at the cutting byte, which a switch sequence starting at the cutting `ESC` removes from the live spans | Medium | High | task0001 AC-3 / AC-5 pin the "string's own last byte" rule, including K15 and a three-read case with differing spans (TM-2) |
| An existing test pins the old "drop without emitting" behavior for a cut OSC 777 / OSC 133 string, contradicting FR8 | Low | Medium | task0001 AC-7 runs every existing test unchanged; a conflict is reported as a plan deviation, not resolved by editing the test |
| A corpus expectation is wrong for the plain tab | Low | High | D1: task0002 reports it; verify reruns both halves |

## Open Questions

- [ ] A switch to the alternate screen whose bytes are split across two reads,
  and which is undone within the second read, leaves the reader's alternate-
  screen state equal to the shadow parser's at the end of that read, so the
  reader's live-span cross-check does not fall back and an OSC 133 mark inside
  it is judged live on the mux path while the plain tab suppresses it. This
  applies to BEL / ST-terminated marks alike, predates the feature, and lies in
  the live-span derivation that FR8 keeps unchanged; no SC-1 delivery splits
  inside a switch to the alternate screen that comes before the cut string.
  Left unchanged.
