# Feature: Linux PRIMARY Selection Support

## Overview

Add first-class support for the X11/Wayland PRIMARY selection on Linux, independent
of the standard CLIPBOARD. Text selection automatically writes to PRIMARY, middle-click
reads from PRIMARY (falling back to CLIPBOARD), and the two clipboards coexist without
destroying each other. On Linux, the `copy_on_select` and `middle_click_paste` settings
are removed from the UI and force-overridden to match standard terminal behavior
(`copy_on_select = false`, `middle_click_paste = true`). Windows behavior is unchanged.

## Objectives

- Match the clipboard behavior of mainstream Linux terminals (xterm, gnome-terminal,
  konsole, WezTerm) so that users can select text with the mouse and paste it via
  middle-click without destroying what is in `Ctrl+C`'s CLIPBOARD.
- Keep PRIMARY and CLIPBOARD as independent storage so a single eMterm session can
  hold two distinct payloads simultaneously.
- Simplify the Linux settings UI by removing two options that no longer make sense
  once PRIMARY is supported natively.
- Preserve Windows behavior byte-for-byte.

## User Stories

### US1: Select-and-middle-click paste on Linux
As a Linux user, I want to select text with the mouse and middle-click to paste it,
so that I can use the same fast text workflow that every other Linux terminal
provides.

**Acceptance Criteria:**
- [ ] Selecting text in the terminal writes that text to PRIMARY.
- [ ] Middle-clicking on the terminal pastes the PRIMARY content into the PTY.
- [ ] Selection works for single-line, multi-line, wrapped, scrollback, and
      Unicode/emoji content.

### US2: PRIMARY and CLIPBOARD coexist
As a Linux user, I want `Ctrl+C` copies to survive even when I select other text,
so that I do not lose the content I explicitly copied.

**Acceptance Criteria:**
- [ ] After `Ctrl+C`-ing `Bar`, selecting `Foo` leaves CLIPBOARD == `Bar`.
- [ ] Middle-click after the above pastes `Foo` (from PRIMARY).
- [ ] `Ctrl+V` after the above pastes `Bar` (from CLIPBOARD).

### US3: Cross-application PRIMARY exchange
As a Linux user, I want PRIMARY content to flow between eMterm and other apps like
gnome-terminal or a text editor, so that my mouse-based clipboard is unified with
the rest of the desktop.

**Acceptance Criteria:**
- [ ] Selecting text in gnome-terminal and middle-clicking in eMterm pastes that
      text.
- [ ] Selecting text in eMterm and middle-clicking in gnome-terminal pastes that
      text.

### US4: Linux settings UI cleanup
As a Linux user, I want the two clipboard-confusion settings to be hidden,
so that I am not offered choices that cannot produce correct Linux behavior.

**Acceptance Criteria:**
- [ ] The settings UI on Linux does not render `copy_on_select` or
      `middle_click_paste` rows.
- [ ] On Linux, whatever values those keys hold in `settings.json` are ignored
      at runtime.

### US5: Windows behavior is unchanged
As a Windows user, I want my eMterm to behave exactly as before this change.

**Acceptance Criteria:**
- [ ] Both settings are still visible in the Windows settings UI.
- [ ] `copy_on_select` still controls whether selection copies to the clipboard.
- [ ] `middle_click_paste` still controls whether middle-click pastes.
- [ ] The Rust build for Windows does not pull in the `arboard` crate.

## Technical Requirements

### Functional Requirements

- **FR1 - PRIMARY write command:** A Tauri command `clipboard_write_primary(text: String)`
  that writes to the X11/Wayland PRIMARY selection on Linux and is a no-op on other
  platforms.
- **FR2 - PRIMARY read command:** A Tauri command `clipboard_read_primary() -> String`
  that reads from PRIMARY on Linux and returns an empty string on other platforms.
- **FR3 - Auto-write on selection:** `SelectionController.onMouseUp` writes the
  current selection to PRIMARY on Linux every time a selection is finalized. This
  is independent of `copy_on_select` (which, on Linux, is treated as always false).
- **FR4 - Middle-click reads PRIMARY first:** `handleMiddleClickPaste` reads from
  PRIMARY on Linux. If PRIMARY is empty, it falls back to reading CLIPBOARD so that
  a "Ctrl+C in another app → middle-click here" workflow still works. On Windows it
  reads CLIPBOARD unconditionally (existing behavior).
- **FR5 - Force-override settings on Linux:** On Linux, the effective values of
  `copy_on_select` and `middle_click_paste` are hardcoded to `false` and `true`
  regardless of what `settings.json` contains. The file is **not** rewritten.
