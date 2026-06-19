# Implementation Plan: Tab Bar Horizontal Scroll and Active Indicator

## Overview

Improve the native tab bar so the tab strip is horizontally scrollable without a
visible scrollbar (wheel / Shift+wheel), the keyboard-selected active cell scrolls
into view, and the active-indicator bar is unique across plain tabs and mux
sub-tabs. Native build only; the WebView tab bar (`src/`) is untouched.

## Objectives

- Make horizontal scrolling of an overflowing tab strip usable via the mouse wheel, with no scrollbar.
- Keep the keyboard-selected active cell visible by scrolling it into view, and only on keyboard activation.
- Ensure exactly one active-indicator bar is painted across plain tabs and mux sub-tabs.
- Preserve all existing behaviors: click switch, drag-reorder, mux sub-tab click, "+"/gear buttons.

## Prerequisites

### Development Environment

- Rust toolchain pinned by the repo (rustfmt `style_edition = 2024`).
- `egui` / `eframe` as already vendored by the `emterm` crate.

### Dependencies

- Internal: `src-tauri/src/ui/tab_bar.rs`, `src-tauri/src/app.rs`,
  `src-tauri/src/render/mod.rs`, `src-tauri/src/window_host.rs` (post-frame flag
  clear), `src-tauri/src/mux/window_group.rs` (read-only).
- External: `egui` primitives `ScrollArea`, `ScrollBarVisibility`,
  `Ui::scroll_to_rect`, wheel/scroll input. No new crates.

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: egui (`TopBottomPanel` + `ScrollArea::horizontal`), winit-driven render loop
- **Key Libraries**: egui — tab strip layout, scroll area, indicator painting

### Design Approach

The tab strip flattens the roster into ordered visual cells (`build_visuals`):
a plain tab is one cell; a mux tab expands into one sub-tab cell per window. Two
active layers exist:

- Plain-tab layer: `App::active` (the active roster index).
- Mux-window layer: `MuxWindowGroup::active`, surfaced per cell as
  `MuxSubTabCell::active`.

The four functional changes are localized:

- **FR1** suppresses the scrollbar on the existing overflow `ScrollArea` without
  disabling scrolling.
- **FR2/FR3** wire wheel-to-horizontal scroll for the hovered strip. FR3
  (Shift+wheel) already works: egui's input layer folds shift+vertical-wheel
  into the horizontal axis (`input_state/mod.rs:327-331`), which the
  horizontal-only `ScrollArea` consumes as-is. FR2 (plain vertical wheel) does
  **not** work by default: with the default style
  `always_scroll_the_only_direction = false` a horizontal-only `ScrollArea`
  reads only the horizontal scroll delta (`scroll_area.rs:923-933`), so a
  no-modifier vertical wheel is ignored. FR2 therefore requires an explicit
  enablement (set `always_scroll_the_only_direction = true` on the strip's
  scope, or translate the hovered wheel delta to a horizontal offset).
- **FR4** adds a one-shot "scroll active cell into view" signal on `App`, set only
  by keyboard tab/window switch handlers, consumed once inside the strip layout,
  then cleared post-frame.
- **FR5** gates the mux sub-tab indicator on the parent tab being the active tab.

### Component Interaction

```
key press → App tab/window switch handler → set scroll_active_tab_into_view (FR4)
                                           → request redraw
render::draw_terminal(&App) → read flag value → tab_bar::draw(..., scroll_flag)
   tab_bar::draw → ScrollArea (scrollbar hidden, FR1) → layout_tab_strip
       → active visual cell Rect captured
       → if flag set, scroll_to_rect(active_rect) once (FR4)
       → mux sub-tab indicator painted only if parent tab active (FR5)
window_host (post-frame, &mut App) → clear scroll_active_tab_into_view (FR4)
```

`draw_terminal` holds `&App` (immutable), so the flag is **read** into a value and
threaded into `draw()`; the **clear** happens after the egui pass where
`&mut App` is available (the same place tab events are applied today).

## Implementation Phases

### Phase 1: Scrollbar-free overflow scroll (FR1)

**Goal**: When the strip overflows, the horizontal scroll area renders with no
visible scrollbar while remaining scrollable; the fit (equal-width) path is unchanged.

**Files to Modify**:
- `src-tauri/src/ui/tab_bar.rs` — set scrollbar visibility to always-hidden on the overflow `ScrollArea` (`draw()` overflow branch).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Overflow `ScrollArea` config | Hide the scrollbar on the strip's horizontal scroll area | `needed_w > scroll_w` (overflow) | Strip scrolls horizontally; no scrollbar drawn |

