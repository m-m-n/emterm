# Feature: Comprehensive Special Key Mapping

## Overview

Extend emterm's keyboard handler (`keyEventToBytes`) to support all standard terminal key sequences, including Ctrl+symbol keys, modified special keys (Ctrl/Shift/Alt + Arrow/Home/End/F-keys), and cross-WebView compatibility between WebKitGTK and Chromium.

## Objectives

- Handle all Ctrl+symbol control characters (`Ctrl+[`, `Ctrl+]`, `Ctrl+\`, `Ctrl+Space`, etc.)
- Support xterm-style modifier parameter sequences for modified special keys
- Ensure WebKitGTK compatibility (Tauri on Linux)
- Make Ctrl+J blocking configurable via settings
- Add Shift+Tab (back-tab) support

## Technical Requirements

### Functional Requirements

- **FR1:** Ctrl+symbol keys (`@[\]^_ `) produce correct control characters using `charCode & 0x1F`
- **FR2:** WebKitGTK reports `Ctrl+[` as `key="["` + `ctrlKey=true`; must handle both this and Chromium's `key="Escape"` + `ctrlKey=true`
- **FR3:** Shift+Tab produces `ESC [ Z` (back-tab / CSI Z)
- **FR4:** Modified arrow keys produce `ESC [1;{modifier}A/B/C/D` (xterm modifier parameter)
- **FR5:** Modified Home/End produce `ESC [1;{modifier}H/F`
- **FR6:** Modified Delete/Insert/PageUp/PageDown produce `ESC [{number};{modifier}~`
- **FR7:** Modified F1-F4 produce `ESC [1;{modifier}P/Q/R/S`
- **FR8:** Modified F5-F12 produce `ESC [{number};{modifier}~`
- **FR9:** Ctrl+Alt+letter produces `ESC` prefix + control character (e.g., Ctrl+Alt+C = `ESC 0x03`)
- **FR10:** Ctrl+J blocking is configurable via `skk_mode` setting (default: `true` = blocked). Only plain Ctrl+J (no Alt/Shift/Meta) is blocked
- **FR11:** Existing keybinds (Ctrl+Shift+C/V for copy/paste, Ctrl+Shift+Arrow for prompt jump) take priority over new key sequences

### Non-Functional Requirements

- **NFR1 - Performance:** No measurable increase in key-to-PTY latency
- **NFR2 - Compatibility:** Works on both WebKitGTK (Linux) and Chromium-based WebView (Windows/macOS)
- **NFR3 - Backward Compatibility:** All existing key behaviors preserved

## Implementation Approach

### Architecture

The keyboard handler pipeline:

```
KeyboardEvent (browser)
    │
    ├── [capture phase] handleClipboardShortcut()
    │     └── Copy/Paste/Search keybind check → intercept if match
    │
    └── [bubble phase] handleKeyDown()
          ├── Selection clear on Escape
          ├── Keybind checks (copy, paste, prompt jump, search)
          ├── shouldHandleKey() filter
          ├── IME composition check
          ├── Ctrl+J check (NEW: configurable)
          ├── EditContext / IME bypass
          └── keyEventToBytes() → PTY write
                ├── [1] Application Cursor Keys (DECCKM)
                ├── [2] SPECIAL_KEYS exact match
                ├── [3] Ctrl + letter (a-z) → 0x01-0x1A
                ├── [4] Ctrl + symbol (@[\]^_ ) → charCode & 0x1F  ← NEW
                ├── [5] Modified special keys → xterm modifier seqs  ← NEW
                ├── [6] Ctrl+Alt + letter → ESC + control char       ← NEW
                ├── [7] Alt + key → ESC prefix
                └── [8] Regular printable character
```

### Key Changes

#### 1. Ctrl+Symbol Handler (FR1, FR2)

Add after the existing Ctrl+letter handler (step [4]):

```typescript
// Ctrl + symbol (@[\]^_) -> control characters (0x00, 0x1B-0x1F)
// Handles WebKitGTK where Ctrl+[ reports key="[" (not "Escape")
const code = event.key.charCodeAt(0);
if (code >= 0x40 && code <= 0x5f) {
    return new Uint8Array([code & 0x1f]);
}
// Ctrl+Space -> NUL (0x00)
if (event.key === " ") {
    return new Uint8Array([0x00]);
}
```

The SPECIAL_KEYS entry `{ key: "Escape", ctrl: true, sequence: [0x1b] }` is retained for Chromium compatibility (checked before this handler).

#### 2. Shift+Tab (FR3)

Add to SPECIAL_KEYS:

```typescript
{ key: "Tab", shift: true, sequence: [0x1b, 0x5b, 0x5a] }, // Shift+Tab = ESC [ Z
```

#### 3. Modifier Parameter Helper (FR4-FR8)

```typescript
/**
 * Calculate xterm modifier parameter from modifier key states.
 * Returns 0 if no modifiers, otherwise 1 + modifier_bits.
 * Bit layout: shift=1, alt=2, ctrl=4
 * Exported for testability.
 */
export function calcModifierParam(shift: boolean, alt: boolean, ctrl: boolean): number {
    const bits = (shift ? 1 : 0) + (alt ? 2 : 0) + (ctrl ? 4 : 0);
    return bits === 0 ? 0 : bits + 1;
}
```

#### 4. Modified Special Keys (FR4-FR8)

Add as step [5] in `keyEventToBytes`, before the Alt handler:

```typescript
// Modified special keys -> xterm modifier parameter sequences
const mod = calcModifierParam(event.shiftKey, event.altKey, event.ctrlKey);
if (mod > 0) {
    // Arrow keys: ESC [1;{mod}A/B/C/D
    const arrowLetter = ARROW_KEY_LETTERS[event.key]; // { ArrowUp: 0x41, ... }
    if (arrowLetter !== undefined) {
        return encodeModifiedLetterSeq("1", mod, arrowLetter);
    }

    // Home/End: ESC [1;{mod}H/F
    const navLetter = NAV_KEY_LETTERS[event.key]; // { Home: 0x48, End: 0x46 }
    if (navLetter !== undefined) {
        return encodeModifiedLetterSeq("1", mod, navLetter);
    }

    // Delete/Insert/PageUp/PageDown: ESC [{num};{mod}~
    const tildeNum = TILDE_KEY_NUMBERS[event.key]; // { Delete: "3", Insert: "2", ... }
    if (tildeNum !== undefined) {
        return encodeModifiedTildeSeq(tildeNum, mod);
    }

    // F1-F4: ESC [1;{mod}P/Q/R/S
    const fkeyLetter = FKEY_LETTERS[event.key]; // { F1: 0x50, ... }
    if (fkeyLetter !== undefined) {
        return encodeModifiedLetterSeq("1", mod, fkeyLetter);
    }

    // F5-F12: ESC [{num};{mod}~
    const fkeyTildeNum = FKEY_TILDE_NUMBERS[event.key]; // { F5: "15", ... }
    if (fkeyTildeNum !== undefined) {
        return encodeModifiedTildeSeq(fkeyTildeNum, mod);
    }
}
```

Helper encoding functions:

```typescript
// ESC [{prefix};{mod}{letter}  (e.g., ESC [1;5A)
function encodeModifiedLetterSeq(prefix: string, mod: number, letter: number): Uint8Array

// ESC [{num};{mod}~  (e.g., ESC [3;5~)
function encodeModifiedTildeSeq(num: string, mod: number): Uint8Array
```

#### 5. Ctrl+Alt Combinations (FR9)

Modify the Alt handler condition to also handle Ctrl+Alt:

```typescript
// Alt + key (with or without Ctrl) -> ESC prefix
if (event.altKey && event.key.length === 1) {
    if (event.ctrlKey) {
        // Ctrl+Alt+letter -> ESC + control char
        const char = event.key.toLowerCase();
        if (char >= "a" && char <= "z") {
            return new Uint8Array([0x1b, char.charCodeAt(0) - 96]);
        }
    } else {
        // Alt+key -> ESC + key bytes
        ...existing code...
    }
}
```

#### 6. Ctrl+J Setting (FR10)

**Rust settings struct** (`src-tauri/src/commands/config.rs`):

`skk_mode: bool` is a flat field on `AppSettings` (not nested under a separate struct).

```rust
// IME
#[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
pub skk_mode: bool,
```

**TypeScript settings** (`src/settings/types.ts`):

```typescript
// IME
skk_mode: boolean;
```

**Keyboard handler** (`src/terminal-app/handlers/keyboard.ts`):

```typescript
// Skip plain Ctrl+J only when SKK mode is enabled
const settings = SettingsService.getCached();
if (event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey
    && event.key.toLowerCase() === "j"
    && settings?.skk_mode !== false) {
    return;
}
```

### Lookup Table Definitions

```typescript
const ARROW_KEY_LETTERS: Record<string, number> = {
    ArrowUp: 0x41, ArrowDown: 0x42, ArrowRight: 0x43, ArrowLeft: 0x44
};

const NAV_KEY_LETTERS: Record<string, number> = {
    Home: 0x48, End: 0x46
};

const TILDE_KEY_NUMBERS: Record<string, string> = {
    Insert: "2", Delete: "3", PageUp: "5", PageDown: "6"
};

const FKEY_LETTERS: Record<string, number> = {
    F1: 0x50, F2: 0x51, F3: 0x52, F4: 0x53
};

const FKEY_TILDE_NUMBERS: Record<string, string> = {
    F5: "15", F6: "17", F7: "18", F8: "19",
    F9: "20", F10: "21", F11: "23", F12: "24"
};
```

### Priority Resolution

When a key combination matches both an emterm keybind and a PTY sequence, the emterm keybind takes priority. This is already ensured by the handler order in `handleKeyDown()`:

1. Keybind checks (copy, paste, prompt jump, search) - lines 182-222
2. `keyEventToBytes()` - line 267

No change needed for priority resolution.

### Dependencies

**Internal:**
- `src/pty/keyboard.ts` - main key mapping logic
- `src/terminal-app/handlers/keyboard.ts` - app-level handler (Ctrl+J config)
- `src/settings/types.ts` - settings type definition
- `src-tauri/src/settings.rs` - Rust settings struct

**External:**
- None (no new dependencies)

### File Structure

```
src/
├── pty/
│   ├── keyboard.ts              # MODIFIED: Ctrl+symbol, modifier params, Ctrl+Alt
│   └── keyboard.test.ts         # MODIFIED: New test cases
├── terminal-app/
│   └── handlers/
│       ├── keyboard.ts          # MODIFIED: Ctrl+J config check
│       └── keyboard.test.ts     # MODIFIED: Ctrl+J config test
├── settings/
│   └── types.ts                 # MODIFIED: ImeSettings type
src-tauri/
├── src/
│   └── settings.rs              # MODIFIED: ImeSettings struct
├── locales/
│   ├── en.json                  # MODIFIED: ime.skk_mode label
│   └── ja.json                  # MODIFIED: ime.skk_mode label
src/
├── i18n/
│   └── locales/
│       ├── en.json              # MODIFIED: settings UI label
│       └── ja.json              # MODIFIED: settings UI label
```

## Test Scenarios

### Unit Tests

#### Ctrl+Symbol Keys (FR1, FR2)
- [ ] `Ctrl+[` with `key="["` (WebKitGTK) produces `0x1B` (ESC)
- [ ] `Ctrl+[` with `key="Escape"` (Chromium) produces `0x1B` (ESC)
- [ ] `Ctrl+]` produces `0x1D` (GS)
- [ ] `Ctrl+\` produces `0x1C` (FS)
- [ ] `Ctrl+^` produces `0x1E` (RS)
- [ ] `Ctrl+_` produces `0x1F` (US)
- [ ] `Ctrl+@` produces `0x00` (NUL)
- [ ] `Ctrl+Space` produces `0x00` (NUL)

#### Shift+Tab (FR3)
- [ ] `Shift+Tab` produces `ESC [ Z`

#### Modified Arrow Keys (FR4)
- [ ] `Ctrl+ArrowUp` produces `ESC [1;5A`
- [ ] `Ctrl+ArrowRight` produces `ESC [1;5C`
- [ ] `Shift+ArrowUp` produces `ESC [1;2A`
- [ ] `Alt+ArrowUp` produces `ESC [1;3A`
- [ ] `Ctrl+Shift+ArrowRight` produces `ESC [1;6C`

#### Modified Navigation Keys (FR5, FR6)
- [ ] `Ctrl+Home` produces `ESC [1;5H`
- [ ] `Ctrl+End` produces `ESC [1;5F`
- [ ] `Shift+Home` produces `ESC [1;2H`
- [ ] `Ctrl+Delete` produces `ESC [3;5~`
- [ ] `Ctrl+PageUp` produces `ESC [5;5~`
- [ ] `Shift+Insert` produces `ESC [2;2~`

#### Modified Function Keys (FR7, FR8)
- [ ] `Shift+F1` produces `ESC [1;2P`
- [ ] `Ctrl+F5` produces `ESC [15;5~`
- [ ] `Ctrl+F12` produces `ESC [24;5~`

#### Ctrl+Alt Combinations (FR9)
- [ ] `Ctrl+Alt+C` produces `ESC 0x03`
- [ ] `Ctrl+Alt+A` produces `ESC 0x01`

#### Modifier Parameter Helper
- [ ] No modifiers returns 0
- [ ] Shift only returns 2
- [ ] Alt only returns 3
- [ ] Ctrl only returns 5
- [ ] Ctrl+Shift returns 6
- [ ] Ctrl+Alt returns 7
- [ ] Ctrl+Alt+Shift returns 8

#### Ctrl+J Setting (FR10)
- [ ] With `skk_mode: true` (default), Ctrl+J is blocked
- [ ] With `skk_mode: false`, Ctrl+J produces `0x0A`

#### Priority / Non-interference (FR11)
- [ ] Unmodified arrow keys still work in normal mode
- [ ] Unmodified arrow keys still work in application mode (DECCKM)
- [ ] Existing Escape (no modifier) still produces `0x1B`
- [ ] Regular characters are unaffected
- [ ] Ctrl+a-z still produces correct control characters

### Edge Cases
- [ ] `Ctrl+[` in application cursor mode still produces `0x1B` (not affected by DECCKM)
- [ ] `Ctrl+Shift+ArrowUp` is intercepted by prompt jump (not sent to PTY)
- [ ] `Shift+Tab` in application mode produces `ESC [ Z` (not affected by DECCKM)

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] `Ctrl+z Ctrl+[` enters tmux copy-mode on Linux (WebKitGTK)
- [ ] `Ctrl+Left/Right` performs word movement in bash/zsh
- [ ] `Shift+Tab` performs reverse tab completion
- [ ] Ctrl+J is configurable via settings
- [ ] No regression in existing key handling
- [ ] All unit tests pass on both Docker and host environments
