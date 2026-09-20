# Feature: mouse-report-reset-active-gesture

## Overview

The task0005 / task0006 rework of the mouse-report decision layer introduced a
ghost-drag regression: a `RecordUpdates.reset` raised because mouse tracking is
inactive erases every gesture-ownership slot, including the slot of a left
button that is still physically held, so the following left release finds no
owner and decides `Disposition::Nothing`, leaving `host.dragging` stuck true.
This feature narrows what that reset erases and broadens the local left-release
arm so a left release — and a focus loss — always terminates a local selection
drag. The DEC mouse-report protocol surface delivered by PR #69 is preserved
byte-for-byte; only the local-ownership path is repaired.

Requirements document: `feature-docs/mouse-report-reset-active-gesture/REQUIREMENTS.md`.

## Objectives

- Remove the ghost-drag regression introduced by the task0005 / task0006 rework
  of the mouse-report decision layer, restoring the pre-rework guarantee that a
  left-button release always terminates a local selection drag.
- Keep `host.dragging` a strictly bounded state: every path that sets it true
  has a guaranteed terminator, so the pointer-state-gated features
  (`update_resize_hint`, `refresh_link_hover`, PTY-output link re-detection) can
  never be permanently disabled.
- Preserve the mouse-reporting behaviour PR #69 delivered byte-for-byte; this is
  a repair of the local-ownership path only, not a change to the DEC
  mouse-report protocol surface.

## User Stories

### US1: A left drag survives an interleaved wheel notch or second button press

As a terminal user, I want a left-button release to end my selection drag even
when I turned the wheel or pressed another button while holding left, so that
the selection stops following the pointer and lands in PRIMARY.

**Acceptance Criteria:**

- [ ] AC-1: Repro step sequence 1-4 (left press-drag on the grid, wheel one
      notch without releasing, then release left) ends with
      `host.dragging == false`, `app.pending_selection_anchor == None`, and the
      dragged selection present in PRIMARY.
- [ ] AC-2: The same sequence with a middle-button or right-button press
      substituted for the wheel notch ends in the identical state.
- [ ] AC-4: After any of AC-1..AC-3, `update_resize_hint` and
      `refresh_link_hover` (pointer_routing.rs:227) and the PTY-output link
      re-detection (event_loop.rs:649) run again on the next pointer motion.
- [ ] AC-9: Regression tests exist that fail against the current revision and
      pass after the fix, one per AC-1 / AC-2 / AC-3.

### US2: Losing window focus mid-drag ends the drag

As a terminal user, I want a drag that is in flight when the window loses focus
to be finished rather than left live, so that the selection does not keep
extending when I come back.

**Acceptance Criteria:**

- [ ] AC-3: Losing window focus mid-drag ends with `host.dragging == false`,
      `app.pending_selection_anchor == None`, and the selection in PRIMARY; no
      subsequent `PointerMoved` extends the selection.
- [ ] AC-4: After any of AC-1..AC-3, `update_resize_hint` and
      `refresh_link_hover` (pointer_routing.rs:227) and the PTY-output link
      re-detection (event_loop.rs:649) run again on the next pointer motion.

### US3: Reporting behaviour and chrome clicks are unaffected

As a terminal user running a mouse-reporting application, I want the repair to
change nothing about what bytes reach the PTY and nothing about clicks on the
terminal chrome, so that no new behaviour appears as a side effect of the fix.

**Acceptance Criteria:**

- [ ] AC-5: A reset observation occurring while no button is held still clears
      every gesture-ownership slot and still resets the cell-change cache (FR6).
- [ ] AC-6: A reset observation occurring while a button IS held still resets
      the cell-change cache, and still clears the slots of the buttons that are
      not held.
- [ ] AC-7: Every existing test in `src-tauri/src/window_host/mouse_report.rs`'s
      `mod tests` continues to pass unchanged in meaning; report byte sequences,
      target-tab selection and the wheel matrix are unaltered (FR4).
