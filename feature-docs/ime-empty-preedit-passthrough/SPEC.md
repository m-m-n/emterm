# Feature: Empty-Preedit Key Passthrough

## Overview

`WinitImeBridge` currently keeps a single `im_composing` flag that is set by any
non-empty `Ime::Preedit` and cleared only by `Ime::Commit` or `Ime::Disabled`.
An input method that empties its preedit without committing (SKK deleting the
whole ▽ buffer) leaves the flag stuck true, and `dispatch_key_event` then returns
`KeyDispatchResult::Consumed` for every subsequent key, so nothing reaches the
PTY. This feature replaces the single flag with a two-level state model and gates
key suppression on the level that is correct for each platform.

## Objectives

- Restore key delivery to the PTY once the IM's preedit becomes empty on Unix.
- Keep suppressing keys on Windows while an IMM32 composition is alive, even when
  its preedit is empty.
- Replace the factually wrong comment about Wayland cursor-only updates with the
  verified winit behavior.

## User Stories

### US1: SKK conversion buffer emptied

As a Wayland user of fcitx5-skk, I want BackSpace to reach the shell after I have
deleted the entire ▽ buffer, so that I can keep editing my command line without
first typing a throwaway character.

**Acceptance Criteria:**
- [ ] After `Ime::Preedit("▽A")` followed by `Ime::Preedit("")`, the next
      `dispatch_key_event` returns `Passthrough`.
- [ ] Both preedits still reach `pump` so the overlay is drawn and then cleared.

### US2: Windows composition with an external candidate window

As a Windows user of an IME that renders candidates outside the terminal, I want
navigation keys to keep going to the IME while a composition is open, so that
choosing a candidate does not also scroll or edit the shell line.

**Acceptance Criteria:**
- [ ] On Windows, between `Ime::Enabled` and `Ime::Disabled`, `dispatch_key_event`
      returns `Consumed` even when the last preedit was empty.
- [ ] On non-Windows targets, `Ime::Enabled` alone never causes `Consumed`.

## Technical Requirements

### Functional Requirements

- **FR1:** `WinitImeBridge` MUST track two independent boolean states in place of
  `im_composing`: `has_preedit` (the last observed preedit was non-empty) and
  `ime_enabled` (the IME lifecycle is open).
- **FR2:** `Ime::Preedit(text, _)` MUST set `has_preedit = !text.is_empty()` and
  MUST NOT modify `ime_enabled`. The event MUST continue to be mirrored into the
  queue as `ImeEvent::Preedit(text.clone())`, including when `text` is empty.
- **FR3:** `Ime::Enabled` MUST set `ime_enabled = true` and MUST NOT modify
  `has_preedit`. It MUST NOT push any event onto the queue.
- **FR4:** `Ime::Commit(text)` MUST set `has_preedit = false` and MUST NOT modify
  `ime_enabled`. It MUST continue to push `ImeEvent::Commit(text.clone())` only
  when `text` is non-empty.
- **FR5:** `Ime::Disabled` MUST set both `has_preedit = false` and
  `ime_enabled = false`, and MUST continue to push `ImeEvent::FocusOut`.
- **FR6:** `Ime::DeleteSurrounding { .. }` MUST leave both states unchanged and
  MUST remain a documented no-op.
- **FR7:** On `target_os = "windows"`, `dispatch_key_event` MUST return
  `Consumed` when `ime_enabled` is true and `Passthrough` otherwise.
- **FR8:** On every other target, `dispatch_key_event` MUST return `Consumed`
  when `has_preedit` is true and `Passthrough` otherwise.
- **FR9:** The module and field documentation MUST state the verified winit
  behavior that justifies the platform split: winit-wayland emits
  `Ime::Preedit("")` only to clear the preedit and emits `Ime::Enabled` on
  `TextInputEvent::Enter` (whole focus lifetime); winit-x11 emits `Ime::Enabled`
  when the XIC is allowed; winit-win32 maps `Ime::Enabled` /
  `Ime::Disabled` to `WM_IME_STARTCOMPOSITION` / `WM_IME_ENDCOMPOSITION`.
