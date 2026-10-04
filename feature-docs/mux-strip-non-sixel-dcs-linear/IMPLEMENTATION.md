# Implementation Plan: mux-strip-non-sixel-dcs-linear

## Overview

Adds budget regression tests for the input `(ESC P x)*N` followed by one
`ESC \` (SPEC.md "the input") on every mux daemon path that scans PTY output:
the shared strip, the scrollback write filter, the two snapshot builders, the
production reader (Detached and with suppressed reads) and the client-parity
scan. Production code changes only when one of those tests shows non-linear
behavior (FR7).

## Technology Stack

- **Language / Framework**: Rust, the crate's existing cargo `#[test]` harness
  (lib tests, no `#[ignore]`).
- **Key libraries**: none added. The feature introduces no new dependency, so
  no license entry is recorded (project license MIT is unaffected).

## Layer Structure

| Layer | Location | Role in this feature |
|-------|----------|----------------------|
| Shared strip | `src-tauri/src/mux/scrollback_filter.rs` | Code under test (FR1, and inside FR3). Never depends on the IPC layer |
| Write path | `src-tauri/src/mux/ipc/pty_spawn/` (write filter, reader, suppressed replacement, client-parity scan) | Code under test (FR2, FR4, FR5, FR6) |
| Snapshot assembly | `src-tauri/src/mux/snapshot_bytes.rs` | Code under test (FR3) |
| Test tree | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` and its child modules under `tests/` | Hosts the new tests; reaches every entry point above (all are visible inside `crate::mux`) and the existing reader harnesses |

Dependency direction: tests depend on the code under test; no production
module depends on test code. A production change under FR7 keeps the existing
direction (the shared strip stays independent of the IPC layer).

## Shared Components

None. The feature has a single task; it reuses existing test harnesses, which
its task plan names.

## Conventions

- **The input**: a repeated three-byte unit (`ESC`, `P`, `x`) followed by one
  ST (`ESC \`). The DCS body `x` is not SIXEL, so no path removes any byte of
  it: the expected output of every path is the input itself (FR1, NFR4).
- **Budget**: every budget assertion compares wall-clock time with the
  existing 10 s budget constant shared inside the pty_spawn test tree; no new
  budget value is introduced (NFR1, SPEC A5).
- **Sizes**: 2 MiB for the strip, snapshot and direct-scan calls; at least the
  512 KiB pending-cap range on the write path; reader reads of at most 65,536
  bytes (NFR1, SPEC A4).
- **Test identity**: each test's doc comment names the acceptance criterion,
  FR and TM-n it verifies, as the existing feature test modules do.
- **Failure messages**: each assertion message carries the case name; each
  budget assertion message carries the elapsed time.
- **Record**: the test-docs record follows FR8 (`red_confirmed: false`, the
  FR8 `red_reason`).

## Cross-task Design Decisions

### D1: Regression guard first, production change only on evidence

- **Decision**: the tests are written to pass at HEAD (SPEC A1, A3).
  Production code is changed only when a test of FR1-FR6 exceeds the budget or
  otherwise shows non-linear growth. Such a change makes that path linear and
  keeps every strip-target decision: a non-SIXEL DCS stays byte for byte and a
  SIXEL DCS is removed as before (FR7, NFR4).
- **Rationale**: SPEC A1 records that every scan on these paths stops at the
  first `ESC` at HEAD.
- **Affected tasks**: task0001.

### D2: One task

- **Decision**: FR1-FR8 are one task.
- **Rationale**: every test shares one input generator, one test module, one
  module registration and one test-docs record; parallel tasks would edit the
  same registration file and duplicate the generator.
- **Affected tasks**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A load-dependent overrun of a budget test unrelated to this feature (SPEC A5) | Medium | Low | Treated as no regression signal when the test passes on a re-run with no code change; noted in the test-docs record |
| The suppressed-reads harness delivers more chunks than its capacity-16 channel holds before join, so the reader blocks and the run never ends | Medium | Medium | The suppressed-read case keeps its total delivered chunk count within the channel capacity (NFR1, SPEC A4) |
| A FR7 linearity fix changes a strip-target decision | Low | High | The new tests assert the non-SIXEL input is written unchanged; every existing strip-target test stays green (TM-2) |

## Open Questions

- None.
