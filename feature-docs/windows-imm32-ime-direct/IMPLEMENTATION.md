# Implementation Plan: windows-imm32-ime-direct

## Overview

Fixes the Windows + CorvusSKK "not responding" freeze by routing the Windows
`set_ime_cursor_area` delivery around winit (direct IMM32 calls from the
Windows window sink) and deferring the IME detach until the composition has
ended. This is a single-task feature (task0001); per-task design lives in
`tasks/task0001.md` — this document records only the feature-wide decisions
the review and verify phases rely on.

## Technology Stack

- **Language**: Rust (`src-tauri` crate). All new code sits behind the `gui`
  feature, and the Windows-specific part additionally behind
  `#[cfg(windows)]` (NFR2).
- **winit `=0.31.0-beta.2`** — pinned, unmodified, not forked or patched, no
  `[patch.crates-io]` (NFR1). Remains the sink for the allow-state on every
  target and for the cursor area on non-Windows targets. License: Apache-2.0
  (existing dependency, unchanged).
- **windows-sys 0.59** — existing direct dependency
  (`[target.'cfg(windows)'.dependencies]`); gains the `Win32_UI_Input_Ime`
  feature. Feature flag only — no new crate, no version change. License:
  MIT OR Apache-2.0 — compatible with the project's MIT license.
- **raw-window-handle 0.6** (`rwh_06`) — existing dependency (also
  re-exported by winit); used to obtain the HWND from the winit window
  (FR6). License: MIT OR Apache-2.0 OR Zlib — compatible.

**License check**: no new dependency crates are introduced. The only
manifest change is a feature flag on an existing MIT OR Apache-2.0
dependency; no conflict with `project.license: MIT`.

## Layer Structure

Unchanged. The integration seam is the existing `BridgeWindow` abstraction
in `src-tauri/src/ime/winit_bridge.rs`: platform-neutral bridge state
(recording, dedup, deferred flush) above, platform window sink below. The
app / window-host wiring — recording during event dispatch, flush from
`about_to_wait` — is kept as-is; no module outside `ime/winit_bridge.rs`
changes, and no new module is added.

## Shared Components

Single-task feature — no cross-task component contracts.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| —         | —              | —                            | —             |

## Conventions

- Windows-specific code compiles only under `gui` + `#[cfg(windows)]`; the
  CLI-only build (`--no-default-features`) must stay winit-free (NFR2).
- IMM32 error posture: an unavailable / non-Win32 window handle or a null
  input-method context results in a silent no-op — no logging, no retry —
  matching FR2's defined error case.
- Testability rule: every decision (when to hold or deliver the detach)
  lives in platform-neutral, host-testable bridge state; the
  `#[cfg(windows)]` sink stays a thin, decision-free executor of the FR2
  call recipe.

## Cross-task Design Decisions

None beyond the conventions above (single task). SPEC-settled boundaries
that must not be re-litigated during implementation or review:

- Enable (`set_ime_allowed(true)`) stays winit-routed on ALL targets (FR4);
  only the Windows cursor-area delivery bypasses winit (FR1), and
  non-Windows targets keep the current winit-routed delivery (FR5).
- The `Drop`-path detach is NOT deferred — a known residual hole (bridge
  swapped mid-composition), explicitly out of scope.
- Root cause is settled (winit-win32 holds its window-state mutex across
  IMM32 calls while TSF re-enters wndproc on the same thread); no
  re-diagnosis in scope.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The `#[cfg(windows)]` code is compile-checked but never executed on the dev host | High (structural) | High if the IMM32 recipe is wrong on-device | Keep the Windows sink decision-free; windows cross-target check is a mandatory automated gate; real-device manual gate (VERIFICATION TS5) before acceptance |
| Deferred detach changes delivery timing on X11 / Wayland | Low | Medium | Existing `winit_bridge` unit suite must pass unchanged (TS3); Linux manual spot check (TS6) |
| A composition whose Disabled event never arrives keeps a detach held indefinitely | Low | Low | SPEC-settled accepted failure mode: the next focus-in overwrites the held detach (last-writer-wins); no timeout machinery |
| `Drop`-path detach during a live composition (bridge swap) | Low | Medium | Out of scope by SPEC; documented residual hole |

## Open Questions

- None. All requirements are resolved; no TBDs.
