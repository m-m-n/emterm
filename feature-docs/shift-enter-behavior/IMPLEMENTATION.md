# Implementation Plan: Shift+Enter Behavior Setting

## Overview

Replace the boolean setting `shift_enter_as_alt_enter` with the three-value
enum setting `shift_enter_behavior` (Rust settings + key rewrite site, and
the settings-panel WebView UI).

## Technology Stack

- **Language**: Rust (native terminal stack) + TypeScript (settings panel
  WebView). No new dependencies (no license entries).

## Layer Structure

Existing layers unchanged:

- Rust settings layer (`settings.rs` struct + partial-merge + persistence)
  is the SSOT for the setting value.
- The key-event layer (`window_host.rs`) consults the setting at the
  existing Shift+Enter rewrite site.
- The settings-panel WebView mirrors the setting over the established
  settings JSON wire (Settings Pattern: Rust struct ⇄ `AppSettings`
  interface).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Setting wire contract | Serialize the behavior choice in settings.json | Key `shift_enter_behavior`; JSON string values exactly `none` / `alt_enter` / `kitty_csi_u`; default `alt_enter`; unknown/null values resolve to the default | task0001 (Rust serde), task0002 (TS union + select values) |

Both tasks implement against this contract independently; neither task
reads the other's files.

## Conventions

- Follow the project Settings Pattern: Rust `serde(default = ...)` +
  null-tolerant deserialization; TS `AppSettings` mirrors the Rust struct
  field-for-field; section renderers own the UI controls; locale strings
  live in `web-shared/i18n/locales/{ja,en}.json`.
- The legacy key `shift_enter_as_alt_enter` disappears from the saved
  settings shape; it is read only as a migration input (task0001).

## Cross-task Design Decisions

### D1: Rewrite stays at the existing key-event site

The three-way branch replaces the current boolean branch in
`window_host.rs`. For `kitty_csi_u` the raw 7-byte sequence is written to
the same output path as encoder-produced bytes (host PTY write or mux
PtyInput frame), bypassing the key encoder, which cannot express CSI u.
Affected: task0001 only (recorded here because it fixes the byte-level
behavior the UI describes to the user).

### D2: Select option order

The settings-panel select lists options in the order `alt_enter`
(default), `none`, `kitty_csi_u`. Affected: task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `kitty_csi_u` produces garbage input in applications that do not parse CSI u | Certain for such apps | Low (user-selected opt-in) | Option description text states this; default remains `alt_enter` |
| Legacy-key migration misread drops a user's `false` choice | Low | Low | Dedicated deserialization tests (old-key-only, both-keys, neither) |

## Open Questions

- [ ] None
