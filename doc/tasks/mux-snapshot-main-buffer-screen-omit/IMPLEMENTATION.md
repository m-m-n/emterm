# Implementation Plan: mux Snapshot Main-Buffer Screen Omission

## Overview

Branch `build_snapshot_bytes` on the captured `alt_screen` flag so the daemon vt100 dump (`contents_formatted()`) is only included for alt-screen panes, and update the surrounding doc comments / tests to match. Same change pass also removes the leftover `[DECSTBM-trace]` investigation logs, the suspect-dump filesystem writes, and the `probe_*` tests under `mux::ipc::reattach`.

## Objectives

- Eliminate the apt-progress-bar trash on main-buffer snapshot restore by omitting the daemon vt100 dump for that mode (FR1).
- Preserve alt-screen snapshot fidelity by keeping the existing layout for that mode (FR2).
- Codify the main/alt split in module doc comments and SPEC.md (FR3).
- Remove all investigation-only code added during the apt-bar bug hunt (FR4).

## Prerequisites

### Development Environment

- Rust toolchain pinned by the project (already required for normal eMterm builds).
- `CARGO_TARGET_DIR=src-tauri/target` for quick checks / tests (per `.claude/rules/build-location.md`).

### Dependencies

- No external crate or version change. `vt100` 0.16.2 stays as is.
- IPC wire protocol stays as is (codec / message types untouched).

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: Custom mux IPC layer (`src-tauri/src/mux/`), `tokio` runtime, `vt100` shadow parser.
- **Key Libraries**: `vt100` (daemon shadow parser, unchanged), `term_core` (client ANSI parser, unchanged).

### Design Approach

The snapshot payload composition is the single place to encode the main/alt split. Both the reattach path (`build_shadow_parser_snapshot`) and the on-demand path (`handle_request_pane_snapshot`) already funnel through `build_snapshot_bytes(scrollback, screen, alt_screen)`, so the contract change is localized.

The client-side replay (`tabs.rs::reset_and_replay`) does not need to change — for `alt_screen=false` panes the snapshot byte stream simply contains scrollback + alt-mode reset and the client's fresh term_core rebuilds the visible viewport on its own.

### Component Interaction

```
client RequestPaneSnapshot ──▶ daemon handle_request_pane_snapshot
                                   │
                                   ▼
                       build_shadow_parser_snapshot
                                   │
                                   ▼
                       build_snapshot_bytes (alt/main branch)
                                   │
                                   ▼
                       PtyOutput chunk (unchanged wire shape)
                                   │
                                   ▼
                       client tabs.rs::reset_and_replay → term_core
```

## Implementation Phases

### Phase 1: Investigation Code Removal

**Goal**: Restore the modules touched by the apt-bar bug hunt to a noise-free state. All `[DECSTBM-trace]` lines, suspect-dump writes, and `probe_*` tests are gone before behavioral changes land.

**Files to Modify**:

- `crates/term_core/src/terminal_core.rs` - Drop the `[DECSTBM-trace]` warn log in `set_scroll_region`.
- `crates/term_core/src/reflow.rs` - Drop the `[DECSTBM-trace]` warn log in `resize_post_cleanup`.
- `src-tauri/src/tabs.rs` - Drop the `[DECSTBM-trace]` warn logs in `reset_frame_for_replay` and `apply_offthread_swap`.
- `src-tauri/src/mux/ipc/reattach.rs` - Drop the `[DECSTBM-trace]` warn log block AND the suspect-dump filesystem writes inside `build_snapshot_bytes`. Drop the `probe_*` test functions (PROBE 2–6, 5 tests) from the `#[cfg(test)]` module.

**Files to Create**: None.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `terminal_core::set_scroll_region` | Apply DECSTBM region | Region indices validated | Region applied. No trace log emitted. |
| `reflow::resize_post_cleanup` | Re-clamp state after resize | Resize event delivered | Cleanup applied. No trace log emitted. |
| `tabs::reset_frame_for_replay` | Reset client frame before replay | Replay requested | Frame reset. No trace log emitted. |
| `tabs::apply_offthread_swap` | Swap in off-thread-built core | Swap arrived | Swap installed. No trace log emitted. |
| `ipc::reattach::build_snapshot_bytes` | Compose snapshot bytes | scrollback / screen / alt_screen supplied | Bytes composed. No trace log / suspect dump emitted. |
| `ipc::reattach` `probe_*` tests | (investigation-only) | n/a | Removed. |

**Processing Flow** (diagram-convertible):

