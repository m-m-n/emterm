# Feature: mouse-reporting

## Overview

eMterm reports pointer events to the terminal application when that application
enables DEC private mouse-tracking modes 1000 (normal / button tracking), 1002
(button-event / drag tracking) or 1003 (any-event tracking), in either the X10
or the SGR (DECSET 1006) encoding. Today those modes fall through
`MODE_ACTION_TS_FALLBACK`, a dead path left over from the pre-native WebView
build, so the request is accepted and then dropped. Every existing local pointer
behaviour stays reachable behind one predictable escape hatch: holding Shift
suppresses reporting and routes the event to eMterm's existing local handling.

Requirements source: `feature-docs/mouse-reporting/REQUIREMENTS.md`.

## Objectives

- Let terminal applications running under eMterm receive mouse events, so
  full-screen and TUI programs (vim, less, tmux, Claude Code, htop) that request
  DECSET 1000/1002/1003 become mouse-usable instead of silently ignoring the
  pointer.
- Close the last unimplemented arm of eMterm's "Full ANSI control sequence
  support" product claim on the pointer side: DEC private modes
  1000/1002/1003/1006 currently fall through `MODE_ACTION_TS_FALLBACK`, a dead
  path left over from the pre-native WebView build, so the request is accepted
  and then dropped.
- Preserve every existing local pointer behaviour (text selection, Ctrl+click
  link open, middle-click PRIMARY paste, scrollback wheel scroll, DECSET 1007
  arrow translation, egui chrome interaction) behind a single predictable escape
  hatch, so enabling application mouse support never costs the user an existing
  capability.

## User Stories

### US1: Operate a mouse-aware application with the pointer
As a user running a mouse-aware full-screen or TUI program inside eMterm, I want
my clicks, drags and wheel notches to reach that program, so that it becomes
mouse-usable instead of ignoring the pointer.

**Acceptance Criteria:**
- [ ] AC2 — With 1000 active, a left press at grid cell (col 1, row 1) emits `CSI M` followed by bytes 32, 33, 33, and its release emits `CSI M` followed by bytes 35, 33, 33. (FR2, FR5)
- [ ] AC3 — With 1000 and 1006 both active, the same press emits `CSI < 0 ; 1 ; 1 M` and the release emits `CSI < 0 ; 1 ; 1 m`. (FR2, FR5)
- [ ] AC4 — With 1000 active (no 1002/1003), pointer motion emits nothing. With 1002 active, motion with no button held emits nothing, while motion with the left button held emits a report whose button code includes the motion bit 32. With 1003 active, motion with no button held emits a report with button code 3 plus the motion bit. (FR3)
- [ ] AC6 — With any tracking mode active, a wheel-up notch on the MAIN screen emits button code 64 and does not change `App::scroll_offset`; a wheel-down notch emits 65. The same holds on the ALTERNATE screen, and no DECSET 1007 arrow bytes are written. (FR4)
- [ ] AC8 — With a tracking mode active, a Ctrl+left press emits a report whose button code has bit 16 set and does not open the hovered link; a middle press emits button code 1 and does not paste PRIMARY. (FR6, FR8)

### US2: Keep every local pointer behaviour reachable
As a user of eMterm's local pointer features, I want one predictable escape
hatch while an application is tracking the mouse, so that selection, link open,
PRIMARY paste and scrollback scroll never become unreachable.

**Acceptance Criteria:**
- [ ] AC7 — With no tracking mode active, wheel behaviour is byte-for-byte what it is today: DECSET 1007 arrow translation on the alternate screen when the mode bit and `settings.alternate_scroll_enabled` are both on, scrollback scroll otherwise. (FR4)
- [ ] AC9 — With a tracking mode active, Shift+drag produces no report and produces a text selection; Shift+Ctrl+click produces no report and opens the hovered link; Shift+middle-click produces no report and pastes PRIMARY; Shift+wheel produces no report and scrolls eMterm's scrollback. (FR7)
- [ ] AC10 — No emitted report ever has the Shift bit (value 4) set in its button code. (FR6, FR7)
- [ ] AC12 — With a tracking mode active, a press on the tab bar, on the status bar, on the scrollbar overlay, on the mux sidebar (persistent or overlay), on the CSD edge-resize hot zone, or while the profile selector is visible, emits no report and retains its current local behaviour. (FR10)

