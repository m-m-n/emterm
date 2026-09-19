# Verification Document: mouse-reporting

## Overview

**Feature**: mouse-reporting / **SPEC.md**: `feature-docs/mouse-reporting/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mouse-reporting/IMPLEMENTATION.md`

This document covers the INTEGRATED verification run after every task is merged.
Task-level acceptance criteria live in the per-task plans under
`feature-docs/mouse-reporting/tasks/`.

Scenario identifiers TS-1 through TS-13 correspond one-to-one with SPEC.md's
TS1 through TS13; the hyphenated form is this document's identifier form.
TS-14 through TS-18 are integrated success-criteria checks derived from SPEC.md's
success criteria AC13 through AC16, which SPEC.md states as criteria rather than
as numbered test scenarios.

TS-19 through TS-21 were added after review round 1. They have no SPEC.md
counterpart by construction: they verify requirements SPEC.md already states
(FR2, FR3, FR4, FR7, FR10, NFR1) at a granularity the original scenario set left
to manual confirmation, which is why the round-1 defects reached review. The
requirement set itself is unchanged.

TS-22 through TS-24 were added after verify round 1, on the same basis: they
verify FR2, FR3, FR7, FR11 and NFR1 — all already stated by SPEC.md — at the
granularity the seam of IMPLEMENTATION.md D12 makes observable. The requirement
set is again unchanged.

The design step is `skipped` for this feature, so no mockup visual comparison is
part of this plan.

## Build Verification

| Component | Command | Expected |
|---|---|---|
| main | `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| term_core | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml` | exit code 0, no errors |
| cli_only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors (AC14, NFR3) |
| web | `bun run build:viewer && bun run build:settings` | exit code 0 — expected to be unaffected by this feature (NFR2) |

All commands are run from the project root, never after changing directory into
the crate directory.

## Test Verification

