# Feature: SSH Connection

## Overview

Add SSH connection management to eMterm, allowing users to connect to remote hosts via openssh command launched as a PTY session. The feature integrates with the existing profile system, enabling SSH connections from the + icon tab creation flow. SSH host configurations can be loaded from ~/.ssh/config (read-only) or manually added in eMterm settings.

## Objectives

- Auto-detect openssh command path on application startup
- Parse ~/.ssh/config to display available hosts (read-only)
- Provide CRUD operations for SSH connection entries in eMterm settings
- Extend profiles with an SSH connection reference field
- Launch SSH connections as PTY sessions in new tabs

## User Stories

### US1: Auto-detect SSH Command
As a user, I want eMterm to automatically find the ssh command on my system, so that I don't have to manually configure the path.

**Acceptance Criteria:**
- [ ] On startup, if ssh_command_path setting is empty, search PATH for ssh
- [ ] If found, auto-populate the ssh_command_path setting
- [ ] If not found, leave the field empty
- [ ] If ssh_command_path is already set, skip detection

### US2: View SSH Hosts from .ssh/config
As a user, I want to see my existing SSH hosts from ~/.ssh/config, so that I can reuse my existing SSH configuration.

**Acceptance Criteria:**
- [ ] On startup, parse ~/.ssh/config and extract Host names
- [ ] Display hosts as a read-only list in the SSH settings section
- [ ] Visually distinguish .ssh/config entries from eMterm entries
- [ ] Only load .ssh/config when ssh_command_path is set

### US3: Manage SSH Connections in eMterm
As a user, I want to add, edit, delete, and duplicate SSH connection entries, so that I can manage connections independently of .ssh/config.

**Acceptance Criteria:**
- [ ] Add new SSH connection via modal dialog
- [ ] Edit existing eMterm SSH connection via modal dialog
- [ ] Delete eMterm SSH connections
- [ ] Duplicate any entry (from .ssh/config or eMterm) into eMterm settings
- [ ] Validate hostname (required), port (1-65535), identity_file (exists if specified)

### US4: Connect via Profile
As a user, I want to associate an SSH connection with a profile and launch it from the + icon, so that I can connect to remote hosts with one click.

**Acceptance Criteria:**
- [ ] Profile editor shows an "SSH Connection" dropdown listing eMterm SSH entries
- [ ] When a profile with SSH connection is selected, launch ssh as PTY session in new tab
- [ ] SSH session disconnection behaves like normal shell exit
- [ ] If referenced SSH connection is deleted, show error on connection attempt

## Technical Requirements

### Functional Requirements
- **FR1: SSH Command Detection** - On startup, detect openssh binary path via PATH search. Linux: `which ssh`. Windows: `where ssh.exe` or check `C:\Windows\System32\OpenSSH\ssh.exe`.
- **FR2: SSH Config Parsing** - Parse `~/.ssh/config` on startup (when ssh_command_path is set). Extract `Host` directive values as a list of strings for display. Additionally, parse per-host directives (`Hostname`, `Port`, `User`, `IdentityFile`) to populate fields when importing. Ignore wildcards (`*`, `?`), `Host *` entries, and comment lines.
- **FR3: SSH Connection CRUD** - Store SSH connection entries in settings.json under `ssh_connections` array. Each entry: name, hostname, port, username, identity_file, ssh_options (array of {key, value} pairs). The ssh_options array is rendered as a dynamic Key=Value list with + button, and each entry is converted to `-o Key=Value` when building ssh args.
- **FR4: SSH Connection Duplication/Import** - Import .ssh/config entries into eMterm settings with all available fields populated (hostname from `Hostname` or host alias, port from `Port`, username from `User`, identity_file from `IdentityFile`). For eMterm entries, duplicate all fields including ssh_options.
- **FR5: Profile SSH Reference** - Add `ssh_connection_name: String` field to Profile struct. When non-empty, the profile launches ssh instead of the configured shell.
- **FR6: SSH PTY Session Launch** - Build ssh command args from SSH connection settings and launch via `PtySession::new()` with ssh binary as the shell path.
- **FR7: SSH Settings UI** - Add "SSH" category to settings panel between Profiles and the end. Contains ssh_command_path text input, .ssh/config host list (read-only), and eMterm SSH connection list (editable).

