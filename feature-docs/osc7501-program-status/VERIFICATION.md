# Verification Document: osc7501-program-status

## Overview
**Feature**: osc7501-program-status / **SPEC.md**: `feature-docs/osc7501-program-status/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/osc7501-program-status/IMPLEMENTATION.md`

Run every command from the project root (no `cd`).

## Build Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (mux_ipc): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/mux_ipc/Cargo.toml`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Command (CLI-only build, scenario TS10): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for each command

## Test Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (mux_ipc): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/mux_ipc/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0, every test passes
- Known flakiness: the `tabs` replay tests can fail nondeterministically in
  a parallel run, and the `tmux_sockets` discover tests rarely fail in a
  parallel run. When only those fail, re-run the failing tests with the
  libtest `--test-threads=1` option; a pass there counts as a pass.
- Coverage target: no coverage tool is configured. Target: every Acceptance
  Criterion of every task plan maps to at least one passing test in its
  `test-docs/osc7501-program-status/taskNNNN.tests.yaml` record.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Parser: the six states, malformed pairs, unknown keys, repeated keys, missing / unknown state, every id limit, title / msg invalid base64, size caps and control characters, the 4096-byte sequence limit (task0001 AC-1, AC-2, AC-6) | Each case is accepted, skipped, ignored or discarded exactly as FR2 / FR3 / FR5 state; boundary values (4096 vs 4097 bytes, each title / msg / id cap) land on the correct side; a discarded or ignored report leaves the table unchanged | Unit |
| TS2 | Record table: full replacement, subtree and full clear, app inheritance, conditional kind / progress, 257th-record eviction, summary rank and tie-break, export / import (task0001 AC-3, AC-5, AC-7) | Table contents and summary match FR4 / FR5 / FR6 / FR10; the least recently updated record is evicted; import of an export reproduces the table and its eviction order | Unit |
| TS3 | Lifecycle: OSC 133 A, RIS, DECSTR, alternate screen, A on the alternate screen (task0001 AC-4, task0003 AC-4, task0004 AC-4) | A live main-screen A removes `working` / `blocked` / `idle` and keeps `done` / `error`; RIS removes every record; DECSTR, alternate-screen switches and an A on the alternate screen remove nothing | Unit / Integration |
| TS4 | Plain tab through callbacks and term_core: BEL and ST reports, the `?` query, composite with OSC 777, internal-code mapping after the core swap, coalesce gate, mux-attached discard (task0003 AC-1 to AC-7) | Both terminators reach the table; a `?` query gets exactly one answer with its own terminator and changes nothing; the badge aggregate shows the composite; mux inner-content reports change nothing while queries are still answered once | Integration |
| TS5 | Aggregation and display: composition rank, seven-level cross-pane order with `error`, error badge unseen and seen, md3 `error` role per preset, `{agent_status}` value, empty string and version bump (task0002 AC-1 to AC-6, task0005 AC-1 to AC-5) | Orders match D2 / FR11; unseen `error` renders U+274C with a filled fallback and seen renders U+1F4A4 with a ring; the color is the md3 `error` role; `{agent_status}` is one of the five state words or empty and its version advances only on change | Unit / Integration |
| TS6 | Notifications: `error` gated by `agent_notify_on_done`, name order (title, app, OSC 777 name, default), title sanitization (task0002 AC-3, AC-7, task0001 AC-6) | `error` notifies only with `agent_notify_on_done` on and every existing gate passing; the body uses `error` / `エラー` and the name chosen by D5; control and invisible characters are removed and the name is at most 80 characters | Unit / Integration |
| TS7 | mux: scanner order and bound, wire `error` and summary round trip, revision and broadcast, replay-derived resync, ring and snapshot stripping, suppressed-output query delivery, WaitAgentState on the composite and `error`, daemon RIS, `emterm mux wait --state error` (task0002 AC-1, AC-5, AC-8, task0004 AC-1 to AC-6, task0006 AC-1 to AC-3) | Items keep byte order with OSC 777 and OSC 133; the summary item round-trips; resync is silent; replay bytes contain no OSC 7501; a suppressed `?` query reaches the GUI once; waits match the composite and `error` | Unit / Integration |
| TS8 | Hot-upgrade handoff: schema round trip, older versions, capture and restore (task0007 AC-1 to AC-4) | Records and their update order survive write and read; older documents decode with no records; invalid or excess records are dropped on restore | Unit / Integration |
| TS9 | Regression: every existing agent_status, agent_status_model, exit_latch, notifications and mux agent-status test (task0002 AC-3, task0004 AC-6) | All pass; a pane that received only OSC 777 shows, notifies and answers the mux API exactly as before | Unit / Integration |
| TS10 | CLI-only build compiles with the Program Status core (task0001 AC-8) | The `--no-default-features` check exits 0 | Build |

