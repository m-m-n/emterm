# Feature: mouse-report-guard-local-arms

## Overview

The SC-8 grid-ownership guard introduced by the mouse-reporting feature answers
every event over a chrome-guarded region with one uniform rejection arm:
`Disposition::Nothing`. The perform step drops that value at `None => {}`, so the
local pointer behaviour those regions used to have disappears — a wheel notch no
longer moves the scrollback and a middle press no longer pastes PRIMARY. This
feature replaces the uniform rejection with an explicit per-region decision that
names the pre-regression local arm where one existed, keeps `Nothing` where the
event never reached the decision unit before, and pins the arm identity in tests
so the regression cannot recur silently.

Requirements source:
`feature-docs/mouse-report-guard-local-arms/REQUIREMENTS.md`.

## Objectives

- Restore, on every chrome-guarded region, the local pointer behaviour the
  mouse-reporting SC-8 grid-ownership guard silently removed — wheel notches move
  the scrollback again, and a middle press pastes PRIMARY again — so the
  mouse-reporting feature's own AC12 ("emits no report and retains its current
  local behaviour") actually holds.
- Close the test gap that let the regression through: the guarded-region tests
  assert only that no report bytes were produced, which cannot distinguish
  `Disposition::Nothing` from a named local arm. After this change the tests pin
  the local-arm identity per region.
- Replace the current "one uniform rejection arm, harmless because upstream early
  returns make it unreachable" design with an explicit per-region decision, so a
  future reordering of the chrome guards cannot turn tab-bar horizontal
  scrolling, mux-sidebar list scrolling or profile-selector list scrolling into
  terminal scrolling.

## User Stories

### US1: Get local pointer behaviour back on the chrome-guarded regions
As a user of eMterm's local pointer behaviour, I want a wheel notch and a middle
press over the status bar, the scrollbar overlay, the CSD title bar and the
resize bands to do what they did before mouse-reporting landed, so that the
feature's own "retains its current local behaviour" promise actually holds.

**Acceptance Criteria:**
- [ ] AC1 — A wheel notch over the bottom status-bar strip names `Local(LocalArm::ScrollScrollback)` and the scrollback moves, with tracking inactive and with tracking active alike. (FR1, NFR5)
- [ ] AC2 — A wheel notch over the right-edge scrollbar overlay names `Local(LocalArm::ScrollScrollback)` and the scrollback moves. (FR1)
- [ ] AC3 — A wheel notch over the CSD title-bar band (`y < TITLE_BAR_HEIGHT`) names `Local(LocalArm::ScrollScrollback)` and the scrollback moves. (FR1, FR9)
- [ ] AC4 — A wheel notch over any of the four CSD edge-resize hot zones names `Local(LocalArm::ScrollScrollback)` and the scrollback moves. (FR1)
- [ ] AC5 — On the alternate screen with the DECSET 1007 mode bit and `alternate_scroll_enabled` both on, a wheel notch over each of AC1-AC4's regions names `Local(LocalArm::TranslateToArrowBytes)` instead of `ScrollScrollback`. (FR1, CF6)
- [ ] AC8 — With `middle_click_paste` on, a middle press over the bottom status strip names `Local(LocalArm::PastePrimary)` and PRIMARY is pasted. (FR3)
- [ ] AC9 — With `middle_click_paste` on, a middle press over the scrollbar overlay, over the mux sidebar, and over a resize hot zone that does not overlap the top strip each names `Local(LocalArm::PastePrimary)`. (FR3)
- [ ] AC10 — A middle press over the CSD title-bar band, over the tab-bar band, or while the profile selector is visible names `Nothing`; so does any middle press when `middle_click_paste` is off. (FR3)
- [ ] AC11 — A left press over any guarded region names `Nothing` — never `BeginSelectionDrag` and never `OpenHoveredLink`; a right press over any guarded region names `Nothing`. (FR4)
- [ ] AC12 — A press over a guarded region records no gesture owner, and the matching left release therefore names no arm and appends no bytes — in particular it never reaches `CompleteSelectionAndPublishToPrimary`. (FR5)
- [ ] AC13 — For every guarded region x every button identity x every event kind x both encodings x every tracking-mode combination, no report bytes are produced — the parent feature's AC12 reporting prohibition is unchanged. (FR7)
- [ ] AC14 — Motion over a guarded region names `Nothing` with `updates == RecordUpdates::default()`; the next motion over the grid at the previously-cached cell still reports. (FR6, FR8)

