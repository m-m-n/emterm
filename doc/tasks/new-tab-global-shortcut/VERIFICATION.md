# Verification Document: New Tab (Global Settings) Shortcut

## Overview

**Feature**: New Tab (Global Settings) Shortcut — `new_tab_global` keybind (default `Ctrl+Shift+G`)
**SPEC.md**: `doc/tasks/new-tab-global-shortcut/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/new-tab-global-shortcut/IMPLEMENTATION.md`

## Build Verification

- **Command** (from `sdd.yaml`): `bun tauri build`
- **Expected**: exit code 0, no compilation errors, no clippy warnings beyond baseline.
- **Notes**: Inner TDD loop does not need a full `bun tauri build`; final verification (sdd.6) runs it via the configured workflow.

### Implementation Result (Phase 4)

- Build: deferred to sdd.6-verify (per inner TDD loop scope).
- Compile check during TDD: `cargo test` builds the lib and tests successfully under Docker (no compile errors).
- TypeScript: `tsc --noEmit` (`bun run typecheck`) exit code 0.

## Test Verification

- **Command** (from `sdd.yaml`):
  `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml && bun test && bun run typecheck"`
- **Coverage target**: maintain parity with existing `new_tab` coverage; do not lower overall repository coverage.

### Implementation Result (Phase 4)

- **Rust tests** (`cargo test --manifest-path src-tauri/Cargo.toml`): all green
  - lib: 998 passed, 0 failed, 1 ignored
  - cli: 10 passed, 0 failed
  - integration: 10 + 7 + 6 + 4 passed, 0 failed
- **TypeScript tests** (`bun test`): 2325 passed, 17 todo, 0 failed (2342 total across 106 files)
- **Typecheck** (`bun run typecheck`): exit code 0, no errors
- **New keyboard-handler scenarios** (TS-4..TS-8): 4 new tests pass (Ctrl+Shift+G with/without profiles, Ctrl+Shift+T regression, Ctrl+Alt+N override)
- **New Rust tests**: `test_keybind_settings_default` extended; `test_deserialize_keybinds_missing_new_tab_global`, `test_deserialize_keybinds_null_new_tab_global`, `test_deserialize_keybinds_custom_new_tab_global` added — all pass

### Test Scenarios from SPEC.md

| ID    | Scenario                                                                                                                                | Expected Result                                                                                            | Test Type   |
|-------|-----------------------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------|-------------|
| TS-1  | `KeybindSettings` default in Rust includes `new_tab_global = "Ctrl+Shift+G"`                                                            | Default-value test asserts the literal                                                                     | Unit (Rust) |
| TS-2  | Loading config JSON missing the `new_tab_global` field                                                                                  | Resulting `keybinds.new_tab_global == "Ctrl+Shift+G"`                                                       | Unit (Rust) |
| TS-3  | Loading config JSON with `"new_tab_global": null`                                                                                       | Resulting `keybinds.new_tab_global == "Ctrl+Shift+G"` via `deserialize_null_with!`                          | Unit (Rust) |
| TS-4  | `Ctrl+Shift+G` with NO profiles registered                                                                                              | `tabManager.createTab` is called once with no profile; profile selector NOT shown; `handleKeyDown` returns `true` | Unit (TS)   |
| TS-5  | `Ctrl+Shift+G` with profiles registered AND a default profile set                                                                       | `tabManager.createTab` is called with no profile; `tabBarUI.createTabWithProfile` NOT called               | Unit (TS)   |
| TS-6  | `Ctrl+Shift+G` with profiles registered AND no default profile                                                                          | Same as TS-5 (no profile selector shown, global tab created)                                               | Unit (TS)   |
| TS-7  | `Ctrl+Shift+T` regression: existing profile-aware path still triggered                                                                  | Existing `handleNewTab` path runs; `tabBarUI.createTabWithProfile` invoked when default profile exists      | Unit (TS)   |
| TS-8  | User overrides `keybinds.new_tab_global` to `Ctrl+Alt+N`                                                                                | New key triggers profile-less tab creation; old key (`Ctrl+Shift+G`) no longer matches `new_tab_global`     | Unit (TS)   |
| TS-9  | All existing test mocks of `KeybindSettings` include the new field                                                                      | `bun run typecheck` is clean across the repo                                                                | Unit (TS)   |
| TS-10 | Settings UI renders the `new_tab_global` row in the Tab Management subsection between `new_tab` and `close_tab`                         | DOM contains an input row with the localized label                                                          | Manual / Optional E2E |
| TS-11 | Latency parity: keypress-to-tab-create timing matches `new_tab` baseline                                                                | Subjective parity (no perceivable delay)                                                                    | Manual      |

