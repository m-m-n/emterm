# Feature: osc133-mark-token-exact-match

## Overview

The plain-tab OSC 133 mark candidate that `NativeCallbacks::handle_semantic_prompt`
(`src-tauri/src/callbacks.rs`) pushes to the OSC 7501 feed and the agent-status
latch feed is decided by the same criterion as term_core's live prompt marks
(`parse_osc133_mark` in `crates/term_core/src/osc_handler.rs`): the first
`;`-separated token of the payload equals exactly `A`, `B`, `C` or `D`.
A malformed OSC 133 token (for example `133;AB`, `133;Ax`) no longer consumes a
later well-formed mark's live mark.

## Objectives

- The plain-tab OSC 133 mark candidate pushed by `handle_semantic_prompt` to the
  OSC 7501 feed and the agent-status latch feed is decided by the same criterion
  as `parse_osc133_mark`: the first `;`-separated token of the payload equals
  exactly `A`, `B`, `C` or `D`.
- A malformed OSC 133 token (for example `133;AB`, `133;Ax`) no longer consumes a
  later well-formed mark's live mark, so the OSC 7501 record table and the
  agent-status latch follow the well-formed marks only.

## User Stories

None defined for this feature. The acceptance criteria are listed below.

**Acceptance Criteria:**
- [ ] AC-1 (FR1, FR2): Feeding `\x1b]133;AB\x07\x1b]7501;id=job:state=working\x07\x1b]133;A\x07`
  to a plain tab in one call leaves no `job` record in the tab's OSC 7501 record
  table.
- [ ] AC-2 (FR1, FR2): For each malformed first token (`AB`, `Ax`, `Bx`, `Cx`,
  `Dx`, `D0`), an OSC 133 delivered with the main screen and with the alternate
  screen leaves both `pending_program_status_feed` and `pending_latch_feed` empty.
- [ ] AC-3 (FR3): `A`, `B`, `C`, `D`, `D;0`, `D;x` and `A;aid=1` delivered on the
  main screen each push exactly one candidate of the matching kind to each feed;
  the existing callbacks and output-pipeline tests for OSC 133 candidates keep
  passing.
- [ ] AC-4 (NFR1): The full `--lib` test suite of src-tauri and term_core passes,
  and the `--no-default-features` check compiles.

## Technical Requirements

### Functional Requirements
- **FR1:** Mark kind decided by exact first-token match.
  `handle_semantic_prompt` (`src-tauri/src/callbacks.rs`) decides the OSC 133 mark
  kind from the payload's first `;`-separated token, and recognizes a kind only
  when that token is exactly `A`, `B`, `C` or `D` — the same criterion as
  `parse_osc133_mark` in `crates/term_core/src/osc_handler.rs`. Any other first
  token (for example `AB`, `Ax`, `Bx`, `Cx`, `Dx`, `D0`, empty) is an
  unrecognized kind.
- **FR2:** Unrecognized kind pushes nothing to either feed.
  An OSC 133 whose kind is unrecognized under FR1 pushes no candidate to
  `pending_program_status_feed` (OSC 7501 feed) and no candidate to
  `pending_latch_feed` (agent-status latch feed), on either screen, through both
  `on_osc_with_screen` and the existing `on_osc` notification.
- **FR3:** Well-formed marks keep their current behavior.
  A payload whose first token is exactly `A`, `B`, `C` or `D` keeps its current
  behavior: on the main screen it pushes exactly one candidate of the matching
  kind to each feed, regardless of any further `;`-separated fields (`D;0`,
  `D;42`, `D;x`, `A;aid=1`); on the alternate screen it pushes nothing.

### Non-Functional Requirements
- **NFR1 - term_core behavior unchanged:** term_core's live prompt-mark recording
  (`parse_osc133_mark`, `push_pending_prompt_mark`) and the `on_osc(133, …)` /
  `on_osc_with_screen` notification delivery are unchanged.

### Assumptions
- A1: The task description's `src-tauri/src/callbacks.rs:1232` refers to
  `handle_semantic_prompt`, which is at line 1241 at base revision 089c7417.
- A2: The fix covers both plain-tab feeds (OSC 7501 feed and agent-status latch
  feed), since `handle_semantic_prompt` pushes the same candidate to both and both
  are reconciled front-to-back against term_core's live marks.