### US2: Have the tests pin the local arm, not merely the absence of bytes
As a developer maintaining the pointer path, I want the guarded-region test to
assert the decided `Disposition` per region, so that reverting any arm to
`Nothing` fails the suite instead of passing review and CI as the original
regression did.

**Acceptance Criteria:**
- [ ] AC16 — The guarded-region test asserts the full decided `Disposition` per region, so swapping any arm back to `Nothing` fails the suite. A test suite that only asserts absence of bytes does not satisfy this criterion. (BO2, CF5)

### US3: Keep egui's own scroll regions safe from a future guard reordering
As a user of eMterm's egui chrome, I want the tab strip, the mux sidebar and the
profile selector to keep their own scrolling regardless of where the chrome
guards sit in the routing order, so that a future reordering cannot turn those
gestures into terminal scrolling.

**Acceptance Criteria:**
- [ ] AC6 — A wheel notch over the tab-bar band names `Disposition::Nothing`, and the tab strip keeps scrolling horizontally via `handle_mouse_wheel`'s existing egui forward — the terminal does not scroll. (FR2, FR9)
- [ ] AC7 — A wheel notch over the mux sidebar names `Nothing` (the sidebar's window list keeps scrolling), and a wheel notch while the profile selector is visible names `Nothing` (the modal's list keeps scrolling). (FR2)
- [ ] AC15 — `point_belongs_to_grid` returns false whenever the title-bar bool or the tab-bar bool is true, and its answer for every other input combination is unchanged from before the decomposition. (FR9)

## Technical Requirements

### Functional Requirements

