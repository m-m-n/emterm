# Feature: selection-clear-on-enter-copy

## Overview

TUI programs such as Claude Code rewrite lines in place, which leaves stale
mouse-selection highlights on screen; none of the six existing selection-clear
conditions is driven by key input, so none of them resolves this. This feature
adds two key-input-driven triggers — forwarding Enter to the PTY, and copying via
`keybinds.copy` — that clear `selection` and `pending_selection_anchor` as a pair.
The trigger set is deliberately limited to those two so that "select, read, and
keep typing" keeps working.

Requirements document: `feature-docs/selection-clear-on-enter-copy/REQUIREMENTS.md`.

## Objectives

- Resolve the mouse-selection highlight left behind by TUI line rewrites (Claude
  Code and similar) through a key-input-driven trigger.
- Add "Enter forwarded to the PTY" and "copy via `keybinds.copy`" to the six
  existing selection-clear conditions.
- Keep the trigger set limited to Enter and copy so the "select, read, and keep
  typing" use case is not broken.

## User Stories

### US1: Clearing the selection by pressing Enter

As a user running a TUI in eMterm, I want the selection highlight to disappear
when I press Enter, so that highlights stranded by line rewrites go away as I keep
working.

**Acceptance Criteria:**
- [ ] AC1: The selection is cleared when Enter is sent to the PTY.
- [ ] AC2: Shift+Enter also clears it (all `shift_enter_behavior` modes).
- [ ] AC5: The IME composition-commit Enter does not clear the selection.
- [ ] AC6: Pressing a modifier key alone does not clear the selection.
- [ ] AC10: PTY-forwarded keys other than Enter (printable keys, cursor keys, etc.)
      do not clear the selection.

### US2: Clearing the selection by copying

As a user running a TUI in eMterm, I want the selection highlight to disappear
once I have copied it, so that the highlight does not outlive the copy operation.

**Acceptance Criteria:**
- [ ] AC3: The selection is cleared when copied via `keybinds.copy`.
- [ ] AC4: `keybinds.copy` with no selection only consumes the chord and does
      nothing else.
- [ ] AC7: Clearing drops both `selection` and `pending_selection_anchor`.
- [ ] AC8: The highlight actually disappears after clearing (a redraw happens).
- [ ] AC9: An already-set PRIMARY is unaffected and middle-click paste keeps
      working as before.
- [ ] AC11: Unit tests exist that verify the above.

## Technical Requirements

### Functional Requirements

- **FR1 — Clear the selection when Enter is forwarded:** When Enter is sent to the
  PTY, clear `selection` and `pending_selection_anchor`. The implementation site is
  inside the `if forwarded` branch at `src-tauri/src/window_host/event_loop.rs:497`,
  adding the existing local `is_enter` at line 448 of the same file
  (`matches!(event.logical_key, WinitKey::Named(NamedKey::Enter))`) as a condition.
  PTY-forwarded keys other than Enter (ordinary printable keys, cursor keys, etc.)
  do not clear.
- **FR2 — Clear on Shift+Enter:** Shift+Enter clears in every `shift_enter_behavior`
  mode (None / AltEnter / KittyCsiU / Lf). `is_enter` is evaluated before
  `shift_enter_rewrite` is applied, so it is true in all modes including the
  RawBytes path.
- **FR3 — Clear on copy:** When `keybinds.copy` (default Ctrl+Shift+C) actually
  copies, clear inside the `if let Some(sel) = app.selection` block at
  `src-tauri/src/window_host/key_routing.rs:101`, immediately after
  `host.set_clipboard(&text)`.
- **FR4 — Preserve current behaviour of copy with no selection:** `keybinds.copy`
  with no selection keeps its current behaviour of merely consuming the chord with
  `return true`, with no side effect on the clipboard or on selection state.
- **FR5 — Clear as a pair:** Clearing always sets both `selection` and
  `pending_selection_anchor` to `None`. The pairing used by the six existing sites
  is preserved by routing through a shared helper.
- **FR6 — Redraw:** The highlight actually disappears in the frame after clearing
  (a redraw happens).

### Non-Functional Requirements

- **NFR1 - Compatibility:** Do not change the behaviour of the six existing clear
  conditions (left-button press / tab switch / column-count-changing resize /
  alt-screen enter-exit / frame reset / scrollback eviction).
- **NFR2 - Compatibility:** Do not clear on the IME composition-commit Enter or on a
  modifier key pressed alone. Both are already satisfied without an extra guard by
  the existing early `return` and by `winit_key_to_bytes` returning `None`.
