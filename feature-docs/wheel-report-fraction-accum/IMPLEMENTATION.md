# Implementation Plan: Wheel Report Fraction Accumulator

## Overview

The mouse-report wheel path gains a fraction accumulator so consecutive
sub-notch deltas sum until a whole notch is available, instead of being floored
to zero and discarded. The existing two-layer notch cap, the purity of the
decision units, and the alternate-scroll path all stay as they are.

## Technology Stack

- **Language**: Rust — the `src-tauri` binary crate, GUI feature only. Every
  file this feature touches lives under `src-tauri/src/window_host/`, which is
  already gated behind the default-on `gui` feature, so the CLI-only build
  (`--no-default-features`) is unaffected by construction.
- **Key libraries**: none added. **No new dependency is introduced by this
  feature**, therefore `project.license` (MIT) needs no review and no license
  line is recorded here beyond this statement.
- **Test harness**: the crate's own unit test harness, run through the
  `project.components.main` test command. No window, event loop or GPU surface
  is required by anything this feature adds (NFR4).

## Layer Structure

The wheel path already has three layers. This feature adds work to two of them
and leaves the third untouched. The dependency direction is one-way and must
stay that way.

| Layer | Module | Responsibility after this change | May depend on |
|---|---|---|---|
| Decision (pure) | `src-tauri/src/window_host/mouse_report.rs` | Decides *whether* a wheel event reports, for *which* tab, and whether the per-gesture records must be reset. Sees the event kind, the tracking-mode observation and the active tab — never the delta magnitude. Mutates nothing; every record change travels in the returned outcome. | plain values only |
| Value conversion (pure) | `src-tauri/src/window_host/input_translate.rs` | Turns a line delta plus the carried fraction into a bounded signed notch count and the fraction to carry forward. Owns the non-finite rejection and the float-domain saturation. | plain values only |
| Host / routing | `src-tauri/src/window_host/pointer_routing.rs`, `src-tauri/src/window_host/mod.rs` | Samples the tracking modes, calls the decision layer, applies the outcome, reads and writes the accumulator store, calls the conversion layer, and performs the PTY write. | both pure layers |

The pure layers never call into the host layer and never read host state. The
host layer holds no derived copy of anything the decision layer computes — in
particular it never re-derives the reset condition (see D2).

## Shared Components

These three contracts cross the module seam described above. They are pinned
here — rather than only inside the task plan — because the review and verify
phases are scoped feature-wide and check against this document, and because a
later re-plan must not silently redefine them.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Report-path accumulate-and-consume unit, in the conversion layer beside the existing alternate-scroll fraction helper | Fold one event's line delta into the carried fraction and hand back the whole notches to report plus the fraction to carry forward | **Inputs**: the carried fraction, and the event's line delta. **Precondition**: the carried fraction is finite and its magnitude is below one notch; the delta may be any value, including non-finite. **Postconditions**: (a) a non-finite delta yields zero notches and returns the carried fraction bit-identical — the store is never touched; (b) otherwise the returned notch count is the signed whole part of the sum of the carried fraction and the delta, taken toward zero in the same sign-preserving way the alternate-scroll fraction helper already uses (round down when the total is non-negative, round up when it is negative); (c) the notch magnitude is saturated at the report cap **while still a floating-point value**, before any conversion to an integer count; (d) saturated-away magnitude is discarded, never carried (see D4); (e) the returned fraction is finite and its magnitude is strictly below one notch | task0001 |
| Report accumulator store | Holds the carried fraction between wheel events, scoped to the tab and tracking session that produced it | **Invariant**: exactly one store for the report path, distinct from the alternate-scroll accumulator (FR7). **Reads/writes**: read once per wheel event before the conversion call, written once after it. **Reset**: zeroed only through the reset signal below. **Rejected events**: an event the grid-ownership gate rejects neither writes a changed value nor resets it — the whole record value is unchanged across such an event (FR6) | task0001 |
| Reset signal | Tells the host when the carried fraction must be discarded | **Producer**: the decision layer, as part of the outcome's record updates; already computed today from the tracking-active observation and the tab comparison. **Consumer**: the outcome-application step, which zeroes the accumulator in the same branch that already resets the sibling per-gesture state. **Precondition on ordering**: the outcome is applied before the new delta is folded in. **Postcondition**: after a reset the accumulator is exactly zero, and the rejected-event branch emits no reset — its updates stay the default value | task0001 |

## Conventions

- **Naming**: the report accumulator and its conversion unit are named for the
  report path and never reuse, alias or shadow the alternate-scroll names or
  the alternate-scroll cap constant.
- **Error handling**: this feature defines no error type and no error code. The
  single error-shaped condition is a non-finite delta, handled as an early
  return that produces no report and stores nothing.
- **Logging**: none added. Wheel events are high-frequency; per-event logging on
  this path is out of scope and would be a regression in its own right.
- **Tests**: written in the existing style of this module — plain-value tests
  colocated with the code under test or in the module's existing test file,
  never requiring a window, an event loop or a GPU surface.

## Cross-task Design Decisions

