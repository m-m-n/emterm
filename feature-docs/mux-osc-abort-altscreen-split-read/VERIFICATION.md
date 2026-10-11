# Verification Document: mux-osc-abort-altscreen-split-read

## Overview
**Feature**: mux-osc-abort-altscreen-split-read / **SPEC.md**: `feature-docs/mux-osc-abort-altscreen-split-read/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-osc-abort-altscreen-split-read/IMPLEMENTATION.md`

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
| TS1 | Live-span tracker unit tests in the pty_spawn test module: the tracker corpus (SC-1x streams, every cut form after a split switch back to the main screen for ?1049 / ?1047 / ?47, `ESC (` and `ESC )` designator-slot switches, the `ESC [?104` 9 / 7 prefix ambiguity, `ESC [?10` then `4x`, an unfinished final sequence) at every single cut, every pair of cuts and one byte per read, with a real shadow parser; the scanner's items under the tracker's spans; the fallback, adoption and shadow-parser switch-set cases (task0001 AC-1, AC-2, AC-6) | Each read's spans equal the bulk extraction's spans restricted to that read, except the undecided tail, which is not live; no check disagrees; the scanner's items equal those under the bulk-restricted spans, and K16's reads give one prompt-start mark; an unmodelled form falls back and is adopted; a rebuilt shadow parser is adopted; vt100 switches on ?1049 and ?47, not on ?1047 | Unit |
| TS2 | Mux half of SC-1x rows K16–K37 through the real `pty_reader_loop` and daemon agent-status task, in every mux delivery the row names (task0001 AC-3) | Each row ends in its final OSC 7501 table and final OSC 777 state; K32–K34 and K36 cause no broadcast and no revision change | Integration |
| TS3 | Plain-tab half of SC-1x rows K16–K37, each in one `process_combined` call (task0002 AC-1 to AC-3) | The same final tables and OSC 777 states as TS2; K32–K34 and K36 record no summary change; no OSC 7501 answer is written | Integration |
| TS4 | Scrollback write path on the reproduction reads: K16 cut at [8, 15, 23] through the reader, with no read suppressed and with each single read suppressed (task0001 AC-4) | The ring content and the chunks delivered to the client equal the unmodified reader's pinned values; the fallback read writes nothing (`133;A` is not in the ring) | Integration |
| TS5 | Full `--lib` suites of src-tauri and term_core, and the `--no-default-features` check (task0001 AC-7, task0002 AC-4) | Every test passes, every existing test with its assertions unchanged (`fallback_path_keeps_the_whole_chunk_gate`, `round3_*`, `round4_*`, `escape_state_carry`, `program_status_feed`, SC-1 K1–K15 on both sides, `dblesc_*`); the CLI-only check exits 0 | Unit / Build |
| TS6 | Robustness: every proper prefix of every switch pattern, live and after `ESC (`, followed by each of the 256 byte values on both screens; a stream of at least 1 MiB of prefixes, switches and marks in 64 KiB reads and (first 64 KiB) 1-byte reads (task0001 AC-5) | No panic; the carried partial never exceeds 7 bytes; every span lies inside its read, ascending and non-overlapping | Unit |
| TS7 | No new log output (task0001 AC-7) | The feature diff adds no log call | Review |
| TS8 | term_core unchanged (task0001 AC-7, task0002 AC-4) | `git diff --stat <implement base_commit>..HEAD -- crates/term_core` prints nothing | Search |
| TS9 | Linux and Windows (task0001 AC-7, task0002 AC-4) | The feature diff adds no platform-gated code; manual check M2 passes on Windows | Review / Manual |

