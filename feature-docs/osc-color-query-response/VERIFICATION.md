# Verification Document: OSC Color Query Response

## Overview

**Feature**: osc-color-query-response
**SPEC.md**: `feature-docs/osc-color-query-response/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/osc-color-query-response/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the merged feature.
Task-level acceptance criteria live in `tasks/taskNNNN.md`.

Scenario IDs `TS-1` … `TS-14` correspond one-to-one with SPEC.md's `TS1` …
`TS14`. `TS-15` … `TS-19` are added here to give FR5, FR10 and NFR4-NFR6
their own verification item. `TS-20` and `TS-21` are added by review round 1's
rework (task0005, task0006); they verify requirements that already exist, and
introduce none.

## Build Verification

All commands run from the project root
(`.claude/rules/core-build-location.md`: never `cd` into `src-tauri/`).

| Component | Command | Expected |
|---|---|---|
| rust_app | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no warnings introduced by this feature |
| rust_term_core | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml` | exit code 0 |
| rust_cli_only | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0 (NFR5) |
| typescript | `bun run build:viewer && bun run build:settings` | exit code 0 — untouched by this feature, regression only |

## Test Verification

| Component | Command |
|---|---|
| rust_app | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` |
| rust_term_core | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` |
| typescript | `bun test` — untouched by this feature, regression only |

**Coverage target**: the project configures no coverage tool, so no numeric
threshold is set. Coverage is judged by the scenario table below: every
scenario marked Unit or Integration must have a passing automated test.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|---|---|---|---|
| TS-1 | Response formatter output shape at boundary components (0x00, 0x7f, 0xff) | Each 8-bit component is expanded to 16 bits and the text form is `rgb:rrrr/gggg/bbbb`; the existing inline tests are extended, not replaced | Unit |
| TS-2 | Single OSC 10 / 11 / 12 query | Returns the live foreground / background / cursor color respectively; a set payload (not a query token) returns no response | Unit |
| TS-3 | Chained default-color payload `?;?;?` | Three responses, codes 10, 11, 12 in that order; a four-element chain stops after code 12; a query element neither consumes the next element nor shifts code progression | Unit |
| TS-4 | OSC 4 query value resolution | Overlay set → overlay value; overlay unset and index < 16 → the 16-color array value; overlay unset and index in 16..=255 → the xterm cube / grayscale value | Unit |
| TS-5 | Mixed set-and-query payload (`1;rgb:10/00/00;5;?`) | The set for index 1 is applied and the query for index 5 is answered, in payload order | Unit |
| TS-6 | OSC 110 / 111 under an active preset scheme | Foreground / background are restored from the scheme mirrors, not from the built-in default constants | Unit |
| TS-7 | OSC 110 / 111 under a user scheme whose color spec fails to parse | Falls back to the construction seed, matching the parse-guarded mirror update | Unit |
| TS-8 | OSC 104 after a palette set | Empty payload clears the overlay and restores the 16-color array from the mirror; an explicit index list restores only the listed indices | Unit |
| TS-9 | OSC 104 change reporting | Reports changed when either the overlay or the 16-color array actually changed (including the newly covered case where only the array changed); reports unchanged when nothing changed | Unit |
| TS-10 | Existing reset tests | The existing reset tests are updated to the new semantics rather than deleted, and pass | Unit |
| TS-11 | Drain of a response produced mid-chunk | The response is drained once and forwarded through the existing device-response write; two separately parsed frames each carrying a query each receive their own response, with none lost or overwritten | Integration |
| TS-12 | Color query embedded in replayed snapshot bytes | The replay and off-thread swap paths discard the pending response; no PTY write occurs. Existing discard regression tests pass unchanged and are extended to cover an OSC color query payload | Integration |
| TS-13 | Frame carrying a color query is not coalesced | The frame is classified as a device query and parsed alone, so a following frame's query cannot overwrite an undelivered response | Integration |
| TS-14 | Manual end-to-end in a real session (see Manual Testing) | Query probes answer with the rendered colors; OSC 110 / 104 redraw in the configured scheme | Manual |
| TS-15 | Response terminator matches the request terminator | A BEL-terminated request is answered with BEL; an ST-terminated request is answered with ST; verified for both OSC 4 and OSC 10 / 11 / 12 | Unit |
| TS-16 | Invalid, unsupported and non-query input is inert | Index ≥ 256, a non-decimal index, a query token under an OSC code the theme does not own, and an unparseable spec element each produce no response bytes, no theme state change, and no visible-change report | Unit |
| TS-17 | Layering | `crates/term_core` builds and its tests pass standalone; its manifest gains no GUI-layer dependency and no dependency on the app crate | Integration |
| TS-18 | CLI-only build | `cargo check --no-default-features` passes with the term_core changes in place | Integration |
| TS-19 | Cross-platform parity | No Unix-only API appears on the response path; the Windows cross-build is unaffected | Manual |
| TS-20 | Color query and color set after an off-thread history swap | A tab whose live core was replaced by the off-thread swap still answers a query (exactly once, correctly terminated, through the existing device-response route) and still applies a set; the answering responder is the same registered instance as before the swap, a swapped and a never-swapped core behave identically, and the existing stale-response discard assertions pass unmodified | Integration |
| TS-21 | Chained color-query fan-out is bounded | A payload built at the OSC payload length limit and packed with query tokens produces at most the declared per-dispatch number of answers, with the remainder dropped before allocation; pending response content stays within the declared byte budget even when the payload is split across several dispatches in one parse pass; payloads within the budgets are unchanged in answers, order and set application | Unit |

