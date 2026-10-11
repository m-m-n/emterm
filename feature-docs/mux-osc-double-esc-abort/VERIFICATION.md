# Verification Document: mux-osc-double-esc-abort

## Overview
**Feature**: mux-osc-double-esc-abort / **SPEC.md**: `feature-docs/mux-osc-double-esc-abort/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-osc-double-esc-abort/IMPLEMENTATION.md`

All commands run from the project root (`.claude/rules/core-build-location.md`).

## Build Verification
- Command (src-tauri, compiles term_core as a path dependency): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build, scenario TS5): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Coverage target: no coverage tool is configured; every task Acceptance Criterion maps to at least one passing test.
- The `tabs` replay tests can fail nondeterministically when run in parallel; rerun a failing one with `-- --test-threads=1` before treating it as a failure.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Feed scanner unit tests in program_status_feed: cut transitions after an in-body ESC (`ESC ESC`, `ESC [0m`, `ESC ]`, `ESC c`, a plain byte), OSC 7501 / OSC 777 / OSC 133 cut handling, item order, string-end liveness including the `ESC [?1049h` cut and term_core as the mark oracle, every single split and byte-by-byte feed with fixed liveness, the three-feed liveness case, retained length after a cut (task0001 AC-1 to AC-5) | `\` still completes as ST; other bytes cut and are handled as the byte after an ESC; cut OSC 7501 yields nothing; cut canonical agent-status yields one Report; cut OSC 133 is classified as its BEL form and emitted only when the string's own last byte was live; cut items precede the cutting byte's items; splits do not change the items; retained length is 0 after each cut | Unit |
| TS2 | Mux half of SC-1 rows K1–K5 through the real `pty_reader_loop` and daemon task, including K3 / K4 split between the two ESCs (task0001 AC-6) | K1–K4 keep root `working` with no broadcast and no revision change; K5 ends empty with exactly one broadcast and one revision step | Integration |
| TS3 | Plain-tab half of SC-1 rows K1–K5 through `process_combined`, including K3 / K4 as two calls split between the two ESCs (task0002 AC-1) | The same final tables as TS2; K1–K4 record no summary change and write no OSC 7501 answer; K5 records one change, the empty table | Integration |
| TS4 | SC-1 rows K6–K15 on both paths: cut OSC 777 agent-status, canonical D→A cut pairs, alternate-screen cut marks, cut prompt starts on the 7501 table, the `ESC [?1049h` cut; K14 / K15 at each cut point (task0001 AC-6, task0002 AC-2, AC-3) | Both paths end in each row's final OSC 7501 table and final OSC 777 state | Integration |
| TS5 | Full `--lib` suites of src-tauri and term_core, and the `--no-default-features` check (task0001 AC-7, task0002 AC-4) | Every test passes, every existing test with its assertions unchanged (program_status_feed, daemon leading_zero_length, plain-tab SC-4 parity, the term_core RIS oracle test); the CLI-only check exits 0 | Unit / Build |
| TS6 | Robustness: a stream of at least 1 MiB of repeated cut strings of every kind and cut form through the scanner (task0001 AC-5) | Finishes within 2 seconds in the test build without panic; retained length never exceeds 8 KiB | Unit |
| TS7 | Cut, discarded or rejected strings are not logged (task0001 AC-7) | The feature diff adds no log call on any cut, discard or reject path; the scanner's one-shot carry-overflow warning is unchanged | Review |
| TS8 | term_core unchanged (task0001 AC-7, task0002 AC-4) | `git diff --stat <implement base_commit>..HEAD -- crates/term_core` prints nothing | Search |
| TS9 | Linux and Windows (task0001 AC-7, task0002 AC-4) | The feature diff adds no platform-gated code; manual check M2 passes on Windows | Review / Manual |

## Code Quality Verification
- Format: `rustfmt --check` on the changed `.rs` files only (project rustfmt configuration; never crate-wide)
- Static analysis: no linter is configured; the build check reports no new warning in the changed files

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | After `ESC]7501;state=working BEL`, `ESC]07501;state=clear ESC ESC \` leaves root `working` on the plain tab and the mux pane; no broadcast and no revision change on mux | TS2, TS3 (K1); manual M1 |
| AC2 | The same for the canonical `ESC]7501;state=clear ESC ESC \` | TS2, TS3 (K2) |
| AC3 | Split between the two ESCs gives the same tables on both paths; any split and byte-by-byte feed give the same scanner items | TS2, TS3 (K3, K4); TS1 |
| AC4 | `ESC]7501;state=error ESC ESC ]7501;state=clear BEL` applies the clear on both paths | TS1; TS2, TS3 (K5) |
| AC5 | A cut agent-status report (`ESC ESC \` and `ESC X`) yields one Report; OSC 777 state working on both paths | TS1; TS4 (K6, K7) |
| AC6 | Cut `133;D` then `133;A` fire the inferred clear and the 7501 prompt start on both paths; cut `0133;A` yields only the prompt-start-only item | TS1; TS4 (K8, K9, K11–K13) |
| AC7 | Cut OSC 133 on the alternate screen yields nothing; `ESC]133;A ESC [?1049h` yields a main-screen mark; both hold under splits | TS1; TS4 (K10, K14, K15) |
| AC8 | Report then Reset for an `ESC c` cut; mark then OSC 7501 item for an `ESC ]` cut | TS1 |
| AC9 | Existing program_status_feed, daemon leading_zero_length, plain-tab parity and RIS parity tests pass unchanged | TS5 |
| AC10 | `cargo check --no-default-features` succeeds | TS5 |
| G1 | All functional requirements implemented and tested | Functional Requirements Coverage below |
| G2 | All test scenarios pass | TS1–TS9 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2 |
| FR2 | task0001 | TS1, TS2, TS3 |
| FR3 | task0001 | TS1, TS4 |
| FR4 | task0001 | TS1, TS4 |
| FR5 | task0001 | TS1, TS2 |
| FR6 | task0001 | TS1, TS2, TS4 |
| FR7 | task0001, task0002 | TS2, TS3, TS4 |
| FR8 | task0001, task0002 | TS5 |
| FR9 | task0001, task0002 | TS1, TS2, TS3, TS4 |
| NFR1 | task0001 | TS1, TS6 |
| NFR2 | task0001 | TS5 |
| NFR3 | task0001 | TS7 |
| NFR4 | task0001, task0002 | TS9 |
| NFR5 | task0001, task0002 | TS8 |

## E2E Testing
No E2E framework covers this path; TS2 and TS4 drive the real mux reader and daemon task, and TS3 and TS4 drive the plain tab's live output path.

## Manual Testing (E2E Not Possible)
- [ ] M1: In a release build, open a plain tab and a mux pane (daemon restarted on the new build). In each, print `\x1b]7501;state=working\x07`, then `\x1b]07501;state=clear\x1b\x1b\\`. The working state stays in the tab badge / status bar of both.
- [ ] M2: On Windows, repeat M1 (NFR4).

## Performance / Security Verification (if applicable)
- NFR1: no panic on any input, retained bytes within 8 KiB, O(1) added state — TS1 (retained length 0 after each cut, split and byte-by-byte feeds), TS6.
- TM-1: a cut OSC 7501 string yields no item and changes no record, broadcast or revision on the mux path — TS1 (every spelling and cut form yields no OSC 7501 item), TS2 (K1–K4 keep the table with no broadcast and no revision change), TS3 (the plain tab's reference result for the same rows).
- TM-2: a cut OSC 133 string yields an item only when its own last byte was live — TS1 (alternate-screen cut yields nothing, the `ESC [?1049h` cut yields the main-screen mark, term_core oracle, the three-feed liveness case), TS4 (K10, K14, K15 on both paths).
- TM-3: the cut path cannot panic and keeps state bounded — TS1 (every split offset, byte-by-byte, reads beginning with the in-body ESC or the cutting byte), TS6 (1 MiB hostile stream within 2 seconds, retained length within 8 KiB).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build verification | 2 | 2 | 0 | 0 |
| Test scenarios (TS1–TS9) | 9 | 7 | 0 | 2 |
| Code quality | 1 | 1 | 0 | 0 |
| SPEC success criteria (AC1–AC10, G1–G2) | 12 | 12 | 0 | 0 |
| Manual testing (M1–M2) | 2 | 0 | 0 | 2 |
| Performance / Security (NFR1, TM-1–TM-3) | 4 | 4 | 0 | 0 |
