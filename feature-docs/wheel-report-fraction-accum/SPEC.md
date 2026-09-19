# Feature: wheel-report-fraction-accum

Requirements document: `feature-docs/wheel-report-fraction-accum/REQUIREMENTS.md`
(Japanese). This specification renders the same requirements for implementation;
the two documents carry identical FR / NFR / AC / TS identifiers.

## Overview

The mouse-report path floors `lines.abs()` in `wheel_report_notches`
(`src-tauri/src/window_host/input_translate.rs:472-478`), so every `|lines| < 1.0`
yields `0` and the wheel handler's `if notches != 0` guard
(`src-tauri/src/window_host/pointer_routing.rs:943-949`) writes nothing. This
feature adds a report-path fraction accumulator so consecutive sub-notch deltas
sum until a whole notch is available, while preserving the existing two-layer
`MAX_WHEEL_REPORT_NOTCHES` cap and the alternate-scroll path's independent
accumulator.

## Objectives

- High-precision wheel input (trackpad / pixel-delta) reaches a mouse-reporting
  application instead of being discarded below the one-notch threshold, so
  sub-notch scrolling is not silently dead on the report path.
- The carried fraction is confined to the gesture, tab and tracking session that
  produced it — it can never leak across a tab switch or across a tracking-mode
  release.
- The report path keeps its existing two-layer notch cap
  (`MAX_WHEEL_REPORT_NOTCHES`) as a security property, and cannot be pushed into
  a permanently poisoned state by a non-finite delta.

## User Stories

Requirements analysis produced acceptance criteria and test scenarios rather
than user stories for this feature; see "Acceptance Criteria" and "Test
Scenarios" below.

## Technical Requirements

### Functional Requirements

- **FR1 — Accumulate sub-notch wheel deltas on the report path** (status:
  resolved): The report path accumulates fractional wheel deltas across events so
  consecutive sub-notch deltas sum until a whole notch is available. Today
  `wheel_report_notches(lines)`
  (`src-tauri/src/window_host/input_translate.rs:472-478`) floors `lines.abs()`,
  so every `|lines| < 1.0` yields 0 and the wheel handler's `if notches != 0`
  guard (`src-tauri/src/window_host/pointer_routing.rs:943-949`) writes nothing at
  all — a pixel-delta device normalized to fractions of a cell row
  (`pointer_routing.rs:867-870`) can therefore never produce a mouse report. The
  accumulation mirrors the fraction bookkeeping the alternate-scroll path already
  uses via `accumulate_alt_scroll_lines` (`input_translate.rs:340-349`), but over
  a report-path store of its own (see FR7).

- **FR2 — Consume whole notches, retain the signed remainder** (status:
  resolved): Each wheel event folds its delta into the accumulator, consumes the
  integer portion of the new total as the notch count that drives the PTY write
  (`bounded_wheel_report_duplicate` at `pointer_routing.rs:946`), and stores the
  leftover fraction back. Sign is preserved in both directions exactly as
  `accumulate_alt_scroll_lines` preserves it (floor for a non-negative total, ceil
  for a negative one, `input_translate.rs:342-347`), so a direction reversal nets
  against the stored remainder rather than restarting from zero. The wheel event's
  report direction (`MouseEventKind::WheelUp` / `MouseEventKind::WheelDown`, chosen
  at `pointer_routing.rs:905-909`) must agree with the sign of the notch count
  actually consumed.

- **FR3 — Reject a non-finite delta before mutating the accumulator** (status:
  resolved): A non-finite `lines` value is rejected before the accumulator is
  mutated — early return, no store, no report — so the accumulator can never be
  driven to NaN (an inf followed by a -inf would otherwise produce NaN, after which
  every later comparison is false and the report path goes permanently silent).
  Resolves question `report-accum.clamp-non-finite` (option
  `guard_before_accumulate`). The existing non-finite early return in
  `wheel_report_notches` (`input_translate.rs:473-475`) and in
  `alternate_scroll_wheel_bytes` (`input_translate.rs:367-369`) stay as they are;
  this guard sits ahead of the accumulate step, not in place of them.