**Processing Flow** (diagram-convertible):
1. `draw()` computes `needed_w` vs `scroll_w`.
   - `needed_w > scroll_w` → overflow branch: build the horizontal scroll area with scrollbar visibility set to always-hidden, then lay out the strip.
   - otherwise → fit branch (equal-width, non-scrolling): unchanged.

**Implementation Steps**:
1. **Suppress the scrollbar** — Configure the overflow-branch horizontal scroll area so its scrollbar is never shown, leaving scroll behavior intact.

**Dependencies**: None. Blocks Phase 2 (same scroll area).

**Testing Approach**:
- Unit: existing strip-layout tests still pass (no cell-geometry regression).
- Manual: with overflowing tabs, confirm no scrollbar is visible.

**Acceptance Criteria**:
- [ ] When tabs overflow, no scrollbar is rendered.
- [ ] The fit (non-overflow) path is unchanged.

**Estimated Effort**: small

---

### Phase 2: Wheel / Shift+wheel horizontal scroll (FR2, FR3)

**Goal**: While the pointer is over the tab bar, vertical wheel and Shift+wheel
scroll the strip horizontally.

**Files to Modify**:
- `src-tauri/src/ui/tab_bar.rs` — enable plain vertical wheel → horizontal scroll on the strip (FR2); Shift+wheel (FR3) already maps to horizontal in egui and needs no change.

**egui 0.29.1 baseline (verified against the vendored source)**:
- **FR3 (Shift+wheel)** — works out of the box. egui's input layer rewrites
  shift+vertical-wheel onto the horizontal axis (`input_state/mod.rs:327-331`),
  and the horizontal-only `ScrollArea` consumes that horizontal delta.
- **FR2 (plain vertical wheel)** — does **not** work with the default style.
  A horizontal-only `ScrollArea` reads only the horizontal scroll delta unless
  `ui.style().always_scroll_the_only_direction` is `true`
  (`scroll_area.rs:923-933`), and the default is `false` (`style.rs:1232`). So a
  no-modifier vertical wheel over the strip is ignored until this is enabled.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Vertical-wheel → horizontal enablement (FR2) | Set `always_scroll_the_only_direction = true` for the strip's scroll scope so a hovered no-modifier vertical wheel scrolls the strip horizontally | Strip overflowing; pointer over the tab bar | Vertical wheel moves the strip horizontally; tab selection unchanged |
| Shift+wheel passthrough (FR3) | None — egui already maps Shift+wheel to horizontal scroll | Strip overflowing; pointer over the tab bar | Strip scroll offset changes; tab selection unchanged |

**Processing Flow** (diagram-convertible):
1. Pointer hovers the tab strip and the wheel rolls.
   - Shift+wheel → egui has already folded the delta onto the horizontal axis; the horizontal `ScrollArea` consumes it (no change).
   - Plain vertical wheel → consumed as horizontal only when `always_scroll_the_only_direction = true` on the strip scope; enable it (FR2). (Alternative: translate the hovered wheel delta to a horizontal offset explicitly.)
2. Tab selection state is not consulted or mutated by the scroll path.

**Implementation Steps**:
1. **Enable vertical-wheel → horizontal (FR2)** — On the strip's scroll scope, set `ui.style_mut().always_scroll_the_only_direction = true` (the strip is horizontal-only, so the flag only affects this area) so a hovered no-modifier vertical wheel scrolls horizontally. No new keybinding.
2. **Confirm Shift+wheel (FR3)** — Verify Shift+wheel still scrolls the strip horizontally (already mapped by egui; no code change expected).
3. **Explicit translation fallback** — If the style flag is undesirable for any reason, convert the hovered wheel delta to a horizontal scroll offset for the strip instead.

**Dependencies**: Requires Phase 1 (same scroll area). Blocks nothing.

**Testing Approach**:
- Manual: hover the overflowing strip; plain vertical wheel scrolls horizontally (FR2); Shift+wheel scrolls horizontally (FR3); selection does not change.
- Unit: scroll is an egui input-driven behavior not directly assertable without driving raw wheel input; covered by a manual check and the existing no-selection-change click tests as a regression guard.

**Acceptance Criteria**:
- [ ] Hovering the tab bar and rolling the wheel vertically scrolls the strip horizontally.
- [ ] Shift+wheel also scrolls the strip horizontally.
- [ ] Tab selection is not changed by scrolling.

**Estimated Effort**: small

---

### Phase 3: Active-cell scroll-into-view on keyboard switch (FR4)

