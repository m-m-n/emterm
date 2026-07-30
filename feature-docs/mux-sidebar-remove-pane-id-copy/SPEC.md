# Feature: Remove the pane-ID copy button from the mux sidebar

## Overview

The mux sidebar's pane rows currently render a copy-to-clipboard icon at the
row's right edge whenever the GUI knows that pane's daemon-minted public pane
ID (added by task0006 AC-5). This feature removes that icon together with its
click hit target and the clipboard-write path that exists solely to serve it.
Everything else the sidebar draws — the agent-status badge, the number/name
layout, the row colors, both placement variants — stays exactly as it is.

## Objectives

- No copy affordance renders in a sidebar pane row.
- No copy hit target exists in a sidebar pane row; the whole row body remains
  the window-switch target.
- No code remains whose only purpose was that button.

## User Stories

### US1: A pane row shows no copy affordance

As an eMterm user, I want the sidebar pane row to show only the window number,
the agent-status badge and the pane name, so that the row carries no unneeded
control.

**Acceptance Criteria:**
- [ ] A row whose pane has a known public pane ID paints no copy icon.
- [ ] The row's name text may use the width up to the row's own right padding
      (no reserved icon region).

### US2: Clicking anywhere in a row switches windows

As an eMterm user, I want a click anywhere in the row — including the area the
icon used to occupy — to switch to that window, so that the row behaves as one
target.

**Acceptance Criteria:**
- [ ] A click at the row's right edge yields `switch_to_window`.
- [ ] No frame can produce a clipboard-copy request originating from the
      sidebar.

## Technical Requirements

### Functional Requirements

- **FR1:** `ui::mux_sidebar::draw_rows` renders no copy-to-clipboard icon and
  no hover text for one, for every entry regardless of what the entry knows
  about its pane's public ID.
- **FR2:** `ui::mux_sidebar` registers no additional `ui.interact` hit region
  inside a row. A click in the row (including its right edge) sets
  `SidebarOutcome::switch_to_window` and nothing else.
- **FR3:** The sidebar-originated clipboard path is removed end to end:
  `SidebarOutcome::copy_pane_id`, `render::draw_terminal`'s
  `clipboard_copy_request` collection, `render::FrameEvents::clipboard_copy`
  (including its term in `FrameEvents::any()`), and the
  `window_host` block that applies it via `set_clipboard`.
- **FR4:** The support code that existed only for the icon is removed:
  `COPY_ICON_SIZE`, `COPY_ICON_GAP`, `copy_icon_rect`, `paint_copy_icon`,
  `SidebarEntry::public_pane_id`, the `public_pane_id` attachment in
  `render::draw_terminal`, and the `locale` parameter of
  `mux_sidebar::draw` / `draw_persistent` / `draw_overlay` / `draw_rows`
  (the icon's hover text was its only consumer).

### Non-Functional Requirements

- **NFR1 - Behavior preservation:** Everything the sidebar draws other than
  the icon is unchanged: row height / gap / corner radius, the number column,
  the agent-status badge (presence, diameter, gap, ring), the row color pairs
  (active / hovered / plain), the separator rule, the overlay card geometry,
  `sidebar_width`, `point_in_sidebar`, `top_chrome_inset`, and `build_entries`
  ordering / numbering / active marking.
- **NFR2 - Test integrity:** The task0006 AC-5 copy-icon tests are removed
  (they pin behavior that no longer exists) and replaced by regression tests
  for FR1/FR2. Every other `ui::mux_sidebar` test keeps asserting what it
  asserts today. The full unit-test suite passes.
- **NFR3 - No dead code:** `cargo check` produces no new warnings — in
  particular no unused constant / function / field / parameter left behind by
  the removal.
- **NFR4 - Unrelated state preserved:** `App::mux_public_pane_ids` and
  `App::mux_public_pane_id()` remain, with their existing tests, because the
  map still keys agent-notification rate limiting.
  `WindowHost::set_clipboard` remains — the selection-copy keybinds use it.
- **NFR5 - CLI build:** The CLI-only build (`--no-default-features`) is
  unaffected; all touched code is behind the `gui` feature.

## Implementation Approach

### Affected files

```
src-tauri/src/ui/mux_sidebar.rs   # icon paint / hit test / helpers /
                                  #   constants / entry+outcome fields /
                                  #   locale parameter / tests
src-tauri/src/render/mod.rs       # public_pane_id attachment,
                                  #   clipboard_copy_request collection,
                                  #   FrameEvents::clipboard_copy + any()
src-tauri/src/window_host.rs      # frame_events.clipboard_copy application
```

### Data flow, before and after

Before:

```
daemon AgentStatusUpdate → App::mux_public_pane_ids
  → render::draw_terminal attaches SidebarEntry::public_pane_id
  → mux_sidebar::draw_rows paints the icon, hit-tests it,
    returns SidebarOutcome::copy_pane_id
  → FrameEvents::clipboard_copy
  → window_host: WindowHost::set_clipboard
```

After:

```
daemon AgentStatusUpdate → App::mux_public_pane_ids
  → (notification rate-limit keys only)

mux_sidebar::draw_rows → SidebarOutcome::switch_to_window → TabEvent::MuxSwitch
```

### Dependencies

**Internal:** `ui::md3` (row colors — unchanged), `App::agent_status_pane_badge`
(badge attachment — unchanged), `i18n::Locale` (no longer needed by this
module).

**External:** none.

## Test Scenarios

### Unit Tests

- [ ] `draw` over an entry with a public pane ID known: the row produces no
      interact region beyond the row itself, and `SidebarOutcome` has only
      `switch_to_window`.
- [ ] Clicking at the row's right edge (the former icon rect's center) yields
      `switch_to_window == Some(index)`.
