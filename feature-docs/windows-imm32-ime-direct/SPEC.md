# Feature: windows-imm32-ime-direct

## Overview

On Windows, composing text with CorvusSKK freezes eMterm ("not responding").
This feature routes the Windows `set_ime_cursor_area` path around winit by
calling IMM32 directly from a `#[cfg(windows)]` `BridgeWindow` implementation,
and defers the IME detach (`set_ime_allowed(false)`) until the composition has
ended. winit itself is not modified, and X11 / Wayland plus the CLI-only build
keep their current behavior.

Requirements document: `feature-docs/windows-imm32-ime-direct/REQUIREMENTS.md`.

## Objectives

- Eliminate the "not responding" freeze on Windows when composing text with
  CorvusSKK, without modifying winit itself.
- Preserve current IME behavior on X11 / Wayland and in the CLI-only build.

## Root Cause (settled; no re-diagnosis in scope)

The mechanism is settled by a full-memory hang dump analysis:

1. winit-win32 0.31.0-beta.2 `window.rs:1025` holds the `WindowState` mutex
   across its `Imm*` calls.
2. TSF / CorvusSKK re-enters wndproc on the same thread while that mutex is
   held.
3. `event_loop.rs:121` re-locks the same mutex on that re-entry and blocks
   forever.

The analysis record lives in a project-local `tmp/` note, which is gitignored;
the mechanism is therefore carried in this document's own text rather than by
reference to that path.

## User Stories

### US1: Repeated conversion commits do not freeze the app
As a Windows + CorvusSKK user, I want to commit conversions repeatedly, so that
I can keep typing Japanese without the app becoming unresponsive.

**Acceptance Criteria:**
- [ ] AC1: `set_ime_cursor_area` calls IMM32 directly and does not pass through
      winit's `request_ime_update` (code review + unit tests with a mock
      `BridgeWindow`; automatable on any host).
- [ ] AC3: On a real Windows + CorvusSKK machine, repeated conversion commits do
      not freeze the app. **Manual, real-device verification only** (cannot run
      on the Linux host or in CI).

### US2: Focus-out mid-conversion does not freeze the app
As a Windows + CorvusSKK user, I want to Alt+Tab away during an active
conversion, so that switching windows never wedges the terminal.