**Goal**: When the active cell changes via keyboard (plain-tab selection or mux
window selection), the active visual cell is scrolled into view if it is off-screen.
Triggered only by keyboard activation; never by mouse scroll or unrelated repaints.

**Files to Modify**:
- `src-tauri/src/app.rs` — add a one-shot flag field; raise it on a committed
  plain-tab switch and on a committed mux window switch.
  - **Plain-tab path**: `NextTab`/`PrevTab`/`JumpTab` all funnel through
    `switch_to_tab(idx)` (`app.rs:1376`), which early-returns when
    `idx >= len || idx == self.active` (no-op). Set the flag **inside
    `switch_to_tab`** after the `self.active = idx` commit, so the "active
    actually changed" guard is the function's own early-return — no need to
    re-derive it in each of the three handlers (which would risk drift).
  - **Mux path**: set it in `dispatch_mux_action` (`app.rs:2145`) on the
    existing `if outcome == MuxActionOutcome::Changed` block (`app.rs:2240`),
    next to the `needs_full_redraw = true` it already sets. Do not modify mux
    active-window state beyond the existing switch. Caveat: `switch_to` /
    `SelectWindow(d)` report `Changed` even when the digit targets the
    already-active window (no same-index short-circuit before the
    `SwitchWindow` send), so a same-window `prefix <digit>` jump will set the
    flag. The scroll-into-view of an already-visible active cell is a harmless
    no-op, so this is acceptable; do not add a redundant pre-check unless TS-9
    requires the strict "already-active does not set the flag" assertion (see
    Testing Approach).
- `src-tauri/src/render/mod.rs` — read the flag value (immutable `&App` at
  `draw_terminal`, `mod.rs:193`) and thread it into `tab_bar::draw`
  (`mod.rs:244`).
- `src-tauri/src/ui/tab_bar.rs` — accept the flag on `draw` / `layout_tab_strip`
  (also updating the in-file test call sites of `draw`, see Implementation
  Steps), capture the active visual cell's `Rect` during layout, and request
  scroll-into-view once when the flag is set.
- `src-tauri/src/window_host.rs` — clear the one-shot flag after the egui pass
  in `render(&mut self, app: &mut App)` (`window_host.rs:1233`), after the
  `egui_ctx.run(...)` closure returns (where `&mut App` is available — the same
  post-frame block that applies `frame_events.tab` at `window_host.rs:1366`).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `App` one-shot flag (e.g. `scroll_active_tab_into_view`) | Signal "scroll the active cell into view next frame" | Set only by keyboard tab/window switch handlers | Read once by the strip; cleared after the frame |
| Keyboard switch handlers | Raise the flag when the active cell changed by keyboard | A `NextTab`/`PrevTab`/`JumpTab` or mux next/prev/digit changed the active cell | Flag is set; redraw requested |
| Strip active-cell capture | Find the active visual cell's `Rect` (plain cell at `active_idx`, or the active mux sub-tab cell within the active tab) and request scroll-into-view once | Flag set; active cell laid out this frame | Active cell scrolled into view; best-effort no-op if its rect is unavailable |
| Post-frame clear | Reset the flag so it never re-fires on an unrelated repaint | One frame after the flag was set | Flag is `false` |

**Processing Flow** (diagram-convertible):
1. Keyboard tab/window switch handler runs.
   - active cell changed → set the one-shot flag, request redraw.
   - no change (e.g. switch to the already-active cell) → leave flag unchanged.
2. `render::draw_terminal` reads the flag value (immutable `&App`) and passes it into `tab_bar::draw`.
3. Inside `layout_tab_strip`, while iterating visual cells, record the active visual cell's `Rect`:
   - plain-tab cell where cell index `== active_idx`, or
   - mux sub-tab cell where `tab == active_idx && mux_cell.active`.
4. After the active cell's `Rect` is known:
   - flag set → request scroll-into-view for that rect exactly once.
   - flag not set → do nothing.
5. After the egui pass, `window_host` clears the flag (it had `&mut App`).