- **FR4 — Saturate the accumulated notch magnitude while still a float** (status:
  resolved): The magnitude consumed from the accumulator is saturated at
  `MAX_WHEEL_REPORT_NOTCHES` (`input_translate.rs:457`) while it is still a
  floating-point value, ahead of the float-to-integer cast — the same ordering
  `wheel_report_notches` already documents and applies
  (`input_translate.rs:464-477`), because casting an unclamped huge magnitude to
  `i32` saturates at `i32::MAX` and falsifies the cap postcondition for the window
  between the cast and any later clamp. `bounded_wheel_report_duplicate`
  (`pointer_routing.rs:737-744`) remains the independent second clamp layer
  referencing the same constant, so removing either layer individually still leaves
  the overall bound intact (task0001 D2). Resolves question
  `report-accum.clamp-non-finite` (option `guard_before_accumulate`).

- **FR5 — Reset the accumulator on tab change and on tracking release** (status:
  resolved): The report accumulator is zeroed when the active tab changes and when
  mouse tracking is released, so a carried remainder cannot cross a gesture
  boundary or a tab boundary. The reset decision is taken from the observation
  `decide_wheel_event` already computes — `tracking_active = mode_1000 || mode_1002
  || mode_1003`, `tab_changed = records.built_for_tab != Some(active_tab)`,
  `reset = !tracking_active || tab_changed`
  (`src-tauri/src/window_host/mouse_report.rs:981-983`) — and travels in the
  outcome's `RecordUpdates.reset` (`mouse_report.rs:1007-1014`), never re-derived
  host-side. Recommended seam: join the existing `RecordUpdates` seam by carrying
  the accumulator inside `MouseReportRecords` (`mouse_report.rs:421-426`,
  round-tripped by `WindowHost::mouse_report_records` / `set_mouse_report_records`,
  `mod.rs:494-507`), so `apply_outcome`'s existing `if outcome.updates.reset` branch
  zeroes it alongside `cell_cache.reset()` and `gesture_owner.clear_all()`
  (`mouse_report.rs:1027-1030`). A separate host-side reset is acceptable only if it
  is driven by `outcome.updates.reset`: it cannot recompute `tab_changed` after
  `apply_outcome` has run, because `apply_outcome` overwrites
  `records.built_for_tab` at `mouse_report.rs:1031-1033`. Ordering is load-bearing:
  the reset must be applied before the new delta is folded in, which the wheel
  handler's existing call order already gives (`apply_outcome` at
  `pointer_routing.rs:940`, notch derivation at `:943`). Interaction with FR6:
  because the rejected-notch branch returns `RecordUpdates::default()` (reset false,
  `built_for_tab` None, `mouse_report.rs:976-979`), riding this seam preserves the
  rejected-notch invariance with no additional code. Note the observation is sampled
  at pointer events, not latched: the modes are read from the active tab's core on
  every event (decision D2, `pointer_routing.rs:898-901`) and are never mirrored
  host-side, so "tracking released then re-enabled with no intervening wheel event"
  is not covered by reset alone — meeting the "re-enabling tracking starts from
  zero" half of AC-7 additionally requires the inactive-to-active transition to be
  observed (for example by carrying the last-seen tracking-active state in
  `MouseReportRecords` beside `built_for_tab`).

- **FR6 — A rejected notch neither advances nor resets the accumulator** (status:
  resolved): A wheel event the grid-ownership gate rejects
  (`point_belongs_to_grid` false, `mouse_report.rs:971-980`) leaves the accumulator
  exactly as it was — neither advanced by its delta nor reset — preserving the
  record-update invariance that branch documents by returning
  `RecordUpdates::default()`. The rejected branch's disposition
  (`rejected_wheel_disposition`, `mouse_report.rs:684-`) is unchanged.

- **FR7 — Keep the alternate-scroll accumulator separate and untouched** (status:
  resolved): The report accumulator is a distinct store from
  `WindowHost::alt_scroll_accum` (`mod.rs:237`, initialized `mod.rs:473`).
  `alt_scroll_accum`'s behaviour is unchanged, including its screen-switch reset
  `if !app.alt_screen { host.alt_scroll_accum = 0.0; }` on both of its arms
  (`pointer_routing.rs:971-973` and `:980-982`), its deliberate non-mutation on the
  tracking-active Shift+wheel branch (`pointer_routing.rs:954-962`), and the
  `MAX_ALT_SCROLL_NOTCHES` constant it is tuned against
  (`input_translate.rs:332`), which is never aliased to `MAX_WHEEL_REPORT_NOTCHES`
  (task0001 D3).