### US3: Get correct protocol behaviour from the terminal
As a terminal application, I want my DECSET 1000/1002/1003/1006 request to take
effect and my reports to arrive correctly encoded, so that I can rely on the
terminal's mouse support.

**Acceptance Criteria:**
- [ ] AC1 — `CSI ? 1000 h` sets the normal-tracking mode bit and returns `MODE_ACTION_NONE`; `CSI ? 1000 l` clears it. The same holds for 1002, 1003 and 1006. (FR1)
- [ ] AC5 — With 1002 or 1003 active and the pointer moving within one cell, exactly one motion report is emitted; crossing into the next cell emits a second. (NFR1)
- [ ] AC11 — With 1000 active and 1006 inactive, an event at column 224 or row 224 emits no bytes at all; the same event with 1006 active emits `CSI < 0 ; 224 ; ... M` carrying the true coordinate. (FR9, FR5)

## Technical Requirements

### Functional Requirements

- **FR1 — Core-side mouse-tracking mode state:** `TerminalCore::handle_set_mode` tracks DEC private modes 1000 (normal / button tracking), 1002 (button-event / drag tracking), 1003 (any-event tracking) and 1006 (SGR extended encoding) as core-side mode bits, set on `CSI ? Pm h` and cleared on `CSI ? Pm l`, returning `MODE_ACTION_NONE` instead of the current `MODE_ACTION_TS_FALLBACK`. The host reads the active tracking mode and the active encoding through `get_mode`, matching the pattern DECSET 1007 already uses via `MODE_ALTERNATE_SCROLL`.
- **FR2 — Button press and release reporting:** While any of DECSET 1000 / 1002 / 1003 is active, a pointer button press and its matching release inside the terminal grid area are reported to the active tab's PTY instead of driving eMterm's local selection / link / paste handling. Left, middle and right buttons map to button codes 0, 1 and 2; in the X10 encoding a release is reported as button code 3, while in the SGR encoding the press button code is retained and the final byte distinguishes press (`M`) from release (`m`).
- **FR3 — Motion reporting for 1002 and 1003:** Under DECSET 1002 a motion report is emitted only while at least one button is held, carrying that button's code plus the motion bit (value 32). Under DECSET 1003 motion is reported whether or not a button is held; with no button held the button code is 3 plus the motion bit. Under DECSET 1000 alone no motion is reported. Report volume is capped by NFR1.
- **FR4 — Wheel reporting takes precedence over both local wheel consumers:** While any of DECSET 1000 / 1002 / 1003 is active, wheel notches are reported to the application as button code 64 (up) / 65 (down) on BOTH the main screen and the alternate screen. eMterm's scrollback scroll (`App::scroll_up_by` / `scroll_down_by`) and the DECSET 1007 alternate-scroll arrow translation (`alternate_scroll_wheel_bytes`) apply only when no tracking mode is active, making the three wheel consumers mutually exclusive behind one check placed ahead of both existing paths in `handle_mouse_wheel`.
- **FR5 — X10 and SGR (1006) report encodings:** With DECSET 1006 inactive, reports use the X10 encoding `CSI M Cb Cx Cy`, where each of the button byte, the 1-based column and the 1-based row is biased by 32. With DECSET 1006 active, reports use the SGR encoding `CSI < Cb ; Cx ; Cy M` for a press and `... m` for a release, with unbiased decimal button code and 1-based coordinates. Coordinates are always 1-based and expressed in terminal cells derived from the pointer pixel position.
- **FR6 — Modifier bits in the button code:** The button code carries the Ctrl modifier as bit value 16 and the Alt/Meta modifier as bit value 8, taken from the host's current modifier state (`WindowHost::current_mods`). The Shift modifier bit (value 4) is never set in an emitted report, because Shift is consumed locally by FR7 and therefore can never be present at emission time.
- **FR7 — Shift is the sole local override:** Holding Shift suppresses mouse reporting entirely for that event and routes it to eMterm's existing local handling, even while a tracking mode is active. Consequently Shift+drag selects text, Shift+Ctrl+click opens a hovered link, Shift+middle-click pastes PRIMARY, and Shift+wheel scrolls eMterm's scrollback. No other modifier, key, setting or menu suppresses reporting.
- **FR8 — Ctrl+click and middle-click are reported while tracking is active:** While a tracking mode is active and Shift is not held, a Ctrl+left-click is reported to the application with modifier bit 16 set rather than invoking `WindowHost::try_open_link_at_pointer`, and a middle-click is reported with button code 1 rather than performing the `settings.middle_click_paste` PRIMARY paste. Both local behaviours remain reachable via FR7's Shift override, and both are unchanged when no tracking mode is active.
- **FR9 — X10 coordinate overflow suppresses the report:** In the X10 (non-SGR) encoding, an event whose 1-based column or row would exceed 223 (the largest value the 32-biased single byte can represent) produces no report at all. The coordinate is never clamped to 223 and no partial or truncated report is emitted. The SGR encoding has no such limit and reports the true coordinate.
- **FR10 — Existing egui chrome guards run before reporting:** Mouse reporting applies only to events over the terminal grid. Every guard already present in `pointer_routing.rs` keeps precedence over reporting: the profile-selector modal, the CSD title bar + tab bar top strip, the status-bar bottom strip, the right-edge scrollbar overlay, the mux sidebar (persistent panel and overlay card, via `ui::mux_sidebar::point_in_sidebar`), and the CSD edge-resize hot zone. A press, release, motion or wheel event that one of those guards consumes is never reported to the application.
- **FR11 — Reports are written to the active tab's PTY:** Every emitted report is written to the active tab's PTY input channel through the same `Tab::write_input` path the DECSET 1007 arrow translation already uses, so reports reach a local PTY and a mux-attached remote pane identically and inherit the existing input transport without a new channel.

