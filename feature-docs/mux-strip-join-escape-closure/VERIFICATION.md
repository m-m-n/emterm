# Verification Document: mux-strip-join-escape-closure

## Overview
- **Feature**: mux-strip-join-escape-closure
- **SPEC.md**: `feature-docs/mux-strip-join-escape-closure/SPEC.md`
- **IMPLEMENTATION.md**: `feature-docs/mux-strip-join-escape-closure/IMPLEMENTATION.md`

Scope: regression tests and a decision record. No production change. Run every command from the integration worktree root.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command for the CLI-only build (NFR1): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: both exit with code 0 and report no errors.

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not measured, because `project.components` configures no coverage tooling. Verification passes when every scenario below passes.
- Known unrelated flakiness:
  - The tabs replay tests can fail nondeterministically under parallel runs; they are stable with one test thread.
  - The tmux socket discovery tests rarely fail under parallel runs.

  Re-run a failure of these tests before attributing it to this feature.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Write filter, one cut-free feed of `ESC ESC ESC[6n[6n[5n`. Then the snapshot strip on the resulting ring, then replay of the snapshot output in a fresh term_core (SPEC AC-1). | The ring is exactly `ESC ESC DEL [6n[5n`. The stripped scrollback equals the ring. The replay response buffer is empty. | Unit |
| TS-2 | Table of all eight removed construct kinds × continuations {`c`, `(0` + probe text}. Each case: one cut-free write-filter feed, the snapshot strip, term_core replay, compared with the raw-stream reference with the construct's own effect excluded (SPEC AC-2). | Rows, cursor and responses equal the reference in every case. The prefix text stays on screen (no reset). The probe text is displayed as typed (no G0 switch). No response is produced. | Unit |
| TS-3 | Rerun the existing `mux::ipc::pty_spawn::tests::strip_concat_query::strip_concat_one_call_closes_an_open_csi_at_every_removed_construct` (SPEC AC-3). | The test passes unchanged. `test-docs/mux-strip-join-escape-closure/task0001.tests.yaml` lists it under the acceptance criterion for FR3. | Unit (existing) |
| TS-4 | Run the `--lib` test command and the `--no-default-features` check (SPEC AC-5). | Both exit with code 0. | Integration (build) |
| TS-5 | Inspect `feature-docs/mux-strip-join-escape-closure/DECISIONS.md` for the scenario mapping (FR4; SPEC AC-4). Added at create-plan because SPEC.md has no scenario for FR4. | DECISIONS.md maps scenarios 1-3 to mux-strip-concat-query-closure D1, with the written bytes and the pinning test for each. It states why DEL is inert in term_core: DEL cancels an open CSI, and right after a lone ESC it is an ignored unknown escape final. It states that the task-proposed CAN / `closure_for` move is superseded by D1, and that `closure_for` stays the cut-only closure. | Inspection |
| TS-6 | Inspect the same DECISIONS.md for the kept-string-body join residual (FR5; SPEC AC-4). Added at create-plan because SPEC.md has no scenario for FR5. | The residual entry gives the mechanism, the `ESC]11;` + removed construct + `?BEL` reproduction, and the impact (OSC 11 response still inducible, with the reply path being a later live continuation). It names the condition, the wrap dump block at `snapshot_bytes.rs:490`, and lists both unchanged pinned expectations from SPEC FR5. | Inspection |
| TS-7 | Inspect `git diff` from `workflow.implement.base_commit` to the integration branch tip (NFR2; SPEC AC-5). Added at create-plan because SPEC.md has no scenario for NFR2. | The diff shows no change to non-test code under `src-tauri/src` and no change under `crates/term_core`. Changes to the sibling test modules are visibility-only. No existing test's expectations or name change. No predecessor feature's DECISIONS.md or test-docs record changes. | Inspection |

## Code Quality Verification
- Format: workflow.yaml configures no `format_command`, so no format check runs. A crate-wide formatter run is not part of verification.
- Static analysis: none configured beyond the build check above.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | The Functional Requirements Coverage table below. TS-1, TS-2, TS-3, TS-5 and TS-6 pass. |
| SC-2 | All test scenarios pass | TS-1 to TS-7 |
| SC-3 | Documentation is complete (DECISIONS.md per FR4 and FR5) | TS-5, TS-6 |
| SC-4 | Code review is completed | The review step in workflow.yaml is completed with no residual critical or high finding |
| SC-5 | No production-code change and no change to existing test expectations or names (NFR2) | TS-7 |
| AC-1 | SPEC AC-1 (FR1) | TS-1 |
| AC-2 | SPEC AC-2 (FR2) | TS-2 |
| AC-3 | SPEC AC-3 (FR3) | TS-3 |
| AC-4 | SPEC AC-4 (FR4, FR5) | TS-5, TS-6 |
| AC-5 | SPEC AC-5 (NFR1, NFR2, NFR3) | TS-4, TS-7, plus the NFR3 timing check below |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-5 |
| FR5 | task0001 | TS-6 |
| NFR1 | task0001 | TS-4 |
| NFR2 | task0001 | TS-7 |
| NFR3 | task0001 | TS-1, TS-2 (oracle convention), plus the timing check below |

## Manual Testing (E2E Not Possible)
- None. The feature has no user-facing behavior change, no E2E framework was detected, and the design step was skipped, so there is no mockup comparison.

## Performance / Security Verification
- NFR3: each new test finishes within the existing 10 s per-test budget. Check the per-test duration in the `--lib` run.
- TM-1: the predecessor closure (mux-strip-concat-query-closure D1) prevents a join into a query, both in the one-call ring and across the write strip -> snapshot strip -> term_core chain. Two checks cover this:
  - TS-3: the existing one-call test passes.
  - TS-1: the ring is `ESC ESC DEL [6n[5n`, the snapshot leaves it unchanged, and replay gives zero responses.

  The task0001 AC-7 control passes as well: the replay term_core answers `ESC[5n`.
- TM-2: the same closure prevents a RIS or a G0 designation across a removal. TS-2 covers this: across eight kinds × two continuations, rows, cursor and responses equal the reference, the prefix is kept, the probe is displayed as typed, and no response is produced.

  The task0001 AC-7 controls pass as well: `ESC c` resets, and `ESC(0` changes the probe text.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (default check, CLI-only check) | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-4) | 4 | 4 | 0 | 0 |
| Document / diff inspection (TS-5 to TS-7) | 3 | 0 | 0 | 3 |
| Performance (NFR3 timing) | 1 | 1 | 0 | 0 |
| Security (TM-1, TM-2) | 2 | 2 | 0 | 0 |
| **Total** | 12 | 9 | 0 | 3 |

The inspection items are carried out by reading DECISIONS.md and the diff. They do not need a running application.
