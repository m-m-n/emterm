# Feature: Cursor Settings Fix (style / blink / color)

## Overview

Three cursor rendering bugs: (1) the `cursor_style` setting never changes the
rendered cursor shape, (2) `cursor_blink: false` is resurrected by tab
switches, and (3) the cursor is painted with the SGR pen (text foreground)
color instead of the color scheme's cursor color. This feature makes the
settings and the active color scheme the authoritative defaults for cursor
shape, blink, and color, while keeping escape-sequence overrides
(DECSCUSR / OSC 22 / OSC 12) authoritative until reset.

## Objectives

- Cursor shape follows the `cursor_style` setting (block / underline / bar).
- `cursor_blink: false` survives tab switches, DECSC/DECRC, and terminal reset.
- Cursor color comes from the active color scheme's cursor color, not the pen fg.
- Escape sequences override settings; resets return to the settings-derived defaults.

## User Stories

### US1: Settings-driven cursor appearance
As a user, I want the cursor style / blink / color I configure (or that my
color scheme defines) to actually show on screen, so that the terminal matches
my configuration.

**Acceptance Criteria:**
- [ ] Changing `cursor_style` in the settings panel changes the cursor shape in all existing tabs immediately.
- [ ] With `cursor_blink: false`, the cursor never blinks, including after switching tabs and back.
- [ ] The cursor is painted with the color scheme's cursor color.

### US2: Application escape-sequence override
As a user running vim or other TUI apps, I want DECSCUSR / OSC 22 / OSC 12 to
take precedence over my settings while active, so that apps can shape the
cursor, and I want my configured defaults back when the app resets them.

**Acceptance Criteria:**
- [ ] DECSCUSR shape/blink and OSC 22 style requests win over settings.
- [ ] DECSCUSR default (`CSI SP q` / `CSI 0 SP q` — the space intermediate is part of DECSCUSR; plain `CSI q` is DECLL and stays out of scope), OSC 112, and terminal reset restore the settings-derived shape/blink and the scheme cursor color.

## Technical Requirements

### Functional Requirements

- **FR1:** The rendered cursor shape reflects the effective cursor style. The
  settings value (`cursor_style`: block / underline / bar) is the default;
  it must reach the renderer's cursor overlay. (Today
  `TerminalCore::set_cursor_style` has no callers, so the renderer's
  `core.get_cursor_style()` always reads the initial value = block.)
- **FR2:** The effective blink state defaults to the `cursor_blink` setting
  and is NOT clobbered by tab switches, DECSC/DECRC save/restore, or terminal
  reset. (Today `blink` lives in `CursorState`, which save/restore and reset
  paths overwrite.) When blink state is reset by a control sequence, it
  returns to the settings value, not a hard-coded `true`.
- **FR3:** The cursor overlay is painted with the active color scheme's cursor
  color (`theme.cursor_fg`) by default. The SGR pen color
  (`core.get_cursor_fg()`) must not be used as the cursor color.
  (Today `render/mod.rs` uses `core.get_cursor_fg()` with `theme.fg` fallback.)
- **FR4:** Escape-sequence precedence: DECSCUSR (shape + blink variants) and
  OSC 22 override the settings-derived style/blink; OSC 12 overrides the
  scheme cursor color. DECSCUSR default, OSC 112, and terminal reset restore
  the settings/scheme-derived defaults.
- **FR5:** A settings change (style / blink) and a color-scheme change apply
  immediately to all existing tabs, and newly spawned tabs inherit the current
  values. Applying settings must not wipe an escape-sequence override that an
  app in some tab has actively set — an app-set DECSCUSR/OSC state persists
  until that app resets it.

### Non-Functional Requirements

- **NFR1 - Performance:** No measurable regression on the per-frame render
  path (cursor overlay is drawn every frame; avoid added locking or
  allocation there).
- **NFR2 - Compatibility:** The CLI-only build
  (`cargo check --no-default-features`) keeps compiling; `crates/term_core`
  changes stay GUI-agnostic.

## Implementation Approach

### Current defect map (investigated)

