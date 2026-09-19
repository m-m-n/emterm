# Implementation Plan: wheel-report-notch-clamp

## Overview

Bound the mouse-report wheel path so that one wheel event can never request an
unbounded allocation or an unbounded PTY write. A report-path-only notch cap is
introduced in the translation module and enforced twice — once where the wheel
delta becomes a signed notch count, and once again where the single-notch report
payload is duplicated — so neither side has to trust the other.

## Technology Stack

- **Language**: Rust — the existing `emterm` binary crate under `src-tauri/`.
  All changes stay inside the `window_host` module tree.
- **Key libraries**: none added. The change uses only the standard library and
  code already present in the crate.
- **New dependencies**: none (NFR4). Because no new dependency is introduced,
  the `project.license: MIT` constraint has nothing to check against; no license
  record is added to this document.
- **Test framework**: the crate's built-in test harness with standard-library
  assertions only. No new test framework crate is added (NFR6 — explicitly no
  property-testing and no benchmarking crate).

## Layer Structure

| Layer | Module | Responsibility | May depend on |
|-------|--------|----------------|---------------|
| Translation | `src-tauri/src/window_host/input_translate.rs` | Turns raw pointer/wheel input values into transport-neutral quantities (notch counts, byte sequences). Holds the safety caps. Knows nothing about tabs, app state, or the PTY. | nothing in this feature |
| Routing | `src-tauri/src/window_host/pointer_routing.rs` | Decides which consumer a wheel event reaches, builds the outgoing buffer, and performs the single PTY write. | Translation |
| Test | `src-tauri/src/window_host/tests.rs` | Unit tests for both layers above. | Translation, Routing |

Allowed dependency direction is Routing → Translation only. The translation
layer never reaches back into routing, so the cap constant and the conversion
contract remain testable without any host state.

## Shared Components

These three items form the contract surface of this feature. They are pinned
here — rather than only in the task plan — because the review, verify and any
later rework tasks read this document to decide whether a change still honours
the security property.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `MAX_WHEEL_REPORT_NOTCHES` (translation layer) | The report path's own upper bound on notches per wheel event. Value: 100. | Precondition: none. Postcondition: it is a compile-time constant of the translation layer, visible no wider than the `window_host` module tree, and is referenced by BOTH clamp layers. It is never aliased to, nor derived from, `MAX_ALT_SCROLL_NOTCHES`. | task0001 |
| `wheel_report_notches(lines: f32) -> i32` (translation layer) | Converts a wheel delta already normalised to lines into a signed whole-notch count. | Precondition: any `f32` whatsoever, including non-finite and extreme finite values. Postconditions, for every input: (1) the absolute value of the result is at most `MAX_WHEEL_REPORT_NOTCHES`; (2) a non-finite input yields 0; (3) an input whose magnitude is below one whole line yields 0; (4) a non-zero result carries the sign of `lines` (non-negative input → positive result, otherwise negative); (5) below the cap, the magnitude is the floor of the absolute value. The visible signature is unchanged. | task0001 |
| Bounded wheel-report duplication helper (routing layer) | Produces the buffer that one wheel event writes to the PTY, by repeating a single-notch report payload. | Precondition: a single-notch payload byte slice (possibly empty) and a requested repeat count anywhere in the unsigned 32-bit range. It reads no application, tab, host or terminal state — it is a pure function of its two arguments. Postconditions: (1) the effective repeat count is the requested count capped at `MAX_WHEEL_REPORT_NOTCHES`; (2) the returned buffer is exactly the payload repeated that effective count of times, in order; (3) the returned buffer's length never exceeds payload length × `MAX_WHEEL_REPORT_NOTCHES`; (4) the preallocated capacity and the repetition bound are derived from the SAME capped value, so they can never diverge. | task0001 |

## Conventions

- **Naming**: the cap constant name states its path (`WHEEL_REPORT`), not its
  value, so it cannot be mistaken for the alternate-scroll cap. The duplication
  helper's name states what it produces and that it is bounded.
- **Visibility ceiling**: nothing introduced by this feature is visible beyond
  the `window_host` module tree. The existing module-internal visibility level
  is the ceiling for the new constant and the new helper alike (NFR4).
