# Verification Document: Linux PRIMARY Selection Support

## Overview
**Feature**: linux-primary-selection
**SPEC.md**: `doc/tasks/linux-primary-selection/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/linux-primary-selection/IMPLEMENTATION.md`

## Build Verification

### Frontend (TypeScript)
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Expected: exit code 0, no type errors
- Scope: all TS sources including new `src/platform.ts`, modified `ClipboardBridge`, `ClipboardManager`, `SelectionController`, `terminal-app/index.ts`, `terminal-app/ui-handler.ts`, `settings/sections/terminal-behavior-section.ts`, and new `settings/effective-settings.ts`

### Backend (Rust)
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --manifest-path src-tauri/Cargo.toml"`
- Expected: exit code 0 on both Linux and Windows targets
- Additional check: `cargo tree --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` must NOT list arboard

## Test Verification

### Frontend tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: minimum 80% for new modules, target 90% for `platform.ts` and `effective-settings.ts`

### Backend tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- Coverage target: minimum 80% for `src-tauri/src/commands/clipboard_primary.rs`

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `isLinux()` before `initPlatform()` is awaited | Returns `false` (defensive) | Unit (TS) |
| TS-2 | `isLinux()` after init with platform = `"linux"` | Returns `true` | Unit (TS) |
| TS-3 | `isLinux()` after init with platform = `"windows"` | Returns `false` | Unit (TS) |
| TS-4 | `ClipboardBridge.writePrimary(text)` on non-Linux | Returns `false` without invoking Tauri command | Unit (TS) |
| TS-5 | `ClipboardBridge.writePrimary(text)` on Linux success | Returns `true`, invokes `clipboard_write_primary` exactly once with `{ text }` | Unit (TS) |
| TS-6 | `ClipboardBridge.writePrimary(text)` on Linux error | Catches, logs via `console.warn`, returns `false` | Unit (TS) |
| TS-7 | `ClipboardBridge.readPrimary()` on non-Linux | Returns `""` without invoking Tauri command | Unit (TS) |
| TS-8 | `ClipboardBridge.readPrimary()` on Linux success | Returns the invoked result string | Unit (TS) |
| TS-9 | `ClipboardBridge.readPrimary()` on Linux error | Catches, logs, returns `""` | Unit (TS) |
| TS-10 | `effectiveCopyOnSelect` on Linux with raw `true` | Returns `false` | Unit (TS) |
| TS-11 | `effectiveCopyOnSelect` on Linux with raw `false` | Returns `false` | Unit (TS) |
| TS-12 | `effectiveCopyOnSelect` on Windows with raw `true` | Returns `true` | Unit (TS) |
| TS-13 | `effectiveCopyOnSelect` on Windows with raw `undefined` | Returns `false` (preserves existing default) | Unit (TS) |
| TS-14 | `effectiveMiddleClickPaste` on Linux with raw `false` | Returns `true` | Unit (TS) |
| TS-15 | `effectiveMiddleClickPaste` on Windows with raw `false` | Returns `false` | Unit (TS) |
| TS-16 | `effectiveMiddleClickPaste` on Windows with raw `undefined` | Returns `true` (preserves existing default) | Unit (TS) |
| TS-17 | `SelectionController.onMouseUp` on Linux with non-empty selection | `writePrimary` called once with the selected text; `copy()` NOT called (Linux effective copy_on_select = false) | Integration (TS) |
| TS-18 | `SelectionController.onMouseUp` on Linux with empty selection | `writePrimary` not called | Integration (TS) |
| TS-19 | `SelectionController.onMouseUp` on Windows with `copy_on_select = true` | `copy()` called; `writePrimary` not called | Integration (TS) |
| TS-20 | `handleMiddleClickPaste` on Linux with PRIMARY non-empty | Pastes PRIMARY text, `read()` not called | Integration (TS) |
| TS-21 | `handleMiddleClickPaste` on Linux with PRIMARY empty, CLIPBOARD non-empty | Falls back to `read()`, pastes CLIPBOARD text | Integration (TS) |
| TS-22 | `handleMiddleClickPaste` on Linux with both empty | No PTY write | Integration (TS) |
| TS-23 | `handleMiddleClickPaste` on Windows | Only `read()` called; no `readPrimary()` call | Integration (TS) |
| TS-24 | `handleMiddleClickPaste` with multi-line PRIMARY content | Existing multi-line confirmation dialog is shown | Integration (TS) |
| TS-25 | `clipboard_write_primary` on non-Linux target | Returns `Ok(())` without side effects | Unit (Rust) |
| TS-26 | `clipboard_read_primary` on non-Linux target | Returns `Ok("")` without side effects | Unit (Rust) |
| TS-27 | `clipboard_write_primary` on Linux with arboard init failure | Returns `Err(message)` (test with injected failure if feasible; otherwise document as manual test) | Unit (Rust, Linux) |
| TS-28 | `clipboard_read_primary` on Linux with no content available | Returns `Ok("")` (PRIMARY empty case) | Unit (Rust, Linux) |

