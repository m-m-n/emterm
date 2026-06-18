# Verification Document: mux Off-Thread Snapshot Replay (案a)

## Overview

**Feature**: mux-offthread-replay
**SPEC.md**: `doc/tasks/mux-offthread-replay/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-offthread-replay/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors. (Release builds only on explicit user request.)

### Result (sdd.4-implement)

- Default `cargo check`: **exit 0**, no warnings, no errors.
- CLI-only `cargo check --no-default-features` (TS-10): **exit 0**, green. The off-thread path lives in the GUI-only `tabs.rs`/`app.rs`; the always-built `term_core` pure builder compiles in the CLI-only configuration.
- No release build was run (per project rule — release builds only on explicit user request).

## Test Verification

- Default suite (single-thread): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- term_core unit tests: `CARGO_TARGET_DIR=src-tauri/target cargo test -p term_core`
- Coverage target: not coverage-driven; assert the named scenarios below.

### Result (sdd.4-implement)

Run single-threaded (`-- --test-threads=1`) per the project's `pump_all` flakiness note:

- `emterm` lib tests: **1812 passed, 0 failed, 1 ignored**.
- `term_core` lib tests: **646 passed, 0 failed, 4 ignored**.
- `cli_subcommands` integration tests: **12 passed, 0 failed**.

New tests added by this feature (all passing):

| ID | Test fn | Location |
|----|---------|----------|
| TS-1 | `test_build_from_snapshot_matches_reset_and_replay` | `crates/term_core/src/terminal_core.rs` |
| TS-2 | `test_build_from_snapshot_empty_payload` | `crates/term_core/src/terminal_core.rs` |
| TS-3 | `test_build_from_snapshot_is_send_across_threads` (+ `const _` static assert) | `crates/term_core/src/terminal_core.rs` |
| TS-4 | `ts4_threshold_boundary_sync_vs_offthread`, `ts4_offthread_dispatch_leaves_displayed_core_intact` | `src-tauri/src/tabs.rs` |
| TS-5 | `ts5_offthread_swap_plus_live_equals_contiguous_parse`, `ts5_queued_live_output_applied_in_order` | `src-tauri/src/tabs.rs` |
| TS-6 | `ts6_newer_switch_supersedes_in_flight`, `ts6_sync_snapshot_supersedes_pending_switch` | `src-tauri/src/tabs.rs` |
| TS-7 | `ts7_worker_failure_falls_back_to_sync_reparse` | `src-tauri/src/tabs.rs` |
| TS-8 | `ts8_offthread_swap_reconciles_active_tab_on_pump` | `src-tauri/src/app.rs` |
| TS-9 | `ts9_no_residual_rows_after_offthread_swap_to_shorter_pane`, `ts9_marks_and_baseline_parity_with_sync_path` | `src-tauri/src/tabs.rs` |
| TS-12 | `ts12_resize_supersedes_and_redispatches_at_new_grid`, `ts12_noop_resize_keeps_in_flight_parse` | `src-tauri/src/tabs.rs` |

Plus support tests: `ts3_live_output_queued_during_pending_switch`, `swap_replaces_outgoing_content`, `poll_pending_switch_idle_when_none`.

NFR2 held: no new `pump_all` polling-loop async test. TS-8 calls `pump_all` exactly once after blocking the worker ready (re-staged on a buffered channel); all worker logic is exercised as pure functions / single-call helpers.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Pure builder vs synchronous `reset_and_replay` for a representative payload | Identical grid, scrollback, and drained marks/actions | Unit |
| TS-2 | Pure builder for an empty payload | No panic; grid equals a synchronously reset+replayed empty core | Unit |
| TS-3 | `TerminalCore` movable across threads | Compile-time assertion holds | Unit (compile) |
| TS-4 | Threshold boundary: payload `== OFFTHREAD_REPLAY_THRESHOLD_BYTES` vs one byte below | `==` → off-thread path; below → synchronous path | Unit |
| TS-5 | Snapshot off-thread parse + queued live bytes applied after swap | Final grid equals one contiguous parse of snapshot+live (no loss/reorder) | Unit |
| TS-6 | Supersession: targets A→B→C with B in flight | Only C's core swapped in; B's result discarded | Unit |
| TS-7 | Worker failure/panic | Synchronous fallback yields the correct core for the latest target | Unit |
| TS-8 | Post-swap marks/folds/selection + per-pane scroll vs synchronous path | Match the legacy synchronous replay for the same snapshot | Integration |
| TS-9 | Off-thread swap to a shorter pane | No residual rows from the outgoing (longer) unit (FR2) | Integration |
| TS-10 | CLI-only `--no-default-features` check | exit 0, green; off-thread path is GUI-only | Build |
| TS-11 | `Ctrl+B n n n` across history-heavy panes (~2 MiB) | UI stays responsive; outgoing pane visible until swap; no blank flicker; live output not lost | Manual/Perf |
| TS-12 | Grid resize during a pending switch | In-flight parse is superseded; re-dispatch at the new grid; no stale-sized core swapped in | Unit |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml` (PostToolUse format hook enforces)
- Static analysis: default `cargo check` warnings clean for touched files.

