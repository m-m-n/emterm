# Feature: Windows Esc Key via Win32 Input Mode

## Overview

On Windows, eMterm uses portable-pty 0.8 which always opens ConPTY with `PSEUDOCONSOLE_WIN32_INPUT_MODE`. Under this mode, the PTY input stream is interpreted as Win32 Input Mode VT key sequences. A bare `0x1b` byte (the current encoding for the Escape key) is not reliably delivered to terminal applications such as vim — users on Windows cannot exit vim's insert mode. This feature replaces the bare-byte encoding with a Win32 Input Mode VT key event sequence for Escape, mirroring the existing `encode_backspace_win32()` work, and audits other keys that may suffer the same fate.

## Objectives

- Emit Escape as a proper Win32 Input Mode VT key event sequence on Windows so vim and other TUI applications observe a real Escape key event.
- Preserve modifier state (Ctrl / Shift / Alt) through the ControlKeyState bitmask.
- Audit other keys that use a leading `0x1b` and may be misinterpreted in WIN32_INPUT_MODE; fix only those that demonstrably misbehave.
- Keep Linux / macOS encoding identical to the current behavior.

## User Stories

### US1: vim exits insert mode

As a Windows user, I want to press Esc inside vim's insert mode and have vim return to normal mode, so that I can edit files with the usual modal workflow.

**Acceptance Criteria:**
- [ ] Pressing Esc inside `vim` insert mode on a Windows build of eMterm returns vim to normal mode.
- [ ] `:q` and other ex-commands work after the Esc.

### US2: modifier-qualified Esc reaches applications

As a Windows user, I want Ctrl+Esc / Alt+Esc / Shift+Esc to be transmitted with the correct ControlKeyState bits, so that line editors and TUIs that bind those chords see them.

**Acceptance Criteria:**
- [ ] `encode(Key::Escape, mods)` on Windows produces a VT key event sequence with the correct `Cs` field for each modifier.
- [ ] Unit tests cover Ctrl, Alt, Shift, and combined modifiers.

### US3: no Linux/macOS regression

As a Linux/macOS user, I want Esc to continue sending the single byte `0x1b`, so that existing TUI applications keep working unchanged.

**Acceptance Criteria:**
- [ ] On non-Windows builds, `encode(Key::Escape, mods)` still returns `b"\x1b"` (plus the Alt prefix when applicable).
- [ ] All existing tests in `pty/input.rs` pass without modification.

## Technical Requirements

### Functional Requirements

- **FR1 — Windows Escape via Win32 Input Mode:** On Windows builds, `encode(Key::Escape, mods)` MUST return a Win32 Input Mode VT key event sequence (key-down + key-up pair) instead of a bare `0x1b`. The sequence layout is `ESC [ Vk;Sc;Uc;Kd;Cs;Rc _` with `Vk=27`, `Sc=1`, `Uc=27`, `Kd=1` for down then `Kd=0` for up, `Rc=1`.

- **FR2 — Modifier propagation:** The `Cs` field MUST encode `SHIFT_PRESSED` (0x10), `LEFT_CTRL_PRESSED` (0x08), and `LEFT_ALT_PRESSED` (0x02) as ORed bits whenever the corresponding modifier is set. This MUST be consistent with `encode_backspace_win32()` (`pty/input.rs:159-173`).

- **FR3 — Other-key audit:** During implementation, audit the WIN32_INPUT_MODE behavior of remaining keys that use a leading `0x1b` (notably `Alt+letter` chords which become `0x1b` + ASCII). For each key demonstrably broken on Windows, emit a Win32 Input Mode VT key event sequence in the same pattern. Findings (including keys that turned out to work) MUST be recorded in `IMPLEMENTATION.md`.

- **FR4 — Non-Windows parity:** On `#[cfg(not(windows))]`, `encode(Key::Escape, mods)` MUST behave identically to the current implementation (`out.push(0x1b)` after the Alt prefix). No code outside `#[cfg(windows)]` guards may change semantics.