- [ ] AC-8: A left press on the tab bar / status bar / scrollbar overlay / mux
      sidebar followed by its release causes no PRIMARY or CLIPBOARD write and
      no fold toggle (FR5).

## Technical Requirements

### Functional Requirements

- **FR1 — Reset preserves physically-held gestures:** When an outcome's
  `RecordUpdates.reset` is applied (mouse_report.rs:1027-1030) because mouse
  tracking is inactive (`!tracking_active`), gesture-ownership slots belonging
  to buttons that are currently physically held are preserved; only slots for
  buttons not currently held are cleared. The tab-change trigger keeps clearing
  every slot (assumption A3, FR6). The cell-change-cache half of the reset is
  unchanged and still runs unconditionally. Concretely: a left press over the
  grid with tracking inactive records `GestureOwner::Local` for Left
  (mouse_report.rs:792); a subsequent wheel notch (`decide_wheel_event`,
  mouse_report.rs:983) or a middle/right press (`decide_press`,
  mouse_report.rs:753) — both of which set `reset = true` because
  `tracking_active` is false — must no longer erase that Left slot while Left is
  still down.
- **FR2 — A left release always finishes the local drag:** A left-button release
  that is not Report-owned — i.e. the recorded owner is `GestureOwner::Local` OR
  no owner is recorded at all (`peek(Left) == None`, mouse_report.rs:807) —
  takes the `CompleteSelectionAndPublishToPrimary` local arm. That arm clears
  `host.dragging`, consumes `app.pending_selection_anchor`, and publishes a
  completed selection to PRIMARY (and to CLIPBOARD when `copy_on_select` is on),
  exactly as pointer_routing.rs:361-406 does today. The no-owner left release
  must no longer decide bare `Disposition::Nothing`.
- **FR3 — Focus loss terminates a live local drag:** The
  `WindowEvent::Focused(false)` arm (event_loop.rs:229-253), alongside the
  existing `mouse_report::clear_all` call at event_loop.rs:249, clears
  `host.dragging` and consumes `app.pending_selection_anchor`. When a left
  selection drag with a materialized selection was in flight at the moment focus
  was lost, the selection is published to PRIMARY under the same rules a release
  applies (including the `copy_on_select` CLIPBOARD mirror). See assumption A1.
- **FR4 — Report-owned behaviour is untouched:** Every Report-owned disposition
  keeps its current behaviour byte-for-byte: press / release encoding
  (`compose_button_code` / `encode_report`), the release targeting
  `records.built_for_tab` rather than `active_tab` (mouse_report.rs:812), the
  motion gate and cell-change filter, and the wheel-consumer matrix. A
  Report-owned left release must NOT take the local selection-completion arm.
- **FR5 — No new side effect for a release with no drag in flight:** A left
  release that arrives with no local drag in flight (`host.dragging == false`
  and `app.pending_selection_anchor == None`) — for example the release of a
  press that landed on the tab bar, the status bar, the scrollbar overlay or the
  mux sidebar, which records no owner today (mouse_report.rs:738-750) —
  produces no PRIMARY or CLIPBOARD write, and no fold-click toggle, that it does
  not already produce at the current revision. Broadening FR2 must not turn
  chrome clicks into selection re-publishes.
- **FR6 — Stale gesture records are still cleared:** The reset observation
  continues to clear gesture-ownership slots for buttons that are NOT currently
  held, and, for the tab-change trigger, continues to clear every slot including
  held ones. The repair narrows what reset erases; it does not remove the reset.
  Scope of the narrowing across the two reset triggers is fixed by assumption
  A3.

### Non-Functional Requirements

- **NFR1 - Maintainability (the decision layer stays window-free):** No
  signature in `src-tauri/src/window_host/mouse_report.rs` may take or return a
  winit type, an egui window handle, a GPU surface, a PTY, or a `term_core` mode
  type. This is the module's stated AC-8 / AC-1 invariant
  (mouse_report.rs:14-20, 401-403) and is what makes every unit exercisable from
  a bare `#[test]`. If held-button state must reach `decide_press` /
  `decide_wheel_event` / `apply_outcome`, it travels as plain bools or the
  existing plain `HeldButtons` value (mouse_report.rs:436-448).
