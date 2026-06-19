# Feature: Tab Bar Horizontal Scroll and Active Indicator

## Overview

Improve the native tab bar for two cases: when there are more tabs than fit the
window width, and when mux tabs and plain tabs coexist. The tab strip becomes
horizontally scrollable without a visible scrollbar (wheel and Shift+wheel), the
active tab scrolls into view on keyboard navigation, and the active-indicator bar
is shown for exactly one cell across the whole strip.

## Objectives

- Make horizontal scrolling of the tab strip usable via mouse wheel, with no scrollbar.
- Keep the keyboard-selected active tab visible by scrolling it into view.
- Ensure the active-indicator bar is unique across plain tabs and mux sub-tabs.

## User Stories

### US1: Scroll the tab strip with the wheel
As a user with many tabs, I want to scroll the tab strip left/right with the mouse
wheel (no scrollbar), so that I can reach off-screen tabs quickly.

**Acceptance Criteria:**
- [ ] When tabs overflow, no scrollbar is rendered.
- [ ] Hovering the tab bar and rolling the wheel vertically scrolls the strip horizontally.
- [ ] Shift+wheel also scrolls the strip horizontally.
- [ ] Tab selection is not changed by scrolling.

### US2: Keep the active tab in view
As a user, I want the active tab to scroll into view when I change tabs by keyboard,
so that the selected tab is never hidden off-screen.

**Acceptance Criteria:**
- [ ] Pressing a tab-switch key (Ctrl+PageUp/PageDown, Ctrl+Tab/Ctrl+Shift+Tab, Ctrl+1..9)
      scrolls the newly active cell into view when it is off-screen.
- [ ] Scroll-into-view fires only as a result of keyboard activation when switching among
      existing tabs (new-tab creation is a separate trigger, see US4/FR6), not on unrelated repaints.

### US3: Unique active indicator with mixed tabs
As a user with mux tabs and plain tabs, I want only one active-indicator bar visible,
so that the active tab is unambiguous.

**Acceptance Criteria:**
- [ ] Activating a plain tab removes the active-indicator bar from any non-active mux tab's sub-tabs.
- [ ] The active-indicator bar is shown for exactly one cell across the whole strip.
- [ ] Re-activating a mux tab restores the bar on its previously active window's sub-tab.

### US4: See a newly created tab or mux window
As a user, I want a newly created tab or mux window to be brought into view, so that what
I just opened is visible even when the strip overflows.

**Acceptance Criteria:**
- [ ] Creating a tab (via the `+` button or a keybind) makes it active and scrolls it into
      view when it lands off-screen.
- [ ] Creating a mux window on the active tab makes its new sub-tab active and scrolls it
      into view when it lands off-screen.

## Technical Requirements

### Functional Requirements

- **FR1 - Scrollbar-free overflow scroll:** When the tab strip overflows the available
  width, the horizontal scroll area is rendered without a visible scrollbar. When tabs
  fit, the existing equal-width (non-scrolling) layout is kept.
- **FR2 - Wheel horizontal scroll:** While the pointer is over the tab bar, vertical mouse
  wheel motion scrolls the tab strip horizontally.
- **FR3 - Shift+wheel horizontal scroll:** Shift+wheel scrolls the tab strip horizontally.
- **FR4 - Active tab scroll-into-view:** When the active cell changes via keyboard
  (plain-tab selection or mux window selection), the active visual cell is scrolled into
  view if it is outside the visible strip. This is triggered only by keyboard activation
  when switching among existing tabs; creating a new tab is a separate trigger (FR6).
- **FR5 - Unique active indicator:** A mux sub-tab's active-indicator bar is painted only
  when its parent mux tab is the currently active tab. When the parent tab is not active,
  no sub-tab indicator is painted. The mux group's active-window state is not modified.