## Code Quality Verification
- Format (TypeScript): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"` (project uses tsc as the format gate)
- Format (Rust): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml --check"`
- Static analysis (Rust): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"`

## File Structure Verification

### Files to Create
- `src-tauri/src/commands/clipboard_primary.rs` — new Tauri commands for PRIMARY read/write
- `src/platform.ts` — cached platform detection helper
- `src/settings/effective-settings.ts` — effective-value accessors for `copy_on_select` and `middle_click_paste`
- `e2e-tests/specs/linux-primary-selection.e2e.js` — Linux-only E2E spec

### Files to Modify
- `src-tauri/Cargo.toml` — add arboard target-gated dependency
- `src-tauri/src/commands/mod.rs` — declare `clipboard_primary` submodule
- `src-tauri/src/app.rs` — register `clipboard_write_primary` and `clipboard_read_primary`
- `src/main.ts` — await `initPlatform()` during startup
- `src/selection-v2/ClipboardBridge.ts` — add `writePrimary` / `readPrimary`
- `src/selection-v2/SelectionController.ts` — write PRIMARY on mouseup (Linux)
- `src/clipboard/manager.ts` — mirror `writePrimary` / `readPrimary`
- `src/terminal-app/index.ts` — use `effectiveMiddleClickPaste` to gate middle-click listener
- `src/terminal-app/ui-handler.ts` — resolve paste text via PRIMARY → CLIPBOARD priority
- `src/settings/sections/terminal-behavior-section.ts` — skip rendering the two rows on Linux
- `README.md` — describe the Linux clipboard behavior change
- `CHANGELOG.md` (or release notes source) — note the runtime override and UI change

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Linux drag-selection writes to PRIMARY | E2E: select text, read PRIMARY via helper, assert match |
| SC-2 | Linux middle-click pastes PRIMARY content | E2E: select text, middle-click, assert PTY received the text |
| SC-3 | `Ctrl+C` content survives subsequent selections | E2E: Ctrl+C → select other text → Ctrl+V → assert original text |
| SC-4 | Linux settings panel omits the two rows | E2E: render settings panel, assert `copy-on-select` and `middle-click-paste` rows are absent from DOM |
| SC-5 | `settings.json` values for the two keys are ignored on Linux | Integration: seed `settings.json` with `copy_on_select: true`, run selection, assert CLIPBOARD unchanged |
| SC-6 | Windows settings panel shows both rows unchanged | Manual on Windows |
| SC-7 | X11 environment works | Manual on X11 host |
| SC-8 | Wayland environment works | Manual on Wayland host |
| SC-9 | PRIMARY failures produce warn-only logs, no crash | Integration: mock bridge to throw, assert `console.warn` called, app still responsive |
| SC-10 | PRIMARY interop with another Linux terminal | Manual cross-app test |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 PRIMARY write Tauri command | Phase 1 | TS-25, TS-27, SC-1 |
| FR2 PRIMARY read Tauri command | Phase 1 | TS-26, TS-28, SC-2 |
| FR3 Auto-write on mouseup | Phase 3 | TS-17, TS-18, TS-19, SC-1, SC-3 |
| FR4 Middle-click PRIMARY-first | Phase 3 | TS-20, TS-21, TS-22, TS-23, TS-24, SC-2 |
| FR5 Force-override settings on Linux | Phase 4 | TS-10 through TS-16, SC-5 |
| FR6 Hide settings rows on Linux | Phase 4 | SC-4, SC-6 |
| FR7 Cached platform detection | Phase 2 | TS-1, TS-2, TS-3 |
| NFR1 Non-blocking PRIMARY write | Phase 3 | Integration: `onMouseUp` returns without awaiting writePrimary |
| NFR2 Resilience | Phase 2, 3 | TS-6, TS-9, SC-9 |
| NFR3 Compatibility | Phase 1, 5 | SC-6, SC-7, SC-8 |
| NFR4 Build isolation | Phase 1 | `cargo tree` on Windows target |
| NFR5 Logging via warn+ | Phase 2, 3 | TS-6, TS-9 (assert log level) |