- **NFR2 - Maintainability (records are never mutated at decision time):** SC-10
  property 3 (mouse_report.rs:494-497, 507-509) is preserved: `decide_*`
  functions remain pure functions of their plain inputs, and every record
  mutation continues to travel in `RecordUpdates` for `apply_outcome` to apply
  exactly once.
- **NFR3 - Maintainability (test style matches the project convention):** New
  tests are inline `#[cfg(test)] mod tests {}` units next to the code under
  test, named `<subject>_<scenario>_<expected>`, constructed per-test with no
  shared global fixture, and built from the existing `base_button_inputs` /
  `base_motion_inputs` / `base_wheel_inputs` helpers
  (mouse_report.rs:1667-1725). Those helpers default to tracking ON, so a repro
  test must set `mode_1002 = false`. No new test-framework dependency
  (test/README.md "Test Framework": no proptest, no criterion).
- **NFR4 - Compatibility (build-surface invariance):** The CLI-only build
  (`--no-default-features`) still compiles — the touched modules are all
  GUI-gated — and both Linux and Windows remain supported; no platform-specific
  API is introduced (.claude/rules/core-architecture.md).
- **NFR5 - Security (the wheel-report notch clamp is not weakened):**
  `bounded_wheel_report_duplicate` / `MAX_WHEEL_REPORT_NOTCHES`
  (pointer_routing.rs:737-744) is documented as a security property, not a
  feel-tuning knob. Work on the wheel path must leave the cap intact.

## Implementation Approach

### Architecture

**Layering (unchanged by this feature):**

```
┌──────────────────────────────────────────────────────────┐
│ event_loop.rs   winit event arms                         │
│                 (Focused(false), PTY-output link redetect)│
├──────────────────────────────────────────────────────────┤
│ pointer_routing.rs  pointer event routing, local arms     │
│                     (CompleteSelectionAndPublishToPrimary)│
├──────────────────────────────────────────────────────────┤
│ mouse_report.rs  window-free decision layer (NFR1)        │
│                  decide_press / decide_release /          │
│                  decide_wheel_event → Outcome             │
│                  apply_outcome → MouseReportRecords       │
├──────────────────────────────────────────────────────────┤
│ host / app state  WindowHost.dragging,                    │
│                   WindowHost.mouse_report_held,           │
│                   App.pending_selection_anchor            │
└──────────────────────────────────────────────────────────┘
```

**Component notes:**

- `mouse_report.rs` holds the pure decision functions and `apply_outcome`, the
  single place where `RecordUpdates` is applied to `MouseReportRecords`
  (NFR2).
- `HeldButtons` is a separate `WindowHost` field (mod.rs:263) and is
  deliberately not part of `MouseReportRecords` (assumption A4).
- `host.dragging` and `app.pending_selection_anchor` are declared in
  `src-tauri/src/window_host/mod.rs` and `src-tauri/src/app.rs` respectively,
  outside the three files the bug report names (assumption A5).

### Data Flow

```
winit pointer/wheel event
  → pointer_routing::handle_pointer_button / wheel path
      updates host.mouse_report_held  (pointer_routing.rs:486-494, before any decision)
  → mouse_report::decide_press / decide_release / decide_wheel_event   (pure)
      → Outcome { disposition, RecordUpdates { reset, ... } }
  → mouse_report::apply_outcome (+ HeldButtons companion, assumption A4)
      → MouseReportRecords mutated exactly once
  → disposition dispatch
      Report-owned  → encode_report → PTY bytes            (unchanged, FR4)
      Local         → CompleteSelectionAndPublishToPrimary  (FR2)
                        clears host.dragging,
                        consumes app.pending_selection_anchor,
                        publishes to PRIMARY (+ CLIPBOARD when copy_on_select)

WindowEvent::Focused(false)
  → event_loop.rs:229-253
      mouse_report::clear_all (event_loop.rs:249)
      + window-free cleanup helper → same terminator as the release path (FR3)
```

