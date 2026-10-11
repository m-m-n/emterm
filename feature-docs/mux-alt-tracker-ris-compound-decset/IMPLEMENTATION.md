# Implementation Plan: mux-alt-tracker-ris-compound-decset

## Overview

The mux reader's `AltSpanTracker` learns two screen changes it does not
follow today when its client-rule screen and its shadow-rule screen disagree.
The first is RIS (`ESC c`). The second is a DEC private mode set or reset
whose parameter list holds 47, 1047 or 1049 in a form outside the six switch
patterns. With both changes, a mux pane ends in the same OSC 7501 table and
OSC 777 state as a plain tab that processes the same bytes in one call,
however the bytes are split into reads.

The work has two tasks:

- task0001: the tracker change, its unit tests, and the mux half of the
  parity corpus.
- task0002: the plain-tab half of the parity corpus (tests only).

## Technology Stack

- **Language**: Rust (edition and toolchain as already pinned by the project)
- **Crates touched**: `emterm` (src-tauri) only. term_core is not changed
  (NFR5). The shadow parser (vt100) is used as before.
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Location | Role in this feature |
|---|---|---|
| Plain-tab path (reference) | `crates/term_core`, `src-tauri/src/tabs/` | Defines the expected behavior that the mux path is aligned to. Not changed (NFR5). |
| Mux ingestion | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | Changed: `AltSpanTracker` (RIS, an `ESC c` in a designator slot, DEC private CSI parse state, compound detection). Unchanged: the main-buffer extraction, the switch-pattern list it shares with the tracker, `AgentStatusFeedScanner`, `ScrollbackWriteFilter` (FR8). |
| Mux reader | `src-tauri/src/mux/ipc/pty_spawn/mod.rs` (`pty_reader_loop`) | Unchanged. It keeps calling the tracker once per read with the shadow parser's state before and after the read, and hands the result to the scanner only. |
| Daemon task | `src-tauri/src/mux/daemon/` | Unchanged consumer of the scanner's items. |

The dependency direction does not change, and no type that crosses a layer
changes shape.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| SC-1y: parity corpus rows K38-K57 | The fixed expectations that both paths are tested against (FR6, FR7) | See "Parity corpus extension (SC-1y)" below. Precondition: each path starts from the row's seed and builds the stream exactly from the notation, with the listed length. Postcondition: the path ends in the row's final OSC 7501 table and final OSC 777 state and, where given, causes the listed number of table changes. The mux path must do this in every delivery the row lists; the plain path in one bulk call. The rows continue the SC-1x numbering (K16-K37), and that corpus stays as it is. | task0001 (mux half), task0002 (plain half) |

### Parity corpus extension (SC-1y)

Notation:

- `ESC` is 0x1B and `BEL` is 0x07.
- **E(P)** is `ESC [?P h` and **L(P)** is `ESC [?P l`, for P in 1049 and
  1047. Each is 8 bytes.
- **A** is `ESC ] 133;A` with no terminator (7 bytes).
- **DS** is `ESC (` and **DC** is `ESC )` (2 bytes each): the byte after
  either one is a charset designator.
- **R** is `ESC c` (2 bytes): RIS.
- **CC** is `ESC [ c` (3 bytes): a CSI whose final byte is `c`. It is not
  RIS.
- **W** is `ESC ] 7501;state=working BEL` (21 bytes): an OSC 7501 working
  report for the root.
- **C1h**, **C1l**, **Crh**, **C7h** and **C7l** are `ESC [?1;1049h`,
  `ESC [?1;1049l`, `ESC [?1049;1h`, `ESC [?1;1047h` and `ESC [?1;1047l`
  (10 bytes each).
- **C0h** is `ESC [?01049h` and **Ceh** is `ESC [?;1049h` (9 bytes each).
- **P1** is `ESC [?1;10` (7 bytes) and **P2** is `ESC [?10` (5 bytes): the
  start of a DEC private CSI that the R right after it interrupts.
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
- **cuts a..b**: for every offset o from a to b inclusive, two reads cut
  at o (stream[0..o] then stream[o..]).
- **bytes**: every byte in its own read.

