# Implementation Plan: Linux PRIMARY Selection Support

## Overview
Add Linux PRIMARY selection as an independent clipboard alongside CLIPBOARD:
selection auto-copies to PRIMARY, middle-click pastes from PRIMARY with CLIPBOARD
fallback, and the `copy_on_select` / `middle_click_paste` settings are force-overridden
and hidden on Linux. Windows behavior is unchanged.

## Objectives
- Provide xterm/gnome-terminal-equivalent select-and-middle-click paste behavior on Linux
- Allow PRIMARY and CLIPBOARD to coexist so that `Ctrl+C` content survives subsequent selections
- Remove and force-override the two clipboard-related settings on Linux without mutating settings.json
- Preserve Windows behavior byte-for-byte

## Prerequisites

### Development Environment
- Rust 1.85+ (project's `rust-version`)
- Bun (package manager / bundler)
- Docker (for E2E tests)

### Dependencies
- `arboard` Rust crate with `wayland-data-control` feature, target-gated to Linux
- Existing `invoke<string>("get_platform")` Tauri command (already registered in `src-tauri/src/commands/wsl.rs`)
- Existing selection and middle-click infrastructure in `src/selection-v2/` and `src/terminal-app/`

## Architecture Overview

### Technology Stack
- **Backend language**: Rust (Tauri)
- **Frontend language**: TypeScript (vanilla, bundled with Bun)
- **Key libraries**:
  - `arboard` — cross-platform clipboard crate (X11 + Wayland PRIMARY support)
  - `tauri-plugin-clipboard-manager` — existing CLIPBOARD plugin (unchanged)

### Design Approach
- **Compile-time platform isolation on Rust side**: PRIMARY read/write commands are
  always registered, but their bodies are `#[cfg(target_os = "linux")]`. On Windows
  the commands compile to no-ops that return `Ok(())` / `Ok("")`. `arboard` is a
  `cfg(target_os = "linux")` dependency so the Windows build never pulls it in.
- **Runtime platform detection on TS side**: A single cached predicate (`isLinux()`)
  is resolved once during startup via the existing `get_platform` Tauri command. All
  selection/paste/settings code calls this predicate synchronously after startup.
- **Additive bridge, not replacement**: `writePrimary` / `readPrimary` are new
  methods added alongside the existing `write` / `read` CLIPBOARD methods. No
  existing callers need to change their CLIPBOARD semantics.
- **Runtime settings override, not file mutation**: A pair of effective-value
  accessors (`effectiveCopyOnSelect`, `effectiveMiddleClickPaste`) wraps the raw
  settings. On Linux they return hardcoded values; on Windows they return the raw
  values. `settings.json` is never rewritten.
- **Selective UI hiding**: The settings panel conditionally skips rendering the
  two affected toggle rows on Linux (they are absent from the DOM, not greyed out).

### Component Interaction

Startup:
1. `main.ts` boot calls `initPlatform()` which invokes `get_platform` and caches the result
2. `TerminalApp` and downstream modules can then call `isLinux()` synchronously

Selection path (Linux):
1. User drags mouse, releases
2. `SelectionController.onMouseUp` obtains the selected text
3. If a selection exists and `isLinux()` is true, `writePrimary(text)` is dispatched fire-and-forget
4. If `copy_on_select` effective value is true (Windows only), `copy()` writes to CLIPBOARD as before

Middle-click paste path (Linux):
1. User middle-clicks in the terminal container
2. `handleMiddleClickPaste` is invoked (gated by `effectiveMiddleClickPaste`)
3. `readPrimary()` is called first; if the returned text is empty, `read()` (CLIPBOARD) is called
4. The resulting text flows through the existing multi-line confirmation dialog and PTY write

Failure path:
- Any PRIMARY read/write error is caught, logged via `console.warn` (or Rust `log::warn!`), and swallowed
- The main flow continues unaffected

## Implementation Phases

### Phase 1: Rust backend (PRIMARY Tauri commands)

**Goal**: Expose `clipboard_write_primary` and `clipboard_read_primary` Tauri
commands that are fully functional on Linux (X11 + Wayland) and no-op on Windows.

**Files to Create**:
- `src-tauri/src/commands/clipboard_primary.rs` — new module containing both commands

**Files to Modify**:
- `src-tauri/Cargo.toml` — add `arboard` as a Linux target-gated dependency with `wayland-data-control` feature
- `src-tauri/src/commands/mod.rs` — declare the new `clipboard_primary` submodule
- `src-tauri/src/app.rs` — register the two commands in `tauri::generate_handler!`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `clipboard_write_primary` | Write a string to the OS PRIMARY selection on Linux; no-op elsewhere | Tauri runtime is initialized | On Linux: PRIMARY contains the given text, or `Err` on failure. On non-Linux: `Ok(())` unconditionally |
| `clipboard_read_primary` | Read the current PRIMARY selection on Linux; return empty string elsewhere | Tauri runtime is initialized | On Linux: returns the current PRIMARY content, empty string if unavailable, or `Err` on hard failure. On non-Linux: `Ok("")` unconditionally |

**Processing Flow** (diagram-convertible):

1. `clipboard_write_primary(text)` invoked from frontend
   - If `target_os == "linux"` → initialize arboard clipboard, write `text` to PRIMARY selection
     - Success → return Ok
     - Init or write error → return Err(message)
   - Else → return Ok (no-op)

2. `clipboard_read_primary()` invoked from frontend
   - If `target_os == "linux"` → initialize arboard clipboard, read PRIMARY selection
     - Success → return Ok(text)
     - Content-not-available → return Ok("")
     - Other error → return Err(message)
   - Else → return Ok("")

**Implementation Steps** (max 5-7):
1. **Add arboard dependency** — Add `arboard` to `[target.'cfg(target_os = "linux")'.dependencies]` with `wayland-data-control` feature
2. **Create clipboard_primary module** — New file `src-tauri/src/commands/clipboard_primary.rs` with both commands; bodies gated by `#[cfg(target_os = "linux")]`
3. **Wire module into commands tree** — Export from `src-tauri/src/commands/mod.rs`
4. **Register commands** — Add to the `tauri::generate_handler!` list in `src-tauri/src/app.rs`
5. **Rust unit tests** — Verify Linux variant wraps arboard errors correctly; verify non-Linux variant is a compile-time no-op

**Dependencies**: None (leaf phase)
**Blocks**: Phases 2-4

**Testing Approach**:
- Unit: Non-Linux variant returns `Ok(())` / `Ok("")` without touching any OS resources
- Unit: Linux variant propagates arboard initialization failure as `Err` (use a mock/flag if arboard can't be stubbed, otherwise skip Linux-only tests in CI non-Linux images)
- Integration: None at this layer (deferred to Phase 3 where the TS bridge is involved)
- Manual: On a Linux dev machine, call the command from a test TS harness and verify xterm can read the PRIMARY content via middle-click

**Acceptance Criteria**:
- [ ] `cargo build --manifest-path src-tauri/Cargo.toml` succeeds on Linux and Windows
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes
- [ ] New commands appear in the `invoke_handler` list
- [ ] Windows binary does not contain arboard symbols (verified by `cargo tree --target x86_64-pc-windows-msvc` not listing arboard)

**Estimated Effort**: small

---

### Phase 2: Frontend platform detection + ClipboardBridge extension

**Goal**: Provide a cached `isLinux()` predicate usable synchronously after startup
and add PRIMARY read/write methods to the clipboard abstractions.

**Files to Create**:
- `src/platform.ts` — platform detection helper with async `initPlatform()` and synchronous `isLinux()` / `isWindows()` predicates

**Files to Modify**:
- `src/main.ts` — await `initPlatform()` during the boot sequence before constructing `TerminalApp`
- `src/selection-v2/ClipboardBridge.ts` — add `writePrimary(text)` and `readPrimary()` methods
- `src/clipboard/manager.ts` — mirror the same two methods on `ClipboardManager` so both abstractions stay in sync

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `initPlatform()` | Resolve the platform identifier via `get_platform` and cache it in module state | Tauri runtime is available | Subsequent `isLinux()` / `isWindows()` calls return the correct value synchronously |
| `isLinux()` | Return cached Linux flag | `initPlatform()` has completed | Returns `true` on Linux, `false` otherwise (including pre-init states, which are defensive-false) |
| `ClipboardBridge.writePrimary(text)` | Write text to PRIMARY on Linux, no-op elsewhere | Platform cache populated | On Linux: dispatches Tauri command, returns `true` on success / `false` on caught error. On non-Linux: returns `false` without invoking a command |
| `ClipboardBridge.readPrimary()` | Read PRIMARY on Linux, empty string elsewhere | Platform cache populated | On Linux: returns PRIMARY content or empty string on failure. On non-Linux: returns empty string without invoking a command |

**Processing Flow**:

1. App startup (`main.ts`)
   - `initPlatform()` is awaited before other async boot steps
     - Tauri `invoke("get_platform")` succeeds → cache value
     - Tauri invocation fails (unexpected) → fall back to empty string, log warning, predicates all return false

2. `writePrimary(text)` invoked
   - If not Linux → return false (early exit, no Tauri call)
   - Else → dispatch Tauri command
     - Success → return true
     - Exception → `console.warn` with error, return false

3. `readPrimary()` invoked
   - If not Linux → return empty string (early exit, no Tauri call)
   - Else → dispatch Tauri command
     - Success → return text
     - Exception → `console.warn` with error, return empty string

**Implementation Steps**:
1. **Create platform module** — `src/platform.ts` exposes async initializer and synchronous predicates backed by module-scope cache
2. **Call initializer during boot** — Modify `main.ts` to await `initPlatform()` before constructing `TerminalApp` / attaching handlers
3. **Extend ClipboardBridge** — Add `writePrimary` / `readPrimary` to `ClipboardBridge` (selection-v2)
4. **Mirror on ClipboardManager** — Add the same two methods to the legacy `ClipboardManager` so both abstractions remain interchangeable
5. **Add focused unit tests** — Platform predicates return cached value; bridge methods respect non-Linux short-circuit; bridge methods swallow errors and log

**Dependencies**: Phase 1 (commands must exist to be invoked). Startup ordering must not race `initPlatform()` against the first selection event — since selection requires user interaction, natural delay is sufficient.
**Blocks**: Phases 3, 4

**Testing Approach**:
- Unit: `isLinux()` before init → false. After init with `"linux"` → true. After init with `"windows"` → false
- Unit: `writePrimary` / `readPrimary` non-Linux short-circuit; verify no Tauri invocation
- Unit: `writePrimary` / `readPrimary` Linux path catches and logs thrown errors
- Integration: Boot sequence test verifies `initPlatform()` is awaited (smoke test via a mock of `invoke`)

**Acceptance Criteria**:
- [ ] `bun test` passes with new unit tests
- [ ] `bun run typecheck` passes
- [ ] `isLinux()` / `isWindows()` mutually exclusive after init

**Estimated Effort**: small

---

### Phase 3: Selection & middle-click wiring

**Goal**: On Linux, every finalized selection writes to PRIMARY automatically, and
middle-click paste reads from PRIMARY first with CLIPBOARD fallback.

**Files to Modify**:
- `src/selection-v2/SelectionController.ts` — on `mouseup`, write the finalized selection text to PRIMARY when on Linux
- `src/terminal-app/ui-handler.ts` — in `handleMiddleClickPaste`, consult PRIMARY first on Linux, fall back to CLIPBOARD on empty / failure

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `SelectionController.onMouseUp` (modified) | Finalize the selection and, on Linux, push its text to PRIMARY independently of `copy_on_select` | A left-button `mouseup` has fired and a selection was active | On Linux: PRIMARY contains the selected text (or write was silently logged and skipped on failure). CLIPBOARD is unchanged unless `copy_on_select` effective value is true (Phase 4) |
| `handleMiddleClickPaste` (modified) | Resolve the text to paste using PRIMARY → CLIPBOARD priority on Linux; CLIPBOARD only on Windows | Middle-click was fired and middle-click paste is effectively enabled | Either the resolved text is sent to the PTY (after the existing multi-line confirmation), or no paste occurs if both sources are empty / errored |

**Processing Flow**:

1. `onMouseUp` (left button release)
   - Clear any pending selection start
   - If a selection is active
     - Call `endSelection()`
     - Obtain the current selected text
     - If text is non-empty and `isLinux()` → fire-and-forget `writePrimary(text)` (errors logged in bridge)
     - If `effectiveCopyOnSelect(settings)` → call existing `copy()` (writes CLIPBOARD)

2. `handleMiddleClickPaste`
   - If `isLinux()`
     - text ← `readPrimary()`
     - If text is empty → text ← `read()` (CLIPBOARD)
   - Else
     - text ← `read()` (CLIPBOARD)
   - If text is empty → return without action
   - Else → `exitScrollback()`, run existing multi-line dialog flow, write bytes to PTY

**Implementation Steps**:
1. **Modify `onMouseUp`** — After `endSelection()`, retrieve the selected text once and dispatch the PRIMARY write on Linux via the bridge; keep the existing `copy_on_select` branch unchanged (to be made effective-aware in Phase 4)
2. **Modify `handleMiddleClickPaste`** — Replace the unconditional `paste()` call with a priority-order resolver; preserve all downstream behavior (multi-line dialog, PTY write, scrollback exit, IME refocus)
3. **Factor a small helper if needed** — If resolution logic grows, extract `resolvePasteText(bridge)` for testability
4. **Add integration tests** — Fake bridge + fake `isLinux` to verify both selection and paste paths behave as specified on Linux and on Windows
5. **Confirm no regression in mouse tracking** — Selection should still be suppressed when PTY mouse tracking is enabled (e.g., vim with mouse support); PRIMARY write is only attempted when selection was actually made

**Dependencies**: Phase 1, Phase 2
**Blocks**: Phase 5 testing scenarios

**Testing Approach**:
- Unit: `onMouseUp` on Linux with non-empty selection calls `writePrimary` exactly once with the expected text
- Unit: `onMouseUp` on Linux with empty selection does not call `writePrimary`
- Unit: `onMouseUp` on Windows does not call `writePrimary`
- Unit: `handleMiddleClickPaste` on Linux: PRIMARY non-empty → paste PRIMARY; PRIMARY empty → paste CLIPBOARD
- Unit: `handleMiddleClickPaste` on Linux: PRIMARY empty and CLIPBOARD empty → no PTY write
- Unit: `handleMiddleClickPaste` on Windows → only reads CLIPBOARD
- Integration: Selection → middle-click round-trip via mocked bridge

**Acceptance Criteria**:
- [ ] `bun test` covering SelectionController and ui-handler passes
- [ ] Mouse-tracking suppression is not regressed
- [ ] Multi-line paste confirmation dialog still fires for multi-line PRIMARY content

**Estimated Effort**: small to medium

---

### Phase 4: Settings force-override + Linux UI hiding

**Goal**: On Linux, `copy_on_select` and `middle_click_paste` have hardcoded
effective values (`false` and `true` respectively) regardless of `settings.json`,
and the settings UI does not present these rows.

**Files to Create**:
- `src/settings/effective-settings.ts` (or an addition inside `settings-service.ts`)
  — single module exposing `effectiveCopyOnSelect(settings)` and `effectiveMiddleClickPaste(settings)` accessors

**Files to Modify**:
- `src/selection-v2/SelectionController.ts` — use `effectiveCopyOnSelect` instead of reading `settings.copy_on_select` directly
- `src/terminal-app/index.ts` — use `effectiveMiddleClickPaste` instead of `settings?.middle_click_paste !== false` around the middle-click listener
- `src/settings/sections/terminal-behavior-section.ts` — wrap the two `renderToggle` calls for `copy-on-select` and `middle-click-paste` in a `!isLinux()` guard so they are not rendered at all on Linux

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `effectiveCopyOnSelect(settings)` | Return the policy-adjusted effective value for `copy_on_select` | Settings object is available; platform cache populated | On Linux: always `false`. On Windows: raw `settings.copy_on_select ?? false` |
| `effectiveMiddleClickPaste(settings)` | Return the policy-adjusted effective value for `middle_click_paste` | Settings object is available; platform cache populated | On Linux: always `true`. On Windows: raw `settings.middle_click_paste !== false` (existing default-true behavior) |
| Settings panel (modified) | Conditionally omit DOM rendering of the two toggles on Linux | Panel renderer runs | Two rows are absent from the DOM on Linux; present and functional on Windows |

**Processing Flow**:

1. Reading `copy_on_select` anywhere in UI logic
   - Replace direct access with `effectiveCopyOnSelect(settings)`
     - Linux path → returns false without touching raw value
     - Windows path → returns raw value (or default false)

2. Reading `middle_click_paste` anywhere in UI logic
   - Replace direct access with `effectiveMiddleClickPaste(settings)`

3. Rendering the Terminal Behavior settings section
   - For each of the two affected toggles:
     - If `isLinux()` → skip rendering
     - Else → render as before

**Implementation Steps**:
1. **Create accessors** — Single module with two pure functions that consult `isLinux()` and return the policy value
2. **Rewire SelectionController** — Replace `settings?.copy_on_select` access with `effectiveCopyOnSelect(settings)`
3. **Rewire terminal-app middle-click gate** — Replace the inline `settings?.middle_click_paste !== false` check with `effectiveMiddleClickPaste(settings)`
4. **Hide settings rows on Linux** — Wrap the two `renderToggle` blocks in `terminal-behavior-section.ts` with `if (!isLinux())`
5. **Unit-test the accessors** — Verify linux/windows branches return the hardcoded / raw values respectively
6. **Verify settings.json is not mutated** — The save pipeline is unchanged; existing `settings.json` with `copy_on_select: true` on Linux should remain `true` in the file even after the runtime override is applied

**Dependencies**: Phase 2 (platform detection), Phase 3 (call sites to rewire)
**Blocks**: Phase 5 (verification)

**Testing Approach**:
- Unit: `effectiveCopyOnSelect` with Linux flag returns false regardless of `settings.copy_on_select` value (true/false/undefined)
- Unit: `effectiveMiddleClickPaste` with Linux flag returns true regardless of raw value
- Unit: Non-Linux branch returns raw value, preserving existing Windows defaults
- Integration: Selection on Linux with `settings.copy_on_select = true` does not write CLIPBOARD (only PRIMARY)
- Integration: Middle-click on Linux with `settings.middle_click_paste = false` still pastes
- Integration: Saving any unrelated setting on Linux does not mutate `copy_on_select` or `middle_click_paste` in `settings.json`
- Manual: Open settings panel on Linux and visually confirm the two rows are absent; on Windows confirm they are present

**Acceptance Criteria**:
- [ ] Linux runtime ignores `settings.json` values for the two keys
- [ ] `settings.json` retains the original key values after runtime override (file is not rewritten)
- [ ] Linux settings panel does not render the two rows
- [ ] Windows settings panel renders both rows exactly as today

**Estimated Effort**: small

---

### Phase 5: Testing & documentation

**Goal**: Full validation pass including E2E on Linux, plus README / CHANGELOG
updates describing the Linux behavior change.

**Files to Create**:
- `e2e-tests/specs/linux-primary-selection.e2e.js` (Linux-only test spec, guarded
  to be skipped on Windows CI) — selection → middle-click round-trip scenarios

**Files to Modify**:
- `README.md` — short note under Linux / clipboard section explaining the new
  PRIMARY behavior and that the two settings are Linux-hidden
- `CHANGELOG.md` (or the release notes source, e.g. `doc/release/SPECIFICATION.md`)
  — describe the behavior change and migration guidance

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| E2E Linux spec | Exercise the full selection → PRIMARY → middle-click path inside the Docker E2E harness | `./scripts/run-e2e-docker.sh build` image exists and current debug binary is available | Automated confirmation that the feature works end to end |
| README / CHANGELOG updates | Communicate the behavior change to users | Phases 1-4 merged | Users understand that their existing `copy_on_select` / `middle_click_paste` settings are overridden on Linux |

**Processing Flow**:
1. Run full TS test suite (Docker): `bun test` + `bun run typecheck`
2. Run full Rust test suite (Docker): `cargo test`
3. Run full E2E suite (Docker): `./scripts/run-e2e-docker.sh test`
4. Update README / CHANGELOG text
5. Final review pass, ready for `/sdd.5-check` and `/sdd.6-verify`

**Implementation Steps**:
1. **Author E2E spec** — Uses tauri-driver + WebdriverIO to simulate mouse drag, mouseup, middle-click, and PTY output assertion
2. **Run Docker E2E** — `./scripts/run-e2e-docker.sh test linux-primary-selection.e2e.js`
3. **Update README** — Describe Linux PRIMARY behavior in the existing clipboard / selection section
4. **Update CHANGELOG** — Mention the runtime override of the two settings on Linux
5. **Cross-platform smoke** — Manually confirm Windows build still compiles and settings panel still shows both rows (Windows dev or CI run)

**Dependencies**: Phases 1-4
**Blocks**: `/sdd.5-check`, `/sdd.6-verify`, `/user-code-review`

**Testing Approach**:
- E2E (Docker, Linux): Full select-to-paste workflow, including CLIPBOARD isolation check
- E2E (Docker, Linux): Settings panel rendering assertion (the two removed rows are not in the DOM)
- Manual: Interop with another Linux terminal (e.g., gnome-terminal) via PRIMARY
- Manual: Confirm Windows behavior unchanged (visual and functional check of settings panel and existing paste behavior)

**Acceptance Criteria**:
- [ ] `bun test` passes across all Phase 1-4 changes
- [ ] `cargo test` passes on both target platforms
- [ ] `./scripts/run-e2e-docker.sh test` passes, including the new spec
- [ ] README / CHANGELOG describes the behavior change
- [ ] No regressions reported in other selection / paste / settings flows

**Estimated Effort**: small to medium

---

## Complete File Structure

```
doc/tasks/linux-primary-selection/
├── 要件定義書.md                        # existing (Phase 0)
├── SPEC.md                               # existing (Phase 0)
├── IMPLEMENTATION.md                     # this document
├── VERIFICATION.md                       # companion verification plan
└── sdd.yaml                              # SDD metadata

src-tauri/
├── Cargo.toml                            # MOD: + arboard (linux target)
└── src/
    ├── app.rs                            # MOD: register new commands
    └── commands/
        ├── mod.rs                        # MOD: declare submodule
        └── clipboard_primary.rs          # NEW: read/write PRIMARY

src/
├── main.ts                               # MOD: await initPlatform during boot
├── platform.ts                           # NEW: platform detection helper
├── selection-v2/
│   ├── ClipboardBridge.ts                # MOD: +writePrimary/+readPrimary
│   └── SelectionController.ts            # MOD: write PRIMARY on mouseup (Linux)
├── clipboard/
│   └── manager.ts                        # MOD: mirror writePrimary/readPrimary
├── terminal-app/
│   ├── index.ts                          # MOD: effectiveMiddleClickPaste gate
│   └── ui-handler.ts                     # MOD: PRIMARY-first paste resolution
└── settings/
    ├── effective-settings.ts             # NEW: effective-value accessors
    └── sections/
        └── terminal-behavior-section.ts  # MOD: hide rows on Linux

e2e-tests/
└── specs/
    └── linux-primary-selection.e2e.js    # NEW: E2E spec (Linux-only)

README.md                                  # MOD: describe Linux clipboard behavior
CHANGELOG.md (or release notes source)     # MOD: note the runtime override
```

## Testing Strategy

- **Unit** — Target 80%+ coverage for new TS modules (`platform.ts`,
  `effective-settings.ts`, new `ClipboardBridge` / `ClipboardManager` methods).
  Target 90%+ for the small-surface `clipboard_primary.rs` (both Linux and non-Linux
  variants).
- **Integration** — Exercise the Selection → PRIMARY write and middle-click →
  PRIMARY / CLIPBOARD resolution paths with fake bridges.
- **E2E (Docker)** — Follow the `docker-e2e-testing` skill, using `tauri-driver`
  to drive a real build. One new spec for the feature's golden path plus settings
  panel assertion. See CLAUDE.md for command syntax.
- **Manual** — Items requiring human visual judgment (settings panel row hiding on
  Linux, interop with another terminal) or cross-platform (Windows regression
  pass).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `arboard` (Rust) | 3.x | Cross-platform clipboard with X11/Wayland PRIMARY support |

No new TypeScript dependencies. Existing `@tauri-apps/api` `invoke` and the
already-registered `get_platform` Tauri command are reused for platform detection.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| arboard API shape differs from spec draft (`set().clipboard(Primary)` vs legacy `set_text_with_clipboard`) | Medium | Low | Pin exact version during Phase 1, adjust the call site, tests catch any mismatch |
| Wayland compositor without `wayland-data-control` support | Medium | Low | Silent fallback via the existing error path — write/read fail, warning log, feature degrades to CLIPBOARD-only |
| `initPlatform()` not awaited early enough, causing `isLinux()` to return false during early interactions | Low | Medium | Place the await early in `main.ts` before user-facing handlers are attached; unit test the boot sequence |
| Windows build accidentally pulls in arboard via a non-gated dependency | Low | Medium | Use `[target.'cfg(target_os = "linux")'.dependencies]`; verify with `cargo tree --target x86_64-pc-windows-msvc` in CI |
| A user who previously relied on `copy_on_select=true` on Linux experiences sudden CLIPBOARD non-update | Medium | Low | Document clearly in README / CHANGELOG; note that PRIMARY now holds the selected text and Ctrl+C still works unchanged |
| Mouse-tracking interaction: PTY apps that enable mouse tracking may previously have suppressed selection; behavior must remain the same | Low | Medium | No change needed — selection suppression still flows through `shouldHandleSelection`; PRIMARY write is only triggered after a real selection was made |

## Open Questions

- [ ] Exact arboard API to use — pinned during Phase 1 implementation
- [ ] Whether to provide a hidden / undocumented escape hatch for Linux users who
      want the old behavior — tentatively no, but worth flagging to reviewers

## Success Metrics

- [ ] Functional completeness: all seven FRs (FR1-FR7) behave as specified on Linux
- [ ] Quality: `bun test`, `bun run typecheck`, `cargo test`, and `./scripts/run-e2e-docker.sh test` all pass
- [ ] Performance: selection-end to PRIMARY dispatch < 1 ms on the main thread; end-to-end PRIMARY write under 50 ms typical
- [ ] Compatibility: Windows binary size and dependency tree are unchanged (arboard not present)
- [ ] User-visible: `Ctrl+C` content survives arbitrary selections on Linux; middle-click pastes selected text
