# Verification Document: Wheel Report Fraction Accumulator

## Overview

**Feature**: wheel-report-fraction-accum
**SPEC.md**: `feature-docs/wheel-report-fraction-accum/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/wheel-report-fraction-accum/IMPLEMENTATION.md`

This document covers the integrated verification of the whole feature. Per-task
completion is governed by the acceptance criteria in
`feature-docs/wheel-report-fraction-accum/tasks/task0001.md`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors.
- Supplementary (project policy, feature-gate guard — the whole change lives in
  a GUI-gated module, so this must stay green):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: exit code 0, zero failures. The single-threaded flag is required —
  some pre-existing tests in this crate are non-deterministic under parallel
  execution.
- Coverage target: not applicable — this project has no coverage tooling
  configured in `workflow.yaml`. Coverage is asserted structurally instead:
  every acceptance criterion maps to at least one scenario below.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Sub-notch accumulation crossing — three consecutive same-direction deltas of four tenths of a notch | No report for the first two events; exactly one notch of report bytes at the third | Unit |
| TS-2 | Remainder retention and reversal — one and a half notches, then a reversal of seven tenths, then a further nine tenths in the reversed direction | First event reports one notch and holds half a notch; the reversal nets to minus two tenths with no report; the third event reports exactly one notch in the reversed direction | Unit |
| TS-3 | Non-finite guard — from a known non-zero accumulator, feed not-a-number, then positive infinity, then negative infinity, then a finite whole notch | No report and a bit-identical accumulator after each non-finite event; the following finite delta still emits its notch | Unit |
| TS-4 | Cap saturation sweep — the floating-point extremes, values just under, at and just over the cap, their negatives, and an accumulator pre-loaded near the cap | The notch magnitude never exceeds the report cap in any case, and the saturation happens before the conversion to an integer count | Unit |
| TS-5 | Independent duplication cap — ask the duplication step for a count far above the cap | Exactly the cap's worth of repetitions, with allocation for no more | Unit |
| TS-6 | Tab-change reset — accumulate nine tenths against one tab, then decide a wheel event whose active tab is a different one | The outcome carries the reset and the new tab marker; applying it zeroes the accumulator; the bytes written for the new tab's event are those of a zero-accumulator nine-tenths delta, i.e. none | Integration |
| TS-7 | Tracking-release reset and re-enable — accumulate nine tenths with a tracking mode active, then observe a wheel event with every tracking mode off, then re-enable tracking on the same tab | The release-observing outcome carries the reset and the accumulator is zeroed; the first wheel event of the re-enabled session accumulates from zero, including the release-then-re-enable ordering with no intervening wheel event | Integration |
| TS-8 | Rejected-notch invariance — a wheel event the grid-ownership gate rejects, carrying a delta that would otherwise have crossed a notch boundary | The decision returns record updates equal to the default value; applying the outcome leaves the accumulator and the rest of the per-gesture record bit-identical | Integration |
| TS-9 | Whole-notch regression guard — the pre-existing notch-conversion expectations | All still pass, unmodified | Unit |
| TS-10 | Alternate-scroll path unchanged — the alternate-scroll fraction helper's whole-and-fraction contract, and the tracking-active modifier branch | The helper's contract is unchanged and the alternate-scroll accumulator is still left untouched on that branch | Unit |

## Code Quality Verification

- Format: not configured — `project.components.main.format_command` is empty in
  `workflow.yaml`. No formatting command is run for this feature, and no
  crate-wide reformat is performed.
- Static analysis: covered by the build command above; no separate lint command
  is configured.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Sub-notch deltas sum and report exactly once at the crossing event | TS-1 |
| AC-2 | The signed remainder is retained and a reversal nets against it; direction matches the consumed count | TS-2 |
| AC-3 | A non-finite delta reports nothing and leaves the accumulator untouched; the path cannot be silenced | TS-3 |
| AC-4 | The notch magnitude is bounded by the report cap, saturated before the integer conversion | TS-4 |
| AC-5 | The duplication step caps independently | TS-5 |
| AC-6 | No remainder crosses a tab change | TS-6 |
| AC-7 | No remainder crosses a tracking-session boundary | TS-7 (with the bounded residual case below) |
| AC-8 | A rejected event advances nothing and resets nothing | TS-8 |
| AC-9 | Whole-notch behaviour is unchanged | TS-9 |
| AC-10 | The alternate-scroll path is unchanged | TS-10 |

