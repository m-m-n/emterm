# Implementation Plan: mux Window Close Redraw

## Overview
Make a mux window's shell-exit close reconcile the screen with the now-active
window, by reusing the same snapshot-request redraw path that an explicit window
switch already performs.

## Objectives
- After a close changes the active window, render only the now-active window.
- Reuse the explicit-switch reconcile (request the now-active pane's snapshot);
  add no new redraw mechanism.
- Leave explicit switches, non-mux tabs, and the WebView frontend unchanged.

## Prerequisites

### Development Environment
- Rust toolchain as pinned by the repo (`rust-toolchain`), default `gui` feature.

### Dependencies
- No new external dependencies.
- Internal pieces already present and reused as-is:
  - `Tab::request_pane_snapshot` (sends `RequestPaneSnapshot` to the daemon).
  - `MuxWindowGroup::remove_pane` (removes window/pane/scroll, re-clamps active).
  - `MuxWindowGroup::active_pane_id` (active window's pane id).
  - The off-thread snapshot replay path that swaps the daemon's `Snapshot`
    reply into the displayed core.

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Framework**: native terminal stack (winit/wgpu/egui); mux client in `src-tauri/src/`
- **Key Libraries**: none added

### Design Approach
The defect is asymmetry between two inbound-message handlers in
`Tab::apply_mux_message`:

- The `SwitchWindow` arm syncs the active window **and** reconciles the screen by
  requesting the now-active pane's snapshot.
- The `PtyExited` arm removes the window and re-clamps the active index but does
  **not** reconcile the screen, so the closed window's grid stays displayed and
  the now-active window's live output overlaps it.

The fix brings the `PtyExited` arm to parity: when removing a window changes the
active window (and the group is not emptied), request the now-active pane's
snapshot so the existing replay path redraws it.

Active-window change is detected by comparing the active pane **id** captured
before removal with the active pane id after removal — robust to the index
re-clamp inside `remove_pane`, and naturally distinguishes "active window
closed", "earlier-indexed window closed (indices shift)", and "later-indexed
window closed (active unchanged)".

### Component Interaction
`daemon → PtyExited → Tab::apply_mux_message` (this fix) `→ request_pane_snapshot
→ daemon → Snapshot → off-thread replay → displayed core swap → render`.

## Implementation Phases

### Phase 1: Reconcile the screen on a close-induced active-window change

**Goal**: After `PtyExited` removes a mux window, if the active window changed to
a different still-existing window, the now-active window's content is requested
and replayed; if the active window is unchanged, or the group is emptied, no
snapshot is requested.

**Files to Modify**:
- `src-tauri/src/tabs.rs` — the `MessageType::PtyExited` arm of
  `Tab::apply_mux_message`; add unit tests alongside the existing mux tests.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| close-reconcile decision (testable helper) | Given the active pane id before removal and the group state after removal, decide which pane (if any) needs a screen reconcile | Removal has been applied to the group | Returns the now-active pane id when the active window changed and the group is non-empty; returns "none" when the active window is unchanged or the group is empty |
| `PtyExited` arm | Remove the window for the exited pane and wire the decision helper's result to the reconcile request | A `PtyExited` for a pane in this tab's group arrives | Removed window gone; if the helper returns a pane id, a snapshot for it was requested; if group emptied, tab marked `exited`; if active unchanged, no request |

**Testability note**: `request_pane_snapshot` writes to the PTY and is dropped
when a tab has no live PTY (the unit-test case), so it is not directly
observable — mirroring how existing `SwitchWindow` tests assert state rather than
the fire-and-forget send. Therefore the active-change decision is factored into
the helper above so unit tests assert its returned pane id (or "none") directly,
independent of a live PTY.

**Processing Flow** (diagram-convertible):
1. Receive `PtyExited` for a pane id.
2. If this tab has no mux group → no-op (existing behavior).
3. Record the active pane id (the "before" active).
4. Remove the window owning the exited pane.
   - Unknown pane id → no-op (no removal, no request).
   - Removal succeeded → continue.
5. If the group is now empty → mark the tab `exited`; do not request a snapshot.
6. Else read the active pane id again (the "after" active).
   - After differs from before → request the after-active pane's snapshot.
   - After equals before → request nothing (active window unchanged).

**Implementation Steps** (high level):
1. **Capture before-active** — read the active pane id prior to removal.
2. **Remove and branch on emptiness** — keep the existing empty-group → `exited`
   behavior unchanged and snapshot-free.
3. **Decide via helper** — for a non-empty group, the decision helper compares
   after-active to before-active and returns the pane id to reconcile, or "none".
4. **Reconcile** — when the helper returns a pane id, request that pane's
   snapshot (parity with the `SwitchWindow` reconcile).
5. **Unit-test the helper** — assert FR1/FR2/FR3 against the helper's return.

**Dependencies**: Requires the existing `request_pane_snapshot` and off-thread
replay path. Blocks nothing.

**Testing Approach**:
- Unit: active-window close triggers a snapshot request for the new active pane;
  non-active close triggers none; last-window close marks `exited` with no
  request; unknown pane id is a no-op.
- Manual: 3-window mux tab, exit the active window's shell, confirm the
  now-active window shows only its own content with no overlap and without a
  manual switch.

**Acceptance Criteria**:
- [ ] FR1: active-window close requests the new active pane's snapshot.
- [ ] FR2: non-active close requests no snapshot, display unchanged.
- [ ] FR3: last-window close marks `exited`, requests no snapshot.

**Estimated Effort**: small

---

## Complete File Structure
```
doc/tasks/mux-window-close-redraw/
├── 要件定義書.md
├── SPEC.md
├── IMPLEMENTATION.md
├── VERIFICATION.md
├── sdd.yaml
└── tasks.yaml
src-tauri/src/
└── tabs.rs            # PtyExited arm fix + unit tests
```

## Testing Strategy
- Unit: cover FR1/FR2/FR3 and the unknown-pane no-op in `tabs.rs` tests, using
  the existing test helpers that seed a mux group with multiple windows.
- Manual: human-judged visual confirmation of no overlap after a shell-exit
  close (no E2E framework in this project).

## Dependencies
| Package | Version | Purpose |
|---------|---------|---------|
| (none)  | -       | No new dependencies |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Requesting a snapshot when the active window did not actually change (extra redraw) | Low | Low | Gate the request on a before/after active-pane-id difference |
| Redundant snapshot requests when several `PtyExited` drain in one pump | Low | Low | Off-thread replay supersedes prior pending switches; the final active window wins |
| Test helpers cannot drive the snapshot side effect | Low | Medium | Assert via an observable signal (e.g. the control message queued by `request_pane_snapshot` / a recorded request), mirroring existing `SwitchWindow` tests |

## Open Questions
- [ ] Per-pane scroll-position reload on close is out of scope: the user reported
  only content overlap, the closed window's scroll entry is dropped by
  `remove_pane`, and reconciling content is sufficient. Revisit only if a
  scroll-position discrepancy is observed after this fix.

## Success Metrics
- [ ] FR1–FR3 implemented and unit-tested.
- [ ] No overlap in the manual scenario.
- [ ] Existing mux tests pass; CLI-only build still compiles.