### Non-Functional Requirements

- **NFR1 - Performance:** Motion reports are emitted only on cell change. For DECSET 1002 and 1003, a motion report is emitted only when the (column, row) cell actually differs from the last reported cell; the last reported cell is cached in the host. `handle_pointer_moved` runs once per raw winit motion event on a path the project already optimised for CPU, so report volume must be capped at grid resolution rather than pointer-pixel resolution.
- **NFR2 - Scope / configuration:** No new user setting. The feature introduces no setting: `crates/app_settings` gains no field, the TypeScript `AppSettings` mirror in `src-tauri/web-shared/settings/types.ts` is unchanged, and the settings panel gains no row. Emission is governed solely by the application's DECSET request plus FR7's Shift override.
- **NFR3 - Build compatibility:** CLI-only build keeps compiling. The `crates/term_core` changes (FR1) stay free of GUI-only crates so `cargo check --no-default-features` still succeeds; all winit / egui / pointer-side code (FR2-FR11) lives under the existing `#[cfg(feature = "gui")]` module tree in `src-tauri/src/window_host/`.
- **NFR4 - Portability:** Linux and Windows parity, no platform-specific branches. Reporting behaviour is identical on Linux and Windows. Encoding and routing derive from winit's platform-independent `ButtonSource` / `MouseScrollDelta` / modifier state, so the feature adds no `#[cfg(unix)]` / `#[cfg(windows)]` branch. macOS remains out of scope.
- **NFR5 - Maintainability:** Tests follow the existing inline convention with no new dependency. Verification uses inline `#[cfg(test)] mod tests {}` blocks next to the code under test, in the `fn <subject>_<scenario>_<expected>()` naming style dominant in `crates/term_core/`, with each unit under test constructed explicitly per test and no shared global fixture. No test framework crate is added (the project uses neither `proptest` nor `criterion`), and no E2E harness is introduced.
- **NFR6 - Testability:** Encoding logic is unit-testable without a window. Button-code composition, coordinate biasing, the X10 overflow decision and the cell-change filter are expressed as pure functions taking plain values, so they are exercisable by unit tests without a winit window, a GPU surface or a live PTY — mirroring how `accumulate_alt_scroll_lines` and `alternate_scroll_wheel_bytes` are factored in `window_host::input_translate`.
- **NFR7 - Test coverage:** Report byte-sequence generation ships with unit tests. Unit tests covering the generated report byte sequences are part of the deliverable, not optional follow-up: both encodings (X10 per FR5 and SGR per FR5), every reported event kind (press and release per FR2, motion per FR3, wheel per FR4), the modifier bits per FR6, and the X10 overflow suppression per FR9 each have at least one test asserting the exact emitted bytes. Realised by test scenarios TS3, TS4, TS5 and TS6, which run under NFR5's inline convention and NFR6's window-free factoring.