- **FR10:** `notify_focus(false)` MUST clear both `has_preedit` and
  `ime_enabled` before delegating to the window's IME-allowed toggle.
  `notify_focus(true)` MUST NOT modify either state. Rationale: `ime_enabled`
  is otherwise cleared only by `Ime::Disabled`, and winit-win32's
  `ImeRequest::Disable` arm returns without emitting `Ime::Disabled`
  (`winit-win32/src/window.rs`), unlike winit-wayland which synthesizes it
  (`winit-wayland/src/window/mod.rs`). Losing focus mid-composition on Windows
  would otherwise latch the gate open and suppress every subsequent key.
- **FR11:** The key-suppression decision MUST be reachable as a
  platform-parameterized pure predicate over `(has_preedit, ime_enabled,
  windows_gate)`, so that both platform branches are exercisable by unit tests
  on any host. `dispatch_key_event` MUST obtain its answer from that predicate,
  passing the compile-time target as `windows_gate`.

### Non-Functional Requirements

- **NFR1 - Performance:** The key dispatch path MUST stay a constant-time boolean
  check; no allocation or locking may be added to `dispatch_key_event`.
- **NFR2 - Compatibility:** The crate MUST build for `x86_64-unknown-linux-gnu`
  and `x86_64-pc-windows-msvc`, and `cargo check --no-default-features` MUST
  still pass.
- **NFR3 - Testability:** Both platform branches of the gate MUST be covered by
  unit tests in the existing `mod tests` of `winit_bridge.rs` that RUN on the
  development host. Tests gated to a single target with `#[cfg]` do not satisfy
  this requirement on their own, because the project's Windows workflow is a
  build/check only and never executes test code.

## Implementation Approach

### Architecture

The change is confined to one file. The seam between `App` and the OS IME client
(`ImeBackend`) is unchanged; only the backend's internal state machine and its
`dispatch_key_event` predicate change.

```
winit WindowEvent::Ime ─→ WinitImeBridge::on_winit_ime ─→ (has_preedit, ime_enabled)
                                     │                            │
                                     └─→ VecDeque<ImeEvent> ──→ pump ──→ App
                                                                  │
window_host KeyboardInput ─→ dispatch_key_event ──── gate ────────┘
```

### State Machine

| Incoming `Ime` variant | `has_preedit` | `ime_enabled` | Queued event |
|---|---|---|---|
| `Enabled` | unchanged | `true` | none |
| `Preedit(t, _)` | `!t.is_empty()` | unchanged | `Preedit(t)` |
| `Commit(t)` | `false` | unchanged | `Commit(t)` when `t` non-empty |
| `Disabled` | `false` | `false` | `FocusOut` |
| `DeleteSurrounding {..}` | unchanged | unchanged | none |

Gate used by `dispatch_key_event`:

| Target | Predicate |
|---|---|
| `cfg(target_os = "windows")` | `ime_enabled` |
| otherwise | `has_preedit` |

### Rationale for the platform split

Verified against the pinned `winit 0.31.0-beta.2` sources:

- `winit-wayland/src/seat/text_input/mod.rs` emits `Ime::Enabled` from
  `TextInputEvent::Enter`, i.e. once per focus-in, spanning ordinary direct
  input. Gating on it would swallow all typing on Wayland. The same file emits
  an empty `Ime::Preedit` only to clear a previous preedit; cursor-only updates
  carry the still non-empty text.
- `winit-x11/src/ime/mod.rs` emits `Ime::Enabled` when the XIC is created and
  allowed, which likewise spans direct input. `winit-x11/src/event_processor.rs`
  emits `Ime::Preedit("")` for both `ImeEvent::Start` and `ImeEvent::End`, so an
  empty preedit is ambiguous there; this is tolerable because the same file
  suppresses `WindowEvent::KeyboardInput` entirely while its internal composing
  state is set, leaving no application-visible key between Start and End.
- `winit-win32/src/event_loop.rs` sends `Ime::Enabled` from
  `WM_IME_STARTCOMPOSITION` and `Ime::Disabled` from `WM_IME_ENDCOMPOSITION`, so
  on Windows the pair delimits exactly one composition. `winit-win32/src/ime.rs`
  treats a zero-length `GCS_COMPSTR` as a valid empty preedit, so an empty
  preedit inside a live composition is a legitimate Windows state.

