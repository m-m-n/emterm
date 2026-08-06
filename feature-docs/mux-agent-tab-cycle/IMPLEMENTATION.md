# Implementation Plan: mux-agent-tab-cycle

## Overview

A new mux prefix action `next-agent-window` cycles the active mux window
through only the windows whose panes have a reported (uncleared) agent
status, in display order with wrap-around. Out-of-range conditions
(non-mux tab, zero qualifying windows) are silent no-ops.

## Technology Stack

- **Rust (`gui` feature)** — key dispatch, mux window group model,
  agent-status model: all behavioral changes.
- **TypeScript (settings WebView)** — one settings-panel row for the new
  keybind.
- **New external dependencies**: none. (License check per project license
  MIT: nothing added, no conflict possible.)

## Adopted Assumptions (TBD resolutions, create-plan)

| ID | Resolution |
|----|------------|
| FR1 (status: assumed) | The cycle operation is a new mux action named `next-agent-window`, bound by default to the mux prefix (default Ctrl+Z) followed by **Ctrl+A**. Ctrl+A is currently unused as a follow-up key. The binding is user-configurable through `settings.mux.keybinds`, like every existing mux action. |
| FR6 (status: assumed) | **any-reported-state**: a mux window qualifies when at least one of its panes has an agent-status entry whose state is one of Idle / Working / Blocked / Done. Panes whose status was cleared (no state) or never reported do not qualify. |
| mux-tab = mux window (adopted) | "mux タブ" in the requirements means a mux window inside the active GUI tab's mux window group. Cycling happens among the mux windows of that group, in the group's display order. GUI-level tabs are not the cycle unit. |

## Layer Structure

```
key input (mux prefix follow-up-key table)     <- binding registration
        |
        v
mux action dispatch (App)                      <- new action handling, no-op guards
        |  reads (read-only)
        v
mux window group model / agent-status model    <- display order + qualifying predicate
```

- Dispatch reads the models; the models gain at most read-only query
  helpers and never learn about key input or dispatch.
- Settings mirrors: the Rust action-name list (validation) and the TS
  settings section (UI) both carry the same action identifier.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Action identifier `next-agent-window` | Names the cycle operation everywhere | Exact string `next-agent-window` (kebab-case, matching existing mux action names). Appears in: the Rust default follow-up-binding table (default key: Ctrl+A), the Rust mux action-name list used for settings validation, and the TS settings action list. Settings key: `settings.mux.keybinds["next-agent-window"]`; the keybinds map shape (action name → key) is unchanged, so no settings-schema change. | task0001 (registers + handles), task0002 (settings UI row) |

## Conventions

- No new source files: behavior is added to the existing key-dispatch path
  and existing models.
- Native user-visible strings (none expected; if any is added):
  `crate::i18n` inline `t(ja, en)`. WebView strings: keys in
  `src-tauri/web-shared/i18n/locales/{en,ja}.json`.
- Out-of-range conditions are silent no-ops, matching existing mux action
  behavior — no error dialog, no log output above debug level.

## Cross-task Design Decisions

### 1. Action registration spans two tasks via the shared identifier

task0001 registers and handles the action on the Rust side; task0002 adds
the settings-UI row. Both implement against the Shared Components contract
above; neither reads the other's plan.

### 2. Cycle-target resolution is a pure selection

Resolution is expressed as a pure decision over (display-ordered window
list with a per-window qualifying flag, current window index) → optional
target window, evaluated at key-event time. Rationale: unit-testable
without a GUI context (TS-1 … TS-5), and event-driven with no polling or
caching (NFR2).

Wrap rule: scanning starts at the position after the current window and
wraps once through the whole order, with the current window considered
last. Consequences: if the only qualifying window is the current one, the
result is the current window (active window unchanged); if no window
qualifies, there is no target (no-op, FR5). Affects task0001;
VERIFICATION.md relies on this decision for the unit-level scenarios.

### 3. Non-mux no-op reuses the existing guard

Dispatching a mux action while the active GUI tab has no mux window group
already resolves to nothing in the existing dispatch path. FR4 requires no
new mechanism — only that the new action goes through the same guard.
Affects task0001 (TS-6).

### 4. Feature gate (NFR1)

All Rust changes live in modules already compiled only under the `gui`
feature; nothing is added to always-built crates, so the
`--no-default-features` (CLI-only) build is unaffected.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A user has already bound Ctrl+A to another action in `settings.mux.keybinds` | Low | Low | Defaults apply only for actions absent from user settings; duplicate/override handling follows the existing keybind-validation behavior unchanged. |
| Parallel arrays (windows / pane ids) drift while building the ordered qualify list | Low | Medium | Build the list in a single traversal through the group's existing accessors; unit-cover multi-window groups. |
| Ambiguity of a multi-pane window's qualification | Low | Low | Predicate is existential: one qualifying pane qualifies the window (recorded in the FR6 assumption above). |

## Open Questions

- [ ] NFR1 / NFR2 / NFR3 have no TS-n scenario ID; they are verified by the
      build / format / typecheck commands and review, per VERIFICATION.md.
      (Deliberate mapping — not a coverage gap.)