### D1 — The accumulator lives with the per-gesture mouse-report records

The carried fraction is stored alongside the other per-gesture mouse-report
state rather than as a free-standing host field. Rationale: the reset it needs
is exactly the reset that state already receives, so joining that seam gives the
tab-change and tracking-release reset (FR5) and the rejected-event invariance
(FR6) with no second mechanism to keep in sync. Affected: task0001.

### D2 — The reset decision is never re-derived host-side

The host applies the reset the decision layer reported; it does not recompute
"did the tab change" or "is tracking active" for itself. Rationale: the
outcome-application step overwrites the record's tab marker, so any host-side
re-derivation after that point reads a value that has already moved, and any
re-derivation before it duplicates a condition that must stay single-sourced
(NFR1). Affected: task0001.

### D3 — Two clamp layers, one constant

The conversion layer saturates the consumed magnitude, and the duplication step
independently caps the repetition count. Both reference the same report-cap
constant; neither is derived from, nor aliased to, the alternate-scroll cap.
Rationale: the bound is a security property — removing either layer on its own
must still leave the overall bound intact. Affected: task0001.

### D4 — Saturated-away magnitude is discarded, not banked

When saturation clips the notch magnitude, the clipped excess is dropped. It is
neither returned as a notch count nor left in the accumulator for a later event.
Rationale: banking it would turn one absurd delta into a long tail of capped
reports on subsequent events, which defeats the purpose of the cap. Affected:
task0001.

### D5 — Report direction follows the consumed notch sign

The up/down direction of the emitted report is derived from the sign of the
notch count actually consumed, not from the sign of the raw delta. Rationale:
once a remainder can net against an opposite-direction delta, the raw delta's
sign is no longer the authority on what was reported; deriving it from the
consumed count makes the agreement structural instead of incidental (FR2).
Affected: task0001.

### D6 — The non-finite guard belongs to the conversion unit

The rejection of a non-finite delta sits inside the accumulate-and-consume unit,
ahead of any mutation, rather than being a precondition each call site is
trusted to check. Rationale: one guard, one place, and it is reachable from a
plain-value test — a guard spread across call sites can be omitted at a new call
site without any test noticing. The existing non-finite early returns elsewhere
on the wheel path stay exactly as they are; this guard is additional, not a
replacement. Affected: task0001.

### D7 — The alternate-screen reset is deliberately not mirrored

The alternate-scroll accumulator is zeroed on a screen switch. The report
accumulator is not: entering or leaving the alternate screen does not by itself
discard a report-path remainder. Rationale: recorded as assumption A-5 in the
SPEC; it is a separate concern from the tab-change and tracking-release reset,
which is in scope and required. Affected: task0001.

### D8 — Single-task decomposition

The feature is planned as one task. Rationale: the accumulator store, the
conversion unit and the wheel-handler wiring are a single value flowing through
a single call site with a load-bearing ordering between them; splitting them
would force one side to compile against a placeholder for a bound that is itself
the security property under review, and would put the same clamp logic in two
implementers' hands. This is a deliberate choice against the usual bias toward
smaller tasks, taken because the coupling here is intrinsic rather than
incidental.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The cap is enforced only after the float-to-integer conversion, so an enormous delta saturates at the integer maximum first and the bound is briefly false | Medium | High (bounded-output security property) | Contract postcondition (c) states the saturation happens in the float domain ahead of the conversion; AC-4 tests it as a sweep over extreme deltas including a pre-loaded accumulator |
| A non-finite delta poisons the accumulator, after which every comparison is false and the report path is permanently silent | Medium | High (silent permanent breakage) | D6 puts the guard inside the conversion unit ahead of any mutation; AC-3 tests an infinity followed by a negative infinity followed by a finite delta |
| The carried fraction leaks across a tab switch or a tracking session and produces a report the user never gestured for | Medium | Medium | D1 and D2 ride the existing reset seam; AC-6 and AC-7 test both boundaries |
| The reset is applied after the delta is folded in, so the resetting event's own delta is lost or the stale remainder survives it | Low | Medium | Ordering is stated as a precondition on the reset-signal contract and re-stated in the task plan; the existing call order already satisfies it and must not be reordered |
| Touching the alternate-scroll accumulator or its cap constant while working next door | Low | Medium | FR7 and D3 forbid it explicitly; AC-10 is a regression guard over the alternate-scroll behaviour |
| Over-sensitive scrolling: accumulation makes a trackpad report far more notches per gesture than the user expects | Low | Low | Manual verification item in VERIFICATION.md; the bound itself is unaffected |

## Open Questions

- [ ] AC-7's second half ("re-enabling tracking starts from zero") is only
      observable if some pointer event sampled the tracking-inactive state
      between the release and the re-enable — tracking-mode bits are read from
      the active tab on each pointer event and are never mirrored host-side
      (SPEC assumption A-4). The task plan requires the transition to be
      observable through stored last-observed state so that every case reachable
      by an actual event sequence resets; a release-and-re-enable with no
      intervening pointer event at all remains outside what any host-side
      mechanism can see. Confirm at verification that this residual case is
      acceptable.
