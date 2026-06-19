# Feature: mux Window Close Redraw

## Overview

When a mux window's shell exits (`exit` / Ctrl+D), that window is removed and a
different window becomes active. Currently the display keeps the closed window's
grid in the active `TerminalCore`, and the now-active window's live output is
painted on top of it, so the two windows' contents overlap on screen. This
feature makes the close-induced active-window change reconcile the screen with
the now-active window — the same way an explicit window switch already does.

## Objectives

- After a shell-exit close changes the active window, render only the
  now-active window's content.
- Reuse the existing explicit-switch redraw path (request the now-active pane's
  snapshot and replay it) rather than introducing a separate mechanism.
- Do not change the behavior of explicit window switches or non-mux tabs.

## User Stories

### US1: Closing a mux window shows the next window cleanly
As a terminal user running several mux windows, I want the screen to show only
the newly-active window after I exit a shell, so that I am not confused by
leftover content from the window I just closed.

**Acceptance Criteria:**
- [ ] Exiting the shell in the active window of a 2+ window mux tab makes a
      different window active and the screen shows only that window's content.
- [ ] No content from the closed window remains overlaid on the new active
      window's content.
- [ ] The corrected content appears without the user manually switching away
      and back.

### US2: Closing a non-active window does not disturb the view
As a terminal user, I want exiting a shell in a window I am not currently
viewing to leave my visible window untouched.

**Acceptance Criteria:**
- [ ] When a non-active window's shell exits, the active window stays active and
      its on-screen content is unchanged.

### US3: Closing the last window still closes the tab
As a terminal user, I want exiting the last window's shell to close the whole
tab, preserving today's behavior.

**Acceptance Criteria:**
- [ ] When the closed window was the last one in the tab, the tab is closed
      (reaped by `App::pump_all`) and no redraw is requested.

## Technical Requirements

### Functional Requirements

- **FR1:** When a `PtyExited` message removes a mux window and, as a result, the
  active window changes to a different (still-existing) window, the client must
  reconcile the screen with the now-active window — i.e. request that window's
  pane snapshot so it is replayed into the displayed core, identical to the
  reconcile performed on an inbound `SwitchWindow`.
- **FR2:** When the removed window was *not* the active window (the active
  window is unchanged after removal), no snapshot request is issued and the
  displayed content is left as-is.
- **FR3:** When removing the window empties the group (last window), the tab is
  marked `exited` (existing behavior) and no snapshot request is issued.

### Non-Functional Requirements

- **NFR1 - Compatibility:** The explicit window-switch redraw path and non-mux
  tab behavior are unchanged.
- **NFR2 - Build:** The change lives under `feature = "gui"` and must not break
  the CLI-only build (`--no-default-features`).
- **NFR3 - Scope:** Only the native (`src-tauri/`) implementation is changed;
  the WebView frontend (`src/`) is not touched.

## Implementation Approach

### Architecture

The fix is localized to the mux client message handling in
`src-tauri/src/tabs.rs`.

**Relevant existing pieces:**

- `Tab::apply_mux_message` — handles inbound mux messages.
  - `MessageType::SwitchWindow` arm (`tabs.rs:1135`): on an explicit/daemon
    switch, after syncing the active index it calls
    `self.request_pane_snapshot(msg.pane_id)` to reconcile the screen with the
    now-active window. **This is the redraw path the close case must reuse.**
  - `MessageType::PtyExited` arm (`tabs.rs:1218`): removes the window via
    `MuxWindowGroup::remove_pane` and, when the group becomes empty, sets
    `self.exited = true`. **It does not currently request a snapshot for the new
    active window — this is the defect.**
- `MuxWindowGroup::remove_pane` (`window_group.rs:229`): removes the
  window/pane/scroll entry and re-clamps `active` into `[0, len-1]`. Returns the
  removed index.
- `MuxWindowGroup::active_pane_id` — the pane id of the currently active window.
- `Tab::request_pane_snapshot` (`tabs.rs:1942`): sends `RequestPaneSnapshot` to
  the daemon for a pane id; the daemon replies with a `Snapshot`, which the
  existing off-thread replay path swaps into the displayed core.

### Data Flow

```
shell exits
  → daemon detects pane EOF → broadcasts PtyExited(pane_id)
  → Tab::apply_mux_message(PtyExited)
       capture active_pane_id BEFORE removal
       MuxWindowGroup::remove_pane(pane_id)   # active index re-clamped
       if group now empty → exited = true (close tab; no snapshot)
       else compare active_pane_id AFTER removal
            if it differs from before → request_pane_snapshot(new active pane)
  → daemon replies Snapshot(new active pane)
  → off-thread replay swaps the now-active window's grid into the displayed core
  → screen shows only the now-active window
```

### Behavioral Note: when does the active window change on removal?

`remove_pane` keeps `active` in range by clamping. The active window changes as
a result of removal when either:
- the removed window *was* the active one, or
- the removed window's index was below the active index (indices shift down).

Rather than reason about indices, the implementation compares the active pane
id captured *before* removal with the active pane id *after* removal; a snapshot
is requested only when they differ and the group is non-empty. This naturally
covers FR1/FR2/FR3 and is robust to the index re-clamping.

### File Structure

```
src-tauri/src/
└── tabs.rs        # PtyExited arm of Tab::apply_mux_message (the fix)
                   # + unit tests for the close-then-reconcile behavior
```

## Test Scenarios

### Unit Tests
- [ ] Closing the active window in a 3-window group changes the active pane id
      and triggers a snapshot request for the new active pane.
- [ ] Closing a non-active window leaves the active pane id unchanged and issues
      no snapshot request.
- [ ] Closing the last remaining window sets `exited = true` and issues no
      snapshot request.
- [ ] Closing a window with an unknown pane id is a no-op (no removal, no
      snapshot request).

### Integration / Manual Tests
- [ ] In a running build: open a mux tab with 3 windows, run distinguishable
      output in each, exit the shell in the active window, and confirm the
      newly-active window shows only its own content (no overlap) without a
      manual switch.

### Edge Cases
- [ ] Group becomes empty (last window) → tab closes, no snapshot request,
      `mux kill` is not blocked (preserves the existing reaping behavior).
- [ ] Several `PtyExited` for distinct panes drain within one `pump` → the final
      active window is reconciled correctly.

### Regression
- [ ] Explicit window switch (inbound `SwitchWindow`) still reconciles and draws
      the correct window.
- [ ] Non-mux tabs and the WebView frontend are unaffected.

## Success Criteria

- [ ] All functional requirements (FR1–FR3) are implemented and unit-tested.
- [ ] The manual scenario shows no overlap after a shell-exit close.
- [ ] Existing mux tests pass (no regression in switch / close-tab behavior).
- [ ] CLI-only build (`--no-default-features`) still compiles.

## References

- 要件定義書: `doc/tasks/mux-window-close-redraw/要件定義書.md`
- Explicit-switch reconcile precedent: `src-tauri/src/tabs.rs` `SwitchWindow` arm (`request_pane_snapshot`)
- Window removal/clamp: `src-tauri/src/mux/window_group.rs` `remove_pane`
