# Verification Document: logging.rs RUST_LOG Process Env Isolation

## Overview

**Feature**: logging-rust-log-isolation
**SPEC.md**: `doc/tasks/logging-rust-log-isolation/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/logging-rust-log-isolation/IMPLEMENTATION.md`

This document defines how the feature is verified. Build, test, and code-quality verifications are automated. The user-visible bug (fnm INFO leak on Windows pwsh startup) and the cross-cutting logger behavior (format / log-file persistence) are verified manually.

## Build Verification

- **Linux quick check (default features)**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  Expected: exit code 0, no new warnings.
- **Linux quick check (CLI-only)**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  Expected: exit code 0. CLI-only build path also goes through `logging::init`.
- **Windows cross-build (optional)**:
  `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`
  Expected: exit code 0. Skipped in `sdd.5-check`; required only if the user needs an updated Windows binary for TS-6.

## Test Verification

- **Command**:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  Expected: all existing tests pass plus the four new `resolved_filters_*` tests.
- **Coverage target**: 100% of `resolved_filters` branches (None / Some("") / Some(non-empty) — all covered by TS-1..TS-4).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `resolved_filters(None)` | Returns `DEFAULT_FILTER` (`"info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn"`) | Unit |
| TS-2 | `resolved_filters(Some(""))` | Returns `DEFAULT_FILTER` | Unit |
| TS-3 | `resolved_filters(Some("debug"))` | Returns `"debug"` | Unit |
| TS-4 | `resolved_filters(Some("wgpu_core=info,naga=trace"))` | Returns `"wgpu_core=info,naga=trace"` | Unit |
| TS-5 | Process env stays clean on Linux | After eMterm launches, `cat /proc/$(pidof emterm)/environ \| tr '\0' '\n' \| grep RUST_LOG` returns no match. Inside a spawned shell tab, `echo $RUST_LOG` is empty. | Manual (Linux) |
| TS-6 | fnm INFO leak resolved on Windows | On the host where the bug was originally reported, opening a pwsh tab produces no `INFO  fnm::version_files .nvmrc. exists?` lines at startup. | Manual (Windows) |
| TS-7 | Explicit `RUST_LOG=debug emterm` still propagates | (a) eMterm's stderr shows debug-level entries; (b) inside a spawned shell, `echo $RUST_LOG` (or `$env:RUST_LOG`) reports `debug`. | Manual (Linux preferred, Windows acceptable) |
| TS-8 | Log format and persistence unchanged | Trigger a known `log::warn!` site. The stderr line is `[WARN][NATIVE-POC] ...`. In a release build, the same warn record appears in `~/.local/share/net.laser5.app.emterm/logs/emterm.log`. | Manual (Linux release build) |

## Code Quality Verification

- **Format check**:
  `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check src-tauri/src/logging.rs`
  Expected: no diff.
- **Unsafe-count delta**:
  Baseline (commit `fae9141`): `grep -c 'unsafe\s*{' src-tauri/src/logging.rs` returns **1** (single `unsafe { std::env::set_var(...) }` block at line 192).
  Expected post-change: same `grep` returns **0**.
  Net decrease MUST be exactly 1. Run the same grep before and after the Phase 1 patch to verify.

## File Structure Verification

### Files to Create

- (none)

### Files to Modify

- `src-tauri/src/logging.rs`
  - Add `DEFAULT_FILTER` const.
  - Add `resolved_filters(env_value: Option<&str>) -> String` private helper.
  - Rewrite `init()`'s `INIT.call_once` body (drop `unsafe { std::env::set_var(...) }`, switch to `Builder::new() + parse_filters`).
  - Update `init()` doc comment.
  - Add `resolved_filters_*` unit tests in the existing `#[cfg(test)] mod tests`.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR5 implemented and verified by unit tests | TS-1 … TS-4 pass; manual review of `init()` body and doc comment. |
| SC-2 | `cargo test --lib` passes on Linux | Re-run the Test Verification command. |
| SC-3 | `cargo check` (default + `--no-default-features`) passes | Re-run the Build Verification commands. |
| SC-4 | `unsafe` count in `logging.rs` decreased by exactly 1 | grep before vs. after (see Code Quality Verification). |
| SC-5 | Manual TS-5 / TS-7 / TS-8 pass on Linux | Recorded in VERIFICATION_RESULT.md. |
| SC-6 | Manual TS-6 passes on Windows | Recorded in VERIFICATION_RESULT.md by the user / on the Windows host. |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (Drop std::env::set_var) | Phase 1 | Document review (no `set_var` in logging.rs) + SC-4 |
| FR2 (In-process filter via parse_filters) | Phase 1 | Document review + TS-7 (filter actually applied) |
| FR3 (Pure resolved_filters helper) | Phase 1 | TS-1 … TS-4 |
| FR4 (Existing logger behavior preserved) | Phase 1 | TS-8 |
| FR5 (Unit tests for resolved_filters) | Phase 1 | TS-1 … TS-4 |
| NFR1 (No new unsafe; net -1) | Phase 1 | SC-4 |
| NFR2 (No behavioral regression on log output) | Phase 1 | TS-8 |
| NFR3 (Change confined to logging.rs) | Phase 1 | Document review (git diff scope) |
| NFR4 (init() doc comment updated) | Phase 1 | Document review |

