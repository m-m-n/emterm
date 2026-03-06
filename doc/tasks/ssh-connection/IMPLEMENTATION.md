# Implementation Plan: SSH Connection

## Overview

Add SSH connection management to eMterm, enabling users to connect to remote hosts via openssh launched as a PTY session. Integrates with the existing profile system, settings UI, and tab creation flow.

## Objectives

- Auto-detect openssh command path on application startup
- Parse ~/.ssh/config to display available hosts with per-host directives (read-only)
- Provide CRUD operations for SSH connection entries in eMterm settings
- Extend profiles with an SSH connection reference field
- Launch SSH connections as PTY sessions in new tabs

## Prerequisites

### Development Environment

- Rust toolchain (stable)
- Bun package manager
- Docker (for testing)

### Dependencies

- No new external crate dependencies (std library covers file I/O, PATH search)
- Existing `portable-pty` for SSH process spawning
- Existing Tauri IPC infrastructure

## Architecture Overview

### Technology Stack

- **Backend**: Rust (Tauri) - SSH detection, config parsing, settings, PTY launch
- **Frontend**: Vanilla TypeScript - SSH settings UI, editor modal, profile integration
- **IPC**: Tauri commands (synchronous for detection, async for config loading)

### Design Approach

SSH connections reuse the existing PTY infrastructure. Instead of spawning a shell, `pty_spawn` receives the ssh binary path and constructed arguments. The backend provides detection and parsing utilities; the frontend handles UI and orchestrates the connection flow.

### Component Interaction

```
Frontend (Tab Creation)
  |-- Profile with ssh_connection_name
  |-- Lookup SshConnection from settings
  |-- Build ssh args
  |-- Call pty_spawn(ssh_path, args, ...)
  v
Backend (PTY Manager)
  |-- Spawns ssh process as PTY session
  |-- Reader thread handles output
  v
Existing Terminal Rendering (unchanged)
```

## Current Implementation Status

Significant groundwork already exists from a prior implementation attempt. The plan below addresses remaining gaps and corrections.

**Already implemented:**
- `src-tauri/src/ssh/detect.rs` - SSH binary detection with platform support, arg builder, tilde expansion
- `src-tauri/src/ssh/config.rs` - Basic .ssh/config parser (host names only)
- `src-tauri/src/commands/ssh.rs` - Tauri commands (detect, load hosts, validate identity file)
- Settings schema: SshConnection struct, ssh_command_path, ssh_connections fields
- Frontend types: SshConnection, Profile.ssh_connection_name
- SSH settings section: command path input, detect button, host list, CRUD, drag-reorder
- SSH editor modal: create/edit SSH connections
- Profile editor: SSH connection dropdown
- Backend validation: SSH connection name/hostname/port checks
- i18n keys for SSH-related UI strings
- Tauri command registration in app.rs

**Gaps requiring implementation:**
1. Config parser returns only host names; needs per-host directive parsing (FR2, FR4)
2. SshConnection uses `extra_options: String`; SPEC requires `ssh_options: Vec<SshOption>` with dynamic key-value UI (FR3)
3. Import from .ssh/config doesn't populate per-host fields (FR4)
4. SSH PTY session launch not wired into tab creation flow (FR6)
5. Auto-detection on startup not implemented (FR1 startup behavior)
6. Config parser keywords are case-sensitive; SPEC says case-insensitive (FR2)

## Implementation Phases

### Phase 1: Config Parser Enhancement (FR2)

**Goal**: Extend .ssh/config parser to return per-host directives and make directive matching case-insensitive.

**Files to Modify**:
- `src-tauri/src/ssh/config.rs` - Add SshConfigHost struct, parse per-host directives
- `src-tauri/src/commands/ssh.rs` - Update load_ssh_config_hosts return type

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| SshConfigHost | Data struct for parsed host block | N/A | Contains host, hostname, port, user, identity_file |
| parse_ssh_config_hosts | Parse .ssh/config into SshConfigHost list | File path provided | Returns list of hosts with all parsed directives |
| load_ssh_config_hosts command | Return parsed hosts to frontend | ssh_command_path may or may not be set | Returns Vec of SshConfigHost |

**Processing Flow**:
1. Read file content line by line
2. On `Host` line (case-insensitive) -> start new host block, extract host aliases
   - Skip wildcard-containing aliases
3. On directive line (Hostname, Port, User, IdentityFile, case-insensitive) -> associate with current host block
4. Return collected host blocks

**Implementation Steps**:
1. **Define SshConfigHost struct** - host alias, hostname, port (default 22), user, identity_file fields with serialization
2. **Refactor parser to track per-host state** - maintain current host block while parsing directives, case-insensitive keyword matching
3. **Update Tauri command signature** - change return type from Vec of String to Vec of SshConfigHost
4. **Update existing unit tests** - verify directive extraction, case-insensitive matching
5. **Add new test cases** - per-host directive parsing, defaults, mixed case keywords

**Dependencies**: None (standalone backend change)

