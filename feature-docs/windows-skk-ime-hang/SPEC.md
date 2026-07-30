# Feature: windows-skk-ime-hang

## Overview

On Windows with the CorvusSKK IME, pressing `l` (the SKK mode-switch key)
during conversion mode makes eMterm stop responding (OS-level "Not
Responding"; the window must be killed). This feature removes the only
eMterm-controlled synchronous blocking path between the winit event dispatch
(wndproc) and the IMM32 API, so the identified deadlock mechanism cannot
occur, while keeping Linux (X11 / Wayland) behavior observably unchanged.

## Objectives

- Eliminate synchronous `request_ime_update` calls issued from inside winit
  window-event dispatch.
- Keep IME request ordering, dedup, and detach-on-drop semantics intact.
- Zero behavioral change on non-Windows targets (regression-proof via
  existing unit tests).

## Background Analysis (from create-spec investigation)

Verified against the pinned `winit 0.31.0-beta.2` sources and eMterm code:

1. eMterm calls `Window::request_ime_update` synchronously from inside
   winit event dispatch:
   - `WinitImeBridge::notify_focus` → `BridgeWindow::set_ime_allowed` —
     called from the `WindowEvent::Focused` arm in `window_host.rs`.
   - `WinitImeBridge::notify_cursor_rect` →
     `BridgeWindow::set_ime_cursor_area` — called from
     `App::notify_cursor_rect_if_changed` during `RedrawRequested`
     handling (WM_PAINT dispatch on Windows).
2. `winit-win32`'s `request_ime_update` executes its closure **inline**
   when invoked on the event-loop thread (`thread_executor
   .execute_in_thread`), calling `ImmAssociateContextEx` /
   `ImmSetCompositionWindow` / `ImmSetCandidateWindow` directly
   (`winit-win32/src/window.rs`).
3. Those IMM32 calls perform synchronous message exchanges with the IME.
   CorvusSKK is a TSF text service bridged through CUAS; when the IME side
   is itself blocked in a synchronous send to the eMterm window, an
   eMterm-side inline Imm* call creates a mutual-wait (AB-BA) deadlock.
   The wndproc never returns → Windows marks the window Not Responding —
   matching the reported symptom exactly.
4. SKK's `l` (conversion mode → ASCII mode) triggers composition
   teardown, candidate-window destruction, and context re-association in
   one burst, maximizing the window for the race in (3).
5. `winit-win32` itself consistently drops its `window_state` lock before
   dispatching events to the application; no winit-internal deadlock was
   found. The eMterm-side inline dispatch path in (1) is the only
   eMterm-fixable synchronous blocking path.

The build environment is Linux-only; the mechanism above cannot be
reproduced locally. Per the task's acceptance criteria a theoretically
correct fix is acceptable (see Assumptions).

## Technical Requirements

### Functional Requirements

- **FR1:** Deferred IME request queue. `WinitImeBridge` must not invoke
  `BridgeWindow::set_ime_allowed` / `set_ime_cursor_area` synchronously
  from any code path reachable from winit window-event dispatch
  (`window_event`). Instead, the bridge records the intent
  (pending allow-state change and/or pending cursor-area update) and a
  new flush entry point executes the recorded requests later, from
  `about_to_wait` (outside any wndproc message dispatch). Applies
  uniformly to all platforms — a single code path, no `#[cfg]` split in
  the request flow.
- **FR2:** Flush semantics. The flush entry point must preserve the
  current observable contract:
  - Ordering: an allow-state change and a cursor-area update recorded in
    the same turn are flushed in the order allow-state first, then
    cursor area (matching today's enable-then-seed sequence).
  - Coalescing: multiple updates of the same kind recorded before a flush
    collapse to the last value (at most one `set_ime_allowed` and one
    `set_ime_cursor_area` call per flush).
  - Dedup: the existing `last_cursor_area` dedup and the
    `notify_cursor_rect_if_changed` cell-level dedup keep working —
    identical rects still produce no call.
  - Idle cost: a flush with nothing recorded performs no calls and no
    allocation.
- **FR3:** Drop-path detach. `WinitImeBridge::drop` continues to detach
  from the IM server. Drop is not part of winit event dispatch, so it
  may keep calling `set_ime_allowed(false)` directly; any pending
  recorded requests are discarded on drop.
- **FR4:** Lifecycle-anomaly diagnostics. The bridge logs unusual IME
  lifecycle transitions at `warn` level (the minimum level persisted in
  release builds), each latched or rate-limited so a runaway IME cannot
  flood the log: at minimum `Ime::Enabled` while `ime_enabled` is
  already true, and `Ime::Disabled` while `ime_enabled` is already
  false. Normal transitions are not logged above `debug`.
- **FR5:** Non-Windows behavior preservation. The key-suppression
  predicate (`should_suppress_key`), the `ImeEvent` translation in
  `on_winit_ime`, and the platform gate selection are unchanged. All
  existing unit tests in `ime/winit_bridge.rs` and `app.rs` keep passing;
  tests may only be mechanically adapted where a call site now requires
  an explicit flush to observe the mock window (the asserted call
  sequences themselves stay the same).

### Non-Functional Requirements

- **NFR1 - Latency:** Recorded requests are flushed within the same
  event-loop turn (`about_to_wait` runs after each event batch), so IME
  candidate-window positioning gains no user-perceivable delay.
- **NFR2 - Dependencies:** No new crate dependencies.
- **NFR3 - Testability:** All new behavior is exercisable on a Linux
  host via the existing `BridgeWindow` mock; no test may require a
  Windows host or a live IME.

## Implementation Approach

### Affected code

```
src-tauri/src/ime/winit_bridge.rs   # request recording + flush (FR1-FR4)
src-tauri/src/ime/backend.rs        # ImeBackend trait: flush entry point
                                    # (default no-op so NullBackend is
                                    # unaffected)
src-tauri/src/app.rs                # App-level passthrough to the backend
                                    # flush
src-tauri/src/window_host.rs        # about_to_wait: invoke the flush
```

### Data Flow

```
wndproc (WM_SETFOCUS / WM_PAINT / WM_IME_*)
  → window_event handler (record intent only; no Imm* reachable)
  → ... dispatch returns, wndproc returns ...
about_to_wait
  → App::flush_ime_requests → WinitImeBridge::flush
  → BridgeWindow::set_ime_allowed / set_ime_cursor_area
  → winit request_ime_update (inline Imm* — now outside message dispatch)
```

The bridge constructor (`with_handle` / `init`) currently calls
`set_ime_allowed(true)` directly. Construction happens during
`can_create_surfaces` (also winit dispatch), so the initial enable is
recorded and flushed through the same mechanism.

### Dependencies

**Internal:** `ime::backend::ImeBackend` trait (all backends),
`window_host` event loop wiring.
**External:** none added. winit stays pinned at `=0.31.0-beta.2`.

## Test Scenarios

### Unit Tests

- [ ] TS-1: A `notify_focus(true)` recorded during "event dispatch" does
  not call the mock window until flush; after flush the mock sees exactly
  one `set_ime_allowed(true)`.
- [ ] TS-2: Multiple `notify_cursor_rect` calls before one flush coalesce
  to a single `set_ime_cursor_area` with the last rect; identical rects
  across flushes are deduped as today.
- [ ] TS-3: Allow-state + cursor-area recorded in the same turn flush in
  order (allow first).
- [ ] TS-4: Flush with nothing pending performs no mock calls.
- [ ] TS-5: Drop with pending requests discards them and still calls
  `set_ime_allowed(false)` exactly once.
- [ ] TS-6: Construction records the initial enable; first flush emits it
  (mock call sequence `[true]` after flush, `[]` before).
- [ ] TS-7: `should_suppress_key` truth table and all existing gate /
  translation tests pass unchanged (adapted only for explicit flush
  where they assert mock window calls).
- [ ] TS-8: Lifecycle-anomaly logging paths are exercised (double
  Enabled / double Disabled) and are latched (second occurrence does not
  log again) — assert via state, not log capture, if log capture is
  unavailable.

### Integration Tests

- [ ] Existing `cargo test --lib` suite passes.
- [ ] `cargo check` (default features) passes.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Not applicable (no E2E infrastructure; Windows hang is not
  reproducible in the build environment).

### Edge Cases

- [ ] Backend replacement mid-session (settings change → NullBackend):
  pending requests on the old bridge are discarded by drop (TS-5);
  NullBackend's flush is a no-op.
- [ ] Focus lost with a pending enable: last-writer-wins — the flush
  emits only the final allow state (coalescing, FR2).

## Success Criteria

- [ ] No code path from `window_event` dispatch reaches
  `request_ime_update` synchronously (grep-provable: `BridgeWindow`
  side-effect methods are invoked only from the flush entry point and
  `Drop`).
- [ ] All FR test scenarios pass on the Linux host.
- [ ] Existing IME test suite passes without semantic changes.
- [ ] `cargo check` and `cargo test --lib` green.

## Assumptions

Recorded per batch mode (no user available; Codex CLI unavailable — all
decisions taken by Claude):

- **A1:** The hang mechanism is not reproducible in the Linux build
  environment. The fix targets the most plausible mechanism identified by
  source analysis (synchronous IMM32 reentrancy during wndproc dispatch,
  Background Analysis 1-5). The task's acceptance criteria explicitly
  allow a theoretically-correct implementation without on-device
  verification.
- **A2:** The `l`-key specificity is explained by SKK mode-switch
  composition teardown (Background Analysis 4); no separate `l`-specific
  code path exists or is added in eMterm.
- **A3:** Deferring requests to `about_to_wait` is safe on all three
  targets: on Wayland/X11 the underlying winit calls are already
  asynchronous protocol requests, so only their issue point moves within
  the same loop turn.
- **A4:** The design step is skipped — no user-visible UI change.
- **A5:** Feature slug `windows-skk-ime-hang` derived from the Notion
  task title.

## References

- Task: https://www.notion.so/3ad3509ec8ee80a5b31fcdd8f9a87bb4
- REQUIREMENTS.md (Japanese requirements document, same directory)
- `src-tauri/src/ime/winit_bridge.rs` module docs (two-state gate
  rationale from the Linux SKK fix)
- winit-win32 0.31.0-beta.2: `src/window.rs` (`request_ime_update`),
  `src/ime.rs` (IMM32 calls), `src/event_loop.rs` (WM_IME_* handlers)
