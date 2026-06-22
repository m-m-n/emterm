# Verification Document: Windows Esc Key via Win32 Input Mode

## Overview

**Feature**: windows-esc-win32-input-mode
**SPEC.md**: `doc/tasks/windows-esc-win32-input-mode/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/windows-esc-win32-input-mode/IMPLEMENTATION.md`

This document defines how the feature is verified. Build, test, and code-quality verifications are automated. End-to-end behavior (vim Esc, Alt+letter chord, audit confirmations) is verified manually on a Windows host because this project has no E2E framework.

## Build Verification

- **Linux quick check command**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  Expected: exit code 0, no errors, no new warnings introduced by this feature.
- **Linux quick check (CLI-only feature)**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  Expected: exit code 0. Confirms the change does not accidentally break the CLI-only build gate (input.rs is GUI-gated, so this is a sanity check).
- **Windows cross-build command**:
  `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`
  Expected: exit code 0, produces `src-tauri/target-win/x86_64-pc-windows-msvc/release/emterm.exe`.
- **Release Linux build (optional, only if needed by sdd.6-verify)**:
  `make build`
  Expected: exit code 0.

## Test Verification

- **Command**:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  Expected: all tests pass, including the new Esc Win32 Input Mode tests on Windows-targeted runs and the non-Windows `b"\x1b"` parity test on Linux runs.
- **Coverage target**: the touched code path in `src-tauri/src/pty/input.rs` is short and pure. Aim for 100% line coverage of `encode_escape_win32` plus the Windows shim branch (achieved by TS-1 through TS-5). TS-6 covers the non-Windows preserved branch.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `encode(Key::Escape, Modifiers::NONE)` on Windows build | Returns `b"\x1b[27;1;27;1;0;1_\x1b[27;1;27;0;0;1_"` | Unit (`#[cfg(windows)]`) |
| TS-2 | `encode(Key::Escape, Modifiers { ctrl: true, .. NONE })` on Windows build | Returns the sequence with `Cs = 8` (LEFT_CTRL_PRESSED) in both records | Unit (`#[cfg(windows)]`) |
| TS-3 | `encode(Key::Escape, Modifiers { alt: true, .. NONE })` on Windows build | Returns the sequence with `Cs = 2` (LEFT_ALT_PRESSED) | Unit (`#[cfg(windows)]`) |
| TS-4 | `encode(Key::Escape, Modifiers { shift: true, .. NONE })` on Windows build | Returns the sequence with `Cs = 16` (SHIFT_PRESSED) | Unit (`#[cfg(windows)]`) |
| TS-5 | `encode(Key::Escape, Modifiers { ctrl: true, shift: true, .. NONE })` on Windows build | Returns the sequence with `Cs = 24` (LEFT_CTRL | SHIFT) | Unit (`#[cfg(windows)]`) |
| TS-6 | `encode(Key::Escape, Modifiers::NONE)` on non-Windows build | Returns `b"\x1b"` (existing `enter_tab_backspace_escape` assertion, re-gated) | Unit (`#[cfg(not(windows))]`) |
| TS-7 | vim insert-mode exit on Windows host | Pressing Esc in `vim` insert mode returns to normal mode; `:q` exits | Manual (Windows) |
| TS-8 | less / TUI Esc behavior on Windows host | Esc in `less` and one additional TUI behaves as on Linux | Manual (Windows) |
| TS-9 | Alt+letter chord on Windows (FR3 audit) | Type `Alt+b` in pwsh + PSReadLine; verdict (works / broken) is recorded in IMPLEMENTATION.md "Audit Notes" | Manual (Windows) |
| TS-10 | Arrow / nav / F-keys on Windows (FR3 audit) | Cursor / Home / End / PageUp / PageDown / Delete / Insert / F1-F12 behave correctly in pwsh and vim; verdict recorded | Manual (Windows) |
| TS-11 | Audit documentation completeness (FR3) | IMPLEMENTATION.md "Audit Notes" subsection exists and lists every encode() candidate with a verdict | Document review |
| TS-12 | Doc-comment parity review (NFR3) | The new `encode_escape_win32` doc comment mirrors the structure of `encode_backspace_win32`'s doc comment (rationale paragraph, sequence layout, Microsoft Terminal spec #4999 reference). Reviewer signs off in VERIFICATION_RESULT.md. | Document review |

## Code Quality Verification

