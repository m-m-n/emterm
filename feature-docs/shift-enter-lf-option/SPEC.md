# Feature: Shift+Enter LF Option (and hidden kitty_csi_u)

## Overview

Add an `lf` value to the `shift_enter_behavior` setting that makes a bare
Shift+Enter send the single byte LF (0x0a), and remove `kitty_csi_u` from
the settings-panel select while keeping its implementation and behavior
intact as a hidden wire value.

## Objectives

- Provide a Shift+Enter mode with no stray characters in non-CSI-u
  applications that still inserts a newline in Claude Code.
- Hide the unconditional CSI u option from the UI until a real kitty
  keyboard protocol implementation exists.

## User Stories

### US1: Choose LF for Shift+Enter
As a user, I want Shift+Enter to send LF, so that Claude Code inserts a
newline while the shell behaves as if I pressed Enter.

**Acceptance Criteria:**
- [ ] Selecting the LF option makes a bare Shift+Enter send exactly 0x0a.
- [ ] The choice persists through the settings panel.

### US2: kitty_csi_u kept but not offered
As a user with `kitty_csi_u` in settings.json, I want its behavior
unchanged, so that my existing setup keeps working even though the option
is no longer offered.

**Acceptance Criteria:**
- [ ] `kitty_csi_u` still parses and still sends `\x1b[13;2u`.
- [ ] The select shows it only while it is the current value.

## Technical Requirements

### Functional Requirements

- **FR1:** Add the variant `lf` (wire value `"lf"`) to the
  `shift_enter_behavior` enum in the native settings, the shared
  app_settings schema, and the TypeScript union. With `lf`, a bare
  Shift+Enter (no Ctrl/Alt) sends the single byte 0x0a through the same
  output path as the existing raw-byte mode, identical for
  `EncodeTarget::HostPty` and `EncodeTarget::PosixPty`.
- **FR2:** The settings-panel select offers exactly `alt_enter`, `none`,
  `lf` in that order.
- **FR3:** Grandfathering: when the loaded value is `kitty_csi_u`, the
  select additionally shows the `kitty_csi_u` option (existing label) so
  the current state is visible and re-selectable; once another value is
  saved, it is no longer offered.
- **FR4:** `kitty_csi_u` remains a fully functional hidden wire value: its
  enum variant, deserialization, persistence, and byte behavior
  (`\x1b[13;2u`) are unchanged.
- **FR5:** Add ja/en locale strings for the `lf` option: ja label
  「改行 (LF) として送信」, en label "Send as newline (LF)", with a
  description conveying "newline in Claude Code; same as Enter in the
  shell", phrased to match the surrounding locale style.

### Non-Functional Requirements

- **NFR1 - Behavior preservation:** The default stays `alt_enter`; the
  legacy-boolean migration and null-precedence rules are unchanged; the
  byte behavior of `none` / `alt_enter` / `kitty_csi_u` is unchanged;
  modifier isolation (Ctrl/Alt combinations untouched) applies to `lf`
  exactly as to the other values.

## Implementation Approach

### Architecture

Extends the shift-enter-behavior feature in place:

```
window_host.rs shift_enter_rewrite
  ├─ None      → plain Enter encoding
  ├─ AltEnter  → Alt+Enter encoding
  ├─ KittyCsiU → raw bytes \x1b[13;2u   (unchanged)
  └─ Lf        → raw byte 0x0a          (new, same raw-byte path)
```

UI: the option list is computed from the current value (static three
options, plus `kitty_csi_u` when it is the current value).

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/settings.rs` — native enum + parsing.
- `crates/app_settings/src/types.rs`, `crates/app_settings/src/settings.rs`
  — shared schema enum (serde wire values).
- `src-tauri/src/window_host.rs` — rewrite decision + raw-byte constant.
- `src-tauri/web-shared/settings/types.ts` — TS union.
- `src-tauri/web-shared/settings/sections/terminal-behavior-section.ts`
  — select options logic.
- `src-tauri/web-shared/settings/sections/terminal-behavior-section.test.ts`
  — section tests.
- `src-tauri/web-shared/i18n/locales/{ja,en}.json` — labels.

**External Dependencies:** none added.

### File Structure

```
src-tauri/src/settings.rs
src-tauri/src/window_host.rs
src-tauri/src/settings_store.rs               # round-trip coverage for lf
src-tauri/src/settings_window/commands.rs     # boundary test for lf
crates/app_settings/src/types.rs
crates/app_settings/src/settings.rs
src-tauri/web-shared/settings/types.ts
src-tauri/web-shared/settings/sections/terminal-behavior-section.ts
src-tauri/web-shared/settings/sections/terminal-behavior-section.test.ts
src-tauri/web-shared/i18n/locales/ja.json
src-tauri/web-shared/i18n/locales/en.json
```

## Test Scenarios

### Unit Tests
- [ ] TS-1: `lf` rewrite — bare Shift+Enter yields exactly the byte 0x0a
      for both `EncodeTarget::HostPty` and `EncodeTarget::PosixPty`;
      Ctrl/Alt combinations are untouched under `lf`.
- [ ] TS-2: Deserialization — `"lf"` parses in the native settings and the
      app_settings schema; unknown values still fall back to the default.
- [ ] TS-3: Persistence — `lf` survives the settings-store round-trip and
      the settings-window load/save boundary.
- [ ] TS-4: Regression — `none` / `alt_enter` / `kitty_csi_u` byte
      behavior and the migration/null-precedence tests are unchanged and
      still pass.

### Integration Tests
- [ ] TS-5: `bun run typecheck` passes with the extended union.
- [ ] TS-6: `bun test` passes; section tests cover: three options in FR2
      order when the current value is not `kitty_csi_u`; four options
      (including `kitty_csi_u`) when it is; selecting `lf` saves `"lf"`.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual scenario (VERIFICATION): real-app check of `lf` in Claude
      Code and a shell.

### Edge Cases
- [ ] Edge 1: current value `kitty_csi_u` → switch to `lf` → the kitty
      option disappears from subsequent renders.
- [ ] Edge 2: settings.json hand-edited to `"lf"` loads correctly in the
      native terminal without the panel.

## Security Considerations

- **Input Validation:** unchanged whitelist parsing; unknown strings
  resolve to the default.

## Error Handling

No new error paths.

## Success Criteria

- [ ] All functional requirements are implemented and tested.
- [ ] All test scenarios pass.
- [ ] Behavior of the three existing values is byte-identical to before.

## Open Questions

- None.

## References

- Requirements: feature-docs/shift-enter-lf-option/REQUIREMENTS.md
- Predecessor feature: feature-docs/shift-enter-behavior/SPEC.md
- Investigation: tmp/discussion-shift-enter-kitty-protocol.md