## Code Quality Verification
- Format: `rustfmt --check` on the changed `.rs` files only (project rustfmt configuration; never crate-wide)
- Static analysis: no linter is configured; the build check reports no new warning in the changed files

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | The reproduction reads (`ESC [?1049h`, `ESC [?1049`, `l ESC ] 133;A`, `ESC ESC \`) on a mux pane seeded with root `working` end in an empty table, the same as the plain tab's bulk result | TS1 (K16 items); TS2, TS3 (K16); manual M1 |
| AC2 | The mark after a switch back to the main screen split inside its sequence, with BEL / ST / `ESC ESC \` / `ESC [0m`, split between the string's end and its terminator, for ?1049 / ?1047 / ?47, matches the plain tab (empty) | TS1; TS2, TS3 (K17–K28) |
| AC3 | Back to the main screen, a mark, and into the alternate screen again in one read, split inside the switch back: matches the plain tab (empty) | TS1; TS2, TS3 (K29–K31) |
| AC4 | A mark on the alternate screen after a split switch into it leaves root `working` on both paths | TS1; TS2, TS3 (K32–K34) |
| AC5 | Designator-slot switches (`ESC (` before the switch's `ESC`), split inside the sequence and between `ESC`, `(` and the designator, match the plain tab | TS1; TS2, TS3 (K35, K36) |
| AC6 | Existing write-path tests pass unchanged; the fallback read of the AC1 stream writes nothing to the ring | TS4, TS5 |
| AC7 | src-tauri and term_core `--lib` suites pass; `cargo check --no-default-features` succeeds | TS5 |
| G1 | All functional requirements implemented and tested | Functional Requirements Coverage below |
| G2 | All test scenarios pass | TS1–TS9 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2 |
| FR2 | task0001 | TS1, TS2, TS3 |
| FR3 | task0001 | TS1, TS2, TS3 |
| FR4 | task0001 | TS1, TS2, TS3 |
| FR5 | task0001 | TS4, TS5 |
| FR6 | task0001, task0002 | TS2, TS3 |
| FR7 | task0001, task0002 | TS1, TS2, TS3 |
| FR8 | task0001, task0002 | TS5 |
| NFR1 | task0001 | TS1, TS6 |
| NFR2 | task0001 | TS5 |
| NFR3 | task0001 | TS7 |
| NFR4 | task0001 | TS9 |
| NFR5 | task0001, task0002 | TS8 |

## E2E Testing
No E2E framework covers this path; TS2 drives the real mux reader and daemon task, TS3 drives the plain tab's live output path, and TS4 drives the real reader's write path.

## Manual Testing (E2E Not Possible)
- [ ] M1: In a release build, open a plain tab and a mux pane (daemon restarted on the new build), each running `sh` (no OSC 133 prompt marks of its own). In each, run `printf '\033]7501;state=working\007\033[?1049h'; sleep 0.2; printf '\033[?1049'; sleep 0.2; printf 'l\033]133;A'; sleep 0.2; printf '\033\033\\'`. The working state disappears from the tab badge / status bar of both.
- [ ] M2: On Windows, repeat M1 (NFR4).

## Performance / Security Verification (if applicable)
- NFR1: no panic on any input; the carried state is O(1) with at most 7 bytes of a partial switch sequence — TS6, TS1 (every split and one byte per read).
- TM-1: an alternate-screen mark is never live on the mux path, and a switch completed across reads or sitting in a designator slot changes liveness exactly as term_core does — TS1 (span equality at every split; the unmodelled-form fallback; rebuilt-parser adoption; the vt100 switch-set pin), TS2 (K32–K34 and K36 keep root `working` with no broadcast and no revision change; K35 and K16–K31 reach the plain tab's table), TS3 (the plain tab's reference result for the same rows).
- TM-2: the tracker cannot panic, keeps its carried state within 7 bytes, and processes each read in one forward pass — TS6 (every prefix followed by every byte value, 1 MiB in 64 KiB and 1-byte reads); one forward pass per read by code review of the tracker.
- TM-3: the exact live spans reach only the feed scanner; the ring content, the chunks delivered to the client and the ring-written ranges are derived as before — TS4 (pinned ring and delivered chunks with no read and with each single read suppressed), TS5 (the existing write-path tests unchanged).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build verification | 2 | 2 | 0 | 0 |
| Test scenarios (TS1–TS9) | 9 | 7 | 0 | 2 |
| Code quality | 1 | 1 | 0 | 0 |
| SPEC success criteria (AC1–AC7, G1–G2) | 9 | 9 | 0 | 0 |
| Manual testing (M1–M2) | 2 | 0 | 0 | 2 |
| Performance / Security (NFR1, TM-1–TM-3) | 4 | 4 | 0 | 0 |
