# Verification Document: mux Snapshot Main-Buffer Screen Omission

## Overview

**Feature**: mux Snapshot Main-Buffer Screen Omission
**SPEC.md**: `doc/tasks/mux-snapshot-main-buffer-screen-omit/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-snapshot-main-buffer-screen-omit/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings introduced by this change.
- Actual: exit code 0. `Finished dev profile [unoptimized + debuginfo] target(s) in 3.80s`. No warnings reported. (sdd.4-implement Phase 2 / Phase 3 close-out, 2026-06-28)

Additional check (CLI-only build):

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0. Confirms the change does not accidentally tighten feature-gated boundaries.
- Actual: exit code 0. `Finished dev profile [unoptimized + debuginfo] target(s) in 1.78s`. No warnings reported. (sdd.4-implement, 2026-06-28)

## Test Verification

- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Coverage target: maintain the project's existing coverage; this change touches a single composition function and adds direct layout assertions for both branches.

### Actual Test Results (sdd.4-implement, 2026-06-28)

- **src-tauri `--lib`** — scoped `mux::ipc` run: `84 passed; 0 failed; 0 ignored; 1936 filtered out`. All snapshot-builder tests green, including:
  - `mux::ipc::reattach::tests::build_snapshot_bytes_layout_is_clear_scrollback_screen` (refactored, alt + main branches)
  - `mux::ipc::reattach::tests::build_snapshot_bytes_strips_rich_content_from_scrollback` (alt branch)
  - `mux::ipc::reattach::tests::build_snapshot_bytes_main_buffer_omits_screen_part` (NEW, FR1 lock-in)
  - `mux::ipc::reattach::tests::build_shadow_parser_snapshot_emits_scrollback_before_screen` (alt branch)
  - `mux::ipc::reattach::tests::build_shadow_parser_snapshot_empty_scrollback_is_clear_plus_shadow` (alt branch)
  - `mux::ipc::handlers::tests::snapshot_bytes_unchanged_after_lock_scope_guardrail` (alt branch)
  - `mux::ipc::handlers::tests::handle_request_pane_snapshot_emits_snapshot_kind` (alt branch)
- **src-tauri `--lib`** — full suite: `2007 passed; 10 failed; 3 ignored` under parallel run. Under `-- --test-threads=1`: `2014 passed; 3 failed; 3 ignored`. Remaining failures investigated against baseline (`git stash` of this change + same single-thread run): two `tabs::tests::ts*_offthread_*` cases pass in isolation (documented flake — see MEMORY `project_test_execution_notes`) and `tabs::tests::welcome_without_windows_leaves_group_none` fails on baseline as well (pre-existing, NOT caused by this change). `status_bar::runtime::tests::runtime_time_provider_timer_fires_wake` only fires under the parallel run and is the documented timer-load flake.
- **crates/term_core `--lib`**: `685 passed; 0 failed; 7 ignored; 0 measured; 0 filtered out`.

### Test Scenarios from SPEC.md

| ID   | Scenario | Expected Result | Test Type |
|------|----------|-----------------|-----------|
| TS-1 | `build_snapshot_bytes(scrollback, screen, alt_screen=false)` excludes the supplied `screen` slice | Output bytes contain `SNAPSHOT_CLEAR_HOME` + stripped scrollback + `ESC[?1049l`; the `screen` slice is absent | Unit (new test `build_snapshot_bytes_main_buffer_omits_screen_part`) |
| TS-2 | `build_snapshot_bytes(scrollback, screen, alt_screen=true)` includes the supplied `screen` slice | Output bytes contain `SNAPSHOT_CLEAR_HOME` + stripped scrollback + `screen` + `ESC[?1049h`, in that order | Unit (refactor of `build_snapshot_bytes_layout_is_clear_scrollback_screen`) |
| TS-3 | `build_shadow_parser_snapshot` honors the alt/main split via `build_snapshot_bytes` | When the shadow parser is in alt-screen mode the returned bytes follow TS-2; when in main-buffer mode they follow TS-1. Verified by refactored `build_shadow_parser_snapshot_emits_scrollback_before_screen` (alt=true path) and `build_shadow_parser_snapshot_empty_scrollback_is_clear_plus_shadow` (re-targeted for the new contract). | Unit (refactored) |
| TS-4 | Rich-content stripping continues to apply regardless of mode | `build_snapshot_bytes_strips_rich_content_from_scrollback` (updated to use the alt-screen branch so the SCREEN-presence assertion stays valid post-fix) still passes; all current `build_snapshot_replay_*` unit tests continue to pass with mechanical updates limited to the new layout. | Unit (existing/refactored) |
| TS-5 | apt progress-bar round-trip on main-buffer renders cleanly | Same-tab click and cross-tab round-trip during / after `sudo apt reinstall <pkg>` does not collapse progress-bar bytes onto log lines | Manual |
| TS-6 | Alt-screen TUI round-trip renders cleanly | After tab round-trip with vim / htop / less / man running, alt-screen content is identical to the pre-switch viewport | Manual |
| TS-7 | Investigation artefacts removed | `grep -R "\[DECSTBM-trace\]" src-tauri/ crates/` returns 0 hits; `grep -nE "fn probe_" src-tauri/src/mux/ipc/reattach.rs` returns 0 hits; no suspect-dump filesystem writes remain in `build_snapshot_bytes` | Automated check (Bash) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml` (per project convention; rustfmt is configured but not strictly enforced — verify the touched files are clean).
- Static analysis: No project-mandated lint pass beyond `cargo check`. Run `cargo clippy` on the touched crates if convenient (optional).

### Actual Format Result (sdd.4-implement, 2026-06-28)

- Command (scoped to touched files per project convention — see MEMORY `feedback_no_crate_wide_cargo_fmt`):
  `cargo fmt -- src-tauri/src/mux/ipc/reattach.rs src-tauri/src/mux/ipc/handlers.rs src-tauri/src/tabs.rs crates/term_core/src/terminal_core.rs crates/term_core/src/reflow.rs`
- Result: completed silently (no diff output). `handlers.rs` had been auto-reformatted by a PostToolUse hook during editing; second pass confirmed no further changes.
- Static analysis: `cargo check` is the project-mandated bar and passed clean (see Build Verification above). No `cargo clippy` run by this implementation step.

## File Structure Verification

### Files to Create

- None. (Confirmed: no new files written.)

### Files to Modify

- [x] `src-tauri/src/mux/ipc/reattach.rs` — alt/main branch in `build_snapshot_bytes`; doc-comment updates on `SNAPSHOT_CLEAR_HOME` const, `build_snapshot_bytes`, `build_shadow_parser_snapshot`; refactored layout tests (alt branch); new `build_snapshot_bytes_main_buffer_omits_screen_part`; removed all 6 `probe_*` tests; removed `[DECSTBM-trace]` log and suspect-dump scope.
- [x] `src-tauri/src/mux/ipc/handlers.rs` — doc-comment update on `handle_request_pane_snapshot` (main/alt split note); refactored two layout-dependent tests (`snapshot_bytes_unchanged_after_lock_scope_guardrail`, `handle_request_pane_snapshot_emits_snapshot_kind`) to drive the parser into alt-screen mode so the SCREEN-presence assertions remain valid under the new contract. (Not in the original plan's Files-to-Modify list, but required because two pre-existing handler tests asserted SCREEN-CONTENT presence with `alt_screen = false`, which is the exact invariant the new contract inverts. See "Deviations" below.)
- [x] `src-tauri/src/tabs.rs` — removed `[DECSTBM-trace]` warn logs in `reset_frame_for_replay` and `apply_offthread_swap` (returned file to HEAD baseline).
- [x] `crates/term_core/src/terminal_core.rs` — removed `[DECSTBM-trace]` warn log + the `old_top`/`old_bottom` locals that only fed it in `set_scroll_region` (returned file to HEAD baseline).
- [x] `crates/term_core/src/reflow.rs` — removed `[DECSTBM-trace]` warn log + the `old_top`/`old_bottom` locals that only fed it in `resize_post_cleanup` (returned file to HEAD baseline).

### Investigation-Code Removal Audit (FR4 / TS-7)

- `grep -R "\[DECSTBM-trace\]" src-tauri/ crates/` → 0 hits.
- `grep -nE "fn probe_" src-tauri/src/mux/ipc/reattach.rs` → 0 hits.
- Suspect-dump filesystem scope inside `build_snapshot_bytes` → removed.

### Deviations from Plan

- Added `src-tauri/src/mux/ipc/handlers.rs` to the touched set (one doc-comment update + two test refactors). The plan only listed `reattach.rs` for the test refactors, but two handler-side tests had identical pre-existing layout assertions (SCREEN-CONTENT presence with `alt_screen = false`) that would otherwise break under the new contract. Refactoring them mirrors the reattach.rs approach (drive parser into alt-screen mode via ESC[?1049h before feeding screen bytes), so the contract change is consistently exercised on both paths.

## SPEC.md Compliance

### Success Criteria

| ID   | Criterion | How to Verify |
|------|-----------|---------------|
| SC-1 | FR1 / FR2 implemented and covered by unit tests | TS-1 + TS-2 + TS-3 pass |
| SC-2 | FR3 doc comments reflect the main/alt split | Inspect the doc comments on `build_snapshot_bytes`, `build_shadow_parser_snapshot`, `handle_request_pane_snapshot` |
| SC-3 | FR4 investigation-code removal is complete | TS-7 automated check returns 0 hits everywhere |
| SC-4 | `--lib` test suites pass with the documented `CARGO_TARGET_DIR` | TS-4 + the new tests pass |
| SC-5 | Manual verification scenarios 1–5 pass | TS-5 + TS-6 (apt round-trip + alt-screen round-trip + log file inspection) |

### Functional Requirements Coverage

| Requirement | Phase     | Verification |
|-------------|-----------|--------------|
| FR1         | Phase 2   | TS-1 (unit) + TS-5 (manual) |
| FR2         | Phase 2   | TS-2 / TS-3 (unit) + TS-6 (manual) |
| FR3         | Phase 3   | SC-2 inspection |
| FR4         | Phase 1   | TS-7 automated check |
| NFR1        | Phase 2   | TS-1 confirms reduced byte payload; no perf benchmark required |
| NFR2        | Phase 2   | TS-3 confirms wire-shape callers untouched; existing `mux_throughput.rs` integration test exercises the wire path |
| NFR3        | Phase 3   | SC-2 inspection |

## E2E Testing

The project has no E2E framework (`test/README.md` documents this — there is no `docker-compose.e2e.yml` and no `e2e-tests/` directory). E2E coverage is provided by the manual scenarios below.

### Existing E2E Regression (Phase 3.8)

- Skipped: no `e2e-tests/` directory, no `docker-compose.e2e.yml`, and `sdd.yaml` has no `e2e_test_command`. (sdd.4-implement, 2026-06-28)

## Manual Testing (E2E Not Possible)

- [ ] Run `sudo apt reinstall <package>` in an emterm mux tab. Click the same tab while apt is running and again right after it finishes. Verify no row collapse.
- [ ] Switch to another tab and back during / after apt. Verify no row collapse.
- [ ] Run an alt-screen TUI (vim / htop / less / man) and perform a tab round-trip. Verify the alt-screen content is restored cleanly.
- [ ] Inspect `~/.local/share/net.laser5.app.emterm/logs/emterm.log` after running the scenarios. Verify no `[DECSTBM-trace]` lines were emitted.

## Performance Verification

- NFR1 expects the main-buffer snapshot payload to strictly decrease vs. the pre-fix path. TS-1 demonstrates the byte composition; no separate benchmark is required because the change removes data rather than adds work.

## Security Verification

- N/A — the change rearranges internal byte composition only and adds no new attack surface.

## Verification Summary

| Category   | Items | Automated | E2E | Manual |
|------------|-------|-----------|-----|--------|
| Unit       | 4     | 4         | 0   | 0      |
| Manual     | 4     | 0         | 0   | 4      |
| Cleanup    | 1     | 1 (TS-7)  | 0   | 0      |
| **Totals** | **9** | **5**     | **0** | **4** |
