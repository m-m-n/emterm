# Verification Document: Terminal Profiles

## Overview

**Feature**: Terminal Profiles
**SPEC.md**: `doc/tasks/terminal-profiles/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/terminal-profiles/IMPLEMENTATION.md`

## Build Verification

- Command: `bun tauri build`
- Expected: exit code 0, no errors

## Test Verification

- Rust: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- TypeScript: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- TypeScript typecheck: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- E2E: `./scripts/run-e2e-docker.sh test`
- Coverage target: minimum 80%, target 90% for core logic (env var parsing, validation, profile resolution)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | Profile struct serialization/deserialization | Roundtrip produces identical data | Unit (Rust) |
| TS-02 | Default profile resolution (empty profiles) | Returns null/none | Unit (TS) |
| TS-03 | Default profile resolution (one default) | Returns the default profile | Unit (TS) |
| TS-04 | Environment variable parsing (valid KEY=VALUE) | Correct key-value map | Unit (TS) |
| TS-05 | Environment variable parsing (empty lines) | Empty lines ignored | Unit (TS) |
| TS-06 | Environment variable parsing (malformed lines) | Malformed lines skipped | Unit (TS) |
| TS-07 | Environment variable parsing (value contains =) | Key is before first =, value is rest | Unit (TS) |
| TS-08 | Default flag exclusivity | Setting new default clears old one | Unit (TS) |
| TS-09 | Profile validation (empty name) | Rejected with error message | Unit (Rust) |
| TS-10 | Settings load with profiles field | Profiles loaded correctly | Integration (Rust) |
| TS-11 | Settings load without profiles field | Empty profiles array, no error | Integration (Rust) |
| TS-12 | PTY spawn with profile shell_path and shell_args | Shell starts with correct program and args | Integration (Rust) |
| TS-13 | PTY spawn with profile environment variables | Env vars visible in spawned shell | Integration (Rust) |
| TS-14 | PTY spawn with profile working directory | Shell starts in specified directory | Integration (Rust) |
| TS-15 | PTY spawn without profile params (backward compat) | Uses default shell, no env changes | Integration (Rust) |
| TS-16 | Profile duplication | New profile with "(Copy)" suffix added | Unit (TS) |
| TS-17 | Keybind matching for profile_selector | Ctrl+Shift+P matches profile_selector | Unit (TS) |
| TS-18 | Zero profiles: all creation paths use global settings | Tab created with global shell_path/shell_args | Integration (TS) |

## Code Quality Verification

- TypeScript typecheck: `bun run typecheck`
- Rust check: `cargo check --manifest-path src-tauri/Cargo.toml`
- Rust clippy: `cargo clippy --manifest-path src-tauri/Cargo.toml`

## File Structure Verification

### Files to Create

- `src/profile/profile-selector.ts` - Profile selector modal overlay component
- `src/profile/profile-editor.ts` - Profile edit dialog (modal form)
- `src/profile/types.ts` - Profile helpers (env var parsing, default flag management)

### Files to Modify

- `src-tauri/src/commands/config.rs` - Profile struct, profiles field in AppSettings, validation, profile_selector keybind
- `src-tauri/src/pty/session.rs` - Accept env_vars and working_directory in PtySession::new
- `src-tauri/src/pty/manager.rs` - Forward new params in create_session_atomic
- `src-tauri/src/lib.rs` - Extended pty_spawn command with env_vars and working_directory
- `src/settings/types.ts` - Profile interface, profiles field, profile_selector keybind
- `src/settings/settings-sections.ts` - Profiles section renderer, profile_selector keybind UI
- `src/settings/settings-panel.ts` - Register "profiles" category
- `src/tab-bar/tab-manager.ts` - createTab with profile configuration
- `src/tab-bar/tab-bar-ui.ts` - + button behavior based on profile state
- `src/tab-bar/types.ts` - CreateTabOptions extension with profile fields
- `src/tab-bar/keyboard-handler.ts` - Handle profile_selector keybind
- `src/terminal-app/index.ts` - Accept profile spawn options
- `src/pty/client.ts` - Extended spawn options forwarding
- `src/types/pty.ts` - PtySpawnOptions extension
- `src/main.ts` - Wire profile resolution into createTerminalApp factory
- `src/i18n/locales/en.json` - Profile UI labels and keybind labels
- `src/i18n/locales/ja.json` - Profile UI labels and keybind labels
- `src-tauri/locales/en.json` - Profile validation messages
- `src-tauri/locales/ja.json` - Profile validation messages

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All functional requirements (FR1-FR10) implemented and tested | Run full test suite; check each FR mapping below |
| SC-02 | All test scenarios pass | `cargo test` + `bun test` + `./scripts/run-e2e-docker.sh test` exit 0 |
| SC-03 | Selector modal appears within 100ms | Manual timing or E2E performance measurement |
| SC-04 | Backward compatibility with existing settings.json | Unit test: deserialize settings without profiles field |
| SC-05 | No regression in existing E2E tests | `./scripts/run-e2e-docker.sh test` passes all existing specs |
| SC-06 | Code follows existing project patterns | Code review: serde patterns, i18n, settings UI components |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Profile Data Model | Phase 1 | Unit test: serialization roundtrip; TypeScript type matches Rust |
| FR2: Profile CRUD | Phase 3 | E2E: create, edit, delete profiles in settings UI |
| FR3: Profile Duplication | Phase 3 | Unit test: duplicate produces "(Copy)" suffix; E2E: duplicate button works |
| FR4: Profile Reordering | Phase 3 | E2E: drag and drop changes order; order persists after reload |
| FR5: Default Profile Flag | Phase 3 | Unit test: single default enforced; E2E: toggle default in settings |
| FR6: Profile Selector Modal | Phase 4 | E2E: modal appears, keyboard nav works, selection creates tab |
| FR7: Tab Creation Integration | Phase 4 | E2E: tab created with profile-specific settings; verify shell |
| FR8: Environment Variable Parsing | Phase 2+3 | Unit test: parsing logic; Integration: env vars in spawned shell |
| FR9: New Keybind for Selector | Phase 5 | E2E: Ctrl+Shift+P opens selector; keybind customizable |
| FR10: Settings UI Launch Button | Phase 3 | E2E: launch button creates tab with profile settings |

