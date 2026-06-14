# Verification Document: mux Window Tabs (native-poc port)

## Overview
**Feature**: mux Window Tabs (native-poc port)
**SPEC.md**: `doc/tasks/mux-window-tabs-native-port/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-window-tabs-native-port/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=native-poc/target cargo check --manifest-path native-poc/Cargo.toml`
- Expected: exit code 0, no errors
- Note: run from project root; do NOT `cd native-poc/` (`.claude/rules/native-poc-build-location.md`).
- **Result**: PASS — exit 0, no errors (run from project root).

## Test Verification
- Command: `CARGO_TARGET_DIR=native-poc/target cargo test --manifest-path native-poc/Cargo.toml --bin emterm-native-poc`
- Coverage target: core logic minimum 80%, critical paths target 90%
- **Result**: PASS — `1390 passed; 0 failed; 1 ignored`. Mux-specific
  coverage: window_group 22 tests, settings loader 9 tests, apc encode 4
  tests, tabs inbound handlers 13 tests, prefix latch +11 tests, app dispatch
  /confirm/observe 20 tests, dialogs 6 + reentry 2, tab_bar group model 4.
  TS-1..TS-16 all covered (see mapping below).
- Note: `app::tests::pump_all_shifts_selection_by_eviction_delta` is a
  pre-existing real-PTY timing test that can flake under full-parallel load;
  it passes in isolation and on baseline (unrelated to this work).

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Compact group label | `getCompactLabel` returns `mux (N)` for N windows | Unit |
| TS-2 | Compact↔expanded toggle | toggle flips state; `tab_always_expand` seeds initial expanded | Unit |
| TS-3 | Active-index clamp on shrink | removing windows re-clamps active index into `[0, len-1]` | Unit |
| TS-4 | mux settings loader | valid `mux.tab_always_expand`/`status_position`/`keybinds`/`statusbar.*` parse; invalid keybind warns + default | Unit |
| TS-5 | APC encode round-trip | encode then decode yields the original `MuxMessage` for CreateWindow/RenameWindow/MoveWindow/SwitchWindow | Unit |
| TS-6 | Welcome ingest | `SessionInfo.windows` + `active_window_index` seed the window list | Unit |
| TS-7 | PaneCreated append | a pending create appends a window (name "Terminal"), becomes active | Unit |
| TS-8 | PtyExited removal | exited pane's window removed; one-window collapse dissolves the group | Unit |
| TS-9 | RenameWindow inbound | window label updated by window id | Unit |
| TS-10 | SwitchWindow inbound | active index synced to daemon-initiated switch by pane id | Unit |
| TS-11 | Prefix mapping | `d`/`c`/`n`/`p`/`,`/`m`/`0..9`/double-prefix map to correct actions; custom `mux.keybinds` override; unknown→cancel; timeout→cancel | Unit |
| TS-12 | Switch index math | next/prev wrap-around; digit clamp; single-window no-op | Unit |
| TS-13 | Move validation + rollback | out-of-range / same-position no-op; optimistic reorder reverts on simulated send failure | Unit |
| TS-14 | Rename stable-id re-resolve | target re-resolved by stable id; aborts if window closed during dialog | Unit |
| TS-15 | Tab-group render model | compact label, sub-tab count, active marker; click hit-test maps to toggle vs switch | Unit |
| TS-16 | Inbound APC sequence | attach→create→switch→rename→exit drives the window list to expected state | Integration |

## Code Quality Verification
- Format: `cargo fmt --manifest-path native-poc/Cargo.toml`
- Static analysis: `CARGO_TARGET_DIR=native-poc/target cargo clippy --manifest-path native-poc/Cargo.toml` (forward-staged-warning policy per Phase 3/4 precedent)
- **Result**: PASS — `cargo fmt` applied (clean). `cargo clippy` produced no
  errors and no warnings on the new mux files (window_group.rs,
  rename_window_dialog.rs, move_window_dialog.rs); pre-existing
  forward-staged warnings unchanged (94, consistent with Phase 3/4).

## Existing E2E Regression (Phase 3.8)
- **Result**: SKIPPED (not in scope). Per SPEC, this task is unit-test
  centric; GUI / mux-daemon gates are host-deferred (Docker cannot drive
  native windows or the mux daemon).

## File Structure Verification

### Files to Create
- [x] `native-poc/src/mux/window_group.rs` - window state model + compact/expanded controller
- [x] `native-poc/src/ui/rename_window_dialog.rs` - rename dialog
- [x] `native-poc/src/ui/move_window_dialog.rs` - move dialog

### Files to Modify
- [x] `native-poc/src/mux/apc.rs` - outbound APC encoder (`encode_emterm_mux`)
- [x] `native-poc/src/mux/prefix.rs` - new-window/rename/move actions + `ActionBindings` from settings
- [x] `native-poc/src/tabs.rs` - `mux_group` state, `apply_mux_message` extension (Welcome/PaneCreated/PtyExited/Rename/Switch), `send_control`
- [x] `native-poc/src/app.rs` - mux-action dispatch (`dispatch_mux_action`/`observe_mux_key`/`confirm_*`), dialogs, `apply_settings` (mux latch + expand + status_position)
- [x] `native-poc/src/window_host.rs` - latch intercept ahead of keybind dispatch; dialog draw in render pass
- [x] `native-poc/src/render/mod.rs` - (dialog overlay drawn via `App::draw_mux_dialogs` in the window_host egui pass)
- [x] `native-poc/src/ui/tab_bar.rs` - tab-group render model + click hit-test (`mux_group_render_model` / `hit_test_mux_group`)
- [x] `native-poc/src/ui/status_bar.rs` - honors `StatusBarPosition` (mux row); `mux.status_position` applied via `App::status_bar_view_model`
- [x] `native-poc/src/settings.rs` - `mux.*` loader (`MuxSettings` + `RawMux` extension)

