# Implementation Plan: Shift+Enter LF Option

## Overview

Add the `lf` value to `shift_enter_behavior` (native + shared schema + TS)
and restrict the settings-panel select to `alt_enter` / `none` / `lf`,
keeping `kitty_csi_u` as a functional hidden wire value.

## Technology Stack

- **Language**: Rust + TypeScript. No new dependencies (no license
  entries).

## Layer Structure

Unchanged from the shift-enter-behavior feature: native settings layer is
the value SSOT; the key-event layer consults it at the existing rewrite
site; the settings panel mirrors it over the settings JSON wire.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Setting wire contract (extended) | Serialize the behavior choice | Key `shift_enter_behavior`; JSON string values exactly `none` / `alt_enter` / `kitty_csi_u` / `lf`; default `alt_enter`; unknown/null → default | task0001 (Rust serde, both layers), task0002 (TS union) |
| UI option policy | Which values the select offers | Offered set: `alt_enter`, `none`, `lf` in this order; `kitty_csi_u` appended as a fourth option IF AND ONLY IF it is the currently loaded value | task0002 (renderer); task0001 unaffected |

Both tasks implement against these contracts independently.

## Conventions

- Follow the predecessor feature's patterns exactly: the `lf` variant is
  added everywhere the `kitty_csi_u` variant exists (native enum,
  app_settings schema enum, TS union, decision function, tests), with the
  same null-tolerant/default semantics.
- Locale keys live in the existing `settings.terminal.*` namespace.

## Cross-task Design Decisions

### D1: LF uses the existing raw-byte path

The `lf` rewrite emits the single byte 0x0a through the same raw-byte
output path introduced for `kitty_csi_u` (host PTY write or mux PtyInput
frame), making it EncodeTarget-independent by construction. Affected:
task0001.

### D2: Grandfathering is render-time only

Whether `kitty_csi_u` appears in the select is computed from the loaded
value at render time; no extra persisted state. Affected: task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A migration/sentinel test in app_settings breaks when the enum grows | Low | Low | Run the full settings test set; NFR1 regression scenario (TS-4) |
| Select with a current value not in the offered list renders wrongly if grandfathering is missed | Low | Medium | Dedicated section test for the 4-option case (TS-6) |

## Open Questions

- [ ] None
