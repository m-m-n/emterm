# Feature: Shift+Enter as Alt+Enter

## 1. Overview

Add a setting `shift_enter_as_alt_enter` that remaps Shift+Enter to send the same escape sequence as Alt+Enter (ESC + CR: `0x1b 0x0d`). Default: ON. This enables multiline input in applications that interpret Alt+Enter as a newline (e.g., web-based AI interfaces).

## 2. Functional Requirements

### FR1: Setting field `shift_enter_as_alt_enter`

- Type: `bool`
- Default: `true`
- Persisted in `settings.json`
- Rust: `serde(default = "default_true", deserialize_with = "deserialize_null_true")`
- TypeScript: `shift_enter_as_alt_enter: boolean` in `AppSettings` interface

### FR2: Shift+Enter key remapping

When `shift_enter_as_alt_enter` is `true`:
- Shift+Enter (without Ctrl or Alt) sends `[0x1b, 0x0d]` (ESC + CR)

When `shift_enter_as_alt_enter` is `false`:
- Shift+Enter sends `[0x0d]` (CR)

### FR3: Alt+Enter mapping

- Alt+Enter always sends `[0x1b, 0x0d]` (ESC + CR) regardless of the setting

### FR4: Modifier exclusion

- The remapping does NOT apply when Ctrl is held (Ctrl+Shift+Enter is unaffected)
- The remapping does NOT apply when Alt is held (Alt+Shift+Enter follows standard modified key handling)
- Plain Enter is unaffected (`[0x0d]`)

### FR5: Additional special key mappings (shipped alongside)

- Shift+Backspace: sends `[0x7f]` (DEL, same as plain Backspace)
- Shift+Escape: sends `[0x1b]` (ESC, same as plain Escape)

### FR6: Settings UI toggle

- Toggle switch in Terminal Behavior section, placed after "Middle Click Paste"
- i18n labels:
  - en: "Shift+Enter as Alt+Enter" / "Make Shift+Enter send the same escape sequence as Alt+Enter for multiline input"
  - ja: "Shift+EnterをAlt+Enterとして送信" / "Shift+EnterでAlt+Enterと同じエスケープシーケンスを送信し、マルチライン入力を可能にします"

### FR7: Default-true cache handling

- When `SettingsService.getCached()` returns `null` (cache not yet loaded), the setting is treated as `true`
- Pattern: `cachedSettings?.shift_enter_as_alt_enter !== false`

## 3. Non-Functional Requirements

### NFR1 - Backward Compatibility

- `keyEventToBytes` accepts both `CursorKeysMode` (string) and `KeyboardOptions` (object) as the second parameter
- Existing callers passing `CursorKeysMode` continue to work without changes
- When called with `CursorKeysMode`, `shiftEnterAsAltEnter` defaults to `undefined` (falsy, remapping inactive)

## 4. Architecture

### Data Flow

```
User presses Shift+Enter
  -> KeyboardHandler.handleKeyDown()
  -> reads cachedSettings?.shift_enter_as_alt_enter
  -> calls keyEventToBytes(event, { cursorKeysMode, shiftEnterAsAltEnter })
  -> if shiftEnterAsAltEnter && shiftKey && !ctrlKey && !altKey
       -> returns [0x1b, 0x0d]
     else
       -> falls through to SPECIAL_KEYS table ([0x0d])
  -> PtyClient.write(bytes)
  -> PTY
```

### File Structure

```
src-tauri/src/commands/config.rs   # AppSettings struct: shift_enter_as_alt_enter field
src/settings/types.ts              # AppSettings interface: shift_enter_as_alt_enter field
src/pty/keyboard.ts                # KeyboardOptions interface, keyEventToBytes logic
src/pty/keyboard.test.ts           # Tests for the remapping behavior
src/terminal-app/handlers/keyboard.ts  # KeyboardHandler: reads setting, passes to keyEventToBytes
src/settings/settings-sections.ts  # renderToggle for the setting
src/i18n/locales/en.json           # English labels
src/i18n/locales/ja.json           # Japanese labels
```

## 5. Detailed Design

### 5.1 KeyboardOptions Interface

```typescript
export interface KeyboardOptions {
  cursorKeysMode?: CursorKeysMode;
  shiftEnterAsAltEnter?: boolean;
}
```

### 5.2 keyEventToBytes Signature

```typescript
export function keyEventToBytes(
  event: KeyboardEvent,
  cursorKeysModeOrOptions?: CursorKeysMode | KeyboardOptions,
): Uint8Array | null
```

Argument normalization:
- If `cursorKeysModeOrOptions` is an object: use as `KeyboardOptions`
- If it is a string or `undefined`: wrap as `{ cursorKeysMode: value ?? "normal" }`

### 5.3 Remapping Priority

The Shift+Enter remapping check runs **before** the SPECIAL_KEYS table lookup and DECCKM handling:

1. Shift+Enter remapping (if option enabled)
2. DECCKM application cursor keys
3. SPECIAL_KEYS table
4. Ctrl+letter
5. Modified special keys
6. Ctrl+Alt combinations
7. Alt+key prefix
8. Regular printable characters

### 5.4 SPECIAL_KEYS Table Entries

| Key | Shift | Alt | Ctrl | Sequence | Notes |
|-----|-------|-----|------|----------|-------|
| Enter | - | - | - | `[0x0d]` | CR |
| Enter | yes | - | - | `[0x0d]` | CR (fallback when setting OFF) |
| Enter | - | yes | - | `[0x1b, 0x0d]` | ESC + CR |
| Backspace | yes | - | - | `[0x7f]` | DEL (same as plain Backspace) |
| Escape | yes | - | - | `[0x1b]` | ESC (same as plain Escape) |

### 5.5 Caller Integration

```typescript
// In KeyboardHandler.handleKeyDown()
const bytes = keyEventToBytes(event, {
  cursorKeysMode: state.getModes().cursorKeys,
  shiftEnterAsAltEnter: cachedSettings?.shift_enter_as_alt_enter !== false,
});
```

### 5.6 Rust Setting Definition

```rust
#[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
pub shift_enter_as_alt_enter: bool,
```

Default impl: `shift_enter_as_alt_enter: default_true()`

## 6. Test Scenarios

### Unit Tests (keyboard.test.ts)

- [ ] Shift+Enter (setting OFF / no option) -> CR `[0x0d]`
- [ ] Alt+Enter -> ESC + CR `[0x1b, 0x0d]`
- [ ] Shift+Enter with `shiftEnterAsAltEnter: true` -> ESC + CR `[0x1b, 0x0d]`
- [ ] Shift+Enter with `shiftEnterAsAltEnter: false` -> CR `[0x0d]`
- [ ] Ctrl+Shift+Enter with `shiftEnterAsAltEnter: true` -> does NOT produce ESC + CR
- [ ] Plain Enter with `shiftEnterAsAltEnter: true` -> CR `[0x0d]`
- [ ] `KeyboardOptions` object with `cursorKeysMode` works (backward compatibility)
- [ ] Shift+Backspace -> DEL `[0x7f]`
- [ ] Shift+Escape -> ESC `[0x1b]`

### Rust Tests (config.rs)

- [ ] Default settings: `shift_enter_as_alt_enter` is `true`
- [ ] Round-trip: serialize with `shift_enter_as_alt_enter: false`, deserialize, assert `false`
