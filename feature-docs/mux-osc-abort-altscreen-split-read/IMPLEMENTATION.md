# Implementation Plan: mux-osc-abort-altscreen-split-read

## Overview

The mux reader carries the partial state of an alternate-screen switch
sequence from one PTY read to the next and hands the feed scanner live spans
that equal the bulk extraction of the same bytes, so a mux pane ends in the
same OSC 7501 table and OSC 777 state as a plain tab however the switch
sequences are split across reads. The scrollback write path is unchanged.
Two tasks: the reader-side change with its unit and daemon-level tests
(task0001), and the plain-tab half of the parity corpus (task0002, tests
only).

## Technology Stack

- **Language**: Rust (edition and toolchain as already pinned by the project)
- **Crates touched**: `emterm` (src-tauri) only. term_core is not changed
  (NFR5). The shadow parser (vt100 0.16) is used as before.
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Location | Role in this feature |
|---|---|---|
| Plain-tab path (reference) | `crates/term_core`, `src-tauri/src/callbacks.rs`, `src-tauri/src/tabs/` | The behavior the mux path is aligned to. Not changed (A2, NFR5). |
| Mux ingestion | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | New: the split-aware live-span tracker, beside the main-buffer extraction. Unchanged: the extraction, the feed scanner, the scrollback write filter. |
| Mux reader | `src-tauri/src/mux/ipc/pty_spawn/mod.rs` (`pty_reader_loop`) | Feeds the scanner the tracker's spans. The write path, its fallback and the ranges handed to the suppressed-chunk pipeline are unchanged (FR5). |
| Daemon task | `src-tauri/src/mux/daemon/tasks.rs` | Unchanged consumer of the scanner's items. |

Dependency direction is unchanged, and no item type changes shape, so no
consumer changes.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| SC-1x: parity corpus extension K16–K37 | The fixed expectations both paths are tested against (FR6, FR7) | See "Parity corpus extension (SC-1x)" below. Each path starts from the row's seed, delivers the row's stream as D3 says, and asserts the row's final OSC 7501 table, final OSC 777 state and, where given, the table-change count. Precondition: the stream is built exactly from the notation below and has the listed length. The rows extend the SC-1 corpus of mux-osc-double-esc-abort (K1–K15), which stays as it is. | task0001 (mux half), task0002 (plain half) |

### Parity corpus extension (SC-1x)

Notation:

- `ESC` is 0x1B, `BEL` is 0x07, `\` is 0x5C.
- **E(P)** is `ESC [?P h` (switch to the alternate screen) and **L(P)** is
  `ESC [?P l` (switch back to the main screen), for P in 1049, 1047, 47.
  E(1049), L(1049), E(1047), L(1047) are 8 bytes; E(47), L(47) are 6 bytes.
- **A** is `ESC ] 133;A` with no terminator (7 bytes).
- **Forms F**: **BEL**; **ST** = `ESC \`; **EE** = `ESC ESC \` (the second
  `ESC` cuts the string); **EX** = `ESC [0m` (the `[` cuts the string).
- **DS** is `ESC (` (2 bytes): the byte after it is a charset designator.
- **X** is `ESC [?104x` (7 bytes): a prefix of ?1047 / ?1049 followed by a
  byte that completes no switch.
- **S-W** (the only seed used): one root `working` record, no OSC 777 state —
  the same seed as SC-1's S-W.

Deliveries (offsets are byte offsets into the row's stream):

- **one**: the whole stream in one read (mux) or one call (plain).
- **cuts S**: for every offset o in the set S, two reads cut at o
  (stream[0..o] then stream[o..]). "a..b" is every offset from a to b
  inclusive.
- **reads cut at [o1, o2, …]**: the reads stream[0..o1], stream[o1..o2], …,
  up to the stream's end.
- **AC1 shape a..b**: for every o from a to b inclusive, reads cut at
  [|E(P)|, o, string end], where "string end" is the offset right after A's
  last byte (23 for P = 1049 / 1047, 19 for P = 47).

| Row | Stream | Length | Mux deliveries | Final OSC 7501 table | Final OSC 777 state | Table changes |
|---|---|---|---|---|---|---|
| K16 | E(1049) L(1049) A EE | 26 | one; reads cut at [8, 15, 23] | empty | none | — |
| K17 | E(1049) L(1049) A BEL | 24 | one; cuts 9..15 and 23; AC1 shape 9..15 | empty | none | — |
| K18 | E(1049) L(1049) A ST | 25 | as K17 | empty | none | — |
| K19 | E(1049) L(1049) A EE | 26 | as K17 | empty | none | — |
| K20 | E(1049) L(1049) A EX | 27 | as K17 | empty | none | — |
| K21 | E(1047) L(1047) A BEL | 24 | as K17 | empty | none | — |
| K22 | E(1047) L(1047) A ST | 25 | as K17 | empty | none | — |
| K23 | E(1047) L(1047) A EE | 26 | as K17 | empty | none | — |
| K24 | E(1047) L(1047) A EX | 27 | as K17 | empty | none | — |
| K25 | E(47) L(47) A BEL | 20 | one; cuts 7..11 and 19; AC1 shape 7..11 | empty | none | — |
| K26 | E(47) L(47) A ST | 21 | as K25 | empty | none | — |
| K27 | E(47) L(47) A EE | 22 | as K25 | empty | none | — |
| K28 | E(47) L(47) A EX | 23 | as K25 | empty | none | — |
| K29 | E(1049) L(1049) A BEL E(1049) | 32 | one; cuts 9..15 | empty | none | — |
| K30 | E(1047) L(1047) A BEL E(1047) | 32 | one; cuts 9..15 | empty | none | — |
| K31 | E(47) L(47) A BEL E(47) | 26 | one; cuts 7..11 | empty | none | — |
| K32 | E(1049) A BEL L(1049) | 24 | one; cuts 1..8 | root `working` | none | 0 |
| K33 | E(1047) A BEL L(1047) | 24 | one; cuts 1..8 | root `working` | none | 0 |
| K34 | E(47) A BEL L(47) | 20 | one; cuts 1..6 | root `working` | none | 0 |
| K35 | DS E(1049) A BEL | 18 | one; cuts 1..9 | empty | none | — |
| K36 | E(1049) DS L(1049) A BEL | 26 | one; cuts 1..17 | root `working` | none | 0 |
| K37 | X A BEL | 15 | one; cuts 1..7 | empty | none | — |

Every row uses seed S-W. "Table changes" is the number of OSC 7501 table
changes the stream causes: on the mux side the number of `AgentStatusUpdate`
broadcasts and the revision delta, on the plain side the number of pending
summary changes. "—" means the row does not assert it.

What the rows stand for: K16 is the reproduction (SPEC AC1). K17–K28 are
SPEC AC2 (the mark after a switch back to the main screen that is split
inside its sequence, with the string's end split from its terminator or cut).
K29–K31 are SPEC AC3 (back to the main screen and into the alternate screen
again within one read). K32–K34 are SPEC AC4 (the mark on the alternate
screen after a switch into it that is split inside its sequence or delivered
in a read of its own: it must not count). K35 and K36 are SPEC AC5 (a switch
whose `ESC` is the designator after `ESC (`, which term_core does not take as
a switch, split inside the sequence and between `ESC`, `(` and the
designator). K37 is the FR4 rule that a partial sequence followed by a byte
that completes no switch leaves the bytes as they are.

## Conventions

- **Test names**: new tests start with `altsplit_` and carry this feature's
  acceptance-criterion labels in their names or doc comments. Existing tests
  keep their names and their assertions; no test is renamed, so no
  predecessor test-docs record changes.
- **Corpus lists**: the K1–K15 lists on both sides are not edited (the plain
  side has a test that pins exactly the names K1–K15). Each side holds
  K16–K37 in a list of its own, with the same row shape as its K1–K15 list.
- **Logging**: the feature adds no log call (NFR3).
- **Feature gate**: the production change stays inside mux code built with
  `--no-default-features` and gains no dependency on a `gui`-gated item
  (NFR2).
- **Platform**: no platform-gated code is added (NFR4).
- **term_core**: no file under `crates/term_core/` changes (NFR5).

## Cross-task Design Decisions

### D1: The plain tab is the oracle

term_core and the plain-tab callbacks define the expected behavior (A2); the
feature changes only the mux reader. task0002 changes no production file. If
a row's plain-tab outcome differs from SC-1x, task0002 reports the row as a
plan deviation instead of changing term_core, the callbacks or the
expectation, and task0001 keeps asserting SC-1x as written until the corpus
is corrected. Affects: task0001, task0002.

### D2: Parity through one shared corpus, asserted on each path

The mux half needs only mux code (it builds without `gui`); the plain half
needs the GUI tab. The two halves therefore live in different test modules
and are written in parallel; parity holds because both assert the same SC-1x
expectations, as SC-1 did. Affects: task0001, task0002.

### D3: Delivery

On the mux side each delivery is one read per part, handed out by the
existing test-controlled multi-read reader into the real `pty_reader_loop`
and the daemon agent-status task, so the live spans come from the real
reader. On the plain side each row is delivered in one `process_combined`
call: the bulk processing that SPEC TS3 names as the reference. Affects:
task0001, task0002.

### D4: File-overlap map

No file is declared by both tasks.

| File | Task |
|---|---|
| `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | task0001 |
| `src-tauri/src/mux/ipc/pty_spawn/mod.rs` | task0001 |
| `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | task0001 |
| `src-tauri/src/mux/ipc/pty_spawn/tests/altscreen_split_read.rs` | task0001 |
| `src-tauri/src/mux/daemon/tests.rs` | task0001 |
| `src-tauri/src/tabs/tests/output_pipeline.rs` | task0002 |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The shadow parser (vt100 0.16) does not act on ?1047. A cross-check that counts ?1047 as a switch disagrees with it whenever a read ends after an unbalanced ?1047 switch, falls back and adopts the main screen while the client is on the alternate screen, so an alternate-screen mark in the next read would count (K33 cut at 8) | High | High | task0001's tracker models the shadow parser's actual switch set for its cross-check and pins that set with a test (task0001 AC-6) |
| A corpus expectation is wrong for the plain tab | Low | High | D1: task0002 reports it; verify reruns both halves |
| An existing test pins today's per-read liveness (a mark after a designator-slot switch, or after a split switch) and contradicts FR1 | Low | Medium | task0001 AC-7 runs every existing test unchanged; a conflict is reported as a plan deviation, not resolved by editing the test |
| The scanner's exact spans leak into the write path or the suppressed-chunk pipeline | Medium | High | task0001 AC-4 pins the ring content and the delivered bytes of the reproduction reads against the unmodified reader (THREAT-MODEL TM-3) |

## Open Questions

- [ ] FR1 says the per-read live spans equal the bulk extraction "for every
  byte". A read that ends inside a possible switch sequence cannot know its
  tail's liveness until the next read. task0001 makes such an undecided tail
  non-live: that equals the bulk whenever the sequence completes (the bulk
  drops a completed switch's bytes); when it never completes, the bulk calls
  those bytes live. The scanner makes no liveness decision on those bytes, so
  items, tables and states are unaffected (task0001 AC-1, AC-2). Confirm this
  reading of FR1 / FR4 in review.
- [ ] Pre-existing and out of scope (FR5, A4): the write path's cross-check
  also counts ?1047, which the shadow parser does not follow, so by code
  reading a read that ends after an unbalanced ?1047 switch takes the
  fallback, and the reads after it are extracted from the shadow parser's
  main-screen state, which can write alternate-screen bytes to the
  scrollback ring. Candidate for a follow-up feature.
