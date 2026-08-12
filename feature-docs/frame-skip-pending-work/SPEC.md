# Feature: frame-skip-pending-work

## Overview

The frame-skip gate's `overlay_work` predicate currently depends on
`App::toast_pending()`, which only sees toasts that already exist. This feature adds
pending work that precedes toast creation — undrained SFTP channel events and the
restart-required flag — to that predicate, so an idle tab no longer delays the first
toast. Requirement details are in
[REQUIREMENTS.md](REQUIREMENTS.md).

## Objectives

- Include toast-creation-preceding pending work (undrained SFTP channel events, the
  restart-required flag) in the frame-skip gate's `overlay_work` predicate so first toast
  display is not delayed on idle tabs.
- Close the self-lock reported by em-review (2026-08-11, finding `b5f2cce1822ab271`,
  resolution: deferred): `App::toast_pending()` only sees already-created toasts, yet
  `overlay_work`, the event-loop redraw pacing, and `next_toast_deadline()` all depend on
  it, so a pending SFTP progress event or restart flag can never create its first toast on
  an idle frame.

## User Stories

### US1: Progress toast on an idle tab

As an eMterm user, I want the SFTP upload progress toast to appear on an idle tab, so that
I can see upload progress without touching the window first.

**Acceptance Criteria:**

- [ ] AC1: On an idle tab (cursor blink disabled or window unfocused), starting an SFTP
      upload shows the progress toast promptly after the first progress event arrives.

### US2: Restart toast while idle

As an eMterm user, I want the restart toast to arm and display while the app is idle, so
that I learn about a failed self-spawn after a binary swap.

**Acceptance Criteria:**

- [ ] AC2: After a binary swap causes a self-spawn failure, the restart toast arms and
      displays promptly even while the app is idle (Phase 7 E-1).

## Technical Requirements

### Functional Requirements

- **FR1 — Non-consuming restart peek in self_exec:** Add a non-consuming peek
  `restart_pending()` to `src-tauri/src/self_exec.rs` that reads `RESTART_REQUIRED` via
  `load()` without resetting it. The existing swap-consuming `restart_required()` keeps its
  consume semantics unchanged (constraint from the task description; matches Phase 7 batch
  4 E-1 option a).
- **FR2 — `App::frame_work_pending` predicate:** Add
  `App::frame_work_pending(&self) -> bool` in `src-tauri/src/app/mod.rs` equal to
  `toast_pending() || !sftp_progress_rx.is_empty() || !sftp_result_rx.is_empty() || crate::self_exec::restart_pending()`.
  The SFTP receivers are `crossbeam_channel::Receiver` (`ProgressReceiver` /
  `ResultReceiver` in `src-tauri/src/sftp/service.rs:58-60`), so `is_empty()` gives a
  non-destructive check. The predicate itself must consume nothing (no drain, no swap).
- **FR3 — `overlay_work` uses the new predicate:** In
  `src-tauri/src/window_host/render_surface.rs`, replace the `app.toast_pending()` term in
  the `overlay_work` expression (currently line ~291) with the new `frame_work_pending()`
  predicate so `should_skip_frame` no longer early-returns while pre-toast pending work
  exists.
- **FR4 — event_loop redraw pacing uses the new predicate:** In
  `src-tauri/src/window_host/event_loop.rs`, replace the `self.app.toast_pending()` read
  feeding `toast_redraw_due` (currently line ~645) with the new predicate so redraw pacing
  keeps frames flowing while pre-toast pending work exists.
- **FR5 — `next_toast_deadline` predicate choice (status: tbd):**
  `App::next_toast_deadline()` (`src-tauri/src/app/mod.rs:932`) may either keep using
  `toast_pending()` or move to the new predicate. **TBD reason:** the task description
  explicitly defers this choice to design time; not a user-facing question, to be settled
  in the plan step of the workflow.