- [ ] Row layout with and without a badge is unchanged (number position,
      badge position, name origin), for both `Placement::Persistent` and
      `Placement::Overlay`.
- [ ] `build_entries` still yields order / numbering / active marking as
      before.
- [ ] `FrameEvents::any()` reports `false` for a default `FrameEvents` and
      `true` for each remaining event field.

### Integration Tests

- [ ] `cargo test` (whole crate, `--lib`) passes with no ignored or removed
      coverage beyond the copy-icon tests.

### E2E Tests

**Existing E2E tests**: None detected.
**Run command**: Not detected.

### Edge Cases

- [ ] An entry whose `badge` is `None` and whose pane has no public ID:
      renders number + name only (unchanged from today).
- [ ] An empty entry list: the panel draws with no rows and no placeholder
      (unchanged).
- [ ] A very narrow sidebar at `MIN_WIDTH`: the name ellipsizes against the
      row's right padding without an icon region.

### Performance Tests

Not applicable — the change only removes per-row painting and one hit test.

## Security Considerations

- **Data Protection:** One clipboard-write path is removed. The remaining
  clipboard writes (selection copy) are untouched, so no new data reaches the
  clipboard and one automated write disappears.

## Error Handling

Not applicable — no new failure modes; the removed path had none.

## Success Criteria

- [ ] All functional requirements (FR1–FR4) are implemented.
- [ ] All non-functional requirements (NFR1–NFR5) hold.
- [ ] All test scenarios pass.
- [ ] `cargo check` and `cargo test` are clean.

## Assumptions

Recorded per batch mode: these points were not confirmed with the user. The
Codex consultation loop was skipped because the `codex` CLI is not installed
on this host, so each was decided from the task description plus the code.

- **A1 (scope of removal):** The task asks for the button, its click
  detection and its clipboard-copy processing to be removed. Since the sidebar
  icon is the *only* producer of `FrameEvents::clipboard_copy`, that field and
  its `window_host` application block are removed as well rather than left as
  permanently-dead plumbing.
- **A2 (`locale` parameter):** `mux_sidebar`'s `locale` argument existed only
  to localize the icon's hover text, so it is removed from the module's
  function signatures and from both `render/mod.rs` call sites. Keeping an
  unused parameter would trip NFR3.
- **A3 (`SidebarEntry::public_pane_id`):** Removed, since the icon was its
  only reader. `App::mux_public_pane_id()` and the map behind it are kept
  (NFR4) — they are still used for notification rate-limit keys, and their
  tests cover daemon public-ID learning, which is out of scope here.
- **A4 (`WindowHost::set_clipboard`):** Kept. The selection-copy keybinds
  (Ctrl+Shift+C / mouse-up) use it.
- **A5 (tests):** The task0006 AC-5 test block in `ui::mux_sidebar::tests` is
  deleted rather than adapted, and replaced with regression tests asserting
  the icon and its hit target are gone (FR1/FR2).
- **A6 (format command):** `rustfmt` is not installed on this host, so
  `project.components.main.format_command` is left empty and formatting is not
  run as a workflow command.

## References

- Requirements document: `feature-docs/mux-sidebar-remove-pane-id-copy/REQUIREMENTS.md`
- Notion task: [https://www.notion.so/3a93509ec8ee8135abdcfe9498fa7b24](https://www.notion.so/3a93509ec8ee8135abdcfe9498fa7b24)
