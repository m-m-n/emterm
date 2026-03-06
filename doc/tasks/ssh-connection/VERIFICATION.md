# Verification Document: SSH Connection

## Overview

**Feature**: SSH Connection
**SPEC.md**: `doc/tasks/ssh-connection/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/ssh-connection/IMPLEMENTATION.md`

## Build Verification

- Command: `bun tauri build`
- Expected: exit code 0, no errors

## Test Verification

### Rust Tests

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c 'cargo test --manifest-path src-tauri/Cargo.toml'`
- Coverage target: minimum 80%, target 90% on ssh/ module

### TypeScript Tests

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c 'bun test'`

### TypeScript Typecheck

- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c 'bun run typecheck'`

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | SSH detection returns valid path on Linux when ssh is installed | Non-empty path string containing "ssh" | Unit |
| TS-02 | SSH detection returns empty string when ssh is not in PATH | Empty string | Unit |
| TS-03 | SSH detection on Windows checks System32 path | System32 path checked first, then PATH | Unit |
| TS-04 | .ssh/config parser extracts Host names correctly | List of host names | Unit |
| TS-05 | .ssh/config parser skips `Host *` entries | Wildcard entries excluded | Unit |
| TS-06 | .ssh/config parser skips wildcard patterns | Entries with `*` or `?` excluded | Unit |
| TS-07 | .ssh/config parser handles comment lines | Comment lines ignored | Unit |
| TS-08 | .ssh/config parser handles multi-value Host lines | Each value becomes separate entry | Unit |
| TS-09 | .ssh/config parser returns empty list when file does not exist | Empty list, no error | Unit |
| TS-10 | .ssh/config parser extracts per-host directives (Hostname, Port, User, IdentityFile) | SshConfigHost with all fields populated | Unit |
| TS-11 | .ssh/config parser directive keywords case-insensitive | "hostname", "HOSTNAME", "HostName" all work | Unit |
| TS-12 | SshConnection serialization/deserialization with defaults | Default port 22, empty strings for optional fields | Unit |
| TS-13 | SSH command argument construction with all fields | Correct arg array with -p, -i, -o, user@host | Unit |
| TS-14 | SSH command argument construction with minimal fields (hostname only) | Single-element array ["hostname"] | Unit |
| TS-15 | SSH command argument construction with custom port | ["-p", "port", "hostname"] | Unit |
| TS-16 | SSH command argument construction with identity file | ["-i", "path", "hostname"] | Unit |
| TS-17 | SSH command argument construction with ssh_options (-o Key=Value) | ["-o", "Key=Value", "hostname"] | Unit |
| TS-18 | Validation: empty hostname rejected | Error returned | Unit |
| TS-19 | Validation: port 0 rejected, port 1 and 65535 accepted | Port 0 error, 1 and 65535 pass | Unit |
| TS-20 | Validation: port 65536 rejected | Error returned (frontend validation) | Unit |
| TS-21 | Identity file validation with ~ expansion | ~ expanded to home dir before check | Unit |
| TS-22 | Settings round-trip: save and load SSH connections | Connections preserved through save/load cycle | Integration |
| TS-23 | Profile with ssh_connection_name persists correctly | Field preserved in settings.json | Integration |
| TS-24 | SSH connection duplication creates independent copy | Copy has "(Copy)" suffix, fields copied | Integration |
| TS-25 | Backward compatibility: settings with extra_options field loads correctly | No deserialization error, field migrated or ignored | Unit |

## Code Quality Verification

- Format (Rust): `cargo fmt --manifest-path src-tauri/Cargo.toml`
- Format check: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

## File Structure Verification

### Files to Modify

- `src-tauri/src/ssh/config.rs` - Add SshConfigHost struct, per-host directive parsing, case-insensitive matching
- `src-tauri/src/ssh/detect.rs` - Update build_ssh_args for ssh_options Vec
- `src-tauri/src/commands/ssh.rs` - Update load_ssh_config_hosts return type to Vec of SshConfigHost
- `src-tauri/src/commands/config/settings.rs` - Add SshOption struct, replace extra_options with ssh_options
- `src-tauri/src/commands/config/validation.rs` - Update if needed for new field structure
- `src-tauri/src/commands/config/mod.rs` - Update tests
- `src/settings/types.ts` - Add SshOption, SshConfigHost interfaces, update SshConnection
- `src/settings/settings-sections.ts` - Update import handler for SshConfigHost fields
- `src/ssh/ssh-editor.ts` - Replace extra_options input with dynamic key-value list
- Frontend tab creation module - Add SSH launch path

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All functional requirements (FR1-FR7) implemented and tested | Run Rust and TypeScript test suites; all pass |
| SC-02 | All unit test scenarios pass | Run test commands in Docker |
| SC-03 | Existing E2E tests pass without regression | Run `./scripts/run-e2e-docker.sh` |
| SC-04 | SSH settings UI matches existing settings patterns | Manual visual comparison with profile editor |
| SC-05 | Linux and Windows platforms supported | Platform-gated code review, CI on both platforms |
| SC-06 | Settings migration: existing settings.json loads correctly | Unit test with old-format JSON |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: SSH Command Detection | Phase 5 | Unit tests (TS-01, TS-02, TS-03); manual startup test |
| FR2: SSH Config Parsing | Phase 1 | Unit tests (TS-04 through TS-11); manual with real .ssh/config |
| FR3: SSH Connection CRUD | Phase 2 | Unit tests (TS-12, TS-18, TS-19); manual UI test |
| FR4: SSH Connection Duplication/Import | Phase 3 | Unit test (TS-24); manual import test |
| FR5: Profile SSH Reference | Phase 4 | Integration test (TS-23); manual profile editor test |
| FR6: SSH PTY Session Launch | Phase 4 | Manual test with SSH server |
| FR7: SSH Settings UI | Already implemented | E2E test; manual visual verification |

## E2E Testing (Docker)

- [ ] Existing E2E tests pass without regression (`./scripts/run-e2e-docker.sh`)
- [ ] SSH settings section is visible in settings panel
- [ ] SSH connection add dialog opens and closes
- [ ] SSH command path field is displayed

## Manual Testing (E2E Not Possible)

- [ ] Auto-detect ssh command path on startup (empty -> detected)
- [ ] Auto-detect skipped when ssh_command_path already set
- [ ] Import .ssh/config entry populates all fields (hostname, port, user, identity_file)
- [ ] SSH editor: add/remove key-value option pairs via dynamic UI
- [ ] Create profile with SSH connection, launch from + menu -> SSH session opens
- [ ] SSH session: type commands, receive output, disconnect (exit) closes tab normally
- [ ] Profile references deleted SSH connection -> error message shown
- [ ] Empty ssh_command_path with SSH profile -> error message shown
- [ ] .ssh/config with only `Host *` -> empty host list displayed
- [ ] .ssh/config with Include directives -> not followed (only main file parsed)
- [ ] SSH connection name conflict on duplication -> "(Copy)" suffix added
- [ ] Settings with old extra_options format loads without error

## Performance Verification

- SSH command detection completes within 1 second on startup (NFR1)
- .ssh/config parsing completes within 1 second (NFR1)

## Security Verification

- [ ] Passwords are never stored in settings (NFR2)
- [ ] Private key file contents are never read, only path stored (NFR2)
- [ ] SSH arguments passed as array elements, not shell-concatenated string (command injection prevention)
- [ ] .ssh/config access is read-only
- [ ] Input validation: hostname required, port range 1-65535, identity file existence checked

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Unit Tests | 25 | 25 | 0 | 0 |
| E2E Tests | 4 | 0 | 4 | 0 |
| Manual Tests | 12 | 0 | 0 | 12 |
| Performance | 2 | 0 | 0 | 2 |
| Security | 5 | 0 | 0 | 5 |
| **Total** | **48** | **25** | **4** | **19** |

## Actual Test Results

### Rust Tests
- **Result**: All passed (494 tests, 0 failed, 3 ignored)
- SSH-specific: 52 tests passed (24 config + 20 detect + 8 settings/validation)

### TypeScript Tests
- **Result**: 1840 pass, 1 fail (pre-existing TabDragHandler test, unrelated to SSH)
- TypeScript typecheck: pass (no errors)

### Code Quality
- `cargo fmt`: applied, no issues

### Known Issues
- 1 pre-existing test failure in `TabDragHandler > drag start > settings tab cannot be dragged` (not related to SSH changes)