### Result (sdd.4-implement)

- `rustfmt --edition 2024 --check` on the five touched files (`terminal_core.rs`, `callbacks.rs`, `tabs.rs`, `app.rs`, `fold.rs`): **clean** (exit 0). The PostToolUse hook formats each edited file; no crate-wide `cargo fmt` was run (would have touched unrelated rustfmt-divergent files).
- `cargo check`: **no warnings**. The earlier `done` dead-code and `SwapOutcome` visibility warnings were resolved (the field is read by `poll_pending_switch`; the method is `pub(crate)`).

## File Structure Verification

### Files to Modify
- [x] `crates/term_core/src/terminal_core.rs` - pure snapshot-replay builder (`build_from_snapshot` + `SnapshotReplay`) + thread-safety assertion (`const _` static assert) + `scrollback_capacity()` getter + unit tests
- [x] `src-tauri/src/tabs.rs` - `OFFTHREAD_REPLAY_THRESHOLD_BYTES`, `PendingSwitch` + `SwapOutcome`, `pending_switch` field, size-branch dispatch, live-output queue, supersession (new switch + resize), `dispatch_offthread_replay` / `poll_pending_switch` / `apply_offthread_swap` / `apply_queued_live_output`, refactored `reset_frame_for_replay` into shared `reset_frame_prompts_folds` + `apply_replay_reconcile`, tests
- [x] `src-tauri/src/app.rs` - `pump_all` per-tab completion poll + active-tab full-redraw integration with the existing pane-switch/frame-reset reconciliation; `set_grid_size` loop now `&mut self.tabs` (resize takes `&mut self`); integration test

### Files Modified (not in original plan, scoped support changes)
- [x] `crates/term_core/src/callbacks.rs` - `TerminalCallbacks: Send` supertrait (required so `TerminalCore` is `Send`); test `Recorder` switched `RefCell`→`Mutex`, `Rc`→`Arc`
- [x] `src-tauri/src/fold.rs` - `#[cfg(test)] region_count()` accessor for the fold-parity test

### Files to Create
- (none — all changes are in existing source files)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | UI thread not blocked proportional to scrollback size on switch | TS-11 (manual/perf); compare against `mux-snapshot-reparse-offthread` measurement |
| SC-2 | Outgoing pane stays visible until swap; no blank flicker | TS-11 (manual); TS-9 (no residual rows) |
| SC-3 | Live output during pending switch not lost/reordered | TS-5, TS-8 |
| SC-4 | FR2/FR3 + marks/folds/selection invariants preserved | TS-8, TS-9 |
| SC-5 | Default `cargo test` (single-thread) + CLI-only `cargo check` green | TS-10 + default suite |
| SC-6 | No new flaky `pump_all` async tests; worker logic unit-tested as pure functions | TS-1, TS-5, TS-6, TS-7 (pure); review test modules |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 off-thread reparse + main-thread swap | 1, 2, 3 | TS-1, TS-3, TS-4 |
| FR2 pending-switch display (keep outgoing) | 2 | TS-9, TS-11 |
| FR3 live-output ordering | 2, 3 | TS-5, TS-8 |
| FR4 size-threshold fast path | 2 | TS-4 |
| FR5 supersession on rapid re-switch (incl. resize) | 2 | TS-6, TS-12 |
| FR6 reconciliation split | 1, 3 | TS-1, TS-8 |
| FR7 synchronous fallback on worker failure | 3 | TS-7 |
| NFR1 invariant preservation | 3, 4 | TS-8, TS-9 |
| NFR2 deterministic non-flaky testability | 1, 4 | TS-1, TS-2, TS-5 |
| NFR3 portability (Linux/Windows/CLI-only) | 1, 4 | TS-3, TS-10 |
| NFR4 no memory regression (1 core/tab) | 2 | Design review (no per-pane resident cores, no LRU) — no automated test |

## Manual Testing (E2E Not Possible)

(No project E2E framework. Native terminal has no WebView; verify via behavior + `emterm.log`.)

