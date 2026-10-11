# Implementation Plan: mux-alt-tracker-detection-read-boundary

## Overview

The mux reader's `AltSpanTracker` changes how it ends and continues a
detection read, that is, a read in which it detected a compound DEC private
set or reset while its two screens disagreed. After the first detection in a
read, every later compound in the same read is detected as well (FR1). The
carried item that the scan records after the last detected compound is kept
for the next read instead of being cleared (FR2). Everything else about a
detection read stays as it is (FR3). With both changes, a mux pane ends in
the same OSC 7501 table and OSC 777 state as a plain tab for the new parity
rows K58-K64, in every delivery each row lists (FR4).

The work has two tasks:

- task0001: the tracker change and its doc comments, the tracker unit tests
  (new tests and the one renamed test), the predecessor's test-docs record,
  and the mux half of the parity rows.
- task0002: the plain-tab half of the parity rows (tests only).

## Technology Stack

- **Language**: Rust (edition and toolchain as already pinned by the project)
- **Crates touched**: `emterm` (src-tauri) only. term_core is not changed
  (NFR5). The shadow parser (vt100) is used as before.
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Location | Role in this feature |
|---|---|---|
| Plain-tab path (reference) | `crates/term_core`, `src-tauri/src/tabs/` | Defines the expected behavior that the mux path is aligned to. Not changed (NFR5). |
| Mux ingestion | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | Changed: the detection condition in the tracker's scan and the end-of-read handling of a detection read, plus their doc comments. Unchanged: the main-buffer extraction, the switch-pattern list, `AgentStatusFeedScanner`, `ScrollbackWriteFilter`, the shadow parser (FR8). |
| Mux reader | `src-tauri/src/mux/ipc/pty_spawn/mod.rs` (`pty_reader_loop`) | Unchanged. It keeps calling the tracker once per read with the shadow parser's state before and after the read. |
| Daemon task | `src-tauri/src/mux/daemon/` | Unchanged consumer of the scanner's items. |

The dependency direction does not change, and no type that crosses a layer
changes shape. The signature of the tracker's per-read entry point stays as
it is (FR8).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| SC-1z: parity corpus rows K58-K64 | The fixed expectations that both paths are tested against (FR4, FR5) | See "Parity corpus extension (SC-1z)" below. Precondition: each path starts from the row's seed and builds the stream exactly from the notation, with the listed length. Postcondition: the path ends in the row's final OSC 7501 table and final OSC 777 state and, where given, causes the listed number of table changes. The mux path must do this in every delivery the row lists; the plain path in one bulk call. The rows continue the K38-K57 numbering, and every earlier list (K1-K15, K16-K37, K38-K57) stays as it is (FR5, SPEC A4). | task0001 (mux half), task0002 (plain half) |

### Parity corpus extension (SC-1z)

Notation (the same notation the K38-K57 rows use, restated so that both
halves can build their streams from this document alone):

- `ESC` is 0x1B and `BEL` is 0x07.
- **E(P)** is `ESC [?P h` and **L(P)** is `ESC [?P l`, for P in 1049 and
  1047. Each is 8 bytes.
- **A** is `ESC ] 133;A` with no terminator (7 bytes).
- **DS** is `ESC (` (2 bytes): the byte after it is a charset designator.
- **R** is `ESC c` (2 bytes): RIS.
- **W** is `ESC ] 7501;state=working BEL` (21 bytes): an OSC 7501 working
  report for the root.
- **C1h**, **C1l**, **Crh**, **C7h** and **C7l** are `ESC [?1;1049h`,
  `ESC [?1;1049l`, `ESC [?1049;1h`, `ESC [?1;1047h` and `ESC [?1;1047l`
  (10 bytes each). The h-forms take term_core to the alternate screen and
  the l-forms to the main screen. The shadow parser follows C1h, C1l and Crh
  and ignores C7h and C7l.
- **M1** is E(1047) (8 bytes). After it, term_core is on the alternate
  screen and the shadow parser is on the main screen, because the shadow
  parser ignores ?1047.
- **M2** is DS E(1049) (10 bytes). After it, term_core is on the main
  screen, because the switch's `ESC` is the designator. The shadow parser is
  on the alternate screen.
- **S-W** is the only seed used: one root `working` record and no OSC 777
  state.

