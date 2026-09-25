# Verification Document: test-seam-serialization

## Overview

**Feature**: test-seam-serialization / **SPEC.md**:
`feature-docs/test-seam-serialization/SPEC.md` / **IMPLEMENTATION.md**:
`feature-docs/test-seam-serialization/IMPLEMENTATION.md`

All commands run from the integration worktree root (never `cd` into
`src-tauri/`). If `build.rs` reports missing `viewer/dist` / `settings/dist`
bundles, run `bun run build:viewer` and `bun run build:settings` first
(`core-commands.md`).

## Build Verification

Three configurations (workflow.yaml `project.components`: `main`,
`main-cli`, `main-windows`):

- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests`
- Expected: exit code 0 for each, and zero `warning:` lines attributed to
  the `emterm` crate.

## Test Verification

- Command (workflow.yaml `main.test_command`, default parallelism, no
  `--test-threads=1`):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no line-coverage threshold (no coverage tooling in the
  project); verification is scenario-based (table below).
- Failure classification (SPEC A-1, AC-1):
  - A failure of a `RESTART_REQUIRED`-related or SFTP-seam-related test is
    never excluded.
  - A failure confirmed as a known unrelated flake (e.g. the `tabs.rs`
    replay test, `test/README.md:40`) is rerun and recorded as an
    exception.
  - A run with any such exception is not reported as an overall pass.
  - Any new failure is not excluded.

### Test Scenarios from SPEC.md

TS-1 to TS-7 come from SPEC.md. TS-8 to TS-11 were added at create-plan
because SPEC.md defines no scenario for FR5, NFR1, NFR2 or NFR4.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | FR4, AC-5: the `timing.rs` tests that call the seams (`frame_work_pending_true_when_progress_channel_nonempty_and_consumes_nothing`, `frame_work_pending_true_when_result_channel_nonempty_and_consumes_nothing`, `next_toast_deadline_some_when_pretoast_progress_event_pending`) run after the seams route through the helpers | All three pass; the new `service.rs` seam-content unit test passes | Unit |
| TS-2 | FR6, AC-5: `every_progress_and_result_send_site_routes_through_the_wake_helpers` runs with the widened scan region; the red-case test feeds synthetic text with a bare progress / result send in a seam block, the same send only inside a `mod tests` body, and text with no `mod tests` boundary | The structural check passes on the real file. The red-case test detects both seam-block violations, reports none for the `mod tests`-only case, and reports the missing boundary. The task's test record notes the widened check failed against the pre-change bare seams | Unit |
| TS-3 | FR1, FR2, AC-2, AC-3: the four `self_exec.rs` restart tests and the nine guarded `timing.rs` tests run in the parallel `--lib` run | All 13 pass, including `restart_flag_test_guard_stays_usable_after_a_panicking_span` | Unit |
| TS-4 | FR3, AC-4: inspect `self_exec.rs` | No "single-threaded" wording; the `RESTART_FLAG_TEST_LOCK` doc describes default parallel execution and exclusivity through the guard | Inspection |
| TS-5 | FR7: in `src-tauri/src`, excluding `timing.rs` / `self_exec.rs`, search for tests that touch `RESTART_REQUIRED` directly or through `frame_work_pending` / `next_toast_deadline` / `pump_toasts` / `pump_restart_toast` (or any caller chain reaching them) | The audit record exists; every hit holds a `RestartFlagTestGuard` for its full touch span | Inspection |
| TS-6 | AC-1, NFR5: full parallel `cargo test --lib` run | Every failure is classified as related (not excluded) or a confirmed known unrelated flake (rerun, recorded, not reported as overall pass) | Integration |
| TS-7 | AC-6, NFR3: the three `cargo check` configurations in Build Verification | Each exits 0 with zero warnings | Build |
| TS-8 | FR5: inspect the doc comments in `sftp/service.rs` | The `#[cfg(test)] impl SftpService` doc says events go through `send_progress` / `send_result` and no longer says they are placed on the channels directly; the `send_progress` doc is unchanged from the base | Inspection |
| TS-9 | NFR1: review the feature diff against the base | Every changed line lies inside a `#[cfg(test)]` item or a comment; no production body, signature or `cfg` gating changed | Inspection |
| TS-10 | NFR2: review the feature diff against the base | `src-tauri/Cargo.toml` and the lock file are unchanged; no new dependency | Inspection |
| TS-11 | NFR4: run `rustfmt --check --edition 2024 <file>` on each Rust file the feature modified, and list the diff's file set | Each check passes; no file outside the feature's modified set is reformatted | Code quality |

## Code Quality Verification

- Format: `rustfmt --check --edition 2024 <file>` for each modified Rust
  file (TS-11). The workflow.yaml `main.format_command`
  (`cargo fmt --manifest-path src-tauri/Cargo.toml --check`) is crate-wide
  and fails on pre-existing drift in seven untouched files. Do not use it
  as the pass/fail gate for this feature (NFR4), and never run crate-wide
  `cargo fmt` without `--check`.
- Static analysis: the three `cargo check` configurations (TS-7), with zero
  warnings.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Parallel `cargo test --lib` (no `--test-threads=1`) passes deterministically for `RESTART_REQUIRED`-related and SFTP-seam-related tests; failure classification per SPEC A-1 | TS-6 (with TS-1, TS-2, TS-3) |
| AC-2 | `self_exec.rs` 4 restart tests and `timing.rs` 9 guarded tests pass in parallel | TS-3 |
| AC-3 | A panic inside a guard span leaves no raised flag | TS-3 (`restart_flag_test_guard_stays_usable_after_a_panicking_span`) |
| AC-4 | No "single-threaded" claim; doc explains Mutex + RAII guard serialization | TS-4 |
| AC-5 | Seams send via the helpers; reverting either seam to a bare send makes the widened check fail | TS-1, TS-2 |
| AC-6 | `cargo check` passes with zero warnings in the three configurations | TS-7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-3, TS-5 |
| FR2 | task0001 | TS-3 |
| FR3 | task0001 | TS-4 |
| FR4 | task0001 | TS-1, TS-2 |
| FR5 | task0001 | TS-8 |
| FR6 | task0001 | TS-2 |
| FR7 | task0001 | TS-5 |
| NFR1 | task0001 | TS-9 |
| NFR2 | task0001 | TS-10 |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0001 | TS-11 |
| NFR5 | task0001 | TS-6 |

## Manual Testing (E2E Not Possible)

The project has no E2E suite (SPEC.md: none detected). These are
reviewer-judgment checks rather than runtime checks:

- [ ] TS-4: `self_exec.rs` doc wording (FR3).
- [ ] TS-5: FR7 audit record is complete (touch set includes the caller
      closure; search scope covers all test code under `src-tauri/src`).
- [ ] TS-8: seam doc wording (FR5).
- [ ] TS-9 / TS-10: diff confined to `#[cfg(test)]` items and comments; no
      manifest or lock change.

No mockup comparison: the design step was skipped (no UI change).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | TS-7 | 1 | 0 | 0 |
| Unit | TS-1, TS-2, TS-3 | 3 | 0 | 0 |
| Integration | TS-6 | 1 | 0 | 0 |
| Code quality | TS-11 | 1 | 0 | 0 |
| Inspection | TS-4, TS-5, TS-8, TS-9, TS-10 | 0 | 0 | 5 |
| **Total** | 11 | 6 | 0 | 5 |