## Code Quality Verification
- Format: no format command is configured for any component. Do not run a
  crate-wide formatter; when formatting changed files, limit it to the
  files this feature changed.
- Static analysis: none configured beyond the build checks above; the build
  produces no new warnings in the files this feature changed.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | OSC 7501 with BEL and with ST reaches the plain-tab badge and `{agent_status}` | TS4, TS5; manual M1 |
| AC2 | Six state values: replacement, subtree / full clear, independent `error` display and rank | TS1, TS2, TS5 |
| AC3 | Parsing rules, discard conditions, every limit, 256-record eviction | TS1, TS2 |
| AC4 | title / msg base64 decoding; invalid base64, oversize and control characters reject the whole report | TS1 |
| AC5 | `OSC 7501 ; ?` answered with `?` and the same terminator | TS4, TS7 |
| AC6 | Lifecycle: OSC 133 A, close, RIS, DECSTR, alternate screen | TS3, TS4 |
| AC7 | `error` notification gated by `agent_notify_on_done`, name from the sanitized title or app | TS6; manual M3 |
| AC8 | mux pane parity: daemon delivery, detached reports, silent resync, no re-answer on replay, WaitAgentState on the composite and `error` | TS7; manual M4 |
| AC9 | Records and eviction order survive a daemon hot upgrade | TS8; manual M5 |
| AC10 | Every existing OSC 777 agent-status test passes; OSC 777-only panes unchanged | TS9 |
| G1 | All functional requirements are implemented and tested | Functional Requirements Coverage below |
| G2 | All test scenarios pass | TS1 to TS10 |
| G3 | Performance goal (NFR4) is met | Performance / Security Verification: NFR4 |
| G4 | Security requirements are satisfied | Performance / Security Verification: NFR5, TM-1 to TM-7 |
| G5 | Documentation is complete | Task plans, IMPLEMENTATION.md, THREAT-MODEL.md and test-docs records exist and agree with the merged code |
| G6 | Code review is completed | The review phase finishes with no open critical or high finding |
| G7 | The `--no-default-features` build compiles (NFR1) | TS10 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0003, task0004 | TS4, TS5, TS7 |
| FR2 | task0001 | TS1, TS2 |
| FR3 | task0001 | TS1, TS2 |
| FR4 | task0001 | TS1, TS2, TS5 |
| FR5 | task0001 | TS1 |
| FR6 | task0001 | TS1, TS2 |
| FR7 | task0001, task0003, task0004 | TS3 |
| FR8 | task0003, task0006 | TS4, TS7 |
| FR9 | task0002, task0003, task0004 | TS4, TS5 |
| FR10 | task0001 | TS1, TS2, TS5 |
| FR11 | task0002 | TS1, TS2, TS5, TS6 |
| FR12 | task0002 | TS6 |
| FR13 | task0005 | TS4, TS5 |
| FR14 | task0002, task0003, task0004 | TS7 |
| FR15 | task0006 | TS7 |
| FR16 | task0002, task0004 | TS7 |
| FR17 | task0007 | TS8 |
| FR18 | task0002 | TS9 |
| FR19 | — (exclusion statement) | Review: no OSC 9;4 mapping, no terminfo change, kind / progress / msg not displayed, OSC 9 handling unchanged |
| NFR1 | task0001 | TS10 |
| NFR2 | task0001 | Review: no code copied from external implementations |
| NFR3 | task0003, task0004 | Manual M6 (Windows) |
| NFR4 | task0003 | Review: Performance / Security Verification NFR4 |
| NFR5 | task0001, task0002, task0005 | TS1, TS5, TS6 |