### API Design

No network or IPC API is introduced. The internal surface change is confined to
the decision layer:

- `mouse_report::apply_outcome` gains a companion entry point that additionally
  takes the plain `HeldButtons` value; the existing `apply_outcome` is kept as a
  thin delegating wrapper so the roughly 30 inline test call sites need no edit
  (assumption A4). The three production call sites move to the companion.
- The held-button exclusion is performed inside that companion, not by threading
  held flags into `ButtonEventInputs` / `WheelEventInputs` (assumption A4,
  NFR2).
- The focus-loss cleanup is structured so it is reachable without a winit window
  (NFR1); if the mutation can only be expressed at the `event_loop.rs` arm, it
  is factored into a window-free helper.

### Database Schema

Not applicable. This feature persists no data and changes no schema.

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/window_host/mouse_report.rs`: the window-free decision layer;
  the reset observation and release decision live here.
- `src-tauri/src/window_host/pointer_routing.rs`: pointer routing, the
  `CompleteSelectionAndPublishToPrimary` arm (pointer_routing.rs:361-406), the
  held-button record update (pointer_routing.rs:486-494), the side-button early
  return (pointer_routing.rs:510-521), the CSD edge-resize short-circuit
  (pointer_routing.rs:438-445), and the wheel notch clamp
  (pointer_routing.rs:726-744).
- `src-tauri/src/window_host/event_loop.rs`: the `Focused(false)` arm
  (event_loop.rs:229-253) and the PTY-output link re-detection
  (event_loop.rs:649).
- `src-tauri/src/window_host/mod.rs`: `WindowHost.dragging`,
  `WindowHost.mouse_report_held` (mod.rs:263),
  `WindowHost.mouse_report_gesture_owner`, and the `mouse_report_records()` /
  `set_mouse_report_records()` accessors (assumption A5).
- `src-tauri/src/app.rs`: `App.pending_selection_anchor` (assumption A5).
- `src-tauri/src/window_host/tests.rs`: the structural test at
  tests.rs:1782-1791 whose delegate list must gain the companion entry point's
  name (assumption A7).

**External Dependencies:**

- None added. No new test-framework dependency (NFR3).

### File Structure

```
src-tauri/src/
├── app.rs                        # App.pending_selection_anchor (A5)
└── window_host/
    ├── mod.rs                    # WindowHost.dragging / mouse_report_held (A5)
    ├── mouse_report.rs           # FR1, FR4, FR6 + inline #[cfg(test)] mod tests
    ├── pointer_routing.rs        # FR2, FR5
    ├── event_loop.rs             # FR3
    └── tests.rs                  # structural delegate list (A7)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mouse-report-reset-active-gesture/**`
- `test-docs/mouse-report-reset-active-gesture/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1 — Left drag survives a wheel notch** (AC-1, AC-9; FR1, FR2): Apply
      `decide_press(Left, tracking inactive, over grid)`, then
      `decide_wheel_event(WheelUp, over grid, tracking inactive)`, then
      `decide_release(Left)` against one `MouseReportRecords` value threaded
      through `apply_outcome`. Assert the release's disposition is
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)`.
- [ ] **TS-2 — Left drag survives a second button press** (AC-2, AC-9; FR1,
      FR2): Same as TS-1 with a middle press (and again with a right press) in
      place of the wheel notch. Assert the left release still names
      `CompleteSelectionAndPublishToPrimary`, and that the middle/right slot is
      recorded and cleared independently.
- [ ] **TS-3 — No-owner left release still completes the drag** (AC-1, AC-2;
      FR2): `decide_release(Left)` against a `MouseReportRecords` whose Left slot
      is `None` and whose records indicate a local drag is live yields the
      completion arm rather than `Disposition::Nothing`.