- **FR5 — Unit tests:** Add Rust tests that assert (a) the exact Win32 Input Mode byte sequence for unmodified Esc on Windows, (b) the byte sequence for each single modifier and at least one combined modifier on Windows, and (c) that the existing non-Windows assertion (`b"\x1b"`) still holds, gated by `#[cfg(not(windows))]`.

### Non-Functional Requirements

- **NFR1 — Compatibility:** Works against portable-pty 0.8.1 with `PSEUDOCONSOLE_WIN32_INPUT_MODE` enabled. Does not depend on toggling that flag off.
- **NFR2 — Performance:** Per-keystroke cost equivalent to the existing `encode_backspace_win32()` (a `format!()` call plus a single `write_input`). No measurable latency regression in interactive typing.
- **NFR3 — Maintainability:** `encode_escape_win32()` doc comment MUST follow the same shape as `encode_backspace_win32()`'s, including a pointer to Microsoft Terminal spec #4999.
- **NFR4 — Portability:** Linux / macOS builds remain bit-identical for Esc encoding (covered by FR4 + FR5).

## Implementation Approach

### Architecture

The change is confined to `src-tauri/src/pty/input.rs`. The encoder function `encode(key, mods) -> Vec<u8>` already has the precedent of a Windows-only early return for Backspace; we extend the same pattern for Escape.

```
                  encode(key, mods)
                          │
        ┌─────────────────┴───────────────────┐
        ▼ #[cfg(windows)]                     ▼ #[cfg(not(windows))]
  matches Backspace?                    fall through
        │ yes → encode_backspace_win32                 │
        │ no                                           │
  matches Escape?                                      │
        │ yes → encode_escape_win32  (NEW)             │
        │ no  → fall through                           │
        └──────────────┬────────────────┬──────────────┘
                       ▼                ▼
                Apply Alt prefix → match key { ... } → Vec<u8>
```

### Data Flow

```
winit KeyboardInput
  → window_host translation → Key::Escape + Modifiers
  → encode(Key::Escape, mods)
  → (Windows) encode_escape_win32(mods)
  → tab.write_input(bytes)
  → PtySession writer thread → ConPTY (WIN32_INPUT_MODE)
  → child shell / TUI application observes a KEY_EVENT_RECORD with VK_ESCAPE
```

### API Design

No public API change. New private function:

```rust
#[cfg(windows)]
fn encode_escape_win32(mods: Modifiers) -> Vec<u8>;
```

Signature mirrors `encode_backspace_win32(mods)`.

### Database Schema

N/A — no persistent state.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/pty/input.rs` — only file changed.
- `src-tauri/src/window_host.rs` and `src-tauri/src/callbacks.rs` — unchanged callers.

**External Dependencies:**
- `portable-pty` 0.8.1 — ConPTY backend, WIN32_INPUT_MODE always on.
- No new crate dependencies introduced.

### File Structure

```
src-tauri/
  src/
    pty/
      input.rs              # encode_escape_win32 added; encode() gets the early-return shim
```

No new files. No module reorganization.

## Test Scenarios

### Unit Tests

Add to the existing `#[cfg(test)] mod tests` in `src-tauri/src/pty/input.rs`:

- [ ] `escape_emits_win32_input_mode_pair` (`#[cfg(windows)]`): `encode(Key::Escape, Modifiers::NONE)` equals `b"\x1b[27;1;27;1;0;1_\x1b[27;1;27;0;0;1_"`.
- [ ] `escape_win32_includes_ctrl_modifier` (`#[cfg(windows)]`): with `Modifiers { ctrl: true, .. NONE }`, `Cs` == 8.
- [ ] `escape_win32_includes_alt_modifier` (`#[cfg(windows)]`): with `Modifiers { alt: true, .. NONE }`, `Cs` == 2.
- [ ] `escape_win32_includes_shift_modifier` (`#[cfg(windows)]`): with `Modifiers { shift: true, .. NONE }`, `Cs` == 0x10 (16).
- [ ] `escape_win32_combined_modifiers` (`#[cfg(windows)]`): Ctrl+Shift Esc, `Cs` == 0x18 (24).
- [ ] `escape_emits_bare_1b_on_unix` (`#[cfg(not(windows))]`): `encode(Key::Escape, Modifiers::NONE)` equals `b"\x1b"`.
- [ ] The existing `enter_tab_backspace_escape` test continues to compile and pass on non-Windows (gated as it already is).

