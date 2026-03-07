# Verification Document: Profile Editor SHELL/SSH Tab UI

## Overview

**Feature**: Profile Editor SHELL/SSH Tab UI
**SPEC.md**: `doc/tasks/profile-shell-ssh-tabs/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/profile-shell-ssh-tabs/IMPLEMENTATION.md`

## Build Verification

- Command: `bun tauri build`
- Expected: exit code 0, no errors

## Test Verification

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c 'bun test'`
- TypeCheck: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c 'bun run typecheck'`
- Expected: exit code 0, no errors

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | New profile opens with SHELL tab selected | SHELL tab active, shell fields visible, SSH panel hidden | Unit |
| TS-2 | Profile with ssh_connection_name opens with SSH tab selected | SSH tab active, SSH dropdown visible, shell fields hidden | Unit |
| TS-3 | Profile without ssh_connection_name opens with SHELL tab selected | SHELL tab active | Unit |
| TS-4 | Save from SHELL tab | Profile has empty ssh_connection_name | Unit |
| TS-5 | Save from SSH tab | Profile has empty shell_path, shell_args, env_vars, working_directory | Unit |
| TS-6 | Tab switch from SSH to SHELL | ssh_connection_name cleared | Unit |
| TS-7 | Tab switch from SHELL to SSH | Shell fields cleared | Unit |
| TS-8 | SSH connections empty | SSH tab is disabled, cannot be selected | Unit |
| TS-9 | Editing SSH profile when referenced connection deleted | SSH tab selected, dropdown shows no matching selection | Unit |
| TS-10 | Rapid tab switching | No state inconsistency | Unit |

## Code Quality Verification

- TypeCheck: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c 'bun run typecheck'`
- Expected: exit code 0

## File Structure Verification

### Files to Create

- (none)

### Files to Modify

- `src/profile/profile-editor.ts` - Add tab bar UI, restructure form fields into tab panels, tab state management
- `src/styles/settings-panel.css` - Add `.profile-editor-tabs`, `.profile-editor-tab`, `.profile-editor-tab.active`, `.profile-editor-tab.disabled`, `.profile-editor-tab-panel` styles
- `src/i18n/locales/en.json` - Add `settings.profiles.tabShell` ("Shell") and `settings.profiles.tabSsh` ("SSH")
- `src/i18n/locales/ja.json` - Add `settings.profiles.tabShell` ("Shell") and `settings.profiles.tabSsh` ("SSH")

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | SHELL/SSH tabs displayed in profile editor modal | E2E: open profile editor, verify tab bar element exists with two tabs |
| SC-2 | Tab switching correctly shows/hides fields and clears values | Unit test: switch tabs and assert field values and panel visibility |
| SC-3 | SSH tab disabled when no SSH connections exist | Unit test: provide empty ssh_connections, assert SSH tab has disabled state |
| SC-4 | Auto-selection works for existing profiles | Unit test: provide profile with ssh_connection_name, assert SSH tab is active |
| SC-5 | ARIA roles and keyboard navigation implemented | Manual: inspect DOM for role=tablist/tab/tabpanel; test arrow key navigation |
| SC-6 | i18n works for both English and Japanese | Manual: switch language, verify tab labels update |
| SC-7 | Existing E2E tests pass without regression | E2E: run `./scripts/run-e2e-docker.sh` |
| SC-8 | TypeScript typecheck passes | Run typecheck command |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Tab bar inside profile editor | Phase 1 | E2E + Manual: tab bar rendered between name field and form fields |
| FR2: SHELL tab displays shell fields | Phase 1 | Unit: SHELL panel contains shell_path, shell_args, env_vars, working_directory |
| FR3: SSH tab displays dropdown | Phase 1 | Unit: SSH panel contains SSH connection dropdown |
| FR4: Tab switching clears other mode's values | Phase 1 | Unit: assert values cleared on tab switch |
| FR5: Disable SSH tab when no connections | Phase 1 | Unit: SSH tab disabled when ssh_connections is empty |
| FR6: Auto-select tab based on ssh_connection_name | Phase 1 | Unit: SSH tab selected for profile with ssh_connection_name |

## E2E Testing (Docker)

- [ ] Profile editor modal opens and shows SHELL/SSH tab bar
- [ ] SSH tab is disabled when no SSH connections configured
- [ ] Existing E2E tests pass without regression (`./scripts/run-e2e-docker.sh`)

## Manual Testing (E2E Not Possible)

- [ ] Tab styling matches MD3 design tokens (visual inspection)
- [ ] Keyboard navigation with arrow keys between tabs feels responsive
- [ ] Tab switching animation/transition is smooth
- [ ] i18n labels display correctly in both English and Japanese
- [ ] Screen reader announces tab roles correctly (accessibility)

## Performance Verification (if applicable)

- Tab switching latency: imperceptible (< 16ms, DOM show/hide only)

## Security Verification (if applicable)

- (not applicable - frontend-only UI change with no new data flows)

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Unit Tests | 10 | 10 | 0 | 0 |
| Success Criteria | 8 | 2 | 2 | 4 |
| Functional Requirements | 6 | 5 | 1 | 0 |
| E2E Tests | 3 | 0 | 3 | 0 |
| Manual Tests | 5 | 0 | 0 | 5 |
| **Total** | **32** | **17** | **6** | **9** |
