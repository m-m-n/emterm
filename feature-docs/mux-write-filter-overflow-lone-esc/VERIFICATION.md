# Verification Document: mux-write-filter-overflow-lone-esc

## Overview
**Feature**: mux-write-filter-overflow-lone-esc / **SPEC.md**: `feature-docs/mux-write-filter-overflow-lone-esc/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-write-filter-overflow-lone-esc/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/mux-write-filter-overflow-lone-esc/THREAT-MODEL.md`

Every command runs from the integration worktree root, without `cd`, with the `CARGO_TARGET_DIR` shown.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors
- CLI-only build (NFR5): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every test of the new module `mux::ipc::pty_spawn::tests::overflow_lone_esc` passes; no existing test fails and no existing test's assertion was edited.
- Coverage target: no coverage tool is configured for this crate. Coverage is judged by traceability: every task0001 Acceptance Criterion except AC-9 maps to at least one named test in `test-docs/mux-write-filter-overflow-lone-esc/task0001.tests.yaml`, and AC-9 is verified by TS-8.
- A failure of a test outside this feature that the task record lists among its baseline failures is attributed to the baseline, not to this feature.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | (SPEC AC-1; FR1, FR6; task0001 AC-1) Hold an OSC at the 512 KiB cap, overflow it with a continuation ending in BEL, `abc`, a lone ESC, then feed `[6n` | No `ESC[6n` in the concatenated written bytes; term_core fed them gives no response | Unit |
| TS-2 | (SPEC AC-2; FR1, NFR2; task0001 AC-2) Each strip target (`[6n`, `[5n`, `[c`, OSC 777 viewer launch, OSC 9999 emterm-md, OSC 777 agent-status, Kitty APC, SIXEL DCS) as the continuation after the overflow-ending ESC, also after the endings ESC ESC, `ESC[6` ESC and an open string body ending in ESC | Each construct absent from the written bytes; total written bytes equal the feeding where the final ESC and its continuation arrive in one non-overflowing call | Unit |
| TS-3 | (SPEC AC-3, AC-4; FR2, FR3, FR4, FR5; task0001 AC-3, AC-4) State right after the overflowing call, and non-strip continuations after it | `pending()` is the single ESC (`pending_len()` 1, also for every TS-2 ending), held construct start 0, written bytes = strip of the run without its final ESC, written state = term_core end state (Ground), no designator wait, no carried completion, held byte attributed to the overflowing call's dims; `x` / `[H` written after the ESC with no byte lost; `]0;t` BEL reported as a carried completion; `\` after an open string body written as ESC `\` | Unit |
| TS-4 | (SPEC AC-5; FR4; task0001 AC-5) Reader fallback closing and a cut at the start of the next call after the held ESC, compared with term_core fed the raw stream with a 47 / 1047 / 1049 `h` / `l` pair in place of the cut (`view_after_a_cut` oracle) | Nothing written for the held ESC, pending empty, state Ground, a second fallback writes nothing, replay matches the reference with no cursor-position report; one DEL after the open-CSI ending, nothing after the open-string-body ending | Unit |
| TS-5 | (SPEC AC-6; FR3, NFR3; task0001 AC-6) Overflow regression matrix for runs outside FR1: ESC + a complete removed construct (R9 form), `ESC ( ESC`, `ESC[6` + a removed construct, an overflow followed by a cut in the same call, plain-byte endings | Written bytes equal the write-path strip of the whole run (plus the one closure where a cut follows); state unchanged from today; pending empty | Unit |
| TS-6 | (SPEC AC-7; FR1, FR6; task0001 AC-7) Production reader: reads grow an OSC past the cap, the crossing read ends in BEL, `abc`, ESC, the next read is `[6n` | The pane's ring contains no `ESC[6n`; term_core replaying the ring gives no cursor-position report | Integration |
| TS-7 | (SPEC AC-8; NFR1, NFR3, NFR5; task0001 AC-8) Full `--lib` suite and the CLI-only cargo check | Both exit 0; the existing budget tests (BUDGET, 10 s) and the overflow tests named in SPEC NFR3 pass unchanged | Integration |
| TS-8 | (NFR4; task0001 AC-9; added at planning because SPEC lists no scenario for NFR4) Review the doc comments of `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | No doc comment says pending is empty or always empty right after the overflow flush; the cut-aware feed's "Overflow." paragraph and the pending accessor's postcondition state "empty, or exactly the one held live lone ESC when the flush was in the call's last segment" | Manual (review) |

## Code Quality Verification
- Format: no `format_command` is configured in workflow.yaml; do not run a crate-wide formatter (it rewrites unrelated files).
- Static analysis: none configured.
- Review checks on the integrated diff:
  - NFR1: the overflow branch strips each flushed run once (over the run without its final byte when that byte is ESC); no loop over the fed bytes is added; the extra work is O(1).
  - NFR3: no file outside task0001's `files` changed (`scrollback_filter.rs`, `pty_spawn/mod.rs` and `pty_spawn/suppressed_output.rs` are untouched); no existing test assertion edited and no existing test renamed.
  - NFR5: the diff adds no platform-gated code and no GUI-only crate, so the Windows build compiles the same code as the Linux build.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below; TS-1 to TS-6 pass |
| SC-2 | All test scenarios pass | TS-1 to TS-7 pass; TS-8 review passes |
| SC-3 | Performance meets specified goals | TS-7 (budget tests) and the NFR1 review check |
| SC-4 | Security requirements are satisfied | TM-1 and TM-2 items below |
| SC-5 | Documentation is complete | TS-8 |
| SC-6 | Code review is completed | The review phase ends with no residual critical / high finding |
| SC-7 | The full `--lib` test suite and the `--no-default-features` cargo check pass (SPEC AC-8) | TS-7 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-6 |
| FR2 | task0001 | TS-3 |
| FR3 | task0001 | TS-3, TS-5 |
| FR4 | task0001 | TS-3, TS-4 |
| FR5 | task0001 | TS-3 |
| FR6 | task0001 | TS-1, TS-6 |
| NFR1 | task0001 | TS-7, review check NFR1 |
| NFR2 | task0001 | TS-2 |
| NFR3 | task0001 | TS-5, TS-7, review check NFR3 |
| NFR4 | task0001 | TS-8 |
| NFR5 | task0001 | TS-7, review check NFR5 |

## Manual Testing (E2E Not Possible)
- [ ] TS-8: doc-comment review of `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` (NFR4).

## Performance / Security Verification (if applicable)
- NFR1: the existing budget tests (BUDGET, 10 s), including `escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget`, pass in the TS-7 run; the overflow branch's extra work is O(1) (review check NFR1).
- TM-1: the overflow flush holds a final live lone ESC so the next read strips it with its continuation, and a cut or fallback drops it with at most the one closure its written state decides — checked by TS-1, TS-2 and TS-4 (filter level) and TS-6 (production reader): no strip target in executable form in the written bytes or the ring, and no cursor-position report on term_core replay.
- TM-2: pending holds at most one byte after an overflow flush and the flushed run is stripped in a single pass — checked by TS-3 (`pending_len()` is 1 for every ending), TS-7 (budget tests pass) and review check NFR1.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 8 | 7 | 0 | 1 |
| Code quality review | 3 | 0 | 0 | 3 |
| Performance (NFR1) | 1 | 1 | 0 | 0 |
| Security (TM-1, TM-2) | 2 | 2 | 0 | 0 |
| Total | 16 | 12 | 0 | 4 |
