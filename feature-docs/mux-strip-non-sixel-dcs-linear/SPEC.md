# Feature: mux-strip-non-sixel-dcs-linear

## Overview

This feature adds budget tests for a stream of repeated non-SIXEL DCS
introducers `(ESC P x)*N` followed by a trailing `ESC \` (called "the input"
below) on the shared strip, the scrollback write filter, the snapshot
builders, the production reader and the client-parity scan. Production code
changes only when a test shows non-linear behavior. Requirements are defined
in `REQUIREMENTS.md`.

## Objectives

- The shared strip, the scrollback write filter, the snapshot builder and the
  client-parity scan finish in time linear in the input length for the input.
- Regression tests detect any return of quadratic scanning for that input
  shape on the write path (write filter and production reader, including the
  suppressed-delivery scan) and on the snapshot path.

## User Stories

Not applicable. The feature adds regression tests for byte-stream processing
in the mux daemon and touches no UI.

**Acceptance Criteria:**
- [ ] AC-1 (FR1, NFR1): The shared strip entry points finish within 10 s on
  the 2 MiB input and return it unchanged.
- [ ] AC-2 (FR2, NFR1): The write filter finishes within 10 s in the one-call,
  piecewise and overflow cases; its emitted bytes equal the input and pending
  is empty at the end.
- [ ] AC-3 (FR3, NFR1): Both snapshot builders finish within 10 s on the 2 MiB
  scrollback; the scrollback part equals the input and the segment offsets
  are non-decreasing and within the payload.
- [ ] AC-4 (FR4, NFR1): The Detached production reader finishes within 10 s
  and its ring equals the input.
- [ ] AC-5 (FR5, FR6, NFR1): `client_parity_scan::scan` called directly
  finishes within 10 s with and without excluded pieces and reports no item;
  the production reader with suppressed reads runs both replacement branches
  within 10 s, its ring equals the input and it sends no empty non-EOF
  PtyOutput chunk.
- [ ] AC-6 (FR7, NFR4): No production code changes when AC-1 to AC-5 pass;
  when a change is needed, AC-1 to AC-5 pass after it and every existing test
  stays green.
- [ ] AC-7 (FR8): The test-docs record lists the tests with
  `red_confirmed: false` and the `red_reason` of FR8.
- [ ] AC-8 (NFR2, NFR3): The full `--lib` run passes and the
  `--no-default-features` cargo check succeeds.

## Technical Requirements

### Functional Requirements
- **FR1:** Shared strip budget test. A test runs the shared strip entry
  points (`strip_replayable_rich_content`,
  `strip_pty_output_for_scrollback_write`, `strip_rich_content_and_remap`
  with watch offsets, and
  `strip_pty_output_for_scrollback_write_with_written_state` started in
  ground) on `(ESC P x)*N` + `ESC \` sized to the 2 MiB ring. Each call
  finishes within the budget and returns the input unchanged byte for byte
  (no SIXEL DCS is present, so nothing is removed).
- **FR2:** Write filter budget test. A test feeds `ScrollbackWriteFilter` the
  input (a) in one call with the held run just under
  `SCROLLBACK_FILTER_PENDING_CAP` (512 KiB), (b) in reader-sized pieces of at
  most 65,536 bytes with the held chain just under the cap, and (c) past the
  cap so the overflow flush runs. Each case finishes within the budget, the
  emitted bytes concatenated equal the input, and pending is empty after the
  trailing ST.
- **FR3:** Snapshot path budget test. A test builds a snapshot
  (`build_snapshot_bytes` and `build_resume_snapshot_bytes`) from a 2 MiB
  scrollback of `(ESC P x)*N` + `ESC \` with dimension segments. Each
  finishes within the budget, the scrollback part of the payload equals the
  input, and the mapped segment offsets are non-decreasing and within the
  payload.
- **FR4:** Production reader (Detached) budget test. A test drives the input
  through the production reader on a pane with no connected owner
  (`run_reader_without_owner`) in reads of at most 65,536 bytes, covering the
  pending-cap range. The run finishes within the budget and the ring equals
  the input.
- **FR5:** Client-parity scan budget test. A test calls
  `client_parity_scan::scan` directly with the input as a long chunk
  (2 MiB), with no excluded pieces and with excluded pieces. Each call
  finishes within the budget and reports no item; with no excluded pieces
  and a trailing complete ST it reports no tail.
- **FR6:** Production reader with suppressed reads budget test. A test drives
  the input through the production reader with suppressed reads
  (`run_reader_with_suppressed_reads`) so both branches of
  `prepare_suppressed_replacement` run: a suppressed read while the write
  filter holds the chain (non-empty pending, scan with excluded pieces,
  `suppressed_output.rs:246`) and a suppressed read after the trailing ST
  (empty pending, scan with no excluded pieces, `suppressed_output.rs:250`).
  The run finishes within the budget, the ring equals the input, and no
  PtyOutput chunk other than the EOF marker is empty.
- **FR7:** Production code change only on a non-linear finding. Production
  code is changed only when a test of FR1-FR6 shows non-linear behavior.
  Such a change makes the affected path linear and keeps every strip-target
  decision unchanged (a non-SIXEL DCS stays byte for byte; a SIXEL DCS is
  removed as before).
- **FR8:** Test-docs record.
  `test-docs/mux-strip-non-sixel-dcs-linear/taskNNNN.tests.yaml` lists the
  new tests per acceptance criterion with `red_confirmed: false` and a
  `red_reason` stating that the quadratic scan was already resolved before
  this feature (`scan_body_end` and `find_st` stop at the first ESC), so the
  tests are regression guards that were green from the start.

### Non-Functional Requirements
- **NFR1 - Budget and input size:** Each budget assertion uses the existing
  10 s budget convention. Input lengths are chosen so a quadratic scan would
  exceed the budget by orders of magnitude: at least the 512 KiB pending-cap
  range on the write path and 2 MiB on the strip, snapshot and direct scan
  calls. Reader-level cases are bounded by the 65,536-byte read buffer and,
  for suppressed reads, by the harness's capacity-16 output channel.
- **NFR2 - Test style:** Tests use the existing cargo `#[test]` harness
  without `#[ignore]`, with no new dependency, in the style of the existing
  budget tests.