If FR3 surfaces additional keys requiring Win32 Input Mode treatment, add analogous tests for those.

### Integration Tests

No new integration test. The existing `cli_subcommands.rs` does not exercise the GUI key path.

### E2E Tests

**Existing E2E tests**: None (`test/README.md` confirms no E2E infrastructure).
**Run command**: N/A

- [ ] Manual verification on a Windows host:
  - [ ] Launch eMterm (GUI build), open vim, press `i`, then Esc. Confirm normal mode resumes.
  - [ ] Press `:q` and confirm vim exits.
  - [ ] Repeat in `less` (Esc should trigger no action or quit, depending on context).
- [ ] Manual verification of Linux Esc behavior (unchanged): vim in a Linux eMterm session still exits insert mode normally.

### Edge Cases

- [ ] Esc immediately after a printable character (e.g. typing `iabcEsc`): the Win32 Input Mode pair MUST be emitted in a single `write_input` call after the trailing `c`, so vim sees the Escape event distinctly from the typed text.
- [ ] Esc held down (auto-repeat): every repeat MUST emit its own down+up pair (no state needed; current encoder is stateless).
- [ ] Esc with all three modifiers (Ctrl+Shift+Alt+Esc): `Cs` == 0x1A (26); test or note in `IMPLEMENTATION.md`.

### Performance Tests

N/A — single-byte-or-thereabouts encode, no benchmark warranted.

## Security Considerations

N/A — purely local byte translation for outgoing PTY input. No untrusted input is parsed. No data persisted.

## Error Handling

`encode_escape_win32()` cannot fail (pure string formatting). The caller, `encode()`, already returns `Vec<u8>` and has no error channel. No new error codes required.

## Performance Optimization

### Performance Goals

- Same as today's Backspace encoding: a single `format!()` plus a single `write_input` call. No measurable interactive latency change.

### Optimization Strategies

None planned. The hot path is already tight.

### Caching Strategy

None.

## Success Criteria

- [ ] FR1 / FR2 / FR3 / FR4 / FR5 are implemented and verified by the tests in "Test Scenarios > Unit Tests".
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes (Linux host).
- [ ] Windows cross-build (`make win-build`) compiles cleanly.
- [ ] Manual verification on a Windows host (see "Test Scenarios > E2E Tests") confirms vim Esc works.
- [ ] `IMPLEMENTATION.md` records the FR3 audit outcome.

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。`/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- (none — FR3 is a planned-during-implementation audit, not a tbd)

## Implementation Phases

### Phase 1: Esc encoder + unit tests

**Goals:** Land FR1, FR2, FR4, FR5 with unit-test coverage.
**Deliverables:**
- `encode_escape_win32()` in `src-tauri/src/pty/input.rs`
- `#[cfg(windows)]` early-return shim in `encode()`
- Unit tests for the Windows path and a re-asserted non-Windows path

### Phase 2: Other-key audit (FR3)

**Goals:** Identify any other keys that misbehave under WIN32_INPUT_MODE.
**Deliverables:**
- Audit notes in `IMPLEMENTATION.md` covering: Alt+letter chords, arrow / nav keys, F-keys (the latter two are expected to work; the audit is for completeness).
- If broken keys are found, additional `encode_<key>_win32()` helpers + tests.

### Phase 3: Manual verification on Windows

**Goals:** Confirm Esc actually works in vim and adjacent TUIs.
**Deliverables:**
- User-run manual verification per the acceptance criteria.
- Any follow-up bug reports filed if a corner case is still broken.

## References

- `tmp/issues-windows-mux-2026-06-22.md` — origin report
- `src-tauri/src/pty/input.rs:141-173` — existing `encode_backspace_win32()` and its doc comment
- Microsoft Terminal spec #4999 — "Improved keyboard handling in conpty"
- `doc/tasks/windows-esc-win32-input-mode/要件定義書.md` — Japanese requirements document