**Acceptance Criteria:**
- [ ] AC2: While a composition is alive, `set_ime_allowed(false)` is not sent; it
      is sent by the flush after `Ime::Disabled` is received (unit tests against
      the bridge's pending-state logic; automatable on any host).
- [ ] AC4: On the real device, Alt+Tab focus-out during an active conversion does
      not freeze the app. **Manual, real-device verification only.**

### US3: The candidate window keeps tracking the caret
As a Windows + CorvusSKK user, I want the candidate window to follow the cursor,
so that conversion candidates appear where I am typing.

**Acceptance Criteria:**
- [ ] AC5: On the real device, the candidate window follows the cursor position.
      **Manual, real-device verification only.**

### US4: Non-Windows and CLI-only builds are unaffected
As an X11 / Wayland user and as a CLI-only build user, I want IME behavior and
the CLI build to be unchanged, so that this fix carries no regression cost.

**Acceptance Criteria:**
- [ ] AC6: X11 / Wayland IME behavior is unchanged (existing `winit_bridge` unit
      test suite stays green + manual spot check on the Linux host).
- [ ] AC7: `cargo check --no-default-features` (CLI-only) passes.

## Technical Requirements

### Functional Requirements

- **FR1 - Windows cursor-area path bypasses winit:** On Windows,
  `set_ime_cursor_area` does not go through winit's `request_ime_update`; a
  `#[cfg(windows)]` `BridgeWindow` implementation in
  `src-tauri/src/ime/winit_bridge.rs` calls IMM32 directly, so the call never
  passes through winit's `Mutex<WindowState>` and the TSF re-entry into wndproc
  can acquire that mutex normally.
- **FR2 - IMM32 call sequence and window forms match winit:** The direct path
  performs `ImmGetContext` -> no-op if null -> `ImmSetCompositionWindow`
  (`CFS_POINT`, `ptCurrentPos` = `(x, y + height)`) -> `ImmSetCandidateWindow`
  (`CFS_EXCLUDE`, `ptCurrentPos` = `(x, y)`) -> `ImmReleaseContext`, with
  `rcArea` = `(x, y, x + width, y + height)` for both. Coordinates are physical
  pixels, passed through without conversion.
- **FR3 - Deferred IME detach:** While a composition is alive,
  `set_ime_allowed(false)` is not sent; it is sent by a `flush` after
  `Ime::Disabled` is received. `pending_allow` remains last-writer-wins, so a
  focus-in during a pending detach overwrites and cancels it.
- **FR4 - Enable stays winit-routed:** `set_ime_allowed(true)`
  (`ImeRequest::Enable`) continues through winit, because winit gates WM_IME_*
  processing on `ime_capabilities.is_some()` (winit-win32
  `event_loop.rs:1415/1428/1479`); bypassing Enable would stop all
  `WindowEvent::Ime` delivery.
- **FR5 - Non-Windows path unchanged:** X11 / Wayland keep the current
  winit-routed `BridgeWindow` implementation; observable IME behavior on those
  targets does not change.
- **FR6 - HWND acquisition and dependency:** The HWND is obtained from winit's
  `Window` via `rwh_06::HasWindowHandle`. The existing direct `windows-sys`
  dependency (0.59, already carrying `Win32_Foundation` /
  `Win32_System_Console` in `src-tauri/Cargo.toml`) gains the
  `Win32_UI_Input_Ime` feature.

### Non-Functional Requirements

- **NFR1 - Maintainability (no winit modification):** winit is not forked or
  patched (`[patch.crates-io]` is out of scope); the pinned
  `winit = "=0.31.0-beta.2"` dependency stays as-is.
- **NFR2 - Compatibility (CLI-only build unaffected):** `cargo check
  --no-default-features` (CLI-only, no winit) continues to pass; the new code
  lives behind the `gui` feature and `#[cfg(windows)]`.
- **NFR3 - Availability (IMM32 thread requirement satisfied by existing
  flush):** IMM32 calls run on the event-loop thread: `flush` executes inside
  `about_to_wait`, which satisfies IMM32's calling-thread requirement without
  new threading machinery.

## Implementation Approach

### Architecture

The integration seam is the existing `BridgeWindow` trait
(`set_ime_allowed` / `set_ime_cursor_area`). The deferred-flush architecture
(`pending_allow` / `pending_cursor_area`, flushed from `about_to_wait`) already
exists and is kept; only the Windows sink behind `BridgeWindow` changes.

```
IME state changes
    -> record into pending_allow / pending_cursor_area   (unchanged)
    -> flush() from about_to_wait                        (unchanged)
        -> BridgeWindow::set_ime_allowed(true)   --> winit  (FR4, all targets)
        -> BridgeWindow::set_ime_allowed(false)  --> winit, deferred until
                                                     Ime::Disabled  (FR3)
        -> BridgeWindow::set_ime_cursor_area(..)
             - #[cfg(windows)]   --> IMM32 direct   (FR1, FR2)
             - other targets     --> winit          (FR5)
```

### Windows cursor-area call recipe (FR2)

Executed on the event-loop thread, inside the `flush` performed from
`about_to_wait` (NFR3):

1. Obtain the HWND from winit's `Window` through `rwh_06::HasWindowHandle`
   (FR6).
2. `ImmGetContext(hwnd)`.
3. If the returned context is null, return without doing anything (no-op).
4. `ImmSetCompositionWindow` with `dwStyle = CFS_POINT`,
   `ptCurrentPos = (x, y + height)`, `rcArea = (x, y, x + width, y + height)`.
5. `ImmSetCandidateWindow` with `dwStyle = CFS_EXCLUDE`,
   `ptCurrentPos = (x, y)`, `rcArea = (x, y, x + width, y + height)`.
6. `ImmReleaseContext(hwnd, himc)`.

| Call | `dwStyle` | `ptCurrentPos` | `rcArea` |
|------|-----------|----------------|----------|
| `ImmSetCompositionWindow` | `CFS_POINT` | `(x, y + height)` | `(x, y, x + width, y + height)` |
| `ImmSetCandidateWindow` | `CFS_EXCLUDE` | `(x, y)` | `(x, y, x + width, y + height)` |

`x` / `y` / `width` / `height` are physical pixels and are passed through
without conversion.

### Deferred detach state rules (FR3)

- A composition is alive from the observation of `Ime::Enabled` until
  `Ime::Disabled` is received.
- While alive, a `set_ime_allowed(false)` is held rather than delivered.
- After `Ime::Disabled` arrives, the next `flush` delivers it exactly once.
- `pending_allow` stays last-writer-wins: a focus-in arriving while a detach is
  pending overwrites it, so no detach is ever sent.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/ime/winit_bridge.rs`: the `BridgeWindow` trait and the existing
  deferred-flush machinery (`pending_allow` / `pending_cursor_area`, flush from
  `about_to_wait`) — the integration seam, kept as-is.

**External Dependencies:**
- `winit = "=0.31.0-beta.2"`: pinned, unmodified, not forked or patched (NFR1).
- `windows-sys` 0.59 (existing direct dependency in `src-tauri/Cargo.toml`, with
  `Win32_Foundation` / `Win32_System_Console`): add the `Win32_UI_Input_Ime`
  feature (FR6).
- `rwh_06::HasWindowHandle`: HWND acquisition from winit's `Window` (FR6).

### File Structure

```
src-tauri/
├── src/ime/winit_bridge.rs   # BridgeWindow trait, deferred flush,
│                             #   #[cfg(windows)] IMM32-direct implementation
└── Cargo.toml                # windows-sys: + Win32_UI_Input_Ime feature
```

New code lives behind the `gui` feature and `#[cfg(windows)]` (NFR2).

## Test Scenarios

### Unit Tests
- [ ] **TS1** (FR3): With a composition open (`Ime::Enabled` observed, no
      `Ime::Disabled` yet), `notify_focus(false)` + `flush` does not deliver
      `set_ime_allowed(false)`; after `Ime::Disabled` arrives, the next `flush`
      delivers it exactly once. Uses a mock `BridgeWindow`, following the
      existing pattern in `winit_bridge.rs` tests.
- [ ] **TS2** (FR3): A focus-in arriving while a detach is pending overwrites
      `pending_allow` (last-writer-wins) so no detach is ever sent.
- [ ] **TS3** (FR1, FR5): Existing deferred-flush / dedup / ordering tests remain
      green — the recording/flush contract is unchanged; only the Windows sink
      behind `BridgeWindow` changes.

### Integration Tests
- [ ] **TS4** (FR1, FR2, FR6, NFR2): Automated build gates — `cargo test --lib`
      (src-tauri, `CARGO_TARGET_DIR=src-tauri/target`), `cargo check
      --no-default-features`, and (for the Windows code path compiling at all)
      `make win-build` / cargo xwin cross-check.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Manual Verification (real device / host)
- [ ] **TS5** (FR1, FR2, FR3, NFR3): Manual, Windows + CorvusSKK real device —
      repeated conversion commits without freeze; Alt+Tab mid-conversion without
      freeze; candidate window tracks the caret. This is the gate for AC3, AC4
      and AC5, which cannot run on the Linux development host or in CI.
- [ ] **TS6** (FR5): Manual, Linux host — X11 / Wayland composition round-trip
      unchanged.

### Edge Cases
- [ ] `ImmGetContext` returns null: the direct path returns without further
      IMM32 calls (FR2).
- [ ] Focus-in during a pending detach: `pending_allow` is overwritten and the
      detach is cancelled (FR3).

## Out of Scope

Explicitly excluded by the task:

- Forking or patching winit, including `[patch.crates-io]`.
- Filing an upstream winit issue or PR.
- Deferring the `Drop`-path `set_ime_allowed(false)`. This is a **known residual
  hole**: when a bridge is swapped mid-composition (e.g. by a settings change),
  the `Drop` path can still run `set_ime_allowed(false)` during an active
  composition. It is recorded here as out of scope and is deliberately not
  fixed by this feature.
- The unrelated Wayland DnD issue.

Design step: skipped. This is a bug fix in IME plumbing (IMM32 call routing
inside the winit bridge). No visual or UI surface is added or changed — the only
user-visible outcome is the absence of a freeze and the existing candidate
window continuing to track the caret — so there is nothing for a design step to
specify.

## Success Criteria

- [ ] AC1: On Windows, `set_ime_cursor_area` calls IMM32 directly and does not
      pass through winit's `request_ime_update`. *(code review + unit tests with
      a mock `BridgeWindow`; automatable on any host)*
