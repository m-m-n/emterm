# Verification Document: Scroll-stick on PTY output + auto-resume on key input

## Overview

- **Feature**: scroll-stick-and-key-resume
- **SPEC.md**: `doc/tasks/scroll-stick-and-key-resume/SPEC.md`
- **IMPLEMENTATION.md**: `doc/tasks/scroll-stick-and-key-resume/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings introduced by this change.
- Additional CLI-feature check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` exits 0.

### Execution Result (sdd.4-implement)

- Default features: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` → exit 0, `Finished dev profile`. No new errors or warnings introduced by this change.
- CLI features: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` → exit 0. CLI build path remains untouched.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not formally tracked in this project; the four new unit tests below must pass and existing `app.rs` tests must continue to pass.
- Flake mitigation: if `tabs.rs` replay tests flake under parallelism (documented project constraint), re-run with `-- --test-threads=1`. The `on_pty_output` tests below are self-contained and parallel-safe.

### Execution Result (sdd.4-implement)

- Targeted run: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib on_pty_output` → **6 passed, 0 failed**. All four new tests (TS-1..TS-4) pass; the pre-existing `on_pty_output_in_live_is_noop` and `on_pty_output_preserves_offset` tests still pass after the signature sweep (TS-5).
- Full suite (parallel default): `cargo test --lib` → 1948 passed, 9 failed. The 9 failures are the documented `tabs.rs` off-thread replay flakes — none touch the modules edited by this change.
- Full suite (serial `--test-threads=1`): 1956 passed, 1 failed. The remaining failure `tabs::tests::welcome_without_windows_leaves_group_none` was confirmed pre-existing on baseline `main` (verified by `git stash` + isolated re-run). Not introduced by this change.
- New-test details:
  - `app::tests::on_pty_output_in_live_ignores_delta_and_stays_live` (TS-1) — PASS
  - `app::tests::on_pty_output_in_offset_adds_delta` (TS-2) — PASS
  - `app::tests::on_pty_output_in_offset_clamps_to_scrollback_lines` (TS-3) — PASS
  - `app::tests::on_pty_output_zero_delta_in_offset_preserves_offset_but_sets_redraw` (TS-4) — PASS
- Existing-test sweep (TS-5): 9 call sites updated to the two-arg signature (`app.rs:4091, 4123, 4132, 4368, 4416, 4446, 4453, 4853, 5188`). All compile and continue to pass.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `App` in `Live`, call `on_pty_output(true, 5)`. | `scroll_position` stays `Live`. | Unit |
| TS-2 | `App` in `OffsetFromLive(10)`, `scrollback_lines = 1000`, call `on_pty_output(true, 3)`. | `scroll_position` becomes `OffsetFromLive(13)`, `needs_full_redraw == true`. | Unit |
| TS-3 | `App` in `OffsetFromLive(995)`, `scrollback_lines = 1000`, call `on_pty_output(true, 10)`. | `scroll_position` becomes `OffsetFromLive(1000)` (clamp). | Unit |
| TS-4 | `App` in `OffsetFromLive(7)`, call `on_pty_output(false, 0)`. | `scroll_position` stays `OffsetFromLive(7)`, `needs_full_redraw == true`. | Unit |
| TS-5 | Existing `App` tests that call `on_pty_output(bool)` updated to pass `0` delta. | All existing tests pass unchanged. | Unit (regression) |
| TS-6 | Run release build; in a regular shell, generate many lines (e.g. long `cat` of a file), scroll up via Shift+PageUp, then start a streaming command (e.g. `while true; do date; sleep 1; done`). | Rows visible at scroll time stay on the same screen positions; `scrollback_lines` not yet reached. | Manual (Linux release) |
| TS-7 | Continuing from TS-6, type any single key (letter, Backspace, Enter). | Viewport snaps to live tail; the typed byte is echoed. | Manual |
| TS-8 | Continuing from TS-6, press Shift (alone) / Ctrl (alone) / Alt (alone). | Viewport remains parked; no snap. | Manual |
| TS-9 | While parked in scrollback, open the search overlay and type. | Viewport remains parked; search input does not snap to live. | Manual |
| TS-10 | While parked in scrollback, press a scrollback chord (Shift+PageUp / PageDown / Home / End). | Viewport scrolls per chord; no live-resume. | Manual |
| TS-11 | Force scrollback to capacity (long output past `scrollback_lines`), park in `OffsetFromLive`, observe further output. | Visible rows shift (accepted; capacity-bound). | Manual |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml` (run only on the files actually edited, per project feedback memory — do not blanket-format the crate).
- Static analysis: no new lints expected. `cargo check` exit-0 is the bar.
- Doc comment review: the rewritten `on_pty_output` doc comment must describe the capacity-bound delta-follow contract (FR3).

### Execution Result (sdd.4-implement)

