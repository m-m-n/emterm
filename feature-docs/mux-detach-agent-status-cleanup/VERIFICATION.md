# Verification Document: mux-detach-agent-status-cleanup

## Overview

**Feature**: mux-detach-agent-status-cleanup
**SPEC.md**: `feature-docs/mux-detach-agent-status-cleanup/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-detach-agent-status-cleanup/IMPLEMENTATION.md`

This document defines the INTEGRATED verification of the feature. Per-task
acceptance criteria live in `feature-docs/mux-detach-agent-status-cleanup/tasks/task0001.md`.

## Build Verification

Component `rust` (GUI build, default features):

```
bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml
```

Component `rust-cli` (CLI-only build, verifies the feature gates still
compile — NFR5):

```
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

- Expected: exit code 0, no errors, no new warnings attributable to this
  feature.

## Test Verification

Component `rust`:

```
bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
```

- Expected: exit code 0. The whole suite passes, not only the new scenarios —
  in particular the existing per-tab scoping tests from the
  mux-agent-status-pane-key-collision work and the detach-driving overlay
  tests must remain green (SPEC AC-7).
- Coverage target: no coverage tool is configured for this project
  (`project.components` declares no coverage command), so coverage is not a
  numeric gate here. The gate is the scenario mapping below: every scenario
  present and passing.
- The single-threaded flag is part of the configured command and is not
  optional: some replay tests in this crate are order-sensitive.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | A mux-attached tab reports a pane state and learns its public id; a daemon-confirmed detach frame is delivered through the application's mux-message entry point and the app is pumped | The tab's aggregated badge reports nothing for that pane, the public-pane-id lookup for (that tab's scope, that wire pane id) returns nothing, and the pane's notification rate-limit record is released | Unit |
| TS-2 | TS-1 continued: a fresh attach on the SAME tab reusing the same wire pane id, with no status update from the new connection yet | Tab badge and per-pane badge both report nothing — no state and no agent name carried over from the previous connection | Unit |
| TS-3 | A mux-attached tab whose OWN plain-tab key holds a state is detached and the app is pumped | That plain-tab entry still reports its state, and the per-tab inferred-clear latch is intact | Unit |
| TS-4 | Two tabs whose groups both hold wire pane id 1; the first tab is detached | The second tab's model entry, public-pane-id mapping and derived notification rate-limit key are unchanged | Unit |
| TS-5 | A daemon-confirmed detach frame is applied to a tab holding a seeded group (tab layer, no app) | The closed-agent-status-pane drain returns exactly the group's wire pane ids; a second drain returns an empty list | Unit |
| TS-6 | A pane-exit sequence that removes every window of a group (tab layer, no app) | Each removed wire pane id is yielded exactly once by the drain — no double push from the detach-side queueing | Unit |

## Code Quality Verification

- Format: no format command is configured for either component in
  `project.components`; formatting is enforced by the project's own editor
  hook rather than by a verification command, so nothing is run here.
- Static analysis: none beyond the compiler's own diagnostics produced by the
  two build commands above.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Detach clears the discarded panes' badge contribution and public-pane-id mapping | TS-1, TS-5 |
| AC-2 | Re-attach on the same tab shows nothing from the previous connection until the new one reports | TS-2 |
| AC-3 | Detach releases each discarded pane's notification rate-limit identity | TS-1 |
| AC-4 | The tab's own plain-tab entry and inferred-clear latch survive a detach | TS-3 |
| AC-5 | A detach on one tab never touches another tab's identically-numbered wire pane | TS-4 |
| AC-6 | The connection-scope doc comment no longer claims entries survive detach and re-attach | Read the corrected comment in the diff; confirm it states the value-constant / entries-released pair |
| AC-7 | The full library suite passes, including the pre-existing scoping and overlay tests | The Test Verification command above, exit code 0 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-5, TS-6 — the detach path now discards; the pane-exit path and the tab-death paths are confirmed unchanged and still discarding |
| FR2 | task0001 | TS-1, TS-5, TS-6 — plus a diff review confirming no second teardown routine was added |
| FR3 | task0001 | TS-1, TS-4 — all three stores asserted per discarded pane, with the rate-limit identity resolved before the mapping is removed |
| FR4 | task0001 | TS-2, plus a diff review confirming every scope derivation site is unchanged and the doc comment is corrected |
| FR5 | task0001 | TS-3 |
| FR6 | task0001 | TS-1, TS-2 |
| NFR1 | task0001 | TS-1, TS-2 — the release at detach is what bounds the three maps by the currently-live panes; no separate load test (SPEC: asserted structurally) |
| NFR2 | task0001 | No TS-level scenario. Verified by change-set containment: the diff touches only GUI-gated modules under `src-tauri/src`, and neither the mux wire-protocol crate nor any settings surface appears in it |
| NFR3 | task0001 | TS-4 |
| NFR4 | task0001 | TS-5, plus a diff review confirming the tab layer gained no mutable application borrow and still communicates through the latch |
| NFR5 | task0001 | No TS-level scenario. Verified by the CLI-only build command under Build Verification |

## E2E Testing

None. `project.components` declares no E2E command for either component, and
the project has no E2E harness covering the GUI state layer. The scenarios
above are the automated coverage.

## Manual Testing (E2E Not Possible)

The reported repro depends on two real daemons and on human reading of the
tab badge, which no automated scenario in this project can reproduce. It is
performed against a release build.

- [ ] MT-1 (the reported repro): open a tab, connect to host A over SSH and
      attach to a mux session; start an agent in one window and drive it to a
      reported state; detach; in the SAME tab connect to host B over SSH and
      attach; with no agent started on host B, confirm the tab badge and the
      mux sidebar show nothing carried over from host A.
- [ ] MT-2 (single host, same tab): attach, drive a pane to a reported state,
      detach, re-attach to the same daemon, and confirm the badge is empty
      until the agent reports again.
- [ ] MT-3 (plain-tab status survives): on a mux-attached tab whose shell also
      reports its own status, detach and confirm the tab's own status is
      still shown after the tab reverts to a plain tab.
- [ ] MT-4 (notification not suppressed): after MT-2's re-attach, confirm the
      first qualifying transition on the re-used pane still raises its
      notification rather than being swallowed by the previous connection's
      rate-limit record.

No mockup comparison item applies: the design step was skipped, and this
feature introduces no new or changed visual surface.

## Performance / Security Verification

- NFR1 (bounded state growth): no load or stress harness is used. The bound
  follows from the release happening at detach, which TS-1 and TS-2 assert
  directly. A reviewer confirms the release is unconditional on the detach
  path rather than gated on a setting or on the tab being active.
- Security: the SPEC records no security requirement for this feature. The
  change adds no input parsing, no new external surface and no wire-protocol
  change, so no security-specific verification is defined.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 6 | 6 | 0 | 0 |
| Success criteria | 7 | 6 | 0 | 1 (AC-6, by diff reading) |
| Requirements | 11 | 9 | 0 | 2 (NFR2, NFR5 — by build and change-set review) |
| Manual scenarios | 4 | 0 | 0 | 4 |
