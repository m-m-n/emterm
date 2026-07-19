# Feature: Shift+Enter Behavior Setting

## Overview

Replace the boolean setting `shift_enter_as_alt_enter` with a three-value
enum setting `shift_enter_behavior` that selects what byte sequence a bare
Shift+Enter press sends to the PTY: pass-through, Alt+Enter rewrite, or an
unconditional kitty CSI u sequence.

## Objectives

- Let the user choose among three Shift+Enter behaviors from the settings
  panel.
- Preserve current default behavior (`alt_enter`).
- Migrate existing `shift_enter_as_alt_enter` values transparently.

## User Stories

### US1: Choose Shift+Enter behavior
As a user, I want to select how Shift+Enter is delivered to the shell, so
that multi-line input works with the applications I use.

**Acceptance Criteria:**
- [ ] The settings panel offers `none` / `alt_enter` / `kitty_csi_u`.
- [ ] Each value produces the specified byte sequence.

## Technical Requirements

### Functional Requirements

- **FR1:** Add setting `shift_enter_behavior` as a Rust enum with serde
  values `none` / `alt_enter` / `kitty_csi_u`; default `alt_enter`. Remove
  the boolean field `shift_enter_as_alt_enter` from the settings struct.
- **FR2:** With `none`, a bare Shift+Enter (no Ctrl/Alt) is not rewritten:
  the PTY receives the same bytes as a plain Enter (`\r`).
- **FR3:** With `alt_enter`, a bare Shift+Enter is delivered as Alt+Enter
  (identical to the current `shift_enter_as_alt_enter: true` behavior).
- **FR4:** With `kitty_csi_u`, a bare Shift+Enter sends the raw bytes
  `\x1b[13;2u` (7 bytes) without any protocol negotiation. The same bytes
  are sent regardless of `EncodeTarget` (`HostPty` and `PosixPty` / mux).
- **FR5:** Settings migration: when `shift_enter_behavior` is absent from
  settings.json and `shift_enter_as_alt_enter` is present, map `true` →
  `alt_enter` and `false` → `none`. When the new key is present it wins.
  Absent both → default `alt_enter`. `null` for the new key follows the
  existing `deserialize_null` default pattern.
- **FR6:** Settings panel: replace the existing toggle in the Terminal
  Behavior section with a three-option select bound to
  `shift_enter_behavior`; add ja / en locale strings for the label,
  description, and the three option labels.
- **FR7:** Mirror the setting in the TypeScript `AppSettings` interface as
  the string union `"none" | "alt_enter" | "kitty_csi_u"`.

### Non-Functional Requirements

- **NFR1 - Behavior isolation:** Enter presses with Ctrl or Alt modifiers
  (including Ctrl+Shift+Enter) are not affected by this setting. Key
  handling that never reaches the PTY (e.g. the search bar's Shift+Enter
  navigation) is unchanged.

## Implementation Approach

### Architecture

The rewrite decision lives where the current boolean is consulted:
`window_host.rs` key-event handling, before `winit_key_to_bytes`. For
`kitty_csi_u` the raw bytes are written directly (bypassing
`winit_key_to_bytes`, which cannot express CSI u).

```
winit KeyEvent (Enter, mods=Shift only)
  └─ match settings.shift_enter_behavior
       ├─ None      → mods.shift=false → winit_key_to_bytes → "\r"
       ├─ AltEnter  → mods.shift=false, mods.alt=true → winit_key_to_bytes
       └─ KittyCsiU → write b"\x1b[13;2u" directly (same path as the
                       encoded bytes: tab.write / mux PtyInput frame)
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/settings.rs`: settings struct, defaults, partial-merge,
  validation.
- `src-tauri/src/window_host.rs`: key-event rewrite site (currently
  `shift_enter_as_alt_enter` branch).
- `src-tauri/src/settings_store.rs`: persistence tests referencing the old
  field.
- `src-tauri/web-shared/settings/types.ts`: `AppSettings` mirror.
- `src-tauri/web-shared/settings/sections/terminal-behavior-section.ts`:
  section renderer.
- `src-tauri/web-shared/i18n/locales/{ja,en}.json`: labels.

**External Dependencies:** none added.

### File Structure

```
src-tauri/src/settings.rs                     # enum ShiftEnterBehavior, field swap,
                                              # legacy-key migration
src-tauri/src/window_host.rs                  # 3-way branch at the rewrite site
src-tauri/src/settings_store.rs               # update persistence tests
src-tauri/web-shared/settings/types.ts        # union type
src-tauri/web-shared/settings/sections/terminal-behavior-section.ts  # select UI
src-tauri/web-shared/i18n/locales/ja.json     # labels (replace shiftEnterAsAltEnter keys)
src-tauri/web-shared/i18n/locales/en.json
```

## Test Scenarios

### Unit Tests
- [ ] TS-1: Rewrite decision — `none`: bare Shift+Enter yields the plain
      Enter encoding (`\r`).
- [ ] TS-2: Rewrite decision — `alt_enter`: bare Shift+Enter yields the
      Alt+Enter encoding.
- [ ] TS-3: Rewrite decision — `kitty_csi_u`: bare Shift+Enter yields
      exactly `\x1b[13;2u`, for both `EncodeTarget::HostPty` and
      `EncodeTarget::PosixPty`.
- [ ] TS-4: Modifier isolation — Ctrl+Enter, Alt+Enter, Ctrl+Shift+Enter
      are not rewritten under any of the three values.
- [ ] TS-5: Deserialization — new key present (each of the three values,
      plus `null`), old key only (`true` → `alt_enter`, `false` → `none`),
      both keys present (new wins), neither (default `alt_enter`).
- [ ] TS-6: settings_store round-trip persists `shift_enter_behavior`.

### Integration Tests
- [ ] TS-7: `bun run typecheck` passes with the union type and select UI.
- [ ] TS-8: `bun test` passes (settings section rendering suite).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual scenario (VERIFICATION): in a running eMterm, each of the
      three values produces the expected behavior in an application that
      distinguishes them.

### Edge Cases
- [ ] Edge 1: IME composition Enter is consumed by the IME before the key
      handler; unaffected.
- [ ] Edge 2: Search-bar-open state — Shift+Enter navigates matches and is
      handled before PTY forwarding; unaffected.
- [ ] Edge 3: mux session (PosixPty target) — `kitty_csi_u` bytes arrive
      at the remote PTY unmodified.

## Security Considerations

- **Input Validation:** unknown string values for `shift_enter_behavior`
  in settings.json fall back to the default via the existing settings
  validation/default pattern.

## Error Handling

No new error paths. Invalid settings values resolve to the default.

## Success Criteria

- [ ] All functional requirements are implemented and tested.
- [ ] All test scenarios pass.
- [ ] Default behavior is byte-identical to the current
      `shift_enter_as_alt_enter: true`.

## Open Questions

- None.

## References

- Requirements: feature-docs/shift-enter-behavior/REQUIREMENTS.md
- Discussion report: tmp/discussion-shift-enter-kitty-protocol.md