## Code Quality Verification

- **Format**: `cargo fmt --manifest-path src-tauri/Cargo.toml && bun x prettier --write 'src/**/*.{ts,json}'`
  - Expected: no diff after running.
- **Static analysis**: existing `cargo clippy` and TypeScript `bun run typecheck` baselines — no new warnings/errors.

### Implementation Result (Phase 4)

- **rustfmt** on touched files (`settings.rs`, `tests/defaults.rs`, `tests/deserialization.rs`): clean, no diff
- **prettier** on touched TS/JSON files: clean after explicit `bun x prettier --write` invocation; reformatted whitespace in pre-existing test mocks (whitespace-only changes alongside the new field). Functional content of all tests unchanged.
- **typecheck**: clean (see Test Verification)
- **Note**: A pre-existing rustfmt drift in `src-tauri/src/pty/reader.rs` (use ordering) exists in the repo on `main`; it is unrelated to this feature and not introduced by these changes.

## File Structure Verification

### Files to Create

- (none) — feature is fully additive within existing files.

### Files to Modify

- [x] `src-tauri/src/commands/config/settings.rs` — added `define_keybinds!` entry for `new_tab_global` (after `new_tab`).
- [x] `src-tauri/src/commands/config/tests/defaults.rs` — assert `new_tab_global == "Ctrl+Shift+G"`.
- [x] `src-tauri/src/commands/config/tests/deserialization.rs` — added 3 tests (missing field, null, custom value).
- [x] `src/settings/types.ts` — added `new_tab_global: string` to `KeybindSettings` interface.
- [x] `src/settings/settings-panel.test.ts` — extended mock `KeybindSettings` with `new_tab_global`.
- [x] `src/settings/settings-applier.test.ts` — extended mock `KeybindSettings` with `new_tab_global`.
- [x] `src/settings/sections/keybinds-section.ts` — render new row between `new_tab` and `close_tab`.
- [x] `src/tab-bar/keyboard-handler.ts` — added dispatch branch for `Ctrl+Shift+G` placed BEFORE `new_tab`.
- [x] `src/tab-bar/keyboard-handler.test.ts` — added scenarios TS-4..TS-8 (4 new tests).
- [x] `src/tab-bar/tab-manager.test.ts` — extended mock `KeybindSettings` with `new_tab_global`.
- [x] `src/tab-bar/tab-bar-ui.test.ts` — extended mock `KeybindSettings` with `new_tab_global`.
- [x] `src/tab-bar/drag-handler.test.ts` — extended mock `KeybindSettings` with `new_tab_global`.
- [x] `src/i18n/locales/en.json` — added `settings.keybinds.newTabGlobal = "New Tab (Global)"`.
- [x] `src/i18n/locales/ja.json` — added `settings.keybinds.newTabGlobal = "新しいタブ (グローバル設定)"`.

## SPEC.md Compliance

### Success Criteria

| ID   | Criterion                                                                                              | How to Verify                                                                  |
|------|--------------------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------|
| SC-1 | All FRs (FR1–FR5) implemented and tested                                                               | TS-1..TS-9 + manual TS-10 cover every FR (see coverage table below)            |
| SC-2 | Test scenarios pass under Docker                                                                       | Run the configured `test_command` in `sdd.yaml`                                |
| SC-3 | `bun run typecheck` passes                                                                             | Included in `test_command`                                                     |
| SC-4 | `cargo test --manifest-path src-tauri/Cargo.toml` passes                                               | Included in `test_command`                                                     |
| SC-5 | `bun test` passes                                                                                      | Included in `test_command`                                                     |
| SC-6 | `cargo fmt` and prettier produce a clean diff                                                          | Run `format_command` from `sdd.yaml`; check `git status`                       |
| SC-7 | No regression in `Ctrl+Shift+T` behavior                                                               | TS-7                                                                           |
| SC-8 | Settings UI renders new row in en/ja                                                                   | TS-10 (visual)                                                                 |
| SC-9 | Backward compatibility with existing `config.json`                                                     | TS-2, TS-3                                                                     |

