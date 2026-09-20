# Feature: focus-loss-drag-termination

## Overview

The `WindowEvent::Focused(false)` arm terminates a live local left selection
drag unconditionally today, so a focus steal — a notification, a
focus-follows-mouse compositor, another window raising — destroys a drag the
user is still physically performing with the left button down. This feature
narrows that termination to the case where the left button is not held,
preserves the drag intact while it is held, and delegates the preserved drag's
termination to the release path's existing no-owner `drag_in_flight` fallback.
The decision itself is extracted as a pure, winit-free predicate so its full
truth table is provable from a bare `#[test]`.

Requirements document: `feature-docs/focus-loss-drag-termination/REQUIREMENTS.md`.

## Objectives

- Stop a window focus loss from destroying a left selection drag the user is
  still physically performing: the drag must survive a focus steal
  (notification, compositor focus-follows-mouse, another window raising) as long
  as the left button is down.
- Keep the bounded-state guarantee the prior feature
  (mouse-report-reset-active-gesture) established: every path that sets
  `host.dragging` true still has a guaranteed terminator, so the
  pointer-state-gated features (`update_resize_hint`, `refresh_link_hover`,
  PTY-output link re-detection) can never be permanently disabled.
- Keep the termination decision expressible and provable without a winit window,
  preserving the decision layer's bare-`#[test]` testability invariant.

## Supersession

This SPEC supersedes three items of
`feature-docs/mouse-report-reset-active-gesture/SPEC.md`, named explicitly
(FR6):

| Superseded item in the prior SPEC | Reads, after supersession |
|---|---|
| **FR3** — "Focus loss terminates a live local drag" | Focus loss terminates a live local left drag **only when the left button is not held**. While it is held, the arm publishes nothing and the drag is preserved (FR1, FR3). |
| **AC-3** — "Losing window focus mid-drag ends with `host.dragging == false` …" | That end state is required only for the not-held case (AC-2). With the left button held, `host.dragging` stays true and the pending anchor stays in place (AC-1). |
| **Error Handling** state-condition row "Focus lost with a live left drag" | Split by the held-button gate: held → preserve (FR3); not held → clear, consume the anchor, publish (FR1). See this document's own Error Handling table. |

**Refreshed line references.** The prior SPEC's stale references are replaced by
the current ones:

| Prior SPEC (stale) | Current |
|---|---|
| `event_loop.rs:229-253` | `event_loop.rs:227-282` |
| `event_loop.rs:249` | `event_loop.rs:253-255` and `event_loop.rs:262-265` |
| `pointer_routing.rs:361-406` | `pointer_routing.rs:391-412` and `pointer_routing.rs:424-443` |

**Not superseded.** The prior SPEC's FR1, FR2, FR4, FR5, FR6 and all of its
other acceptance criteria are untouched and remain in force.

## User Stories

### US1: A drag survives a focus steal while the button is still down

As a terminal user, I want a selection drag I am still physically performing to
survive the window losing focus, so that a notification or another window
raising does not throw away the selection I am in the middle of making.

**Acceptance Criteria:**

- [ ] AC-1: Focus is lost while a local left drag is in flight AND the left
      button is held → `host.dragging` stays true,
      `app.pending_selection_anchor` stays `Some`, nothing is written to PRIMARY
      or CLIPBOARD, and no fold toggle fires. (FR1, FR3)