| Row | Stream | Length | Mux deliveries | Final OSC 7501 table | Final OSC 777 state | Table changes |
|---|---|---|---|---|---|---|
| K38 | M1 R W A BEL | 39 | one; cuts 1..38; bytes | empty | none | — |
| K39 | E(1049) DS L(1049) R W A BEL | 49 | one; cuts 1..48; bytes | empty | none | — |
| K40 | E(1049) R W A BEL | 39 | one; cuts 1..38; bytes | empty | none | — |
| K41 | M1 CC W A BEL | 40 | one; cuts 8..11; bytes | root `working` | none | — |
| K42 | E(1049) DS R A BEL | 20 | one; cuts 1..19; bytes | root `working` | none | 0 |
| K43 | E(1049) DC R A BEL | 20 | one; cuts 1..19; bytes | root `working` | none | 0 |
| K44 | M1 C1h A BEL | 26 | one; cuts 8..17; bytes | root `working` | none | 0 |
| K45 | M1 C1l A BEL | 26 | one; cuts 8..17; bytes | empty | none | — |
| K46 | M1 Crh A BEL | 26 | one; cuts 8..17; bytes | root `working` | none | 0 |
| K47 | M1 C7h A BEL | 26 | one; cuts 8..17; bytes | root `working` | none | 0 |
| K48 | M1 C7l A BEL | 26 | one; cuts 8..17; bytes | empty | none | — |
| K49 | M2 C1h A BEL | 28 | one; cuts 10..19; bytes | root `working` | none | 0 |
| K50 | M2 C1l A BEL | 28 | one; cuts 10..19; bytes | empty | none | — |
| K51 | M2 Crh A BEL | 28 | one; cuts 10..19; bytes | root `working` | none | 0 |
| K52 | M2 C7h A BEL | 28 | one; cuts 10..19; bytes | root `working` | none | 0 |
| K53 | M2 C7l A BEL | 28 | one; cuts 10..19; bytes | empty | none | — |
| K54 | M2 C0h A BEL | 27 | one; cuts 10..18; bytes | root `working` | none | 0 |
| K55 | M2 Ceh A BEL | 27 | one; cuts 10..18; bytes | root `working` | none | 0 |
| K56 | M1 P1 R W A BEL | 46 | one; cuts 9..17; bytes | empty | none | — |
| K57 | M1 P2 R W A BEL | 44 | one; cuts 9..15; bytes | empty | none | — |

"Table changes" is the number of OSC 7501 table changes that the stream
causes. On the mux side it is the number of `AgentStatusUpdate` broadcasts
and the revision delta. On the plain side it is the number of pending summary
changes. "—" means the row does not assert it.

What the rows stand for:

- RIS:
  - K38 is reproduction 1 (SPEC AC1).
  - K39 is AC1 with the mismatch made by ?1049.
  - K40 is RIS on an alternate screen where both rules agree. That read used
    to take the fallback; it now gets exact spans.
  - The cuts of K38-K40 include the cut between `ESC` and `c` (AC3).
  - K41 is the AC3 negative case: `ESC [ | c` (cut at 10) is a CSI, not RIS.
  - K42 and K43 are AC4: an `ESC c` in a designator slot on the alternate
    screen.
- Compound forms:
  - K44-K53 are AC5: every compound form in both mismatch directions.
  - K49 cut at 10 is reproduction 2 (SPEC AC2).
  - The cuts of K44-K55 include the cut right before the compound and every
    offset inside it. Inside Crh this includes `ESC [?104 | 9;1h` and
    `ESC [?1049 | ;1h`.
  - K54 and K55 are the leading-zero and empty-parameter edge cases.
- RIS that interrupts a CSI:
  - K56 is RIS inside a CSI parse (`ESC [?1;10 | ESC c`).
  - K57 is RIS while a partial switch is carried (`ESC [?10 | ESC c`, FR2).

## Conventions

- **Test names**: new tests start with `altmismatch_`. Each one carries the
  acceptance-criterion labels of its task plan in its name or doc comment.
  No existing test is renamed, so no predecessor test-docs record changes
  (`.claude/rules/test-docs-records.md`).