- **FR1 — Wheel over a damaged guard region names the pre-regression wheel consumer:** When `point_belongs_to_grid` is false, `decide_wheel_event` decides the disposition by calling `wheel_consumer(tracking_active = false, inputs.mods.shift, inputs.on_alt_screen, inputs.alt_scroll_mode_bit, inputs.alt_scroll_setting)` and mapping `TranslateToArrows` -> `Local(LocalArm::TranslateToArrowBytes)` and `ScrollScrollback` -> `Local(LocalArm::ScrollScrollback)`. `tracking_active` is passed as `false` unconditionally — the region is not grid-owned, so the tracking application has no claim on the notch. `ReportToApplication` is therefore unreachable in this branch. A fixed `ScrollScrollback` is explicitly rejected: it would break alternate-screen arrow translation (CF6).
- **FR2 — Guard regions consumed by an upstream early branch keep `Nothing` on the wheel path:** The wheel arm from FR1 is given only to the regions whose wheel events actually reach `decide_wheel_event`: the CSD title-bar band, the bottom status strip, the right-edge scrollbar overlay, the CSD resize hot zone. The regions `handle_mouse_wheel` already consumes and returns from — the tab-bar band, the mux sidebar, and any position while the profile selector is visible (CF1) — decide `Disposition::Nothing`.
- **FR3 — Middle press over a guard region pastes PRIMARY in the four regions it previously reached:** When `point_belongs_to_grid` is false, the button is `Middle`, and `middle_click_paste_enabled` is true, `decide_press` names `Local(LocalArm::PastePrimary)` for the bottom status strip, the scrollbar overlay, the mux sidebar, and the part of the CSD resize hot zone that does not overlap the top strip. It names `Nothing` for the CSD title-bar band, the tab-bar band, and while the profile selector is visible — a middle press never reached `decide_press` from those three before the regression (CF2), so giving them `PastePrimary` would be new behaviour, not a repair. With `middle_click_paste_enabled` false the answer is `Nothing` everywhere. Note the structural point: the region set that a press reaches differs from the set a wheel reaches (FR2) — the mux sidebar is press-reachable but wheel-unreachable, the title-bar band is the reverse.
- **FR4 — Left and right presses over a guard region name no local arm:** For a rejected position, a `Left` press decides `Nothing` — never `BeginSelectionDrag`, never `OpenHoveredLink` — and a `Right` press decides `Nothing`, in every region. A left press on the CSD resize band is a native window resize handled by the existing early guard (`pointer_routing.rs:429-436`) and that ordering is not changed.
- **FR5 — A rejected press still records no gesture owner:** The outcome of an SC-8-rejected press carries `updates.gesture == None` — neither `Record(_, GestureOwner::Local)` nor `Record(_, GestureOwner::Report)` — even now that the press may name a local arm. Recording `Local` would route the matching left release into `CompleteSelectionAndPublishToPrimary`, completing a selection that was never begun. SC-9's documented post ("a press SC-8 rejected records no owner") is preserved verbatim.
- **FR6 — Motion over a guard region stays `Nothing`:** `decide_motion_event`'s rejection arm is unchanged. `handle_pointer_moved` performs its local work (egui forwarding, sidebar hover, resize hint, link hover, selection-drag extension) before the decision runs, so a motion local arm would double-execute work already done.
- **FR7 — No rejected position ever reports:** For every region, every button identity, both encodings and every tracking mode combination, a position `point_belongs_to_grid` rejects produces `Disposition::Report` on none of the three paths. Only the local half of the answer changes; mouse-reporting FR10 / AC12's reporting prohibition is intact.
- **FR8 — A rejected event's record updates stay as they are:** The rejected-position outcome keeps `updates == RecordUpdates::default()` (`reset: false`, `built_for_tab: None`, `cache_cell: None`, `gesture: None`). Restoring a local arm changes the `disposition` field only. This preserves the SC-4 property that a suppressed motion advances no cached cell (parent AC5 / TS-21).
- **FR9 — `GridOwnershipInputs.in_top_strip` decomposes into two region bools:** `in_top_strip` is replaced by two independent bools — one for the CSD title-bar band, one for the tab-bar band. `grid_ownership_inputs` (`pointer_routing.rs:116-123`) populates them from the same geometry it uses today: title-bar band = `position.y < TITLE_BAR_HEIGHT`, tab-bar band = `TITLE_BAR_HEIGHT <= position.y < TITLE_BAR_HEIGHT + effective_tab_bar_height(app.show_tab_bar)` — the identical boundary the `handle_mouse_wheel` tab-bar guard uses (CF1), so the two can never disagree. `point_belongs_to_grid` keeps its exact meaning and truth table: any region bool true, or the profile selector visible, yields false.
- **FR10 — Region-specific dispatch, with a stated precedence for overlapping regions:** The rejection answer is computed per region, not as one uniform value that happens to be harmless because upstream guards make it unreachable. When more than one region bool is true for the same position, the suppressing regions win: profile-selector-visible first, then the tab-bar band and the mux sidebar, then the arm-bearing regions. This makes the answer deterministic for the overlaps that exist in practice (the resize band's top edge inside the title-bar band, its bottom edge inside the status strip, its right edge inside the scrollbar overlay) and guarantees no event that egui is already consuming can additionally move the terminal.

### Non-Functional Requirements

- **NFR1 - Testability / purity:** The decide units stay pure functions. `decide_wheel_event`, `decide_button_event` and their helpers keep SC-10 property 3: they take and return plain values only — no winit type, no `term_core` type, no window handle, no GPU surface, no PTY in any signature — and mutate no record at decision time (every record change continues to travel in `RecordUpdates` for `apply_outcome` to apply). Every new test must be runnable from a bare `#[test]` with no window, surface or PTY constructed.
- **NFR2 - Maintainability / type surface:** No new `Disposition` variant. The rejected-position answer is expressed with the existing `Local(..)` and `Nothing` values. The reviewer's alternative (b), adding `Disposition::NotGridOwned`, is not adopted: it would blur the decide/perform boundary and weaken SC-10's contract that one value names one concrete action. `LocalArm` likewise gains no variant.
- **NFR3 - Compatibility:** Guard order and SC-8 semantics unchanged. The early-return order inside `handle_pointer_button` and `handle_mouse_wheel` is not reordered, and no guard is removed or added. `point_belongs_to_grid` keeps its meaning ("does the terminal grid own this point") and its truth table; only what the callers do with a `false` answer changes.
- **NFR4 - Performance:** No new cost or logging on the pointer hot path. The change adds no allocation, no lock acquisition and no log line per pointer event. `wheel_consumer` is a pure branch over five bools; the region dispatch is a match over bools already present in the input struct.
- **NFR5 - Scope:** The repair is independent of mouse-reporting being enabled. Because SC-8 is consulted ahead of the tracking-active read, the defect occurs whether or not any tracking mode is active. The restored behaviour must hold identically in both states, and the tests must cover both.
- **NFR6 - Regression safety:** Existing mouse-reporting behaviour is not regressed. Every currently-green assertion in `mouse_report.rs`'s inline test module continues to pass, with the single intended exception of strengthening the guarded-region assertions from "no bytes" to "this exact disposition" (TS9). The Rust suite is run as `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`.

## Implementation Approach

### Architecture

**System Architecture:**

```
┌──────────────────────────────────────────────────────────────┐
│ winit event loop (pointer button / motion / wheel)           │
├──────────────────────────────────────────────────────────────┤
│ window_host::pointer_routing — existing early guards, order  │
│ unchanged (NFR3); grid_ownership_inputs builds the region    │
│ bools (FR9)                                                  │
├──────────────────────────────────────────────────────────────┤
│ window_host::mouse_report — SC-10 decide units, pure (NFR1): │
│   decide_wheel_event   : FR1, FR2, FR10                      │
│   decide_press         : FR3, FR4, FR5, FR10                 │
│   decide_motion_event  : FR6 (unchanged)                     │
│   point_belongs_to_grid: FR9 (meaning unchanged)             │
├──────────────────────────────────────────────────────────────┤
│ window_host::input_translate::wheel_consumer — reused as-is  │
│ with tracking_active = false (FR1, CF6)                      │
├──────────────────────────────────────────────────────────────┤
│ perform step in pointer_routing — executes the named arm     │
│ (:983 wheel, :690 button); Nothing still falls to None => {} │
└──────────────────────────────────────────────────────────────┘
```

**Component Diagram:**

```
pointer_routing::grid_ownership_inputs  : populates the two region bools that
                                          replace in_top_strip (FR9, CF4)
mouse_report::point_belongs_to_grid     : same meaning and truth table (FR9, NFR3)
mouse_report::decide_wheel_event        : per-region wheel answer (FR1, FR2, FR10)
mouse_report::decide_press              : per-region press answer (FR3, FR4, FR5, FR10)
mouse_report::decide_motion_event       : rejection arm untouched (FR6)
mouse_report::decide_release            : untouched; never consults SC-8 (CF3)
input_translate::wheel_consumer         : reused with tracking_active = false (FR1)
pointer_routing perform step            : unchanged; executes Local(..) arms (FR8)
```

### Data Flow

```
winit pointer event
  → early guards in pointer_routing (order unchanged, NFR3)
      wheel : profile selector / tab-bar band / mux sidebar consume + return (CF1)
      button: left-only guards + unconditional top-strip / profile-selector (CF2)
  → grid_ownership_inputs → two region bools + the other region bools (FR9)
  → point_belongs_to_grid → false
  → per-region dispatch with overlap precedence (FR10)
      wheel  : arm-bearing region → wheel_consumer(tracking_active = false) (FR1)
               suppressing region → Nothing (FR2)
      press  : Middle + paste enabled + press-reachable region → PastePrimary (FR3)
               Left / Right / paste disabled → Nothing (FR3, FR4)
      motion : Nothing (FR6)
  → RecordUpdates::default() in every case (FR5, FR8)
  → perform step executes the named Local arm; Report is never produced (FR7)
```

### API Design

No programmatic API is added. The change is internal to
`src-tauri/src/window_host/`. Two internal shapes change:

**`GridOwnershipInputs` (FR9):**

```
before: in_top_strip: bool          // title-bar band OR tab-bar band (CF4)
after : <title-bar band bool>       // position.y < TITLE_BAR_HEIGHT
        <tab-bar band bool>         // TITLE_BAR_HEIGHT <= position.y
                                    //   < TITLE_BAR_HEIGHT
                                    //     + effective_tab_bar_height(app.show_tab_bar)
```

`point_belongs_to_grid`'s signature and truth table are unchanged: any region
bool true, or the profile selector visible, yields false (FR9, AC15).

**Rejected-position disposition table (FR1, FR2, FR3, FR4, FR6, FR10):**

```
region                          wheel                         middle press      left/right press   motion
CSD title-bar band              Local(wheel_consumer arm)     Nothing           Nothing            Nothing
tab-bar band                    Nothing                       Nothing           Nothing            Nothing
bottom status strip             Local(wheel_consumer arm)     Local(PastePrimary)  Nothing         Nothing
scrollbar overlay               Local(wheel_consumer arm)     Local(PastePrimary)  Nothing         Nothing
mux sidebar                     Nothing                       Local(PastePrimary)  Nothing         Nothing
CSD resize hot zone             Local(wheel_consumer arm)     Local(PastePrimary)* Nothing         Nothing
profile selector visible        Nothing                       Nothing           Nothing            Nothing

* the part that does not overlap the top strip (FR3)
  "wheel_consumer arm" = ScrollScrollback, or TranslateToArrowBytes on the
  alternate screen with the 1007 mode bit and the setting both on (FR1, CF6)
  middle press answers are Nothing everywhere when middle_click_paste is off (FR3)
  overlapping regions resolve by FR10's precedence
```

### Database Schema

Not applicable — the feature persists nothing and introduces no settings field.
The only state it touches is the field composition of `GridOwnershipInputs`
(FR9), a value built per pointer event.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/window_host/mouse_report.rs`: the three SC-10 decide units and
  `point_belongs_to_grid`, plus the inline test module that holds every automated
  scenario (FR1-FR10, NFR1).
- `src-tauri/src/window_host/pointer_routing.rs`: `grid_ownership_inputs` (FR9),
  the early guards whose order must not change (NFR3), and the perform step that
  executes the named arms (FR8).
- `src-tauri/src/window_host/input_translate.rs`: `wheel_consumer`, reused
  unchanged with `tracking_active = false` (FR1, CF6).

**External Dependencies:**
- None. No crate is added, and no test framework crate is added — the automated
  scenarios are bare `#[test]` functions in the existing inline module (NFR1).

### File Structure

```
src-tauri/src/window_host/
├── mouse_report.rs        # FR1-FR8, FR10 decide-unit answers; FR9 GridOwnershipInputs
│                          # shape; inline #[cfg(test)] mod tests holds TS1-TS11
└── pointer_routing.rs     # FR9 grid_ownership_inputs population;
                           # guard order and perform step unchanged (NFR3, FR8)
```

Untouched by this change (NFR3): the early-return order in
`handle_pointer_button` and `handle_mouse_wheel`, `decide_release`,
`decide_motion_event`'s rejection arm (FR6), and `input_translate::wheel_consumer`
itself.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mouse-report-guard-local-arms/**`
- `test-docs/mouse-report-guard-local-arms/**`

`feature-docs/mouse-report-guard-local-arms/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mouse-report-guard-local-arms/**` covers
`test-docs/mouse-report-guard-local-arms/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites it
and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mouse-report-guard-local-arms/` directory at all; the declared
`test-docs/mouse-report-guard-local-arms/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

Harness note: there is no E2E harness in this repository and none is introduced
(`test/README.md` "E2E Tests: None at the moment."). Every automated scenario
below is a Rust unit test in the existing inline `#[cfg(test)] mod tests` of
`src-tauri/src/window_host/mouse_report.rs`, calling `decide_wheel_event` /
`decide_button_event` / `decide_motion_event` directly on plain `*EventInputs`
values built from the existing `base_wheel_inputs` / `base_button_inputs` /
`base_motion_inputs` helpers — no window, no GPU surface, no PTY. Naming follows
`<subject>_<scenario>_<expected>`. Run with
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`.

### Unit Tests

- [ ] TS1 (AC1, AC2, AC3, AC4, AC6, AC7, FR1, FR2, FR10): Wheel x region matrix, tracking inactive, main screen — for each of the seven region inputs (title-bar band, tab-bar band, bottom strip, scrollbar overlay, mux sidebar, resize hot zone, profile-selector-visible) x {WheelUp, WheelDown}, assert the exact disposition: `Local(ScrollScrollback)` for title-bar band / bottom strip / scrollbar overlay / resize hot zone, `Nothing` for tab-bar band / mux sidebar / profile selector. Location: `src-tauri/src/window_host/mouse_report.rs` — inline `#[cfg(test)] mod tests`.
- [ ] TS2 (AC1, AC13, NFR5): The same matrix with each of 1000 / 1002 / 1003 active (and both encodings) — the dispositions are identical to TS1, and `apply_outcome` appends no bytes for any cell of the matrix. Location: same inline module.
- [ ] TS3 (AC5, FR1): Wheel over each of the four arm-bearing regions with `on_alt_screen`, `alt_scroll_mode_bit` and `alt_scroll_setting` all true, tracking inactive — `Local(TranslateToArrowBytes)`. Flipping any one of the three off returns to `Local(ScrollScrollback)`. With Shift held the answer is unchanged. Location: same inline module.
- [ ] TS4 (AC8, AC9, AC10, FR3): Middle-press x region matrix with `middle_click_paste_enabled = true` — `Local(PastePrimary)` for bottom strip / scrollbar overlay / mux sidebar / resize hot zone; `Nothing` for title-bar band / tab-bar band / profile-selector-visible. Location: same inline module.
- [ ] TS5 (AC10, FR3): The same matrix with `middle_click_paste_enabled = false` — `Nothing` in every region. Location: same inline module.
- [ ] TS6 (AC11, AC12, FR4, FR5): Left-press x region matrix — every cell is `Nothing` with `updates.gesture == None`; then feeding the resulting records into a left `Release` yields `Nothing` and appends no bytes. Right-press x region matrix: every cell is `Nothing`. Location: same inline module.
- [ ] TS7 (AC14, FR6, FR8): Motion x region matrix — `Nothing` with `updates == RecordUpdates::default()`; a following grid-owned motion at the same cell still reports. Location: same inline module.
- [ ] TS8 (AC15, FR9): `point_belongs_to_grid` truth table over the decomposed inputs — false when the title-bar bool alone is true, false when the tab-bar bool alone is true, and unchanged for every other single-region and multi-region combination. Location: same inline module.
- [ ] TS9 (AC16, BO2, CF5): Strengthen (or replace) `ac3_ts19_guarded_regions_reject_every_event_kind_and_button` at `mouse_report.rs:1721` so it asserts the decided `SequenceOutcome.disposition` per (region, kind, button) cell against an expected table, in addition to `dest.is_empty()`. The test must fail if any arm is reverted to `Nothing`. Location: existing test in the same inline module, must be edited.
- [ ] TS10 (AC7, FR10): Overlap precedence — `resize_hot_zone && bottom_strip` -> the local arm; `mux_sidebar && bottom_strip` -> `Nothing`; `profile_selector_visible` combined with any arm-bearing region -> `Nothing`; `tab_bar_band && resize_hot_zone` -> `Nothing`. Location: same inline module.
- [ ] TS11 (AC1, AC3, FR9): Tab bar hidden (`effective_tab_bar_height == 0`) — the tab-bar band is empty, so a position just below `TITLE_BAR_HEIGHT` is grid-owned while a position above it is the title-bar band and still names `ScrollScrollback`. Location: same inline module.

### Integration Tests

None. The resolved test scenarios contain no integration-type scenario; coverage
is split between the unit scenarios above and the manual scenarios below.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

- [ ] The project has no E2E harness and none is introduced; the end-to-end
      coverage below is user-driven manual verification. Rationale for the
      manual split: the decide units are pure and fully unit-testable, but
      `handle_mouse_wheel` / `handle_pointer_button` take `&mut WindowHost` and
      `&mut App` and cannot be constructed in a test. The wiring from a real
      winit event through the early guards to the decide call is therefore
      confirmable only by hand.
- [ ] MS1 (AC1, AC2, AC3, AC4) — manual: Release build, scrollback filled — wheel over the bottom status bar, over the right-edge scrollbar overlay, over the CSD title bar, and over each of the four resize bands; the scrollback moves in all seven places. Repeat with a mouse-reporting application running (tracking active) — same result.
- [ ] MS2 (AC8) — manual: With `middle_click_paste` on and PRIMARY holding known text, middle-click on the status bar; the text is pasted.
- [ ] MS3 (AC6, AC7) — manual: Wheel over the tab bar still scrolls the tab strip horizontally; wheel over the mux sidebar (persistent and overlay) still scrolls the window list; with the profile selector open, wheel still scrolls the modal list only. In all three, the terminal scrollback does not move.
- [ ] MS4 (AC11) — manual: Left-press-drag starting on the status bar or the scrollbar overlay starts no terminal selection; left-press on a resize band still resizes the window.

### Edge Cases

- [ ] Tab bar hidden: the tab-bar band collapses to zero height, so the boundary between the title-bar band and the grid sits exactly at `TITLE_BAR_HEIGHT` (FR9; TS11).
- [ ] Overlapping regions: the resize band's top edge inside the title-bar band, its bottom edge inside the status strip, its right edge inside the scrollbar overlay — resolved by FR10's precedence (TS10).
- [ ] A left press inside a CSD resize hot zone whose cached `host.current_resize_dir` is `None` bypasses the early resize guard and reaches `decide_press`; it names `Nothing` (FR4, assumption A1; TS6).
- [ ] `middle_click_paste_enabled` false: every middle press over every guarded region names `Nothing` (FR3; TS5).
- [ ] Tracking active vs. inactive: the restored answers are identical in both states, because SC-8 is consulted ahead of the tracking-active read (NFR5; TS2).

### Performance Tests

No load or stress test is specified. The feature's performance constraint is NFR4
(no allocation, no lock acquisition and no log line added per pointer event),
satisfied structurally: `wheel_consumer` is a pure branch over five bools and the
region dispatch is a match over bools already present in the input struct.

## Security Considerations

The resolved requirements contain no security requirement for this feature, so
none is specified here. The change adds no input channel and no stored data: it
only changes which local arm the decide units name for a position SC-8 rejects,
and FR7 keeps the reporting prohibition intact.

## Error Handling

No error codes are introduced. The one "no action" condition is the suppressing
answer itself:

| Condition | Behaviour |
|---|---|
| A rejected position in a suppressing region (tab-bar band, mux sidebar on the wheel path, profile selector visible) | `Disposition::Nothing`; the perform step falls to `None => {}` and egui's own handling — already performed upstream — is the whole behaviour (FR2, FR10) |
| A rejected position with `middle_click_paste_enabled` false, or a left / right press, or a motion | `Disposition::Nothing` with `updates == RecordUpdates::default()` (FR3, FR4, FR6, FR8) |

### Error Flow

```
Rejected position → FR10 precedence → suppressing region?
   yes → Nothing (perform: None => {})
   no  → name the pre-regression local arm (FR1 wheel / FR3 middle press)
         → Report is never produced on any path (FR7)
```

## Performance Optimization

### Performance Goals

- No allocation, no lock acquisition and no log line added per pointer event
  (NFR4).

### Optimization Strategies

- Reuse `wheel_consumer` rather than re-deriving the wheel matrix: it is a pure
  branch over five bools (FR1, NFR4).
- Express the region dispatch as a match over bools already present in
  `GridOwnershipInputs`, so no extra geometry computation enters the hot path
  (FR9, FR10, NFR4).

### Caching Strategy

None added. `RecordUpdates::default()` is preserved for rejected events, so no
cached cell is advanced (FR8).

## Success Criteria

- [ ] All functional requirements (FR1-FR10) are implemented and tested
- [ ] All automated test scenarios (TS1-TS11) pass under
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] All manual test scenarios (MS1-MS4) are executed against a release build
- [ ] The decide units remain pure: no winit type, `term_core` type, window
      handle, GPU surface or PTY appears in any signature, and every new test
      runs from a bare `#[test]` (NFR1)
- [ ] No `Disposition` variant and no `LocalArm` variant is added (NFR2)
- [ ] The early-return order in `handle_pointer_button` and `handle_mouse_wheel`
      is unchanged and no guard is added or removed (NFR3)
- [ ] Every currently-green assertion in `mouse_report.rs`'s inline test module
      still passes, with TS9's strengthening as the single intended change (NFR6)
- [ ] Documentation is complete
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every functional requirement (FR1-FR10) has `status: resolved`; no
requirement carries a `tbd_reason`.

## Assumptions

These are the assumptions recorded with the resolved requirements; they govern
the decisions above. All four are reversible.

- **A1** (reversible): A left press inside a CSD resize hot zone whose cached `host.current_resize_dir` is `None` bypasses the early resize guard and reaches `decide_press`. At base commit 1d5af0d that press started a selection; FR4 makes it `Nothing`. Treated as intended, not as a second regression, because the task description directs it explicitly and it matches the parent feature's FR10/AC12. Flagged because it is the one place this change is not strict pre-regression parity.
- **A2** (reversible): `in_mux_sidebar` and `in_bottom_strip` are geometrically disjoint today, so FR10's precedence rule is future-proofing rather than a change of today's observable behaviour.
- **A3** (reversible): `GridOwnershipInputs` is `pub(super)` and, within the two files in read scope, is constructed only in `grid_ownership_inputs` (`pointer_routing.rs:116`) and in `mouse_report.rs`'s own tests. FR9's field split is assumed to have no construction site elsewhere in `window_host/`. The compiler will catch any missed site, so the risk is a build error, not a silent behaviour change.
- **A4** (reversible): `scroll_by_wheel_notch` and the `alt_scroll_accum` bookkeeping in `handle_mouse_wheel`'s perform step need no change: a restored `ScrollScrollback` arm from a guarded region enters the same `tracking.any_active()` branch structure as a grid-owned notch.

## Implementation Phases (if applicable)

Not applicable — the resolved requirements define no phasing. The design step is
skipped: this is a pure event-routing repair inside `window_host`, touching no UI
element, layout, design token or visual surface.

## References

- Requirements document:
  `feature-docs/mouse-report-guard-local-arms/REQUIREMENTS.md`
- `feature-docs/mouse-reporting/SPEC.md` — the parent feature, whose FR10 / AC12
  reporting prohibition FR7 preserves and whose AC12 local-behaviour promise BO1
  restores
- `feature-docs/mouse-reporting/IMPLEMENTATION.md` — the D11 "Boundary" paragraph
  and the SC-8 row (line 53) that state the contract this regression broke (CF7)
- `src-tauri/src/window_host/pointer_routing.rs` — early guards (CF1, CF2),
  `grid_ownership_inputs` (CF4), perform step (CF3)
- `src-tauri/src/window_host/mouse_report.rs` — the three uniform rejections
  (CF3), `GridOwnershipInputs` (CF4), the test that missed the regression (CF5)
- `src-tauri/src/window_host/input_translate.rs` — `wheel_consumer` (CF6)
- `.claude/rules/core-commands.md`, `.claude/rules/core-build-location.md` — the
  `CARGO_TARGET_DIR` test command named in NFR6
- `test/README.md` — "E2E Tests: None at the moment." (harness note)