## E2E Testing

This project has no E2E framework (`test/README.md` confirms). Section omitted.

## Manual Testing (E2E Not Possible)

Performed by the developer / user during `sdd.6-verify`:

- [ ] **TS-5 — Linux process env stays clean**: launch eMterm; in another terminal, run `cat /proc/$(pidof emterm)/environ | tr '\0' '\n' | grep -c '^RUST_LOG='`. Expected: 0 (no match). Open a tab in eMterm, run `echo "RUST_LOG=$RUST_LOG"`. Expected: `RUST_LOG=` (empty).
- [ ] **TS-6 — Windows fnm INFO leak resolved**: on the host where the bug was originally reported, build / install the new eMterm, launch, open a pwsh tab with the existing `$PROFILE` running `fnm env --use-on-cd | Out-String | Invoke-Expression`. Expected: no `INFO  fnm::version_files .nvmrc. exists?` lines at startup.
- [ ] **TS-7 — explicit RUST_LOG still propagates**: launch `RUST_LOG=debug emterm` (Linux) or set `$env:RUST_LOG = "debug"` then launch (Windows). Confirm (a) eMterm's stderr shows `[DEBUG][NATIVE-POC] ...` lines, (b) inside a spawned shell, `echo $RUST_LOG` reports `debug`.
- [ ] **TS-8 — log format and persistence unchanged**: in a release build, trigger a known `log::warn!` site (e.g. an intentional invalid action). Confirm (a) the stderr line matches `[WARN][NATIVE-POC] ...`, (b) the same record appears in `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.

## Performance Verification

Not applicable. The patch removes one env-table write per startup; no hot path is affected. No benchmark.

## Security Verification

Not applicable. Local logger init only; no untrusted input parsed; one `unsafe` removed.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (Linux check, CLI-only check) | 2 | 0 | 0 |
| Test | 1 command (`cargo test --lib`) | 1 | 0 | 0 |
| Test scenarios | 8 (TS-1 … TS-8) | 4 (TS-1 … TS-4) | 0 | 4 (TS-5 … TS-8) |
| Code quality | 2 (rustfmt --check, unsafe-count delta) | 2 | 0 | 0 |

## Phase 3 Execution Results (sdd.4-implement)

Recorded after the Phase 1 implementation patch landed in
`src-tauri/src/logging.rs`. Manual scenarios (TS-5 / TS-6 / TS-7 / TS-8)
remain pending for `sdd.6-verify`.

### Build Verification

| Check | Command | Exit | Notes |
|-------|---------|------|-------|
| Linux default features | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | 0 | `Finished dev profile ... in 2.24s`. No new warnings. |
| Linux CLI-only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | 0 | `Finished dev profile ... in 0.31s`. |
| Windows cross-build | (skipped) | n/a | Only required if a fresh Windows binary is needed for TS-6. |

### Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Result: **1908 passed; 0 failed; 3 ignored; 0 measured**.
- New `logging::tests` cases:
  - `resolved_filters_none_returns_default` — pass
  - `resolved_filters_empty_returns_default` — pass
  - `resolved_filters_passes_user_value` — pass
  - `resolved_filters_passes_module_scoped_value` — pass
- Existing `logging::tests` (`offset_suffix_renders_rfc3339_style`, `timestamp_carries_an_explicit_offset`) still pass.

### Code Quality Verification

| Check | Command | Result |
|-------|---------|--------|
| rustfmt | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check src-tauri/src/logging.rs` | no diff |
| `unsafe` count | `grep -c 'unsafe\s*{' src-tauri/src/logging.rs` | **0** (was 1 pre-change; net −1 as required by NFR1) |

### File Structure Verification

- [x] `src-tauri/src/logging.rs`
  - Added `DEFAULT_FILTER` const.
  - Added `resolved_filters(env_value: Option<&str>) -> String` private helper with doc comment.
  - Rewrote `init()`'s `INIT.call_once` body (removed `unsafe { std::env::set_var(...) }`, switched to `Builder::new() + parse_filters`).
  - Updated `init()` doc comment to state "Configures an in-process filter via `env_logger::Builder::parse_filters`; the process env table is never modified".
  - Added four `resolved_filters_*` unit tests.

### Existing E2E Regression (Phase 3.8)

Skipped — the project has no E2E framework (see `test/README.md`).

### Manual Verification (Phase 2) Status

Remains pending; to be executed during `sdd.6-verify`:

- [ ] TS-5 — Linux process env stays clean
- [ ] TS-6 — Windows fnm INFO leak resolved
- [ ] TS-7 — explicit `RUST_LOG` still propagates
- [ ] TS-8 — log format and persistence unchanged (Linux release build)
