# Verification Document: mux-strip-open-string-body-closure

## Overview
- **Feature**: mux-strip-open-string-body-closure
- **SPEC.md**: `feature-docs/mux-strip-open-string-body-closure/SPEC.md`
- **IMPLEMENTATION.md**: `feature-docs/mux-strip-open-string-body-closure/IMPLEMENTATION.md`
- **THREAT-MODEL.md**: `feature-docs/mux-strip-open-string-body-closure/THREAT-MODEL.md`

Scope: the shared strip writes the string-body closure (ESC + CAN) at a removal
inside an open written OSC / DCS / APC body. The scope also covers the inverted
predecessor expectations, one rename with its test-docs record update, a
decision record and doc comments. Run every command from the integration
worktree root.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command for the CLI-only build (NFR1): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: both exit with code 0 and report no errors.

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Name resolution (NFR1): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
- Coverage target: not measured, because `project.components` configures no coverage tooling. Verification passes when every scenario below passes.
- Known unrelated flakiness. Re-run a failure of these tests before attributing it to this feature:
  - The tabs replay tests can fail nondeterministically under parallel runs. They are stable with one test thread.
  - The tmux socket discovery tests rarely fail under parallel runs.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Each OSC color-query head (`ESC]11;`, `ESC]10;`, `ESC]12;`, `ESC]4;1;`) × each of the eight removed construct kinds, followed by `?` BEL, goes through one cut-free write-filter feed. Then build_snapshot_bytes, then replay of the payload through the themed client. The DCS head `ESC Px` and APC head `ESC _x` are run the same way (SPEC AC-1; task0001 AC-3). | The ring is exactly head + ESC CAN + the construct's re-emitted C0 bytes + `?` BEL. The stripped scrollback equals the ring. The replay produces no response. Rows and cursor equal view_after_a_cut of head + construct then `?` BEL. For the DCS / APC heads, rows, cursor and responses equal the raw-stream reference. Control: the themed client answers `ESC]11;?BEL` and each head + `?` BEL. | Unit |