- **Format**:
  `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
  Expected: exit code 0, no formatting diff in `src-tauri/src/pty/input.rs`.
- **Static analysis** (clippy is not part of this project's CI per CLAUDE.md, but a local sanity sweep is recommended):
  `CARGO_TARGET_DIR=src-tauri/target cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` (optional)
  Expected: no new clippy warnings on the modified file. Skip if clippy is not configured.
- **No dead code**: the existing `#[allow(dead_code)]` on `input.rs` covers helpers that may be unused on a given target; do not remove or add new `allow` attributes.

## File Structure Verification

### Files to Create

- (none)

### Files to Modify

- `src-tauri/src/pty/input.rs`
  - Add `encode_escape_win32(mods: Modifiers) -> Vec<u8>` (`#[cfg(windows)]`).
  - Add Windows early-return shim for `Key::Escape` inside `encode()`.
  - Add unit tests for TS-1 through TS-5 (Windows-gated) and TS-6 (non-Windows-gated).
  - Tighten the existing `enter_tab_backspace_escape` test by guarding its Escape assertion to non-Windows targets.

### Files Updated (documentation)

- `doc/tasks/windows-esc-win32-input-mode/IMPLEMENTATION.md`
  - Append "Audit Notes (FR3)" subsection with per-candidate verdict (Phase 2 only).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR5 implemented and verified | Tests TS-1 … TS-6 pass; manual TS-7 confirms vim Esc on Windows. |
| SC-2 | `cargo test --lib` passes on Linux host | Re-run the Test Verification command. |
| SC-3 | Windows cross-build compiles cleanly | Re-run the Windows cross-build command. |
| SC-4 | Manual verification on Windows confirms vim Esc | TS-7 result captured by sdd.6-verify. |
| SC-5 | Audit outcome recorded in IMPLEMENTATION.md | TS-11 document review. |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (Windows Esc via Win32 Input Mode) | Phase 1 | TS-1 |
| FR2 (Modifier propagation through Cs) | Phase 1 | TS-2, TS-3, TS-4, TS-5 |
| FR3 (Audit other keys) | Phase 2 | TS-9, TS-10, TS-11 |
| FR4 (Non-Windows parity) | Phase 1 | TS-6 |
| FR5 (Unit tests added) | Phase 1 | TS-1 … TS-6 are themselves the FR5 deliverable |
| NFR1 (Compatible with portable-pty 0.8 WIN32_INPUT_MODE) | Phase 1 | Manual TS-7 + Windows cross-build |
| NFR2 (No latency regression) | Phase 1 | Heuristic — same encode shape as `encode_backspace_win32` |
| NFR3 (Doc-comment parity) | Phase 1 | TS-12 |
| NFR4 (Linux/macOS bit-identical Esc) | Phase 1 | TS-6 + non-Windows `cargo test` |

## E2E Testing

This project has no E2E framework (`test/README.md` confirms). Section omitted.

## Manual Testing (E2E Not Possible)

Performed by the user on a Windows host (or VM / remote) during sdd.6-verify.

- [ ] **TS-7 — vim Esc**: Launch eMterm (Windows GUI build). Run `vim foo.txt`. Press `i` to enter insert mode. Type a few characters. Press Esc. Confirm the status bar's `-- INSERT --` disappears. Press `:q!` and confirm vim exits cleanly.
- [ ] **TS-8 — TUI Esc adjacency**: Inside pwsh on the same Windows build, run `less <some-file>`. Press Esc-q or just q to confirm normal less keybindings still work. Run a second TUI of choice (e.g. `nano`, `nvim`) and confirm Esc behaves.
- [ ] **TS-9 — Alt+letter chord**: In pwsh with PSReadLine, position cursor over a word and press `Alt+b` (back-word) and `Alt+f` (forward-word). Record whether PSReadLine moves the cursor correctly, or whether it instead emits a literal `b` / `f` after the Esc. Append the verdict to IMPLEMENTATION.md "Audit Notes".
- [ ] **TS-10 — Navigation / F-keys**: In pwsh and vim, exercise Up/Down/Left/Right, Home/End, PageUp/PageDown, Delete/Insert, F1–F12. Record any key whose behavior diverges from a known-working terminal (e.g. Windows Terminal) in the audit notes.
- [ ] **TS-11 — Audit documentation**: Confirm IMPLEMENTATION.md contains an "Audit Notes (FR3)" subsection listing every encode() candidate (Esc, Backspace, Alt+letter, arrows, Home/End, PageUp/Down, Delete/Insert, F1-F12, Shift+Tab) with a verdict.
- [ ] **TS-12 — Doc-comment parity**: Read `src-tauri/src/pty/input.rs` and confirm the doc comment on `encode_escape_win32` mirrors `encode_backspace_win32`'s shape (rationale paragraph explaining WIN32_INPUT_MODE, the byte sequence layout `ESC [ Vk;Sc;Uc;Kd;Cs;Rc _`, and the Microsoft Terminal spec #4999 reference).

