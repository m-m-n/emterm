# Feature: mux-strip-join-escape-closure

## Overview

The task lists three attack scenarios against the mux scrollback strip. On current main, mux-strip-concat-query-closure D1 (`Written::close_before_removal`) already defeats all three, so this feature makes no production behavior change. It adds the regression tests the DoD requires and records in this feature's DECISIONS.md how each scenario maps to D1, plus the kept-string-body join residual. Requirements document: `feature-docs/mux-strip-join-escape-closure/REQUIREMENTS.md`.

## Objectives

- Confirm on current main that the task's three attack scenarios do not succeed:
  1. `ESC[6` + removed OSC + `n` in one cut-free run.
  2. `ESC ESC ESC[6n[6n[5n` through the write strip, then the snapshot strip, then term_core.
  3. `ESC` + removed construct + `c` (RIS) and `ESC` + removed construct + `(0` (G0 designation).

  mux-strip-concat-query-closure D1 already defeats all three. `Written::close_before_removal` writes one DEL before a construct removed while the written stream is inside a CSI or right after a written lone ESC. No production behavior change is made.
- Add the regression tests the DoD requires. The existing `strip_concat_one_call_closes_an_open_csi_at_every_removed_construct` already covers DoD 1. This feature adds the DoD-2 two-stage test and a scenario-3 replay test.
- Record in this feature's DECISIONS.md how each scenario maps to D1 (DoD: follows the fixed version) and the kept-string-body join residual.

## User Stories

Not applicable: this feature adds regression tests and decision records only and has no user-facing behavior change.

### Acceptance Criteria

- [ ] AC-1 (FR1): `ESC ESC ESC[6n[6n[5n` written through one cut-free `ScrollbackWriteFilter` call is `ESC ESC DEL [6n[5n`. The snapshot strip leaves it unchanged, and replaying the snapshot payload through term_core gives zero responses.
- [ ] AC-2 (FR2): For every removed construct kind, `ESC` + construct + `c` and `ESC` + construct + `(0` + probe text, written in one cut-free call and passed through the snapshot strip, replay with no reset, no G0 switch and no response. Rows, cursor and responses equal the raw-stream reference with the construct's own effect excluded.
- [ ] AC-3 (FR3): `mux::ipc::pty_spawn::tests::strip_concat_query::strip_concat_one_call_closes_an_open_csi_at_every_removed_construct` passes and is listed in this feature's test-docs record.
- [ ] AC-4 (FR4, FR5): This feature's DECISIONS.md contains the scenario-to-D1 mapping, the superseded-mitigation note and the kept-string-body join residual with reproduction and impact (including the `snapshot_bytes.rs:490` wrap dump-block condition).
- [ ] AC-5 (NFR1, NFR2, NFR3): The `--lib` tests and the `--no-default-features` check pass. `git diff` shows no production-code change and no change to existing test expectations or names.

## Technical Requirements

### Functional Requirements
- **FR1:** DoD-2 test: write strip, snapshot strip, term_core gives zero responses. A test feeds the byte string `ESC ESC ESC [ 6 n [ 6 n [ 5 n` to a `ScrollbackWriteFilter` in one cut-free call (the write strip). It passes the written ring through the snapshot strip (`build_snapshot_bytes`, which calls `strip_rich_content_and_remap`), replays the resulting payload through a term_core `TerminalCore`, and asserts that the response buffer is empty. It also pins the written ring as `ESC ESC DEL [6n[5n` and asserts that the snapshot strip leaves those bytes unchanged. The existing reattach helper (`strip_concat_query.rs` `assert_reattach_matches_reference`) replays through the snapshot but never asserts an empty replay response, so it does not satisfy this requirement.
- **FR2:** Scenario-3 test: no RIS and no G0 designation across a removal. The test covers each removed construct kind:
  - OSC 777 launch with BEL and with ST
  - OSC 9999 emterm-md
  - agent-status report
  - Kitty APC
  - SIXEL DCS
  - answered CSI query
  - CSI query with an embedded C0 byte

  For each kind, it feeds two inputs in one cut-free `ScrollbackWriteFilter` call: `ESC` + construct + `c`, and `ESC` + construct + `(0` followed by probe text that DEC line drawing would change. Each input is written, passed through the snapshot strip and replayed through term_core. Rows, cursor and responses must equal the raw-stream reference with the construct's own effect excluded. That means no reset (earlier screen content kept), probe letters shown as typed (G0 not switched), and no response.