### Non-Functional Requirements

- **NFR1 — Decision units stay pure** (status: resolved): `decide_wheel_event`
  remains a pure function of its plain inputs (SC-10 property 3); it mutates no
  record, and every record change travels in the returned
  `SequenceOutcome.updates` (`mouse_report.rs:507-528`). The delta magnitude is not
  visible to it — `WheelEventInputs` (`mouse_report.rs:592-`) carries only `kind`,
  not `lines` — so the accumulate-and-consume step stays on the host side of the
  seam while the reset rides the outcome.

- **NFR2 — Bounded work and allocation per wheel event** (status: resolved): One
  wheel event performs O(1) accumulator work and at most
  `MAX_WHEEL_REPORT_NOTCHES` payload repetitions; the single capped value derives
  both the preallocation size and the repetition bound (task0001 D5,
  `pointer_routing.rs:738-742`) so they cannot diverge.

- **NFR3 — Two clamp layers, one constant** (status: resolved): Both clamp layers
  reference the same `MAX_WHEEL_REPORT_NOTCHES` constant, and neither is aliased to
  nor derived from `MAX_ALT_SCROLL_NOTCHES`.

- **NFR4 — Testable without a window or event loop** (status: resolved): The
  accumulate / clamp / reset logic is exercisable as plain values, with no
  `WindowHost`, winit event loop or GPU surface — matching the existing style of
  `src-tauri/src/window_host/tests.rs` and the inline `mod tests` blocks in
  `mouse_report.rs` / `input_translate.rs`.

## Acceptance Criteria

- [ ] **AC-1** (FR1, FR2): A run of same-direction sub-notch deltas whose sum
  reaches 1.0 produces exactly one notch of report bytes at the crossing event;
  every earlier event in the run produces no PTY write.
- [ ] **AC-2** (FR2): The leftover fraction is retained across events and is
  sign-preserving; a direction reversal nets against the stored remainder instead
  of restarting from zero, and the emitted report direction matches the sign of the
  notch count consumed.
- [ ] **AC-3** (FR3): A non-finite delta (NaN, +inf, -inf) produces no report and
  leaves the accumulator byte-identical; a subsequent finite delta behaves exactly
  as if the non-finite event had never arrived — in particular an inf followed by a
  -inf does not silence the report path.
- [ ] **AC-4** (FR4): For every possible f32 delta and every reachable accumulator
  state, the notch count handed to the duplication step has absolute value at most
  `MAX_WHEEL_REPORT_NOTCHES`, saturated while still a float, ahead of the
  float-to-integer cast.
- [ ] **AC-5** (FR4, NFR3): `bounded_wheel_report_duplicate` still caps
  independently; fed a `requested_count` above the cap it emits exactly
  `MAX_WHEEL_REPORT_NOTCHES` repetitions and allocates for no more.
- [ ] **AC-6** (FR5): A remainder accumulated while tab A is active contributes
  nothing to any report emitted after the active tab becomes B; the first wheel
  event on B reports the same bytes it would have reported from a zero accumulator.
- [ ] **AC-7** (FR5): A remainder accumulated while tracking is active is discarded
  once tracking is released, and re-enabling tracking starts accumulation from
  zero — the first wheel event of the new tracking session reports the same bytes
  it would have reported from a zero accumulator.
- [ ] **AC-8** (FR6): A wheel event rejected by the grid-ownership gate advances
  nothing and resets nothing; the accumulator and the whole `MouseReportRecords`
  value are unchanged across the event, and the outcome's updates stay equal to
  `RecordUpdates::default()`.
- [ ] **AC-9** (FR1, FR4): Whole-notch report behaviour is unchanged; the existing
  `wheel_report_notches` expectations (signed whole counts, sub-notch zero,
  non-finite zero, cap boundaries, cap sweep) still hold.
