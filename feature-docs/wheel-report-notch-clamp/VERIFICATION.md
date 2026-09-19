# Verification Document: wheel-report-notch-clamp

## Overview

**Feature**: wheel-report-notch-clamp
**SPEC.md**: `feature-docs/wheel-report-notch-clamp/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/wheel-report-notch-clamp/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
completion conditions live in `feature-docs/wheel-report-notch-clamp/tasks/`.

## Build Verification

Run every command from the project root.

| Component | Command | Expected |
|-----------|---------|----------|
| main | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| cli_only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors |

The cli_only command is TS10's second half: it proves the feature-gated build
still compiles after the change (NFR4).

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every scenario below reported as passing.
- Coverage target: no numeric coverage gate is configured for this project. The
  gate used here is requirement coverage (see "Functional Requirements
  Coverage"): every requirement must have at least one passing scenario.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Conversion boundary values | 99.999 → 99, 100.0 → 100, 100.999 → 100, 101.0 → 100, and each negated input yields the negated result (-99.999 → -99, -100.0 → -100, -100.999 → -100, -101.0 → -100) | Unit |
| TS2 | Conversion pathological input | NaN → 0, +infinity → 0, -infinity → 0, largest finite value → the cap, most negative finite value → the negated cap | Unit |
| TS3 | Conversion near-zero and sub-notch input | +0.0, -0.0, +0.999, -0.999 → 0; 1.999 → 1; -1.999 → -1 | Unit |
| TS4 | Conversion clamp invariant | Over a swept set of finite inputs (small, boundary, large, extreme; both signs) every result's magnitude is at most `MAX_WHEEL_REPORT_NOTCHES` | Unit |
| TS5 | Duplication helper degenerate cases | Requested count 0 → empty buffer; requested count 1 → byte-for-byte identical to the payload | Unit |
| TS6 | Duplication helper around the cap | Requested count 100 → exactly 100 concatenations; requested count 101 and the largest unsigned 32-bit value → exactly 100 each | Unit |
| TS7 | Duplication helper length invariant | Across representative payloads (empty payload and a realistic single-notch report payload) × requested counts {0, 1, 99, 100, 101, largest unsigned 32-bit value}, the returned length never exceeds payload length × `MAX_WHEEL_REPORT_NOTCHES` | Unit |
| TS8 | Existing conversion regression guard | The three pre-existing wheel-conversion tests pass with their assertions unedited | Unit (regression) |
| TS9 | Alternate-scroll path unaffected | The pre-existing alternate-scroll clamp test passes unmodified, confirming the alternate-scroll cap was neither re-tuned nor shared | Unit (regression) |
| TS10 | Feature-gate compilation | Both the default-feature library test build and the no-default-features check build succeed | Build |

## Code Quality Verification

- Format: no `format_command` is configured for this project's components; the
  repository does not enforce a crate-wide format pass, so no format gate is run
  for this feature.
- Static analysis: no static-analysis command is configured. The compiler's own
  diagnostics from the two build commands above are the gate — the change must
  introduce no new warnings in the touched files.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | For any `f32` input the conversion returns a magnitude at most `MAX_WHEEL_REPORT_NOTCHES`; the integer-saturation attack path is unreachable | TS2 + TS4 pass; code inspection confirms the saturation precedes the float-to-integer conversion |
| AC2 | The duplication helper never allocates or returns more than payload length × the cap, for any requested count up to the largest unsigned 32-bit value | TS6 + TS7 pass; code inspection confirms one capped value feeds both the preallocation and the repetition bound |
| AC3 | `MAX_WHEEL_REPORT_NOTCHES` exists as an independent constant equal to 100 and `MAX_ALT_SCROLL_NOTCHES` keeps its current value | TS1 (cap position at 100) + TS9 pass; code inspection confirms the two constants are independent definitions |
| AC4 | The three existing conversion tests pass with assertions unedited | TS8 passes; diff review confirms those three tests are untouched |
| AC5 | Unit tests exercising the clamp with infinity, NaN and very large finite values exist | TS2 + TS4 present and passing |
| AC6 | Default-feature library tests pass and the no-default-features check still succeeds | TS10: both commands exit 0 |
| AC7 | A zero notch count writes nothing to the PTY; a non-zero notch count produces exactly one write call per wheel event | Manual item M1 (code inspection of the rewired branch) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS9 — the cap sits at 100 on the report path while the alternate-scroll cap is untouched; code inspection confirms the two are independent definitions |
| FR2 | task0001 | TS1, TS2, TS4 |
| FR3 | task0001 | TS1, TS2, TS3, TS8 |
| FR4 | task0001 | TS5 — the helper is called directly from tests with no host state constructed |
| FR5 | task0001 | TS5, TS6, TS7 |
| FR6 | task0001 | TS5 (the branch's output now comes from the helper) + manual item M1 for the write granularity |
| FR7 | task0001 | TS1, TS2, TS3, TS4 |
| FR8 | task0001 | TS5, TS6, TS7 |
| NFR1 | task0001 | TS7 — worst-case buffer length bounded by payload length × the cap |
| NFR2 | task0001 | TS6, TS7 — per-event byte volume bounded at the cap's worth of notches |
| NFR3 | task0001 | TS3, TS8 — below-cap behaviour unchanged; manual item M2 confirms the feel |
| NFR4 | task0001 | TS10 — both builds succeed; diff review confirms no dependency manifest change and no widened visibility |
| NFR5 | task0001 | TS9 — the alternate-scroll clamp test passes unmodified |
| NFR6 | task0001 | TS8, TS9 — the existing suite still passes; diff review confirms the new tests follow the existing inline style, naming and standard-library assertions, with no new test framework crate |

## E2E Testing

No E2E framework is configured for this project (`e2e_test_command` is empty for
both components), and the feature has no UI or process-level surface to drive.
No E2E scenario is defined.

## Manual Testing (E2E Not Possible)

- [ ] M1 (AC7, FR6): Code-inspect the rewired wheel-report branch — confirm the
      zero-notch guard still short-circuits before any allocation or write, and
      that exactly one PTY write call remains on the non-zero path, carrying the
      whole duplicated buffer. This is a manual item because the branch needs
      application and tab state that the unit-test suite does not construct.
- [ ] M2 (NFR3): Scroll with the wheel in a mouse-tracking-enabled TUI and
      confirm the scrolling feel is indistinguishable from before the change
      (normal deltas are far below the cap, so no clamping should be reachable
      by hand).
- [ ] M3 (NFR4): Diff-review the dependency manifests — confirm no crate
      dependency was added and no item gained visibility beyond the
      `window_host` module tree.

No mockup comparison item applies: the design step is `skipped` for this feature
and no DESIGN.md or mockup exists.

## Performance / Security Verification

| Item | Threshold / check |
|------|-------------------|
| NFR1 worst-case allocation | One wheel event's report buffer is at most single-notch payload length × `MAX_WHEEL_REPORT_NOTCHES` (kilobyte scale), against the roughly 21 GB request possible before the fix. Checked by TS7. |
| NFR2 per-event PTY volume | One wheel event writes at most the cap's worth of notches. Checked by TS6 + TS4. |
| SEC1 clamp position | The clamp is applied to the floating-point magnitude BEFORE the float-to-integer conversion, so the saturated integer extreme is never observable. Checked by TS2 + TS4 and by code inspection. |
| SEC2 capacity source | The preallocation size is never computed from an unclamped notch count; capacity and repetition bound derive from the same capped value. Checked by TS7 and by code inspection. |
| SEC3 cap independence | The bound is expressed as its own constant so an alternate-scroll feel adjustment cannot relax it. Checked by TS9 and by code inspection. |
| Attack-surface scope | The defect is locally reachable only, via wheel input on a host already running a mouse-tracking TUI. No remote or cross-user path is in scope. |

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests | 9 (TS1–TS9) | 9 | 0 | 0 |
| Feature-gate compilation | 1 (TS10) | 1 | 0 | 0 |
| Success criteria | 7 (AC1–AC7) | 6 | 0 | 1 |
| Manual items | 3 (M1–M3) | 0 | 0 | 3 |
