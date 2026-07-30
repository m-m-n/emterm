# Verification Document: mux Window List Overlay Auto-Dim

## Overview

**Feature**: mux-sidebar-overlay-dim / **SPEC.md**: `feature-docs/mux-sidebar-overlay-dim/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mux-sidebar-overlay-dim/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/app_settings/Cargo.toml`
- Command (feature-gate regression):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/app_settings/Cargo.toml --lib`
- Coverage target: no numeric project target; every acceptance criterion of both tasks must have at least
  one test, and no pre-existing test may be deleted or weakened
- Known baseline noise: the library suite has pre-existing intermittent failures in the tab replay and
  tmux-socket test modules that are unrelated to this feature. Compare against the base commit before
  attributing any failure here.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Mux settings deserialized with the display-mode key absent | Overlay display mode | Unit |
| TS-2 | Mux settings deserialized with the display-mode key explicitly false, and explicitly null | false → persistent mode; null → overlay mode | Unit |
| TS-3 | Freshly constructed app state | Overlay card's open flag is open | Unit |
| TS-4 | Resolver with the hover flag set | Full opacity, regardless of the switch timestamp | Unit |
| TS-5 | Resolver with a switch recorded at the current instant, no hover | Full opacity, applied without interpolation | Unit |
| TS-6 | Resolver with a switch older than hold + fade, no hover; and a second switch inside the hold window | First case exactly the idle opacity; second case still full opacity past the first expiry | Unit |
| TS-7 | Resolver sampled part-way through the dim fade; then the bright condition becomes true again | Mid-fade value strictly between idle and full; return to full is immediate | Unit |
| TS-8 | Overlay drawn at full opacity; persistent panel drawn with a dim opacity supplied | Overlay appearance matches the pinned corner radius and fill alpha; persistent panel fully opaque | Unit |
| TS-9 | Frame-skip gate's overlay-work input during a fade on a clean grid, and after settling; hover predicate transition | Work true during fade, false when settled; transition requests a redraw | Integration |
| TS-10 | Deadline contribution when settled, when the overlay is not shown, and while a hold or fade is active | Absent in the first two cases, present in the third | Unit |
| TS-11 | Sidebar module source scanned for raw color constructors; opacity and duration constants located | Constraint satisfied; each constant defined exactly once | Unit |
| TS-12 | Toggle action's handler and its default binding | Unchanged behavior and unchanged default binding | Unit |
| TS-13 | Release-feature and CLI-only builds | Both check cleanly | Build |
| TS-14 | Idle overlay observed over the terminal on a real display | Terminal glyphs behind the card are legible and the list is still discernible | Manual |
| TS-15 | Hover, key switch, and 3-second settle observed on a real display | Transitions look correct: immediate brighten, smooth fade out after 3 seconds | Manual |

## Code Quality Verification

- Format: not enforced project-wide; formatting is applied by the editor hook on touched files only. No
  crate-wide format command is run as part of verification (`format_command` is empty in workflow.yaml).
- Static analysis: the compiler's own warnings from the build commands above; no additional linter is
  configured for this crate set.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR10 implemented and covered by tests | Requirements coverage table below; both task suites green |
| SC-2 | All test scenarios pass | TS-1 … TS-13 automated; TS-14/TS-15 manual |
| SC-3 | Idle repaint behavior holds (no feature-attributable repaint when settled) | TS-10 |
| SC-4 | Existing sidebar expectations intact (corner radius, fill alpha, no raw color constructors) | TS-8, TS-11 |
| SC-5 | CLI-only build still checks cleanly | TS-13 |
| SC-6 | Constants documented at their definition site | TS-11 plus review of the touched files |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0002 | TS-3 |
| FR3 | task0002 | TS-4, TS-9 |
| FR4 | task0002 | TS-5, TS-6 |
| FR5 | task0002 | TS-6 |
| FR6 | task0002 | TS-8 |
| FR7 | task0002 | TS-7 |
| FR8 | task0002 | TS-9 |
| FR9 | task0002 | TS-12 |
| FR10 | task0002 | TS-8 |
| NFR1 | task0002 | TS-10 |
| NFR2 | task0002 | TS-11 |
| NFR3 | task0001, task0002 | TS-2, TS-8, TS-13 |
| NFR4 | task0002 | TS-14 |

## E2E Testing

The project has no GUI end-to-end harness; its integration tests cover CLI subcommands only and are
unaffected by this feature. `e2e_test_command` is empty in workflow.yaml, and no E2E scenario is claimed
here.

- [ ] The existing CLI integration tests still pass (covered by the library/build commands above).

## Manual Testing (E2E Not Possible)

Requires a real display and a mux-attached session; the assistant cannot perform these.

- [ ] TS-14: With the overlay idle, terminal output that the card covers is readable, and the window list
      is still recognizable as a list.
- [ ] TS-15: Hovering the card brightens it immediately; leaving it dims it; switching windows by keyboard
      brightens it and it dims roughly three seconds later; a rapid second switch postpones the dimming.
- [ ] The toggle binding still hides and shows the card.
- [ ] With `mux.window_sidebar_overlay` explicitly set to false in the settings file, the persistent panel
      looks and behaves exactly as before.
- [ ] No visible idle CPU increase while the card sits dim (observed against the process's own idle
      baseline).

## Performance / Security Verification (if applicable)

- NFR1: no deadline armed and no repaint requested in the settled state — asserted by TS-10 and
  cross-checked in the manual idle-CPU observation above.
- Security: no new persisted data, no external I/O, no new dependency — nothing to verify beyond the
  clamped opacity range (TS-4 … TS-7 cover the range invariant).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Settings default | 2 | 2 | 0 | 0 |
| Opacity state machine | 5 | 5 | 0 | 0 |
| Rendering | 2 | 2 | 0 | 0 |
| Frame scheduling | 2 | 2 | 0 | 0 |
| Build / compatibility | 2 | 2 | 0 | 0 |
| Visual & usability | 2 | 0 | 0 | 2 |
| **Total** | **15** | **13** | **0** | **2** |