- **FR3:** DoD-1 test is the existing test. DoD 1 (one cut-free feed of `ESC[6` + removed construct + `n` leaves no query in the ring) is verified by the existing `mux::ipc::pty_spawn::tests::strip_concat_query::strip_concat_one_call_closes_an_open_csi_at_every_removed_construct`. This feature's test-docs record lists it under the corresponding acceptance criterion, and no duplicate test is added.
- **FR4:** Decision record: scenario-to-D1 mapping. This feature's DECISIONS.md maps scenarios 1-3 to mux-strip-concat-query-closure D1 (`Written::close_before_removal`) and to the term_core behavior that makes DEL inert: it cancels an open CSI, and right after a lone ESC it is an ignored unknown escape final. It states that the task's proposed mitigation (CAN after ESC, moving `closure_for` into `scrollback_filter`) is superseded by D1, and that `closure_for` stays the cut-only closure. The predecessor features' DECISIONS.md and test-docs records are not edited.
- **FR5:** Decision record: kept-string-body join residual. This feature's DECISIONS.md records the join inside an unterminated kept OSC/DCS/APC body as a residual, out of scope for this feature. It records the following points:
  - **Mechanism:** `WrittenState` treats a string body as ground, so a construct removed inside it writes no closing, and the bytes after the construct join the body.
  - **Reproduction:** `ESC]11;` + a removed construct (e.g. `ESC]777;emterm;markdown;x BEL` or `ESC[6n`) + `?BEL` in one cut-free call writes `ESC]11;?BEL`. The snapshot strip keeps it, and term_core dispatches OSC 11 `?` with a BEL terminator, which the color responder answers. The raw stream aborts OSC 11 at the construct's ESC (Unterminated, empty data) and gets no answer.
  - **Impact:** an OSC 11 response can still be induced. Responses produced during snapshot replay are discarded by the client (`tabs/replay.rs`), so the PTY-reply path is a later live continuation. A ring ending in `ESC]11;` + a removed construct leaves the replayed parser inside the OSC body, and a live `?BEL` is answered.
  - **Condition:** that path applies only when the main-buffer resume snapshot appends nothing after the scrollback. On ring wrap, `append_wrapped_dump_block_if_applicable` (`snapshot_bytes.rs:490`) appends a screen-restore dump block.
  - **Unchanged pinned expectations:**
    - the `ESC]0;t` row of `scrollback_filter::tests::strip_concat_a_construct_removed_in_ground_adds_no_closing`
    - the `body_head` loop of `strip_concat_query::strip_concat_a_construct_removed_in_ground_or_a_kept_string_adds_no_closing`