**Testing Approach**:
- Unit: parse content with various directive combinations, case variations, multi-host blocks
- Integration: N/A (pure parsing logic)

**Acceptance Criteria**:
- [ ] Parser extracts Hostname, Port, User, IdentityFile per host block
- [ ] Directive keywords matched case-insensitively
- [ ] Existing host-name-only tests still pass
- [ ] Default port 22 when Port directive absent

**Estimated Effort**: small

---

### Phase 2: SshConnection Schema Migration (FR3)

**Goal**: Replace `extra_options: String` with `ssh_options: Vec<SshOption>` structured key-value pairs, maintaining backward compatibility.

**Files to Modify**:
- `src-tauri/src/commands/config/settings.rs` - Add SshOption struct, change SshConnection.extra_options to ssh_options
- `src-tauri/src/ssh/detect.rs` - Update build_ssh_args to accept ssh_options
- `src/settings/types.ts` - Update SshConnection interface
- `src/ssh/ssh-editor.ts` - Replace extra_options text input with dynamic key-value list
- `src/settings/settings-sections.ts` - Update SSH section rendering for new field
- `src-tauri/src/commands/config/mod.rs` - Update tests for new field

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| SshOption | Key-value pair for -o options | N/A | Serializable struct with key, value fields |
| SshConnection.ssh_options | Structured option storage | N/A | Replaces extra_options string field |
| SSH editor key-value UI | Dynamic list with add/remove | Editor modal open | User can add/remove -o options as key=value pairs |
| build_ssh_args | Convert ssh_options to -o args | Valid SshConnection fields | Each option becomes "-o Key=Value" argument |

**Processing Flow**:
1. SshOption entries stored as array of {key, value} objects
2. Each entry rendered as two text inputs (key, value) with a remove button
3. "+" button adds new empty entry
4. On save, filter out entries with empty key
5. build_ssh_args converts each entry to "-o Key=Value"

**Implementation Steps**:
1. **Add SshOption struct and migrate SshConnection** - replace extra_options with ssh_options Vec, add backward-compatible deserialization
2. **Update build_ssh_args** - accept structured options, generate -o Key=Value pairs
3. **Update TypeScript types** - SshConnection interface, SshOption interface
4. **Implement dynamic key-value UI in SSH editor** - add/remove rows for ssh_options
5. **Update settings section and duplication logic** - handle new field structure
6. **Update all tests** - Rust unit tests, TypeScript types

**Dependencies**: None (can be done independently of Phase 1)

**Testing Approach**:
- Unit: SshOption serialization/deserialization, build_ssh_args with structured options
- Unit: backward compatibility - loading settings.json with old extra_options field
- Manual: SSH editor key-value UI interaction

**Acceptance Criteria**:
- [ ] SshConnection uses ssh_options Vec of SshOption
- [ ] Old settings.json with extra_options loads without error (backward compat)
- [ ] SSH editor shows dynamic key-value list with + button
- [ ] build_ssh_args produces correct -o Key=Value arguments

**Estimated Effort**: medium

---

### Phase 3: Import Enhancement (FR4)

**Goal**: Import from .ssh/config populates all available fields (hostname, port, user, identity_file) using parsed directives from Phase 1.

**Files to Modify**:
- `src/settings/settings-sections.ts` - Update import button handler to use SshConfigHost fields
- `src/settings/types.ts` - Add SshConfigHost interface for frontend

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| SshConfigHost (TS) | Frontend type matching Rust struct | Phase 1 complete | Type-safe access to parsed host directives |
| Import handler | Create SshConnection from SshConfigHost | Host list loaded with directives | New connection has all available fields populated |

**Processing Flow**:
1. load_ssh_config_hosts returns SshConfigHost array with all directives
2. Import button creates SshConnection with fields populated from SshConfigHost
   - name = host alias
   - hostname = SshConfigHost.hostname (or host alias if empty)
   - port = SshConfigHost.port
   - username = SshConfigHost.user
   - identity_file = SshConfigHost.identity_file

**Implementation Steps**:
1. **Add SshConfigHost TypeScript interface** - mirror Rust SshConfigHost struct
2. **Update host list rendering** - show additional info (hostname, user) when available
3. **Update import handler** - populate all fields from parsed directives
4. **Update Tauri invoke type** - change from string array to SshConfigHost array

**Dependencies**: Requires Phase 1 (config parser enhancement)

**Testing Approach**:
- Manual: import .ssh/config entry, verify all fields populated in editor

**Acceptance Criteria**:
- [ ] Import creates connection with hostname, port, user, identity_file from config
- [ ] Host list shows connection details (hostname, user if available)

**Estimated Effort**: small

---

### Phase 4: SSH Session Launch (FR5, FR6)

**Goal**: Wire SSH connections into the tab creation flow so profiles with ssh_connection_name launch ssh instead of a shell.