| Bug | Site | Defect |
|-----|------|--------|
| Style ignored | `src-tauri/src/render/mod.rs` (cursor overlay, ~line 950) reads `core.get_cursor_style()` | `TerminalCore::set_cursor_style` (`crates/term_core/src/terminal_cursor.rs`) has zero callers: neither settings seeding nor DECSCUSR/OSC 22 dispatch writes it. Settings only reach `theme.cursor_style` (`Theme::from_settings`), which the overlay never reads. |
| Blink revived | `CursorState.blink` (`crates/term_core`) | Blink is part of `CursorState`, so `save_cursor`/`restore_cursor` (DECSC/DECRC, alt-screen enter/leave) and `CursorState::new()` reset paths restore stale/default blink. Seeding happens at tab spawn (`tabs.rs:438`) and settings apply (`app.rs:2046`) only. |
| Color = fg | `src-tauri/src/render/mod.rs:950` | Cursor color computed as `packed_to_egui(core.get_cursor_fg(), theme.fg, theme)`; `core.get_cursor_fg()` is the SGR pen fg, and the fallback is `theme.fg` — `theme.cursor_fg` (scheme cursor color, OSC 12/112 target) is never consulted. |

### Design direction (planner decides details)

- Introduce a single "effective cursor state" resolution: settings/scheme
  defaults + optional sequence override, readable by the renderer without
  extra locks. Whether that lives in `term_core` (e.g. a terminal-level
  cursor style/blink separate from `CursorState`) or in the theme/tab layer
  is a planning decision, but the precedence rules of FR4 are normative.
- OSC 22 handling already mutates `theme.cursor_style`
  (`render/theme.rs:apply_cursor_style`); OSC 12/112 already mutate/reset
  `theme.cursor_fg`. DECSCUSR dispatch may not exist yet in
  `crates/term_core` — verify and add if missing.
- Blink must move out of (or be shielded from) `CursorState` save/restore.

### Dependencies

**Internal Dependencies:**
- `crates/term_core`: cursor state, DECSCUSR parsing/dispatch.
- `src-tauri/src/render/{mod,theme}.rs`: cursor overlay drawing, theme.
- `src-tauri/src/{app,tabs}.rs`: settings apply loop, tab spawn seeding.

**External Dependencies:** none new.

## Test Scenarios

### Unit Tests
- [ ] TS-1: Settings `cursor_style: bar` → effective style read by the renderer path is bar (per tab, after spawn).
- [ ] TS-2: Settings change at runtime updates the effective style/blink of every existing tab.
- [ ] TS-3: `cursor_blink: false` + DECSC/DECRC round-trip → blink stays false.
- [ ] TS-4: `cursor_blink: false` + terminal reset (RIS / `restore_cursor` with no saved state) → blink stays false.
- [ ] TS-5: DECSCUSR `CSI 3 SP q` (blinking underline) overrides settings shape+blink; `CSI 0 SP q` / `CSI SP q` returns to settings-derived shape+blink.
- [ ] TS-6: OSC 22 "underline" overrides settings style; OSC 22 "" resets to settings style.
- [ ] TS-7: Cursor overlay color = scheme cursor color by default; OSC 12 override wins; OSC 112 restores scheme cursor color (not `theme.fg`).
- [ ] TS-8: Applying settings while an app override is active preserves the override (FR5).

### Integration Tests
- [ ] Existing suite passes: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`.
- [ ] CLI feature gate: `cargo check --no-default-features` passes.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual verification (user): style change visible across tabs; blink stays off across tab switches; cursor color matches scheme; vim DECSCUSR behaves.

### Edge Cases
- [ ] Alt-screen enter/exit (mode 1049) does not resurrect blink or style.
- [ ] New tab spawned after a settings change inherits the new values.
- [ ] Unknown `cursor_style` string falls back to block (existing `parse_or_warn` behavior preserved).

## Error Handling

No new error surfaces. Invalid `cursor_style` values keep the existing
warn-once + block fallback.

## Success Criteria

- [ ] All functional requirements implemented and covered by tests.
- [ ] All listed test scenarios pass.
- [ ] NFR1/NFR2 verified (no render-path regression, CLI build compiles).

## Open Questions

- None.

## References

- REQUIREMENTS.md: feature-docs/cursor-settings-fix/REQUIREMENTS.md
- Cursor overlay: src-tauri/src/render/mod.rs (~line 903-960)
- Theme OSC handling: src-tauri/src/render/theme.rs (apply_cursor_style, OSC 12/112)
- Cursor state: crates/term_core/src/terminal_cursor.rs