## Code Quality Verification

- **Format**: `workflow.yaml` declares no `format_command` for any component.
  Rust formatting is enforced by the project's edit-time hook (formatting
  policy), so no separate format step runs here.
- **Static analysis**: the `cargo check` commands under Build Verification are
  the static-analysis gate. No new compiler warnings may be introduced by the
  feature's files.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|---|---|---|
| SC-A | FR1-FR10 implemented and tested | Functional Requirements Coverage table below; every row has ≥ 1 task and ≥ 1 scenario |
| SC-B | TS-1 … TS-21 pass (TS-14 and TS-19 manual) | Run both Rust test commands; perform the manual section |
| SC-C | NFR1-NFR6 satisfied | TS-1 (NFR1), TS-13 (NFR2), TS-12 (NFR3), TS-17 (NFR4), TS-18 (NFR5), TS-19 (NFR6) |
| SC-D | Both Rust test commands and the CLI-only check pass | Build + Test Verification sections |
| SC-E | Existing replay-discard regression tests pass unchanged | TS-12; confirm the pre-existing assertions were extended, not weakened or removed |
| SC-F | REQUIREMENTS.md 11.1 acceptance criteria met | Manual Testing section plus the scenario table |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|---|---|---|
| FR1 | task0002, task0005 | TS-2, TS-3, TS-14, TS-20 |
| FR2 | task0002, task0005, task0006 | TS-4, TS-14, TS-20, TS-21 |
| FR3 | task0002 | TS-4 |
| FR4 | task0002, task0005, task0006 | TS-5, TS-21 |
| FR5 | task0001, task0002 | TS-15 |
| FR6 | task0004 | TS-11 |
| FR7 | task0003 | TS-6, TS-7, TS-8 |
| FR8 | task0003 | TS-6, TS-7, TS-10, TS-14 |
| FR9 | task0003 | TS-8, TS-9, TS-10, TS-14 |
| FR10 | task0002, task0006 | TS-9, TS-16 |
| NFR1 | task0001, task0002 | TS-1 |
| NFR2 | task0004 | TS-13 |
| NFR3 | task0004, task0005 | TS-12, TS-20 |
| NFR4 | task0001, task0002 | TS-17 |
| NFR5 | task0001 | TS-18 |
| NFR6 | task0001, task0004 | TS-19 |

