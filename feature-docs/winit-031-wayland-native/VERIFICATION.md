# Verification Document: winit 0.31 Migration and Wayland Native Startup

## Overview
**Feature**: winit-031-wayland-native / **SPEC.md**:
`feature-docs/winit-031-wayland-native/SPEC.md` / **IMPLEMENTATION.md**:
`feature-docs/winit-031-wayland-native/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (Windows): `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: new pure logic (backend decision, drop-path mapping,
  synthetic gate) fully unit-covered

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | GUI build check on winit 0.31.0-beta.2 | exit 0 | Integration (build) |
| TS-2 | Rust lib test suite | all pass | Unit |
| TS-3 | CLI-only feature check (`--no-default-features`) | exit 0 | Integration (build) |
| TS-4 | Windows cross-check (`cargo xwin check`) | exit 0 | Integration (build) |
| TS-5 | Backend decision: `wayland` → ForceWayland; `x11`+DISPLAY → ForceX11; `x11` without DISPLAY / empty / unknown → Auto | decisions as specified | Unit |
| TS-6 | Synthetic key press gate: synthetic press/release dropped; real events unchanged | no PTY bytes / dispatch for synthetic | Unit |
| TS-7 | Drop-path mapping: non-empty list → upload entry point in order; empty list → no-op | as specified | Unit |

## Code Quality Verification
- Format: (none — project does not enforce rustfmt)
- Static analysis: covered by cargo check gates above

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Wayland native by default; stray-q eliminated | M-1, M-2 |
| SC-2 | D&D works on native Wayland | M-3 |
| SC-3 | X11 opt-in works with synthetic guard | M-4, TS-6 |
| SC-4 | All build variants + tests green | TS-1–TS-4 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-4, M-1 |
| FR2 | task0001 | TS-5, M-4 |
| FR3 | task0001 | TS-7, M-3 |
| FR4 | task0002 | TS-6 |
| NFR1 | task0001 | TS-4 |
| NFR2 | task0001 | TS-3 |
| NFR3 | task0001 | M-5 |

## E2E Testing
(no project E2E framework — omitted)

## Manual Testing (E2E Not Possible)
- [ ] M-1: On a Wayland session, start eMterm with no `EMTERM_BACKEND`;
      verify Wayland native startup (e.g. absent from `xlsclients` /
      backend log line) and that keyboard input, IME, rendering, and child
      WebView windows (Markdown viewer / settings) work
- [ ] M-2: With Claude Code busy inside eMterm, start an Xwayland Qt app
      (`QT_QPA_PLATFORM=xcb strawberry`), close it with Ctrl+Q several
      times — no `q` appears in the terminal input
- [ ] M-3: Drag a file from a file manager onto the terminal on native
      Wayland — the SFTP upload entry point receives it (single and
      multiple files)
- [ ] M-4: `EMTERM_BACKEND=x11 emterm` starts on the X11 backend; basic
      input works
- [ ] M-5: `src-tauri/Cargo.toml` pins winit `=0.31.0-beta.2` with a
      beta / bump-at-stable comment

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | TS-1, TS-3, TS-4 | 3 | 0 | 0 |
| Unit | TS-2, TS-5, TS-6, TS-7 | 4 | 0 | 0 |
| Manual | M-1–M-5 | 0 | 0 | 5 |