- [ ] AC-5: After the drag preserved by AC-1, the eventual left release
      terminates it: `decide_release(Left)` with no recorded owner and
      `drag_in_flight == true` yields
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)`, which
      clears `host.dragging` and consumes the pending anchor. (FR3, FR8)

### US2: Focus loss with the button up behaves exactly as it does today

As a terminal user, I want focus loss with no button held to keep ending the
drag and publishing the selection, so that the fix introduces no new behaviour
on the path that already works.

**Acceptance Criteria:**

- [ ] AC-2: Focus is lost while a local left drag is in flight AND the left
      button is NOT held → `host.dragging` becomes false,
      `app.pending_selection_anchor` becomes `None`, and a materialized selection
      is published to PRIMARY (plus CLIPBOARD when `copy_on_select` is on) —
      today's behaviour, unchanged. (FR1)
- [ ] AC-3: Focus is lost with no local drag in flight → pure no-op for
      selection state and for PRIMARY, whether or not the left button is held.
      (FR1, FR7)
- [ ] AC-4: The left-held value that gates AC-1/AC-2 is the one recorded before
      `mouse_report::clear_all` runs; after the arm completes
      `host.mouse_report_held` is zeroed exactly as today. (FR2, FR7)
- [ ] AC-9: Every existing test in `src-tauri/src/window_host/` continues to
      pass, with the single exception of the structural assertion updated under
      FR5/AC-7; no mouse-report byte sequence, target-tab selection or
      wheel-matrix behaviour changes. (FR7, NFR5)

### US3: The decision stays provable without a window

As a maintainer, I want the termination decision to be a pure predicate over
plain bools, so that its whole truth table is asserted by a bare `#[test]` with
no winit window, and the arm's fidelity to it is pinned structurally.

**Acceptance Criteria:**

- [ ] AC-6: The termination decision exists as a pure function over plain bools
      whose full truth table is asserted by a bare `#[test]` requiring no winit
      window. (FR4, NFR1, NFR2)
- [ ] AC-7: `src-tauri/src/window_host/tests.rs`'s structural source-text
      assertion pins the new composed form of the focus-loss arm, and still
      asserts that `event_loop.rs` never contains `handle_fold_click`. (FR5)
- [ ] AC-8: The SPEC states in prose which of the prior feature's items it
      supersedes (FR3, AC-3, the "Focus lost with a live left drag" state-table
      row) and carries refreshed line references in place of the stale ones.
      (FR6)
- [ ] AC-10: The CLI-only build still compiles and no winit/egui/PTY/`term_core`
      type appears in any new or changed signature in the decision layer.
      (NFR1, NFR4)

## Technical Requirements

### Functional Requirements

- **FR1 — Focus-loss termination is gated on the left button not being held:**
  The `WindowEvent::Focused(false)` arm
  (src-tauri/src/window_host/event_loop.rs:227-282) terminates a live local left
  drag ONLY when the left button is not held at that moment. Termination means
  exactly what it means today: `publish_local_drag(host, &mut self.app)` runs
  (event_loop.rs:254), clearing `host.dragging`, consuming
  `app.pending_selection_anchor`, and publishing any materialized selection to
  PRIMARY (plus CLIPBOARD when `copy_on_select` is on). The current
  unconditional composition
  `if local_drag_in_flight(host, &self.app) { publish_local_drag(...) }`
  (event_loop.rs:253-255) is replaced by a composition that additionally
  requires the left button to be not held.
- **FR2 — The held-button state is read before `clear_all` zeroes it:** The
  left-held input to FR1's decision is read from `host.mouse_report_held.left`
  (the plain `HeldButtons` value, mouse_report.rs:471-483) BEFORE
  `mouse_report::clear_all(&mut host.mouse_report_gesture_owner, &mut host.mouse_report_held)`
  runs at event_loop.rs:262-265. Reading it after `clear_all` would always
  observe `false` and degrade FR1 to today's unconditional termination.
  `clear_all` itself, and its position in the arm, are unchanged.
- **FR3 — While the left button is held, focus loss preserves the drag intact:**
  When focus is lost while a local left drag is in flight AND the left button is
  held, the arm publishes nothing: no PRIMARY write, no CLIPBOARD write, no
  fold-click toggle. `host.dragging` stays true and
  `app.pending_selection_anchor` is left in place, so the drag continues when
  focus returns. Termination of that preserved drag is delegated to the release
  path's no-owner `drag_in_flight` fallback in `decide_release`
  (mouse_report.rs:935: a left release with no recorded owner takes the local
  completion arm when `drag_in_flight` is true) — which is reachable because
  `clear_all` has emptied the gesture-ownership record, making the eventual left
  release a no-owner release.
