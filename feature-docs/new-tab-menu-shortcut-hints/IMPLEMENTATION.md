# Implementation Plan: new-tab-menu-shortcut-hints

## Overview
Show, on the new-tab chooser opened by the tab bar's + button, the shortcut
that opens a row's target: `new_tab_global` on the "Global Settings" row and
`new_tab` on the row of the profile the `new_tab` keybind opens. Labels are
derived from the resolved keybind table each time the chooser is drawn
(SPEC.md FR1-FR8, NFR1-NFR4). The work is planned as a single task,
task0001.

## Technology Stack
- **Language / Framework**: Rust; egui for the in-process profile selector dialog (GUI-gated native stack)
- **Test runner**: `cargo test` unit tests in the library target (`--lib`)
- **Reused internal components**: `KeybindTable` / `parse_chord` (`ui::keybinds`), `ProfileSelectorState` and its row-to-choice mapping (`ui::profile_selector`), the md3 color roles and the profile selector's existing layout constants
- **New dependencies**: none (nothing to check against `project.license` MIT)

## Layer Structure
| Layer | Location | Change |
|-------|----------|--------|
| Keybind resolution (pure) | `src-tauri/src/ui/keybinds.rs` | Adds chord label formatting and the reachable-label query |
| Selector UI (state and painter) | `src-tauri/src/ui/profile_selector.rs` | Adds the row-label assignment; the row painter draws the label |
| Application | `src-tauri/src/app/chooser.rs` | Adds the App-level label query over the current App state |
| Render pass | `src-tauri/src/render/overlays.rs` | Attaches each row's label to the row data handed to the painter |
| CLI-only build (everything outside the `gui` feature) | — | No change |

Dependency directions: render → app → ui. Inside the ui layer,
`ui::profile_selector` gains a dependency on `ui::keybinds`; the ui layer
still never depends on app or render. All four changed modules are declared
under the `gui` feature in `src-tauri/src/lib.rs`, so the CLI-only build is
not affected (NFR3).

## Shared Components
None. The feature has one task, so no component built by one task is used by
another task.

## Conventions
- Token reuse only: the label uses the profile selector's existing shell-path font size (label-small, 12px) and the shell path's color roles. Nothing is added to `doc/UI-DESIGN-GUIDELINES.yaml`, `src-tauri/src/ui/md3.rs`, `src-tauri/src/ui/dialog/tokens.rs` or `src-tauri/web-shared/styles.css` (NFR1).
- Labels are derived, never stored: no label is cached on `App` or on the selector state (FR4, SPEC.md A3).
- Label text takes no locale or platform input (NFR2).
- Existing tests in `src-tauri/src/ui/keybinds/tests.rs`, `src-tauri/src/ui/profile_selector.rs` and `src-tauri/src/app/tests/chooser.rs` keep their names. Renaming one would trigger the update duty of `.claude/rules/test-docs-records.md`.

## Cross-task Design Decisions

### D1: Single task
- **Decision**: FR1-FR8 and NFR1-NFR4 are implemented by task0001 alone.
- **Rationale**: the chord formatter, the reachable-label query, the row-label assignment, the App-level query, the overlay wiring and the row painter form one dependency chain, and each step's tests need the step before it. Split tasks run in parallel worktrees, so a consumer task could not compile without a placeholder for its producer, for a change of six files. task0001 has eight acceptance criteria, one above the plan-writing guideline of about seven; this is accepted to keep the task worktree-independent.
- **Affected tasks**: task0001

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The formatter writes a main-key token `parse_chord` rejects (for example the `+` symbol for Plus, which cannot be parsed back because specs are split on `+`) | Medium | Medium - the shown label does not match what the settings accept | task0001 AC-2 checks the round trip over every main key `parse_chord` can produce |
| A long profile name plus the Default badge leaves no room before the label | Medium | Low | task0001 bounds name and badge painting at the label boundary (AC-7); TS-9 checks it visually |
| The full `--lib` run hits nondeterministic tests unrelated to this feature (`tabs.rs` replay tests under parallel execution; the `tmux_sockets` discovery fork race) | Medium | Low | VERIFICATION.md states the rerun procedure |
| Tests that call the public chooser-open path run real tmux discovery and depend on the environment | Low | Low | task0001 tests use the entry-injecting chooser-open variant |

## Open Questions
None.
