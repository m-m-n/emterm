# Verification Document: mux-bridge-capacity-capture-validation

## Overview
**Feature**: mux-bridge-capacity-capture-validation / **SPEC.md**: `feature-docs/mux-bridge-capacity-capture-validation/SPEC.md` / **IMPLEMENTATION.md**: not written (reduced tier; a single task, no file shared between tasks) / **THREAT-MODEL.md**: `feature-docs/mux-bridge-capacity-capture-validation/THREAT-MODEL.md`

## Build Verification
- Command (component `rust`, default features): `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (component `rust_cli_only`): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command (component `rust`, default features): `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Command (component `rust_cli_only`): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib -- --test-threads=1`
- Focused command (TS-4): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::bridge`
- Coverage target: no numeric coverage gate (project.components configures no coverage tooling); every task0001 Acceptance Criterion maps to at least one scenario below.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | The capacity capture helper receives ClientScrollbackCapacity messages with payloads of 0, 3 and 5 bytes, and with 4-byte payloads of value 0 and of a non-zero value | Returns nothing for the 0, 3 and 5 byte payloads; returns the frame body for both 4-byte payloads | Unit |
| TS-2 | Unix: the forwarding loop receives a valid capacity report (5 lines) and then a length-invalid capacity report; reconnect_and_reattach then runs against a stand-in daemon | Both frames reach the daemon side unchanged and in order; the retained capacity is the valid frame body; the first frame after the reconnect handshake is a capacity frame that decodes to 5 lines | Integration |
| TS-3 | A length-invalid capacity report arrives with no prior valid report | The retained capacity stays empty | Unit (async) |
| TS-4 | Run the bridge test module (focused command above) and the CLI-only cargo check | Every bridge test passes, including capture_if_capacity_captures_capacity_and_only_capacity, forward_loop_forwards_and_captures_capacity_frame_unchanged, forward_loop_capacity_capture_replaces_and_is_unaffected_by_other_messages and every reconnect_and_reattach_* test; the check exits 0 | Regression / Build |

## Code Quality Verification
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` (component `rust` format_command)
- Static analysis: none configured in project.components; the default-features and CLI-only cargo check runs under Build Verification are the compile-time gate.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | After valid report, length-invalid report and upgrade-driven reconnect, the first frame resent after the reconnect handshake is the earlier valid capacity body | TS-2 |
| AC-2 | A test in src-tauri/src/mux/bridge/tests.rs covers valid report, length-invalid report, reconnect_and_reattach against a stand-in daemon, and decodes the resent capacity to the valid value | TS-2 (the test lives in src-tauri/src/mux/bridge/tests.rs) |
| AC-3 | The capacity capture helper returns nothing for payload lengths other than 4 and the frame body for a 4-byte payload | TS-1 |
| AC-4 | Both the valid and the length-invalid capacity frames are forwarded to the daemon socket unchanged | TS-2 |
| AC-5 | The existing bridge tests continue to pass | TS-4 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-1, TS-2 |
| NFR1 | task0001 | TS-4; scope check: the source files changed by the feature are limited to src-tauri/src/mux/bridge/mod.rs and src-tauri/src/mux/bridge/tests.rs |
| NFR2 | task0001 | TS-4 (CLI-only cargo check) and Build Verification (both components) |

## Manual Testing (E2E Not Possible)
None. The reproduction steps need a length-invalid capacity frame, which the in-tree sender never produces; TS-2 runs the same sequence (valid report, length-invalid report, upgrade-driven reconnect) against a stand-in daemon.

## Performance / Security Verification
- TM-1: the bridge retains a capacity frame body only when its payload decodes as a valid capacity payload and keeps the previous value otherwise — checked by TS-1 (0, 3 and 5 byte payloads are not retained; 4-byte payloads are) and TS-2 (valid report, length-invalid report, reconnect resends the valid value of 5 lines).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (default features, CLI-only) | 2 | 2 | 0 | 0 |
| Unit tests (TS-1, TS-3) | 2 | 2 | 0 | 0 |
| Integration tests (TS-2) | 1 | 1 | 0 | 0 |
| Regression / build checks (TS-4) | 1 | 1 | 0 | 0 |
| Code quality (format) | 1 | 1 | 0 | 0 |
| Scope check (NFR1) | 1 | 1 | 0 | 0 |
| Security (TM-1) | 1 | 1 | 0 | 0 |
| **Total** | **9** | **9** | **0** | **0** |