### File Structure

```
src-tauri/src/ime/
└── winit_bridge.rs      # struct fields, on_winit_ime, dispatch_key_event, tests
```

No new files, no new dependencies, no public API change.

## Test Scenarios

### Unit Tests

All added to `mod tests` in `src-tauri/src/ime/winit_bridge.rs`.

- [ ] TS-1 (SKK regression, all targets): `Enabled` → `Preedit("▽A")` →
      dispatch is `Consumed` on Windows and `Consumed` on Unix → `Preedit("")` →
      dispatch is `Passthrough` on Unix. Both preedits appear in `pump` output.
- [ ] TS-2 (re-entry, non-Windows): `Preedit("x")` → `Consumed` → `Preedit("")`
      → `Passthrough` → `Preedit("y")` → `Consumed`.
- [ ] TS-3 (winit commit ordering, non-Windows): `Preedit("x")` → `Preedit("")`
      → `Commit("X")` → `Passthrough`; queue order is
      `Preedit("x")`, `Preedit("")`, `Commit("X")`.
- [ ] TS-4 (Enabled is not composition, non-Windows): `Enabled` →
      `Passthrough`.
- [ ] TS-5 (X11 ambiguous start/end, non-Windows): `Preedit("")` →
      `Passthrough` → `Preedit("x")` → `Consumed` → `Preedit("")` →
      `Passthrough`.
- [ ] TS-6 (Windows empty active composition, `cfg(windows)`): `Enabled` →
      `Preedit("")` → `Consumed` → `Disabled` → `Passthrough`.
- [ ] TS-7 (Windows commit does not end the lifecycle, `cfg(windows)`):
      `Enabled` → `Preedit("x")` → `Preedit("")` → `Commit("X")` → `Consumed`
      → `Disabled` → `Passthrough`.
- [ ] TS-8 (DeleteSurrounding is state-neutral): `Preedit("x")` →
      `DeleteSurrounding` → gate unchanged; `Preedit("")` →
      `DeleteSurrounding` → gate unchanged.
- [ ] TS-10 (predicate truth table, runs on every host): the parameterized
      predicate returns, for `windows_gate = true`, exactly `ime_enabled`
      regardless of `has_preedit`; and for `windows_gate = false`, exactly
      `has_preedit` regardless of `ime_enabled`. All four input combinations
      asserted for each value of `windows_gate`.
- [ ] TS-11 (Windows scenarios run on every host): the scenarios of TS-6 and
      TS-7 are ALSO asserted through the parameterized predicate with
      `windows_gate = true`, so their logic executes on a Linux development host
      instead of only compiling for the Windows target. TS-11 supplements TS-6
      and TS-7; it does not replace them. TS-6 and TS-7 must keep asserting
      through `dispatch_key_event` under `cfg(windows)`, because only that path
      proves `dispatch_key_event` passes the compile-time target as the
      selector — a predicate-only test would still pass if that argument were
      hardcoded, silently reverting Windows to the preedit-derived gate this
      feature replaced.
- [ ] TS-12 (focus loss clears the gate): `Enabled` → `Preedit("x")` →
      `notify_focus(false)` → the predicate answers passthrough for BOTH values
      of `windows_gate`.
- [ ] TS-13 (focus gain does not open the gate): `notify_focus(true)` on a
      freshly built bridge leaves both states false, so the predicate answers
      passthrough for both values of `windows_gate`.

### Regression Tests

- [ ] Existing TS-winit-1, TS-winit-2 and TS-winit-4 through TS-winit-7 pass
      unmodified.
- [ ] TS-winit-3 (`commit_clears_composition_and_unblocks_dispatch`) is scoped
      to non-Windows targets. It cannot pass unmodified: it asserts `Consumed`
      after a bare `Preedit` with no preceding `Enabled`, which FR7 makes
      `Passthrough` on Windows, and its second assertion (`Passthrough` after
      `Commit`) contradicts FR4, under which `Commit` does not close the
      lifecycle. Both assertions describe non-Windows semantics exclusively, so
      the whole test is gated rather than rewritten. The equivalent Windows
      sequence is covered by TS-7.