- **FR6 - Hide settings rows on Linux:** The Linux settings panel does not render
  the `copy_on_select` and `middle_click_paste` rows. (They remain visible on
  Windows.)
- **FR7 - Platform detection helper:** A single TypeScript module exposes an
  `isLinux()` (or equivalent) predicate that is resolved once at startup and cached
  for synchronous use.

### Non-Functional Requirements

- **NFR1 - Performance:** PRIMARY write must not block the main thread. The
  `onMouseUp` path should remain synchronous for the caller; PRIMARY write is
  fire-and-forget (`.catch(() => {})`).
- **NFR2 - Resilience:** Any failure in the PRIMARY read/write path must not crash
  or visibly disrupt eMterm. Failures are logged via `console.warn` only.
- **NFR3 - Compatibility:** Works on both X11 and Wayland (the latter via
  `wayland-data-control`-capable compositors). Must not regress Windows builds.
- **NFR4 - Build isolation:** The `arboard` crate is a `cfg(target_os = "linux")`
  dependency so Windows binaries do not pull it in.
- **NFR5 - Logging:** All log output uses `console.warn` or higher so it lands in
  `emterm.log`.

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Frontend (TypeScript)                                           │
│                                                                 │
│  ┌──────────────────┐    ┌──────────────────────────────────┐   │
│  │ platform.ts      │    │ SelectionController.onMouseUp    │   │
│  │ isLinux() (cached)│◀──┤ - Always PRIMARY.write on Linux │    │
│  └──────────┬───────┘    └──────────────────┬───────────────┘   │
│             │                               │                   │
│             ▼                               ▼                   │
│  ┌──────────────────────────────────────────────────┐           │
│  │ ClipboardBridge                                  │           │
│  │  + writePrimary(text: string)                    │           │
│  │  + readPrimary(): string                         │           │
│  │  (read/write CLIPBOARD unchanged)                │           │
│  └──────────────────┬───────────────────────────────┘           │
│                     │ invoke()                                  │
└─────────────────────┼───────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│ Backend (Rust, src-tauri/)                                      │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │ commands/clipboard_primary.rs                           │    │
│  │                                                         │    │
│  │  #[cfg(target_os = "linux")] uses arboard              │    │
│  │   - clipboard_write_primary                             │    │
│  │   - clipboard_read_primary                              │    │
│  │                                                         │    │
│  │  #[cfg(not(target_os = "linux"))] are no-ops           │    │
│  └──────────────────┬──────────────────────────────────────┘    │
│                     │                                            │
│                     ▼                                            │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │ arboard (Linux-only dep)                                │    │
│  │   X11 (x11-clipboard) + Wayland (wayland-data-control)  │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

**Select → PRIMARY (Linux):**
```
User mouse drag → mouseup
  ↓
SelectionController.onMouseUp
  ↓
getSelectedText()
  ↓
if (isLinux()) ClipboardBridge.writePrimary(text)
  ↓
invoke('clipboard_write_primary', { text })
  ↓
Rust: arboard::Clipboard::new()
         .set()
         .primary()      (wayland-data-control or X11)
         .text(text)
```

**Middle-click → paste (Linux):**
```
User middle-click
  ↓
handleMiddleClickPaste
  ↓
if (isLinux())
    text = ClipboardBridge.readPrimary()
    if (!text) text = ClipboardBridge.read()      // CLIPBOARD fallback
else
    text = ClipboardBridge.read()
  ↓
(existing multi-line dialog + PTY write)
```

### API Design

#### Tauri command: `clipboard_write_primary`

```rust
#[tauri::command]
pub fn clipboard_write_primary(text: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use arboard::{Clipboard, LinuxClipboardKind, SetExtLinux};
        let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
        cb.set()
            .clipboard(LinuxClipboardKind::Primary)
            .text(text)
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = text;
    }
    Ok(())
}
```

Front-end invocation:
```ts
await invoke('clipboard_write_primary', { text });
```

#### Tauri command: `clipboard_read_primary`

```rust
#[tauri::command]
pub fn clipboard_read_primary() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        use arboard::{Clipboard, LinuxClipboardKind, GetExtLinux};
        let mut cb = Clipboard::new().map_err(|e| e.to_string())?;
        match cb.get().clipboard(LinuxClipboardKind::Primary).text() {
            Ok(s) => Ok(s),
            Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(String::new())
    }
}
```

> **Note:** The exact arboard API shape (`set().clipboard(...)` vs
> `set_text_with_clipboard(...)`) should be validated against the pinned version
> at implementation time. The semantic intent is: **set/get the PRIMARY selection
> on Linux, no-op elsewhere**.

#### TypeScript: `ClipboardBridge` additions