- **Corpus lists**: neither side edits its K1-K15 or K16-K37 list. The plain
  side keeps the test that pins exactly the names K1-K15. Each side holds
  K38-K57 in a list of its own, with the same row shape as its K16-K37 list.
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
changes no production file. If a row's plain-tab outcome differs from SC-1y,
task0002 reports the row as a plan deviation. It does not change term_core,
the plain-tab path or the expectation. task0001 keeps asserting SC-1y as
written until the corpus is corrected. Affects: task0001, task0002.

### D2: Parity through one shared corpus, asserted on each path

The mux half needs only mux code and builds without `gui`. The plain half
needs the GUI tab. The two halves therefore live in different test modules
and are written in parallel. Parity holds because both halves assert the
same SC-1y expectations. Affects: task0001, task0002.

### D3: Delivery

On the mux side, each delivery is one read per part. The existing
test-controlled multi-read path (`drive_reads` with the S-W seed) hands the
reads to the real `pty_reader_loop` and the daemon agent-status task. On the
plain side, each row is delivered in one `process_combined` call on a seeded
plain tab. That call is the bulk processing SPEC TS5 names as the reference.
Affects: task0001, task0002.

### D4: One task owns the whole tracker change

The RIS rules (FR1-FR3) and the compound rules (FR4, FR5) change the same
scan and the same carried state. A RIS clears the CSI parse state, and every
`ESC`, including a RIS, discards that state. Splitting them into parallel
tasks would put two independent edits on one state machine. The mux parity
rows also need the whole change to pass, so they stay in the same task.
Affects: task0001.

### D5: File-overlap map

No file is declared by both tasks.

| File | Task |
|---|---|
| `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | task0001 |
| `src-tauri/src/mux/ipc/pty_spawn/tests/altscreen_split_read.rs` | task0001 |
| `src-tauri/src/mux/daemon/tests.rs` | task0001 |
| `src-tauri/src/tabs/tests/output_pipeline.rs` | task0002 |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A corpus expectation is wrong for the plain tab | Low | High | D1: task0002 reports it, and verify reruns both halves |
| An existing test pins today's fallback for RIS on the alternate screen, or for a compound in a mismatched state | Low | Medium | task0001 AC-8 runs every existing test unchanged. A conflict is reported as a plan deviation and is not resolved by editing the test |
| The random differential test (`altsplit_ac1_random_streams_and_cuts_match_the_oracle`) skips every stream with an adoption. Compound detections add adoptions, which could push its compared count to 2000 or below | Low | Medium | task0001 AC-8 runs it unchanged. Its alphabet has no `c`, so RIS never occurs in it. A fall to 2000 or below is reported as a plan deviation |
| The shadow parser's real RIS or compound behavior differs from SPEC A1 | Low | High | task0001 pins the real shadow parser's behavior with tests (AC-3, AC-4) |
| The change leaks into the scrollback write path | Low | High | THREAT-MODEL TM-3; task0001 AC-8 |

## Open Questions

- [ ] **Recorded deviation from FR4's literal adoption (task0001 Design §4,
  §5).** FR4 has a detected compound adopt the shadow parser's state after
  the read for both screens, and judges the A3 fallback on the screen before
  and after the read. Taken literally, AC5 cannot hold in three of the ten
  direction-and-form combinations it lists:
  - M1 then `?1;1047h`: term_core stays on the alternate screen, but the
    parser is on the main screen.
  - M2 then `?1;1047l`: term_core stays on the main screen, but the parser
    is on the alternate screen.
  - M2 then `?1;1049l` with the mark in the same read: the parser starts on
    the alternate screen, so the fallback is empty, but the mark is live on
    the plain tab.

  task0001 therefore makes three changes:
  1. It judges the fallback on the client-rule screen at the read's end.
  2. It resynchronizes the shadow-rule screen only, to the parser's state.
  3. It sets the client-rule screen from the compound's final byte.

  It still counts the detection as one adoption. The other seven
  combinations behave as the literal reading does. Confirm this reading of
  FR4 in review.
- [ ] A6 (accepted): a compound can have more parameters than term_core
  keeps (32) and hold 47, 1047 or 1049 only beyond that limit. term_core
  does not switch on it, but the tracker detects it and sets the client-rule
  screen from the final byte. SPEC A6 allows detection to be conservative
  for such inputs.
- [ ] FR5 (accepted): the adoption at the end of a detection read clears
  every carried item, as every adoption does. A partial switch at the end of
  that read is therefore not carried into the next read.