- **NFR3 - Build and test verification:** The full `--lib` test run and the
  `--no-default-features` cargo check succeed.
- **NFR4 - Behavior preservation:** Strip-target decisions and the written
  bytes of every existing path stay unchanged.

## Implementation Approach

### Architecture

**Code paths under test:**
```
Shared strip (scrollback_filter.rs)
  strip_replayable_rich_content
  strip_pty_output_for_scrollback_write
  strip_rich_content_and_remap (watch offsets)
  strip_pty_output_for_scrollback_write_with_written_state (from ground)
  -> scan_body_end (scrollback_filter.rs:875)

Write filter (write_filter.rs)
  ScrollbackWriteFilter (SCROLLBACK_FILTER_PENDING_CAP = 512 KiB)
  -> find_st (write_filter.rs:1069)

Snapshot builders
  build_snapshot_bytes / build_resume_snapshot_bytes

Production reader (pty_reader_loop, read buffer 65,536 bytes, mod.rs:339)
  run_reader_without_owner        (Detached; never reaches client_parity_scan)
  run_reader_with_suppressed_reads
    -> prepare_suppressed_replacement
         non-empty pending -> scan with excluded pieces (suppressed_output.rs:246)
         empty pending     -> scan with no excluded pieces (suppressed_output.rs:250)

Client-parity scan (client_parity_scan.rs)
  client_parity_scan::scan
  -> find_st_terminator (client_parity_scan.rs:405)
```

### Assumptions

- **A1:** The quadratic path described in the task (`find_st_terminator` in
  `scrollback_filter.rs:669-683`) no longer exists at HEAD: the shared strip
  uses `scan_body_end` (`scrollback_filter.rs:875`), which stops at the first
  ESC; the non-SIXEL DCS fallback (`scrollback_filter.rs:774-792`) writes one
  ESC and the next scan stops at the next ESC. The write filter's `find_st`
  (`write_filter.rs:1069`) and the client-parity `find_st_terminator`
  (`client_parity_scan.rs:405`) also stop at the first ESC.
- **A2:** Scope per answer `requirement.write-path-test-scope`
  (`filter_and_reader`): the write-path tests cover `ScrollbackWriteFilter`
  and the production reader, and `client_parity_scan::scan` is exercised both
  by direct calls (for the long input) and through
  `run_reader_with_suppressed_reads` (both branches at
  `suppressed_output.rs:246`/`250`), because `run_reader_without_owner` is
  Detached and never reaches `client_parity_scan`.