### Functional Requirements Coverage

| Requirement | Phase   | Verification                                                  |
|-------------|---------|---------------------------------------------------------------|
| FR1         | Phase 1 + Phase 2 | TS-1, TS-9 (Rust default + TS interface presence)   |
| FR2         | Phase 2 | TS-4, TS-5, TS-6 (profile-less dispatch under all profile states) |
| FR3         | Phase 3 | TS-10 (UI row present in correct position)                    |
| FR4         | Phase 3 | TS-10 (label resolves to en/ja strings)                       |
| FR5         | Phase 2 | TS-7 (regression for `Ctrl+Shift+T`)                          |
| NFR1        | Phase 2 | TS-11 (manual latency parity)                                  |
| NFR2        | Phase 1 | TS-2, TS-3 (missing field + null both yield default)           |
| NFR3        | Phase 1 | TS-1 (default produced via `define_keybinds!` macro path; code review) |
| NFR4        | All     | No platform-specific code introduced; covered by existing CI matrix |

## E2E Testing

Project E2E framework: WebdriverIO via `./scripts/run-e2e-docker.sh test`, specs under `e2e-tests/specs/*.e2e.js`.

Per project policy (CLAUDE.md and `feedback_tdd_scope`), the full E2E suite is NOT run during the TDD inner loop. The following are evaluated only at `sdd.6-verify`:

- [ ] Existing E2E suite passes without regression.
- [ ] (Optional) New spec: open the app, press `Ctrl+Shift+G` with no profiles configured, assert tab count increased and the new tab is active.

## Manual Testing (E2E Not Possible)

- [ ] In English locale, open Settings → Keybinds → Tab Management; confirm a row labeled "New Tab (Global)" appears between "New Tab" and "Close Tab".
- [ ] In Japanese locale, confirm the same row labeled "新しいタブ (グローバル設定)".
- [ ] Edit the row to a new key combination (e.g. `Ctrl+Alt+N`), save, and confirm the new key opens a global-settings tab while `Ctrl+Shift+G` no longer triggers it.
- [ ] Subjective latency: pressing `Ctrl+Shift+G` opens a tab indistinguishably fast compared to `Ctrl+Shift+T`.
- [ ] Open an `config.json` from a previous build (lacking `new_tab_global`) and verify the app starts and the default `Ctrl+Shift+G` works.

## Performance Verification

- **NFR1**: Keypress-to-tab-create latency parity with `Ctrl+Shift+T`. Verified manually (TS-11). Implementation introduces only one additional `matchKeybindStr` call in the keydown handler — O(1) string comparison, no allocations beyond what the existing handler already performs. No perf benchmark is required.

## Security Verification

- [ ] No new IPC commands; no new external inputs. Keybind string validation reuses existing validators.
- [ ] No data flowing across trust boundaries beyond the existing settings file.
- [ ] No XSS / injection surface introduced (only a string value is stored).

## Verification Summary

| Category              | Items | Automated | E2E | Manual |
|-----------------------|-------|-----------|-----|--------|
| Functional (FR1–FR5)  | 5     | 4         | 0   | 1 (FR3/FR4 visual) |
| Non-Functional (NFR1–NFR4) | 4 | 3         | 0   | 1 (NFR1 latency)   |
| Test Scenarios (TS-1..TS-11) | 11 | 9      | 0 (deferred to sdd.6) | 2 |
| Build / Format        | 2     | 2         | 0   | 0      |
| Total                 | 22    | 18        | 0 (inner loop) / deferred | 4 |
