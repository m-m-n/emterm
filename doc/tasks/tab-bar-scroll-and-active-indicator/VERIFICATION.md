# Verification Document: Tab Bar Horizontal Scroll and Active Indicator

## Overview

**Feature**: tab-bar-scroll-and-active-indicator
**SPEC.md**: `doc/tasks/tab-bar-scroll-and-active-indicator/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/tab-bar-scroll-and-active-indicator/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors. (Release builds only on explicit user request.)

### Result (sdd.4-implement)

- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` → exit 0, no errors, no new warnings (default `gui` build).
- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` → exit 0 (CLI-only feature gate intact; all FR1–FR5 changes are under `gui` via the `ui` / `render` / `window_host` modules).
- Release build (`cargo build --release`) intentionally NOT run (per project policy; user-explicit only).

## Test Verification

- Default suite: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --bin emterm`
- Coverage target: not coverage-driven; assert the named scenarios below.

### Result (sdd.4-implement)

The feature's unit/integration tests live in the **`--lib`** target (`ui::tab_bar`
and `app` test modules), not `--bin emterm`. The documented `--bin emterm`
command therefore exercises 0 of these tests; `--lib` is the correct target.

- `... cargo test --lib` (default parallel): **1834 passed**, with a flaky,
  non-deterministic subset of `tabs::tests::*` off-thread-replay-worker tests
  failing (2–4 per run). Verified on the **unmodified baseline (HEAD, this
  feature stashed)** these same `tabs.rs` worker tests fail the same way — it is
  a pre-existing parallel-execution starvation issue (`test_poll_until_swapped`
  spins a bounded 10 000 iterations waiting for a background thread that gets
  CPU-starved under 1800+ parallel tests). Not caused by and not touching this
  feature (no `tabs.rs` change; `git status` confirms).
- `... cargo test --lib -- --test-threads=1` (removes worker starvation):
  **1834 passed, 0 failed, 1 ignored** — clean.
- `... cargo test --bin emterm`: 0 tests (feature tests are in the lib target).
- New tests added (all green): tab_bar `ts2/ts3/ts4` (FR5), `ts5*/ts6*` (FR4
  active-cell selection + scroll-into-view request), app `ts7/ts8` (plain-tab
  flag), `ts9_*` (mux switch flag, option-b strict same-index guard). Existing
  21 tab_bar tests + all app tab-switch tests still pass (TS-1 regression clean).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Existing strip geometry / click / drag tests after FR1 scrollbar-hidden change | All existing `tab_bar.rs` tests still pass; cell rects and click routing unchanged | Unit |