- **Error handling**: out-of-range input is not an error condition. Non-finite,
  sub-notch and over-cap inputs are handled on the normal path by yielding zero
  or by saturating. No error type, no result-carrying return, no logging and no
  panic is introduced anywhere in this feature.
- **Documentation**: the cap constant and both clamp sites carry a doc comment
  stating that the bound is a security property, not a feel-tuning knob, so a
  later reader does not "simplify" it away.
- **Test style** (NFR6): tests live inline in the module tree's existing test
  file, are named `<subject>_<scenario>_<expected>`, and use standard-library
  assertions only.

## Cross-task Design Decisions

### D1: This feature is intentionally a single task

The two clamp layers share one definition — the cap constant. Tasks in this
workflow are implemented fully in parallel in isolated worktrees, so splitting
the translation-side clamp and the consumption-side clamp into two tasks would
force one of two bad outcomes: either the second task cannot compile against a
constant that does not exist in its worktree, or both tasks define the constant
and the merge produces a duplicate definition. Since the whole change is three
files inside one module tree, it is planned as one coherent task instead.
Affected: task0001.

### D2: Two independent clamp layers (defense in depth)

The cap is applied twice against the same constant: once inside the conversion
(which thereby guarantees its own postcondition to every present and future
caller) and once inside the duplication helper (which therefore does not have to
trust its caller). Removing either layer individually must still leave the
overall bound intact; this is the property a reviewer checks. Affected: task0001.

### D3: A dedicated constant, not the alternate-scroll one

The report path gets its own constant even though its value coincides with the
alternate-scroll cap. The two paths are tuned for different reasons — one for
feel, one for safety — and sharing a single constant would let a future feel
adjustment silently relax a security bound. The alternate-scroll constant and
its byte-producing function are left untouched (NFR5). Affected: task0001.

### D4: Saturate BEFORE the float-to-integer conversion

The clamp is applied to the floating-point magnitude, ahead of the conversion to
a signed integer. In this language the float-to-integer conversion saturates at
the integer type's extremes, so clamping only after the conversion would leave
the saturated extreme observable as an intermediate value and would make the
conversion's stated postcondition false for a window of the code. Affected:
task0001.

### D5: One capped value feeds both the capacity and the repetition bound

Inside the duplication helper the capped count is computed once and used for the
preallocation size and for the repetition bound. Deriving them separately is
what makes an over-large allocation possible even when the loop is bounded.
Affected: task0001.

### D6: Write granularity is preserved

One wheel event still produces exactly one PTY write carrying the whole
duplicated buffer, and a zero-notch event still produces no write at all. This
feature bounds the buffer's size; it does not restructure when or how often the
PTY is written. Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Clamping after the conversion instead of before it, leaving the saturated extreme observable | Medium | High | D4 is stated as a contract postcondition over ALL `f32` inputs and is pinned by an invariant test sweeping extreme values (TS2, TS4) |
| Preallocation size and repetition bound drift apart during a later edit | Low | High | D5: a single capped value is computed once and used for both; the length-invariant test (TS7) fails if they diverge |
| Behaviour below the cap changes and breaks callers' expectations | Medium | High | The three existing conversion tests must pass with their assertions unedited (TS8), and the below-cap semantics are restated as contract postconditions |
| The alternate-scroll path is "unified" with the report path during implementation | Low | Medium | D3 plus the existing alternate-scroll clamp test kept unmodified (TS9); the alternate-scroll items are named as out of scope in the task plan |
| The new helper is given host state (tab/app) for convenience, making it untestable | Low | Medium | The helper's precondition explicitly forbids reading any host state; its tests construct it from two plain arguments only |
| Call-site rewiring silently changes write granularity (extra or missing write) | Low | Medium | D6 plus an explicit acceptance criterion; verified by code inspection because the project has no host-state harness for this branch (see Open Questions) |

## Open Questions

- [ ] The call-site behaviour of FR6/AC7 (zero notches write nothing; a non-zero
      notch count produces exactly one PTY write per wheel event) has no
      automated test in the SPEC's scenario set — the routing branch needs
      application and tab state that the existing unit-test suite does not
      construct. It is planned as a code-inspection item in VERIFICATION.md's
      manual section. If the project later gains a host-state harness, this
      should become an automated scenario.