```ts
// src/selection-v2/ClipboardBridge.ts
async writePrimary(text: string): Promise<boolean> {
    if (!isLinux()) return false;
    try {
        await invoke('clipboard_write_primary', { text });
        return true;
    } catch (error) {
        console.warn('[WARN][FRONTEND] Failed to write PRIMARY:', error);
        return false;
    }
}

async readPrimary(): Promise<string> {
    if (!isLinux()) return '';
    try {
        return await invoke<string>('clipboard_read_primary');
    } catch (error) {
        console.warn('[WARN][FRONTEND] Failed to read PRIMARY:', error);
        return '';
    }
}
```

Both the existing `ClipboardBridge` (`src/selection-v2/ClipboardBridge.ts`) and the
`ClipboardManager` (`src/clipboard/manager.ts`) get these methods; callers use
whichever is already in scope.

### Platform detection

A single module in `src/platform.ts` (or extended from an existing utility):

```ts
// src/platform.ts
import { platform } from '@tauri-apps/plugin-os';

let cached: string | null = null;

/** Must be awaited once at startup to prime the cache. */
export async function initPlatform(): Promise<void> {
    cached = await platform();
}

export function isLinux(): boolean {
    return cached === 'linux';
}

export function isWindows(): boolean {
    return cached === 'windows';
}
```

`initPlatform()` is awaited in `main.ts` before the TerminalApp is constructed.
Any code path that runs before `initPlatform()` resolves must not call `isLinux()`;
in practice all selection/paste code runs after user interaction, long after
startup, so this is safe.

### Settings override

The effective-settings accessor lives in `src/settings/settings-applier.ts` or
`src/settings/settings-service.ts` (whichever is the single entry point the rest of
the code uses). A small wrapper computes the effective value:

```ts
// Pseudocode, actual location TBD at plan time.
export function effectiveCopyOnSelect(settings: AppSettings): boolean {
    if (isLinux()) return false;                 // FR5: forced
    return settings.copy_on_select ?? false;
}

export function effectiveMiddleClickPaste(settings: AppSettings): boolean {
    if (isLinux()) return true;                  // FR5: forced
    return settings.middle_click_paste !== false;
}
```

Call sites:
- `SelectionController.onMouseUp` checks `effectiveCopyOnSelect(settings)` for the
  CLIPBOARD write branch; the PRIMARY write is always attempted on Linux.
- `terminal-app/index.ts:384` uses `effectiveMiddleClickPaste(settings)` instead of
  `settings?.middle_click_paste !== false`.

No `settings.json` mutation happens at any point.

### Settings UI hiding

In the settings panel section renderer (`src/settings/settings-sections.ts` or
equivalent), the rows for `copy_on_select` and `middle_click_paste` are wrapped in:

```ts
if (!isLinux()) {
    // render row
}
```

so that on Linux the two rows are not inserted into the DOM at all.

### Dependencies

**Internal:**
- `src/selection-v2/SelectionController.ts` - onMouseUp path for the auto-write
- `src/selection-v2/ClipboardBridge.ts` - new methods
- `src/clipboard/manager.ts` - new methods (mirror API)
- `src/terminal-app/index.ts` - middle-click handler uses `effectiveMiddleClickPaste`
- `src/terminal-app/ui-handler.ts` - `handleMiddleClickPaste` uses `readPrimary` first
- `src/settings/settings-applier.ts` (or service) - effective-value accessors
- `src/settings/settings-sections.ts` (or equivalent) - Linux UI hiding
- `src/platform.ts` - new module (or extension of existing util)
- `src/main.ts` - call `initPlatform()` during startup
- `src-tauri/src/app.rs` - register new commands
- `src-tauri/src/commands/mod.rs` - new submodule
- `src-tauri/src/commands/clipboard_primary.rs` - new file

**External (Rust, Linux only):**
- `arboard = { version = "3", features = ["wayland-data-control"] }`
  (target-gated: `[target.'cfg(target_os = "linux")'.dependencies]`)

**External (TypeScript):**
- `@tauri-apps/plugin-os` for platform detection (add if not already present).

### File Structure

```
src-tauri/
├── Cargo.toml                              # add arboard (linux target)
└── src/
    ├── app.rs                              # register new commands
    └── commands/
        ├── mod.rs                          # export clipboard_primary
        └── clipboard_primary.rs            # NEW: Tauri commands

src/
├── platform.ts                             # NEW: platform detection
├── main.ts                                 # call initPlatform()
├── selection-v2/
│   ├── ClipboardBridge.ts                  # +writePrimary/+readPrimary
│   └── SelectionController.ts              # onMouseUp writes PRIMARY
├── clipboard/
│   └── manager.ts                          # +writePrimary/+readPrimary
├── terminal-app/
│   ├── index.ts                            # middle-click uses effective*
│   └── ui-handler.ts                       # handleMiddleClickPaste path
└── settings/
    ├── settings-applier.ts (or service)    # effective*() accessors
    └── settings-sections.ts                # Linux UI hiding
```