- **FR6 — Update known-limitation doc:** Update the known-limitation paragraph in the
  `App::pump_toasts` doc comment (`src-tauri/src/app/mod.rs:910-916`, the "a pending SFTP
  event with no toast up yet ... relies on another redraw trigger" passage) to match the
  new implementation.
- **FR7 — Unit tests for the new predicate:** Add unit tests asserting the new predicate
  returns true (a) when an SFTP channel is non-empty and (b) when the restart flag is set,
  following the project's inline `#[cfg(test)] mod tests` convention and
  `<subject>_<scenario>_<expected>` naming.

### Non-Functional Requirements

- **NFR1 — Compatibility:** The consume semantics of the existing `restart_required()`
  (swap-reset, arms the toast exactly once) must not change.
- **NFR2 — Build quality:** Zero warnings across the three check configurations: Linux GUI
  `cargo check`, `cargo check --no-default-features`, and `cargo xwin check --tests` for
  the Windows target.
- **NFR3 — Performance:** The predicate check must be cheap enough to run per event-loop
  turn / per render decision (atomic load + channel `is_empty()`; no locking beyond what
  `App` already holds).

## Implementation Approach

### Architecture

The change stays inside the existing native GUI runtime path; no new component is
introduced.

```
SFTP service ──progress/result──> crossbeam channels ─┐
                                                      ├─> App::frame_work_pending()  (FR2)
self_exec RESTART_REQUIRED ──restart_pending()────────┘        │
                                                               ├─> render_surface::overlay_work / should_skip_frame  (FR3)
                                                               └─> event_loop toast_redraw_due pacing               (FR4)
```

### Data Flow

```
send_progress / note_spawn_failure → wake()
  → event loop turn → frame_work_pending() == true
  → frame is not skipped → pump_toasts creates the first toast → toast rendered
```

The wake path already exists (`send_progress` calls `wake()`; `note_spawn_failure` calls
`wake()`), so the fix only needs to stop `should_skip_frame` / redraw pacing from
discarding the frame — no new wake mechanism is required.

### Alternative considered

Moving `pump_toasts` ahead of the skip gate is rejected in the task description as heavier,
because `pump_sftp` needs the egui frame-time clock. The primary fix approach is the
investigated one from the task description (the new predicate).

### API Design

Not applicable — this feature adds no external API.

### Database Schema

Not applicable — this feature has no persistent data.

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/self_exec.rs`: hosts `RESTART_REQUIRED`, the existing consuming
  `restart_required()`, and the new `restart_pending()` peek.
- `src-tauri/src/app/mod.rs`: hosts `toast_pending()`, `pump_toasts` (910-916),
  `next_toast_deadline()` (932), and the new `frame_work_pending()`.
- `src-tauri/src/window_host/render_surface.rs`: `overlay_work` / `should_skip_frame`.
- `src-tauri/src/window_host/event_loop.rs`: `toast_redraw_due` redraw pacing.
- `src-tauri/src/sftp/service.rs:58-60`: `ProgressReceiver` / `ResultReceiver`.

**External Dependencies:**

- `crossbeam_channel`: the SFTP receivers' `is_empty()` provides the non-destructive check.

### File Structure

```
src-tauri/src/
├── self_exec.rs                     # FR1: restart_pending() non-consuming peek
├── app/mod.rs                       # FR2: frame_work_pending(); FR5: next_toast_deadline; FR6: pump_toasts doc
├── window_host/
│   ├── render_surface.rs            # FR3: overlay_work term
│   └── event_loop.rs                # FR4: toast_redraw_due
└── sftp/service.rs                  # ProgressReceiver / ResultReceiver (read-only reference)
```

## Test Scenarios

### Unit Tests

- [ ] TS1: New predicate returns false when no toast, empty channels, and clear restart
      flag. (FR2)
- [ ] TS2: New predicate returns true when the SFTP progress channel holds an event and no
      toast exists yet. (FR2, FR7)
- [ ] TS3: New predicate returns true when the SFTP result channel holds an event. (FR2,
      FR7)
- [ ] TS4: New predicate returns true when `RESTART_REQUIRED` is set, and checking it does
      NOT clear the flag (non-consuming peek), so a subsequent `restart_required()` still
      returns true. (FR1, FR2, FR7)
- [ ] TS5: `restart_required()` consume semantics unchanged — returns true once then false.
      (FR1)

### Integration Tests

None specified.

### E2E Tests

**Existing E2E tests**: None — no E2E infrastructure exists.
**Run command**: Not detected.

- [ ] TS6 (manual): AC1 idle-tab SFTP upload and AC2 restart-toast arming are user-verified,
      consistent with `test/README.md`'s statement that end-to-end behavior is validated
      manually. (FR3, FR4)

### Performance Tests

Covered by NFR3's construction constraint (atomic load + channel `is_empty()`); no separate
load or stress test is specified.

## Security Considerations

Not applicable — this feature changes an internal frame-scheduling predicate and processes
no external input.

## Error Handling

Not applicable — the predicate is a pure boolean read with no failure mode.

## Performance Optimization

### Performance Goals

- The predicate check is cheap enough to run per event-loop turn / per render decision:
  atomic load + channel `is_empty()`, with no locking beyond what `App` already holds
  (NFR3).

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] AC1: On an idle tab (cursor blink disabled or window unfocused), starting an SFTP
      upload shows the progress toast promptly after the first progress event arrives.
- [ ] AC2: After a binary swap causes a self-spawn failure, the restart toast arms and
      displays promptly even while the app is idle (Phase 7 E-1).
- [ ] AC3: The `App::pump_toasts` doc known-limitation text is updated to reflect the new
      implementation.
- [ ] AC4: Unit tests for the new predicate pass — true for a non-empty SFTP channel and
      true for a set restart flag, each independently.
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      passes fully; all three check configurations (Linux GUI, `--no-default-features`,
      `cargo xwin check --tests`) complete with zero warnings.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- [ ] FR5: `next_toast_deadline` predicate choice - The task description explicitly defers
      this choice to design time. Not a user-facing question; to be settled in the plan step
      of the workflow.

## Scope Boundaries

Out of scope per the task description: toast UI design / duration changes, SFTP upload
mechanism or channel-layout changes, and the other `should_skip_frame` terms (dirty /
status bar / egui input).

This is a Linux+Windows GUI-feature code path; `self_exec` restart detection is Linux-only
per its module doc, and the new peek follows the module's existing cfg structure.

The design step is skipped: this is a pure backend/runtime-logic bug fix inside
`src-tauri/src/` (frame-skip gate predicate). No new UI surface; toast UI design changes are
explicitly out of scope in the task description.

## References

- Requirements document: [REQUIREMENTS.md](REQUIREMENTS.md)
- em-review finding `b5f2cce1822ab271` (2026-08-11, resolution: deferred)
- `test/README.md`