## Implementation Approach

### Architecture

**System Architecture:**

```
┌─────────────────────────────────────────────────────────┐
│ winit event loop (pointer button / motion / wheel)      │
├─────────────────────────────────────────────────────────┤
│ window_host::pointer_routing — existing egui chrome     │
│ guards (FR10), then the Shift override (FR7)            │
├─────────────────────────────────────────────────────────┤
│ window_host encoding + routing helpers — pure functions │
│ (FR5, FR6, FR9, NFR1, NFR6)                             │
├─────────────────────────────────────────────────────────┤
│ term_core mode bits — DECSET 1000/1002/1003/1006 (FR1)  │
├─────────────────────────────────────────────────────────┤
│ Tab::write_input — active tab's PTY, local or mux (FR11)│
└─────────────────────────────────────────────────────────┘
```

**Component Diagram:**

```
crates/term_core (csi_modes)        : owns the tracking / encoding mode bits (FR1),
                                      read by the host through get_mode
window_host::pointer_routing        : guard precedence (FR10) and the Shift
                                      override (FR7); decides report vs. local
window_host encoding helper (new)   : button-code composition, coordinate biasing,
                                      X10 overflow decision (FR5, FR6, FR9)
window_host cell-change filter (new): caches the last reported cell (NFR1)
window_host::input_translate        : three-way wheel routing decision (FR4),
                                      alongside the existing DECSET 1007 helpers
tabs::Tab::write_input              : transport for the emitted bytes (FR11)
```

### Data Flow

```
winit pointer event
  → chrome guards (FR10)          → consumed: local behaviour, no report
  → Shift held? (FR7)             → yes: eMterm local handling, no report
  → tracking mode active? (FR1)   → no: today's local behaviour, incl. DECSET 1007 (FR4/AC7)
  → motion gate (FR3) / cell-change filter (NFR1)
  → encode (FR5) with modifier bits (FR6); X10 overflow → no report (FR9)
  → Tab::write_input (FR11)       → application (local PTY or mux-attached remote pane)
```

### API Design

No programmatic API is added. The interface is the terminal control-sequence
protocol.

**Input — mode requests (FR1):**

```
CSI ? 1000 h / CSI ? 1000 l    normal / button tracking
CSI ? 1002 h / CSI ? 1002 l    button-event (drag) tracking
CSI ? 1003 h / CSI ? 1003 l    any-event tracking
CSI ? 1006 h / CSI ? 1006 l    SGR extended encoding
```

Each returns `MODE_ACTION_NONE` and toggles its core-side bit.

**Output — report encodings (FR5):**

```
X10 (1006 inactive):  CSI M Cb Cx Cy
                      Cb, Cx, Cy each biased by 32; Cx/Cy are 1-based cells
SGR (1006 active):    CSI < Cb ; Cx ; Cy M      (press)
                      CSI < Cb ; Cx ; Cy m      (release)
                      unbiased decimal button code, 1-based coordinates
```

**Button code composition:**

```
base:      left 0, middle 1, right 2            (FR2)
release:   3 in X10; press code retained in SGR (FR2)
motion:    + 32                                  (FR3)
motion, 1003 with no button held: 3 + 32         (FR3)
wheel:     64 (up) / 65 (down)                   (FR4)
modifiers: + 8 (Alt/Meta), + 16 (Ctrl); 4 (Shift) never set (FR6, FR7)
```