- [ ] **AC-10** (FR7): The alternate-scroll path is unchanged; `alt_scroll_accum`
  keeps its screen-switch reset, its non-mutation on the tracking-active
  Shift+wheel branch, and its own `MAX_ALT_SCROLL_NOTCHES` bound.

## Implementation Approach

### Architecture

The change is confined to wheel-input routing inside
`src-tauri/src/window_host/`. It adds no UI surface, touches no design token, and
alters nothing a user sees other than the responsiveness of an existing scroll
gesture (the design step was therefore skipped — see "Design Step" below).

**Seam:**

```
pointer event
   │
   ├─ decide_wheel_event(...)            # pure (NFR1); sees `kind`, not `lines`
   │     └─ SequenceOutcome.updates      # carries reset / built_for_tab (FR5)
   │
   ├─ apply_outcome(...)                 # pointer_routing.rs:940
   │     └─ if outcome.updates.reset  →  zero the report accumulator (FR5)
   │                                     alongside cell_cache.reset() /
   │                                     gesture_owner.clear_all()
   │
   └─ notch derivation                   # pointer_routing.rs:943 — host side
         ├─ reject non-finite `lines`    # early return, no store (FR3)
         ├─ fold delta into accumulator  # O(1) (FR1, NFR2)
         ├─ consume integer portion      # floor / ceil by sign (FR2)
         ├─ saturate magnitude as f32    # MAX_WHEEL_REPORT_NOTCHES (FR4) — clamp layer 1
         ├─ cast to integer notch count
         └─ bounded_wheel_report_duplicate(...)  # clamp layer 2 (NFR3), PTY write
```

Ordering is load-bearing: the reset (`apply_outcome`, `pointer_routing.rs:940`)
is applied before the new delta is folded in (`:943`), which the existing call
order already gives (FR5). The non-finite guard precedes the accumulator
mutation (FR3), and saturation precedes the float-to-integer cast (FR4).

### Data Flow

```
lines (f32)
  → non-finite guard (FR3) ──reject──→ no store, no report
  → report accumulator (FR1, FR2, FR5, FR7)
  → whole notches (signed) + retained fraction
  → float saturation at MAX_WHEEL_REPORT_NOTCHES (FR4)
  → i32 notch count
  → bounded_wheel_report_duplicate (FR4, NFR2, NFR3)
  → PTY bytes, direction = MouseEventKind::WheelUp / WheelDown (FR2)
```

The report accumulator's recommended home is `MouseReportRecords`
(`mouse_report.rs:421-426`), round-tripped by
`WindowHost::mouse_report_records` / `set_mouse_report_records`
(`mod.rs:494-507`). It is a distinct store from `WindowHost::alt_scroll_accum`
(`mod.rs:237`) (FR7). Reports are written to the tab the outcome names, not to
`app.active` (assumption A-6).

### Dependencies

**Internal dependencies:**

- `src-tauri/src/window_host/input_translate.rs` — `wheel_report_notches`,
  `accumulate_alt_scroll_lines`, `alternate_scroll_wheel_bytes`,
  `MAX_WHEEL_REPORT_NOTCHES` (`:457`), `MAX_ALT_SCROLL_NOTCHES` (`:332`).
- `src-tauri/src/window_host/pointer_routing.rs` — the wheel handler,
  `bounded_wheel_report_duplicate` (`:737-744`), the alternate-scroll arms
  (`:954-962`, `:971-973`, `:980-982`).
- `src-tauri/src/window_host/mouse_report.rs` — `decide_wheel_event`,
  `WheelEventInputs`, `SequenceOutcome`, `RecordUpdates`, `MouseReportRecords`,
  `apply_outcome`, `rejected_wheel_disposition`.
- `src-tauri/src/window_host/mod.rs` — `alt_scroll_accum` (`:237`, `:473`),
  `mouse_report_records` / `set_mouse_report_records` (`:494-507`).

**External dependencies:** none beyond the crate's existing dependency set.

### File Structure

