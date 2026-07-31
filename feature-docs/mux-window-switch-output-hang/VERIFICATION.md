# Verification Document: mux-window-switch-output-hang

## Overview
**Feature**: mux-window-switch-output-hang / **SPEC.md**: `feature-docs/mux-window-switch-output-hang/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-window-switch-output-hang/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  (run with `-- --test-threads=1` per project convention for `tabs.rs`
  off-thread-worker test determinism)
- Coverage target: every Acceptance Criterion in task0001 has a passing test

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Pane output channel full for pane A; snapshot requested for pane A; client message for pane B arrives | Connection processes pane B's message within a bounded time (no hang) | Unit/Integration |
| TS-2 | Pane output channel full for pane A; snapshot requested for pane B; pane A output continues | Connection continues forwarding pane A output and processes further messages | Unit/Integration |
| TS-3 | Pane has N PTY output chunks queued, then a snapshot is requested for it | Client observes the N chunks before the snapshot chunk, in original relative order | Unit/Integration |
| TS-4 | Channel/queue bound after the fix | Bound remains finite; backpressure still observable (not silently unbounded) | Unit |
| TS-5 | Full existing mux/tabs test suite | No new failures beyond documented pre-existing/unrelated ones | Regression |
| TS-6 | Manual repro: `seq 1 10000000` in one pane, switch windows repeatedly | Client stays responsive; other tabs usable; no hang after detach/reattach | Manual |

## Code Quality Verification
- Format: not enforced via a workflow.yaml command (project uses rustfmt via
  PostToolUse hook per project formatting policy, not a standalone
  `format_command`)
- Static analysis: covered by the build command (`cargo check`)

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR4 implemented and tested | task0001 Acceptance Criteria AC-1..AC-4 pass |
| SC-2 | All test scenarios pass | TS-1..TS-6 pass |
| SC-3 | No regression in existing mux/tabs test suite | TS-5 passes |
| SC-4 | Code review completed | review phase `residual_critical_high == 0` |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|---------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001 | TS-1, TS-2 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-4 |
| NFR1 | task0001 | TS-5 |
| NFR2 | task0001 | TS-6 (manual — confirms client-side replay unaffected) |

## E2E Testing

No existing E2E framework detected in this repository (per create-spec
Phase 0.5 scan). No automated E2E scenario is added by this feature.

## Manual Testing (E2E Not Possible)

- [ ] TS-6: Run `seq 1 10000000` in one eMterm mux pane; while it is still
      producing output, switch to another mux window/tab several times in
      quick succession; confirm the client remains responsive, other tabs
      accept input, and (if the client is closed and reattached mid-output)
      no tab remains stuck.

## Performance / Security Verification

- Backpressure characteristics (NFR-adjacent, from FR4): the pane output
  channel/queue must remain bounded after the fix — verified by TS-4.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Functional | FR1-FR4 | TS-1..TS-4 | - | - |
| Regression | NFR1 | TS-5 | - | - |
| Manual repro | NFR2 | - | - | TS-6 |
