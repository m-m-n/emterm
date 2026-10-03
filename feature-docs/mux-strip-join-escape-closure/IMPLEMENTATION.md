# Implementation Plan: mux-strip-join-escape-closure

## Overview

This feature adds regression tests that pin mux-strip-concat-query-closure D1 against the three join attack scenarios. It also writes a decision record of the scenario mapping and the kept-string-body join residual. There is no production change, and the feature has a single task (task0001).

## Technology Stack

- **Language**: Rust. The tests live in the `src-tauri` crate's library unit tests and run with the `--lib` test command.
- **Replay engine and oracle**: the workspace terminal core (`crates/term_core`), used through its existing test-visible entry points.
- **New dependencies**: none. Nothing needs a license check against `project.license` (MIT).

## Layer Structure

The pipeline under test runs as follows. Every layer is read-only for this feature.

1. **Write strip.** The mux daemon's scrollback write filter (`ScrollbackWriteFilter`, under `src-tauri/src/mux/ipc/pty_spawn/`) runs on top of the shared strip pass in `src-tauri/src/mux/scrollback_filter.rs`. The predecessor closure lives there.
2. **Scrollback ring.** It holds the write-strip output.
3. **Snapshot strip.** The snapshot assembly entry point in `src-tauri/src/mux/snapshot_bytes.rs` re-strips the ring.
4. **Replay.** A term_core instance consumes the snapshot output, and its pending responses are read back.

New tests depend on all four layers through their existing entry points. No production layer gains a dependency on test code.

## Shared Components

This is a single-task feature: no component is built by one task and consumed by another. The table lists the existing components the task consumes read-only, with the contract the tests rely on.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Scrollback write filter | Removes the rich-content and query constructs, and applies the predecessor closure | Pre: input bytes fed in one cut-free call. Post: the ring holds the stripped bytes. Exactly one DEL precedes a construct removed while the written stream is inside a CSI or right after a written lone ESC. | task0001 |
| Snapshot assembly | Builds the reattach payload and re-strips the ring | Pre: a ring. Post: the scrollback part of the output is the re-stripped ring. A ring containing no removable construct is left unchanged. | task0001 |
| term_core instance | Replay engine and raw-stream oracle | Pre: a fresh instance. Post: rows, cursor and pending responses are observable after feeding bytes. | task0001 |
| Sibling test helpers (`strip_concat_query`, `escape_state_carry`, `round4_cut_csi`) | Reattach replay, reference views and the removed-construct table | Reused as they are. Widening a helper's visibility within the tests module is the only permitted change to them. | task0001 |

## Conventions

- **No production change (NFR2).** The following stay untouched:
  - non-test code under `src-tauri/src`
  - everything under `crates/term_core`
  - existing test bodies, expectations and names
  - predecessor features' DECISIONS.md files and test-docs records
- **Oracle convention (NFR3).** The reference is term_core fed the raw stream, with the removed construct's own effect excluded. It is compared with the replay view on rows, cursor and responses, the same way the sibling test modules compare.
- **Byte notation in documents.** ESC = 0x1B, DEL = 0x7F, BEL = 0x07, ST = ESC + backslash. Spaces inside a byte string are separators only.
- **Naming the predecessor decision.** Always name it by its feature, as "mux-strip-concat-query-closure D1", so it does not collide with this feature's own decision numbering in DECISIONS.md.

## Cross-task Design Decisions

None. The tests and the decision record form one task, because DECISIONS.md cites the test that pins each scenario.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The tests pass vacuously, because the behavior already exists and no red stage can be observed (REQUIREMENTS.md A6) | Medium | Medium | Sensitivity controls (task0001 AC-7) show that the replay core answers the joined query, and that the closure-less joins reset the terminal or switch G0 |
| A construct kind's own effect (an embedded C0 byte, an image construct) makes the raw reference differ in rows or cursor | Medium | Low | Exclude the construct's own effect the way the sibling modules' references already do. The compared fields are never reduced. |
| A sibling helper or the construct table is private to its module | Medium | Low | Widen its visibility only. The sibling module files are already in task0001's file set. |
| Another feature that adds a sibling test module conflicts with this one in the test module registration | Low | Low | Keep both registrations when resolving |
| The new tests exceed the 10 s per-test budget | Low | Low | Use fresh term_core instances and short inputs per case |

## Open Questions

- None.
