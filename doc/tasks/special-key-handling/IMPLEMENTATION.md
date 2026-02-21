# Implementation Plan: Comprehensive Special Key Mapping

## Overview

Extend emterm's keyboard handler to support all standard terminal key sequences: Ctrl+symbol control characters with WebKitGTK compatibility, xterm-style modifier parameter sequences for modified special keys, Ctrl+Alt combinations, and configurable Ctrl+J blocking.

## Objectives

- Fix `Ctrl+[` on WebKitGTK (key="[" + ctrlKey, not key="Escape" + ctrlKey)
- Support all Ctrl+symbol control characters (@[\]^_ and Space)
- Add xterm modifier parameter sequences for modified Arrow/Home/End/Delete/PageUp/PageDown/F-keys
- Add Shift+Tab (back-tab)
- Add Ctrl+Alt+letter sequences
- Make Ctrl+J blocking configurable via settings

## Prerequisites

### Development Environment

- Bun (package manager, test runner, bundler)
- Rust toolchain (for settings backend)
- Docker (for test execution)

### Dependencies

- No new external dependencies required

## Architecture Overview

### Technology Stack

- **Language**: TypeScript (frontend keyboard handler), Rust (settings backend)
- **Framework**: Tauri v2
- **Key Libraries**: None added

### Design Approach

The keyboard handler pipeline in `keyEventToBytes` is extended with two new handler stages inserted into the existing cascade:

1. **Ctrl+symbol handler** - After Ctrl+letter (a-z), handle @[\]^_ range using bitwise AND masking, plus Space as NUL
2. **Modified special key handler** - Before Alt handler, calculate xterm modifier parameter and generate appropriate sequences using lookup tables

The Ctrl+Alt handler modifies the existing Alt handler to also support Ctrl+Alt combinations.

The Ctrl+J setting adds a flat boolean field to `AppSettings` following existing patterns.

### Component Interaction

```
keyEventToBytes processing cascade:
  [1] Application Cursor Keys (DECCKM) - existing
  [2] SPECIAL_KEYS exact match          - MODIFIED (add Shift+Tab)
  [3] Ctrl + letter (a-z)               - existing
  [4] Ctrl + symbol (@[\]^_ Space)      - NEW (Phase 1)
  [5] Modified special keys             - NEW (Phase 2)
  [6] Alt / Ctrl+Alt + key              - MODIFIED (Phase 3)
  [7] Regular printable character        - existing
```

## Implementation Phases

### Phase 1: Ctrl+Symbol Keys & WebKitGTK Fix

**Goal**: Fix `Ctrl+[` on WebKitGTK and support all Ctrl+symbol control characters. After this phase, `Ctrl+z Ctrl+[` enters tmux copy-mode on Linux.

**Files to Modify**:
- `src/pty/keyboard.ts` - Add Ctrl+symbol handler after Ctrl+letter handler
- `src/pty/keyboard.test.ts` - Add tests for Ctrl+symbol keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Ctrl+symbol handler | Convert Ctrl+symbol to control char | `event.ctrlKey` true, `event.key` is single char in @[\]^_ range or Space | Returns byte array with `charCode & 0x1F` (or 0x00 for Space) |

**Processing Flow**:
1. Check: event has Ctrl modifier, no Alt modifier, key is single character
2. Check: character code is in range 0x40-0x5F (@[\]^_)
   - Yes -> return byte with `charCode & 0x1F`
3. Check: character is Space (0x20)
   - Yes -> return NUL byte (0x00)
4. Fall through to next handler

**Implementation Steps**:
1. **Add Ctrl+symbol handler** - Insert handler after existing Ctrl+letter block, covering ASCII 0x40-0x5F range via bitwise masking
2. **Add Ctrl+Space handler** - Special case for Space producing NUL
3. **Add unit tests** - Cover both WebKitGTK (`key="["`) and Chromium (`key="Escape"`) key reporting for Ctrl+[, plus Ctrl+], Ctrl+\, Ctrl+^, Ctrl+_, Ctrl+@, Ctrl+Space
4. **Verify existing Escape mapping** - Ensure the SPECIAL_KEYS entry for `key="Escape"` + `ctrl: true` still works for Chromium