### Non-Functional Requirements
- **NFR1 - Test and CLI-only check pass:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes, and `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` succeeds.
- **NFR2 - No change to production code or existing expectations:** Production code under `src-tauri/src` (non-test) and `crates/term_core` is unchanged. No existing test expectation changes and no existing test is renamed, so no test-docs record of a predecessor feature needs updating.
- **NFR3 - Oracle convention:** The new tests follow the existing oracle convention: term_core fed the raw stream, with the removed construct's own effect (e.g. the answer to a removed `ESC[6n`) excluded, compared on rows, cursor and responses. They reuse the existing helpers of the `strip_concat_query` / `escape_state_carry` / `round4_cut_csi` test modules where they fit, and finish within the existing 10 s test budgets.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────┐
│  ScrollbackWriteFilter (write strip)│
├─────────────────────────────────────┤
│  scrollback ring                    │
├─────────────────────────────────────┤
│  build_snapshot_bytes               │
│  (snapshot strip,                   │
│   strip_rich_content_and_remap)     │
├─────────────────────────────────────┤
│  term_core TerminalCore (replay)    │
└─────────────────────────────────────┘
```

**Component Diagram:**
```
Test input bytes ─▶ ScrollbackWriteFilter (one cut-free call) ─▶ ring
ring ─▶ build_snapshot_bytes ─▶ snapshot payload ─▶ TerminalCore ─▶ rows / cursor / take_response()
Reference: raw stream ─▶ TerminalCore (removed construct's own effect excluded) ─▶ rows / cursor / responses
```

### Data Flow

```
Input ─▶ write strip ─▶ ring ─▶ snapshot strip ─▶ TerminalCore replay ─▶ rows, cursor, responses
                                                                          │
Raw stream ─▶ TerminalCore (construct's own effect excluded) ─────────────┴─▶ compared
```

### API Design

Not applicable: no API is added or changed.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- mux-strip-concat-query-closure D1 (`Written::close_before_removal`): the behavior the new tests pin.
- `strip_concat_query` / `escape_state_carry` / `round4_cut_csi` test modules: existing helpers reused where they fit (NFR3).
- Existing test `mux::ipc::pty_spawn::tests::strip_concat_query::strip_concat_one_call_closes_an_open_csi_at_every_removed_construct`: verifies DoD 1 (FR3).

**External Dependencies:**
- None.

### File Structure

The test file locations are derived at create-plan. Production files are unchanged (NFR2). The decision record is `feature-docs/mux-strip-join-escape-closure/DECISIONS.md` (FR4, FR5).

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-strip-join-escape-closure/**`
- `test-docs/mux-strip-join-escape-closure/**`

`feature-docs/mux-strip-join-escape-closure/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-strip-join-escape-closure/**` covers `test-docs/mux-strip-join-escape-closure/{T}.tests.yaml`, the
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
- [ ] TS-1 (AC-1): Write filter, one cut-free feed of `ESC ESC ESC[6n[6n[5n`. Assert the ring bytes, then `build_snapshot_bytes(ring, ...)` and assert the stripped scrollback equals the ring. Replay the payload in a `TerminalCore` and assert `take_response()` is empty.
- [ ] TS-2 (AC-2): Table over removed construct kinds x continuations {`c`, `(0` + probe}: one cut-free write-filter feed, snapshot strip, term_core replay. Compare with view_after_a_cut-style references (raw `ESC` + construct, then continuation) on rows, cursor and responses. Assert no reset and that the probe text is displayed unchanged.
- [ ] TS-3 (AC-3): Rerun the existing `strip_concat_one_call_closes_an_open_csi_at_every_removed_construct` (no change).

### Integration Tests
- [ ] TS-4 (AC-5, build): Run the `--lib` test command and the `--no-default-features` check.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Every removed construct kind (OSC 777 launch with BEL and with ST, OSC 9999 emterm-md, agent-status report, Kitty APC, SIXEL DCS, answered CSI query, CSI query with an embedded C0 byte) combined with each continuation (`c`, `(0` + probe text): no reset, no G0 switch, no response (TS-2).
- [ ] Removal inside an unterminated kept OSC/DCS/APC body: out of scope; recorded as a residual in DECISIONS.md, and the existing pinned expectations stay unchanged (FR5).

### Performance Tests
- [ ] The new tests finish within the existing 10 s test budgets (NFR3).

## Security Considerations

- **Authentication:** Not applicable.
- **Authorization:** Not applicable.
- **Input Validation:** TS-1 and TS-2 pin the write strip and snapshot strip behavior for scenarios 2 and 3; FR3's existing test pins scenario 1. The kept-string-body join (OSC 11 response still inducible) is recorded as a residual (FR5).
- **Data Protection:** Not applicable.
- **XSS Prevention:** Not applicable.
- **SQL Injection Prevention:** Not applicable.
- **CSRF Protection:** Not applicable.

## Error Handling

### Error Codes

Not applicable.

### Error Flow

Not applicable.

## Performance Optimization

### Performance Goals
- The new tests finish within the existing 10 s test budgets (NFR3).

### Optimization Strategies
- Not applicable.

### Caching Strategy
- Not applicable.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Documentation is complete (DECISIONS.md per FR4 and FR5)
- [ ] Code review is completed
- [ ] No production-code change and no change to existing test expectations or names (NFR2)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None.

## Implementation Phases (if applicable)

Not applicable.

## References

- Requirements document: `feature-docs/mux-strip-join-escape-closure/REQUIREMENTS.md`
- mux-strip-concat-query-closure D1 (`Written::close_before_removal`)
- `scrollback_filter.rs` `strip_pass` / `close_before_removal`
- `crates/term_core/src/parser/escape.rs`, `esc_handler.rs`
- `strip_concat_query.rs` `assert_reattach_matches_reference`
- `snapshot_bytes.rs:490` `append_wrapped_dump_block_if_applicable`
- `tabs/replay.rs`