### Non-Functional Requirements
- **NFR1 - Performance:** SSH command detection completes within 1 second on startup. .ssh/config parsing completes within 1 second.
- **NFR2 - Security:** Passwords are never stored in settings. Private key contents are never read (only path is stored). .ssh/config is read for Host names only.
- **NFR3 - Platform Compatibility:** Support Linux (openssh-client) and Windows (Windows built-in OpenSSH). Git for Windows ssh is not a detection target.
- **NFR4 - Usability:** Auto-detection minimizes manual configuration. Modal dialogs match existing profile editor UX patterns.

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│ Frontend (TypeScript)                           │
│  ├─ SSH Settings Section (settings UI)          │
│  ├─ Profile Editor (ssh_connection_name field)  │
│  └─ Tab Creation (SSH profile handling)         │
├─────────────────────────────────────────────────┤
│ Tauri Commands (IPC)                            │
│  ├─ detect_ssh_command                          │
│  ├─ load_ssh_config_hosts                       │
│  └─ validate_identity_file                      │
├─────────────────────────────────────────────────┤
│ Backend (Rust)                                  │
│  ├─ ssh::detect  (SSH binary detection)         │
│  ├─ ssh::config  (.ssh/config parser)           │
│  └─ settings     (SshConnection struct)         │
├─────────────────────────────────────────────────┤
│ Existing Infrastructure                         │
│  ├─ PtySession   (reused for SSH sessions)      │
│  └─ Settings     (extended with SSH fields)     │
└─────────────────────────────────────────────────┘
```

### Data Flow

**SSH Command Detection (Startup):**
```
App Start → Check ssh_command_path setting
  → Empty: detect_ssh_command (Rust) → PATH search → Return path or empty
  → Non-empty: Skip detection
```

**SSH Config Loading (Startup):**
```
App Start → Check ssh_command_path is set
  → Set: load_ssh_config_hosts (Rust) → Read ~/.ssh/config → Extract Host names → Return list
  → Not set: Skip loading
```

**SSH Connection Launch:**
```
Profile Selected → Check ssh_connection_name
  → Non-empty: Lookup SshConnection by name
    → Build args: [-p port] [-i identity_file] [extra_options] [user@hostname]
    → PtySession::new(ssh_command_path, args, cols, rows)
    → New tab with SSH session
  → Empty: Normal shell launch (existing behavior)
```

### Tauri Commands

#### Command: detect_ssh_command

Detects the openssh binary path on the system.

**Signature:** `fn detect_ssh_command() -> Result<String, String>`

**Returns:** Full path to ssh binary, or empty string if not found.

**Platform behavior:**
- Linux: Search PATH for `ssh` binary
- Windows: Check `C:\Windows\System32\OpenSSH\ssh.exe`, then search PATH for `ssh.exe`

#### Command: load_ssh_config_hosts

Parses ~/.ssh/config and returns a list of Host entries with their directives.

**Signature:** `fn load_ssh_config_hosts() -> Result<Vec<SshConfigHost>, String>`

**Returns:** List of host entries from .ssh/config with parsed directives. Returns empty list if file does not exist.

```rust
pub struct SshConfigHost {
    pub host: String,       // Host alias
    pub hostname: String,   // Hostname directive value (or empty)
    pub port: u16,          // Port directive value (default: 22)
    pub user: String,       // User directive value (or empty)
    pub identity_file: String, // IdentityFile directive value (or empty)
}
```

**Parsing rules:**
- Extract values from `Host` directives
- For each Host block, parse `Hostname`, `Port`, `User`, `IdentityFile` directives
- Skip `Host *` (wildcard-only entries)
- Skip entries containing wildcards (`*`, `?`)
- Skip comment lines (starting with `#`)
- Handle multi-value Host lines (e.g., `Host foo bar` → two entries sharing the same directives)
- Directive keywords are case-insensitive

#### Command: validate_identity_file

Checks if the specified identity file exists.

**Signature:** `fn validate_identity_file(path: String) -> Result<bool, String>`

**Returns:** true if file exists, false otherwise. Expands `~` to home directory.