| TS-2 | Live continuation. The ring is head + a removed construct. It goes through build_resume_snapshot_bytes (no segments, empty screen, main buffer, shared dimensions), then themed replay with the replay's responses discarded, then a live `?` BEL. Also run with the raw head + construct given as the ring (SPEC AC-2; task0001 AC-4). | The payload is the resume clear prefix + the ring, with nothing appended. The live `?` BEL produces no response in either case. Control: a ring holding the head alone answers the live `?` BEL. | Unit |
| TS-3 | Strip-unit table: body heads (`ESC]0;t`, `ESC]11;`, `ESC Px`, `ESC _Xnot-kitty`) × the eight constructs × continuations (`n`, `?` BEL, text, `ESC \`), at all six strip entry points (SPEC AC-3; task0001 AC-1). | Output is head + one ESC CAN + C0 bytes + continuation, with the closure before the C0 bytes. The state form reports Ground, matching client_written_state. Replay equals the raw-stream reference. | Unit |
| TS-4 | All eight constructs concatenated inside one body (SPEC AC-3; task0001 AC-1). | Exactly one ESC CAN, followed by every construct's C0 bytes in order. | Unit |
| TS-5 | Carried states (SPEC AC-3; task0001 AC-2 a to c): the state form started in OscBody / StBody with each construct. Also the write filter after an overflow flush left an OSC body, or one of four DCS / APC bodies, open; the next call is construct + `?` BEL + text. Also a held live lone `ESC` after the flush, completed by the next call. | The output starts with ESC CAN, and the reported state is Ground and agrees with the oracle. The next call writes ESC CAN + C0 bytes + `?` BEL + text, holds nothing and leaves Ground. Replay matches the raw-stream reference. | Unit |
| TS-6 | Head + each held string construct + `?` BEL, split into two calls at every position with no cut. The CSI query constructs are split the same way (SPEC AC-3; task0001 AC-2 d). | Every split writes the same bytes as one call. For the CSI query constructs, the themed replay produces no response and matches the reference. | Unit |
| TS-7 | Remap and its designator form, plus build_snapshot_bytes segments, with watch offsets around a construct removed inside a body (SPEC AC-4; task0001 AC-5). | The construct's first byte maps before the closure. Inside the construct, its last byte, and right after it map past the closure and the C0 bytes. Offsets are non-decreasing and at most the output length, and both forms agree. Segments follow the same mapping, shifted by the clear prefix. | Unit |
| TS-8 | After a strip-written closure: a cut at the end of the call (one and two cuts), a cut at fed offset 0 of the next call, and the fallback closing. Both strips run on body + ESC CAN + each construct, and on body + ESC CAN alone. The vt100 replay copy runs on the ring (SPEC AC-5; task0001 AC-6). | The cuts and the fallback write nothing more and leave Ground. Both strips give body + ESC CAN + C0 bytes and keep body + ESC CAN unchanged. The vt100 copy equals the ring. | Unit |
| TS-9 | The FR2 single definition and the FR8 items (SPEC AC-6, AC-7; task0001 AC-7, AC-8). STRING_BODY_CLOSING has one definition and a re-export. The renamed AC-11 test holds the inverted expectations. Covered also: the two Ground-closure tests without their body rows, the state-form rows and identity, and name resolution in the `--list` output. | One definition, in `scrollback_filter.rs`, with value 0x1B 0x18, and a same-named re-export in `write_filter.rs`. All updated tests pass. The new name appears as a `: test` line and the old name does not appear. | Unit |
| TS-10 | The doc-comment contract (SPEC AC-8; task0001 AC-7). | The new contract test passes: the FR10 stale phrases are absent and the required phrases are present. The predecessor contract test passes unchanged. | Unit |
| TS-11 | The `--lib` suite and the `--no-default-features` check (SPEC AC-9; task0001 AC-10). | Both exit with code 0. Every new test asserts that it finishes within the 10 s budget. | Integration (build) |
| TS-12 | Inspect `feature-docs/mux-strip-open-string-body-closure/DECISIONS.md` (FR9; SPEC AC-7; task0001 AC-9). Added at create-plan because SPEC.md has no scenario for FR9. | It records four things: the ESC + CAN closure decision with the shared STRING_BODY_CLOSING; the before and after value of every changed expectation, including any reported as a plan deviation; the rename and the test-docs record update; and the resolution of mux-strip-join-escape-closure Residual 1. | Inspection |
| TS-13 | Inspect the `git diff` from `workflow.implement.base_commit` to the integration branch tip (FR8, NFR3; SPEC AC-7, AC-9). Added at create-plan because SPEC.md has no scenario for these diff-level conditions. | No change under `crates/term_core`. No predecessor DECISIONS.md or other predecessor `feature-docs/` text changes. In the predecessor test-docs record, only the AC-11 entry changes: it gets the new name and a supersede YAML comment naming this feature's SPEC.md FR1 and FR5, and its red_reason is unchanged. | Inspection |
| TS-14 | Linear pass: 100,000 repetitions of `ESC]0;t` + the BEL-terminated launch construct through the written-state form. Also the output length of every TS-3 case (NFR2; task0001 AC-10). Added at create-plan because SPEC.md lists NFR2 under Performance Tests without a scenario ID. | The output is exactly 100,000 repetitions of `ESC]0;t ESC CAN`, the state is Ground, and the test finishes within the 5 s bound of the existing linear-pass test. No output is longer than its input. | Unit |

## Code Quality Verification
- Format: workflow.yaml configures no `format_command`, so no format check runs. A crate-wide formatter run is not part of verification.
- Static analysis: none configured beyond the build checks above.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | The Functional Requirements Coverage table below; TS-1 to TS-10 pass |
| SC-2 | All test scenarios pass | TS-1 to TS-14 |
| SC-3 | Performance meets specified goals (NFR2) | TS-14 |
| SC-4 | Security requirements are satisfied (no color query the raw stream never made) | TS-1, TS-2, and the TM-1 to TM-3 items below |
| SC-5 | Documentation is complete (DECISIONS.md and doc comments) | TS-10, TS-12 |
| SC-6 | Code review is completed | The review step in workflow.yaml is completed with no residual critical or high finding |
| AC-1 | SPEC AC-1 (FR1, FR7) | TS-1 |
| AC-2 | SPEC AC-2 (FR7) | TS-2 |
| AC-3 | SPEC AC-3 (FR1, FR3) | TS-3, TS-4, TS-5, TS-6 |
| AC-4 | SPEC AC-4 (FR4) | TS-7 |
| AC-5 | SPEC AC-5 (FR5, FR6) | TS-8 |
| AC-6 | SPEC AC-6 (FR2) | TS-9 |
| AC-7 | SPEC AC-7 (FR8, FR9) | TS-9, TS-12, TS-13 |
| AC-8 | SPEC AC-8 (FR10) | TS-10 |
| AC-9 | SPEC AC-9 (NFR1, NFR2, NFR3) | TS-11, TS-13, TS-14 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-3, TS-4 |
| FR2 | task0001 | TS-9 |
| FR3 | task0001 | TS-3, TS-5, TS-6 |
| FR4 | task0001 | TS-7 |
| FR5 | task0001 | TS-8 |
| FR6 | task0001 | TS-8 |
| FR7 | task0001 | TS-1, TS-2 |
| FR8 | task0001 | TS-9, TS-13 |
| FR9 | task0001 | TS-12 |
| FR10 | task0001 | TS-10 |
| NFR1 | task0001 | TS-9 (name resolution), TS-11 |
| NFR2 | task0001 | TS-11, TS-14 |
| NFR3 | task0001 | TS-11 (budget), TS-13 (term_core unchanged), TS-1 to TS-8 (oracle convention) |

## Manual Testing (E2E Not Possible)
- None. The feature changes no user-facing behavior that a person must judge. No E2E framework was detected, and the design step was skipped, so there is no mockup comparison.

## Performance / Security Verification
- NFR2: the closure is written inside the existing single pass with O(1) extra state. TS-14 checks the linear pass within the 5 s bound, and that no output is longer than its input.
- NFR3: each new test finishes within the existing 10 s per-test budget; the tests assert it, and the per-test durations of the `--lib` run (TS-11) show it. TS-13 confirms term_core is unchanged.
- TM-1: the string-body closure at a removal inside an open written body prevents any color query the raw stream never made. These checks cover it:
  - TS-1: no response on snapshot replay through the themed client, with the controls answering.
  - TS-2: no response to a live `?` BEL after a resume replay, with the head-only control answering.
  - TS-3, TS-4: one closure before the C0 bytes at every entry point.
  - TS-5: carried bodies, including after an overflow flush and a held lone `ESC`.
  - TS-6: every split position.
- TM-2: the inserted closure is never doubled, never read as a target start or ST, and never rewritten. TS-8 checks this: cuts and the fallback write nothing more, both strips keep ESC CAN and remove a following target, and the vt100 copy equals the ring.
- TM-3: hostile output with many removals inside bodies stays linear and never grows the ring. TS-14 checks this with the linear pass and the output-length bound.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (default check, CLI-only check) | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-11, TS-14) | 12 | 12 | 0 | 0 |
| Document / diff inspection (TS-12, TS-13) | 2 | 0 | 0 | 2 |
| Performance (NFR2, NFR3) | 2 | 2 | 0 | 0 |
| Security (TM-1, TM-2, TM-3) | 3 | 3 | 0 | 0 |
| **Total** | 21 | 19 | 0 | 2 |

The inspection items are done by reading DECISIONS.md, the test-docs record
and the diff. They do not need a running application.
