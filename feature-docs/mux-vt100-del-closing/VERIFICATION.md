# Verification Document: mux-vt100-del-closing

## Overview
**Feature**: mux-vt100-del-closing / **SPEC.md**: `feature-docs/mux-vt100-del-closing/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-vt100-del-closing/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/mux-vt100-del-closing/THREAT-MODEL.md`

Run every command from the integration worktree root. Do not change into `src-tauri/` first.

## Build Verification
- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only, NFR3): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: both commands exit with code 0 and report no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, and all tests pass
- Coverage target: not measured, because workflow.yaml configures no coverage tool. Every Acceptance Criterion of task0001 maps to at least one scenario below.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | (SPEC AC-1) In `src-tauri/src/mux/ipc/handlers/tests.rs`, call `render_pane_tail` with the D2-shaped tail ESC [ 6 DEL H e l l o CR LF, empty screen, 5 lines, 80 columns | Returns exactly "Hello". Fails on the code before the change | Unit |
| TS-2 | (SPEC AC-2) In `src-tauri/src/mux/session/pane/tests.rs`, unix only, call `MuxPane::from_restored` (alt-screen false) with a ring holding ESC [ 6 DEL H e l l o | Shadow screen row 0 is "Hello", and the pane scrollback `read_all` equals the input bytes, including 0x7F. Fails on the code before the change | Unit |
| TS-3 | (SPEC AC-3) In `src-tauri/src/mux/ipc/handlers/tests.rs`, call `render_pane_tail` with ESC DEL H e l l o CR LF, empty screen, 5 lines, 80 columns | Returns exactly "Hello". Fails on the code before the change | Unit |
| TS-4 | (SPEC AC-4) In `src-tauri/src/mux/scrollback_filter/tests.rs`, run a table of inputs and expected replay copies | DEL becomes CAN in CSI entry, in CSI parameter, after a lone ESC, and at the D3-shaped DEL ending an open CSI. DEL is kept in ground, in OSC, DCS and APC bodies, after a designator introducer, and as the second of two DELs in a CSI. DEL-free inputs (including a complete device query and a raw CAN in a CSI) and the empty input come back unchanged. Length is preserved on every row | Unit |
| TS-5 | (SPEC AC-5) In `src-tauri/src/mux/ipc/handlers/tests.rs`, run the state-reporting strip on ESC [ 6 from ground, then on n H e l l o CR LF with the carried state, and concatenate the outputs | The output contains 0x7F, and `render_pane_tail` of it (empty screen, 5 lines, 80 columns) returns exactly "Hello" | Unit |
| TS-6 | (SPEC AC-6) Run the `--lib` test suite and the `--no-default-features` check from Build Verification | Every test passes, including the existing scrollback_filter, write_filter, pane and handlers tests, with no existing test edited. `CSI_CLOSING_BYTE` is still 0x7F. The CLI-only check compiles | Integration |
| TS-7 | (Planner-added, THREAT-MODEL.md TM-1) In `src-tauri/src/mux/scrollback_filter/tests.rs`, sweep inputs made of each prefix from {empty, ESC, ESC [, ESC [ 6, ESC (, ESC ] 0 ;}, then each byte value 0x00–0xFF, then DEL | No panic. Output length equals input length. Each output byte equals the input byte, or is CAN only where the input byte is DEL | Unit |
| TS-8 | (Planner-added, NFR2) Inspect the replay-copy function and its call sites | The function walks its input once, with O(1) state besides the output copy. In non-test code it is called only from `render_scrollback_rows` and `MuxPane::from_restored`, never from the PTY reader or the ring-write path | Inspection |

## Code Quality Verification
- Format: workflow.yaml configures no format command (`format_command` is empty)
- Static analysis: none configured beyond the build checks above

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | SPEC AC-1: ReadPane renders "Hello" for the D2-shaped tail | TS-1 |
| SC-2 | SPEC AC-2: the restored shadow screen row 0 is "Hello", and the ring keeps its bytes | TS-2 |
| SC-3 | SPEC AC-3: ReadPane renders "Hello" after a closed lone ESC | TS-3 |
| SC-4 | SPEC AC-4: the replay copy replaces and keeps DEL exactly as specified, and preserves length | TS-4 |
| SC-5 | SPEC AC-5: the reproduction path through the state-reporting strip renders "Hello" | TS-5 |
| SC-6 | SPEC AC-6: existing tests pass unedited, `CSI_CLOSING_BYTE` is 0x7F, the CLI-only check compiles, and the `--lib` suite passes | TS-6, Build Verification |
| SC-7 | Every functional requirement is implemented and tested | Functional Requirements Coverage below |
| SC-8 | Performance goal (NFR2) is met | TS-8 |
| SC-9 | Security: the recorded threat is mitigated (TM-1) | TS-7 |
| SC-10 | Documentation: the new function and the changed call sites carry doc comments that state the contract (IMPLEMENTATION.md, Conventions) | Inspection during review |
| SC-11 | Code review is completed | Review phase result in workflow.yaml |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-5 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-4, TS-7 |
| FR4 | task0001 | TS-3, TS-4 |
| FR5 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-5 |
| NFR1 | task0001 | TS-2, TS-6 |
| NFR2 | task0001 | TS-8 |
| NFR3 | task0001 | TS-6 |
| NFR4 | task0001 | TS-6 |

## Manual Testing (E2E Not Possible)
These require a build that contains the change, running as the mux daemon. TS-1, TS-2 and TS-5 cover the same behavior automatically.
- [ ] MT-1: In a mux pane, print ESC [ 6, wait long enough for the rest to arrive in a separate PTY read, then print n, Hello and a newline. Read the pane with mux read (ReadPane). Expected: a line "Hello", and no "ello" on a lower line.
- [ ] MT-2: With MT-1's output in the pane's scrollback, restore the session so that the pane goes through the restored shadow replay. Read the pane again with mux read. Expected: the restored screen shows "Hello" on the line where it was printed, with no "ello" on a lower line.

## Performance / Security Verification
- NFR2: the conversion makes one pass with O(1) extra state besides the output copy, and runs only in ReadPane rendering and the restored scrollback replay, never in the PTY reader or the ring-write path. Checked by TS-8 (inspection).
- TM-1: the conversion is total (no panic, length-preserving, linear, and using only the existing `WrittenState` transition). Checked by TS-7 (automated sweep), with length also asserted on every TS-4 row.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (default and CLI-only checks) | 2 | 2 | 0 | 0 |
| Unit tests (TS-1 to TS-5, TS-7) | 6 | 6 | 0 | 0 |
| Integration / suite (TS-6) | 1 | 1 | 0 | 0 |
| Inspection (TS-8) | 1 | 0 | 0 | 1 |
| Security (TM-1, through TS-7) | 1 | 1 | 0 | 0 |
| Manual (MT-1, MT-2) | 2 | 0 | 0 | 2 |
| Total | 13 | 10 | 0 | 3 |