- [ ] `empty_preedit_is_surfaced_for_overlay_clear` still passes; its dispatch
      assertion holds on every target, but for a different reason per platform —
      a bare `Preedit("")` with no preceding `Enabled` leaves BOTH states false.
      That difference must be documented at the test.

### E2E Tests

**Existing E2E tests**: None detected for the native terminal path.
**Run command**: Not detected.

### Manual Verification (host-deferred)

- [ ] TS-manual-1 (Linux Wayland, fcitx5-skk): reproduce the reported steps —
      type `ABC`, enter ▽ mode, delete the whole buffer with BackSpace, then press
      BackSpace and confirm `C` is deleted.
- [ ] TS-manual-2 (Linux Wayland, ordinary kana-kanji IM): same flow with a
      non-SKK input method.
- [ ] TS-manual-3 (Windows): open a composition with an IMM32 IME, confirm arrow
      keys still drive the candidate window and do not reach the shell, and that
      keys reach the shell again after the composition ends.

## Security Considerations

Not applicable. The change narrows the set of keys eMterm withholds; it does not
widen the interpretation of any external input.

## Error Handling

No new error paths. `dispatch_key_event` remains total over its input.

## Success Criteria

- [ ] All functional requirements FR1-FR9 are implemented.
- [ ] All unit test scenarios TS-1 through TS-8 pass.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml` passes.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` passes.
- [ ] `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml` compiles the Windows branch.
- [ ] `cargo fmt` reports no diff for the changed file.

## Assumptions

Decisions taken without user confirmation during batch-mode spec creation.
Each is recorded here so a human reviewer can challenge it.

- **A1:** The fix is the two-level state model with a platform-specific gate,
  not the minimal `im_composing = !text.is_empty()` change proposed in the
  discussion report. Reason: the minimal change would regress the Windows case
  where an IMM32 composition legitimately holds an empty preedit
  (`winit-win32/src/ime.rs` treats a zero-length `GCS_COMPSTR` as valid).
- **A2:** The Windows gate is `ime_enabled` rather than the current
  preedit-derived flag. This is a behavior change on a platform that cannot be
  exercised in this environment; it rests on the source-level mapping of
  `Ime::Enabled` / `Ime::Disabled` to `WM_IME_STARTCOMPOSITION` /
  `WM_IME_ENDCOMPOSITION`. TS-manual-3 is the human gate. Partially mitigated
  by FR11: the Windows branch's LOGIC is now exercised on every host through
  the parameterized predicate (TS-10, TS-11); what remains unverifiable here is
  only whether the real Windows event stream matches the assumed shape.
- **A7:** FR10 clears the gate on focus loss defensively rather than proving
  that Windows fails to deliver `WM_IME_ENDCOMPOSITION` after
  `ImmAssociateContextEx(NULL)`. What IS established from source is that winit
  itself synthesizes no `Ime::Disabled` on the Windows disable path while
  Wayland does. Clearing locally makes the bridge's own state independent of
  that difference; the cost is that a composition surviving a focus round-trip
  would lose its suppression until the next preedit or enable event, which is
  the strictly safer failure direction (keys reach the terminal rather than
  vanishing).
- **A3:** No X11-specific disambiguation of the ambiguous empty preedit is added.
  Reason: winit-x11 does not emit `WindowEvent::KeyboardInput` while its internal
  composing state is set, so the momentary gate release is not observable.
- **A4:** Option C from the discussion report (per-key passthrough while the
  preedit is empty) is rejected because `RawKeyEvent` carries only a physical key
  code and modifiers, with no logical key identity to classify BackSpace / Enter /
  arrows.
- **A5:** `Ime::DeleteSurrounding` stays a no-op; wiring it up is out of scope.
- **A6:** The `design` workflow step is skipped — this feature has no user-visible
  visual surface beyond the existing preedit overlay, whose rendering is unchanged.

## References

- Discussion report: `tmp/discussion-skk-empty-preedit-key-swallow.md` (main tree)
- Implementation site: `src-tauri/src/ime/winit_bridge.rs`
- Dispatch call site: `src-tauri/src/window_host.rs` (`dispatch_key_event_via_ime`)
- Backend trait: `src-tauri/src/ime/backend.rs`