- **FR4 — The termination decision is a pure, winit-free predicate:** The FR1
  decision is extracted as a pure function over plain bools — a sibling of
  `drag_in_flight` (src-tauri/src/window_host/pointer_routing.rs:364-366) —
  taking the drag-in-flight signal (or its two constituent bools) and the
  left-held bool, and returning whether focus loss terminates. It takes and
  returns no winit type, no egui window handle, no GPU surface, no PTY and no
  `term_core` mode type, so it is drivable from a bare `#[test]`. The existing
  `drag_in_flight` and `local_drag_in_flight` (pointer_routing.rs:364-377) keep
  their current signatures and meaning; the new predicate composes with them
  rather than replacing them.
- **FR5 — The structural source-text assertion is extended to the new composed
  form:** `focus_loss_arm_never_calls_the_fold_click_toggle`
  (src-tauri/src/window_host/tests.rs:1931-1949) currently asserts that
  `event_loop.rs`'s source contains the literal needles
  `"local_drag_in_flight(host, &self.app)"` and
  `"publish_local_drag(host, &mut self.app)"`, and does not contain
  `"handle_fold_click"`. Because FR1 changes the call composition in that arm,
  the needle set is updated to pin the NEW composed form — including the new
  predicate's name and the held-button read — rather than merely appended to.
  The `handle_fold_click` negative assertion is preserved verbatim: the
  focus-loss arm must still never reach the fold-click toggle.