## Test Scenarios

### Unit Tests
- [ ] `platform.ts`: `isLinux()` returns the cached platform value.
- [ ] `ClipboardBridge.writePrimary`: returns `false` on non-Linux without invoking
      the command.
- [ ] `ClipboardBridge.readPrimary`: returns `''` on non-Linux without invoking the
      command.
- [ ] `effectiveCopyOnSelect`: returns `false` on Linux regardless of the
      `settings.json` value; returns the raw value on Windows.
- [ ] `effectiveMiddleClickPaste`: returns `true` on Linux regardless of the
      `settings.json` value; returns the raw value on Windows.

### Integration Tests
- [ ] `SelectionController.onMouseUp` on Linux calls `writePrimary` with the
      selected text when a non-empty selection ends.
- [ ] `handleMiddleClickPaste` on Linux calls `readPrimary` first; on empty result,
      falls back to `read` (CLIPBOARD).
- [ ] `handleMiddleClickPaste` on Windows only calls `read` (CLIPBOARD).
- [ ] Rust: `clipboard_write_primary` / `clipboard_read_primary` no-op variants
      compile and return `Ok` / `Ok("")` on Windows.

### E2E Tests
**Existing E2E tests**: Docker-based tauri-driver suite under `e2e-tests/`.
**Run command**: `./scripts/run-e2e-docker.sh test`

- [ ] Existing selection E2E tests pass without regression.
- [ ] Linux E2E: select text in a pane, middle-click, verify the text is pasted
      back into the PTY.
- [ ] Linux E2E: `Ctrl+C` some text, select other text, `Ctrl+V` → original text
      pastes (CLIPBOARD untouched).
- [ ] Linux E2E: settings panel does not contain the removed rows.

### Edge Cases
- [ ] Empty selection: `onMouseUp` does not attempt to write PRIMARY when the
      selection is empty or whitespace-only.
- [ ] Very large selection (several MB): PRIMARY write does not crash; if the
      platform rejects it, a warning is logged and nothing else happens.
- [ ] Unicode/emoji selection: round-trips through PRIMARY correctly.
- [ ] Multi-line selection: preserved in PRIMARY as `\n`-separated lines.
- [ ] PRIMARY empty + CLIPBOARD empty: middle-click pastes nothing and does not
      throw.
- [ ] PRIMARY read fails at the OS level: fall back to CLIPBOARD; if that also
      fails, paste nothing.
- [ ] Wayland compositor without `wayland-data-control`: write/read fail, logged,
      silently skipped.
- [ ] `settings.json` has `copy_on_select: true` on Linux: ignored; CLIPBOARD is
      not written on selection.
- [ ] `settings.json` has `middle_click_paste: false` on Linux: ignored;
      middle-click still pastes.
- [ ] Mouse tracking (PTY apps like `vim -m`) still suppresses selection as before;
      this feature does not change that.

### Performance Tests
- [ ] Rapid small selections (10/s): no visible lag; PRIMARY write is
      fire-and-forget.
- [ ] One-shot large selection (1 MB plain ASCII): completes within 200 ms on a
      typical Linux desktop.

## Security Considerations

- **Authentication:** N/A (local desktop feature).
- **Authorization:** Uses the OS's existing clipboard permissions; no escalation.
- **Input Validation:** Text passed to PRIMARY is opaque bytes; no parsing.
- **Data Protection:** PRIMARY content is visible to any other X11/Wayland client,
  same as in any other terminal. Users should be aware that selecting a password on
  Linux may expose it to other apps; this is consistent with every other Linux
  terminal. We add no extra sanitization beyond what CLIPBOARD already gets.
- **XSS/SQL/CSRF:** Not applicable (not a web surface).
- **OSC 52 interaction:** OSC 52 remains CLIPBOARD-only. It never writes to PRIMARY,
  so a remote program cannot leak content to PRIMARY behind the user's back.

## Error Handling

### Error Codes / Modes

| Mode | Trigger | User-visible effect |
|------|---------|---------------------|
| PRIMARY write error | arboard / OS clipboard init or set failure | None (warning log only) |
| PRIMARY read error | arboard / OS clipboard init or get failure | Silent fallback to CLIPBOARD |
| PRIMARY empty | `ContentNotAvailable` | Silent fallback to CLIPBOARD |
| CLIPBOARD read error | Tauri plugin failure | Nothing pasted, warning log |