## E2E Testing (Docker)
ref: `docker-e2e-testing` skill

Run command: `./scripts/run-e2e-docker.sh test linux-primary-selection.e2e.js`

Automatable scenarios:
- [ ] E2E-1 (FR3, SC-1): Drag-select "Foo" in the terminal, release mouse, assert PRIMARY contains "Foo" (via a read helper exposed from the test harness or a secondary bridge call)
- [ ] E2E-2 (FR4, SC-2): After E2E-1, middle-click on an empty region, assert the PTY buffer receives "Foo"
- [ ] E2E-3 (SC-3): Seed CLIPBOARD with "Bar" via the existing clipboard bridge, drag-select "Foo", middle-click → assert "Foo" pasted; then Ctrl+V → assert "Bar" pasted
- [ ] E2E-4 (FR5, SC-5): Seed `settings.json` with `copy_on_select: true`, restart the app, select text, assert CLIPBOARD is NOT overwritten
- [ ] E2E-5 (FR6, SC-4): Open settings panel → Terminal Behavior section → assert no DOM node for the two removed rows
- [ ] E2E-6 (FR4): With PRIMARY empty, seed CLIPBOARD with "Bar", middle-click → assert "Bar" pasted (CLIPBOARD fallback)
- [ ] E2E-7 (FR4): With PRIMARY empty and CLIPBOARD empty, middle-click → assert no PTY write occurred

## Manual Testing (E2E Not Possible)
- [ ] M-1 (SC-6): On a Windows machine, verify the settings panel still shows both `copy_on_select` and `middle_click_paste` rows and they function as before
- [ ] M-2 (SC-7): On an X11 Linux host, run the full flow via real mouse input and visually verify PRIMARY interop with xterm or gnome-terminal
- [ ] M-3 (SC-8): On a Wayland Linux host (GNOME/KDE compositor supporting `wayland-data-control`), run the full flow and verify PRIMARY interop
- [ ] M-4 (SC-10): Select text in gnome-terminal, middle-click in eMterm → assert gnome-terminal's PRIMARY content is pasted
- [ ] M-5 (SC-10): Select text in eMterm, middle-click in gnome-terminal → assert eMterm's PRIMARY content is pasted
- [ ] M-6 (Wayland fallback): On a Wayland compositor WITHOUT `wayland-data-control`, verify the feature degrades gracefully (warn log, no crash)

## Performance Verification
- NFR1: `onMouseUp` must dispatch PRIMARY write asynchronously and return within 1 ms — measured by a micro-benchmark or manual profiling
- PRIMARY round-trip end-to-end < 50 ms typical on a clean desktop — measured manually with a stopwatch-style test

## Security Verification
- [ ] SEC-1: OSC 52 (via `src/terminal-app/osc-handler.ts`) must NOT write to PRIMARY — verified by code review and TS test using a mock bridge
- [ ] SEC-2: PRIMARY content is not logged in plain text anywhere (no debug log of selected text) — code review

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Functional (FR) | 7 | 28 unit/integration scenarios (TS-1 .. TS-28) | 7 E2E scenarios (E2E-1 .. E2E-7) | 2 manual (M-2, M-3) |
| Non-functional (NFR) | 5 | Logged-assertion unit tests + build tree check | — | 2 manual (M-2, M-3) |
| Platform regression | Windows | `cargo build` + `cargo tree` | — | 1 manual (M-1) |
| Interop | xterm / gnome-terminal | — | — | 2 manual (M-4, M-5) |
| Compositor edge | Wayland w/o data-control | — | — | 1 manual (M-6) |
| Security | OSC 52 isolation | 1 TS test + code review | — | — |