- **FR6 - New tab / mux window scroll-into-view:** When a new tab is created, or a new mux
  window is appended to the active tab, the newly-active cell is scrolled into view if it
  lands outside the visible strip. This applies to every creation path (the `+` button,
  keybinds, and the asynchronous mux `new-window` flow confirmed by the daemon's
  `PaneCreated`). The FR4 keyboard-only constraint governs switching among existing
  tabs/windows; creating a new tab or mux window is a distinct trigger and is not bound by
  it — a freshly created cell has not been seen yet, so it surfaces regardless of input method.

### Non-Functional Requirements

- **NFR1 - Performance:** The tab strip is laid out and painted every frame. The added
  logic (scroll-into-view check, indicator gating) must not introduce per-frame work that
  meaningfully degrades render latency.
- **NFR2 - Compatibility:** Existing behaviors must continue to work: plain-tab click
  switch, drag-reorder, mux sub-tab click switch, and the fixed "+"/gear buttons.
- **NFR3 - Scope isolation:** Only the native build is changed. The WebView tab bar
  (`src/`) is not touched.

## Implementation Approach

### Architecture

The tab bar is drawn in `src-tauri/src/ui/tab_bar.rs::draw()` using an egui
`TopBottomPanel` with a `ScrollArea::horizontal()` engaged only when
`needed_w > scroll_w`. The strip flattens the roster into visual cells via
`build_visuals()`; plain tabs render one cell, mux tabs expand into header + per-window
sub-tab cells. The active state is two-layered:

- Plain-tab layer: `App::active: usize` (`src-tauri/src/app.rs`).
- Mux-window layer: `MuxWindowGroup::active: usize` (`src-tauri/src/mux/window_group.rs`),
  surfaced into render cells by `mux_group_render_model()` as `MuxSubTabCell::active`.

```
draw(ctx, items, active_idx)
  └─ TopBottomPanel(top)
       ├─ ScrollArea::horizontal (engaged when needed_w > scroll_w)
       │    └─ layout_tab_strip(ui, items, active_idx, width)
       │         ├─ Visual::Tab  → indicator when i == active_idx
       │         └─ Visual::Mux  → indicator when mux_cell.active   ← FR5: gate on parent active
       └─ fixed area ("+" / gear)
```

### Data Flow

```
key press → App tab/window switch → set scroll_into_view flag (FR4)
                                   → request redraw
draw() → ScrollArea (no scrollbar, FR1) → layout_tab_strip
       → active cell rect captured → if flag set, ui.scroll_to_rect(active_rect) → clear flag
       → mux sub-tab indicator painted only if parent tab is active (FR5)
```

### Key Changes

1. **FR1 (scrollbar visibility):** On the `ScrollArea::horizontal()` used at
   `tab_bar.rs:210`, set the scrollbar visibility to always-hidden
   (`egui::scroll_area::ScrollBarVisibility::AlwaysHidden`). The strip stays scrollable;
   only the scrollbar is suppressed.

2. **FR2 / FR3 (wheel scroll):** egui maps wheel/Shift+wheel to horizontal scrolling inside
   a horizontal `ScrollArea`. Confirm the tab-bar `ScrollArea` receives wheel events while
   hovered (it is allocated inside the panel and already consumes scroll). If vertical wheel
   does not translate to horizontal scroll for this area, translate the hovered wheel delta
   to a horizontal scroll offset for the strip explicitly. No new keybinding is added.

3. **FR4 (scroll-into-view):** Add a one-shot flag on `App` (e.g.
   `scroll_active_tab_into_view: bool`) set when a keyboard tab/window switch changes the
   active cell (the `NextTab`/`PrevTab`/`JumpTab` handlers in `app.rs:1532-1589` and the mux
   prefix window-switch path in `app.rs:2145-2241`). Pass it into `draw()` so that, after the
   active visual cell's `Rect` is known inside `layout_tab_strip`, the code calls
   `ui.scroll_to_rect(active_rect, None)` once and the flag is cleared for the next frame.
   The active visual cell is the plain-tab cell at `active_idx`, or the active mux sub-tab
   cell (`mux_cell.active`) within the active mux tab.