- [ ] AC2: While a composition is alive, `set_ime_allowed(false)` is not sent; it
      is sent by the flush after `Ime::Disabled` is received. *(unit tests against
      the bridge's pending-state logic; automatable on any host)*
- [ ] AC3: On a real Windows + CorvusSKK machine, repeated conversion commits do
      not freeze the app. *(**manual, real-device** — Windows + CorvusSKK; cannot
      run on this Linux host or in CI)*
- [ ] AC4: On the real device, Alt+Tab focus-out during an active conversion does
      not freeze the app. *(**manual, real-device** — Windows + CorvusSKK)*
- [ ] AC5: On the real device, the candidate window follows the cursor position.
      *(**manual, real-device** — Windows + CorvusSKK)*
- [ ] AC6: X11 / Wayland IME behavior is unchanged. *(existing `winit_bridge`
      unit-test suite stays green + manual spot check on the Linux host)*
- [ ] AC7: `cargo check --no-default-features` (CLI-only) passes. *(automated:
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
      src-tauri/Cargo.toml --no-default-features`)*

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. All functional and non-functional requirements are `resolved`. AC3-AC5 are
resolved requirements verified manually on a Windows + CorvusSKK real device by
the user; they are not TBDs.

## References

- Requirements document: `feature-docs/windows-imm32-ime-direct/REQUIREMENTS.md`
- winit-win32 0.31.0-beta.2: `window.rs:1025` (mutex held across `Imm*` calls),
  `event_loop.rs:121` (re-lock on wndproc re-entry),
  `event_loop.rs:1415/1428/1479` (`ime_capabilities.is_some()` gating of WM_IME_*)
- Hang dump analysis record: a project-local `tmp/` note (gitignored, therefore
  not a durable reference); its conclusions are transcribed in the Root Cause
  section above.
