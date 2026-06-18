# Verification Document: mux Scroll Isolation

## Overview

**Feature**: mux Scroll Isolation
**SPEC.md**: `doc/tasks/mux-scroll-isolation/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-scroll-isolation/IMPLEMENTATION.md`

Scope: native build only (`src-tauri/src/`). The WebView build (`src/`) is not changed on this branch.

## Build Verification

- Command (release, run only on explicit user request): `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`
- CLI-only feature gate check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors.

### Implementation Results (sdd.4-implement)

- Default-features compile: `cargo check --manifest-path src-tauri/Cargo.toml` → exit 0, no warnings, no errors.
- CLI-only feature gate: `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` → exit 0 (all changes live in `gui`-gated modules; the feature gate is intact).
- Release build (`target-host`) NOT run — per project policy, release builds are only run on explicit user request.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Coverage target: cover the new scroll save/restore branches (tab + pane) and the on-demand snapshot builder ordering; minimum 80% of new logic, target 90% for the switch save/restore path.

### Implementation Results (sdd.4-implement)

- `cargo test --manifest-path src-tauri/Cargo.toml` → **1791 lib tests passed, 0 failed, 1 ignored** (pre-existing); **12 integration tests passed** (`cli_subcommands.rs`); doc-tests 0. No regressions (NFR1).
- New automated tests added:
  - `app.rs`: `switch_to_tab_saves_outgoing_and_restores_incoming_scroll` (TS-1), `switch_to_tab_live_restores_to_bottom` (TS-2), `switch_to_tab_offset_restores_to_same_offset` (TS-3), `switch_to_tab_all_live_introduces_no_scroll` (TS-7), `tab_scroll_position_default_is_live`, `local_pane_switch_round_trip_restores_scroll_position` (TS-5, local path), `local_pane_switch_all_live_introduces_no_scroll` (TS-7 pane), `local_pane_switch_with_empty_scrollback_does_not_crash` (TS-6 switch side), `local_pane_switch_forces_full_redraw` (FR2), `local_pane_switch_noop_does_not_touch_scroll_or_redraw` (NFR1).
  - `mux/window_group.rs`: `pane_scroll_defaults_to_live`, `pane_scroll_set_get_round_trip_is_per_pane`, `pane_scroll_set_on_empty_group_is_noop`, `pane_scroll_follows_reorder_and_survives_remove`, `push_resets_new_pane_scroll_to_live` (per-pane slot + parallel-array invariant).
  - `mux/ipc/reattach.rs`: `build_shadow_parser_snapshot_emits_scrollback_before_screen` (TS-4), `build_shadow_parser_snapshot_empty_scrollback_is_clear_plus_shadow` (TS-6 builder), `build_snapshot_bytes_layout_is_clear_scrollback_screen` (shared layout).
  - `tabs.rs`: `inbound_switch_latches_outgoing_pane_index`, `inbound_switch_to_same_pane_does_not_latch`, `inbound_switch_unknown_pane_does_not_latch` (inbound `SwitchWindow` FR3 wiring).
