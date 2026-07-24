# Verification Document: mux Agent Status & Agent-Facing API

## Overview
**Feature**: mux-agent-status-api /
**SPEC.md**: `feature-docs/mux-agent-status-api/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-agent-status-api/IMPLEMENTATION.md`

## Build Verification
- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli-only): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (mux-ipc): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/mux_ipc/Cargo.toml`
- Command (web): `bun run typecheck`
- Expected: exit code 0, no errors

## Test Verification
- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (mux-ipc): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/mux_ipc/Cargo.toml --lib`
- Command (web): `bun test`
- Note: tabs.rs replay tests are order-sensitive; on flaky failures re-run
  with the approved `--test-threads=1` variant.
- Coverage target: new logic covered by the task ACs below; no numeric
  coverage gate.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | agent-status OSC parsing: 4 states, name decode/sanitize/truncate, clear, invalid/duplicate/unknown-key inputs | Valid → correct event; invalid → whole-sequence rejection, nothing mutated | Unit |
| TS-2 | Revision semantics: set/clear/same-state re-report increment; rejection leaves untouched; lifecycle discard | Monotonic revision; discard on PtyExited/destroy | Unit |
| TS-3 | Snapshot/reattach: OSC stripped from replay bytes; stateful panes re-synced with replay_derived=true; no transition events | State restored, notifications silent, snapshot format unchanged | Integration |
| TS-4 | mux_ipc: new message round-trips; existing message bytes unchanged; PROTOCOL_VERSION bumped once | Full payload equality; legacy codec tests untouched and green | Unit |
| TS-5 | AgentStatusModel: aggregation priority, seen/unseen transitions, counts, plain-tab parity, close discards | Matches FR6/FR7 contract | Unit |
| TS-6 | Badge/summary formatting helpers: priority → dot form (filled/ring), role-color selection, group ordering, zero-count omission, hidden-when-empty | Matches DESIGN-pinned conventions | Unit |
| TS-7 | Notification gating: transition-only, visibility, settings gates, per-pane rate limit | Fires exactly per FR9 matrix | Unit |
| TS-8 | read: tail-N rendered rows, ANSI strip, N/byte caps, unknown pane | Correct text and errors | Unit |
| TS-9 | send: verbatim bytes, no implicit Enter, NUL/oversize rejection, watermark response | Pre-write watermark returned; rejected input writes nothing | Unit |
| TS-10 | wait: level-trigger, --after filtering, timeout code, pane_gone, waiter discard on disconnect | Matches FR12 semantics | Unit |
| TS-11 | `emterm agent-status` CLI: stdout sequence, tmux DCS wrapping, usage errors | Byte-exact sequence; exit codes per convention | Integration |
| TS-12 | CLI-only build: `--no-default-features` compiles and includes agent-status | Build passes | Build |
| TS-13 | Manual live run: badges, summary, seen behavior, notification, read/send/wait round trip | Human-verified against mockups | Manual |
| TS-14 | Public pane ID: compose/parse round-trip, malformed rejection, incarnation uniqueness, EMTERM_PANE_ID injection | Matches FR13 contract | Unit |
| TS-15 | doc/AGENT-STATUS.md content review against IMPLEMENTATION.md contracts | All mandated sections present and consistent | Manual |

## Code Quality Verification
- Format: none enforced (project policy: no crate-wide fmt)
- Static analysis: build commands above (rustc warnings reviewed)

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FRs implemented and tested | Requirements coverage below; all TS pass |
| SC-2 | US1-US3 acceptance criteria pass | TS-5/6/7/8/9/10/13 |
| SC-3 | No regression in existing mux replay/snapshot tests | rust test command green (TS-3 context) |
| SC-4 | CLI-only build intact | TS-12 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0003 | TS-1, TS-11 |
| FR2 | task0001 | TS-11, TS-12 |
| FR3 | task0003 | TS-2 |
| FR4 | task0003 | TS-3 |
| FR5 | task0002, task0003, task0005 | TS-4, TS-3 |
| FR6 | task0005 | TS-5 |
| FR7 | task0005, task0006 | TS-5, TS-6, TS-13 |
| FR8 | task0006 | TS-6, TS-13 |
| FR9 | task0007 | TS-7, TS-13 |
| FR10 | task0002, task0004 | TS-8 |
| FR11 | task0002, task0004 | TS-9 |
| FR12 | task0002, task0004 | TS-10 |
| FR13 | task0002, task0003, task0004, task0006 | TS-14, TS-13 |
| NFR1 | task0001, task0004, task0007, task0008 | TS-1, TS-7, TS-15 |
| NFR2 | task0001, task0002, task0003 | TS-4, TS-12, TS-3 |
| NFR3 | task0003, task0004, task0005 | TS-8, TS-3 |
| NFR4 | task0006, task0007 | TS-6, TS-13 |

## E2E Testing
No project E2E framework — covered by the manual section.

## Manual Testing (E2E Not Possible)
- [ ] TS-13a: two mux panes; `emterm agent-status` reports change tab
      badge / window badge / status-bar summary as specified.
- [ ] TS-13b: focus the tab in the foreground window → unseen emphasis
      clears (filled → ring), counts unchanged.
- [ ] TS-13c: background tab transition to blocked/done fires one OS
      notification; visible tab does not; toggling the setting off stops
      notifications.
- [ ] TS-13d: detach → attach: badges restored, no notification burst.
- [ ] TS-13e: `emterm mux read/send/wait` round trip incl. `--pane
      current`, wait timeout exit code, `--after` with send watermark.
- [ ] TS-13f: モックとの目視照合 — compare against
      `design/mockups/screen-tab-badges.html` and
      `design/mockups/screen-status-bar-summary.html` (dot placement,
      filled vs ring, summary ordering/hiding).
- [ ] TS-15: doc/AGENT-STATUS.md reviewed against IMPLEMENTATION.md
      contracts.

## Performance / Security Verification
- NFR3: no polling paths introduced (code review); read responses capped
  (TS-8).
- NFR1: sanitize coverage in TS-1; trust-boundary documentation in TS-15.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Parsing/core | TS-1, TS-2, TS-14 | 3 | 0 | 0 |
| IPC/replay | TS-3, TS-4 | 2 | 0 | 0 |
| GUI model/UI | TS-5, TS-6 | 2 | 0 | 0 |
| Notifications | TS-7 | 1 | 0 | 0 |
| API | TS-8, TS-9, TS-10 | 3 | 0 | 0 |
| CLI/build | TS-11, TS-12 | 2 | 0 | 0 |
| Manual | TS-13a-f, TS-15 | 0 | 0 | 7 |