**Dependencies**: None

**Testing Approach**:
- Unit: Ctrl+[ with key="[" (WebKitGTK) → 0x1B, Ctrl+[ with key="Escape" (Chromium) → 0x1B, Ctrl+] → 0x1D, Ctrl+\ → 0x1C, Ctrl+^ → 0x1E, Ctrl+_ → 0x1F, Ctrl+@ → 0x00, Ctrl+Space → 0x00
- Unit: Existing Ctrl+a-z still produce correct control characters (regression)
- Manual: `Ctrl+z Ctrl+[` enters tmux copy-mode on Linux (WebKitGTK)

**Acceptance Criteria**:
- [ ] All Ctrl+symbol keys produce correct control characters
- [ ] Both WebKitGTK and Chromium key reporting handled
- [ ] No regression in existing Ctrl+letter handling

**Estimated Effort**: small

---

### Phase 2: Modifier Parameter System & Modified Special Keys

**Goal**: Support xterm-style modifier parameter sequences for all modified special keys (Ctrl/Shift/Alt + Arrow/Home/End/Delete/PageUp/PageDown/F-keys) and Shift+Tab.

**Files to Modify**:
- `src/pty/keyboard.ts` - Add Shift+Tab to SPECIAL_KEYS, add modifier param helper, add lookup tables, add modified key handler
- `src/pty/keyboard.test.ts` - Comprehensive tests for all modified key combinations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Modifier parameter calculator | Compute xterm modifier code from event modifiers | KeyboardEvent with at least one modifier | Returns 0 (no modifiers) or 2-8 (modifier code) |
| Arrow key lookup | Map arrow key names to suffix letters | event.key is "ArrowUp/Down/Left/Right" | Returns letter A/B/C/D or undefined |
| Navigation letter lookup | Map Home/End to suffix letters | event.key is "Home" or "End" | Returns H/F or undefined |
| Tilde key lookup | Map Delete/Insert/PageUp/PageDown to CSI numbers | event.key is one of the tilde-style keys | Returns number string or undefined |
| F-key letter lookup | Map F1-F4 to suffix letters | event.key is F1-F4 | Returns P/Q/R/S or undefined |
| F-key tilde lookup | Map F5-F12 to CSI numbers | event.key is F5-F12 | Returns number string or undefined |
| Modified sequence encoder | Build `ESC [{prefix};{mod}{suffix}` byte array | prefix, modifier code, suffix letter | Returns encoded byte array |
| Modified tilde encoder | Build `ESC [{num};{mod}~` byte array | number string, modifier code | Returns encoded byte array |

**Processing Flow**:
1. Calculate modifier parameter from event modifiers (shift, alt, ctrl)
   - No modifiers -> parameter is 0, skip this handler
2. Look up event.key in arrow key map
   - Found -> encode as `ESC [1;{mod}{letter}`, return
3. Look up in navigation letter map (Home/End)
   - Found -> encode as `ESC [1;{mod}{letter}`, return
4. Look up in tilde key map (Delete/Insert/PageUp/PageDown)
   - Found -> encode as `ESC [{num};{mod}~`, return
5. Look up in F-key letter map (F1-F4)
   - Found -> encode as `ESC [1;{mod}{letter}`, return
6. Look up in F-key tilde map (F5-F12)
   - Found -> encode as `ESC [{num};{mod}~`, return
7. Fall through to next handler

**Implementation Steps**:
1. **Add Shift+Tab entry** - Add to SPECIAL_KEYS array with shift modifier and ESC [ Z sequence
2. **Create modifier parameter calculator** - Pure function computing `1 + shift_bit + alt_bit*2 + ctrl_bit*4`
3. **Define lookup tables** - Five static maps for arrow keys, navigation keys, tilde keys, F1-F4, F5-F12
4. **Create sequence encoding helpers** - Two helpers for letter-style and tilde-style modified sequences
5. **Add modified key handler** - Insert before Alt handler in the cascade; use calculator + lookups + encoders
6. **Add comprehensive tests** - Modifier calculator (all 7 non-zero combinations), each key category with Ctrl/Shift/Alt variants, Shift+Tab

**Dependencies**: None (independent of Phase 1)

**Testing Approach**:
- Unit: Modifier calculator returns correct codes for all modifier combinations
- Unit: Shift+Tab → ESC [ Z
- Unit: Ctrl+ArrowUp → ESC [1;5A, Shift+ArrowRight → ESC [1;2C, Alt+ArrowDown → ESC [1;3B
- Unit: Ctrl+Home → ESC [1;5H, Shift+End → ESC [1;2F
- Unit: Ctrl+Delete → ESC [3;5~, Ctrl+PageUp → ESC [5;5~
- Unit: Shift+F1 → ESC [1;2P, Ctrl+F5 → ESC [15;5~, Ctrl+F12 → ESC [24;5~
- Unit: Unmodified Arrow/Home/End/Delete/F-keys still work (regression)
- Unit: Application cursor mode (DECCKM) still works for unmodified arrows (regression)
- Manual: Ctrl+Left/Right performs word movement in bash/zsh
- Manual: Shift+Tab performs reverse tab completion

**Acceptance Criteria**:
- [ ] All modified special keys produce correct xterm sequences
- [ ] Shift+Tab produces back-tab sequence
- [ ] No regression in unmodified key handling
- [ ] Application cursor mode unaffected

**Estimated Effort**: medium

---

### Phase 3: Ctrl+Alt Combinations & Ctrl+J Setting

**Goal**: Support Ctrl+Alt+letter sequences and make Ctrl+J blocking configurable.

**Files to Modify**:
- `src/pty/keyboard.ts` - Modify Alt handler to support Ctrl+Alt
- `src/pty/keyboard.test.ts` - Ctrl+Alt tests
- `src-tauri/src/commands/config.rs` - Add `skk_mode` field to AppSettings
- `src/settings/types.ts` - Add `skk_mode` to AppSettings interface
- `src/terminal-app/handlers/keyboard.ts` - Conditional Ctrl+J blocking
- `src/terminal-app/handlers/keyboard.test.ts` - Ctrl+J config tests
- `src-tauri/locales/en.json` - i18n label for skk_mode
- `src-tauri/locales/ja.json` - i18n label for skk_mode
- `src/i18n/locales/en.json` - Frontend i18n label
- `src/i18n/locales/ja.json` - Frontend i18n label
- `src/settings/settings-sections.ts` - Settings UI for skk_mode toggle

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Ctrl+Alt handler | Produce ESC + control char for Ctrl+Alt+letter | Both ctrlKey and altKey true, key is a-z | Returns ESC prefix followed by control character byte |
| skk_mode setting (Rust) | Persist SKK mode preference | AppSettings loaded from config | `skk_mode` field available with default `true` |
| skk_mode setting (TS) | Mirror Rust setting in frontend | Settings fetched from backend | `skk_mode` boolean accessible |
| Ctrl+J conditional block | Block Ctrl+J only when skk_mode enabled | Settings cached and accessible | Ctrl+J blocked if skk_mode true, sent to PTY if false |

**Processing Flow (Ctrl+Alt)**:
1. Check: event has Alt modifier, key is single character
2. Check: event also has Ctrl modifier
   - Yes -> produce ESC byte + control character for the letter
3. No Ctrl -> existing Alt behavior (ESC + key bytes)

**Processing Flow (Ctrl+J)**:
1. Read cached settings
2. Check: skk_mode is not explicitly false
   - Yes -> block Ctrl+J (existing behavior)
   - No -> allow Ctrl+J through to keyEventToBytes (produces 0x0A)

**Implementation Steps**:
1. **Modify Alt handler** - Extend condition to accept Ctrl+Alt, add branch for Ctrl+Alt+letter producing ESC + control char
2. **Add skk_mode to Rust settings** - Flat boolean field in AppSettings with `default_true` and `deserialize_null_true`
3. **Add skk_mode to TS settings** - Boolean field in AppSettings interface
4. **Modify Ctrl+J check** - Read skk_mode from cached settings, conditionally block
5. **Add i18n labels** - Both Rust backend and TS frontend locale files
6. **Add settings UI toggle** - Boolean toggle in Terminal section of settings panel
7. **Add tests** - Ctrl+Alt combinations, Ctrl+J with skk_mode true/false

**Dependencies**: Phase 1 (Ctrl+symbol handler should exist before Ctrl+Alt modifies the Alt handler)

**Testing Approach**:
- Unit: Ctrl+Alt+C → ESC 0x03, Ctrl+Alt+A → ESC 0x01
- Unit: Ctrl+J blocked when skk_mode true (or default)
- Unit: Ctrl+J produces 0x0A when skk_mode false
- Unit: Alt+letter still works (regression)
- Integration: Settings round-trip (save skk_mode → reload → verify)
- Manual: fzf accept works with skk_mode disabled

**Acceptance Criteria**:
- [ ] Ctrl+Alt+letter produces correct sequences
- [ ] Ctrl+J is configurable via settings
- [ ] No regression in Alt+letter handling
- [ ] Settings UI shows skk_mode toggle

**Estimated Effort**: medium

---

## Complete File Structure

```
src/
├── pty/
│   ├── keyboard.ts              # MODIFIED: Ctrl+symbol, modifier params, Ctrl+Alt
│   └── keyboard.test.ts         # MODIFIED: Comprehensive new test cases
├── terminal-app/
│   └── handlers/
│       ├── keyboard.ts          # MODIFIED: Conditional Ctrl+J
│       └── keyboard.test.ts     # MODIFIED: Ctrl+J config tests
├── settings/
│   ├── types.ts                 # MODIFIED: Add skk_mode field
│   └── settings-sections.ts     # MODIFIED: Add skk_mode toggle UI
├── i18n/
│   └── locales/
│       ├── en.json              # MODIFIED: skk_mode label
│       └── ja.json              # MODIFIED: skk_mode label
src-tauri/
├── src/
│   └── commands/
│       └── config.rs            # MODIFIED: Add skk_mode to AppSettings
├── locales/
│   ├── en.json                  # MODIFIED: validation message
│   └── ja.json                  # MODIFIED: validation message
```

## Testing Strategy

- **Unit**: Core keyboard mapping logic at 90%+ coverage for new code. All key combinations tested with both modifier states.
- **Integration**: Settings persistence round-trip for skk_mode
- **Manual**: Verify in running app with tmux (Ctrl+z Ctrl+[), bash (Ctrl+Left/Right, Shift+Tab), fzf (Ctrl+J)

## Dependencies

No new external packages required.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| WebKitGTK reports other Ctrl+symbol keys differently than expected | Medium | Low | Test each key on actual WebKitGTK; the bitwise masking approach handles the raw character regardless of browser normalization |
| Modified key sequences conflict with emterm keybinds | Low | Medium | Emterm keybinds checked before keyEventToBytes in the handler cascade |
| DECCKM interaction with modifier sequences | Low | Low | DECCKM only applies to unmodified arrows (already guarded by modifier checks) |

## Open Questions

- None (all requirements clarified during specification)

## Success Metrics

- [ ] `Ctrl+z Ctrl+[` enters tmux copy-mode on Linux (WebKitGTK)
- [ ] `Ctrl+Left/Right` performs word movement in bash/zsh
- [ ] `Shift+Tab` performs reverse tab completion
- [ ] `Ctrl+J` configurable via settings
- [ ] All existing key behaviors preserved (no regression)
- [ ] All unit tests pass in Docker environment