- [x] **TS-4 — Focus loss terminates the drag** (AC-3, AC-9; FR3, NFR1):
      Ticked: the reworded claim below is met by the tests as they stand,
      confirmed by reading `src-tauri/src/window_host/tests.rs` directly.
      The window-free test
      `focus_loss_cleanup_publishes_selection_without_a_window` in
      `src-tauri/src/window_host/tests.rs` composes the window-free state
      core and the recording sink the way the focus-loss adapter composes
      them, over plain values, with no window host constructed and no
      selection resolved, and asserts `dragging` is cleared, the pending
      anchor is consumed and returned, and the publish is requested with the
      expected destinations and payload (NFR1), together with the
      destination-predicate truth-table tests prefixed
      `selection_publish_targets_` in the same module, which cover the
      destination rule this scenario depends on. The adapter's own
      gather-and-adapt wiring (`publish_local_drag`) is covered only by the
      call-site literals the source-scanning test in
      `src-tauri/src/window_host/tests.rs` compares against the event-loop
      file's text, not by these behavioural tests. A recorded sink call in
      these tests is evidence that the publish path requested the write, not
      evidence of an OS-level PRIMARY selection or CLIPBOARD update.
- [ ] **TS-5 — Reset still resets the cell cache during a held gesture** (AC-6;
      FR1, FR6): With Left held and its slot recorded, apply an outcome carrying
      `reset: true` and assert `records.cell_cache.would_report(c, r)` is true
      afterwards for the previously cached cell while `peek(Left)` is still
      `Some(GestureOwner::Local)`.
- [ ] **TS-6 — Reset still clears a not-held button's stale record** (AC-5,
      AC-6; FR6): With a Middle slot recorded but Middle not held, apply an
      outcome carrying `reset: true` and assert `peek(Middle) == None`.
- [ ] **TS-7 — Report-owned paths unchanged** (AC-7; FR4): The existing
      `mod tests` suite in mouse_report.rs (including the byte-exact X10 / SGR
      cases and the tab-targeting release cases) passes unchanged.
- [ ] **TS-8 — Chrome release produces no side effect** (AC-8; FR5): A left
      press with `grid.in_tab_bar_band = true` (records no owner) followed by its
      release produces no PRIMARY publish and no fold toggle.

### Integration Tests

None. Every automated scenario in this feature is a unit-level test inside the
touched modules (NFR3).

### E2E Tests

**Existing E2E tests**: None — E2E automation does not exist in this project
(test/README.md "E2E Tests").
**Run command**: Not detected.

- [ ] **TS-9 — Manual reproduction** (AC-1, AC-2, AC-3, AC-4; FR1, FR2, FR3):
      Run the release binary and walk the four repro steps plus the focus-loss
      variant; confirm the selection stops extending after release and appears
      in PRIMARY via middle-click paste. This scenario is user-executed.

### Edge Cases

- [ ] Tracking mode is enabled by the application mid-drag — the Left slot says
      `Local` while `tracking_active` becomes true. The release must still
      complete the local drag (FR2), not emit a report.
- [ ] Tracking mode is disabled mid-gesture while a Report-owned button is held
      — `decide_release`'s Report branch already answers `Disposition::Nothing`
      when no tracking mode is active (mouse_report.rs:810-828). FR1 must not
      turn that into a spurious report.
- [ ] Active tab changes while a button is physically held (see assumption A3):
      the tab-change trigger keeps clearing every slot.
- [ ] Several buttons held at once — the release of a middle or right button
      must not run the left completion arm; `decide_release`'s
      `GestureOwner::Local` branch already restricts the arm to
      `MouseButtonId::Left` (mouse_report.rs:831-837).
- [ ] A side button with no DEC report identity —
      `winit_button_to_report_identity` returns `None` and
      `handle_pointer_button` returns early on release
      (pointer_routing.rs:510-521), so such a release can never terminate a left
      drag. The left release is still required, which is correct.
- [ ] `MouseButtonId::None` never occupies a gesture slot
      (`GestureOwnership::slot`, mouse_report.rs:348) — the held-button
      exclusion must not accidentally give it one.
- [ ] Focus loss with no drag in flight must remain a pure no-op for selection
      state and for PRIMARY.