4. **FR5 (unique indicator):** In `layout_tab_strip`, the mux sub-tab branch
   (`tab_bar.rs:447-487`) currently paints the indicator when `is_active_cell =
   mux_cell.active`. Gate this on the parent tab being active as well: paint the sub-tab
   indicator only when `tab == active_idx && mux_cell.active`. The sub-tab label color may
   keep its current `mux_cell.active`-based emphasis or be reviewed during planning; the
   required behavior change is that the **bar** is not painted for a non-active parent tab.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/ui/tab_bar.rs`: tab strip layout, scroll area, indicator painting.
- `src-tauri/src/app.rs`: tab/window switch handlers; new scroll-into-view flag; `draw()` call site.
- `src-tauri/src/render/mod.rs`: `TabBarItem` view-model construction (if the flag is threaded through here).
- `src-tauri/src/mux/window_group.rs`: `MuxWindowGroup::active` (read-only for FR5).

**External Dependencies:**
- `egui` / `eframe`: `ScrollArea`, `ScrollBarVisibility`, `Ui::scroll_to_rect`.

### File Structure

```
src-tauri/src/
├── ui/tab_bar.rs        # draw(), layout_tab_strip(), paint_active_indicator() — FR1, FR4, FR5
├── app.rs               # tab/window switch handlers, scroll-into-view flag — FR4
└── render/mod.rs        # TabBarItem view-model (flag threading if needed)
```

## Test Scenarios

### Unit Tests
- [ ] `visual_cell_count` / `build_visuals` unaffected by changes (existing tests still pass).
- [ ] FR5: with a non-active mux tab, the sub-tab indicator is not painted; with the mux tab
      active, the active window's sub-tab indicator is painted. (Assert via the test hooks
      `LAST_MUX_CELLS` / indicator state already used in `tab_bar.rs` tests.)
- [ ] FR4: given an off-screen active cell and the scroll-into-view flag set, the scroll
      offset moves the active cell into the visible range.

### Integration Tests
- [ ] Keyboard tab switch (`NextTab`/`PrevTab`/`JumpTab`) sets the scroll-into-view flag.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] N/A (no E2E infrastructure in this project)

### Edge Cases
- [ ] Tabs exactly fit the width (no overflow): no scroll area, no scrollbar, no scroll-into-view side effects.
- [ ] All cells are mux sub-tabs (no plain tabs): indicator gating and scroll-into-view still behave.
- [ ] Single tab: no overflow, indicator on the only tab.
- [ ] Mouse-driven scroll followed by an unrelated repaint: active tab is NOT force-scrolled back into view (the flag is set only by a keyboard tab-switch or new-tab creation, never by mouse scroll, an existing-tab mouse click, or unrelated repaints).

### Performance Tests
- [ ] No additional per-frame allocations introduced in the layout loop for FR4/FR5 gating.

## Security Considerations

- Not applicable (UI rendering change, no external input handling beyond existing wheel/key events).

## Error Handling

- Not applicable. No new error conditions; scroll-into-view is a best-effort no-op when the
  active cell rect is unavailable.

## Success Criteria

- [ ] All functional requirements (FR1–FR6) are implemented.
- [ ] All test scenarios pass.
- [ ] No regression in click switch, drag-reorder, mux sub-tab click, "+"/gear buttons.
- [ ] WebView tab bar is unchanged.

## Open Questions

> Note: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。

- None.

## References

- 要件定義書: `doc/tasks/tab-bar-scroll-and-active-indicator/要件定義書.md`
- Tab bar implementation: `src-tauri/src/ui/tab_bar.rs`
- Tab/window switch handlers: `src-tauri/src/app.rs`
- Mux window group: `src-tauri/src/mux/window_group.rs`