## Performance Verification

Not applicable. The change replaces a 1-byte push with a 35-byte `format!()`-driven push; this is the same shape as `encode_backspace_win32` and well within interactive-typing latency budgets. No benchmark.

## Security Verification

Not applicable. Purely local outgoing-PTY byte construction; no untrusted input parsed.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 (Linux check, CLI-only check, Windows cross-build) | 3 | 0 | 0 |
| Test | 1 command (`cargo test --lib`) | 1 | 0 | 0 |
| Test scenarios | 12 (TS-1 … TS-12) | 6 (TS-1 … TS-6) | 0 | 6 (TS-7 … TS-12) |
| Code quality | 1 (rustfmt --check) | 1 | 0 | 0 |
| Document review | 2 (Audit Notes presence, doc-comment parity) | 0 | 0 | 2 |

## Implementation Results (Phase 1)

Captured during `sdd.4-implement` execution on 2026-06-22 (Linux host).

### Build Verification

- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` — exit 0, no new warnings.
- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` — exit 0.
- Windows cross-build (`cargo xwin`) — **not executed in this session** per the multi-minute cost and the user's "don't run release builds unsolicited" rule. Linux `cargo check` covers the non-Windows branch; the `#[cfg(windows)]` branch in `pty/input.rs` is not compiled by Linux `cargo check`, so a Windows cross-build (or actual Windows host build) MUST be run before shipping to confirm the new `encode_escape_win32` helper and shim compile. The helper is structured identically to the already-shipped `encode_backspace_win32` (same crate features, same `format!` shape, same `Modifiers` API), so a compile failure is unlikely but not yet confirmed.

### Test Verification

- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` — 1903 passed, 0 failed, 3 ignored.
  - `--test-threads=1` used per project MEMORY (`tabs.rs` replay tests are non-deterministic under parallel execution; not related to this feature).
  - Linux-side test scenario covered: TS-6 (`enter_tab_backspace_escape` still asserts `b"\x1b"` after the new `#[cfg(not(windows))]` guard on the Escape line).
- Windows-gated scenarios TS-1 … TS-5: tests added but **not executed** in this session (Linux host cannot run `#[cfg(windows)]` test bodies). Verification deferred to Windows cross-build / Windows host test run.

### Code Quality Verification

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check src-tauri/src/pty/input.rs` — exit 0, no diff.

### File Structure Verification

- Files modified:
  - `src-tauri/src/pty/input.rs`
    - Added `#[cfg(windows)]` early-return shim for `Key::Escape` in `encode()` (mirrors the Backspace shim).
    - Added `encode_escape_win32(mods: Modifiers) -> Vec<u8>` helper under `#[cfg(windows)]` with doc comment paralleling `encode_backspace_win32` (WIN32_INPUT_MODE rationale, sequence layout, Microsoft Terminal spec #4999 reference).
    - Added `#[cfg(not(windows))]` guard on the existing Escape assertion inside the `enter_tab_backspace_escape` test (TS-6).
    - Added `#[cfg(windows)]` unit tests `escape_emits_win32_input_mode_pair`, `escape_win32_includes_ctrl_modifier`, `escape_win32_includes_alt_modifier`, `escape_win32_includes_shift_modifier`, `escape_win32_combined_modifiers` (TS-1 … TS-5).

### Phase 2 / Manual Verification — Deferred

Phase 2 (FR3 audit) and manual scenarios TS-7 … TS-12 require a Windows host and are not actionable from this Linux session. tasks.yaml Phase 2 tasks remain `pending` and are tracked for `sdd.6-verify` (Windows host execution).

### Known Limitations

- Windows cross-build (`cargo xwin build --release --target x86_64-pc-windows-msvc`) has not been run in this session. The new `#[cfg(windows)]` code path compiled-checks only on a Windows / Windows-cross host. Run before shipping.