### Settings Schema Changes

#### Rust: AppSettings additions

```rust
// New structs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SshOption {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SshConnection {
    pub name: String,
    pub hostname: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub identity_file: String,
    #[serde(default)]
    pub ssh_options: Vec<SshOption>,
}

// AppSettings additions
pub struct AppSettings {
    // ... existing fields ...
    pub ssh_command_path: String,
    pub ssh_connections: Vec<SshConnection>,
}

// Profile additions
pub struct Profile {
    // ... existing fields ...
    pub ssh_connection_name: String,
}
```

#### TypeScript: AppSettings additions

```typescript
export interface SshOption {
  key: string;
  value: string;
}

export interface SshConnection {
  name: string;
  hostname: string;
  port: number;
  username: string;
  identity_file: string;
  ssh_options: SshOption[];
}

export interface AppSettings {
  // ... existing fields ...
  ssh_command_path: string;
  ssh_connections: SshConnection[];
}

export interface Profile {
  // ... existing fields ...
  ssh_connection_name: string;
}
```

### SSH Command Argument Construction

Build the ssh command arguments array from SshConnection:

```
args = []
if port != 22:         args.extend(["-p", port.to_string()])
if identity_file != "": args.extend(["-i", expanded_identity_file])
for opt in ssh_options: args.extend(["-o", format!("{}={}", opt.key, opt.value)])
if username != "":     args.push(format!("{}@{}", username, hostname))
else:                  args.push(hostname)
```

The ssh binary path comes from `ssh_command_path` setting, not from the Profile's `shell_path`.

### Dependencies

**Internal Dependencies:**
- Settings system (config module): Extended with SshConnection struct and new fields
- Profile system: Extended with ssh_connection_name field
- PTY session: Reused as-is for SSH process spawning
- Settings UI: New SSH section added
- Profile editor UI: New SSH connection dropdown added

**External Dependencies:**
- No new crate dependencies (file I/O and process spawning use std library)
- `home` crate (if not already present) for `~` expansion in identity_file path

### File Structure

```
src-tauri/src/
├── commands/
│   ├── ssh.rs              # Tauri commands: detect_ssh_command, load_ssh_config_hosts, validate_identity_file
│   └── mod.rs              # Add ssh module
├── ssh/
│   ├── mod.rs              # Module declarations
│   ├── detect.rs           # SSH binary detection (platform-specific)
│   └── config.rs           # .ssh/config parser
├── commands/config/
│   ├── settings.rs         # Add SshConnection, ssh_command_path, ssh_connections
│   └── types.rs            # Add SshConnection type if needed

src/
├── settings/
│   ├── types.ts            # Add SshConnection interface, update AppSettings and Profile
│   ├── settings-sections.ts # Add renderSshSection
│   └── settings-panel.ts   # Add SSH category
├── ssh/
│   └── ssh-editor.ts       # SSH connection editor modal dialog
├── profile/
│   └── profile-editor.ts   # Add ssh_connection_name dropdown
```

## Test Scenarios

### Unit Tests
- [ ] SSH detection returns valid path on Linux when ssh is installed
- [ ] SSH detection returns empty string when ssh is not in PATH
- [ ] SSH detection on Windows checks System32 path
- [ ] .ssh/config parser extracts Host names correctly
- [ ] .ssh/config parser skips `Host *` entries
- [ ] .ssh/config parser skips wildcard patterns
- [ ] .ssh/config parser handles comment lines
- [ ] .ssh/config parser handles multi-value Host lines
- [ ] .ssh/config parser returns empty list when file does not exist
- [ ] SshConnection serialization/deserialization with defaults
- [ ] SSH command argument construction with all fields
- [ ] SSH command argument construction with minimal fields (hostname only)
- [ ] SSH command argument construction with custom port
- [ ] SSH command argument construction with identity file
- [ ] SSH command argument construction with ssh_options (-o Key=Value)
- [ ] Validation: empty hostname rejected
- [ ] Validation: port 0 rejected, port 1 and 65535 accepted
- [ ] Validation: port 65536 rejected
- [ ] Identity file validation with ~ expansion

