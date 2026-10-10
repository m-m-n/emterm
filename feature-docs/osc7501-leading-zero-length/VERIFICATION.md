# Verification Document: osc7501-leading-zero-length

## Overview
**Feature**: osc7501-leading-zero-length / **SPEC.md**: `feature-docs/osc7501-leading-zero-length/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/osc7501-leading-zero-length/IMPLEMENTATION.md`

All commands run from the project root (`.claude/rules/core-build-location.md`).

## Build Verification
- Command (src-tauri, compiles term_core as a path dependency): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build, scenario TS9): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Coverage target: no coverage tool is configured; every task Acceptance Criterion maps to at least one passing test.
- The `tabs` replay tests can fail nondeterministically when run in parallel; rerun a failing one with `-- --test-threads=1` before treating it as a failure.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | program_status measured parse: whole-sequence length from the received OSC-string length, 4096 / 4097 with BEL and ST, pre-replacement measurement, `?` at any length, canonical entries (task0001 AC-4) | 4096 accepted and 4097 `Ignored`; a body longer than 4096 after replacement is accepted when its received length gives 4096 or less; `?` is `Query` at every length; every existing program_status test passes unchanged | Unit |
| TS2 | term_core received length and saturating OSC number: leading zeros, non-digit bytes before `;`, invalid UTF-8, BEL / ST / unterminated, per-string reset, over `MAX_OSC_LEN`; numbers 65535 / 65536 / 655367501 / 6553652; responder seam (task0001 AC-1, AC-2, AC-3) | The count equals the received bytes between `ESC ]` and the terminator; above-range numbers are dispatched as 65535 and reach no code; `6553652` is not OSC 52; no panic in the debug test build; a responder that implements only the existing method sees unchanged calls | Unit |
| TS3 | Plain tab through the live output path: repro 1, SC-4 rows P3–P9, a report over `MAX_OSC_LEN`, over-long and leading-zero queries (task0001 AC-5, AC-6) | Repro 1 and every over-limit row register nothing; P4, P6 and P8 are accepted; each query gets exactly one `ESC ] 7501;?` answer with its own terminator; an unterminated query gets none | Integration |
| TS4 | Mux feed scanner items under the shared rule: leading-zero, non-digit-head and number-only 7501, numbers near 7501, out-of-range numbers, `0777` agent-status, canonical and non-canonical 133 marks, byte order, split reads, carry bound (task0002 AC-1) | One OSC 7501 item per 7501 spelling carrying the SC-1 body and the received length; none for other numbers; no report for `0777`; canonical marks give the mark item and a non-canonical live prompt start gives the prompt-start-only item; order and carry bound unchanged | Unit |
| TS5 | Daemon and pane through the real reader and task: repro 2, SC-4 rows P4–P9 on mux, queries, `0133;A` against the OSC 7501 table and the OSC 777 latch (task0002 AC-2, AC-3) | Repro 2 leaves no record; boundary rows match SC-4; queries change nothing; `0133;A` removes working / blocked / idle, keeps done / error and fires no OSC 777 inferred clear; canonical `133;D` then `133;A` still fires it | Integration |
| TS6 | Parity corpus SC-4: plain-tab table (task0001 AC-7), mux-pane table and strip verdict (task0002 AC-4) | Every row ends in the SC-4 final table on both paths; the zero-allocation identity matches each row's strip verdict | Integration |
| TS7 | Query gates: coalesce gate and suppressed-output delivery scan with leading-zero, over-long and out-of-range queries (task0002 AC-5) | `07501;?` (BEL, ST) and Z(5000) `7501;?` are queries on both; `65536;?` and `655367501;?` are not; every existing gate and delivery-scan test passes | Unit |
| TS8 | FR9 rename and predecessor record (task0002 AC-6) | The renamed test passes; `test-docs/osc7501-program-status/task0004.tests.yaml` AC-1 lists the new name with the supersede note and unchanged `red_reason`; `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list` shows `mux::ipc::pty_spawn::tests::program_status_feed::ac1_the_shared_number_rule_decides_what_is_an_osc_7501: test` | Unit / Record check |
| TS9 | CLI-only build (task0001 AC-7, task0002 AC-7) | The `--no-default-features` check exits 0 | Build |
| TS10 | term_core protocol independence (task0001 AC-3) | `rg -n 7501 crates/term_core/src` lists only the existing `osc7501-` feature-name mentions in comments | Search |
| TS11 | Robustness: 1 MiB OSC string and one-million-digit number through term_core, the osc_identify 2-second tests, the allocation-free identity test, the scanner carry bound (task0001 AC-1, AC-2; task0002 AC-7) | Each finishes within 2 seconds without panic; identification makes no allocation; retained scanner bytes never exceed 8 KiB | Unit |
| TS12 | Rejected input is not logged (task0001 AC-7, task0002 AC-7) | The feature diff adds no log call on any path that rejects, ignores or does not recognize an OSC 7501 / OSC 133 sequence; the scanner's one-shot carry-overflow warning is unchanged | Review |
| TS13 | Linux and Windows (NFR5) | The change adds no platform-gated code; manual check M3 passes on Windows | Manual |