- [ ] The fold-click toggle inside `complete_selection_and_publish_to_primary`
      (pointer_routing.rs:380-388) fires only when a pending anchor exists, no
      selection exists, and Ctrl is not held; broadening FR2 must not make a
      chrome click satisfy that predicate.
- [ ] The CSD edge-resize press short-circuit (pointer_routing.rs:438-445)
      returns before any mouse-report decision, so a resize handoff records no
      owner and starts no drag — unchanged.
- [ ] A press over the grid whose release arrives over chrome —
      `decide_release` is deliberately position-independent
      (mouse_report.rs:798-805). FR2 must preserve that.
- [ ] A stuck `dragging` today also suppresses event_loop.rs:649's
      PTY-output-driven link re-detection, a symptom the bug report does not
      list; AC-4 covers it.

### Performance Tests

None. The requirements carry no performance target for this feature.

## Security Considerations

- **New attack surface:** None. The change is confined to local pointer-state
  bookkeeping and emits no additional bytes to the PTY.
- **Wheel-report notch clamp (NFR5):** `bounded_wheel_report_duplicate` /
  `MAX_WHEEL_REPORT_NOTCHES` (pointer_routing.rs:726-744) is a documented
  security property and must not be relaxed while working the wheel path.
- **Report byte invariance (FR4):** Byte-for-byte invariance means no report
  sequence can gain or lose bytes as a side effect of this repair.
- **Authentication / Authorization / Input validation / Data protection / XSS /
  SQL injection / CSRF:** Not applicable — this feature has no network surface,
  no persisted data and no WebView content.

## Error Handling

No new error codes or error responses are introduced. The failure modes this
feature addresses are state-machine conditions, handled as follows:

| Condition | Handling |
|---|---|
| Left release arrives with no recorded owner | Take the local completion arm rather than `Disposition::Nothing` (FR2) |
| Left release arrives with no drag in flight | No PRIMARY / CLIPBOARD write and no fold toggle beyond what the current revision already produces (FR5) |
| Focus lost with a live left drag | Clear `dragging`, consume the pending anchor, publish the selection (FR3, assumption A1) |
| Focus lost with no drag in flight | Pure no-op for selection state and for PRIMARY |
| Reset observed while a button is held | Preserve the held button's slot, clear the not-held slots, reset the cell-change cache (FR1, FR6) |
| Reset observed on a genuine tab change | Clear every slot, including held ones (FR6, assumption A3) |

## Performance Optimization

Not applicable. The requirements state no performance goal, and the repair adds
no work to a hot path beyond a held-button check inside `apply_outcome`.

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and tested
- [ ] All test scenarios (TS-1..TS-9) pass
- [ ] All acceptance criteria (AC-1..AC-9) are satisfied
- [ ] Security requirements are satisfied (NFR5 cap intact, no new PTY bytes)
- [ ] The CLI-only build (`--no-default-features`) still compiles (NFR4)
- [ ] The decision layer remains window-free (NFR1) and `decide_*` remains pure
      (NFR2)
- [ ] Documentation is complete
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional requirement is `status: resolved`; no requirement carries
`status: tbd`.

A6 was recorded as unverified when this document was first written, and has
since been verified during planning:

- A6 (impact low, reversible): the pre-rework behaviour named in the bug report
  — a base-code unconditional `(Left, Released) => host.dragging = false` arm —
  is the intended target semantics for FR2. **Verified.** The pre-rework
  revision is in this repository: the rework commit is `34774fab` ("task0006:
  reduce pointer handlers to gather/decide/apply/perform"), and
  `34774fab^:src-tauri/src/window_host/pointer_routing.rs:621-622` carries that
  arm unconditionally. In the same pre-rework file the release short-circuit at
  `:406-438` returns only for `GestureOwner::Report`, with in-source comments at
  `:430-436` stating that the Local and no-owner cases fall through to that arm.
  Independent evidence in the current tree: `src-tauri/src/window_host/mod.rs:172-174`
  documents `dragging` as "whether the left button is currently held", and
  `dragging` is set only at `pointer_routing.rs:339` and cleared only at `:362`,
  with three consumers (`pointer_routing.rs:227`, `event_loop.rs:649`,
  `link_hover.rs:178-179`) that have no self-healing path. FR5's drag-in-flight
  gate on the no-owner left release is a deliberate narrowing of that
  unconditional arm; the guarantee A6 underwrites — a left release with a live
  drag always clears the flag — is preserved.

## Implementation Phases (if applicable)

Not applicable. The repair is a single coherent change across the decision
layer, the pointer routing arms and the focus-loss arm; splitting it into phases
would leave `host.dragging` unterminated in an intermediate state.

## Assumptions

Every assumption below originates in the resolved requirements analysis; the
full reasoning is recorded in
`feature-docs/mouse-report-reset-active-gesture/REQUIREMENTS.md` section 14.

| ID | Impact | Reversible | Assumption |
|---|---|---|---|
| A1 | medium | yes | Focus loss during a live left drag publishes the in-flight selection to PRIMARY, not merely clears the drag state. |
| A2 | low | yes | "Currently pressed buttons" means the host's held-button record (`host.mouse_report_held`, the plain `HeldButtons` value at mouse_report.rs:436-448). |
| A3 | medium | yes | The held-button exclusion applies ONLY to the `!tracking_active` reset trigger. A genuine tab change (`built_for_tab != Some(active_tab)`) keeps clearing every gesture-ownership slot exactly as it does today, and no per-button press-origin tab is introduced. |
| A4 | medium | yes | The held-button exclusion is performed inside `apply_outcome`, not by threading held flags into `ButtonEventInputs` / `WheelEventInputs`. `apply_outcome` gains a companion entry point taking `HeldButtons`, with the existing `apply_outcome` kept as a thin delegating wrapper so the roughly 30 inline test call sites need no edit. |
| A5 | low | yes | `host.dragging` and `app.pending_selection_anchor` are fields on `WindowHost` and `App` respectively, declared outside the three files named in the bug report. |
| A6 | low | yes | The pre-rework behaviour named in the bug report — a base-code unconditional `(Left, Released) => host.dragging = false` arm — is the intended target semantics for FR2. Verified during planning against `34774fab^:src-tauri/src/window_host/pointer_routing.rs:621-622`; see the note above this table. |
| A7 | low | yes | `src-tauri/src/window_host/tests.rs:1782-1791` asserts that pointer_routing.rs's source literally contains the needle `"mouse_report::apply_outcome("`. Adding a companion entry point (A4) and moving the three production call sites to it requires that structural test's delegate list entry to be **replaced** with the companion's name, not merely extended: the needle's trailing `(` blocks a prefix match, so once no call site spells `apply_outcome(` the original entry fails. Only `mouse_report::`-prefixed delegate names are checked by that test — no other identifier introduced by this feature is structurally pinned. |

## Design Step

Skipped. No user-visible surface changes. The repair is confined to
pointer-state bookkeeping in three GUI-internal Rust modules; it introduces no
new UI element, no new design token, no layout, copy, colour or
interaction-affordance change. The only observable difference is the restoration
of behaviour the base code already had. `doc/UI-DESIGN-GUIDELINES.yaml` and its
two mirrors are untouched, so the design-system drift tests in
`ui::dialog::tests` are unaffected.

## References

- Requirements document: `feature-docs/mouse-report-reset-active-gesture/REQUIREMENTS.md`
- Decision layer: `src-tauri/src/window_host/mouse_report.rs`
- Pointer routing and local selection arms: `src-tauri/src/window_host/pointer_routing.rs`
- Event loop arms: `src-tauri/src/window_host/event_loop.rs`
- Host state: `src-tauri/src/window_host/mod.rs`
- App state: `src-tauri/src/app.rs`
- Structural delegate-list test: `src-tauri/src/window_host/tests.rs`
- Test conventions and E2E status: `test/README.md`
- Build surface and platform support: `.claude/rules/core-architecture.md`
- Current mouse-reporting behaviour: PR #69