#### AC-7 residual case (accepted, out of scope)

SPEC assumption A-4 reads the tracking mode bits from the active tab on each
pointer event rather than mirroring them on the host, so the accumulator's
tracking-release reset is carried by a pointer event's reset observation
(`RecordUpdates.reset`), which button-press, button-release and wheel
decisions all set. A grid-rejected event carries `RecordUpdates::default()`
and therefore no reset observation (FR6 / AC-8), so it does not clear the
accumulator either.

The residual case is a tracking release followed by a re-enable with zero
intervening non-rejected pointer events. That case is out of scope for this
feature. It is not unobservable, but it is bounded: the accumulator holds
only a sub-notch remainder (`< 1` notch, a single `f32`, saturated by the
report cap), so the worst outcome is that the first wheel event of the
re-enabled session emits one notch instead of zero — one extra line in a
mouse-aware application. It does not cascade, does not corrupt state, and
self-corrects on the next event. Verification accepts this; no test asserts
against it.

Basis: Codex consultation (LiteLLM proxy, `muse-spark`), which compared
accepting the gap against mirroring the mode bits host-side (breaking A-4)
and against resetting at the terminal parser's mode-write site, and judged
accepting it comparatively better — both alternatives add lifetime,
lock-ordering or cross-thread-write complexity to `MouseReportRecords`
without buying a real fix. Codex additionally claimed only applied wheel
outcomes carry the reset; that claim was checked against
`src-tauri/src/window_host/mouse_report.rs` and is wrong — `decide_press`
and `decide_release` set `reset` as well — so the scope above is stated as
non-rejected pointer events, not wheel events alone.

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-9 |
| FR2 | task0001 | TS-1, TS-2 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-4, TS-5, TS-9 |
| FR5 | task0001 | TS-6, TS-7 |
| FR6 | task0001 | TS-8 |
| FR7 | task0001 | TS-10 |
| NFR1 | task0001 | TS-6, TS-7, TS-8 |
| NFR2 | task0001 | TS-4, TS-5 |
| NFR3 | task0001 | TS-4, TS-5 |
| NFR4 | task0001 | TS-1, TS-6, TS-8 |

## E2E Testing

No E2E framework applies to this feature. `project.components.main.e2e_test_command`
is empty, the SPEC records no existing E2E tests for the wheel path, and the
behaviour under change is driven by physical pointer-device input that no
automated harness in this project can generate.

## Manual Testing (E2E Not Possible)

Run the release build and observe with a high-resolution trackpad. No design
mockup comparison applies — the design step was skipped and this feature adds no
visual surface.

- [ ] Inside an application that enables mouse tracking, small trackpad scroll
      gestures now scroll the application. Before this change the same gestures
      did nothing at all.
- [ ] A slow continuous gesture scrolls smoothly rather than in bursts, and a
      fast flick does not scroll a wildly disproportionate distance.
- [ ] Reversing direction mid-gesture reverses the scroll promptly, without a
      dead zone the size of a full gesture.
- [ ] Scrolling on tab A, switching to tab B and scrolling there produces no
      stray first movement on B.
- [ ] With a conventional wheel mouse, notch-by-notch scrolling behaves exactly
      as before.
- [ ] With mouse tracking off, the alternate-scroll and scrollback behaviours are
      unchanged, including entering and leaving the alternate screen.

## Performance / Security Verification

- NFR2 (bounded work per event): verified structurally, not by benchmark — one
  wheel event does a constant amount of accumulator work and at most the cap's
  worth of payload repetitions, with the preallocation size and the repetition
  bound derived from the same capped value. Confirm by inspection during review
  that the two are not re-derived independently.
- NFR3 / bounded output: confirm by inspection that both clamp layers reference
  the same report-cap constant and that neither is aliased to, nor derived from,
  the alternate-scroll cap. TS-4 and TS-5 cover the two layers separately, so
  removing either layer alone fails a test.
- Input validation: TS-3 is the security test for the non-finite path — the
  failure mode it guards is a permanently silenced report path, not a crash.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 10 | 10 | 0 | 0 |
| Success criteria | 10 | 10 | 0 | 0 |
| Requirements | 11 | 11 | 0 | 0 |
| Performance / security | 3 | 1 | 0 | 2 (inspection during review) |
| Manual scenarios | 6 | 0 | 0 | 6 |