- **A3:** Per answer `requirement.red-phase-handling`
  (`regression_guard_green`): the new tests are added as regression guards
  expected to pass at HEAD, recorded with `red_confirmed: false`, following
  `test-docs/mux-snapshot-strip-can-abort/task0001.tests.yaml` (AC-4 / AC-6
  entries).
- **A4:** Reader-level reads are at most 65,536 bytes (`pty_reader_loop`
  buffer, `mod.rs:339`), and `run_reader_with_suppressed_reads` uses a
  capacity-16 channel with no consumer until join, which bounds the number of
  reads in the suppressed case; the orders-of-magnitude size requirement
  applies to the strip, write-filter, snapshot and direct-scan cases.
- **A5:** The 10 s budget follows the existing `BUDGET` constants of the
  pty_spawn test modules and the TS-9 convention of
  mux-strip-concat-query-closure; a load-dependent overrun of an unrelated
  budget test has been recorded before and is not a regression signal for
  this feature.
- **A6:** No symbol or test is deleted or renamed, so
  `.claude/rules/test-docs-records.md`'s rename duty does not apply.

### Dependencies

**Internal Dependencies:**
- Shared strip, `ScrollbackWriteFilter`, snapshot builders, production reader
  harnesses and `client_parity_scan`: the code paths the tests exercise.

**External Dependencies:**
- None. No new dependency is added (NFR2).

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from every
task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths:

- `feature-docs/mux-strip-non-sixel-dcs-linear/**`
- `test-docs/mux-strip-non-sixel-dcs-linear/**`

`feature-docs/mux-strip-non-sixel-dcs-linear/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-strip-non-sixel-dcs-linear/**` covers
`test-docs/mux-strip-non-sixel-dcs-linear/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

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
- [ ] TS-1 (AC-1): Shared strip entry points on `(ESC P x)*N` + `ESC \` of
  2 MiB - under 10 s, output equals input.
- [ ] TS-2 (AC-2): `ScrollbackWriteFilter`: one call just under the cap,
  pieces of at most 65,536 bytes just under the cap, and past the cap
  (overflow flush) - under 10 s, emitted equals input, pending empty.
- [ ] TS-3 (AC-3): `build_snapshot_bytes` / `build_resume_snapshot_bytes`
  with a 2 MiB scrollback of the form and segments - under 10 s, scrollback
  part equals input, offsets monotone and within the payload.
- [ ] TS-5 (AC-5): `client_parity_scan::scan` direct calls on a 2 MiB chunk
  with and without excluded pieces - under 10 s, no items.

### Integration Tests
- [ ] TS-4 (AC-4): `run_reader_without_owner` with the input in 65,536-byte
  reads over the cap range - under 10 s, ring equals input.
- [ ] TS-6 (AC-5): `run_reader_with_suppressed_reads` with one suppressed
  read while the chain is held and one after the trailing ST - under 10 s,
  ring equals input, only the EOF PtyOutput chunk is empty.
- [ ] TS-7 (AC-6, AC-8): Full `--lib` run and `--no-default-features` cargo
  check.
- [ ] TS-8 (AC-7): The test-docs record names every new test, with
  `red_confirmed: false` and the stated `red_reason`.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] Write filter held run just under `SCROLLBACK_FILTER_PENDING_CAP` and
  past the cap (overflow flush) (FR2).
- [ ] Suppressed read while the write filter holds the chain and after the
  trailing ST (FR6).
- [ ] `client_parity_scan::scan` with no excluded pieces and a trailing
  complete ST reports no tail (FR5).

### Performance Tests
- [ ] Every case in TS-1 to TS-6 finishes within the 10 s budget with the
  input sizes of NFR1.

## Security Considerations

Not applicable.

## Error Handling

Not applicable.

## Performance Optimization

### Performance Goals
- Each budget assertion finishes within 10 s (NFR1).

### Optimization Strategies
- Only when a test of FR1-FR6 shows non-linear behavior: make the affected
  path linear while keeping every strip-target decision unchanged (FR7).

## Success Criteria

- [ ] AC-1 to AC-8 pass.
- [ ] No production code changes when AC-1 to AC-5 pass (AC-6).

## Open Questions

None.

## References

- Requirements document: `feature-docs/mux-strip-non-sixel-dcs-linear/REQUIREMENTS.md`
- Regression-guard record format:
  `test-docs/mux-snapshot-strip-can-abort/task0001.tests.yaml`
- Test-docs record rule: `.claude/rules/test-docs-records.md`