**Files to Modify**:
- Frontend module handling `profile:launch` event and + tab creation - Add SSH launch path

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Tab creation logic | Detect SSH profile and launch appropriately | Profile selected with ssh_connection_name | SSH process spawned as PTY session |
| SSH arg builder (frontend) | Construct ssh args from SshConnection | SshConnection looked up by name | Complete argument array for ssh binary |
| Error handling | Handle missing/deleted SSH connection | Profile references non-existent connection | User-visible error message |

**Processing Flow**:
1. User selects profile (via + menu or profile:launch event)
2. Check if profile has ssh_connection_name
   - Empty -> normal shell launch (existing behavior)
   - Non-empty -> look up SshConnection by name in settings
3. Connection not found -> show error message
4. ssh_command_path empty -> show error message
5. Build ssh arguments from connection fields
6. Call pty_spawn with ssh_command_path as shell, constructed args
7. New tab opens with SSH session

**Implementation Steps**:
1. **Identify tab creation entry point** - locate where pty_spawn is called with profile settings
2. **Add SSH connection lookup** - find SshConnection by name from current settings
3. **Add SSH arg construction on frontend** - translate SshConnection fields to ssh command args
4. **Pass ssh_command_path and args to pty_spawn** - override shell/args when SSH profile detected
5. **Add error handling** - connection not found, ssh_command_path empty

**Dependencies**: Requires Phase 2 (for ssh_options in arg construction)

**Testing Approach**:
- Integration: verify pty_spawn receives correct ssh path and args
- E2E (Docker): SSH settings section visible, SSH profile creation flow
- Manual: actual SSH connection (requires accessible SSH server)

**Acceptance Criteria**:
- [ ] Profile with ssh_connection_name launches ssh as PTY session
- [ ] SSH session disconnect behaves like normal shell exit
- [ ] Missing SSH connection shows error message
- [ ] Empty ssh_command_path shows error message

**Estimated Effort**: medium

---

### Phase 5: Startup Auto-Detection (FR1)

**Goal**: Auto-detect ssh command path on application startup when setting is empty.

**Files to Modify**:
- Frontend app initialization module - Add startup detection call

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Startup detector | Check and populate ssh_command_path | App starting, settings loaded | ssh_command_path populated if ssh found |

**Processing Flow**:
1. App starts, settings loaded
2. Check ssh_command_path
   - Non-empty -> skip detection
   - Empty -> call detect_ssh_command
3. If detected -> save to settings
4. If not found -> leave empty (no error shown)

**Implementation Steps**:
1. **Add startup detection call** - after settings load, check and detect
2. **Save detected path** - persist to settings if found
3. **Ensure single execution** - only run on startup, not on every settings reload

**Dependencies**: None (detect_ssh_command already exists)

**Testing Approach**:
- Unit: detection logic already tested in ssh/detect.rs
- Manual: verify on startup with empty ssh_command_path

**Acceptance Criteria**:
- [ ] ssh_command_path auto-populated on first launch when ssh is available
- [ ] No detection when ssh_command_path already set
- [ ] No error when ssh not found

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
  ssh/
    mod.rs              - Module declarations (exists)
    detect.rs           - SSH detection, arg builder, tilde expansion (exists, modify for ssh_options)
    config.rs           - .ssh/config parser (exists, modify for per-host directives)
  commands/
    ssh.rs              - Tauri commands (exists, modify return type)
    config/
      settings.rs       - SshConnection, SshOption structs (exists, modify schema)
      validation.rs     - SSH validation (exists)
      mod.rs            - Config module (exists)

src/
  settings/
    types.ts            - SshConnection, SshOption, SshConfigHost interfaces (exists, modify)
    settings-sections.ts - SSH section renderer (exists, modify import handler)
    settings-panel.ts   - Settings panel with SSH category (exists, no changes needed)
  ssh/
    ssh-editor.ts       - SSH connection editor modal (exists, modify for key-value UI)
  profile/
    profile-editor.ts   - Profile editor with SSH dropdown (exists, no changes needed)
    types.ts            - Profile helpers (exists, no changes needed)
```

## Testing Strategy

- **Unit (Rust)**: Config parser directives, SshOption serialization, build_ssh_args, backward compat - target 90%+ on ssh/ module
- **Unit (TypeScript)**: Type checks via typecheck command
- **Integration**: Settings round-trip with new schema
- **E2E (Docker)**: SSH settings section visible, editor opens, existing tests pass
- **Manual**: Actual SSH connection, startup auto-detection, import from .ssh/config

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none) | - | No new dependencies required |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Backward compatibility with extra_options field | Medium | High | Add serde alias/migration for old field name |
| .ssh/config parsing edge cases | Low | Low | Graceful degradation - return empty on parse errors |
| SSH process spawning on Windows | Low | Medium | Use same PTY infrastructure, test on both platforms |

## Open Questions

- None. All requirements clarified during specification phase.

## Success Metrics

- [ ] All functional requirements (FR1-FR7) implemented
- [ ] All unit test scenarios pass
- [ ] Existing E2E tests pass without regression
- [ ] SSH settings UI matches existing patterns
- [ ] Linux and Windows supported
- [ ] Settings migration: old settings.json loads correctly with new fields