### Integration Tests
- [ ] Settings round-trip: save and load SSH connections
- [ ] Profile with ssh_connection_name persists correctly
- [ ] SSH connection duplication creates independent copy

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with WebdriverIO + tauri-driver
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression
- [ ] SSH settings section is visible in settings panel
- [ ] SSH connection add dialog opens and closes
- [ ] SSH command path field is displayed

### Edge Cases
- [ ] .ssh/config with no Host entries → empty list
- [ ] .ssh/config with only `Host *` → empty list
- [ ] .ssh/config with Include directives → not followed (only parse main file)
- [ ] SSH connection name conflict on duplication → append suffix (e.g., "name (copy)")
- [ ] Profile references deleted SSH connection → error message on connection attempt
- [ ] ssh_command_path set to invalid/non-existent binary → error on connection attempt
- [ ] .ssh/config with mixed indentation and spacing → robust parsing
- [ ] Empty extra_options field → no extra args passed

## Security Considerations

- **Password Storage:** Passwords are never stored. openssh handles password prompts directly in the terminal.
- **Private Key Protection:** Only the file path is stored in settings. eMterm never reads private key contents.
- **Input Validation:** Hostname, port, and identity_file are validated before use.
- **Command Injection Prevention:** SSH arguments are passed as array elements to CommandBuilder, not as a shell-concatenated string.
- **.ssh/config Access:** Read-only access, Host names only extracted.

## Error Handling

### Error Scenarios

| Scenario | Handling | User Message |
|----------|----------|--------------|
| SSH binary not found on PATH | Leave ssh_command_path empty | (No message - field stays empty) |
| ~/.ssh/config does not exist | Return empty host list | (No message - list stays empty) |
| ~/.ssh/config parse error | Log warning, return empty list | (No message - graceful degradation) |
| Invalid hostname (empty) | Validation error in dialog | "Hostname is required" |
| Invalid port (out of range) | Validation error in dialog | "Port must be between 1 and 65535" |
| Identity file not found | Validation error in dialog | "Identity file not found: {path}" |
| Referenced SSH connection deleted | Error on connection attempt | "SSH connection '{name}' not found" |
| SSH process fails to start | PTY error | "Failed to start SSH: {error}" |
| SSH connection refused | openssh displays error in terminal | (Handled by openssh output) |

## Success Criteria

- [ ] All functional requirements (FR1-FR7) are implemented and tested
- [ ] All unit test scenarios pass
- [ ] Existing E2E tests pass without regression
- [ ] SSH settings UI matches existing settings patterns (modal dialogs, component styles)
- [ ] Linux and Windows platforms supported
- [ ] Settings migration: existing settings.json loads correctly with new fields defaulting

## Open Questions

> **Note**: No unresolved requirements. All questions were clarified during the specification phase.

## Implementation Phases

### Phase 1: Backend Foundation
**Goals:** SSH detection, .ssh/config parsing, settings schema changes
**Deliverables:**
- SSH detection module (platform-specific)
- .ssh/config parser
- SshConnection struct and settings schema additions
- Tauri commands (detect_ssh_command, load_ssh_config_hosts, validate_identity_file)
- Unit tests for all backend modules

### Phase 2: Frontend SSH Settings UI
**Goals:** SSH settings category in settings panel
**Deliverables:**
- SSH settings section with command path input
- .ssh/config host list display (read-only) with Import button that fills all fields
- eMterm SSH connection list (CRUD + duplicate)
- SSH connection editor modal dialog with dynamic Key=Value options (+ button)

### Phase 3: Profile Integration & Connection
**Goals:** Connect profiles to SSH settings and launch SSH sessions
**Deliverables:**
- Profile struct extension (ssh_connection_name)
- Profile editor UI update (SSH connection dropdown)
- SSH PTY session launch logic
- Integration tests

## References

- Existing settings: `src-tauri/src/commands/config/settings.rs`
- Existing profile types: `src/settings/types.ts` (Profile interface)
- PTY session: `src-tauri/src/pty/session.rs`
- Shell detection: `src-tauri/src/pty/shell.rs`
- Settings sections: `src/settings/settings-sections.ts`
- Profile editor: `src/profile/profile-editor.ts`
