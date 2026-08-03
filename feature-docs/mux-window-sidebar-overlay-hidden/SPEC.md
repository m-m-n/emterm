# Feature: mux-window-sidebar-overlay-hidden

## Overview

The mux window sidebar in overlay mode (`settings.mux.window_sidebar_overlay: true`) is not visible right after emterm launches and attaches to a mux session, so the user must toggle it with prefix + Ctrl+S on every launch. This feature adds a single state assignment in the pump logic so that the overlay sidebar opens on the mux attach transition, restoring the "default open" guarantee of AC-7. The change is confined to `src-tauri/src/app.rs`.

## Objectives

- Make the overlay-mode mux window sidebar visible immediately after launching emterm and attaching to a mux session.
- Restore the "default open" guarantee of AC-7 without requiring a manual prefix + Ctrl+S toggle on every launch.

## User Stories

### US1: Sidebar visible from startup
As an emterm user with `window_sidebar_overlay: true`, I want the overlay sidebar to be visible as soon as emterm attaches to my mux session, so that I do not have to toggle it manually on every launch.

**Acceptance Criteria:**
- [ ] Launching emterm via init-mux and attaching to mux with `window_sidebar_overlay: true` shows the sidebar from startup.
- [ ] Immediately after `src-tauri/src/app.rs:3922-3929`, in a pump where `active_mux_attached_prev_pump` transitioned None → Some, `self.mux_sidebar_overlay_open = true` is assigned.

### US2: Reattach restores the open state
As an emterm user, I want a reattach to return the overlay sidebar to open, so that the attach transition behaves consistently regardless of whether I closed the sidebar earlier in the session.

**Acceptance Criteria:**
- [ ] After an explicit Ctrl+S close, detach → reattach returns the sidebar to open.

## Technical Requirements

### Functional Requirements

