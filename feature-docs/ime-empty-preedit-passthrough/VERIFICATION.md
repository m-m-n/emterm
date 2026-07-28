# Verification Document: Empty-Preedit Key Passthrough

## Overview

**Feature**: ime-empty-preedit-passthrough
**SPEC.md**: `feature-docs/ime-empty-preedit-passthrough/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/ime-empty-preedit-passthrough/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

Additional target checks (NFR2):

- CLI-only feature gate: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Windows cross-target: `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc`
- Expected: exit code 0 for both

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: every acceptance criterion of task0001 has at least one
  asserting test in the winit bridge module. No project-wide coverage
  percentage is enforced.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Enable, then a non-empty preedit, then an empty preedit | Key dispatch is passthrough on non-Windows; both preedit events reach the neutral queue in order | Unit |
| TS-2 | Non-empty preedit, empty preedit, non-empty preedit | Key dispatch is consumed again on non-Windows | Unit |
| TS-3 | Non-empty preedit, empty preedit, commit | Key dispatch is passthrough on non-Windows; queue order is preedit, empty preedit, commit | Unit |
| TS-4 | Enable alone, no preedit | Key dispatch is passthrough on non-Windows | Unit |
| TS-5 | Empty preedit, non-empty preedit, empty preedit (X11 ambiguous start/end shape) | Key dispatch is passthrough, then consumed, then passthrough on non-Windows | Unit |
| TS-6 | Enable, empty preedit, disable (Windows) | Key dispatch is consumed after the empty preedit and passthrough after the disable | Unit (Windows target) |
| TS-7 | Enable, non-empty preedit, empty preedit, commit, disable (Windows) | Key dispatch is consumed after the commit and passthrough after the disable | Unit (Windows target) |
| TS-8 | Delete-surrounding after a non-empty preedit, and after an empty preedit | Key dispatch answer is unchanged in both situations | Unit |
| TS-9 | The pre-existing test set of the winit bridge module | All pass unmodified | Unit |
| TS-10 | Predicate truth table under each platform selector | Selector true → result equals lifecycle state alone; selector false → result equals preedit-present state alone; all four state combinations asserted per selector | Unit (runs on host) |
| TS-11 | The TS-6 / TS-7 Windows scenarios driven through the predicate | Same expectations as TS-6 / TS-7, now executed on the development host | Unit (runs on host) |
| TS-12 | Enable, non-empty preedit, then focus loss | Predicate answers "not suppressed" under both selector values | Unit (runs on host) |
| TS-13 | Focus gain on a freshly built bridge | Both states stay false, so the predicate answers "not suppressed" under both selector values; the IME-allowed call sequence assertion still holds | Unit (runs on host) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: covered by the build command's warnings; no separate lint
  command is configured for this crate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 through FR9 are implemented | Read the changed file against the state table in SPEC.md; TS-1 through TS-8 pass |
| SC-2 | TS-1 through TS-9 pass | Run the test command |
| SC-3 | Host build and test pass | Run the build and test commands |
| SC-4 | CLI-only feature gate still compiles | Run the `--no-default-features` check |
| SC-5 | Windows target still compiles | Run the Windows cross-check |
| SC-6 | Formatting is clean | Run the format check |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-6 |
| FR2 | task0001 | TS-1, TS-2, TS-3, TS-5 |
| FR3 | task0001 | TS-4, TS-6 |
| FR4 | task0001 | TS-3, TS-7 |
| FR5 | task0001 | TS-6, TS-7, TS-9 |
| FR6 | task0001 | TS-8 |
| FR7 | task0001 | TS-6, TS-7, TS-manual-3 |
| FR8 | task0001 | TS-1, TS-2, TS-4, TS-5 |
| FR9 | task0001 | TS-9 plus reading the changed file for the removed false claim |
| FR10 | task0003 | TS-12, TS-13 |
| FR11 | task0003 | TS-10, TS-11 |
| NFR1 | task0001, task0003 | Read the suppression predicate: a boolean computation with no allocation or locking |
| NFR2 | task0001, task0003 | The `--no-default-features` check and the Windows cross-check |
| NFR3 | task0001, task0003 | TS-10 and TS-11 execute both gate branches on the development host |

## E2E Testing

The project has no automated E2E suite covering the native terminal input path.
This section is intentionally empty; the equivalent coverage is manual.

## Manual Testing (E2E Not Possible)

The IME path requires a real compositor and a real input method, neither of
which is available to the verification run.

- [ ] TS-manual-1 (Linux, Wayland native, fcitx5-skk): type `ABC` in direct
      input mode, enter SKK conversion mode with Shift+letter, delete the whole
      conversion buffer with BackSpace, then press BackSpace once more and
      confirm `C` is deleted.
- [ ] TS-manual-2 (Linux, Wayland native, ordinary kana-kanji input method):
      repeat the TS-manual-1 flow with a non-SKK input method and confirm the
      same result.
- [ ] TS-manual-3 (Windows, IMM32 input method): open a composition, confirm
      arrow keys drive the candidate window without also moving the shell
      cursor, and confirm keys reach the shell again once the composition ends.
- [ ] TS-manual-4 (Linux, Wayland native): confirm the preedit overlay still
      appears while composing and disappears when the preedit is emptied — the
      rendering path must be unaffected.

## Performance / Security Verification

- NFR1: verified by inspection of the suppression predicate rather than by
  measurement. The path is a single boolean read on every keystroke; a timing
  benchmark would not resolve a difference at that scale.
- No security requirement applies. The change narrows the set of keys the
  terminal withholds and does not widen the interpretation of any external
  input.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios | 9 | 9 | 0 | 0 |
| Success criteria | 6 | 6 | 0 | 0 |
| Manual scenarios | 4 | 0 | 0 | 4 |
