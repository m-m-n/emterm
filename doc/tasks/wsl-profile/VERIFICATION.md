# Verification Document: WSL Profile Support

## Overview
**Feature**: WSL Profile Support
**SPEC.md**: `doc/tasks/wsl-profile/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/wsl-profile/IMPLEMENTATION.md`

## Build Verification
- Command: `bun tauri build`
- Expected: exit code 0, no errors
- Cross-platform: builds successfully on both Linux and Windows targets

## Test Verification
- Rust: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- TypeScript: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- TypeScript typecheck: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Coverage target: minimum 80% for new Rust code

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | Parse `wsl --list --quiet` output with multiple distros | Returns list of distro names | Unit |
| TS-02 | Parse UTF-16LE encoded output | Correctly decodes distro names | Unit |
| TS-03 | Parse output with BOM | BOM stripped, names correct | Unit |
| TS-04 | WSL not installed (command fails) | Returns empty list | Unit |
| TS-05 | No distributions installed | Returns empty list | Unit |
| TS-06 | Output with empty lines and whitespace | Filtered correctly | Unit |
| TS-07 | WslDistribution serialization round-trip | Identical after serialize/deserialize | Unit |
| TS-08 | Profile with wsl_distro_name round-trip | Field persists correctly | Unit |
| TS-09 | Existing settings.json without WSL fields | Loads with defaults (empty) | Unit |
| TS-10 | Distribution name with spaces | Handled correctly | Unit |

## Code Quality Verification
- Rust format: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- TypeScript typecheck: `bun run typecheck`

## File Structure Verification

### Files to Create
- `src-tauri/src/wsl/mod.rs` - WSL module declarations
- `src-tauri/src/wsl/detect.rs` - WSL distribution detection
- `src-tauri/src/commands/wsl.rs` - Tauri WSL command
- `src/settings/sections/wsl-section.ts` - WSL settings section

### Files to Modify
- `src-tauri/src/commands/mod.rs` - Register wsl module
- `src-tauri/src/commands/config/settings.rs` - WslDistribution, wsl_distributions, wsl_distro_name
- `src-tauri/src/lib.rs` - Register commands
- `src/settings/types.ts` - WslDistribution, AppSettings, Profile
- `src/settings/settings-panel.ts` - WSL category
- `src/profile/profile-editor.ts` - WSL tab
- `src/tab-bar/tab-bar-ui.ts` - launchWslProfile
- `src/i18n/locales/en.json` - WSL labels
- `src/i18n/locales/ja.json` - WSL labels

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | WSL distributions detected on Windows | Run app on Windows, check settings WSL section |
| SC-02 | Import/delete workflow functions | Import distro → appears in imported list → delete → removed |
| SC-03 | Profile editor shows Shell/SSH/WSL on Windows | Open profile editor on Windows |
| SC-04 | WSL profile launches session | Create WSL profile → select → new tab with WSL shell |
| SC-05 | Linux hides WSL UI | Run app on Linux → no WSL category in settings, no WSL tab |
| SC-06 | Existing tests pass | Run full test suite |
| SC-07 | TypeScript typecheck passes | `bun run typecheck` exits 0 |
| SC-08 | i18n for en/ja | Switch language, verify WSL labels |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: WSL Distribution Detection | Phase 1 | Unit tests (TS-01 to TS-06, TS-10) |
| FR2: WSL Distribution Import | Phase 2 | Manual: Import button adds to list |
| FR3: WSL Settings Section | Phase 2 | Manual: Section visible on Windows with correct layout |
| FR4: Profile Editor WSL Tab | Phase 3 | Manual: Tab visible on Windows, dropdown works |
| FR5: Profile WSL Reference | Phase 1 | Unit test (TS-08) + Manual: profile saves wsl_distro_name |
| FR6: WSL PTY Session Launch | Phase 3 | Manual: WSL profile opens wsl.exe session |
| FR7: Platform Detection | Phase 1+2 | Manual: UI differs between Windows and Linux |

## E2E Testing (Docker)
- [ ] Existing E2E tests pass without regression (`./scripts/run-e2e-docker.sh`)

Note: WSL-specific E2E tests are not feasible in Docker (WSL unavailable in Linux containers).

## Manual Testing (E2E Not Possible)

### Windows-only tests
- [ ] Settings panel shows WSL category
- [ ] WSL section shows detected distributions from `wsl --list --quiet`
- [ ] Import button adds distribution to imported list
- [ ] Already-imported distribution has disabled Import button
- [ ] Delete button removes distribution from imported list
- [ ] Profile editor shows Shell | SSH | WSL tabs
- [ ] WSL tab shows dropdown with imported distributions
- [ ] WSL tab disabled when no distributions imported
- [ ] Saving from WSL tab creates profile with wsl_distro_name
- [ ] Tab switching clears other mode fields
- [ ] Editing WSL profile auto-selects WSL tab with correct dropdown value
- [ ] WSL profile launches `wsl.exe -d <distro>` in new tab
- [ ] WSL session accepts input and produces output
- [ ] Exiting WSL session behaves like normal shell exit
- [ ] Referenced distribution removed → error on launch attempt

### Linux-only tests
- [ ] Settings panel does NOT show WSL category
- [ ] Profile editor shows Shell | SSH tabs only (no WSL)

### Cross-platform tests
- [ ] Existing settings.json without WSL fields loads correctly
- [ ] Settings with WSL fields saves and loads round-trip

## Performance Verification
- WSL detection (`wsl.exe --list --quiet`): completes within 2 seconds

## Security Verification
- [ ] WSL args passed as array to CommandBuilder (no shell concatenation)
- [ ] No user credentials stored in WSL distribution entries

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Unit Tests | 10 | 10 | 0 | 0 |
| Build | 1 | 1 | 0 | 0 |
| Code Quality | 2 | 2 | 0 | 0 |
| E2E Regression | 1 | 0 | 1 | 0 |
| Windows UI | 15 | 0 | 0 | 15 |
| Linux UI | 2 | 0 | 0 | 2 |
| Cross-platform | 2 | 0 | 0 | 2 |
| Performance | 1 | 0 | 0 | 1 |
| Security | 2 | 0 | 0 | 2 |
| **Total** | **36** | **13** | **1** | **22** |