- TS-8 / TS-9 / TS-10 remain manual (see Manual Testing section); not automated for the native build per plan.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Switching native tabs saves the outgoing tab's scroll position and restores the incoming tab's | Active scroll value equals incoming tab's saved position after switch | Unit |
| TS-2 | A unit saved at bottom (`Live`) restores at bottom | Restored position is `Live` (offset 0) | Unit |
| TS-3 | A unit saved at `OffsetFromLive(n)` restores at offset `n` | Restored position equals `OffsetFromLive(n)` | Unit |
| TS-4 | On-demand snapshot builder emits scrollback before the shadow screen (matches reattach construction) | Snapshot bytes = clear-and-home + scrollback + shadow, in that order | Unit |
| TS-5 | mux pane switch round-trip (A → B → A) restores A's scroll position and A's scrollback is scrollable | After returning to A, A's saved scroll position is restored and past output is reachable by scroll | Integration |
| TS-6 | Switching to a pane with empty scrollback does not crash and shows no residual rows | Switch succeeds; replayed history is empty; no residual rows | Integration |
| TS-7 | All tabs/panes at bottom (`Live`) — switching introduces no scroll | After switch, position is `Live` for the incoming unit | Unit |
| TS-8 | Long unit → short unit switch leaves no residual rows at the bottom | Rendered frame shows only the incoming (shorter) unit's rows | Manual |
| TS-9 | A background pane whose scrollback grew while inactive shows content consistent with its saved scroll position on return | Returned pane follows its saved position (bottom-pinned follows new output; scrolled-up keeps its place) | Manual |
| TS-10 | On-demand snapshot with scrollback transfers comparably to the reattach path | Snapshot size scales like the reattach payload (no unexpected blow-up) | Manual |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml`
- Static analysis: standard `cargo check` / `cargo test` warnings reviewed; no new warnings introduced.

### Implementation Results (sdd.4-implement)

- `cargo fmt --manifest-path src-tauri/Cargo.toml` → exit 0 (applied; touched files reformatted).
- `cargo check` (default features) → 0 warnings, 0 errors after the change.

## File Structure Verification

### Files to Create

- (none)

### Files to Modify

- `src-tauri/src/app.rs` — keep active scroll value single; save/restore at native tab switch and local pane switch; full-redraw on switch
- `src-tauri/src/tabs.rs` — per-tab + per-pane scroll slots; inbound `SwitchWindow` save/restore; snapshot apply recipe unchanged
- `src-tauri/src/mux/window_group.rs` — per-pane scroll slot on the window entry; active-pane scroll accessors
- `src-tauri/src/mux/ipc/handlers.rs` — on-demand snapshot reads pane scrollback and builds a history-bearing snapshot
- `src-tauri/src/mux/ipc/reattach.rs` — shared snapshot byte layout (clear + scrollback + shadow) for on-demand + reattach
- `src-tauri/src/window_host.rs` — renderer reads active-unit scroll offset; full redraw clears stale rows on switch
- `src-tauri/src/render/mod.rs` — row mapping clears non-emitted rows under full redraw
- `crates/term_core/src/terminal_core.rs` — `reset_and_replay` / `reset` behavior unchanged (exercised by FR1)

### Implementation Results (sdd.4-implement)

Files actually modified:

- [x] `src-tauri/src/app.rs` — `switch_to_tab` save/restore (FR3 tab); `switch_to` takes `&mut ScrollPosition` for per-pane save/restore (FR3 pane); `dispatch_mux_action` + `MuxSwitch` write the swapped scroll back and set `needs_full_redraw` on a committed switch (FR2); `pump_all` drains the inbound-switch latch and applies per-pane save/restore + full redraw.
- [x] `src-tauri/src/tabs.rs` — per-tab `scroll_position` field (default `Live`); inbound `SwitchWindow` latches the outgoing pane index (`pending_pane_switch_from`); `take_pending_pane_switch()` drainer.
- [x] `src-tauri/src/mux/window_group.rs` — per-pane scroll positions as a third parallel array `pane_scrolls` (invariant F1 preserved in `seed`/`push`/`remove_pane`/`reorder`); accessors `active_pane_scroll` / `set_active_pane_scroll` / `set_pane_scroll_at`.
- [x] `src-tauri/src/mux/ipc/reattach.rs` — shared `build_snapshot_bytes` (clear + scrollback + screen); `build_shadow_parser_snapshot` now takes scrollback; reattach path reuses the shared layout.
- [x] `src-tauri/src/mux/ipc/handlers.rs` — on-demand snapshot handler resolves the pane's scrollback (no clear) and builds a history-bearing snapshot; snapshot-size warn log retained (scrollback size appended).
- [ ] `src-tauri/src/window_host.rs` — **no change needed.** The renderer already reads `App::scroll_offset()` (the active-unit value) and `collect_cell_inputs` emits a full grid; `needs_full_redraw` already drives the clear.
- [ ] `src-tauri/src/render/mod.rs` — **no change needed.** `collect_cell_inputs` emits cells for all `rows`; under `needs_full_redraw`, `dirty_rows_this_frame` returns `0..rows`, so non-emitted rows are repainted/cleared. FR2 is satisfied by setting the existing flag on the switch paths (done in `app.rs`).
- [ ] `crates/term_core/src/terminal_core.rs` — unchanged (behavior exercised by FR1 via the client's existing `reset_and_replay`).

Design note (divergence): the per-pane scroll slot is stored as a third parallel array (`pane_scrolls`) on `MuxWindowGroup` rather than as a field on the `MuxWindow` struct. This keeps `MuxWindow`'s `PartialEq`/`Eq` (relied on by the tab-bar render tests) depending only on identity + name, and enforces the F1 parallel-array invariant in the same mutators that move `windows` / `pane_ids`. Behavior matches the plan's intent ("per-pane scroll slot on the window entry, accessors that do not expose the parallel arrays").

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1, FR2, FR3 implemented and tested | TS-1..TS-7 automated where feasible; TS-8..TS-10 manual; FR coverage table below |
| SC-2 | Pane/window switch restores scrollback and per-unit scroll position | TS-5 (round-trip), TS-1/TS-2/TS-3 (per-unit position) |
| SC-3 | No residual rows on switch | TS-8 (long→short, manual), TS-6 (empty scrollback, integration) |
| SC-4 | Non-mux tab and single-window mux scroll/render unaffected (regression) | Existing scroll test suite green; manual single-window spot-check |
| SC-5 | `cargo test` (default features) and CLI-only `cargo check` pass | Build + Test verification commands above |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (snapshot includes scrollback) | Phase 2 | TS-4 (builder ordering), TS-5 (history scrollable after switch), TS-6 (empty scrollback) |
| FR2 (full re-render on switch) | Phase 1 (tab) + Phase 3 (pane) | TS-8 (long→short residual rows), TS-6 (no residual rows on empty pane) |
| FR3 (per-tab + per-pane scroll position) | Phase 1 (tab) + Phase 3 (pane) | TS-1, TS-2, TS-3 (tab/position), TS-5 (pane round-trip), TS-7 (all-Live) |
| NFR1 (no regression, scroll-pin preserved) | Phase 1, Phase 3 | SC-4 regression suite; existing scroll-pin tests remain green |
| NFR2 (O(1) save/restore; snapshot transfer comparable to reattach) | Phase 2, Phase 3 | TS-10 (snapshot size comparable); save/restore is a single numeric swap |
| NFR3 (returned-to pane consistent with saved position) | Phase 3 | TS-9 (background-grown pane on return) |

## E2E Testing

No E2E framework is configured for the native build (`e2e_test_command` is empty in `sdd.yaml`). Switch-time visual behavior is verified via the Manual Testing section below.

### Existing E2E Regression (sdd.4-implement, Phase 3.8)

Skipped: `e2e_test_command` is empty in `sdd.yaml` and no native E2E harness (e2e-tests/README, CLAUDE.md E2E section, scripts/*e2e*, docker-compose.e2e) is wired for this branch's native build. Regression coverage is provided by the full `cargo test` suite (1791 + 12 passing, 0 failures).

## Manual Testing (E2E Not Possible)

- [ ] TS-8: Display a long unit, switch to a shorter unit — no residual rows remain at the bottom.
- [ ] TS-9: In pane A scroll up; let B (or A in background) accumulate output; return to A — A shows content consistent with its saved scroll position (scrolled-up keeps place; bottom-pinned follows new output).
- [ ] TS-10: Flush a large scrollback in a pane, switch away and back, confirm the snapshot reply size is comparable to a reattach (check the snapshot-size warn log) with no unexpected blow-up.
- [ ] UC01 walkthrough: pane A large output → switch to pane B → return to A → wheel / Shift+PageUp reaches A's past output with no detach/re-attach.
- [ ] Regression: single-window mux and a non-mux tab scroll and render exactly as before; scroll-pin (scrolled-up viewport stays pinned during new output) still works.

## Performance Verification

- Save/restore on the switch path: a single numeric scroll-position swap (no measurable cost).
- On-demand snapshot with scrollback: transferred size comparable to the reattach path (TS-10). Acceptable per NFR2.

## Security Verification

- Not applicable. No new external input, authentication, or data-exposure surface (per SPEC §Security Considerations).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Scroll position save/restore (FR3) | TS-1, TS-2, TS-3, TS-5, TS-7 | TS-1, TS-2, TS-3, TS-5, TS-7 | 0 | 0 |
| Snapshot scrollback (FR1) | TS-4, TS-5, TS-6, TS-10 | TS-4, TS-5, TS-6 | 0 | TS-10 |
| Full re-render on switch (FR2) | TS-6, TS-8 | TS-6 | 0 | TS-8 |
| Non-functional (NFR1/2/3) | SC-4, TS-9, TS-10 | SC-4 regression suite | 0 | TS-9, TS-10 |
| **Totals (distinct TS)** | 10 | 7 (TS-1..7) | 0 | 3 (TS-8, TS-9, TS-10) |