- **NFR3 - Compatibility:** Do not change PRIMARY selection or middle-click paste
  behaviour.
- **NFR4 - Compatibility:** The change is confined to the GUI feature; the
  `--no-default-features` (CLI-only) build is unaffected.
- **NFR5 - Performance:** Do not regress the existing render-skip optimizations
  (`should_skip_frame` / dirty rows).
- **NFR6 - Maintainability:** Factor the clear decision out as a side-effect-free
  pure function, in the same shape as the existing `shift_enter_rewrite`, so it can
  be verified in isolation.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────┐
│  winit event loop                   │
│  window_host/event_loop.rs          │
├─────────────────────────────────────┤
│  key routing / input translation    │
│  window_host/key_routing.rs         │
│  window_host/input_translate.rs     │
├─────────────────────────────────────┤
│  application state (App)            │
│  app/  — selection,                 │
│         pending_selection_anchor    │
├─────────────────────────────────────┤
│  render (dirty rows / frame skip)   │
└─────────────────────────────────────┘
```

**Component Diagram:**
```
event_loop.rs  --(forwarded && is_enter)-->  App::clear_selection()
key_routing.rs --(after host.set_clipboard(&text), inside
                  `if let Some(sel) = app.selection`)-->  App::clear_selection()

input_translate.rs
  should_clear_selection_on_forward(is_enter, forwarded) -> bool   (pure)
  shift_enter_rewrite(..)                                          (existing, pure)

App::clear_selection()
  selection = None
  pending_selection_anchor = None
```

### Data Flow

```
Key press → event_loop.rs: evaluate is_enter (line 448)
          → shift_enter_rewrite applied
          → forwarded to PTY (line 497, `if forwarded`)
          → is_enter ? App::clear_selection() : no-op
          → next frame: dirty_rows_this_frame unions selection and
            previous_selection → old highlight rows repainted

keybinds.copy → key_routing.rs:101 `if let Some(sel) = app.selection`
              → host.set_clipboard(&text)
              → App::clear_selection()
              → next frame: same redraw path
```

### API Design

N/A — no external or HTTP API surface is added or changed; this is internal key
handling and in-process application state.

### Database Schema

N/A — no persisted data; the feature touches two runtime fields on `App`.

#### Entity Relationship Diagram

N/A — no entities are persisted.

### Dependencies

**Internal Dependencies:**
- `window_host/event_loop.rs`: supplies `forwarded` and the existing `is_enter`
  local (line 448) at the Enter clear site (line 497).
- `window_host/key_routing.rs`: supplies the copy branch (line 101) and
  `host.set_clipboard(&text)`.
- `window_host/input_translate.rs`: home of the existing pure `shift_enter_rewrite`
  and of the new pure clear-decision function.
- `app/`: owns `selection` and `pending_selection_anchor`, and the existing
  `dirty_rows_this_frame` behaviour that unions `selection` and
  `previous_selection`.
- `window_host/pointer_routing.rs`: the fold-click decision at line 355
  (`pending.is_some()`) that EC1 concerns; not modified.

**External Dependencies:**
- winit: `logical_key` / `NamedKey::Enter`, already in use.
- No new external dependency is introduced.

### File Structure

```
src-tauri/src/
├── window_host/
│   ├── event_loop.rs        # Enter clear site (inside `if forwarded`, line 497)
│   ├── key_routing.rs       # copy clear site (after set_clipboard, line 101)
│   ├── input_translate.rs   # pure clear-decision function
│   ├── pointer_routing.rs   # PRIMARY / fold-click paths (unchanged)
│   └── tests.rs             # pure-function tests + include_str! source scans
└── app/
    ├── mod.rs               # new clear_selection helper
    └── tests.rs             # App state-transition tests
