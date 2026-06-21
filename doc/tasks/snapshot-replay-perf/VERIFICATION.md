# Verification Document: snapshot-replay-perf

## Overview

**Feature**: snapshot-replay-perf
**SPEC.md**: `doc/tasks/snapshot-replay-perf/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/snapshot-replay-perf/IMPLEMENTATION.md`

## Build Verification

- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (`term_core` standalone): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors. Warnings limited to ones that already exist on the base branch.

### Actual Build Results (sdd.4-implement)

| Command | Exit | Notes |
|---------|------|-------|
| `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml` | 0 | Clean |
| `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --tests` | 0 | Clean |
| `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | 0 | CLI-only build clean |

## Test Verification

- Command (workspace via `src-tauri`): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Command (`term_core` standalone): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml`
- Coverage target: not measured numerically; the new logic is covered by named scenarios below. Existing tests in `term_core` must remain green.
- Note: `tabs.rs` replay tests can be flaky under parallel execution (see `memory/project_test_execution_notes`). If the full run shows a non-deterministic failure unrelated to this feature, re-run with `--test-threads=1` to confirm.

### Actual Test Results (sdd.4-implement)

| Suite | Command | Result |
|-------|---------|--------|
| term_core --lib | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` | 672 passed, 0 failed, 6 ignored |
| src-tauri --lib | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | 1889 passed, 0 failed, 3 ignored |

Phase 1 / Phase 2 unit tests added by this feature (all passing):
- `ring_buffer::tests::test_bypass_branch_below_capacity_no_eviction` — TS-14 (a)
- `ring_buffer::tests::test_bypass_branch_above_capacity_saturates_and_evicts` — TS-14 (b)
- `ring_buffer::tests::test_disable_bypass_resets_virtual_length_and_restores_live_branch` — TS-14 (cleanup)
- `ring_buffer::tests::test_disable_bypass_preserves_evicted_total` — TS-14 (counter monotonicity)
- `terminal_core::tests::test_build_from_snapshot_restores_scrollback_capacity` — TS-5
- `terminal_core::tests::test_build_from_snapshot_bypass_preserves_evicted_total` — TS-13
- `terminal_core::tests::test_build_from_snapshot_bypass_preserves_mark_stamping` — TS-15

Existing TS-1..TS-4 (`test_build_from_snapshot_*`) all still pass; see "Deviations from Plan" below for the one helper-function adjustment required to keep TS-1 green under the FR2 spec change.

Phase 3 bench asserts (TS-6..TS-9) are intentionally NOT run during `sdd.4-implement`; the benches stay `#[ignore]` and are exercised by `sdd.6-verify` with `--include-ignored`.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `test_build_from_snapshot_matches_reset_and_replay` is run unchanged | Passes — replay output is byte-identical to the synchronous `reset_and_replay` path on the same payload | Unit |
| TS-2 | `test_build_from_snapshot_empty_payload` is run unchanged | Passes — empty payload still produces a `Some(SnapshotReplay)` with empty `actions` | Unit |
| TS-3 | `test_build_from_snapshot_is_send_across_threads` is run unchanged | Passes — the returned `SnapshotReplay` remains `Send` | Unit |
| TS-4 | `test_build_from_snapshot_cancelled_returns_none` is run unchanged | Passes — cancelled drain returns `None` | Unit |
| TS-5 | New `test_build_from_snapshot_restores_scrollback_capacity`: build with `scrollback_lines = 10_000` from a payload that scrolls > 100 viewport rows | Immediately after: `core.scrollback_count() == 0`. After feeding N additional lines via the live PTY path: `core.scrollback_count() == min(N, 10_000)` | Unit (FR2) |
| TS-6 | `snapshot_replay_bench_2mib_seq` is run with `--include-ignored` | `eprintln!` reports per-call < 1000 ms and the new `assert!` passes | Bench (FR4) |
| TS-7 | `snapshot_replay_attribution_2mib_seq` is run with `--include-ignored` | Scrollback-disabled configuration's per-call < 200 ms and the new `assert!` passes; baseline / no-scroll configurations remain reported only (no assert) | Bench (FR5) |
| TS-8 | `strip_replayable_rich_content_bench_2mib_plain` is run with `--include-ignored` | per-call < 30 ms and the new `assert!` passes | Bench (FR5) |
| TS-9 | `scrollback_read_all_bench_2mib_wrapped` is run with `--include-ignored` | per-call < 1 ms and the new `assert!` passes | Bench (FR5) |
| TS-10 | `cargo check` with `--no-default-features` against `src-tauri` | Exit 0; CLI-only build still type-checks | Integration |
| TS-11 | `cargo test` against `src-tauri` | All workspace tests green | Integration |
| TS-12 | Manual: `make build`, open mux, run `seq 1 10000000` in tab A, switch to tab B then back to A | Tab-switch back into A completes without a multi-second stall; qualitatively comparable to tmux | Manual |
| TS-13 | New `test_build_from_snapshot_bypass_preserves_evicted_total`: build with `scrollback_lines = small_C` from a payload that scrolls `S > small_C` lines | `replay.evicted_total == S - small_C` — byte-identical to today's path under D1 v2 (virtual scrollback count preserves the same semantics consumers depend on) | Unit (FR1 + D1) |
| TS-14 | New unit tests for the Phase 1 bypass branch in isolation: enable bypass on a freshly constructed `RingBuffer` with capacity `C`, scroll off (a) `N` viewport rows with `N < C`, and (b) `N` viewport rows with `N > C` | (a) `virtual_scrollback_len == N`, `scrollback_evicted_total == 0`, `get_scrollback_length() == N`, `scrollback_count() == 0`. (b) `virtual_scrollback_len == C`, `scrollback_evicted_total == N - C`, `get_scrollback_length() == C`, `scrollback_count() == 0`. | Unit (FR1) |
| TS-15 | New `test_build_from_snapshot_bypass_preserves_mark_stamping`: feed a payload that emits OSC 133 prompt marks at known scroll positions both before and after the `C`-row threshold; compare per-mark `abs_row` and `evicted_total` against the synchronous `reset_and_replay` path on a fresh core of the same size | All marks' `abs_row` and `evicted_total` match the synchronous path byte-identically | Unit (FR1 + D1) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml` and `cargo fmt --manifest-path crates/term_core/Cargo.toml` over **the touched files only** (project policy in `memory/feedback_no_crate_wide_cargo_fmt`).
- Static analysis: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` — no new warnings in touched files.

### Actual Code Quality Results (sdd.4-implement)

- Format: `rustfmt` was run on each of the 6 touched files individually (NOT a crate-wide `cargo fmt`). `git status` after formatting shows only those 6 files modified — no collateral re-formatting of unrelated files.
- Static analysis: `cargo check --tests` against both manifests succeeds with zero warnings on the touched files.

## File Structure Verification

### Files to Modify

- [x] `crates/term_core/src/ring_buffer.rs` — bypass branch in `ring_push_blank` + bypass state on `RingBuffer` (Phase 1)
- [x] `crates/term_core/src/terminal_core.rs` — internal bypass toggles, body change to `build_from_snapshot` (Phases 1 + 2)
- [x] `crates/term_core/src/bench.rs` — `assert!` thresholds added to `snapshot_replay_bench_2mib_seq` and the scrollback-disabled configuration in `snapshot_replay_attribution_2mib_seq` (Phase 3, FR4/FR5)
- [x] `src-tauri/src/mux/scrollback_filter.rs` — `assert!` added to `strip_replayable_rich_content_bench_2mib_plain` (Phase 3, FR5)
- [x] `src-tauri/src/mux/scrollback_buffer.rs` — `assert!` added to `scrollback_read_all_bench_2mib_wrapped` (Phase 3, FR5)

### Additional File Modified (not in original plan)

- [x] `crates/term_core/src/snapshot.rs` — added the two new `RingBuffer` field initializers (`scrollback_bypass: false`, `virtual_scrollback_len: 0`) to the two `TerminalCore { .. }` literals in `from_snapshot_v2` and `from_snapshot_v1`. Required because adding fields to `TerminalCore` forces every literal initializer to be updated; this is a mechanical fixup, not a behavior change.

## Deviations from Plan

- `grid_fingerprint` test helper in `terminal_core::tests` was changed to omit `core.get_scrollback_length()`. Reason: under the FR2 spec change, the built core's `scrollback_count() == 0` (contents intentionally not populated) while the sync core retains up to `scrollback_capacity` scrollback rows. The helper's purpose is to assert the viewport grid + cursor are byte-identical; scrollback contents are by-spec divergent, and the *observable bookkeeping* (`SnapshotReplay.evicted_total`, `prompt_marks`, `fold_marks`) is checked separately in the same test. Without this change, `test_build_from_snapshot_matches_reset_and_replay` would fail on the legitimate spec-allowed divergence.

### Files to Create

- `doc/tasks/snapshot-replay-perf/VERIFICATION_RESULT.md` — produced by sdd.6-verify

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements (FR1–FR5) implemented | Cross-check `Functional Requirements Coverage` table below |
| SC-2 | All test scenarios TS-1..TS-12 pass | Re-run TS-1..TS-12 |
| SC-3 | `cargo test --manifest-path src-tauri/Cargo.toml` green | TS-11 |
| SC-4 | `cargo check --no-default-features` green | TS-10 |
| SC-5 | MUST perf goal (< 1000 ms) achieved on local machine | TS-6 |
| SC-6 | Manual TS-12 qualitatively confirms predicted improvement | TS-12 |
| SC-7 | `memory/project_mux_output_pipeline_perf.md` updated | Read the file after VERIFICATION_RESULT.md is produced; confirm the line attributing "切替2-3秒" to scrollback compression is updated to reflect the fix |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — Bypass scrollback compression during snapshot replay | Phase 1, Phase 2 | TS-14 (bypass in isolation), TS-13 (`evicted_total` byte-identical via `build_from_snapshot`), TS-15 (mark stamping byte-identical), TS-6 (perf effect) |
| FR2 — Restore live scrollback capacity post-replay | Phase 2 | TS-5 |
| FR3 — Preserve `build_from_snapshot` public signature and external contract | Phase 2 | TS-1, TS-2, TS-3, TS-4 |
| FR4 — `assert!(per_call < 1000ms)` on `snapshot_replay_bench_2mib_seq` | Phase 3 | TS-6 |
| FR5 — Assert thresholds on the three supporting benches | Phase 3 | TS-7, TS-8, TS-9 |
| NFR1 — Performance targets (MUST < 1000 ms; SHOULD < 200 ms; STRETCH < 100 ms) | Phase 1 + 2 enable, Phase 3 pins | TS-6 (MUST gated); SHOULD / STRETCH reported in VERIFICATION_RESULT.md |
| NFR2 — Protocol / format stability | Phases 1–3 (no `src-tauri/src/mux/` runtime changes) | Diff review confirms no daemon-side runtime code changed; only bench-assert additions in mux test mods |
| NFR3 — Live-path correctness | Phases 1 + 2 | TS-1, TS-11 |
| NFR4 — Portability | All phases | TS-10, TS-11, plus on-demand Windows cross-build (`make win-build` is optional but recommended in `sdd.6-verify` if Windows-specific change risk is present — for this feature it is `None`) |

## E2E Testing

This feature does not introduce GUI / network flows; the existing tauri-driver / WebDriverIO suite is not extended. (If a future task wires `snapshot_replay_bench_2mib_seq` into CI, that is a separate scope.)

## Manual Testing (E2E Not Possible)

- [ ] TS-12: tab-switch into heavy-output mux tab feels comparable to tmux. Procedure: `make build`; launch mux; in tab A run `seq 1 10000000` to completion; switch to tab B; switch back to tab A. Acceptable: no multi-second stall, viewport restores promptly.

## Performance Verification

- `snapshot_replay_bench_2mib_seq` per-call: expected < 1000 ms (assert); record actual ms in VERIFICATION_RESULT.md and compare to SHOULD (< 200 ms) / STRETCH (< 100 ms).
- `snapshot_replay_attribution_2mib_seq` scrollback-disabled configuration: expected < 200 ms (assert); record baseline and no-scroll values without assert.
- `strip_replayable_rich_content_bench_2mib_plain`: expected < 30 ms (assert).
- `scrollback_read_all_bench_2mib_wrapped`: expected < 1 ms (assert).

## Security Verification

Not applicable. No new external input boundary, no parser path change, no network change.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit (existing) | 4 (TS-1..4) | 4 | 0 | 0 |
| Unit (new) | 4 (TS-5, TS-13, TS-14, TS-15) | 4 | 0 | 0 |
| Bench (perf-asserted) | 4 (TS-6, TS-7, TS-8, TS-9) | 4 | 0 | 0 |
| Cross-build / integration | 2 (TS-10, TS-11) | 2 | 0 | 0 |
| Manual | 1 (TS-12) | 0 | 0 | 1 |
| **Total** | **15** | **14** | **0** | **1** |