| TS-2 | FR5: a non-active mux tab's sub-tab is laid out (mux cell active) but its parent tab is not the active tab | No active-indicator bar painted for that sub-tab | Unit |
| TS-3 | FR5: the mux tab is the active tab and one window is active | The active window's sub-tab indicator bar is painted | Unit |
| TS-4 | FR5: drawing with a non-active parent mux tab | `MuxWindowGroup::active` (the render model's active window) is unchanged after the draw | Unit |
| TS-5 | FR4: the scroll-into-view flag is set and the active visual cell is off-screen | The strip selects the active visual cell's rect and requests scroll-into-view exactly once | Unit |
| TS-6 | FR4: active visual cell selection picks the plain-tab cell at `active_idx`, or the active mux sub-tab cell within the active mux tab | The correct cell rect is chosen for scroll-into-view | Unit |
| TS-7 | FR4: `NextTab` / `PrevTab` / `JumpTab` change the active index | The one-shot scroll-into-view flag is set | Unit / Integration |
| TS-8 | FR4: `JumpTab`/switch to the already-active tab (no change) | The flag is NOT set | Unit |
| TS-9 | FR4: mux next/prev/digit window switch commits a window change | The flag is set; switching to the already-active window does not set it | Unit |
| TS-10 | Edge: tabs exactly fit the width (no overflow) | No scroll area, no scrollbar, no scroll-into-view side effect | Unit / Manual |
| TS-11 | Edge: single tab | No overflow; indicator on the only tab | Unit / Manual |

## Code Quality Verification

- Format: PostToolUse hook formats each changed file (rustfmt, `style_edition = 2024`). Do NOT run a crate-wide `cargo fmt`.
- Static analysis: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` clean (no new warnings).

### Result (sdd.4-implement)

- Per-changed-file formatting applied by the PostToolUse hook (no crate-wide `cargo fmt` run).
- `cargo check` (default `gui` build): clean, **no new warnings**.

### Known Limitations

- `src-tauri/src/tabs.rs` off-thread replay worker tests (`swap_replaces_outgoing_content`,
  `ts5_*`, `ts9_no_residual_rows_after_offthread_swap_to_shorter_pane`, etc.) are
  flaky under the default parallel `cargo test --lib` due to background-thread
  CPU starvation (bounded-spin `test_poll_until_swapped`). This is **pre-existing**
  (reproduced on unmodified HEAD with this feature stashed) and unrelated to this
  feature. Run `--test-threads=1` for a deterministic green suite. No action taken
  here (out of scope; `tabs.rs` untouched).

## File Structure Verification

### Files to Create

- None.

### Files to Modify

- [x] `src-tauri/src/ui/tab_bar.rs` — FR1 (`ScrollBarVisibility::AlwaysHidden`), FR2 (`always_scroll_the_only_direction = true` on the strip scope; FR3 Shift+wheel needs no change), FR4 (flag param threaded into `draw`/`layout_tab_strip`, active visual cell `Rect` captured, single `ui.scroll_to_rect`), FR5 (indicator gated on `tab == active_idx && mux_cell.active`). Test hooks `LAST_INDICATOR_RECTS` / `LAST_SCROLL_INTO_VIEW_RECT` added; 5 in-file `draw` call sites updated to pass `false`.
- [x] `src-tauri/src/app.rs` — FR4 `scroll_active_tab_into_view: bool` field + init + `scroll_active_tab_into_view()` / `clear_scroll_active_tab_into_view()` accessors; flag raised in `switch_to_tab` (plain-tab) and in `dispatch_mux_action`'s `Changed` block guarded by a before/after active-index compare (TS-9 option b).
- [x] `src-tauri/src/render/mod.rs` — FR4 reads `app.scroll_active_tab_into_view()` and forwards it into `tab_bar::draw`.
- [x] `src-tauri/src/window_host.rs` — FR4 clears the flag immediately after the `egui_ctx.run` closure returns.
- [x] `src-tauri/src/mux/window_group.rs` — read-only (FR5); **no change** (confirmed via `git status`).

### Result (sdd.4-implement)

`git status` changed files: `src-tauri/src/{app.rs, render/mod.rs, ui/tab_bar.rs, window_host.rs}` only.
**No `src/` (WebView) file touched → NFR3 / SC-4 satisfied.** No new files created.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements (FR1–FR5) implemented | Phase acceptance criteria + TS-1..TS-11 |
| SC-2 | All test scenarios pass | `cargo test` green for the new + existing tab_bar/app tests |
| SC-3 | No regression in click switch, drag-reorder, mux sub-tab click, "+"/gear buttons | Existing `tab_bar.rs` tests (TS-1) + manual smoke |
| SC-4 | WebView tab bar unchanged | `git diff` touches no `src/` file (NFR3) |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (scrollbar-free overflow scroll) | Phase 1 | TS-1 (no geometry regression) + manual (no scrollbar) |
| FR2 (wheel horizontal scroll) | Phase 2 | Manual M-1 |
| FR3 (Shift+wheel horizontal scroll) | Phase 2 | Manual M-2 |
| FR4 (active scroll-into-view, keyboard-only) | Phase 3 | TS-5, TS-6, TS-7, TS-8, TS-9 + manual M-3, M-4 |
| FR5 (unique active indicator) | Phase 4 | TS-2, TS-3, TS-4 + manual M-5 |
| NFR1 (per-frame cost) | Phases 3–4 | Code review: boolean gate + single rect compare, no new layout-loop allocations |
| NFR2 (compatibility) | All | TS-1 + manual smoke of click / drag / mux-click / "+"/gear |
| NFR3 (scope isolation) | All | SC-4 (`git diff` has no `src/` changes) |

## E2E Testing

This project has no E2E framework. Not applicable.

## Manual Testing (E2E Not Possible)

- [ ] M-1 (FR2): With more tabs than fit the width, hover the tab bar and roll the wheel vertically — the strip scrolls left/right; the selected tab does not change.
- [ ] M-2 (FR3): Shift+wheel over the tab bar also scrolls the strip horizontally.
- [ ] M-3 (FR4): With an off-screen active tab, press Ctrl+PageUp/PageDown / Ctrl+Tab / Ctrl+1..9 — the newly active tab scrolls into view.
- [ ] M-4 (FR4): Mouse-scroll the strip, then trigger an unrelated repaint — the active tab is NOT force-scrolled back into view.
- [ ] M-5 (FR5): With a mux tab (active sub-tab) and a plain tab, activate the plain tab — the mux sub-tab bar disappears (one bar total); re-activate the mux tab — the bar returns on the previously active window.
- [ ] M-6 (NFR2): Smoke the existing affordances — plain-tab click switch, drag-reorder, mux sub-tab click switch, "+" and gear buttons — all behave as before.
- [ ] M-7 (FR1): With overflowing tabs, confirm no scrollbar is visible.

## Performance Verification

- NFR1: The added per-frame work is a boolean indicator gate (FR5) and, only when the one-shot flag is set, a single rect-vs-viewport scroll-into-view request (FR4). Verify by code review that no new per-frame allocation is added inside the strip layout loop.

## Security Verification

- Not applicable (UI rendering change; no new external input handling beyond existing wheel/key events).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit/Integration scenarios | 11 (TS-1..TS-11) | 9 fully automated; 2 (TS-10, TS-11) automated + manual | 0 | — |
| Manual | 7 (M-1..M-7) | 0 | 0 | 7 |
| Success criteria | 4 (SC-1..SC-4) | 3 | 0 | 1 |