- [ ] TS-11: `Ctrl+B n n n` across panes with large scrollback — switching stays responsive, the outgoing pane stays visible until the new pane is ready, no blank flicker.
- [ ] Switch back to the outgoing pane before the worker completes (supersede to original) — no intermediate flash.
- [ ] Produce output in the target pane during the parse gap — it appears in order after the swap.

## Performance Verification

- Switch latency on a ~2 MiB pane: the `pump_all` iteration handling the swap does not block proportional to scrollback size (the heavy parse runs off-thread). Compare against the synchronous baseline (256 KiB = 30 ms, 1 MiB = 117 ms, 2 MiB = 233 ms) from `doc/tasks/mux-snapshot-reparse-offthread/VERIFICATION_RESULT.md`.

## Security Verification

- N/A — no new external input or trust boundary; local terminal data only.

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Unit | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-7, TS-12 | 8 | 0 |
| Integration | TS-8, TS-9 | 2 | 0 |
| Build | TS-10 | 1 | 0 |
| Manual/Perf | TS-11 (+ 2 manual checks) | 0 | 3 |
| Design review | NFR4 | 0 | 1 |

## Existing E2E Regression (Phase 3.8)

- `sdd.yaml` `e2e_test_command` is empty and the project has no native E2E framework (the native terminal is wgpu+swash with no WebView; Docker E2E covers the WebView build only). **Skipped** — no E2E regression to run for this native-only feature.

## Implementation Results (sdd.4-implement)

All four phases implemented and all named automated scenarios pass.

- **Phase 1** (`term_core`): `build_from_snapshot` pure builder + `SnapshotReplay` + `const _` `Send` static assertion + `scrollback_capacity()` getter. `TerminalCallbacks: Send` supertrait added so `TerminalCore` is `Send` (sound: only impl is `NativeCallbacks`, all-`Arc` `Send` fields; the worker installs no callbacks). TS-1/2/3 pass.
- **Phase 2** (`tabs.rs`): `OFFTHREAD_REPLAY_THRESHOLD_BYTES = 64 KiB`, `PendingSwitch` state (target pane, grid, `mpsc` handoff, live queue, retained payload), size-branch dispatch in the `Snapshot`/`SnapshotRestore` arm (`< threshold` = legacy sync; `>= threshold` = copy payload + frame-discard prompts/folds + spawn worker + enter pending, displayed core untouched), live-output queueing in the `PtyOutput` arm, supersession on newer switch and on grid resize (re-dispatch at the new grid, queue preserved). TS-4/6/12 pass.
- **Phase 3** (`app.rs` + `tabs.rs`): `poll_pending_switch` (non-blocking `try_recv`, per owning tab), `apply_offthread_swap` (swap `*core.lock()`, reconcile from the worker-built core's drained values, then apply queued live bytes in order), `pump_all` integration (per-tab poll + active-tab `needs_full_redraw`; selection drop via the existing `pending_frame_reset` latch), FR7 synchronous-reparse fallback on `Disconnected`. The `reset_frame_for_replay` recipe was factored into shared `reset_frame_prompts_folds` (frame-discard half) + `apply_replay_reconcile` (main-thread half) so the sync and off-thread paths cannot drift. TS-5/7/8 pass.
- **Phase 4**: TS-9 (no residual rows after swap to a shorter pane + marks/folds/baseline parity), TS-10 (CLI-only check green). Ordering/supersession/fallback covered by TS-5/6/7. NFR2 upheld (no `pump_all` polling-loop async test).

### Deviations from the plan

- Added `TerminalCallbacks: Send` (and `Mutex`/`Arc` in the term_core callback tests) — necessary because `TerminalCore` carries `Option<Box<dyn TerminalCallbacks>>`; the static `Send` assertion the plan mandates is unattainable otherwise. Sound given the single production impl is already `Send`; no live wasm crate consumes `term_core` with a non-`Send` callback.
- Added `scrollback_capacity()` (term_core) and a `#[cfg(test)] region_count()` (fold) accessor — small read-only seams needed to size the worker core and assert fold parity.
- `Tab::resize` changed from `&self` to `&mut self` (and the `set_grid_size` loop to `&mut self.tabs`) so a resize can supersede + re-dispatch the in-flight parse (FR5). Sole caller updated.

### Known Limitations

- TS-11 (manual perf: `Ctrl+B n n n` across ~2 MiB history-heavy panes stays responsive) is a manual/perf check, not automated — to be exercised at `sdd.6-verify` / by the user on the target machine.
- NFR4 (1 core per tab, no memory regression) is verified by design review (transient in-flight worker core only; no per-pane resident cores, no LRU), not an automated test, as resolved at verify-plan.