- A3: Whether the callback reuses term_core's parser or mirrors its criterion
  locally is an implementation choice left to the plan; the SPEC fixes only the
  criterion.
- A4: Existing tests pin the current well-formed behavior
  (`osc_133_every_recognized_kind_follows_the_screen_rule_and_an_unrecognized_one_pushes_nothing`,
  `program_status_feed_preserves_true_order_of_reports_marks_and_resets`,
  `test_parse_osc133_mark_kinds`, `osc7501_prompt_start_*`); they stay unchanged.

## Implementation Approach

### Architecture

**Component Diagram:**
```
term_core (crates/term_core)
  parse_osc133_mark / push_pending_prompt_mark   -> live prompt marks   (unchanged, NFR1)
  on_osc(133, …) / on_osc_with_screen            -> notification        (unchanged, NFR1)
        |
        v
NativeCallbacks::handle_semantic_prompt (src-tauri/src/callbacks.rs)
  first ';' token == A|B|C|D ?  (FR1)
        |-- yes, main screen  -> one candidate to each feed (FR3)
        |-- yes, alt screen   -> nothing (FR3)
        '-- no                -> nothing (FR2)
        |
        v
pending_program_status_feed (OSC 7501 feed)
pending_latch_feed          (agent-status latch feed)
```

### Data Flow

```
OSC 133 payload → term_core live mark (parse_osc133_mark)
               → on_osc / on_osc_with_screen → handle_semantic_prompt
               → candidate (only for exact A/B/C/D) → OSC 7501 feed / latch feed
               → front-to-back reconciliation against term_core's live marks
```

### API Design

Not applicable.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `crates/term_core` (`parse_osc133_mark`): the criterion FR1 matches.

**External Dependencies:**
- None.

### File Structure

```
src-tauri/src/
├── callbacks.rs                    # handle_semantic_prompt (FR1, FR2, FR3)
├── callbacks/tests.rs              # TS-2, TS-3
└── tabs/tests/output_pipeline.rs   # TS-1
crates/term_core/src/
└── osc_handler.rs                  # parse_osc133_mark (unchanged, NFR1)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/osc133-mark-token-exact-match/**`
- `test-docs/osc133-mark-token-exact-match/**`

`feature-docs/osc133-mark-token-exact-match/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/osc133-mark-token-exact-match/**` covers
`test-docs/osc133-mark-token-exact-match/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites
it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/osc133-mark-token-exact-match/` directory at all; the declared
`test-docs/osc133-mark-token-exact-match/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (AC-1; FR1, FR2) — `src-tauri/src/tabs/tests/output_pipeline.rs`:
  Fresh plain test tab; `process_combined` with the reproduction bytes in one
  call - the OSC 7501 record table holds no `job` record (is empty).
- [ ] TS-2 (AC-2; FR1, FR2) — `src-tauri/src/callbacks/tests.rs`:
  For each malformed token and each screen state,
  `on_osc_with_screen(OSC_SEMANTIC_PROMPT, token, alt)` - both feeds are empty.
  Also through `on_osc(OSC_SEMANTIC_PROMPT, token)`.
- [ ] TS-3 (AC-3; FR3) — `src-tauri/src/callbacks/tests.rs`:
  For `A`, `B`, `C`, `D`, `D;0`, `D;x`, `A;aid=1` on the main screen - exactly one
  candidate of the matching kind in each feed.

### Integration Tests
- [ ] TS-4 (AC-4; NFR1) — suite:
  - `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  - `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
  - `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] Malformed first tokens `AB`, `Ax`, `Bx`, `Cx`, `Dx`, `D0` and an empty first
  token are unrecognized kinds and push nothing to either feed (FR1, FR2).
- [ ] Well-formed first tokens with further `;`-separated fields (`D;0`, `D;42`,
  `D;x`, `A;aid=1`) push exactly one candidate of the matching kind on the main
  screen (FR3).
- [ ] Well-formed marks on the alternate screen push nothing (FR3).

### Performance Tests

Not applicable.

## Security Considerations

Not applicable.

## Error Handling

Not applicable.

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None.

## References

- term_core OSC 133 parser: `crates/term_core/src/osc_handler.rs` (`parse_osc133_mark`)
- Callback under change: `src-tauri/src/callbacks.rs` (`handle_semantic_prompt`)