## Manual Testing (E2E Not Possible)
- [ ] M1: In a plain tab, emit OSC 7501 reports from the shell (BEL and ST
  terminated; `working`, `blocked`, `done`, `error`, then `clear`). The tab
  badge follows each state, and a status bar template containing
  `{agent_status}` shows the state word and becomes empty after `clear`.
- [ ] M2: Error badge appearance in a dark and a light theme: an unseen
  `error` shows U+274C in the md3 error color; after viewing the tab it
  shows U+1F4A4; the mux sidebar shows the same.
- [ ] M3: With `agent_notify_on_done` on, an `error` report from a
  non-visible tab produces a desktop notification whose name is the
  report's title (or app); with the toggle off, no notification appears.
- [ ] M4: In a mux pane, send reports while detached, then reattach: the
  badge shows the current state and no notification fires. Send a `?`
  query: exactly one answer reaches the program; after a reattach the shell
  prompt shows no stray answer.
- [ ] M5: Hot-upgrade the mux daemon with records present (requires a
  release build the user runs): the badge and `emterm mux wait --state`
  results are unchanged after the upgrade.
- [ ] M6: On Windows, repeat M1 and M4 (NFR3).

## Performance / Security Verification (if applicable)
- NFR1: the Program Status core compiles without the gui feature — TS10.
- NFR4: OSC 7501 handling runs inside the existing output pump and daemon
  feed with work bounded by the SC-1 caps, and notification sending uses
  the existing path; checked in review that no new blocking I/O or waits run
  on the UI thread.
- NFR5: titles and msgs are never interpreted as markup, names are
  sanitized and `{agent_status}` holds only state words — TS1, TS5, TS6.
- TM-1: sequence, title, msg and id caps plus the 256-record cap with
  least-recently-updated eviction — task0001 AC-2 and AC-3 tests (boundary
  sizes, 257th insert).
- TM-2: the daemon scanner's carry stays within its existing bound for
  over-long and unterminated OSC 7501 — task0004 AC-2 tests.
- TM-3: control-character titles discard the report; names have control and
  invisible formatting characters removed and are cut to 80 characters; the
  daemon sends only sanitized titles — task0001 AC-2 and AC-6 tests,
  task0004 AC-3 tests.
- TM-4: the notification body carries the name as plain text through the
  existing escape-on-send path, msg is never shown, `{agent_status}` holds
  only state words — task0002 AC-7 tests, task0005 AC-4 tests.
- TM-5: OSC 7501 is stripped from the ring and snapshots and mux inner
  content cannot change GUI 7501 state — task0006 AC-2 tests, task0003 AC-6
  tests.
- TM-6: the query answer is the fixed sequence with the query's own
  terminator, only for a body of exactly `?`, once per query — task0003
  AC-2 tests.
- TM-7: the handoff decode is version-gated and restore re-validates records
  and applies the cap — task0001 AC-7 tests, task0007 AC-4 tests.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Test scenarios (TS1 to TS10) | 10 | 10 | 0 | 0 |
| Success criteria (AC1 to AC10, G1 to G7) | 17 | 13 | 0 | 4 |
| Manual tests (M1 to M6) | 6 | 0 | 0 | 6 |
| Non-functional checks (NFR1, NFR4, NFR5) | 3 | 2 | 0 | 1 |
| Threat mitigations (TM-1 to TM-7) | 7 | 7 | 0 | 0 |
| Total | 46 | 35 | 0 | 11 |