```
src-tauri/src/window_host/
├── input_translate.rs   # notch conversion, cap constants, alt-scroll fraction helper
├── pointer_routing.rs   # wheel handler, duplication cap, alternate-scroll arms
├── mouse_report.rs      # decision units, RecordUpdates / MouseReportRecords, apply_outcome
├── mod.rs               # WindowHost state (alt_scroll_accum, records accessors)
└── tests.rs             # existing wheel_report_notches tests (1499-1580)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/wheel-report-fraction-accum/**`
- `test-docs/wheel-report-fraction-accum/**`

`feature-docs/wheel-report-fraction-accum/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/wheel-report-fraction-accum/**` covers
`test-docs/wheel-report-fraction-accum/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites
it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/wheel-report-fraction-accum/` directory at all; the declared
`test-docs/wheel-report-fraction-accum/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

All scenarios below are exercisable as plain values, with no `WindowHost`,
winit event loop or GPU surface (NFR4).

### Unit Tests

- [ ] **TS-1** (AC-1): Sub-notch accumulation crossing — feed 0.4, 0.4, 0.4 and
  assert no report for the first two and exactly one notch for the third.
- [ ] **TS-2** (AC-2): Remainder retention and reversal — after 1.5 (one notch
  consumed, 0.5 held), feed -0.7 and assert the net total is -0.2 with no notch
  emitted; then feed -0.9 and assert exactly one downward notch.
- [ ] **TS-5** (AC-5): Independent duplication cap —
  `bounded_wheel_report_duplicate(payload, u32::MAX)` yields exactly
  `MAX_WHEEL_REPORT_NOTCHES` repetitions.
- [ ] **TS-9** (AC-9): Whole-notch regression guard — the existing
  `wheel_report_notches` tests (`src-tauri/src/window_host/tests.rs:1499-1580`)
  still pass unchanged.
- [ ] **TS-10** (AC-10): Alternate-scroll path unchanged —
  `accumulate_alt_scroll_lines` keeps its `(whole, frac)` contract, and the
  tracking-active Shift+wheel branch still leaves `alt_scroll_accum` untouched.

### Integration Tests

- [ ] **TS-6** (AC-6): Tab-change reset — accumulate 0.9 with
  `built_for_tab = Some(A)`, then decide a wheel event with `active_tab = B`;
  assert the outcome carries `reset` true and `built_for_tab` `Some(B)`, that
  applying it zeroes the accumulator, and that the bytes written for B's event are
  those of a zero-accumulator 0.9 delta (i.e. none).
- [ ] **TS-7** (AC-7): Tracking-release reset and re-enable — accumulate 0.9 with a
  tracking mode active; observe a wheel event with every tracking mode off and
  assert the outcome carries `reset` true and the accumulator is zeroed. Then
  re-enable tracking on the same tab and assert the first wheel event of the new
  session accumulates from zero, including the "release then re-enable with no
  intervening wheel event" ordering.
- [ ] **TS-8** (AC-8): Rejected-notch invariance — with `point_belongs_to_grid`
  false, assert `decide_wheel_event` returns `updates == RecordUpdates::default()`
  and that applying the outcome leaves both the accumulator and the rest of
  `MouseReportRecords` bit-identical, for a delta that would otherwise have crossed
  a notch boundary.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] **TS-3** (AC-3): Non-finite guard — from a known non-zero accumulator, feed
  NaN, then +inf, then -inf; assert no report and an unchanged accumulator after
  each, and that a following finite 1.0 still emits its notch.
- [ ] **TS-4** (AC-4): Cap saturation sweep — feed `f32::MAX`, `f32::MIN`, 100.0,
  100.999, 101.0 and their negatives, plus an accumulator pre-loaded near the cap,
  and assert `|notches| <= MAX_WHEEL_REPORT_NOTCHES` in every case.

### Performance Tests

Covered by NFR2 as a structural property rather than a measured benchmark: one
wheel event performs O(1) accumulator work and at most
`MAX_WHEEL_REPORT_NOTCHES` payload repetitions, with the preallocation size and
the repetition bound derived from the same capped value
(`pointer_routing.rs:738-742`).

## Security Considerations

- **Input Validation:** A non-finite `lines` value is rejected before the
  accumulator is mutated (FR3) — early return, no store, no report — so an inf
  followed by a -inf cannot poison the accumulator with NaN and silence the report
  path permanently.
- **Bounded Output:** Two independent clamp layers referencing the same
  `MAX_WHEEL_REPORT_NOTCHES` constant (FR4, NFR3): saturation of the consumed
  magnitude while still a float ahead of the float-to-integer cast, and
  `bounded_wheel_report_duplicate` (`pointer_routing.rs:737-744`). Removing either
  layer individually still leaves the overall bound intact.
- **State Confinement:** The carried fraction is confined to its gesture, tab and
  tracking session (FR5); the rejected-notch branch leaves the accumulator and the
  whole `MouseReportRecords` value unchanged (FR6).
- **Authentication / Authorization / XSS / SQL injection / CSRF:** not applicable —
  this feature adds no network, storage or WebView surface.

## Error Handling

There are no error codes. The single error-shaped condition is a non-finite
delta, handled as an early return with no store and no report (FR3); the
existing non-finite early returns in `wheel_report_notches`
(`input_translate.rs:473-475`) and `alternate_scroll_wheel_bytes`
(`input_translate.rs:367-369`) remain in place.

```
non-finite lines → early return → accumulator unchanged → no PTY write
```

## Constraints and Assumptions

Traced from requirements analysis; the Japanese requirements document records
the same set as A-1 … A-6.

- **A-1:** The whole-notch semantics of `wheel_report_notches` are preserved; they
  are pinned by existing unit tests
  (`src-tauri/src/window_host/tests.rs:1499-1580`). (reversible)
- **A-2:** The two-layer clamp (conversion side + duplication side, one shared
  constant) is preserved rather than collapsed into one layer; the answer to
  `report-accum.clamp-non-finite` states `bounded_wheel_report_duplicate` stays as
  the independent second layer. (reversible)
- **A-3:** The alternate-scroll path keeps its own accumulator and its own
  constant; the report accumulator neither shares storage with `alt_scroll_accum`
  nor aliases `MAX_ALT_SCROLL_NOTCHES`. (reversible)
- **A-4:** Tracking-mode bits are read from the active tab's core on every pointer
  event (decision D2, `pointer_routing.rs:898-901`) and are never mirrored
  host-side, so a tracking-mode transition is observable only at the next pointer
  event. FR5 is stated against that constraint. (reversible)
- **A-5:** The screen-switch reset that `alt_scroll_accum` performs — the
  `if !app.alt_screen { host.alt_scroll_accum = 0.0; }` zeroing at
  `pointer_routing.rs:971-973` and `:980-982` — is deliberately NOT mirrored onto
  the report accumulator; an alternate-screen enter/leave does not by itself
  discard a report-path remainder, and that behaviour is out of scope for this
  feature. This is a separate concern from, and has no bearing on, the tab-change
  and tracking-release reset of FR5, which is in scope and required. (reversible)
- **A-6:** Reports are written to the tab the outcome names, not to `app.active` —
  a remainder is therefore attributable to a specific tab and the FR5 reset is the
  only mechanism preventing cross-tab contribution. (reversible)

## Design Step

Skipped. Resolved by the `design-step.recommendation` answer (gate
`create-spec.design-step`, option `decide_autonomously`, accepting the
recommendation "skip"). The change is confined to wheel-input routing in
`src-tauri/src/window_host/`; it adds no UI surface, touches no design token, and
alters nothing a user sees other than the responsiveness of an existing scroll
gesture.

## Success Criteria

- [ ] All functional requirements (FR1 … FR7) are implemented and tested
- [ ] All non-functional requirements (NFR1 … NFR4) are satisfied
- [ ] All acceptance criteria (AC-1 … AC-10) hold
- [ ] All test scenarios (TS-1 … TS-10) pass
- [ ] Documentation is complete
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement carries `status: resolved`.

## References

- Requirements document: `feature-docs/wheel-report-fraction-accum/REQUIREMENTS.md`
- `src-tauri/src/window_host/input_translate.rs`
- `src-tauri/src/window_host/pointer_routing.rs`
- `src-tauri/src/window_host/mouse_report.rs`
- `src-tauri/src/window_host/mod.rs`
- `src-tauri/src/window_host/tests.rs`