## E2E Testing

The project has no E2E harness (`test/README.md`), and `workflow.yaml`
declares an empty `e2e_test_command` for every component. There is nothing to
automate at this level; the end-to-end evidence is the manual section below.

## Manual Testing (E2E Not Possible)

Investigation follows `.claude/rules/debugging-constraints.md`: read
`emterm.log` directly and observe the screen — DevTools are unavailable.

- [ ] **TS-14a** With a non-default `terminal_color_scheme` configured, run a
      BEL-terminated background query probe in a shell inside eMterm; a
      response of the expected text form appears and the reported color
      matches the background actually rendered.
- [ ] **TS-14b** Send an ST-terminated foreground query and then the same
      query BEL-terminated; each answer ends with the terminator the request
      used.
- [ ] **TS-14c** Query palette index 5, set index 5 to a known color, query it
      again; the second answer reports the value just set.
- [ ] **TS-14d** Query an index in 16..255 that was never set; the answer is
      that index's xterm cube / grayscale color.
- [ ] **TS-14e** Send OSC 110, then query the foreground: the answer is the
      scheme's foreground and the visible text color is the scheme's
      foreground (not the bright-green fallback). Repeat for OSC 111 and the
      background.
- [ ] **TS-14f** Set palette index 5, send OSC 104 with an empty payload:
      index 5 renders in the active scheme's color 5 again and the query
      answers with the same value. Then set several indices, send OSC 104 with
      only index 5 listed: only index 5 is restored; the others keep their
      set values in both rendering and query answers.
- [ ] **TS-14g** Send invalid queries (out-of-range index, non-decimal index):
      no bytes appear at the prompt and nothing on screen changes.
- [ ] **TS-14h** Confirm no answer ever appears as on-screen text or in the
      shell prompt, including after a window switch, a detach/attach cycle,
      and a scrollback restore.
- [ ] **TS-14i** Launch the reporting TUI (Codex) in eMterm and confirm its
      input area background is now distinct from the chat-log background.
- [ ] **TS-14j** Repeat TS-14a and TS-14e inside a mux session over the
      daemon: the answer arrives exactly once (no duplicate) at the
      originating pane.
- [ ] **TS-19** Review the response path for Unix-only APIs, and confirm the
      Windows cross-build still succeeds (`make win-build`).

No mockup comparison item applies: the design step is `skipped` and the
feature introduces no visual design.

## Performance / Security Verification

- **Answering a query does not dirty the grid** (FR1, FR10): covered by TS-16
  and by the change reporting assertions in TS-9; confirm no full-grid dirty
  mark is triggered by a query-only payload.
- **No undelivered response is lost or overwritten** (NFR2): TS-13 for the
  across-frame case and TS-3 / TS-11 for the within-dispatch case.
- **Single delivery route** (FR6): confirm the feature adds no new PTY write
  path and no second live consumer of the response drain — TS-11 plus a
  review of the merged diff.
- **Replay isolation preserved** (NFR3): TS-12 and TS-20; the pre-existing
  discard assertions must pass without modification, including across the
  off-thread swap once the responder is carried over.
- **Untrusted-input amplification is bounded** (review round 1,
  `ecb0d586e27282de`): TS-21; a single payload at the OSC length limit must
  not convert into unbounded response allocation or unbounded pending
  content.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|---|---|---|---|---|
| Unit | TS-1 … TS-10, TS-15, TS-16, TS-21 (13 IDs) | 13 | 0 | 0 |
| Integration | TS-11, TS-12, TS-13, TS-17, TS-18, TS-20 (6 IDs) | 6 | 0 | 0 |
| Manual | TS-14 (10 checks), TS-19 (2 IDs) | 0 | 0 | 11 checks |
| **Total** | **21 scenario IDs** | **19** | **0** | **2 IDs / 11 checks** |
