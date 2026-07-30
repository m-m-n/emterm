# Implementation Plan: windows-skk-ime-hang

## Overview

Remove the only eMterm-controlled synchronous blocking path between winit
window-event dispatch (wndproc) and the IMM32 API by deferring IME requests
into a bridge-internal queue flushed from `about_to_wait`. Single-task
feature; this document pins the one cross-cutting contract (the backend
trait's flush entry point) so reviewers and the verify phase share it.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate, `gui` feature)
- **Key libraries**: winit `=0.31.0-beta.2` (pinned; unchanged), no new
  dependencies (SPEC NFR2)

## Layer Structure

Unchanged. The IME seam stays: `window_host` (winit event loop) → `App`
(neutral routing) → `ime::backend::ImeBackend` (trait) →
`ime::winit_bridge::WinitImeBridge` (platform backend) →
`BridgeWindow` (window side effects). The fix moves WHEN the last arrow is
executed (flush time instead of event-dispatch time), not the layering.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `ImeBackend` flush entry point | Execute IME side-effect requests recorded since the previous flush | Pre: may be called at any frequency, including with nothing recorded. Post: all recorded requests executed in order (allow-state before cursor-area), coalesced to at most one call per kind; no calls and no allocation when nothing is recorded. Default trait implementation is a no-op (NullBackend unaffected). | task0001 |

## Conventions

- Follow the existing `winit_bridge.rs` documentation style: module-level
  rationale comments cite the winit source files they depend on.
- Diagnostic logging: `warn` level minimum (release builds persist only
  warn+, per project logging rules), latched/rate-limited per anomaly kind.

## Cross-task Design Decisions

### Defer IME requests out of event dispatch (SPEC FR1)

All `BridgeWindow` side-effect invocations (`set_ime_allowed`,
`set_ime_cursor_area`) move behind a recorded-intent queue inside
`WinitImeBridge`, executed by the flush entry point that `window_host`
calls from `about_to_wait`. Rationale: on Windows, winit executes
`request_ime_update` inline on the event-loop thread, reaching IMM32 calls
that exchange synchronous messages with the IME; issuing them while inside
wndproc dispatch is the identified deadlock mechanism (SPEC Background
Analysis). `about_to_wait` runs between message dispatches, outside any
wndproc frame. Uniform on all platforms — no `#[cfg]` split in the request
flow. Exception: `Drop` keeps calling the detach directly (not part of
event dispatch; SPEC FR3).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Root cause differs from the identified mechanism (no Windows repro available) | Medium | Hang persists | FR4 anomaly diagnostics give field reports a confirmation surface; fix is defensive and harmless if the hypothesis is wrong |
| Deferral changes Linux IME timing observably | Low | UX regression on Linux | Flush within the same loop turn (`about_to_wait` follows every event batch); existing unit tests pinned to call sequences (SPEC FR5) |
| Initial enable recorded at construction never flushed (window created but loop not yet running) | Low | IME never attaches | Flush is called every `about_to_wait` turn; first turn follows window creation immediately |

## Open Questions

- [ ] On-device confirmation on Windows + CorvusSKK is deferred to field
  verification (build environment cannot reproduce; task acceptance allows
  theoretically-correct implementation).