### Database Schema

Not applicable — the feature persists nothing. Its only added state is runtime
state: the core-side mode bits (FR1) and the cached last-reported cell (NFR1).

### Dependencies

**Internal Dependencies:**
- `crates/term_core` (`csi_modes`): owns the DECSET mode bits the host reads via
  `get_mode` (FR1).
- `src-tauri/src/window_host/` (`pointer_routing`, `input_translate`): pointer
  guard precedence, routing and encoding (FR2-FR10).
- `Tab::write_input`: the existing PTY input transport reused for reports (FR11).
- `ui::mux_sidebar::point_in_sidebar`: one of the chrome guards that keeps
  precedence (FR10).

**External Dependencies:**
- winit: supplies the platform-independent `ButtonSource` / `MouseScrollDelta` /
  modifier state the encoding derives from (NFR4). No new crate is added; no test
  framework crate is added either (NFR5).

### File Structure

```
crates/term_core/
└── src/csi_modes.rs                  # FR1 mode bits; TS1, TS2 inline tests
src-tauri/src/window_host/
├── pointer_routing.rs                # FR7, FR8, FR10 guard/override precedence
├── input_translate.rs                # FR4 three-way wheel routing; TS8 inline tests
├── <encoding helper module>          # FR5, FR6, FR9; TS3, TS4, TS5, TS6
└── <cell-change filter / routing helper>  # NFR1, FR3; TS7, TS9
```