1. Locate each `[DECSTBM-trace]` site and remove the surrounding `log::warn!` / `log::error!` block.
2. In `reattach.rs::build_snapshot_bytes`, also remove the bug-suspect dump scope (the inner block that writes raw scrollback / screen to disk).
3. In `reattach.rs::#[cfg(test)] mod tests`, remove the 5 `probe_*` functions (`probe_real_apt_scrollback_into_fresh_vt100`, `probe_real_apt_bytes_mid_run`, `probe_real_apt_bytes_roundtrip`, `probe_dump_contents_formatted_bytes`, `probe_apt_pattern_full_roundtrip`, plus `probe_scroll_region_survives_snapshot_via_scrollback` if present).
4. Remove any now-dead imports / helpers used only by the removed code.

**Implementation Steps** (5-7 max):

1. **Audit traces** — `grep -R "\[DECSTBM-trace\]" src-tauri/ crates/` and `grep -nE "fn probe_" src-tauri/src/mux/ipc/reattach.rs` to inventory removal sites.
2. **Strip trace logs** — Remove each `[DECSTBM-trace]` block at the audited sites.
3. **Strip suspect dump** — Remove the inner scope in `build_snapshot_bytes` that writes raw scrollback / screen to disk.
4. **Strip probe tests** — Delete the 5 `probe_*` test functions and any helper they own.
5. **Clean unused imports** — Remove imports / use statements that became unused.
6. **Quick check** — `cargo check` (`src-tauri/target` target dir) to confirm the workspace still builds.

**Dependencies**: Independent of Phases 2-3. Blocks the behavioral fix only by keeping the change set readable.

**Testing Approach**:

- Unit: existing `--lib` test suite continues to pass.
- Integration: existing integration tests continue to pass.
- E2E: n/a.
- Manual: visually confirm `~/.local/share/net.laser5.app.emterm/logs/emterm.log` no longer carries `[DECSTBM-trace]` lines after a run.

**Acceptance Criteria**:

- [ ] `grep -R "\[DECSTBM-trace\]" src-tauri/ crates/` returns 0 hits.
- [ ] `grep -nE "fn probe_" src-tauri/src/mux/ipc/reattach.rs` returns 0 hits.
- [ ] `cargo check` and `cargo test --lib` succeed with no warnings introduced.

**Estimated Effort**: small

---

### Phase 2: Main / Alt Split in `build_snapshot_bytes`

**Goal**: Implement the FR1 / FR2 contract — omit the daemon vt100 dump for main-buffer panes; keep it for alt-screen panes — without changing the IPC wire shape.

**Files to Modify**:

- `src-tauri/src/mux/ipc/reattach.rs` - Branch `build_snapshot_bytes` on `alt_screen`; choose between `&[]` and `screen` for the included screen slice; size the output `Vec` accordingly.

**Files to Create**: None.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `build_snapshot_bytes` | Compose the snapshot byte stream from scrollback + (optional) screen + alt-mode toggle | Inputs supplied. `strip_replayable_rich_content` is the only scrollback transform. | Output bytes start with `SNAPSHOT_CLEAR_HOME`, contain the stripped scrollback, contain the daemon vt100 dump iff `alt_screen=true`, and end with `ESC[?1049h` / `ESC[?1049l`. |
| `build_shadow_parser_snapshot` | Call into `build_snapshot_bytes` with the captured screen + alt-mode | Shadow parser locked, scrollback collected | Returns combined bytes. Doc comment updated to mention the main/alt split. |
| `handle_request_pane_snapshot` | On-demand snapshot path | RequestPaneSnapshot received | Calls `build_snapshot_bytes`. Doc comment updated to mention the main/alt split. |

**Processing Flow** (diagram-convertible):

1. Strip rich content from scrollback (unchanged).
2. Choose `alt_mode` byte sequence: `ESC[?1049h` if `alt_screen=true`, else `ESC[?1049l`.
3. Choose `screen_to_include` slice:
   - `alt_screen=true` -> `screen`
   - `alt_screen=false` -> empty slice
4. Allocate output `Vec` with capacity = `SNAPSHOT_CLEAR_HOME.len()` + stripped scrollback length + `screen_to_include.len()` + `alt_mode.len()`.
5. Append in order: `SNAPSHOT_CLEAR_HOME`, stripped scrollback, `screen_to_include`, `alt_mode`.
6. Return the composed bytes.

**Implementation Steps** (5-7 max):