- **FR6 — This SPEC supersedes the prior feature's FR3, AC-3 and its focus-loss
  state-table row:** The SPEC records, explicitly and by name, that it
  supersedes `feature-docs/mouse-report-reset-active-gesture/SPEC.md`'s FR3
  ("Focus loss terminates a live local drag"), its AC-3 ("Losing window focus
  mid-drag ends with `host.dragging == false` …"), and its Error Handling
  state-condition row "Focus lost with a live left drag". After supersession
  those three read: focus loss terminates a live local left drag only when the
  left button is not held. The prior SPEC's stale line references —
  event_loop.rs:229-253 and :249, pointer_routing.rs:361-406 — are refreshed to
  the current ones (event_loop.rs:227-282, :253-255, :262-265;
  pointer_routing.rs:391-412 and :424-443). The prior SPEC's FR1, FR2, FR4, FR5,
  FR6 and all its other acceptance criteria are NOT superseded and remain in
  force. The Supersession section above is this requirement's rendering.
- **FR7 — Everything else in the `Focused(false)` arm is unchanged:** The rest
  of the arm keeps its current behaviour byte-for-byte:
  `self.app.window_focused = focused`, `notify_ime_focus`, `on_ime_focus_lost`,
  `host.current_mods = Modifiers::default()` (event_loop.rs:238),
  `host.pointer_buttons_down = 0` (event_loop.rs:243),
  `mouse_report::clear_all` (event_loop.rs:262-265),
  `host.update_link_cursor()`, the `Focused(true)` branch's `reset_blink_phase`,
  and the trailing `mark_full_redraw` / `request_redraw`. Focus loss with no
  drag in flight remains a pure no-op for selection state and for PRIMARY. No
  DEC mouse-report byte changes.
- **FR8 — No additional terminator is introduced for the lost-release paths:**
  No `Focused(true)` check, no first-motion-after-refocus check, and no separate
  selection-side press record are added. The release path's no-owner
  `drag_in_flight` fallback is the single terminator for a drag preserved by
  FR3. Rationale recorded as assumption A2: a `Focused(true)` / first-motion
  check cannot work because `clear_all` has already zeroed
  `host.mouse_report_held`, so such a check would observe `left == false`
  unconditionally and degrade to today's unconditional termination,
  reintroducing the very bug this feature fixes; and a separate selection-side
  press record would remain `held = true` forever exactly on the lost-release
  path, buying extra state and no coverage.

### Non-Functional Requirements

- **NFR1 - Maintainability (the decision layer stays window-free):** No
  signature in `src-tauri/src/window_host/mouse_report.rs`, and no signature of
  the new predicate (FR4), may take or return a winit type, an egui window
  handle, a GPU surface, a PTY, or a `term_core` mode type. Held-button state
  travels as plain bools or the existing plain `HeldButtons` value
  (mouse_report.rs:471-483). This is the module's stated invariant and is what
  keeps every unit exercisable from a bare `#[test]`.
- **NFR2 - Maintainability (decision functions stay pure):** The new predicate
  is a pure function of its plain inputs: evaluating it mutates nothing and
  performs no I/O, exactly as `drag_in_flight` (pointer_routing.rs:364-366)
  does. All state mutation stays in the existing `publish_local_drag` /
  `apply_outcome` sites; the predicate only decides.
- **NFR3 - Maintainability (test style matches the project convention):** New
  tests are bare `#[test]` functions inside the existing inline `#[cfg(test)]`
  test modules next to the code under test (test/README.md "Test File
  Organization"), named `<subject>_<scenario>_<expected>` (test/README.md "Test
  Naming Conventions"), constructed explicitly per test with no shared global
  fixture. No new test-framework dependency is added — test/README.md "Test
  Framework" states there is no `proptest` and no `criterion`.
- **NFR4 - Compatibility (build-surface invariance):** The CLI-only build
  (`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`)
  still compiles: every touched module is GUI-gated. Both Linux and Windows
  remain supported and no platform-specific API is introduced
  (.claude/rules/core-architecture.md, "Platform support": Linux and Windows,
  macOS out of scope).
- **NFR5 - Security (no new PTY bytes and no relaxation of existing caps):** The
  change is confined to local pointer-state bookkeeping and emits no additional
  bytes to the PTY. The wheel-report notch clamp
  (`bounded_wheel_report_duplicate` / `MAX_WHEEL_REPORT_NOTCHES`,
  pointer_routing.rs) is a documented security property and is untouched. No
  network surface, no persisted data, no WebView content is involved.
- **NFR6 - Performance (no hot-path cost):** The added work is one boolean read
  of `host.mouse_report_held.left` per `Focused(false)` event. Focus transitions
  are not a hot path and no per-frame or per-motion cost is introduced.

## Implementation Approach

### Architecture

**Layering (unchanged by this feature):**

```
┌──────────────────────────────────────────────────────────┐
│ event_loop.rs   winit event arms                          │
│                 Focused(false) arm (:227-282)             │
│                   held read (:262-265 boundary, FR2)      │
│                   gated publish_local_drag (:253-255, FR1)│
├──────────────────────────────────────────────────────────┤
│ pointer_routing.rs  pointer routing, local selection arms │
│                     drag_in_flight (:364-366)             │
│                     local_drag_in_flight (:364-377)       │
│                     NEW termination predicate (FR4, A3)   │
│                     publish/complete arms (:391-412,      │
│                                            :424-443)      │
├──────────────────────────────────────────────────────────┤
│ mouse_report.rs  window-free decision layer (NFR1)        │
│                  HeldButtons (:471-483)                   │
│                  clear_all                                │
│                  decide_release no-owner fallback (:935)  │
├──────────────────────────────────────────────────────────┤
│ host / app state  WindowHost.dragging,                    │
│                   WindowHost.mouse_report_held,           │
│                   App.pending_selection_anchor  (A5)      │
└──────────────────────────────────────────────────────────┘
```

**Component notes:**

- The new predicate is a sibling of `drag_in_flight` and lives in
  `pointer_routing.rs` with `pub(super)` visibility, not in `mouse_report.rs`
  (assumption A3). `tests.rs` already imports from there (tests.rs:19).
- `host.dragging`, `host.mouse_report_held` and `app.pending_selection_anchor`
  keep their current declarations; no field is added and no field's type changes
  (assumption A5).

### Data Flow

```
WindowEvent::Focused(false)                       (event_loop.rs:227-282)
  → read left-held from host.mouse_report_held.left            (FR2)
      MUST happen before clear_all
  → termination predicate(drag-in-flight signal, left_held)    (FR4, pure)
      ├─ true  (in flight AND not held)
      │     → publish_local_drag(host, &mut self.app)          (FR1, :254)
      │         clears host.dragging
      │         consumes app.pending_selection_anchor
      │         publishes to PRIMARY (+ CLIPBOARD when copy_on_select)
      └─ false (held, or not in flight)
            → publish nothing; host.dragging and the pending anchor stay (FR3)
  → mouse_report::clear_all(gesture_owner, held)               (:262-265, FR7)
  → rest of the arm unchanged                                  (FR7)

later, left button released                       (preserved-drag terminator)
  → decide_release(Left), no recorded owner (clear_all emptied it)
      drag_in_flight == true
      → Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)
                                                   (mouse_report.rs:935, FR3/FR8)
```

### API Design

No network or IPC API is introduced. The internal surface change is:

- One new `pub(super)` pure predicate in `pointer_routing.rs`, alongside
  `drag_in_flight` (:364-366), taking the drag-in-flight signal (or its two
  constituent bools) and the left-held bool, and returning whether focus loss
  terminates (FR4, assumption A3).
- `drag_in_flight` and `local_drag_in_flight` (pointer_routing.rs:364-377) keep
  their current signatures and meaning; the new predicate composes with them
  (FR4).
- `mouse_report::clear_all` keeps its signature and its position in the arm
  (FR2, FR7).

### Database Schema

Not applicable. This feature persists no data and changes no schema.

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/window_host/event_loop.rs`: the `Focused(false)` arm
  (:227-282), the gated composition (:253-255), the `clear_all` call
  (:262-265), `host.current_mods` reset (:238), `host.pointer_buttons_down`
  reset (:243).
- `src-tauri/src/window_host/pointer_routing.rs`: `drag_in_flight` (:364-366),
  `local_drag_in_flight` (:364-377), the local selection arms (:391-412,
  :424-443), the wheel notch clamp (`bounded_wheel_report_duplicate` /
  `MAX_WHEEL_REPORT_NOTCHES`), and the new predicate's home (A3).
- `src-tauri/src/window_host/mouse_report.rs`: `HeldButtons` (:471-483),
  `clear_all`, and `decide_release`'s no-owner `drag_in_flight` fallback (:935).
- `src-tauri/src/window_host/mod.rs`: `WindowHost.dragging`,
  `WindowHost.mouse_report_held`, `WindowHost.mouse_report_gesture_owner` (A5).
- `src-tauri/src/app.rs`: `App.pending_selection_anchor` (A5).
- `src-tauri/src/window_host/tests.rs`: the structural assertion
  `focus_loss_arm_never_calls_the_fold_click_toggle` (:1931-1949), the existing
  clear test (:1835-1904), the `drag_in_flight` truth-table sibling
  (:1915-1921), and the import site (:19).
- `feature-docs/mouse-report-reset-active-gesture/SPEC.md`: the document FR6
  supersedes three items of.

**External Dependencies:**

- None added. No new test-framework dependency (NFR3).

### File Structure

```
src-tauri/src/
├── app.rs                        # App.pending_selection_anchor (A5)
└── window_host/
    ├── mod.rs                    # WindowHost.dragging / mouse_report_held (A5)
    ├── mouse_report.rs           # HeldButtons, clear_all, no-owner fallback (FR3)
    ├── pointer_routing.rs        # NEW termination predicate (FR4, A3)
    ├── event_loop.rs             # FR1, FR2, FR3, FR7
    └── tests.rs                  # structural needle set (FR5, A4) + TS-1
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/focus-loss-drag-termination/**`
- `test-docs/focus-loss-drag-termination/**`

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

- [ ] **TS-1 — Termination predicate truth table** (AC-6; FR4, NFR1, NFR2):
      Truth table of the new pure termination predicate: for every combination
      of (drag in flight, left held) assert the returned decision — terminate
      only when drag in flight is true AND left held is false. Bare `#[test]`,
      no winit window, sibling in style to
      `drag_in_flight_is_true_whenever_either_input_is_true`
      (tests.rs:1915-1921).
- [ ] **TS-2 — Structural assertion pins the new composed form** (AC-7; FR5):
      Extend `focus_loss_arm_never_calls_the_fold_click_toggle`
      (tests.rs:1931-1949): the `include_str!("event_loop.rs")` needles are
      replaced with the new composed form (the predicate call and the
      held-button read as they are actually spelled in the arm), and the
      negative `handle_fold_click` assertion is kept. Guard against a needle
      that a prefix match would satisfy stale-ly.
- [ ] **TS-3 — No-owner left release terminates the preserved drag** (AC-5;
      FR3, FR8): `decide_release(Left)` against records whose Left slot is
      `None` with `drag_in_flight = true` yields
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)`; with
      `drag_in_flight = false` it stays `Disposition::Nothing`. This is already
      covered by `no_owner_left_release_completes_selection_only_when_drag_in_flight`
      (mouse_report.rs:3144-3162) — the scenario asserts that test still passes
      unchanged, extended with a comment tying it to this feature's FR3
      delegation.
- [ ] **TS-4 — The three focus-loss state shapes** (AC-1, AC-2, AC-3; FR1,
      FR2): Drive the extracted predicate with the three state shapes
      AC-1/AC-2/AC-3 describe (in-flight + held; in-flight + not held; not in
      flight, both held values) and assert the decision matches. The end-to-end
      arm cannot be driven without a winit window, so the arm's fidelity to this
      predicate is pinned structurally by TS-2 rather than behaviourally.
- [ ] **TS-5 — `clear_all` still empties both records** (AC-4, AC-9; FR7):
      `mouse_report::clear_all` still empties both the gesture-ownership record
      and the held-button record, and a decision made from post-clear records
      reports nothing and takes no local arm — the existing
      `focus_loss_clear_all_empties_gesture_and_held_records_so_the_next_decision_starts_fresh`
      (tests.rs:1835-1904) and
      `ac2_clear_all_empties_gesture_and_held_button_records`
      (mouse_report.rs:2026-2040) pass unchanged.

### Build Checks

- [ ] **TS-6 — CLI-only feature gate intact** (AC-10; NFR4):
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      succeeds, confirming the CLI-only feature gate is intact.

### Integration Tests

None. Every automated scenario in this feature is a unit-level test inside the
touched modules, plus the CLI-only build check (NFR3, TS-6).

### E2E Tests

**Existing E2E tests**: None — E2E automation does not exist in this project
(test/README.md "E2E Tests": none at the moment; assumption A6).
**Run command**: Not detected.

- [ ] **TS-7 — Manual reproduction** (AC-1, AC-2): User-executed reproduction
      against the release binary: (a) start a left drag over the grid, keep the
      button down, cause the window to lose focus (raise another window /
      trigger a notification), return focus and continue moving — the selection
      must still be extending and PRIMARY must be unchanged until release; (b)
      start and finish a left drag, then cause focus loss with the button up —
      behaviour is as today.

### Test Notes

- **The `Focused(false)` arm itself is not behaviourally testable.** Driving the
  arm end to end requires a winit window, which the test layer has none of. The
  consequence, recorded by TS-4: the arm's fidelity to the extracted predicate
  (FR4) is pinned **structurally** by TS-2's source-text assertion over
  `include_str!("event_loop.rs")`, not behaviourally. TS-1 and TS-4 prove the
  predicate's decision; TS-2 proves the arm actually calls it in the composed
  form the predicate expects, including the held-button read FR2 requires.
- **The needle set is replaced, not appended to** (assumption A4). The existing
  needles are exact source substrings of the current composition, so once the
  arm is rewritten they no longer match and the old entries would fail. Only
  literals actually present in the rewritten arm may remain, and the needle must
  not be one a prefix match would satisfy stale-ly (TS-2).

### Edge Cases

- [ ] Focus lost with a live drag and the left button held → nothing published,
      `host.dragging` and the pending anchor preserved (AC-1, FR3).
- [ ] Focus lost with a live drag and the left button not held → today's
      termination, unchanged (AC-2, FR1).
- [ ] Focus lost with no drag in flight → pure no-op for selection state and for
      PRIMARY, regardless of the held value (AC-3, FR7).
- [ ] The held value read after `clear_all` would be `false` unconditionally,
      degrading FR1 to today's unconditional termination — hence FR2's ordering
      requirement (AC-4).
- [ ] The eventual release of a preserved drag is a no-owner release, because
      `clear_all` emptied the gesture-ownership record; the
      `drag_in_flight` fallback at mouse_report.rs:935 is what terminates it
      (AC-5, FR3).
- [ ] A left release that is genuinely lost leaves `host.dragging` preserved
      with no immediate terminator. Accepted, not closed (assumption A2). The
      degradation is bounded and self-healing: the next complete left click
      anywhere in the grid produces a no-owner left release with
      `drag_in_flight == true`, which runs the fallback at mouse_report.rs:935
      and clears `host.dragging`.
- [ ] A press event recorded whose release was never delivered lets
      `host.mouse_report_held.left` report `held = true` while the button is
      physically up — the record is event-derived, not a device query
      (assumption A1).

### Performance Tests

None. The only performance statement is NFR6: one boolean read per
`Focused(false)` event, on a path that is not hot.

## Security Considerations

- **New attack surface:** None. The change is confined to local pointer-state
  bookkeeping and emits no additional bytes to the PTY (NFR5).
- **Wheel-report notch clamp (NFR5):** `bounded_wheel_report_duplicate` /
  `MAX_WHEEL_REPORT_NOTCHES` (pointer_routing.rs) is a documented security
  property and is untouched.
- **Report byte invariance (FR7):** No DEC mouse-report byte changes.
- **Authentication / Authorization / Input validation / Data protection / XSS /
  SQL injection / CSRF:** Not applicable — no network surface, no persisted
  data, no WebView content is involved (NFR5).

## Error Handling

No new error codes or error responses are introduced. The failure modes this
feature addresses are state-machine conditions, handled as follows. This table
replaces the prior SPEC's "Focus lost with a live left drag" row (FR6).

| Condition | Handling |
|---|---|
| Focus lost with a live left drag, left button **held** | Publish nothing; keep `host.dragging` true and `app.pending_selection_anchor` in place (FR3, AC-1) |
| Focus lost with a live left drag, left button **not held** | `publish_local_drag` runs: clear `host.dragging`, consume the pending anchor, publish to PRIMARY (+ CLIPBOARD when `copy_on_select`) (FR1, AC-2) |
| Focus lost with no drag in flight | Pure no-op for selection state and for PRIMARY, whatever the held value (FR1, FR7, AC-3) |
| Left release of a preserved drag | No-owner release; `decide_release`'s `drag_in_flight` fallback (mouse_report.rs:935) takes the local completion arm (FR3, FR8, AC-5) |
| Left release genuinely lost | Accepted residual leak; self-heals on the next complete left click in the grid (assumption A2, FR8) |
| Held record reports `held = true` while the button is physically up | Accepted: the record is event-derived, not a device query (assumption A1) |

## Performance Optimization

Not applicable beyond NFR6. The added work is one boolean read of
`host.mouse_report_held.left` per `Focused(false)` event; focus transitions are
not a hot path and no per-frame or per-motion cost is introduced.

## Success Criteria

- [ ] All functional requirements (FR1-FR8) are implemented and tested
- [ ] All non-functional requirements (NFR1-NFR6) are satisfied
- [ ] All acceptance criteria (AC-1..AC-10) are satisfied
- [ ] All test scenarios (TS-1..TS-7) pass
- [ ] The supersession of the prior feature's FR3 / AC-3 / state-table row is
      recorded in prose with refreshed line references (FR6, AC-8)
- [ ] The CLI-only build (`--no-default-features`) still compiles (NFR4, TS-6)
- [ ] The decision layer remains window-free (NFR1) and the new predicate
      remains pure (NFR2)
- [ ] Documentation is complete
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional and non-functional requirement is `status: ok`; no
requirement carries `status: tbd`.

One follow-up is recorded rather than resolved here: assumption A2 states that a
separate follow-up task is to be filed for a non-event-derived terminator for
the residual lost-release leak. That task is out of this feature's scope (FR8).

## Implementation Phases (if applicable)

Not applicable. The change is a single coherent narrowing of one condition plus
the predicate extraction and the structural-test update; splitting it into
phases would leave the arm and its structural assertion disagreeing in an
intermediate state (FR5, A4).

## Assumptions

Every assumption below originates in the resolved requirements analysis; the
full reasoning is recorded in
`feature-docs/focus-loss-drag-termination/REQUIREMENTS.md` section 14.

| ID | Impact | Reversible | Assumption |
|---|---|---|---|
| A1 | medium | yes | "The left button is held" means the host's event-derived held-button record `host.mouse_report_held.left` (the plain `HeldButtons` value, mouse_report.rs:471-483), not a query of the physical device. Carried forward from the Codex consultation caveat on answer `requirement.fr3-termination-scope`: if a press event was recorded but its release was never delivered, the record can report `held = true` while the button is physically up. |
| A2 | medium | yes | The residual lost-release leak is accepted, not closed. If a left release is genuinely lost, FR3 preserves `host.dragging` with no immediate terminator. Accepted because the two candidate extra terminators do not work: (i) a `Focused(true)` or first-motion-after-refocus check would read a `host.mouse_report_held` that `clear_all` (event_loop.rs:262-265) has already zeroed, so it would observe `left == false` unconditionally and degrade to today's unconditional termination — reintroducing the bug; and (ii) a separate selection-side press record would stay `held = true` forever exactly on the lost-release path, buying extra state and no coverage. The degradation is bounded and self-healing: the next complete left click anywhere in the grid produces a no-owner left release with `drag_in_flight == true`, which runs the fallback at mouse_report.rs:935 and clears `host.dragging`. A separate follow-up task is to be filed for a non-event-derived terminator. |
| A3 | low | yes | The new predicate lives alongside `drag_in_flight` in `src-tauri/src/window_host/pointer_routing.rs` (:364-366) as its sibling, with `pub(super)` visibility, rather than in `mouse_report.rs` — matching where the existing drag-in-flight pair already lives and where tests.rs already imports it from (tests.rs:19). |
| A4 | low | yes | The structural test's needle list (tests.rs:1939-1948) must be REPLACED, not merely extended: the existing needles are exact source substrings of the current composition, so once the arm is rewritten they no longer match and the old entries would fail. Only literals actually present in the rewritten arm may remain. |
| A5 | low | yes | `host.dragging`, `host.mouse_report_held` and `app.pending_selection_anchor` keep their current declarations (`src-tauri/src/window_host/mod.rs`, `src-tauri/src/app.rs`); this feature adds no field and changes no field's type. |
| A6 | low | yes | No E2E automation exists in this project (test/README.md "E2E Tests": "None at the moment. There is no docker-compose.e2e.yml and no e2e-tests/ directory"), so end-to-end confirmation of this feature is the user-executed manual scenario TS-7. Pinned by the absence of any resolved E2E path in this dispatch (`resolved_input_paths.e2e` is empty). |

## Design Step

Skipped. No user-visible surface change. The feature narrows one boolean
condition in the winit `Focused(false)` arm and extracts a pure predicate in
three GUI-internal Rust modules (event_loop.rs, pointer_routing.rs, tests.rs).
It introduces no new UI element, no design token, no layout, copy, colour or
interaction-affordance change; the only observable difference is that an
in-progress drag survives a focus steal. `doc/UI-DESIGN-GUIDELINES.yaml` and
both of its mirrors (`src-tauri/src/ui/md3.rs`,
`src-tauri/web-shared/styles.css`) are untouched, so the drift tests in
`ui::dialog::tests` are unaffected. Gate `create-spec.design-step` was resolved
with `decide_autonomously`, accepting this recommendation.

## References

- Requirements document: `feature-docs/focus-loss-drag-termination/REQUIREMENTS.md`
- Superseded items (FR6): `feature-docs/mouse-report-reset-active-gesture/SPEC.md`
- Event loop arms: `src-tauri/src/window_host/event_loop.rs`
- Pointer routing, `drag_in_flight` and the new predicate's home:
  `src-tauri/src/window_host/pointer_routing.rs`
- Decision layer, `HeldButtons`, `clear_all`, no-owner release fallback:
  `src-tauri/src/window_host/mouse_report.rs`
- Host state: `src-tauri/src/window_host/mod.rs`
- App state: `src-tauri/src/app.rs`
- Structural and clear tests: `src-tauri/src/window_host/tests.rs`
- Test conventions and E2E status: `test/README.md`
- Build surface and platform support: `.claude/rules/core-architecture.md`