**Implementation Steps**:
1. **Add the one-shot flag** — Introduce a boolean field on `App` next to the redraw flags, default off.
2. **Raise it on a committed plain-tab switch** — Set the flag inside `switch_to_tab` after the `self.active = idx` commit (the function already early-returns on a no-op switch, so the flag is raised only when `active` actually moved). `NextTab`/`PrevTab`/`JumpTab` all route through here, so no per-handler change is needed.
3. **Raise it in the mux prefix path** — In `dispatch_mux_action`, set the flag in the existing `outcome == MuxActionOutcome::Changed` block (alongside `needs_full_redraw`). Do not modify mux active-window state beyond the existing switch. (See the same-window-digit caveat above.)
4. **Thread the flag into the strip** — Extend `tab_bar::draw` (and `layout_tab_strip`) to take the flag value; `render::draw_terminal` reads and forwards it. **Update the in-file `tab_bar.rs` test call sites of `draw` (5 sites: the `run_with_click` pair, `capture_rect_with`, the single-tab test, and `mux_cell_rects`) to pass the new argument (e.g. `false`), or give `draw` a thin test-only wrapper so the existing tests keep compiling.**
5. **Capture + scroll-into-view once** — In the strip layout, track the active visual cell's `Rect` and request scroll-into-view a single time when the flag is set.
6. **Clear post-frame** — In `window_host::render`, after the `egui_ctx.run` closure returns, reset the flag.

**Dependencies**: Requires Phase 1/Phase 2 (scrollable strip). Blocks nothing.

**Testing Approach**:
- Unit (tab_bar): given an off-screen active cell and the flag set, the strip requests scroll-into-view for the active cell (assert via the existing test hooks / a captured "scroll target" rect, or a pure helper that selects the active visual cell rect).
- Unit (app): `NextTab`/`PrevTab`/`JumpTab` set the flag when the active index changes; switching to the already-active tab does not set it.
- Unit (app): the mux next/prev/digit switch path sets the flag on a committed window change (`MuxActionOutcome::Changed`). Note on TS-9's "switching to the already-active window does not set it": `next_index`/`prev_index` return `None` for <2 windows (so `switch_to` yields `None`, no flag), but `SelectWindow(d)` (`digit_index`) targeting the current window still reports `Changed` (`switch_to` has no same-index short-circuit). Pick one and align the test: (a) accept the harmless no-op scroll and assert TS-9 only on the next/prev paths, or (b) add a same-index guard (`idx == group.active_index()` → don't raise the flag) and assert TS-9 strictly. Document the choice in the test.
- Manual: with mouse-driven scroll followed by an unrelated repaint, the active cell is NOT force-scrolled back into view (flag only set on keyboard activation).

**Acceptance Criteria**:
- [ ] Pressing a tab-switch key scrolls the newly active cell into view when off-screen.
- [ ] Scroll-into-view fires only as a result of keyboard activation, not on unrelated repaints.
- [ ] The mux window-switch keyboard path also scrolls the active sub-tab into view.

**Estimated Effort**: medium

---

### Phase 4: Unique active indicator across mixed tabs (FR5)

**Goal**: A mux sub-tab's active-indicator bar is painted only when its parent mux
tab is the currently active tab. When the parent is not active, no sub-tab indicator
is painted. The mux group's active-window state is not modified, so re-activating the
mux tab restores the bar on its previously active window's sub-tab.

**Files to Modify**:
- `src-tauri/src/ui/tab_bar.rs` — gate the mux sub-tab indicator on `tab == active_idx` in addition to `mux_cell.active`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Mux sub-tab indicator gate | Paint the sub-tab bar only when the parent mux tab is active | Iterating a mux sub-tab cell | Bar painted iff `tab == active_idx && mux_cell.active`; otherwise no bar |

**Processing Flow** (diagram-convertible):
1. For each mux sub-tab cell of tab `tab` with `mux_cell.active`:
   - `tab == active_idx` (parent tab is the active tab) → paint the active indicator.
   - `tab != active_idx` (parent tab not active) → do not paint the indicator.
2. The mux group's `active` window index is read only and is structurally unreachable from the gate: `draw`/`layout_tab_strip` operate on a per-frame immutable `&[TabBarItem]` snapshot (built in `render/mod.rs` via `mux_group_render_model`), so they cannot touch `MuxWindowGroup` at all. `mux_cell.active` is a copied bool on `MuxSubTabCell`, and `tab`/`active_idx` are plain indices. So FR5's "active-window state is not modified" is guaranteed by the data flow, not just by discipline.
3. The sub-tab label color (currently `mux_cell.active`-based emphasis at `tab_bar.rs:459-464`) is retained as-is — only the **bar** at `tab_bar.rs:474-476` is gated. (Decision: keep the existing label-color behavior; the required change is the indicator bar only, per SPEC.)

**Implementation Steps**:
1. **Gate the indicator** — In the mux sub-tab branch of `layout_tab_strip` (`tab_bar.rs:447-487`), change the `if is_active_cell` guard at `tab_bar.rs:474` so the indicator is painted only when `tab == active_idx && mux_cell.active`. `tab` is already in scope from `Visual::Mux { tab, cell }` and `active_idx` is already a `layout_tab_strip` parameter, so no new threading is needed. Leave the label-color logic (`is_active_cell`) unchanged.