Deliveries (offsets are byte offsets into the row's stream):

- **one**: the whole stream in one read (mux) or one call (plain).
- **cuts a..b**: for every offset o from a to b inclusive, two reads cut at
  o (stream[0..o] then stream[o..]). A row may list two such ranges.
- **bytes**: every byte in its own read.

| Row | Stream | Length | Mux deliveries | Final OSC 7501 table | Final OSC 777 state | Table changes |
|---|---|---|---|---|---|---|
| K58 | M1 C1l C1h A BEL | 36 | one; cuts 1..35; bytes | root `working` | none | 0 |
| K59 | M1 C7l Crh A BEL | 36 | one; cuts 1..35; bytes | root `working` | none | 0 |
| K60 | M2 C1h C1l A BEL | 38 | one; cuts 1..19; cuts 30..37; bytes | empty | none | — |
| K61 | M2 C7h C1l A BEL | 38 | one; cuts 1..19; cuts 30..37; bytes | empty | none | — |
| K62 | M1 C7h DS R A BEL | 30 | one; cuts 1..29; bytes | root `working` | none | 0 |
| K63 | M1 C7h R W A BEL | 49 | one; cuts 1..48; bytes | empty | none | — |
| K64 | M1 C1l E(1047) A BEL | 34 | one; cuts 1..33; bytes | root `working` | none | 0 |

"Table changes" is the number of OSC 7501 table changes that the stream
causes. On the mux side it is the number of `AgentStatusUpdate` broadcasts
and the revision delta. On the plain side it is the number of pending summary
changes. "—" means the row does not assert it.

What the rows stand for:

- Two compounds in one read (FR1):
  - K58 is reproduction 1 (SPEC AC1): M1, then an l-form and an h-form.
  - K59 is the same shape with C7l (which the shadow parser ignores) and
    Crh (a reordered parameter list).
  - K60 is the reverse direction of reproduction 1: M2, then an h-form and
    an l-form.
  - K61 is the K60 shape with C7h as the h-form.
- A carried item after the compound (FR2):
  - K62 is reproduction 2 (SPEC AC2) when cut at 20 (`ESC (` | `ESC c`):
    the designator wait. Cut at 19 carries a lone `ESC` before `(`, and cut
    at 21 carries `ESC` in the designator slot.
  - K63 cut at 19 carries a lone `ESC` that the next read's `c` turns into
    a RIS (predecessor FR2).
  - K64 cut at 23 is `ESC [?10` | `47h` (SPEC FR2): the partial switch,
    completed as ?1047h in the next read.
- The cuts of K58 and K59 also cover a carried CSI parse after a detection
  (for example K58 cut at 25: `ESC [?1;10` | `49h`) and a partial switch
  inside the second compound.

## Conventions

- **Test names**: new tests start with `altdetect_`. Each one carries the
  acceptance-criterion labels of its task plan in its name or doc comment.
  The one test that FR6 renames keeps the name fixed in task0001's plan.
- **Corpus lists**: each side holds K58-K64 in a list of its own, with the
  same row shape as its K38-K57 list. Neither side edits its K1-K15,
  K16-K37 or K38-K57 list, or the tests that pin those lists (FR5, SPEC A4).
- **Existing tests**: apart from the one test that FR6 renames, no existing
  test function changes (FR8). An existing test helper keeps its signature
  and its results; a new helper is added beside it when a new test needs
  more (for example a client-rule walk that takes two or more screen
  override points, SPEC TS1).
- **Logging**: the feature adds no log call (NFR3).
- **Feature gate**: the production change stays inside mux code built with
  `--no-default-features`. It gains no dependency on a `gui`-gated item
  (NFR2).
- **Platform**: no platform-gated code is added (NFR4).
- **term_core**: no file under `crates/term_core/` changes (NFR5).
- **Commands**: every cargo command runs from the project root with
  `--manifest-path` and `CARGO_TARGET_DIR=src-tauri/target`
  (`.claude/rules/core-build-location.md`).

## Cross-task Design Decisions

### D1: The plain tab is the oracle

term_core and the plain-tab path define the expected behavior. task0002
changes no production file. If a row's plain-tab outcome differs from SC-1z,
task0002 reports the row as a plan deviation. It does not change term_core,
the plain-tab path or the expectation. task0001 keeps asserting SC-1z as
written until the corpus is corrected. Affects: task0001, task0002.

### D2: Parity through one shared corpus, asserted on each path

The mux half needs only mux code. The plain half needs the GUI tab. The two
halves therefore live in different test modules and are written in
parallel. Parity holds because both halves assert the same SC-1z
expectations. Affects: task0001, task0002.

### D3: Delivery

On the mux side, each delivery is one read per part. The existing
test-controlled multi-read path (`drive_reads` with the S-W seed) hands the
reads to the real `pty_reader_loop` and the daemon agent-status task. On the
plain side, each row is delivered in one `process_combined` call on a seeded
plain tab. Affects: task0001, task0002.

### D4: One task owns the tracker change and the mux rows

FR1 and FR2 change the same scan and the same end-of-read handling. The mux
rows, the renamed test and the predecessor's test-docs record all depend on
that change to pass, so they stay in the same task. Only the plain-tab rows,
which pass on the unmodified code base, are a task of their own.
Affects: task0001, task0002.

### D5: The cut ranges of K60 and K61

In K60 and K61, a cut at any offset from 20 to 29 puts the l-form's final
byte into a read that starts with both screens on the alternate screen: the
h-form's detection read ended with the client-rule screen on the alternate
screen, and the shadow-rule screen took the parser's alternate state (FR3).
In that read no compound has been detected yet and the screens agree, so the
l-form is not detected (FR1 keeps the predecessor's A2). The existing
post-read cross-check fallback (unchanged by FR8) then gives that read no
live byte, and the mark that follows in the same read is dropped, while the
plain tab counts it. These cuts are therefore not listed. Every other single
cut, one read and one byte per read are listed. K58 and K59 need no such
exclusion: their second compound takes term_core to the alternate screen,
where the same fallback is correct. Affects: task0001, task0002.

### D6: File-overlap map

No file is declared by both tasks.

| File | Task |
|---|---|
| `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | task0001 |
| `src-tauri/src/mux/ipc/pty_spawn/tests/altscreen_split_read.rs` | task0001 |
| `src-tauri/src/mux/daemon/tests.rs` | task0001 |
| `test-docs/mux-alt-tracker-ris-compound-decset/task0001.tests.yaml` | task0001 |
| `src-tauri/src/tabs/tests/output_pipeline.rs` | task0002 |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A row's expectation is wrong for the plain tab | Low | High | D1: task0002 reports it, and verify reruns both halves |
| A listed mux delivery other than those D5 excludes fails because a later compound arrives in a matched state | Low | Medium | task0001 reports the row and the delivery as a plan deviation; a delivery is never dropped silently |
| An existing test other than the renamed one changes its result, because a kept carry or a later detection changes the reads after a detection read | Low | Medium | task0001 AC-7 runs every existing test unchanged. A failure is reported as a plan deviation and is not resolved by editing the test |
| A kept carried item reaches a resume path right after a detection read that no input reached before, and panics the reader thread | Low | High | THREAT-MODEL TM-2; task0001 AC-6 |
| The change leaks into the scrollback write path | Low | High | THREAT-MODEL TM-3; task0001 AC-7 |

## Open Questions

- [ ] **K60 and K61 list no cuts from 20 to 29 (D5).** FR4 asks for a match
  "in every delivery each row lists (every single cut and one byte per
  read)", and AC3 lists "M2 + h-form + l-form" among the rows. At those
  cuts the l-form arrives in a matched state in a read that has not
  detected anything, and FR1 (predecessor A2) and FR8 (the unchanged
  post-read fallback) keep the mark of that read dropped. No SPEC-conforming
  tracker can match the plain tab there. Confirm in review that leaving
  those cuts out of the rows is the intended reading.
- [ ] **Accepted limitation in the unit-test streams.** In the streams M1 +
  l-form + C7h and M2 + h-form + C7l, a delivery that splits the two
  compounds hands the second one to a read that starts in a matched state.
  It is not detected and the shadow parser ignores it, so the tracker keeps
  the first compound's screen while term_core switches (predecessor A2).
  task0001's unit tests pin the tracker's behavior for those deliveries;
  the corpus rows avoid these combinations.