## Code Quality Verification
- Format: `rustfmt --check` on the changed `.rs` files only (project rustfmt configuration; never crate-wide)
- Static analysis: no linter is configured; the build check reports no new warning in the changed files

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Repro 1 in a plain tab registers no record | TS3; manual M1 |
| AC2 | Repro 2 in a mux pane removes the working record, same as a plain tab | TS5, TS6; manual M2 |
| AC3 | 4096 accepted, 4097 rejected, leading zeros included, plain tab and mux, BEL and ST | TS1, TS3, TS5, TS6 |
| AC4 | Invalid UTF-8 measured before replacement on plain tab and mux | TS1, TS3, TS5, TS6 |
| AC5 | Plain-tab table, mux-pane table and strip verdict agree over one corpus | TS6 |
| AC6 | `0133;A` in a mux pane: prompt start only, no OSC 777 inferred clear; canonical D→A unchanged; `0777` agent-status not ingested | TS4, TS5 |
| AC7 | 65535 / 65536 / 655367501 / 6553652: saturation, no code reached, no OSC 52, no panic; non-7501 on osc_identify and mux ingestion | TS2, TS4, TS6 |
| AC8 | Over-long leading-zero query answered exactly once; gate and delivery scan treat `07501;?` as a query | TS3, TS7 |
| AC9 | Canonical tests pass; FR9 rename and record; regression tests for both repros | TS1–TS8 |
| G1 | All functional requirements implemented and tested | Functional Requirements Coverage below |
| G2 | All test scenarios pass | TS1–TS13 |
| G3 | Every existing test for the canonical spelling passes (FR5) | Both test commands above |
| G4 | `--no-default-features` build succeeds (NFR1) | TS9 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002 | TS1, TS3, TS5, TS6 |
| FR2 | task0001 | TS2, TS3 |
| FR3 | task0002 | TS4, TS5, TS6 |
| FR4 | task0001, task0002 | TS6 |
| FR5 | task0001, task0002 | TS1, TS3, TS4, TS7 |
| FR6 | task0002 | TS4, TS5 |
| FR7 | task0001, task0002 | TS2, TS4, TS6 |
| FR8 | task0001, task0002 | TS1, TS3, TS7 |
| FR9 | task0002 | TS8 |
| NFR1 | task0001, task0002 | TS9 |
| NFR2 | task0001 | TS10 |
| NFR3 | task0001, task0002 | TS11 |
| NFR4 | task0001, task0002 | TS12 |
| NFR5 | task0001, task0002 | TS13 |

## E2E Testing
No E2E framework covers this path; TS3, TS5 and TS6 drive the real output path and the real mux reader and task.

## Manual Testing (E2E Not Possible)
- [ ] M1: In a plain tab of a release build, run `python3 -c 'import sys; sys.stdout.buffer.write(b"\x1b]" + b"0" * 4096 + b"7501;state=error\x07")'`. No OSC 7501 error state appears in the tab badge or the status bar.
- [ ] M2: In a mux pane of a release build (daemon restarted on the new build), print `\x1b]7501;state=working\x07`, then `\x1b]07501;state=clear\x07`. The working state disappears, as it does in a plain tab.
- [ ] M3: On Windows, repeat M1 and M2 (NFR5).

## Performance / Security Verification (if applicable)
- NFR3: 1 MiB OSC strings and one-million-digit numbers finish within 2 seconds on term_core and osc_identify; identification stays allocation-free; scanner carry stays within 8 KiB — TS11.
- TM-1: term_core counts the received OSC-string length and program_status applies the 4096-byte check to it — TS2 (count), TS1 (check), TS3 (repro 1, P4–P9, a report over `MAX_OSC_LEN` rejected on the plain tab).
- TM-2: saturating OSC number; above-range numbers reach no code — TS2 (`65536`, `655367501`, `6553652` dispatched as 65535 in the debug test build without panic, no OSC 52 handling, no registered code reached).
- TM-3: the mux path applies the 4096-byte check to the scanner's received length, with the 8 KiB carry kept — TS4 (received length on items, carry bound), TS5 (P4–P9 on a mux pane).
- TM-4: the scanner takes the OSC number only from osc_identify's shared rule — TS4 (near and out-of-range numbers yield no OSC 7501 item), TS6 (strip verdict agrees on every row), and the task0002 AC-4 number-entry agreement test.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build verification | 2 | 2 | 0 | 0 |
| Test scenarios (TS1–TS13) | 13 | 11 | 0 | 2 |
| Code quality | 1 | 1 | 0 | 0 |
| SPEC success criteria (AC1–AC9, G1–G4) | 13 | 13 | 0 | 0 |
| Manual testing (M1–M3) | 3 | 0 | 0 | 3 |
| Performance / Security (NFR3, TM-1–TM-4) | 5 | 5 | 0 | 0 |