```

## Declared Change Set

Expected feature-specific touch points:

- `src-tauri/src/window_host/event_loop.rs`
- `src-tauri/src/window_host/key_routing.rs`
- `src-tauri/src/window_host/input_translate.rs`
- `src-tauri/src/window_host/tests.rs`
- `src-tauri/src/app/mod.rs` (new `clear_selection` helper)
- `src-tauri/src/app/tests.rs`

Beyond the paths above, the exact feature-specific set is derived at create-plan
from every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths above:

- `feature-docs/selection-clear-on-enter-copy/**`
- `test-docs/selection-clear-on-enter-copy/**`

`feature-docs/selection-clear-on-enter-copy/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and restates
none of their rules.

`test-docs/selection-clear-on-enter-copy/**` covers
`test-docs/selection-clear-on-enter-copy/{T}.tests.yaml`, the per-task test record.
It is generated and owned by `implement-phase.md`; this section cites it and
restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is a
deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it. A feature
that produces no implement tasks generates no `test-docs/{feature}/` directory at
all; the declared `test-docs/selection-clear-on-enter-copy/**` entry is still
correct in that case — a declared path that never materializes is not a violation.

## Test Scenarios

Test command:
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`

Build command:
`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`

### Unit Tests

- [ ] TS1 (pure function; covers AC1 / AC2 / AC10 → FR1, FR2, NFR6): Factor the
      clear decision out as a pure function (e.g.
      `should_clear_selection_on_forward(is_enter, forwarded) -> bool`) in
      `src-tauri/src/window_host/input_translate.rs`, and cover its truth table in
      the same shape as the existing `shift_enter_rewrite` tests
      (`src-tauri/src/window_host/tests.rs:1748-1907`). Expected: true only for
      `forwarded=true && is_enter=true`; false for `forwarded=true, is_enter=false`;
      false for `forwarded=false`.
- [ ] TS2 (pure function; covers AC2 → FR2): For each of the four
      `shift_enter_behavior` modes, pin that `is_enter` stays true regardless of the
      `shift_enter_rewrite` result (Modifiers / RawBytes), added next to the existing
      tests. Expected: `is_enter` true in all four modes.
- [ ] TS3 (App state transition; covers AC7 → FR5): Add an `App` helper that drops
      `selection` and `pending_selection_anchor` as a pair (e.g.
      `App::clear_selection()`) and verify directly, on an `App` built with the
      existing builder in `src-tauri/src/app/tests.rs`, that calling it with both
      `Some` leaves both `None`. Expected: both fields become `None`.
- [ ] TS4 (App state transition; covers AC4 → FR4): Calling the same helper on an
      `App` whose `selection` is `None` changes no state and passes nothing to the
      clipboard side. Expected: state unchanged, no clipboard write.
- [ ] TS5 (source scan; covers AC1 / AC3 / AC7 → FR1, FR3, FR5): Add `include_str!`
      source-scan assertions to `src-tauri/src/window_host/tests.rs` pinning that
      (a) the `if forwarded` block in `event_loop.rs` contains an `is_enter`-conditioned
      clear call, and (b) the copy branch in `key_routing.rs` has a clear call
      immediately after `host.set_clipboard(&text)`, inside `if let Some(sel)`. Use the
      same shape as the existing `pointer_routing.rs` scan tests
      (`tests.rs:1488/1536/1581/1629`). Rationale: the call sites themselves cannot be
      driven from a test — the `event_loop` branch lives inside the winit event loop,
      and `key_routing`'s `handle_special_chord` takes a concrete `&mut WindowHost` and
      requires a real window, so neither is callable from a unit test.
- [ ] TS6 (source scan; covers AC5 / AC6 → NFR2): Pin, with the same source-scan
      assertions, that the IME consume path and modifier-key-alone presses never reach
      the clear site (early `return` / `winit_key_to_bytes` returning `None`). Expected:
      the structure holds as asserted.
- [ ] TS7 (App state transition; covers AC8 → FR6): Using the existing behaviour where
      `dirty_rows_this_frame` unions `selection` and `previous_selection` into the dirty
      rows, verify at the `App` level that the frame after `selection` goes from `Some` to
      `None` includes the old highlight rows in the dirty rows. Expected: old highlight
      rows are dirty.
- [ ] TS8 (source scan; covers AC9 → NFR3): Confirm the PRIMARY-set path (`set_primary`
      on pointer release) and the middle-click paste path (`get_primary`) are untouched by
      this change. Expected: both paths unchanged.

### Integration Tests

N/A — the two call sites cannot be driven from an automated test (see TS5's
rationale); their placement is pinned by source-scan assertions instead, and the
end-to-end behaviour is covered by the manual scenario TS9.

### E2E Tests

**Existing E2E tests**: None (`test/README.md`: "E2E Tests: None at the moment")
**Run command**: Not detected
- [ ] TS9 (manual; covers AC1 / AC2 / AC3 / AC10 → FR1, FR2, FR3): On a release
      build, make a selection and confirm the highlight disappears on Enter /
      Shift+Enter / Ctrl+Shift+C and does not disappear on printable-key input.
      Run only on the user's explicit instruction.

### Edge Cases

- [ ] EC1: Pressing Enter while the left button is held (`pending_selection_anchor`
      is `Some`) drops the anchor, so the fold-click decision on release
      (`pointer_routing.rs:355`, `pending.is_some()`) does not hold and the fold
      toggle misfires once. Accepted (ASM5).
- [ ] EC2: In a mux-connected tab, Enter also reaches `forwarded == true` through the
      `PtyInput` frame path, so it is handled identically.
- [ ] EC3: Enter while viewing scrollback snaps to the live tail via the existing
      `scroll_to_live()` in the same branch, so the viewport moves in the same frame
      as the clear.
- [ ] EC4: On frames where `fold_layout` is active, `dirty_rows_this_frame` returns
      all rows, so the redraw cost of a clear is the whole viewport (existing
      behaviour).
- [ ] EC5: Newlines contained in a paste (bracketed paste / middle click) do not pass
      through this branch and do not clear.
- [ ] EC6: A copy whose resolved selection text is empty still clears, because the
      clear sits inside `if let Some(sel)`.
- [ ] EC7: Numpad Enter / Ctrl+Enter / Alt+Enter all produce `logical_key ==
      Named(Enter)` and are therefore clear triggers even under the Enter-only rule
      (ASM1).
- [ ] EC8: Printable keys and cursor keys do not clear even with `forwarded == true`;
      ASM1 rejects them because `is_enter` is false.

### Performance Tests

N/A — no load- or stress-testable workload is added. NFR5 (no regression of the
render-skip optimizations) is covered by the dirty-row verification in TS7.

## Security Considerations

- **SC1 — Clipboard / PRIMARY:** Never write to clipboard or PRIMARY content; the
  existing bracketed-paste sanitization path is not touched.
- **Authentication:** N/A — the feature crosses no authentication boundary.
- **Authorization:** N/A — the feature crosses no authorization boundary.
- **Input Validation:** N/A — no external input is parsed; the change is a branch on
  an already-decoded key event.
- **Data Protection:** N/A — no sensitive data is stored or transmitted; two runtime
  fields are set to `None`.
- **XSS Prevention:** N/A — no WebView or HTML surface is involved.
- **SQL Injection Prevention:** N/A — no database or query construction is involved.
- **CSRF Protection:** N/A — no HTTP request surface is involved.

## Error Handling

### Error Codes

N/A — the change has no failure path; it sets two runtime fields to `None`.

### Error Flow

N/A — no error is raised by either clear site.

## Performance Optimization

### Performance Goals

N/A — no numeric latency or throughput target applies. The applicable constraint is
NFR5: do not regress the existing render-skip optimizations
(`should_skip_frame` / dirty rows).

### Optimization Strategies

- Reuse the existing dirty-row machinery: `dirty_rows_this_frame` already unions
  `selection` and `previous_selection`, so clearing repaints exactly the old
  highlight rows rather than forcing a full-frame repaint (EC4 notes the existing
  exception when `fold_layout` is active).

### Caching Strategy

N/A — nothing is cached by this feature.

## Success Criteria

- [ ] All functional requirements (FR1–FR6) are implemented and tested
- [ ] All test scenarios (TS1–TS9) pass
- [ ] NFR5 holds: the render-skip optimizations are not regressed
- [ ] Security requirements are satisfied (SC1)
- [ ] Documentation is complete (REQUIREMENTS.md, SPEC.md)
- [ ] Code review is completed
- [ ] AC1–AC11 are all satisfied
- [ ] NFR4 holds: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
      src-tauri/Cargo.toml --no-default-features` still succeeds

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. Every requirement (FR1–FR6, NFR1–NFR6) is `status: resolved`.

## Implementation Phases (if applicable)

N/A — the change is confined to two branch sites, one new helper, and their tests;
it is not staged into phases.

## References

- Requirements document: `feature-docs/selection-clear-on-enter-copy/REQUIREMENTS.md`
- Enter clear site: `src-tauri/src/window_host/event_loop.rs` (line 448 `is_enter`,
  line 497 `if forwarded`)
- Copy clear site: `src-tauri/src/window_host/key_routing.rs` (line 101
  `if let Some(sel) = app.selection`)
- Pure-function home: `src-tauri/src/window_host/input_translate.rs`
- Existing test shapes: `src-tauri/src/window_host/tests.rs` (`shift_enter_rewrite`
  tests 1748-1907; `pointer_routing.rs` source scans 1488 / 1536 / 1581 / 1629)
- App test builder: `src-tauri/src/app/tests.rs`
- Fold-click decision (EC1): `src-tauri/src/window_host/pointer_routing.rs:355`
- E2E infrastructure status: `test/README.md`