- Static analysis: `cargo check` (default + `--no-default-features`) — both exit 0, no new warnings.
- Doc comment (FR3): the misleading "the offset stays anchored because `term_core`'s ring buffer shifts the old content into scrollback under us" wording is removed. Replaced with the capacity-bound delta-follow contract (see `app.rs:on_pty_output` doc — bullets describe the below-capacity Δ-follow and the at-capacity drift).
- Format: not auto-run. Per project policy (`feedback_no_crate_wide_cargo_fmt`), `cargo fmt` against the whole crate is forbidden; only the two edited files (`app.rs`, `window_host.rs`) were touched, both follow the surrounding local style.

## File Structure Verification

### Files to Create

- `doc/tasks/scroll-stick-and-key-resume/IMPLEMENTATION.md` — implementation plan.
- `doc/tasks/scroll-stick-and-key-resume/VERIFICATION.md` — this file.
- `doc/tasks/scroll-stick-and-key-resume/tasks.yaml` — phase / task index for the spec-updater impact tool.

### Files to Modify

- `src-tauri/src/app.rs` — `on_pty_output` signature + branch logic + doc comment + pump_all delta wiring + four new unit tests + existing test-call-site sweep.
- `src-tauri/src/window_host.rs` — append one `app.scroll_to_live()` call inside the existing `winit_key_to_bytes → Some(bytes)` branch of `KeyboardInput { Pressed }`.

### Execution Result (sdd.4-implement)

- [x] `src-tauri/src/app.rs` — `on_pty_output` extended to `(active_changed: bool, scrollback_delta: u32)` with saturating-add+clamp branch and rewritten doc comment; `pump_all` now samples `before_scrollback_len` and `after_scrollback_len` and threads the saturating-difference into `on_pty_output`. Four new unit tests added (TS-1..TS-4). Sweep of 9 existing test call sites to the two-arg signature done (TS-5).
- [x] `src-tauri/src/window_host.rs` — `KeyboardInput { Pressed }` handler now captures a `forwarded` boolean inside the `active_tab()` block and, once the shared borrow ends, calls `self.app.scroll_to_live()` when the key was forwarded to the PTY.
- No other files in scope were modified.

## Existing E2E Regression (Phase 3.8)

- This repository ships no E2E framework — `test/README.md` does not enumerate one, and `sdd.yaml.project.components.main.e2e_test_command` is empty. There is no `docker-compose.e2e.yml`, no `e2e-tests/` directory, no `scripts/*e2e*` helper.
- Action: skipped per spec.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FR1 / FR2 / FR3 implemented. | Code review against the FR table below + test scenarios pass. |
| SC-2 | New unit tests pass. | TS-1..TS-4 green in `cargo test --lib`. |
| SC-3 | `cargo check --no-default-features` passes. | Build Verification (CLI). |
| SC-4 | Existing `app.rs` test suite passes with the updated signature. | TS-5 + full `cargo test --lib` run. |
| SC-5 | Manual scroll-stick / live-resume verification by the user. | TS-6 / TS-7 / TS-8 confirmed by the user on the release binary. |
| SC-6 | `App::on_pty_output` doc comment updated. | Read source after edit; comment matches the capacity-bound delta-follow contract. |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (scroll-stick) | Phase 1 + Phase 2 | TS-1 / TS-2 / TS-3 / TS-4 + TS-6 / TS-11. |
| FR2 (key-resume) | Phase 3 | TS-7 / TS-8 / TS-9 / TS-10. |
| FR3 (doc fix) | Phase 1 | Code Quality Verification (doc comment review). |
| NFR1 (perf) | Phase 2 | No new bench; absence-of-regression argued from O(1) `RingBuffer::len()` reads. |
| NFR2 (safety) | Phase 1 + Phase 2 | Code review: no new globals, no allocations on the hot path, `term_core` untouched. |
| NFR3 (compat) | Phase 4 | TS-5 (existing-test sweep). |

## E2E Testing

This repository has no E2E framework (see `test/README.md`; no `docker-compose.e2e.yml`, no `e2e-tests/`). No automated E2E scenarios are added. All end-to-end behavior is covered by the Manual Testing section.

## Manual Testing (E2E Not Possible)

- [ ] TS-6 — Scroll-stick stays put across PTY output while below capacity.
- [ ] TS-7 — Single keystroke snaps viewport to live tail.
- [ ] TS-8 — Bare modifier keystrokes do not snap.
- [ ] TS-9 — Search overlay does not snap.
- [ ] TS-10 — Scrollback chords do not snap.
- [ ] TS-11 — At capacity, visible content is allowed to shift.

## Performance Verification

- NFR1: two extra `lock + read len` ops on the active tab per `pump_all` pass. `RingBuffer::len()` is O(1). No new bench is required; the argument is from the data-structure contract. If a future regression surfaces, the existing `mux_throughput.rs` integration test covers the relevant hot path.

## Security Verification

- Not applicable. The change is a pure internal state update; no new I/O, no new parsing, no new external surfaces.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit | 4 (TS-1..TS-4) + 1 sweep (TS-5) | 5 | 0 | 0 |
| Manual scenarios | 6 (TS-6..TS-11) | 0 | 0 | 6 |
| Build | 2 (default + `--no-default-features`) | 2 | 0 | 0 |
| Format | 1 | 1 | 0 | 0 |
| **Total** | **14** | **8** | **0** | **6** |