### Test Scenario Results (TS-1..TS-16)
| ID | Result | Where |
|----|--------|-------|
| TS-1 | PASS | `mux::window_group::tests::compact_label_is_mux_n` |
| TS-2 | PASS | `window_group` toggle/idempotent/tab_always_expand_seeds_expanded |
| TS-3 | PASS | `window_group` remove_reclamps/set_active_clamps |
| TS-4 | PASS | `settings::tests::loader_mux_*` + `default_mux_settings_match_webview` |
| TS-5 | PASS | `mux::apc::tests::encode_*_round_trips` |
| TS-6 | PASS | `tabs::tests::welcome_seeds_window_list_and_active_index` |
| TS-7 | PASS | `tabs::tests::pane_created_appends_window_named_terminal_*` |
| TS-8 | PASS | `tabs::tests::pty_exited_removes_window` / `_dissolves_group_at_zero` |
| TS-9 | PASS | `tabs::tests::rename_window_updates_label_by_id` |
| TS-10 | PASS | `tabs::tests::switch_window_syncs_active_index_by_pane` |
| TS-11 | PASS | `mux::prefix::tests::*custom_bindings*` + `default_bindings_map_c_comma_m` |
| TS-12 | PASS | `app::tests::dispatch_next_prev_wrap` / `dispatch_digit_clamps_*` |
| TS-13 | PASS | `app::tests::confirm_move_*` (range/same/rollback) |
| TS-14 | PASS | `app::tests::confirm_rename_*` (stable-id re-resolve/closed-window) |
| TS-15 | PASS | `ui::tab_bar::tests::render_model_*` / `hit_test_*` |
| TS-16 | PASS | `tabs::tests::inbound_sequence_attach_create_switch_rename_exit` |

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FRs implemented + unit-tested | TS-1..16 pass; FR coverage table below |
| SC-2 | `cargo test` (native-poc) green | run test command, exit 0 |
| SC-3 | `cargo fmt` / `clippy` clean | run quality commands (forward-staged warnings allowed) |
| SC-4 | `src-tauri` build/test unaffected | inspect diff: no `src-tauri/src/**` or `crates/mux_ipc` wire change |
| SC-5 | APC inband preserved (no socket) | inspect: no `UnixStream`/socket open added in native-poc |
| SC-6 | Behavior parity with WebView tab group | manual host gate (see Manual Testing) |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (tab group UI) | Phase 5 | TS-15 (render model) + draw-path wiring (render/mod.rs→tab_bar mux_cells; apply_tab_event MuxToggle/MuxSwitch) + manual |
| FR2 (window state model) | Phase 1 | TS-1, TS-2, TS-3 |
| FR3 (APC send path) | Phase 2 | TS-5 + SC-5 |
| FR4 (inbound handling) | Phase 2 | TS-6, TS-7, TS-8, TS-9, TS-10, TS-16 |
| FR5 (window switch) | Phase 3 | TS-12 |
| FR6 (new window) | Phase 3 | TS-7, TS-11 |
| FR7 (rename) | Phase 4 | TS-9, TS-14 |
| FR8 (move) | Phase 4 | TS-13 |
| FR9 (close reflect) | Phase 2 | TS-8 |
| FR10 (prefix wiring/extend) | Phase 3 | TS-11 |
| FR11 (settings load + apply) | Phase 1, 5 | TS-4 + manual |
| NFR1 (performance) | all | inspect: on_apc reuse, no hot-path change |
| NFR2 (SSH transparency) | Phase 2 | SC-5 (no socket) |
| NFR3 (compatibility) | all | SC-4 |
| NFR4 (usability parity) | Phase 5 | SC-6 manual |

## Manual Testing (E2E Not Possible)
- [ ] Attach to a real mux session; group shows `mux (N)`; toggle expands sub-tabs (host: needs mux daemon + native window; Docker cannot drive).
- [ ] `prefix c/n/p/0-9/,/m` create/switch/rename/move windows against a live daemon.
- [ ] Settings save re-applies `mux.keybinds` / `tab_always_expand` / `status_position` / `statusbar` without restart.

## Performance Verification
- NFR1: PTY hot path unchanged — verify APC handling stays on the existing `on_apc` route (code inspection).

## Security Verification
- [ ] Rename name / move target clamped & sanitized (TS-13, TS-14).
- [ ] Inbound APC payloads decoded with existing bounded checks (no new unbounded allocation).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test Scenarios | 16 | 16 (15 unit + 1 integration) | 0 | 0 |
| Success Criteria | 6 | 5 | 0 | 1 (SC-6) |
| Manual gates | 3 | 0 | 0 | 3 |
