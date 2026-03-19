# Feature: WSL Profile Support

## Overview

Add WSL (Windows Subsystem for Linux) distribution detection, import, and profile integration to eMterm. On Windows, a new WSL section appears in settings to detect and import installed WSL distributions. The profile editor gains a WSL tab (Shell | SSH | WSL) for creating WSL-based profiles that launch `wsl.exe -d <distro>` as a PTY session. All WSL-related UI is hidden on Linux.

## Objectives

- Detect installed WSL distributions via `wsl.exe --list --quiet`
- Allow importing detected distributions into settings
- Add WSL tab to profile editor for creating WSL profiles
- Launch WSL sessions as PTY processes via `wsl.exe`
- Show WSL UI only on Windows

## User Stories

### US1: Import WSL Distribution
As a Windows user, I want to see my installed WSL distributions and import them, so that I can easily create profiles for them.

**Acceptance Criteria:**
- [ ] WSL section in settings shows detected distributions with Import button
- [ ] Imported distributions appear in the imported list with delete button
- [ ] Already-imported distributions have disabled Import button
- [ ] WSL section is hidden on Linux

### US2: Create WSL Profile
As a user, I want to select an imported WSL distribution in the profile editor WSL tab, so that I can create a profile that launches that distribution.

**Acceptance Criteria:**
- [ ] Profile editor shows Shell | SSH | WSL tabs on Windows
- [ ] WSL tab shows dropdown of imported distributions
- [ ] WSL tab is disabled when no distributions are imported
- [ ] Saving from WSL tab sets `wsl_distro_name` and clears shell/SSH fields

### US3: Launch WSL Session
As a user, I want to open a new tab with a WSL profile, so that I can work in my WSL environment.

**Acceptance Criteria:**
- [ ] Selecting a WSL profile launches `wsl.exe -d <distro_name>` as PTY session
- [ ] Session behaves like a normal terminal session (input/output, resize, exit)
- [ ] If referenced distribution is removed from imports, show error on launch

## Technical Requirements

### Functional Requirements
- **FR1: WSL Distribution Detection** - Execute `wsl.exe --list --quiet` and parse output to return distribution names. Windows only (`#[cfg(windows)]`). Returns empty list if WSL is not installed or command fails.
- **FR2: WSL Distribution Import** - Store imported distributions in `wsl_distributions: Vec<WslDistribution>` in settings.json. Each entry has only a `name` field (the distribution name).
- **FR3: WSL Settings Section** - Add WSL category to settings panel (after SSH). Windows only. Contains: detected distributions list (read-only + Import), imported distributions list (+ Delete).
- **FR4: Profile Editor WSL Tab** - Add WSL tab to profile editor on Windows (Shell | SSH | WSL). Dropdown to select from imported distributions. Mutually exclusive with Shell and SSH tabs.
- **FR5: Profile WSL Reference** - Add `wsl_distro_name: String` field to Profile struct. When non-empty, the profile launches WSL instead of shell or SSH.
- **FR6: WSL PTY Session Launch** - Build command: `wsl.exe -d <distro_name>`. Launch via `PtySession::new()` with `wsl.exe` as shell path and `["-d", distro_name]` as args.
- **FR7: Platform Detection** - Frontend determines whether to show WSL UI. Use Tauri command or existing platform detection to check if running on Windows.

### Non-Functional Requirements
- **NFR1 - Performance:** WSL detection completes within 2 seconds.
- **NFR2 - Security:** `wsl.exe` arguments passed as array to CommandBuilder (no shell concatenation).
- **NFR3 - Platform Compatibility:** Windows 10/11 WSL1 and WSL2. Linux: all WSL UI hidden.
- **NFR4 - Usability:** Follows SSH connection UX patterns. Consistent with existing settings and profile editor design.
- **NFR5 - i18n:** All new labels have en/ja translations.

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│ Frontend (TypeScript)                           │
│  ├─ WSL Settings Section (Windows only)         │
│  ├─ Profile Editor WSL Tab (Windows only)       │
│  └─ WSL Session Launch Logic                    │
├─────────────────────────────────────────────────┤
│ Tauri Commands (IPC)                            │
│  ├─ detect_wsl_distributions (Windows only)     │
│  └─ get_platform                                │
├─────────────────────────────────────────────────┤
│ Backend (Rust)                                  │
│  ├─ wsl::detect  (WSL distribution detection)   │
│  └─ settings     (WslDistribution struct)       │
├─────────────────────────────────────────────────┤
│ Existing Infrastructure                         │
│  ├─ PtySession   (reused for WSL sessions)      │
│  └─ Settings     (extended with WSL fields)     │
└─────────────────────────────────────────────────┘
```

### Data Flow

**WSL Distribution Detection:**
```
Settings WSL section opened
  → detect_wsl_distributions (Rust, Windows only)
  → Execute: wsl.exe --list --quiet
  → Parse stdout lines → filter empty → Return Vec<String>
```

**WSL Import:**
```
User clicks Import on detected distro
  → Add WslDistribution { name } to settings.wsl_distributions
  → Save settings
  → Refresh UI (disable Import button for imported distro)
```

**WSL Session Launch:**
```
Profile selected → Check wsl_distro_name
  → Non-empty: Lookup WslDistribution by name
    → shell_path = "wsl.exe"
    → shell_args = ["-d", distro_name]
    → PtySession::new(shell_path, args, cols, rows)
    → New tab with WSL session
  → Empty: Check ssh_connection_name or shell (existing behavior)