| Component | Command |
|---|---|
| main | `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` |
| term_core | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` |
| web | `bun test` — expected to be unaffected by this feature (NFR2) |

**Coverage target**: the project configures no numeric coverage gate, so none is
asserted here. The coverage obligation for this feature is NFR7's byte-exact
test inventory, verified as TS-18 below rather than as a percentage.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | DECSET 1000 / 1002 / 1003 / 1006 set and clear their mode bits | For each mode, the set request returns the no-host-action value and the mode reads back active; the reset request clears it. The four modes are independent | Unit |
| TS-2 | The existing TS-fallback test is narrowed | The fallback assertion covers only the modes still on that arm (including 1005); 1000 / 1002 / 1003 / 1006 gain positive bit assertions instead; 1015 remains a silent no-op | Unit |
| TS-3 | X10 encoding of press, release, motion and wheel | Table-driven byte-exact results, including the origin case (left press at column 1 row 1 emits the introducer, the letter M, then 32, 33, 33; its release emits 35, 33, 33), the release-becomes-3 rule, the motion bit, and wheel codes 64 and 65 | Unit |
| TS-4 | SGR (1006) encoding of press, release, motion and wheel | The same table with 1006 active: the final byte splits M for press/motion and m for release, the press base is retained on release, and the wheel form carries code 64 / 65 | Unit |
| TS-5 | Modifier bits | Ctrl contributes 16, Alt contributes 8, both contribute 24, and no input combination produces the value 4 | Unit |
| TS-6 | X10 coordinate overflow yields no report | Column 223 and row 223 encode; column 224 or row 224 yields no bytes at all under X10; the same event under SGR carries the true coordinate | Unit |
| TS-7 | Motion is filtered to cell changes | A sequence of cells with repeats yields exactly the distinct consecutive cells; the cached cell carries across calls; a reset makes the next cell report | Unit |
| TS-8 | Wheel routing decision is a pure choice that branches on tracking-active first | Exactly one consumer is chosen for every combination of tracking-active, shift-held, alternate-screen, alternate-scroll mode bit and alternate-scroll setting. While tracking is ACTIVE: scroll-scrollback when Shift is held and report-to-application otherwise — arrow translation is never chosen, including the alternate-screen cell with the mode bit and the setting both on. While tracking is INACTIVE: today's matrix unchanged, with Shift not consulted — arrow translation on the alternate screen when the mode bit and the setting are both on, scroll-scrollback otherwise | Unit |
| TS-9 | Motion reporting gate per tracking mode | 1000 alone never reports motion; 1002 reports only while at least one button is held; 1003 reports always, with the "none" base when no button is held. With several buttons held the lowest-numbered one is carried | Unit |
| TS-10 | Real application mouse interaction | In a mouse-aware application, click positions the cursor, drag selects inside the application, and the wheel scrolls the application's own view on BOTH the main and the alternate screen | Manual |
| TS-11 | Shift override end to end | Shift+drag selects eMterm text with PRIMARY updated; Shift+Ctrl+click opens a hovered link; Shift+middle-click pastes PRIMARY; Shift+wheel moves eMterm's scrollback on the main screen AND, on the alternate screen with alternate-scroll enabled, still moves eMterm's scrollback rather than sending arrow keys to the application. Bare Ctrl+click and bare middle-click instead reach the application | Manual |
| TS-12 | Chrome guards unaffected | With a tracking mode active, the tab close button, the tab strip, the status bar, the scrollbar, the mux sidebar in both placements and a CSD window edge all behave exactly as before, with nothing reaching the application | Manual |
| TS-13 | Mouse reporting over a mux-attached remote pane | A mouse-aware application on a remote mux pane receives reports through the existing input transport | Manual |

### Success-Criteria Checks (from SPEC.md success criteria)

| ID | Check | Expected Result | Test Type |
|----|-------|-----------------|-----------|
| TS-14 | CLI-only build still compiles (AC14) | The cli_only build command above exits 0 | Build |
| TS-15 | The settings surface is untouched (AC13) | The integrated diff touches no file under the settings crate, no TypeScript settings type mirror and no settings-panel source | Inspection |
| TS-16 | No platform-specific branch is introduced (NFR4) | The integrated diff adds no unix-only or windows-only conditional compilation in any file this feature touches | Inspection |
| TS-17 | Both library test suites pass (AC15) | The main and term_core test commands above exit 0, including the narrowed fallback test | Build |
| TS-18 | The byte-exact test inventory exists (AC16, NFR6, NFR7) | The merged change contains a test asserting the exact bytes for each of: an X10 press, an X10 release, an SGR press, an SGR release, a motion report, a wheel-up report, a wheel-down report, a Ctrl-modified report, an Alt-modified report, and an X10 event beyond column/row 223 asserting no bytes — each running with no winit window and no live PTY | Inspection |

### Rework Scenarios (added after review round 1)

These three scenarios are the automated coverage for task0004, task0005 and
task0006. Each runs with no winit window, no GPU surface and no live PTY, under
the same inline test convention as TS-1 through TS-9.

A source-text assertion — one that reads the routing source and compares
substring positions — does not satisfy any of these three. Each is satisfied
only by driving the real decision path and observing its result: the bytes it
produces (or produces none of) and the state it leaves behind. This was the
finding of verify round 1 against all three, and is what IMPLEMENTATION.md D12's
seam exists to make possible.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-19 | One grid-ownership decision covers every chrome region, every button identity and every event kind | Table-driven over (region, event kind, button identity): the decision rejects the top strip, the status-bar bottom strip, the right-edge scrollbar overlay, the mux sidebar in both persistent and overlay placement, and the CSD edge-resize hot zone, and rejects everything while the profile selector is visible — identically for a left / middle / right press, a left / middle / right release, a motion and a wheel notch — and accepts a position over the grid with no region claiming it. A middle or right press or release over each rejected region, and a wheel notch over the bottom strip or the scrollbar overlay, produce no bytes | Unit |
| TS-20 | A button gesture is owned by whichever side took its press, in both shift orderings | Driven as a sequence (press → shift-state change → release). Press with Shift held then release after Shift is lifted: no bytes are emitted, the drag flag is cleared, the pending selection anchor is consumed and the selection reaches PRIMARY. Press without Shift then release after Shift is pressed: the matching release report is emitted and its button code does not carry the shift bit. A press the TS-19 decision rejects records no owner; the record is cleared when its release is delivered and on the observations that reset the cell cache; a second button pressed mid-gesture is owned independently | Unit |
| TS-21 | A suppressed motion advances no cached cell | With a tracking mode active, a motion over each region TS-19 rejects, and any motion while the profile selector is visible, emits nothing AND leaves the cached last-reported cell unchanged — asserted on the cache state, not only on the absence of bytes — so a following motion over the grid at a different cell still reports. This is the ordering assertion: the guard runs before the motion gate and the cell-change filter | Unit |

### Rework Scenarios (added after verify round 1)

These three scenarios cover the wiring defects that the D12 seam makes
observable. They run under the same conditions as TS-19 through TS-21 — no
winit window, no GPU surface, no live PTY — and are likewise not satisfiable by
a source-text assertion.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-22 | A release is governed by its own gesture, not by the state at release time | Driven as a sequence. (a) A reported press followed by the application clearing every tracking mode before the button is released: the release emits no bytes at all — the release path reads "at least one tracking mode is active", not merely which encoding is selected. (b) A reported press followed by an active-tab change before the button is released: the release report is destined for the tab identifier the PRESS recorded, not the tab active at release time. (c) A release arriving with no recorded press: no bytes. In every case the assertion is on the produced bytes and their target tab identifier | Unit |
| TS-23 | Stale records cannot survive into any pointer path | (a) The two reset observations — no tracking mode active, and the active tab differing from the one the records were built against — are carried by the button, motion and wheel decisions alike, so a press or a wheel notch arriving with no intervening motion clears the cached cell and the gesture-ownership record exactly as a motion does; asserted by observing the record state after driving each of the three paths, not only the bytes. (b) A focus-loss clear empties the gesture-ownership record and the held-button record together, so the first event after focus returns is decided from empty records | Unit |
| TS-24 | Motion follows the gesture's owner, not the instantaneous Shift state | With a tracking mode active: a press taken locally because Shift was held, then Shift released, then pointer motion with the button still down — the motion emits no bytes, because the motion decision consults the recorded gesture owner rather than the current shift flag. The mirror case holds too: a reported press, then Shift pressed, then motion — motion reports as before, and the emitted button code does not carry the shift bit | Unit |

## Code Quality Verification

- **Format**: no format command is configured for any component in
  `workflow.yaml`, so no format gate runs. Rust sources follow the project's
  rustfmt style edition already in effect for the files being edited.
- **Static analysis**: none configured; the compiler warnings from the build
  commands above are the static-analysis surface. Expected: no new warning.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | The four DECSET modes set and clear their bits and return the no-host-action value | TS-1 |
| AC2 | X10 left press at column 1 row 1 emits 32, 33, 33 and its release 35, 33, 33 | TS-3 |
| AC3 | With 1006 the same press and release emit the SGR forms with the M / m split | TS-4 |
| AC4 | Motion is emitted per tracking mode and button-held state only | TS-9, TS-3, TS-10 |
| AC5 | Motion within one cell emits exactly one report; crossing emits a second | TS-7, TS-10 |
| AC6 | Wheel emits 64 / 65 on both screens and leaves the scrollback offset unchanged | TS-8, TS-3, TS-10 |
| AC7 | With no tracking mode active, wheel behaviour is byte-for-byte today's, Shift included | TS-8 (tracking-inactive branch), TS-10 |
| AC8 | Ctrl+left press reports with bit 16 and does not open the link; middle press reports base 1 and does not paste | TS-5, TS-11 |
| AC9 | Shift suppresses the report and the local behaviour happens instead — for the wheel this is the scrollback scroll on both screens, never arrow bytes; for a drag this includes the motion and the release that complete the selection, whatever the shift state has become by then (IMPLEMENTATION.md D10) | TS-8 (tracking-active branch), TS-20, TS-24, TS-11 |
| AC10 | No emitted report ever carries the shift bit | TS-5 |
| AC11 | Column/row 224 emits nothing under X10 and the true coordinate under SGR | TS-6 |
| AC12 | An event over any chrome guard emits nothing and keeps its local behaviour — for every button identity, and for motion and wheel alike | TS-19, TS-21, TS-12 |
| AC13 | The settings surface is untouched | TS-15 |
| AC14 | The CLI-only check succeeds | TS-14 |
| AC15 | Both library test suites pass, including the narrowed fallback test | TS-17, TS-2 |
| AC16 | The byte-exact test inventory ships with the change | TS-18 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0002, task0003, task0004, task0005, task0006 | TS-3, TS-4, TS-10, TS-20, TS-22, TS-23 |
| FR3 | task0002, task0003, task0004, task0005, task0006 | TS-3, TS-9, TS-10, TS-21, TS-23, TS-24 |
| FR4 | task0002, task0003, task0004, task0005, task0006 | TS-3, TS-8, TS-10, TS-19 |
| FR5 | task0002 | TS-3, TS-4, TS-6 |
| FR6 | task0002 | TS-5 |
| FR7 | task0002, task0003, task0004, task0005, task0006 | TS-5, TS-8, TS-11, TS-20, TS-22, TS-24 |
| FR8 | task0003 | TS-10, TS-11 |
| FR9 | task0002 | TS-6 |
| FR10 | task0003, task0004, task0005, task0006 | TS-12, TS-19 |
| FR11 | task0003, task0005, task0006 | TS-10, TS-13, TS-22 |
| NFR1 | task0002, task0003, task0004, task0005, task0006 | TS-7, TS-10, TS-21, TS-23 |
| NFR2 | task0003 | TS-15 |
| NFR3 | task0001, task0002, task0003 | TS-14 |
| NFR4 | task0002, task0003, task0006 | TS-16 |
| NFR5 | task0001, task0002, task0003, task0004, task0005, task0006 | TS-17 |
| NFR6 | task0002, task0004, task0005 | TS-3, TS-4, TS-5, TS-6, TS-7, TS-18 |
| NFR7 | task0002 | TS-3, TS-4, TS-5, TS-6, TS-18 |

## E2E Testing

The project has no E2E harness and NFR5 forbids introducing one, so no automated
end-to-end scenario exists for this feature. The end-to-end coverage is the
manual section below.

## Manual Testing (E2E Not Possible)

Performed against a release build (`make build`, then the release binary under
the host target directory). Each item is a human-judgment scenario that cannot
be asserted without a window, a pointer device and a live application.

- [ ] TS-10: In a mouse-aware application (for example an editor with mouse mode
      enabled, or a pager started without alternate-screen suppression), confirm
      that a click positions the cursor, that a drag selects inside the
      application, and that the wheel scrolls the application's own view on both
      the main and the alternate screen.
- [ ] TS-11: With the same application running, confirm Shift+drag selects
      eMterm text and updates PRIMARY, Shift+Ctrl+click over a URL opens it,
      Shift+middle-click pastes PRIMARY, and Shift+wheel moves eMterm's
      scrollback. Run the Shift+wheel case on the alternate screen too, with the
      alternate-scroll setting enabled: it must still move eMterm's scrollback
      and must not send arrow keys to the application. Then confirm bare
      Ctrl+click and bare middle-click reach the application instead.
- [ ] TS-12: With a tracking mode active, exercise the tab close button, the tab
      strip, the status bar, the scrollbar, the mux sidebar in both persistent
      and overlay placement, and a CSD window edge grab. Each must behave
      exactly as before with nothing reaching the application.
- [ ] TS-13: Attach to a mux pane on a remote host, run a mouse-aware
      application there, and confirm reports reach it through the existing input
      transport.

## Performance Verification

- NFR1: motion report volume is capped at grid resolution, not pointer-pixel
  resolution. Verified by TS-7 (the filter yields exactly the distinct
  consecutive cells) and observed in TS-10 (dragging inside an application
  produces no perceptible CPU or latency regression on the pointer path).
- No load or stress test is specified by SPEC.md, and none is added.

## Security Verification

SPEC.md records no security requirement for this feature. The feature adds no
new input channel — reports travel on the existing PTY input path (FR11) — and
persists nothing (NFR2), so no security check is defined here.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios (TS-1 – TS-9) | 9 | 9 | 0 | 0 |
| Test scenarios (TS-10 – TS-13) | 4 | 0 | 0 | 4 |
| Success-criteria checks (TS-14 – TS-18) | 5 | 2 | 0 | 3 |
| Rework scenarios (TS-19 – TS-21) | 3 | 3 | 0 | 0 |
| Rework scenarios (TS-22 – TS-24) | 3 | 3 | 0 | 0 |
| Build verification | 4 | 4 | 0 | 0 |
| **Total** | **28** | **21** | **0** | **7** |

The three manual success-criteria checks are the inspection items TS-15, TS-16
and TS-18, performed against the integrated diff rather than against a running
binary.