### Error Flow

```
Selection end (Linux)
  → writePrimary(text)
    → Err → console.warn, continue
    → Ok  → continue

Middle-click (Linux)
  → readPrimary()
    → '' or Err → read() (CLIPBOARD)
      → '' or Err → paste nothing
      → text     → paste text
    → text       → paste text
```

## Performance Optimization

### Performance Goals
- Selection end → PRIMARY write command dispatch: < 1 ms (main thread work only)
- PRIMARY write completion: < 50 ms typical
- PRIMARY read completion: < 50 ms typical

### Optimization Strategies
- **Fire-and-forget writes:** `writePrimary` is awaited but the result is
  discarded; callers do not block the UI on PRIMARY latency.
- **Cached platform detection:** `isLinux()` is a synchronous cache lookup after
  one-time startup resolution; no repeated async Tauri IPC calls.
- **No PRIMARY for empty selections:** Skip the IPC round-trip entirely when there
  is nothing to write.

### Caching Strategy
- Platform value: cached once per process lifetime.

## Success Criteria

- [ ] All functional requirements (FR1–FR7) are implemented and tested.
- [ ] Unit, integration, and E2E tests pass on Linux.
- [ ] Windows build compiles with no new dependencies pulled in; existing Windows
      E2E tests pass unchanged.
- [ ] Selecting text in eMterm followed by middle-clicking inside or outside
      eMterm reproduces xterm/gnome-terminal behavior.
- [ ] `Ctrl+C` content survives subsequent selections on Linux.
- [ ] Settings UI on Linux does not render the two removed rows.
- [ ] Stray values in `settings.json` are ignored without file mutation.
- [ ] All PRIMARY error paths log via `console.warn` only and never crash or
      disrupt other features.
- [ ] README / CHANGELOG mention the Linux behavior change.

## Open Questions

None.

## Implementation Phases

### Phase 1: Rust backend
**Goals:** PRIMARY read/write Tauri commands working on Linux, no-op on Windows.
**Deliverables:**
- `src-tauri/Cargo.toml` target-gated `arboard` dependency.
- `src-tauri/src/commands/clipboard_primary.rs` with both commands.
- `src-tauri/src/app.rs` registers the commands.
- `cargo test --manifest-path src-tauri/Cargo.toml` passes on Linux and Windows.

### Phase 2: Platform detection + ClipboardBridge extension
**Goals:** Frontend can call the new commands and knows which platform it is on.
**Deliverables:**
- `src/platform.ts` with `initPlatform()` / `isLinux()` / `isWindows()`.
- `main.ts` awaits `initPlatform()` during startup.
- `ClipboardBridge.writePrimary` / `readPrimary`.
- `ClipboardManager.writePrimary` / `readPrimary` (mirror).
- Unit tests for the new methods.

### Phase 3: Selection and middle-click wiring
**Goals:** Selection auto-writes PRIMARY; middle-click reads PRIMARY first.
**Deliverables:**
- `SelectionController.onMouseUp` calls `writePrimary` on Linux.
- `handleMiddleClickPaste` path uses `readPrimary` with fallback on Linux.
- Integration tests.

### Phase 4: Settings force-override and UI hiding
**Goals:** On Linux, the two settings behave as hardcoded and do not appear in the
UI.
**Deliverables:**
- `effectiveCopyOnSelect` / `effectiveMiddleClickPaste` accessors.
- Call sites updated.
- Settings panel rows conditionally hidden on Linux.
- Unit tests for effective-value accessors.

### Phase 5: Testing & documentation
**Goals:** Full validation and user-facing docs.
**Deliverables:**
- Linux / Windows E2E runs green.
- README / CHANGELOG updates describing the Linux behavior change.

## References

- `doc/tasks/linux-primary-selection/要件定義書.md` - requirements in Japanese.
- `doc/tasks/middle-click-paste/SPEC.md` - existing middle-click-paste spec.
- `src/selection-v2/SelectionController.ts` - current selection implementation.
- `src/selection-v2/ClipboardBridge.ts` - current clipboard bridge.
- `src/terminal-app/index.ts:376-400` - middle-click handler registration.
- `src/terminal-app/ui-handler.ts:91-125` - `handleMiddleClickPaste`.
- [arboard crate](https://crates.io/crates/arboard)
- [Freedesktop Clipboard Spec](https://specifications.freedesktop.org/clipboards-spec/clipboards-latest.txt)
- [Wayland wlr-data-control protocol](https://wayland.app/protocols/wlr-data-control-unstable-v1)