**Dependencies**: Independent of Phases 1–3 (can land in any order). Blocks nothing.

**Testing Approach**:
- Unit (tab_bar): with a non-active mux tab, no sub-tab indicator is painted; with the mux tab active, the active window's sub-tab indicator is painted. Assert via the existing `LAST_MUX_CELLS` hook plus a captured "indicator painted" signal (e.g. a thread-local recording which cell rect received the indicator), so the gate is verifiable without GPU readback.
- Unit (tab_bar): mux group active-window state is unchanged by the gate (the render model still reports the same `active` window after a draw with a non-active parent).
- Manual: activate a plain tab while a mux tab has an active sub-tab — the mux sub-tab bar disappears; re-activate the mux tab — the bar returns on the previously active window.

**Acceptance Criteria**:
- [ ] Activating a plain tab removes the indicator bar from any non-active mux tab's sub-tabs.
- [ ] Exactly one indicator bar is shown across the whole strip.
- [ ] Re-activating a mux tab restores the bar on its previously active window's sub-tab.

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
├── ui/tab_bar.rs        # draw(), layout_tab_strip(), indicator gate — FR1, FR2, FR3, FR4, FR5
├── app.rs               # scroll-into-view one-shot flag + keyboard/mux switch handlers — FR4
├── render/mod.rs        # reads the flag, threads it into tab_bar::draw — FR4
├── window_host.rs       # post-frame flag clear — FR4
└── mux/window_group.rs  # MuxWindowGroup::active (read-only for FR5)
```

(No new files. WebView tab bar under `src/` is not touched — NFR3.)

## Testing Strategy

- **Unit** (in `tab_bar.rs` tests and `app.rs` tests): FR4 flag propagation
  (keyboard plain-tab + mux paths), FR4 active-visual-cell selection, FR5
  indicator gating and mux-state preservation, plus regression of existing
  strip-geometry / click / drag tests.
- **Integration**: keyboard tab switch sets the flag (exercised through
  `App::apply_action`).
- **E2E**: none — this project has no E2E framework.
- **Manual**: scroll behavior (wheel / Shift+wheel, no scrollbar), scroll-into-view
  on keyboard switch, no-snap-back after mouse scroll, and visual uniqueness of the
  active indicator with mixed tabs.

Run from the project root with explicit target dir:

- Check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Test: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --bin emterm`

Formatting is applied per-changed-file by the PostToolUse hook; do not run a
crate-wide `cargo fmt`. Release builds (`cargo build --release`) are run only on
explicit user request, not during implementation.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| egui / eframe | as vendored | `ScrollArea`, `ScrollBarVisibility`, `Ui::scroll_to_rect`, wheel input (no version change) |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Plain vertical wheel is not mapped to horizontal scroll for the strip (confirmed: egui's default `always_scroll_the_only_direction = false` ignores the vertical delta on a horizontal-only `ScrollArea`) | High (known) | Low | FR2 sets `always_scroll_the_only_direction = true` on the strip scope; explicit delta-to-horizontal translation remains as a fallback. Shift+wheel (FR3) is unaffected — egui maps it at the input layer |
| `draw_terminal` is `&App` (immutable), so the flag cannot be cleared inside draw | High (known) | Low | Read the flag into a value, thread it into `draw`; clear post-frame in `window_host` where `&mut App` exists |
| Scroll-into-view re-fires on unrelated repaints | Medium | Medium | One-shot semantics: set only by keyboard handlers, cleared every frame |
| Indicator-gate change perturbs strip geometry / breaks existing tests | Low | Medium | The gate only suppresses a painter call; cell allocation is unchanged. Re-run existing tab_bar tests |
| Per-frame cost regression (NFR1) | Low | Low | The gate is a boolean check; scroll-into-view is one rect compare per frame only when the flag is set — no new allocations in the layout loop |

## Open Questions

- [ ] None outstanding. All SPEC open questions are resolved (sdd.yaml has no `tbd` requirements; the SPEC "Open Questions" section is empty and 要件定義書 §14.1 confirms the three decisions).

## Success Metrics

- [ ] Functional completeness: FR1–FR5 implemented; NFR1–NFR3 upheld.
- [ ] Quality: existing tab_bar / app tests stay green; new FR4/FR5 unit tests pass.
- [ ] No regression: click switch, drag-reorder, mux sub-tab click, "+"/gear buttons unchanged; WebView tab bar untouched.
