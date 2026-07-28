# Verification Document: tmux Session Rows in the New-Tab Chooser

## Overview

**Feature**: tmux-session-rows / **SPEC.md**: `feature-docs/tmux-session-rows/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/tmux-session-rows/IMPLEMENTATION.md`

## Build Verification

| Component | Command | Expected |
|-----------|---------|----------|
| rust (GUI) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit 0, no errors |
| cli (no default features) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit 0, no errors |
| windows (cross) | `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` | exit 0, no errors |

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no numeric target for this project; every acceptance criterion of task0001 must be covered by at least one test.
- Known pre-existing flakiness: `tabs::tests::*` fail non-deterministically under default parallelism (documented project-wide). Re-run with `-- --test-threads=1` before treating a `tabs::tests` failure as a regression, and confirm the feature diff does not touch `src-tauri/src/tabs.rs`.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Parse a multi-line list-sessions response | One entry per non-empty line, names verbatim; blank and trailing lines ignored | Unit |
| TS-2 | Enumeration failure paths: non-zero exit, empty output, spawn failure, missing binary | Exactly one fallback entry for that socket; no error, no panic | Unit |
| TS-3 | Row label construction across the three cases | `tmux: {session}` (default socket), `tmux: {socket}: {session}` (named socket), `tmux: {socket}` (fallback) | Unit |
| TS-4 | Attach argument construction | Session entry: socket path + attach-session + exact-match target. Fallback entry: socket path + plain attach. All discrete arguments | Unit |
| TS-5 | Row/choice decode with session rows present | `Global → profiles → tmux` ordering preserved; each index resolves to the intended entry | Unit |
| TS-6 | Deterministic ordering | Entries sorted by socket name, then session name | Unit |
| TS-7 | Fast path parity | Profiles empty and entries empty spawns directly; profiles empty and entries present opens the chooser | Unit |
| TS-8 | Bounded wait against a non-answering socket | Returns within the bound with a fallback entry; child terminated and reaped | Unit |
| TS-9 | Edge-case session names: prefix pair, space, non-ASCII, same name on two sockets | Labels and arguments carry the name verbatim; the exact-match marker distinguishes the prefix pair; both sockets' rows remain distinguishable | Unit |
| TS-10 | Existing chooser and discovery regression net | Existing tests in `tmux_sockets`, `profile_selector`, and the chooser paths of `app` pass unchanged in intent | Unit |

## Code Quality Verification

- Format: not enforced project-wide (`format_command` empty in workflow.yaml). Match the surrounding file's existing style.
- Static analysis: the `cargo check` runs above are the gate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements implemented and tested | Requirement coverage table below; all TS pass |
| SC-2 | All unit test scenarios pass | `cargo test --lib` green (modulo the documented `tabs::tests` flakiness) |
| SC-3 | Windows cross-build and CLI-only build keep compiling | The two build commands above exit 0 |
| SC-4 | Manual scenario M-1 confirmed | Manual Testing section below |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-6, TS-8 |
| FR2 | task0001 | TS-3, TS-5, TS-9, M-1 |
| FR3 | task0001 | TS-4, TS-9, M-1 |
| FR4 | task0001 | TS-7 |
| NFR1 | task0001 | TS-8, M-1 |
| NFR2 | task0001 | cli and windows build commands (SC-3) |
| NFR3 | task0001 | TS-10, M-1 |
| NFR4 | task0001 | TS-4 (arguments are discrete values, no shell string anywhere on the path) |

## E2E Testing

No E2E infrastructure exists in this project; none is added by this feature.

## Manual Testing (E2E Not Possible)

- [ ] M-1: With `tmux new -d -s alpha` and `tmux new -d -s beta` running on the default server, open the `+` menu. Expect rows `tmux: alpha` and `tmux: beta`. Select `beta` and confirm the new tab is attached to session `beta`. Requires a GUI session and a real tmux install, so it is confirmed by the user rather than by the verify phase.
- [ ] M-2: With no tmux server running, open the `+` menu and confirm the chooser looks exactly as before (no tmux rows) and opens without perceptible delay.

## Performance / Security Verification

- NFR1: the bounded-wait test (TS-8) asserts the 300 ms upper bound directly; M-1/M-2 confirm there is no perceptible delay in practice.
- NFR4: verified by inspection plus TS-4 — the enumeration and attach paths must contain no shell invocation and no string concatenation of a path or session name into a command.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Test scenarios | 10 | 10 | 0 | 0 |
| Success criteria | 4 | 3 | 0 | 1 |
| Manual scenarios | 2 | 0 | 0 | 2 |
