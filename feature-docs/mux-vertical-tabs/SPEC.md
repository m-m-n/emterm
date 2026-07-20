# Feature: mux Vertical Tabs (Sidebar Window List)

## Overview

Consolidate the entire mux into a single slot on the top tab bar and present the
mux window list as a vertical tab sidebar rendered natively in egui. The sidebar
supports two placement modes — persistent (default) and right-side overlay —
switchable from the settings panel.

## Objectives

- Scale to many mux windows without top-tab title crushing
- Keep local tabs and mux windows in separate namespaces (clear hierarchy)
- Avoid PTY resizes on sidebar interaction (overlay toggle, window switch)

## User Stories

### US1: Switch windows from the vertical tab list
As a mux user, I want to click a window entry in the sidebar, so that I can
switch to that window quickly even when many windows are open.

**Acceptance Criteria:**
- [ ] Clicking a window entry switches to that window (same behavior as
      today's top-tab click window switch)
- [ ] The active mark follows the newly active window

### US2: Toggle the sidebar as an overlay
As a mux user who wants full terminal width, I want the sidebar as a
keybind-toggled right overlay, so that columns are not permanently consumed.

**Acceptance Criteria:**
- [ ] `Ctrl+Z Ctrl+W` opens the overlay on the right edge; pressing it again
      closes it (toggle is the only close path)
- [ ] Opening/closing the overlay causes no PTY resize

## Technical Requirements

### Functional Requirements

- **FR1:** Top-tab consolidation — while attached to mux, mux occupies exactly
  one top tab; mux windows are no longer expanded into individual top tabs.
  The mux tab title is `mux: <active window name>`, tracking the active
  window's current name (including OSC title rewrites).
- **FR2:** Vertical tab list — the sidebar shows a flat list of mux windows
  (no nesting). Each entry shows the window number, window name, and an
  active mark on the currently active window. No bell/activity indicators.
  The list is shown only while the mux-attached top tab is active; local
  tabs never show it. Sidebar width is a dynamically computed fixed value of
  roughly 20–25% of the app width (no user setting, no drag resize). When
  entries exceed the available height, the list scrolls.
- **FR3:** Click-to-switch — clicking a window entry switches the active mux
  window, equivalent to the current top-tab click switch. Identical in both
  placement modes.
- **FR4:** Persistent mode (default) — the sidebar is a fixed panel on the
  RIGHT edge of the terminal area (same side as the overlay); it is
  never opened/closed at runtime within the mux tab. Because all tabs share
  one terminal grid, switching the active top tab between a mux-attached tab
  and a local tab changes the effective viewport width and triggers a PTY
  resize (accepted behavior; see NFR1). The overlay toggle keybind is a
  no-op in this mode.
- **FR5:** Overlay mode — the sidebar overlays the right side of the terminal
  surface (terminal cells keep their size; the right edge is visually
  covered while open). Toggled by a new mux prefix action, default
  `Ctrl+Z Ctrl+W`, registered in the `DEFAULT_ACTION_BINDINGS` SSOT table so
  it is user-rebindable via `settings.mux.keybinds` and exposed through the
  existing `get_mux_action_defaults` IPC. Toggling causes no PTY resize.
- **FR6:** Placement setting — a boolean setting "オーバーレイで表示"
  (display as overlay), default `false`, following the existing Settings
  pattern: Rust `app_settings` schema + serde defaults, TypeScript
  `AppSettings` mirror, settings UI section rendering, and ja/en locale
  strings. Switching the setting may trigger exactly one PTY resize
  (persistent ⇔ overlay changes the terminal viewport width).

### Non-Functional Requirements

- **NFR1 - Resize discipline:** Mux window switching and overlay open/close
  must not trigger PTY resizes. Accepted resizes are: (a) the single one
  caused by changing the placement setting, and (b) top-tab switches between
  a mux-attached tab and a local tab in persistent mode (all tabs share one
  grid, so the sidebar inset applies to the shared viewport width).
- **NFR2 - Compatibility:** Local (non-mux) tab behavior is unchanged.
  Existing mux prefix follow-ups (Ctrl+D / Ctrl+C / Ctrl+N / Ctrl+P /
  Ctrl+R / Ctrl+T) are unchanged; `Ctrl+W` is currently unused so there is
  no conflict.
- **NFR3 - Design consistency:** The sidebar is rendered natively in egui and
  follows the visual decisions fixed in DESIGN.md, consistent with the MD3
  tokens in `doc/UI-DESIGN-GUIDELINES.yaml`.

## Implementation Approach

### Architecture

The vertical tab component is a single shared egui component; only the
placement strategy differs:

```
┌──────────────────────────────────────────────┐
│ Top tab bar:  [local tab] [mux: <win name>]  │
├──────────────────────────────┬───────────────┤
│                              │ Vertical tabs │
│      terminal surface        │  1 shell   ●  │
│      (wgpu render)           │  2 editor     │
│                              │  3 logs       │
└──────────────────────────────┴───────────────┘
 persistent mode: fixed RIGHT side panel (terminal viewport excludes sidebar
 width; grid origin stays at the left edge)

┌──────────────────────────────────────────────┐
│ Top tab bar:  [local tab] [mux: <win name>]  │
├──────────────────────────────┬───────────────┤
│                              │ Vertical tabs │
│      terminal surface        │  1 shell   ●  │
│  (full width; right edge     │  2 editor     │
│   visually covered)          │  3 logs       │
└──────────────────────────────┴───────────────┘
 overlay mode: drawn over the terminal on the RIGHT edge, toggled by keybind
```

### Data Flow

- Window list source: the GUI already receives mux window metadata
  (id, name, active state) from the daemon (`SessionInfo.windows` /
  window lifecycle messages). The sidebar renders from this existing
  client-side state; no protocol changes are required.
- Window switch: sidebar click invokes the same switch path as the current
  top-tab click for mux windows.
- Overlay toggle: new mux action dispatched through the existing prefix-key
  pipeline (`src-tauri/src/mux/prefix.rs`), consuming `Ctrl+W` after the
  `Ctrl+Z` prefix. In persistent mode the action handler does nothing.

### Settings

| Layer | Change |
|-------|--------|
| `crates/app_settings` | new boolean field under the mux settings group, default `false`, serde default fn |
| `src-tauri/src/settings.rs` (+ store/commands) | plumb the field through GUI runtime settings |
| `src-tauri/web-shared/settings/types.ts` | mirror field in `AppSettings` |
| settings UI section (mux) | toggle control 「オーバーレイで表示」 |
| `src-tauri/web-shared/i18n/locales/{ja,en}.json` | label strings |

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/mux/prefix.rs`: prefix-key action table (`DEFAULT_ACTION_BINDINGS`)
- mux client window state (source of the window list; already present)
- Top tab bar rendering (`src-tauri/src/tabs` / `src-tauri/src/ui`)
- Settings pipeline (app_settings + TS mirror + settings UI)

**External Dependencies:**
- None (egui native rendering; no new crates expected)

## Test Scenarios

### Unit Tests
- [ ] TS-1: Top-tab consolidation — attached mux yields one tab entry titled
      `mux: <active window name>`; renaming the active window (OSC title)
      updates the tab title
- [ ] TS-2: Sidebar list model — entries carry number + name + active flag,
      flat, ordered by window id; 0-window and 1-window lists render without
      error
- [ ] TS-3: Click switch — activating entry N triggers the same window-switch
      call as the top-tab path
- [ ] TS-4: Overlay toggle action — `Ctrl+Z Ctrl+W` maps to the new action in
      `DEFAULT_ACTION_BINDINGS`; action toggles overlay state in overlay
      mode and is a no-op in persistent mode
- [ ] TS-5: Settings field — default `false`; serde null/missing handling per
      the existing pattern; TS mirror type-checks
- [ ] TS-6: Resize discipline — overlay open/close and window switch produce
      no PTY resize call; placement-setting change produces exactly one
- [ ] TS-7: Sidebar visibility — sidebar state is present only when the
      active top tab is mux-attached

### Integration Tests
- [ ] TS-8: Full settings round-trip — toggling 「オーバーレイで表示」 in the
      settings panel persists to settings.json and switches placement mode

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual scenario M-1: attach mux with 3+ windows → sidebar lists all
      windows; click switches; top tab shows `mux: <name>`
- [ ] Manual scenario M-2: overlay mode → `Ctrl+Z Ctrl+W` toggles; TUI app
      (e.g. Claude Code) does not reflow on toggle
- [ ] Manual scenario M-3: persistent mode → `Ctrl+Z Ctrl+W` does nothing;
      local tabs unaffected

### Edge Cases
- [ ] Long window names: truncated with ellipsis within the sidebar width
- [ ] Many windows: sidebar scrolls; active window stays reachable
- [ ] Window closed while sidebar open: entry disappears; active mark moves
      with the daemon's new active window
- [ ] Detach while overlay open: overlay state resets with the mux tab

## Security Considerations

- **Input Validation:** window names come from the daemon (OSC titles) and are
  rendered as plain egui text — no markup interpretation, no injection surface.
- No authentication/authorization/network surface changes.

## Error Handling

- Empty window list (transient during attach/detach): render an empty sidebar;
  no panic, no fallback UI required.
- Click on a window that no longer exists (race with daemon close): the switch
  request is ignored by the existing switch path; sidebar re-renders from the
  next state update.

## Success Criteria

- [ ] All functional requirements (FR1–FR6) are implemented and tested
- [ ] All test scenarios pass
- [ ] NFR1 resize discipline verified (no resize on toggle/switch)
- [ ] Local tab behavior unchanged (NFR2)
- [ ] Sidebar visuals match DESIGN.md (NFR3)

## Open Questions

- None (no `tbd` requirements).

## References

- Discussion report: `tmp/discussion-mux-vertical-tabs.md`
- Requirements (Japanese): `feature-docs/mux-vertical-tabs/REQUIREMENTS.md`
- mux prefix keybinds: `src-tauri/src/mux/prefix.rs`
- MD3 tokens: `doc/UI-DESIGN-GUIDELINES.yaml`