- **FR1 - Re-open overlay sidebar on mux attach transition:** In the pump logic immediately after the existing detach guard at `src-tauri/src/app.rs:3922-3929`, when `active_mux_attached_prev_pump` transitions from None to Some (the active tab's `mux_group` goes from absent to present), assign `self.mux_sidebar_overlay_open = true`. (status: resolved)
- **FR2 - Sidebar visible from startup under init-mux:** When emterm is launched as a new process (e.g. via `~/bin/init-mux`) and attaches to a mux session with `window_sidebar_overlay: true`, the floating overlay sidebar card is displayed from the moment attach completes, with no user toggle required. (status: resolved)
- **FR3 - Reattach restores open state:** After the user explicitly closes the overlay sidebar with prefix + Ctrl+S, a detach followed by a reattach returns the sidebar to open. This is the accepted, intended side effect of the None→Some transition rule (the task's 案2). (status: resolved)

### Non-Functional Requirements

- **NFR1 - Change confinement:** The fix is confined to the pump logic in `src-tauri/src/app.rs`. `src-tauri/src/tabs.rs` (mux protocol handler) and the mux daemon/bridge are not modified. (status: resolved)
- **NFR2 - No settings-schema change:** `mux_sidebar_overlay_open` remains a runtime-only flag (initialized `true` at `src-tauri/src/app.rs:921`); it is not persisted to settings, and the `window_sidebar_overlay` setting value and persistent-mode interaction are unchanged. (status: resolved)

## Implementation Approach

### Architecture

**System Architecture:**

```
┌─────────────────────────────────────────────────────┐
│ app.rs — pump loop                                  │
│   active tab's mux_group: Option<..>                │
│   active_mux_attached_prev_pump: Option<..>         │
│                                                     │
│   Some → None : existing detach guard (unchanged)   │
│   None → Some : mux_sidebar_overlay_open = true     │  ← FR1
│                                                     │
│   runtime flag: mux_sidebar_overlay_open (bool)     │
├─────────────────────────────────────────────────────┤
│ overlay sidebar rendering (unchanged appearance)    │
└─────────────────────────────────────────────────────┘
```

**Component Diagram:**

```
app.rs pump logic  ──reads──>  active tab mux_group
                   ──writes─>  self.mux_sidebar_overlay_open
tabs.rs (mux protocol handler)  — not modified (NFR1)
mux daemon / bridge             — not modified (NFR1)
settings schema                 — not modified (NFR2)
```

### Data Flow

```
pump tick → observe active tab's mux_group
          → compare with active_mux_attached_prev_pump
          → Some→None : existing detach guard (app.rs:3922-3929)
          → None→Some : self.mux_sidebar_overlay_open = true
          → overlay sidebar renders open
```

### API Design

Not applicable — no API surface is added or changed.

### Database Schema

Not applicable — no persisted data. `mux_sidebar_overlay_open` is a runtime-only flag (NFR2).

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/app.rs` pump logic and the existing detach guard at lines 3922-3929: the new assignment sits immediately after it.
- `mux_sidebar_overlay_open` runtime flag (initialized `true` at `src-tauri/src/app.rs:921`).
- The `window_sidebar_overlay` setting (`settings.mux.window_sidebar_overlay`): read-only for this feature; its value and its persistent-mode interaction are unchanged.

**External Dependencies:**

- None.

### File Structure

```
src-tauri/src/
└── app.rs                # pump logic: None→Some assignment (FR1)
                          # inline #[cfg(test)] unit test (TS1)
```

## Test Scenarios

### Unit Tests

- [ ] TS1 (FR1, FR2): Inline `#[cfg(test)]` test in `src-tauri/src/app.rs`, per project convention — simulate a pump sequence where the active tab's `mux_group` goes None → Some and assert `mux_sidebar_overlay_open == true`.

### Integration Tests

- [ ] TS2 (FR1, NFR1): Regression — run the existing `ac7_*` tests covering the detach guard (Some → None still sets the flag false).
- [ ] TS3 (NFR1): Regression — full library suite `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` (note: `tabs.rs` replay tests may need `-- --test-threads=1` if they flake).

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] TS4 (FR2, FR3): Manual verification by the user — launch via `~/bin/init-mux` with `window_sidebar_overlay: true` and confirm the overlay sidebar is visible immediately; then Ctrl+S close, detach, reattach, confirm it reopens.

### Edge Cases

- [ ] Explicit user close followed by reattach: the sidebar reopens. This is accepted behavior per FR3, not a defect.

### Performance Tests

Not applicable.

## Security Considerations

Not applicable — the change is a single runtime boolean assignment in the pump loop; no authentication, authorization, input handling, or data protection surface is involved.

## Error Handling

Not applicable — the change introduces no new failure mode or error code.

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] Immediately after `src-tauri/src/app.rs:3922-3929`, in a pump where `active_mux_attached_prev_pump` transitioned None → Some, `self.mux_sidebar_overlay_open = true` is assigned.
- [ ] Launching emterm via init-mux and attaching to mux with `window_sidebar_overlay: true` shows the sidebar from startup.
- [ ] After an explicit Ctrl+S close, detach → reattach returns the sidebar to open.
- [ ] A new test asserts `mux_sidebar_overlay_open == true` after launch → mux attach completion.
- [ ] Existing tests for the 3927 detach guard (the `ac7_*` test group and related) pass without regression.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. All requirements (FR1-FR3, NFR1-NFR2) are `status: resolved`.

## Assumptions

- Code-location claims in the task description (init `true` at `app.rs:921`, user toggle at `app.rs:3100`, detach guard at `app.rs:3927`, transient-None sources at `tabs.rs:2285` / `tabs.rs:2233`) originate from the task description and were not independently verified in this dispatch; the implementation planner should confirm exact line positions, which may have drifted.
- Root-causing why the 3927 guard fires during the startup sequence (spurious Detached delivery vs. initialization ordering) is explicitly out of scope; a separate task will pursue it.
- Always-open is acceptable UX because the inactive overlay renders at `OVERLAY_IDLE_OPACITY = 0.35` (`app.rs:76`), consistent with the existing design intent.
- Reattach forcing the sidebar open even after an explicit user close is accepted behavior, stated in the task's acceptance criteria.

## Implementation Phases (if applicable)

Not applicable — single-step change (one assignment plus tests).

## References

- Requirements document (Japanese): `feature-docs/mux-window-sidebar-overlay-hidden/REQUIREMENTS.md`
- `src-tauri/src/app.rs` — pump logic, detach guard, `mux_sidebar_overlay_open`, `OVERLAY_IDLE_OPACITY`
