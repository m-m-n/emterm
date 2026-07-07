# Implementation Plan: Cursor Settings Fix (style / blink / color)

## Overview

Make settings and the active color scheme the authoritative defaults for
cursor shape, blink, and color, with escape-sequence overrides
(DECSCUSR / OSC 22 / OSC 12) taking precedence until reset.

## Technology Stack

- **Rust** — `crates/term_core` (parser + terminal state), `src-tauri` (GUI:
  renderer, theme, settings wiring). No new dependencies.

## Layer Structure

- `crates/term_core` — single authority for the *effective* cursor shape and
  blink state (defaults + sequence overrides). GUI-agnostic (NFR2).
- `src-tauri/src/render/` — reads effective shape/blink from the core and the
  effective cursor color from the theme. Never computes precedence itself.
- `src-tauri/src/{tabs,app}.rs` — seeds the core's cursor defaults at tab
  spawn and on settings apply.

Dependency direction unchanged: `src-tauri` → `term_core`.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Cursor style numeric mapping | Canonical shape encoding shared by settings, core, renderer | `0 = block`, `1 = underline`, `2 = bar`. Values > 2 clamp to block (existing core behavior preserved). | task0001, task0002 |
| `TerminalCore::set_cursor_style(style: u8)` | Set the settings-derived DEFAULT shape | Precondition: style in 0..=2 (out-of-range clamps to 0). Postcondition: default shape updated; an active sequence override is NOT cleared; `get_cursor_style()` returns the override if one is active, else the new default. | task0001 (implements), task0002 (calls) |
| `TerminalCore::set_cursor_blink(blink: bool)` | Set the settings-derived DEFAULT blink | Postcondition: default blink updated; an active sequence override is NOT cleared; `get_cursor_blink()` returns the override if one is active, else the new default. | task0001 (implements), task0002 (calls) |
| `TerminalCore::get_cursor_style()` / `get_cursor_blink()` | Report the EFFECTIVE shape / blink | Returns sequence override when active, otherwise the default. Existing callers (renderer overlay, App blink timer) keep their call sites unchanged. | task0001 (implements), task0003 (reads) |
| `Theme.cursor_fg` | Effective cursor color | Seeded from the active color scheme's cursor color at theme construction; OSC 12 overrides it; OSC 112 restores the SCHEME's cursor color (not a hard-coded preset). | task0003 |

Note: `set_cursor_style` / `set_cursor_blink` / `get_cursor_style` /
`get_cursor_blink` already exist with these names and signatures — only their
semantics change (default vs effective). task0002 and task0003 compile and
test against the existing API in their own worktrees; the semantic upgrade
lands via task0001 and the combination is validated in the verify phase.

## Conventions

- Follow existing `term_core` handler layout: CSI dispatch match arms route
  to handler methods in the matching `csi_*.rs` / state modules.
- Tests colocated `#[cfg(test)]` per existing style; GUI-only tests gated as
  the surrounding file already does.
- No locking or per-frame allocation added to the render path (NFR1).

## Cross-task Design Decisions

### D1: term_core is the single authority for shape + blink

Effective state = sequence override (if any) else settings-derived default.
Both live at the TERMINAL level, outside `CursorState`, so DECSC/DECRC
save/restore and cursor-state reset paths cannot clobber them (root cause of
the blink-revival bug). Affected: task0001, task0002, task0003.

Precedence rules (normative, from SPEC FR4):

1. DECSCUSR (`CSI Ps SP q`): Ps 1/2 → block, 3/4 → underline, 5/6 → bar;
   odd values and Ps 0 blink, even values steady. Sets BOTH shape and blink
   overrides. Ps 0 or absent → clears both overrides (restore defaults).
2. OSC 22: named style sets the SHAPE override only; empty payload clears the
   shape override only.
3. Full terminal reset (RIS): clears all cursor overrides; defaults survive.
4. Settings apply (`set_cursor_style` / `set_cursor_blink`): updates defaults
   only; never touches overrides (SPEC FR5).

### D2: theme.cursor_style is no longer a renderer input

The renderer keeps reading `core.get_cursor_style()` (unchanged call site);
with D1 that getter finally returns the settings/sequence-derived value.
`Theme.cursor_style` and its OSC 22 path (`Theme::apply_cursor_style`) remain
in place for theme-layer consumers but are not the cursor overlay's source of
truth. Affected: task0001, task0003.

### D3: Cursor color comes from the theme, never the SGR pen

The cursor overlay's color input is `Theme.cursor_fg` (scheme color, OSC 12
override, OSC 112 reset-to-scheme). `TerminalCore::get_cursor_fg()` (the SGR
pen foreground) is not a cursor-color source. For the focused filled block
cursor: fill = `Theme.cursor_fg`, covered glyph repainted in the cell's
resolved background color so text stays legible under the cursor.
Affected: task0003.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Moving style/blink out of `CursorState` breaks snapshot/serialization consumers (mux) | Medium | Medium | task0001 audits `snapshot.rs` and save/restore tests; keep field layout changes internal to term_core accessors |
| task0002/task0003 merge before task0001 → old semantics until all merged | High (by design) | Low | Each task's tests hold under both old and new semantics; combined behavior checked in verify phase |
| DECSCUSR default (Ps 0) behavior differs across terminals | Low | Low | FR4 fixes it as "restore settings defaults"; matches WezTerm/Alacritty reset-to-default convention |
| Block-cursor color change makes cursor invisible on same-colored cell | Low | Medium | Glyph repaint uses resolved cell background; manual verification item in VERIFICATION.md |

## Open Questions

- None.