Untouched by this change (NFR2, AC13): `crates/app_settings`,
`src-tauri/web-shared/settings/types.ts`, the settings panel.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mouse-reporting/**`
- `test-docs/mouse-reporting/**`

`feature-docs/mouse-reporting/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mouse-reporting/**` covers `test-docs/mouse-reporting/{T}.tests.yaml`,
the per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mouse-reporting/` directory at all; the declared
`test-docs/mouse-reporting/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] TS1 (FR1): DECSET 1000/1002/1003/1006 set and clear their mode bits — for each of 1000, 1002, 1003, 1006 assert `handle_set_mode(mode, true) == 0` (MODE_ACTION_NONE) and `get_mode(...)` is then true, and that the `false` call clears it. Mirrors the existing `decset_1007_toggles_alternate_scroll_bit` construction style. Location: `crates/term_core/src/csi_modes.rs` — inline `#[cfg(test)] mod tests`.
- [ ] TS2 (FR1): `test_mode_ts_fallback` updated for the narrowed fallback set — the existing test asserts 0xFF for `[1, 1000, 1002, 1003, 1005, 1006]`. It must be narrowed to the modes still on the fallback arm and gain positive assertions for the modes moved onto FR1's bits. See reference_impact RI1. Location: `crates/term_core/src/csi_modes.rs` — existing test, must be edited.
- [ ] TS3 (FR2, FR3, FR5, NFR7): X10 encoding of press, release and motion — table-driven: (button, press/release, col, row, mods, motion flag) to expected byte sequence. Includes the (1,1) origin case from AC2 and the release-becomes-3 rule. Also covers the wheel button codes 64/65 in X10 form (FR4). Location: `src-tauri/src/window_host/` (encoding helper module) — inline `#[cfg(test)] mod tests`.
- [ ] TS4 (FR2, FR5, NFR7): SGR (1006) encoding of press and release — same table as TS3 with 1006 active; asserts the `M` / `m` final-byte split and that the press button code is retained on release. Includes the SGR wheel form. Location: `src-tauri/src/window_host/` (encoding helper module).
- [ ] TS5 (FR6, FR7, NFR7): Modifier bits — asserts bit 16 for Ctrl, bit 8 for Alt, both together, and that no input state produces bit 4. Location: `src-tauri/src/window_host/` (encoding helper module).
- [ ] TS7 (NFR1): Motion is filtered to cell changes — feeds a sequence of (col,row) values with repeats and asserts the filter yields exactly the distinct consecutive cells, with the cached last-cell state carried across calls. Location: `src-tauri/src/window_host/` (cell-change filter helper).
- [ ] TS8 (FR4): Wheel routing decision is a pure three-way choice — given (tracking_active, shift_held, alt_screen, mode_1007_bit, alternate_scroll_enabled) assert the chosen consumer is exactly one of report / arrow-translate / scrollback-scroll, covering both screens for the tracking_active case and preserving today's matrix for the tracking-inactive case. Sits alongside the existing `accumulate_alt_scroll_lines` / `alternate_scroll_wheel_bytes` tests. Location: `src-tauri/src/window_host/input_translate.rs` — inline `#[cfg(test)] mod tests`.
- [ ] TS9 (FR3): Motion reporting gate per tracking mode — given (active tracking mode, buttons-held count) assert whether motion reports at all: 1000 never, 1002 only when held, 1003 always. Location: `src-tauri/src/window_host/` (routing helper).

### Integration Tests

None. The resolved test scenarios contain no integration-type scenario; coverage
is split between the unit scenarios above and the manual scenarios below.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] The project has no E2E harness and none is introduced (NFR5); the
      end-to-end coverage below is user-driven manual verification.
- [ ] TS10 (FR2, FR3, FR4, FR8, FR11) — manual: In eMterm run a mouse-aware application (e.g. `vim` with `:set mouse=a`, or `less -X`). Confirm click positions the cursor, drag selects inside the application, and the wheel scrolls the application's own view on BOTH the main and alternate screen. The project has no E2E harness, so this scenario is user-driven. This is the scenario that directly demonstrates the task's goal of operating a mouse-aware TUI application inside eMterm. Location: manual verification against a release build (`make build`; `src-tauri/target-host/release/emterm`).
- [ ] TS11 (FR7, FR8) — manual: With the same mouse-aware application running: Shift+drag selects eMterm text (PRIMARY updated), Shift+Ctrl+click over a URL opens it, Shift+middle-click pastes PRIMARY, Shift+wheel moves eMterm's scrollback. Then confirm bare Ctrl+click and bare middle-click reach the application instead. Location: manual verification against a release build.
- [ ] TS12 (FR10) — manual: With a tracking mode active, click a tab's close button, drag the tab strip with the wheel, click the status bar, drag the scrollbar, click and wheel the mux sidebar in both persistent and overlay placement, and grab a CSD window edge. Each must behave exactly as before, with nothing reaching the application. Location: manual verification against a release build.
- [ ] TS13 (FR11) — manual: Attach to a mux pane on a remote host, run a mouse-aware application there, and confirm reports reach it through the existing `write_input` transport. Location: manual verification against a release build with `emterm mux`.

### Edge Cases

- [ ] TS6 (FR9, NFR7): X10 coordinate overflow yields no report — col=223 and row=223 encode; col=224 or row=224 returns None under X10 and encodes normally under SGR. Boundary-exact per AC11. Location: `src-tauri/src/window_host/` (encoding helper module).
- [ ] Motion under DECSET 1000 alone emits nothing; under 1002 motion with no button held emits nothing (FR3, AC4; TS9).
- [ ] Pointer motion within a single cell emits exactly one report; crossing into the next cell emits a second (NFR1, AC5; TS7).
- [ ] An event consumed by an egui chrome guard is never reported and keeps its current local behaviour (FR10, AC12; TS12).
- [ ] Shift suppresses reporting entirely and routes the event to local handling (FR7, AC9; TS11).

### Performance Tests

No load or stress test is specified. The feature's performance constraint is
NFR1 (motion reports capped at cell granularity), verified by TS7 and AC5.

## Security Considerations

The resolved requirements contain no security requirement for this feature, so
none is specified here. The feature adds no new input channel: reports travel on
the existing `Tab::write_input` PTY path (FR11), and no setting or stored data is
introduced (NFR2).

## Error Handling

No error codes are introduced. The single defined "no output" condition is FR9:

| Condition | Behaviour |
|---|---|
| X10 encoding active and the 1-based column or row exceeds 223 | No report at all; the coordinate is never clamped and no partial or truncated report is emitted (FR9, AC11) |

### Error Flow

```
Event → guards (FR10) → Shift override (FR7) → tracking active? (FR1)
      → motion gate (FR3) / cell-change filter (NFR1)
      → encode (FR5): unencodable X10 coordinate → emit nothing (FR9)
```

## Performance Optimization

### Performance Goals

- Motion report volume is capped at grid resolution rather than pointer-pixel
  resolution (NFR1).

### Optimization Strategies

- Cell-change filter: the host caches the last reported (column, row) and emits a
  motion report only when the cell actually differs (NFR1, TS7). This matters
  because `handle_pointer_moved` runs once per raw winit motion event on a path
  the project already optimised for CPU.

### Caching Strategy

- Last reported (column, row) cell: cached in the host for the lifetime of the
  tracking session (NFR1).

## Success Criteria

- [ ] All functional requirements (FR1-FR11) are implemented and tested
- [ ] All test scenarios (TS1-TS13) pass
- [ ] Performance meets NFR1 (motion reports only on cell change)
- [ ] AC13 — `crates/app_settings`, `src-tauri/web-shared/settings/types.ts` and the settings panel are untouched by the change. (NFR2)
- [ ] AC14 — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` succeeds. (NFR3)
- [ ] AC15 — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` and `... --manifest-path src-tauri/Cargo.toml --lib` both pass, including the updated `test_mode_ts_fallback`. (NFR5, reference_impact RI1)
- [ ] AC16 — The merged change contains unit tests asserting the exact emitted byte sequence for: an X10 press, an X10 release, an SGR press, an SGR release, a motion report, a wheel-up and a wheel-down report, a Ctrl-modified report, an Alt-modified report, and an X10 event beyond column/row 223 (asserting no bytes). Each runs without a winit window or a live PTY. (NFR7, NFR6)
- [ ] Documentation is complete
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement (FR1-FR11, NFR1-NFR7) has `status: resolved`; no
requirement carries a `tbd_reason`.

## Assumptions

These are the assumptions recorded with the resolved requirements; they govern
the decisions above.

- **A1** (impact: high, reversible): While any tracking mode is active, wheel notches are reported as buttons 64/65 on both the main and the alternate screen; eMterm's scrollback scroll and the DECSET 1007 arrow translation apply only when no tracking mode is active.
- **A2** (impact: medium, reversible): No new setting is introduced; emission follows the application's DECSET request alone.
- **A3** (impact: high, reversible): Shift is the sole local override; Ctrl+click and middle-click are reported while tracking is active. While a tracking mode is active, BARE drag no longer selects text — it reports — and text selection stays reachable only via Shift.
- **A4** (impact: medium, reversible): In the X10 encoding, a column or row above 223 produces no report; coordinates are never clamped.
- **A5** (impact: medium, reversible): Motion reports for 1002/1003 are emitted only when the reported cell changes.
- **A6** (impact: medium, reversible): The `MODE_ACTION_TS_FALLBACK` return value is a dead path in the current native build, so moving 1000/1002/1003/1006 off it changes no live consumer other than the test that pins it.
- **A7** (impact: low, reversible): DECSET 1005 (UTF-8 extended reporting) and DECSET 1015 (urxvt extended reporting) are both out of scope. 1005's existing `MODE_ACTION_TS_FALLBACK` arm is left intact, and 1015 is deliberately not given a match arm, so it continues to fall through the `_ => MODE_ACTION_NONE` unknown-mode arm and remains a silent no-op.
- **A8** (impact: low, reversible): Reports are written to the active tab's PTY via the existing `Tab::write_input` path rather than a new channel.
- **A9** (impact: medium, reversible): Coordinates are 1-based and derived from the existing `WindowHost::pixel_to_cell` grid mapping (screen row, not absolute buffer row).

## Implementation Phases (if applicable)

Not applicable — the resolved requirements define no phasing.

## References

- Requirements document: `feature-docs/mouse-reporting/REQUIREMENTS.md`
- `.claude/rules/core-architecture.md`: describes the TS side as the removed
  pre-native WebView build (cited by assumption A6)
- `.claude/rules/core-build-location.md`, `.claude/rules/core-commands.md`: the
  build and test commands named in AC14, AC15 and TS10