## E2E Testing (Docker)

- [ ] Profile CRUD: create new profile with all fields, verify it appears in list
- [ ] Profile CRUD: edit existing profile name and shell_path, verify changes saved
- [ ] Profile CRUD: delete profile, verify removed from list
- [ ] Profile CRUD: duplicate profile, verify "(Copy)" suffix
- [ ] Profile reorder: drag profile to new position, verify order persists
- [ ] Default flag: set profile as default, verify only one default badge shown
- [ ] + button (no profiles): creates tab with global settings (existing behavior)
- [ ] + button (with default): creates tab immediately with default profile
- [ ] + button (no default, profiles exist): shows selector modal
- [ ] Selector modal: arrow keys navigate, Enter selects, Escape cancels
- [ ] Selector modal: click on profile name selects it
- [ ] Tab with profile: verify shell started matches profile shell_path
- [ ] Keybind: Ctrl+Shift+P opens selector when profiles exist
- [ ] Keybind: Ctrl+Shift+P no-op when no profiles
- [ ] Keybind: Ctrl+Shift+T uses default profile when one exists
- [ ] Launch button: settings page launch button opens tab with profile
- [ ] Backward compatibility: existing E2E tests pass without regression

## Manual Testing (E2E Not Possible)

- [ ] Drag-and-drop reordering feels smooth and visually correct
- [ ] Profile selector modal appears without perceptible delay (<100ms subjective)
- [ ] Profile editor dialog layout looks correct on various window sizes
- [ ] Environment variables with special characters (quotes, spaces, unicode) work in spawned shell
- [ ] Working directory with spaces or unicode characters works correctly
- [ ] Profile with non-existent shell_path shows appropriate error
- [ ] Profile with non-existent working_directory falls back to home directory

## Performance Verification

- NFR1: Profile selector modal appears within 100ms of trigger (keyboard or + button click to modal visible)

## Security Verification

- [ ] Profile names rendered as text content, not innerHTML (XSS prevention)
- [ ] Environment variables not sanitized beyond KEY=VALUE parsing (users trusted with own environment)
- [ ] Shell path validated only on PTY spawn, not on save (consistent with existing behavior)

## Edge Cases

| ID | Edge Case | Expected Behavior | Verification |
|----|-----------|-------------------|--------------|
| EC-01 | Zero profiles defined | All tab creation uses global settings | E2E + Unit |
| EC-02 | Single profile marked as default | + button skips selector | E2E |
| EC-03 | All profiles deleted while selector open | Modal closes gracefully | Manual |
| EC-04 | Profile with empty shell_path | Uses system default shell | Unit + Integration |
| EC-05 | Profile with non-existent shell_path | PTY spawn error handling | Manual |
| EC-06 | Profile with non-existent working_directory | Fallback to home directory | Integration |
| EC-07 | env_vars with empty lines or comments | Ignored during parsing | Unit |
| EC-08 | Duplicate profile names | Allowed (no uniqueness constraint) | Unit |
| EC-09 | env_vars value containing = character | Key is before first =, rest is value | Unit |

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit Tests | 18 | 18 | 0 | 0 |
| Code Quality | 3 | 3 | 0 | 0 |
| Functional Requirements | 10 | 4 | 5 | 1 |
| E2E Scenarios | 17 | 0 | 17 | 0 |
| Manual Testing | 7 | 0 | 0 | 7 |
| Performance | 1 | 0 | 0 | 1 |
| Security | 3 | 1 | 0 | 2 |
| Edge Cases | 9 | 5 | 2 | 2 |
| **Total** | **69** | **32** | **24** | **13** |
