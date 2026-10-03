# Verification Document: new-tab-menu-shortcut-hints

## Overview
**Feature**: new-tab-menu-shortcut-hints / **SPEC.md**: `feature-docs/new-tab-menu-shortcut-hints/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/new-tab-menu-shortcut-hints/IMPLEMENTATION.md`

All commands run from the project root of the integration worktree, without
changing directory (`.claude/rules/core-build-location.md`).

## Build Verification
- Command (GUI, default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` (workflow.yaml `project.components.main.build_command`)
- Command (CLI-only): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` (workflow.yaml `project.components.cli_only.build_command`; TS-11)
- Expected: exit code 0, no errors, for both
- The `typescript` component is not in this feature's scope: no file under its commands' reach changes

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` (workflow.yaml `project.components.main.test_command`)
- Coverage target: not configured (workflow.yaml declares no coverage tooling); every TS-n below has a passing automated test or a recorded check
- Known nondeterministic tests unrelated to this feature: the `tabs.rs` replay tests under parallel execution, and the `tmux_sockets` discovery test (fork race, parallel runs only). When one of them fails, rerun it alone or with `--test-threads=1` before recording a failure

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Format the default table's `new_tab_global` and `new_tab` chords (`src-tauri/src/ui/keybinds/tests.rs`) | `Ctrl+Shift+G` and `Ctrl+Shift+T` | Unit |
| TS-2 | Round trip: parse the formatted label of every default-table chord and of PageDown, ArrowUp, Plus, F11, a digit and Comma (`src-tauri/src/ui/keybinds/tests.rs`; task0001 AC-2 extends this to every main key `parse_chord` can produce) | `parse_chord` returns the original chord for each | Unit |
| TS-3 | Format a table built from `ctrl+shift+g`, a table built from an unparseable `new_tab` spec, and a chord with Ctrl, Shift, Alt and T (`src-tauri/src/ui/keybinds/tests.rs`) | `Ctrl+Shift+G`; `new_tab` is `Ctrl+Shift+T`; `Ctrl+Shift+Alt+T` | Unit |
| TS-4 | Chooser-mode label assignment for profiles [A (default), B] with a tmux row (`src-tauri/src/app/tests/chooser.rs` or the profile selector module's tests) | Global row `new_tab_global` label, A `new_tab` label, B none, tmux row none | Unit |
| TS-5 | Chooser-mode assignment with no default profile, and with profiles [A (default), B (default)] (same placement as TS-4) | No default: only the Global row has a label, the `new_tab_global` one. Two defaults: only A has the `new_tab` label | Unit |
| TS-6 | Shadowed chords: `new_tab_global` = profile_selector chord; `new_tab` = copy chord; `new_tab` = `new_tab_global` (same placement as TS-4; task0001 AC-4 adds the remaining priority cases and a lower-priority collision) | Global row none; default-profile row none; default-profile row none while the Global row keeps its label | Unit |
| TS-7 | Selector mode: open through `open_profile_selector` (include_global off) with a default profile configured (`src-tauri/src/app/tests/chooser.rs`) | No row has a label | Unit |
| TS-8 | Apply settings with `keybinds.new_tab_global` = `Ctrl+Alt+G` and `keybinds.new_tab` = `Ctrl+Alt+N`, then reopen the chooser (`src-tauri/src/app/tests/chooser.rs`) | Global row `Ctrl+Alt+G`, default-profile row `Ctrl+Alt+N`; the same labels with the App locale Ja and En | Unit |
| TS-9 | Visual check of the + chooser on a release build (see Manual Testing) | See Manual Testing | Manual |
| TS-10 | Row geometry computation with synthetic row edges, start positions and label widths: a labeled row, an unlabeled row, and a labeled row with no room left for the shell path (`src-tauri/src/ui/profile_selector.rs`; added by create-plan for FR7) | Label right edge at the row right edge minus ROW_PAD_X; shell path range ends ROW_INNER_GAP before the label's left edge, empty when no room remains; name and badge bounded at the same boundary; the unlabeled row's geometry equals today's | Unit |
| TS-11 | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` (added by create-plan for NFR3) | Exit code 0 | Build |
| TS-12 | `git diff --name-only {base}...HEAD`, where `{base}` is workflow.yaml `implement.base_commit` (added by create-plan for NFR1 and NFR3) | Every listed path is under `src-tauri/src/ui/`, `src-tauri/src/app/`, `src-tauri/src/render/`, `feature-docs/new-tab-menu-shortcut-hints/` or `test-docs/new-tab-menu-shortcut-hints/`; none of `doc/UI-DESIGN-GUIDELINES.yaml`, `src-tauri/src/ui/md3.rs`, `src-tauri/src/ui/dialog/tokens.rs`, `src-tauri/web-shared/styles.css` is listed | Static check |

## Code Quality Verification
- Format: not declared (workflow.yaml `project.components.main.format_command` is empty); crate-wide reformatting is out of scope
- Static analysis: the two `cargo check` commands in Build Verification

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Default keybinds: the Global Settings row shows `Ctrl+Shift+G` | TS-1, TS-4 |
| AC-2 | Default keybinds: the default profile's row shows `Ctrl+Shift+T` | TS-1, TS-4 |
| AC-3 | After applying changed `new_tab_global` / `new_tab` keybinds, the next chooser shows the new chords | TS-8 |
| AC-4 | Non-default profile rows, later `is_default` rows and tmux rows show no label; with no default profile only the Global row shows its `new_tab_global` label | TS-4, TS-5 |
| AC-5 | Lowercase specs show in canonical form; unparseable specs show the fallback default; every label parses back to its chord | TS-2, TS-3 |
| AC-6 | Shadowed chords show no label; `new_tab` = `new_tab_global` keeps the Global row's label | TS-6 |
| AC-7 | The Ctrl+Shift+P profile selector shows no label | TS-7 |
| AC-8 | Labels are right-aligned; long shell paths are truncated before the label without overlap | TS-10, TS-9 |
| SC-1 | FR1-FR8 and NFR1-NFR4 are implemented and tested | Functional Requirements Coverage table below |
| SC-2 | TS-1 to TS-8 unit tests pass | Test Verification command |
| SC-3 | TS-9 manual check is completed | Manual Testing |
| SC-4 | The `--no-default-features` build compiles | TS-11 |
| SC-5 | AC-1 to AC-8 are met | The AC rows above |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-4, TS-5 |
| FR2 | task0001 | TS-1, TS-4, TS-5 |
| FR3 | task0001 | TS-4, TS-5 |
| FR4 | task0001 | TS-8 |
| FR5 | task0001 | TS-1, TS-2, TS-3 |
| FR6 | task0001 | TS-6 |
| FR7 | task0001 | TS-10; TS-9 |
| FR8 | task0001 | TS-7 |
| NFR1 | task0001 | TS-12; TS-9 (font and colors) |
| NFR2 | task0001 | TS-8 (same labels for Ja and En); TS-9 (Japanese and English UI); review: the label path contains no platform-conditional code |
| NFR3 | task0001 | TS-11, TS-12 |
| NFR4 | task0001 | TS-1 to TS-6 call the formatter and the assignment with no egui context |

## Manual Testing (E2E Not Possible)
Prerequisite: a Linux GUI release build (`make build`, `.claude/rules/core-commands.md`), at least two profiles with one set as default, and one profile whose shell path is long.

- [ ] TS-9 (FR7, NFR1, NFR2): Open the chooser with the tab bar's + button and confirm:
  - The Global Settings row shows the `new_tab_global` shortcut and the default profile's row shows the `new_tab` shortcut, each right-aligned at the row's right padding; no other row shows a label.
  - On a labeled row with a long shell path, the shell path is cut off before the label with a visible gap and does not overlap it.
  - On a labeled row with a long profile name, the name and the Default badge do not paint under the label.
  - The label color matches the shell path's color on a normal row and on the highlighted row (move the highlight with the arrow keys).
  - With the UI language set to Japanese and to English, the label strings are identical.
  - Ctrl+Shift+P opens the profile selector with no labels.

## Performance / Security Verification (if applicable)
Not applicable: THREAT-MODEL.md's verdict is `no-trust-boundary` (no TM-n), and SPEC.md states no performance requirement.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (GUI `cargo check`) | 1 | 1 | 0 | 0 |
| Build (CLI-only, TS-11) | 1 | 1 | 0 | 0 |
| Unit tests (TS-1 to TS-8, TS-10) | 9 | 9 | 0 | 0 |
| Static check (TS-12) | 1 | 1 | 0 | 0 |
| Manual (TS-9) | 1 | 0 | 0 | 1 |
| **Total** | 13 | 12 | 0 | 1 |
