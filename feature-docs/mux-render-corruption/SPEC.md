# Feature: mux Render Corruption Fix (line-content mixing after window/tab switch)

## Overview

Claude Code running inside eMterm's mux intermittently shows corrupted output
after a mux window/tab switch: content from different logical lines appears
merged into a single displayed line. The corruption is confined to the
affected window (no cross-window leakage). This feature identifies the root
cause and fixes it, including determining whether it shares a root cause with
the known unfixed replay coordinate-drift bug documented in
`tmp/apt-progress-bar-regression-2026-07-09.md`.

## Objectives

- Identify the root cause of line-content mixing after window/tab switch
- Determine the relationship to the known resize-during-output scrollback
  line-count mixing / fixed-row replay coordinate-drift bug (PROBE D)
- Fix the root cause and add regression tests reproducing it

## Observed Facts

- Environment: Claude Code inside eMterm mux
- Trigger: occurs after window/tab switches, but not deterministically
- Symptom: contents of distinct lines rendered merged into one line
- Not reproduced/checked outside mux; noticed recently

## Technical Requirements

### Functional Requirements

- **FR1:** Investigate and identify the root cause of the post-switch render
  corruption. The investigation must explicitly conclude whether it is the
  same root cause as the known replay coordinate-drift bug
  (`tmp/apt-progress-bar-regression-2026-07-09.md`) or a distinct defect.
- **FR2:** Fix the identified root cause so that snapshot replay after a
  window/tab switch reproduces the pre-switch screen content without mixing
  content from distinct lines. If FR1 concludes the known coordinate-drift
  bug is the same root cause, fix it as part of this work.
- **FR3:** Add unit/integration tests that reproduce the root-cause scenario
  (failing before the fix, passing after), including a scenario where the
  scrollback contains lines produced at different terminal widths (resize
  interleaved with output).

### Non-Functional Requirements

- **NFR1 - Performance:** The fix must not noticeably degrade mux window
  switch / reattach latency.
- **NFR2 - Compatibility:** Linux and Windows builds both keep compiling and
  passing tests (`cargo check --no-default-features` stays green).
- **NFR3 - Regression Safety:** All existing Rust and TypeScript tests pass.

## Implementation Approach

### Suspect Area (from prior investigations)

The mux snapshot/replay pipeline:

- Daemon side: scrollback accumulation and snapshot assembly
  (`src-tauri/src/mux/` daemon code; scrollback is a 2 MiB byte buffer;
  snapshot is delivered as `MessageType::PtyOutput` and replayed by the GUI)
- GUI side: replay application into `term_core` grid on window switch /
  reattach
- Known related mechanism: bytes emitted at different terminal widths coexist
  in scrollback; replay assumes a fixed row count, causing coordinate drift
  (PROBE D evidence in `tmp/apt-progress-bar-regression-2026-07-09.md`)

The investigation task decides the actual fix layer (daemon snapshot
assembly vs GUI replay) based on evidence; this spec does not prescribe it.

### Dependencies

**Internal Dependencies:**
- `crates/term_core`: ANSI parser + grid the replay is applied to
- `src-tauri/src/mux/`: daemon, snapshot assembly, client bridge
- `crates/mux_ipc`: mux protocol types

## Test Scenarios

### Unit Tests

- [ ] Replay of a snapshot containing resize-interleaved scrollback restores
      lines without cross-line content mixing
- [ ] Window-switch snapshot round-trip: grid content before switch equals
      grid content after replay

### Integration Tests

- [ ] Existing `tabs.rs` replay tests keep passing (run with
      `--test-threads=1` per project testing notes)

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Final on-device confirmation (repeated mux window switches show no
      corruption) is performed manually by the user

## Error Handling

Not applicable beyond existing logging: any diagnostic logging added during
investigation must use `warn` or higher to be visible in release logs, and
temporary probes are removed before completion.

## Success Criteria

- [ ] Root cause identified and its relationship to the known
      coordinate-drift bug documented
- [ ] Regression tests added and passing
- [ ] All existing tests pass (Rust + TypeScript)
- [ ] User performs final on-device verification

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。

- None (investigation uncertainty is inherent to FR1, not an unresolved
  requirement)

## References

- Requirements: `feature-docs/mux-render-corruption/REQUIREMENTS.md`
- Known-bug report: `tmp/apt-progress-bar-regression-2026-07-09.md` (main
  working tree, gitignored — investigation reads it from there)