```

### Tauri Commands

#### Command: detect_wsl_distributions

Detects installed WSL distributions.

**Signature:** `fn detect_wsl_distributions() -> Result<Vec<String>, String>`

**Returns:** List of distribution names. Empty list if WSL is not available.

**Implementation:**
- Execute `wsl.exe --list --quiet`
- Parse stdout: split by newlines, trim whitespace, filter empty lines
- Note: `wsl --list --quiet` output may be UTF-16LE encoded on Windows; handle encoding
- On Linux: this command is not registered (gated by `#[cfg(windows)]`)

#### Command: get_platform (if not already available)

Returns the current platform identifier.

**Signature:** `fn get_platform() -> String`

**Returns:** `"windows"`, `"linux"`, etc.

### Settings Schema Changes

#### Rust: AppSettings additions

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WslDistribution {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub name: String,
}

pub struct AppSettings {
    // ... existing fields ...
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub wsl_distributions: Vec<WslDistribution>,
}

pub struct Profile {
    // ... existing fields ...
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub wsl_distro_name: String,
}
```

#### TypeScript: AppSettings additions

```typescript
export interface WslDistribution {
  name: string;
}

export interface AppSettings {
  // ... existing fields ...
  wsl_distributions: WslDistribution[];
}

export interface Profile {
  // ... existing fields ...
  wsl_distro_name: string;
}
```

### WSL Command Argument Construction

```
shell_path = "wsl.exe"
args = ["-d", distro_name]
```

Launched via existing `PtySession::new()` with these as shell path and args.

### Dependencies

**Internal Dependencies:**
- Settings system: Extended with `WslDistribution` struct and `wsl_distributions` field
- Profile system: Extended with `wsl_distro_name` field
- PTY session: Reused as-is for WSL process spawning
- Settings UI: New WSL section (mirrors SSH section pattern)
- Profile editor UI: New WSL tab (mirrors SSH tab pattern)

**External Dependencies:**
- No new crate or npm dependencies

### File Structure

```
src-tauri/src/
├── commands/
│   └── wsl.rs              # NEW: Tauri command detect_wsl_distributions
├── wsl/
│   ├── mod.rs              # NEW: Module declarations
│   └── detect.rs           # NEW: WSL distribution detection (Windows only)
├── commands/config/
│   └── settings.rs         # MODIFIED: Add WslDistribution, wsl_distributions, wsl_distro_name

src/
├── settings/
│   ├── types.ts            # MODIFIED: Add WslDistribution, update AppSettings and Profile
│   └── sections/
│       └── wsl-section.ts  # NEW: WSL settings section renderer
├── settings/
│   └── settings-panel.ts   # MODIFIED: Add WSL category (Windows only)
├── profile/
│   └── profile-editor.ts   # MODIFIED: Add WSL tab (Windows only)
├── tab-bar/
│   └── tab-bar-ui.ts       # MODIFIED: Add WSL profile launch logic
├── i18n/locales/
│   ├── en.json             # MODIFIED: Add WSL labels
│   └── ja.json             # MODIFIED: Add WSL labels
```

## Test Scenarios

### Unit Tests
- [ ] WSL detection parses `wsl --list --quiet` output correctly
- [ ] WSL detection handles UTF-16LE encoded output
- [ ] WSL detection returns empty list when WSL is not installed
- [ ] WSL detection returns empty list on command execution failure
- [ ] WSL detection filters empty lines and whitespace
- [ ] WslDistribution serialization/deserialization with defaults
- [ ] Profile with wsl_distro_name serialization round-trip
- [ ] Settings migration: existing settings.json loads with wsl_distributions defaulting to empty

### Integration Tests
- [ ] Settings round-trip: save and load wsl_distributions
- [ ] Profile with wsl_distro_name persists correctly
- [ ] WSL detection command executes successfully on Windows

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with WebdriverIO + tauri-driver
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] WSL not installed → empty detection list, empty section message
- [ ] No distributions installed → empty detection list
- [ ] Distribution already imported → Import button disabled
- [ ] Profile references removed distribution → error message on launch
- [ ] `wsl.exe --list --quiet` outputs BOM or UTF-16LE → handled in parsing
- [ ] Distribution name with spaces → handled correctly in args array

## Security Considerations

- **Command Injection Prevention:** WSL arguments passed as array elements to CommandBuilder, not shell-concatenated.
- **No Credential Storage:** WSL distributions are accessed via the local system; no passwords or keys involved.

## Error Handling

| Scenario | Handling | User Message |
|----------|----------|--------------|
| WSL not installed | Return empty list | (Empty state message in section) |
| `wsl --list` fails | Return empty list, log warning | (Empty state message) |
| Distribution already imported | Disable Import button | (Button disabled) |
| Referenced distribution removed | Error on launch | "WSL distribution '{name}' not found in imported list" |
| `wsl.exe` fails to start | PTY error | "Failed to start WSL: {error}" |

## Success Criteria

- [ ] WSL distributions detected and displayed on Windows
- [ ] Import/delete workflow functions correctly
- [ ] Profile editor shows Shell | SSH | WSL tabs on Windows
- [ ] WSL profile launches `wsl.exe -d <distro>` in new tab
- [ ] Linux environment shows no WSL-related UI
- [ ] All existing tests pass without regression
- [ ] TypeScript typecheck passes
- [ ] i18n labels for both English and Japanese

## Open Questions

> **Note**: No unresolved requirements.

## References

- SSH connection spec: `doc/tasks/ssh-connection/SPEC.md` (reference implementation pattern)
- Profile shell/SSH tabs spec: `doc/tasks/profile-shell-ssh-tabs/SPEC.md` (tab UI pattern)
- Existing settings: `src-tauri/src/commands/config/settings.rs`
- Profile types: `src/settings/types.ts`
- SSH settings section: `src/settings/sections/ssh-section.ts` (UI pattern reference)