1. **Branch on alt_screen** — Introduce the `screen_to_include` selection inside `build_snapshot_bytes`.
2. **Adjust capacity hint** — Update the `Vec::with_capacity` argument to count `screen_to_include`, not `screen`, so the main-buffer path does not over-allocate.
3. **Preserve append order** — Keep the same append order (`SNAPSHOT_CLEAR_HOME` -> scrollback -> screen-slice -> alt_mode) so callers continue to see a deterministic byte layout.
4. **Confirm callers unaffected** — Visually verify `build_shadow_parser_snapshot` and `handle_request_pane_snapshot` still pass scrollback / screen / alt_screen and need no signature change.
5. **Quick check** — `cargo check` to confirm the workspace still builds.

**Dependencies**: After Phase 1 (clean baseline). Blocks Phase 3.

**Testing Approach**:

- Unit: see Phase 3 test additions / refactors.
- Integration: existing `src-tauri/tests/mux_throughput.rs` continues to exercise the wire path; no new integration tests required.
- E2E: n/a.
- Manual: see SPEC.md "Manual Verification" — apt round-trip + alt-screen round-trip.

**Acceptance Criteria**:

- [ ] `build_snapshot_bytes(_, _, false)` output does not contain the supplied `screen` bytes.
- [ ] `build_snapshot_bytes(_, _, true)` output contains the supplied `screen` bytes positioned after stripped scrollback.
- [ ] `Vec::with_capacity` no longer counts the screen slice on the main-buffer branch.

**Estimated Effort**: small

---

### Phase 3: Doc Comments + Test Coverage

**Goal**: Make the main/alt split discoverable from the source itself, and lock in the new contract with unit tests.

**Files to Modify**:

- `src-tauri/src/mux/ipc/reattach.rs`
  - Update the doc comments on `build_snapshot_bytes`, `build_shadow_parser_snapshot`, and `handle_request_pane_snapshot` to explain the main/alt split.
  - Update the `SNAPSHOT_CLEAR_HOME` constant doc to note that for `alt_screen=false` the snapshot intentionally relies on scrollback-only reconstruction after the clear.
  - Refactor the existing layout tests so each asserts the correct branch after the fix:
    1. `build_snapshot_bytes_layout_is_clear_scrollback_screen` (currently asserts `b"SBSC"` with `alt_screen=false`) — rewrite to cover both branches: alt=false case asserts the output is `clear + "SB" + ESC[?1049l` (no "SC"); alt=true case asserts the output is `clear + "SB" + "SC" + ESC[?1049h`.
    2. `build_shadow_parser_snapshot_emits_scrollback_before_screen` (currently feeds a non-alt parser and asserts the screen string is present) — drive the parser into alt-screen mode first (or directly set up an alt-screen parser) so the test correctly exercises the alt=true path where the screen part is included.
    3. `build_shadow_parser_snapshot_empty_scrollback_is_clear_plus_shadow` (currently uses a non-alt parser and asserts `ONLY-SCREEN` is present) — drive the parser into alt-screen mode so the assertion continues to be valid under the new contract; OR rename + repurpose to assert the alt=false empty-scrollback case (clear + ESC[?1049l with no screen).
    4. `build_snapshot_bytes_strips_rich_content_from_scrollback` (currently calls with `alt_screen=false` and asserts `SCREEN` is preserved) — change the call site to `alt_screen=true` (or drop the `SCREEN`-presence assertion) so the test continues to validate rich-content stripping without conflating it with the new main/alt layout split.
  - Add a new `build_snapshot_bytes_main_buffer_omits_screen_part` test that supplies a recognizable `screen` slice (e.g. `b"SCREEN-SHOULD-BE-ABSENT"`) and asserts it is absent from the alt=false output.

**Files to Create**: None.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `build_snapshot_bytes_main_buffer_omits_screen_part` (new test) | Lock in FR1 layout | n/a | Asserts that for `alt_screen=false` the output is `SNAPSHOT_CLEAR_HOME` + stripped scrollback + `ESC[?1049l`, with no occurrence of the supplied `screen` slice. |
| `build_snapshot_bytes_alt_screen_includes_screen_part` (refactor) | Lock in FR2 layout | n/a | Asserts that for `alt_screen=true` the output is `SNAPSHOT_CLEAR_HOME` + stripped scrollback + `screen` + `ESC[?1049h`. |
| Doc comments | Codify FR3 | Functions exist | Each of the three functions documents which mode includes the daemon dump and why. |

**Processing Flow** (diagram-convertible):

1. Update `build_snapshot_bytes` doc comment to describe the main/alt branch and reference the discussion document's rationale (scrollback completeness vs. alt-buffer exclusion).
2. Update `build_shadow_parser_snapshot` doc comment to note that callers do not need to filter `screen` themselves — `build_snapshot_bytes` handles the alt/main split.
3. Update `handle_request_pane_snapshot` doc comment likewise.
4. Refactor the existing layout tests to align with the new contract (one alt=true assertion, one alt=false assertion).
5. Add the new `*_main_buffer_omits_screen_part` test.
6. Run the full `--lib` test suite.

**Implementation Steps** (5-7 max):

1. **Doc — build_snapshot_bytes (+ `SNAPSHOT_CLEAR_HOME`)** — Rewrite the function doc to spell out the alt/main split and rationale; extend the constant doc to note the scrollback-only reconstruction expectation for `alt_screen=false`.
2. **Doc — build_shadow_parser_snapshot + handle_request_pane_snapshot** — Mention that both paths funnel through `build_snapshot_bytes` and inherit its split contract.
3. **Refactor 4 existing tests** — Update each so it asserts the post-fix contract for the correct branch (see the "Files to Modify" sub-list above for site-specific guidance).
4. **Add new test** — Add `build_snapshot_bytes_main_buffer_omits_screen_part` that supplies a recognizable `screen` slice and confirms it is absent from the alt=false output.
5. **Run tests** — `cargo test --manifest-path src-tauri/Cargo.toml --lib` to confirm the suite is green.

**Dependencies**: After Phase 2 (uses the new branch).

**Testing Approach**:

- Unit: new + refactored layout tests assert FR1 / FR2.
- Integration: no new tests.
- E2E: n/a.
- Manual: SPEC.md "Manual Verification" scenarios.

**Acceptance Criteria**:

- [ ] Doc comments on the three functions describe the main/alt split.
- [ ] Layout tests cover both `alt_screen` branches.
- [ ] `cargo test --lib` passes for `src-tauri` and `crates/term_core`.

**Estimated Effort**: small

---

## Complete File Structure

```
crates/term_core/src/
  terminal_core.rs   # [DECSTBM-trace] log removed in set_scroll_region
  reflow.rs          # [DECSTBM-trace] log removed in resize_post_cleanup
src-tauri/src/
  mux/ipc/reattach.rs # alt/main branch + doc comments + test updates + probe_* removal + suspect-dump removal
  tabs.rs            # [DECSTBM-trace] logs removed in reset_frame_for_replay + apply_offthread_swap
doc/tasks/mux-snapshot-main-buffer-screen-omit/
  SPEC.md
  要件定義書.md
  IMPLEMENTATION.md
  VERIFICATION.md
  sdd.yaml
  tasks.yaml
```

## Testing Strategy

- **Unit**: New + refactored tests in `src-tauri/src/mux/ipc/reattach.rs` cover the alt/main layout contract directly. Adjacent existing tests (`build_snapshot_bytes_strips_rich_content_from_scrollback`, the two `build_shadow_parser_snapshot_*` tests) are refactored so each exercises a single branch of the new contract.
- **Integration**: `src-tauri/tests/mux_throughput.rs` exercises the wire path; no new integration tests required.
- **E2E**: None. The project has no E2E framework.
- **Manual**: SPEC.md "Manual Verification" — apt round-trip plus alt-screen TUI round-trip.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| vt100 | 0.16.2 (unchanged) | Daemon shadow parser. Still produces `contents_formatted()` for alt-screen use. |
| term_core | workspace (unchanged) | Client ANSI parser. Now drives the main-buffer reconstruction alone. |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Replay of scrollback-only path leaves the client viewport in a state that differs visibly from the daemon vt100 dump | Low | Medium | PROBE 6 demonstrated parity on the actual apt 17,632-byte scrollback. The new layout tests guard the contract. Manual scenarios cover apt + alt-screen rounds. |
| Accidentally regress alt-screen layout while touching `build_snapshot_bytes` | Low | High | Phase 3 refactors the existing alt-test to assert the alt=true layout explicitly. |
| Removing investigation code touches a site still depended on | Very Low | Low | Phase 1 runs `cargo check` immediately after removals. |

## Open Questions

- [ ] None. SPEC.md "Open Questions" is empty; the daemon-vt100 root-cause investigation is captured as a follow-up task and is explicitly out of scope.

## Success Metrics

- [ ] All FR1 / FR2 / FR3 / FR4 acceptance criteria from SPEC.md are met.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` passes for `src-tauri` and `crates/term_core`.
- [ ] `grep -R "\[DECSTBM-trace\]" src-tauri/ crates/` returns 0 hits.
- [ ] Manual apt + alt-screen round-trip verification succeeds.
