# Feature: Terminal Profiles

## Overview

Terminal profiles allow users to define named sets of shell-related settings (shell path, arguments, environment variables, working directory) and select a profile when creating new tabs. This enables quick switching between different shell environments without manually changing global settings.

## Objectives

- Enable users to define multiple named shell configurations
- Integrate profile selection into the tab creation workflow
- Maintain full backward compatibility with existing global settings
- Provide a foundation for future SSH client integration

## User Stories

### US1: Create and Manage Profiles
As a user, I want to create named shell profiles with different shell paths, arguments, environment variables, and working directories, so that I can quickly switch between different shell environments.

**Acceptance Criteria:**
- [ ] Can create a new profile with name, shell_path, shell_args, env_vars, working_directory
- [ ] Can edit an existing profile
- [ ] Can delete a profile
- [ ] Can duplicate a profile
- [ ] Can reorder profiles via drag and drop
- [ ] Can set one profile as default

### US2: Open Tabs with Profiles
As a user, I want to open new tabs using a specific profile, so that each tab starts with the correct shell environment.

**Acceptance Criteria:**
- [ ] + button opens tab with global settings when no profiles exist
- [ ] + button shows selection dialog (MD3 select with "Global Settings" + profiles) when profiles exist
- [ ] + button dialog pre-selects default profile if set, otherwise "Global Settings"
- [ ] Ctrl+Shift+T always opens default profile (or global settings if none)
- [ ] New keybind opens profile selector modal
- [ ] Settings page launch button opens tab with that profile

### US3: Backward Compatibility
As an existing user, I want the terminal to behave exactly as before if I don't use profiles, so that the update doesn't break my workflow.

**Acceptance Criteria:**
- [ ] No profiles defined → all tab creation uses global shell_path/shell_args
- [ ] Existing settings.json without profiles field loads without error
- [ ] Global shell_path and shell_args settings remain in the settings UI

## Technical Requirements

### Functional Requirements
- **FR1: Profile Data Model** — Define a `Profile` struct/interface with fields: name, shell_path, shell_args, env_vars, working_directory, is_default. Store as `profiles: Vec<Profile>` in `AppSettings`.
- **FR2: Profile CRUD** — Implement create, read, update, delete operations for profiles through the existing settings save mechanism.
- **FR3: Profile Duplication** — Duplicate an existing profile with a modified name (e.g., "Profile Name (Copy)").
- **FR4: Profile Reordering** — Support drag-and-drop reordering in the settings UI. Persist order in the profiles array.
- **FR5: Default Profile Flag** — Allow exactly one profile to be marked as default. Setting a new default clears the previous one.
- **FR6: Profile Selector Modal** — Display a modal overlay with a simple list of profile names. Support keyboard navigation (arrow keys, Enter, Escape).
- **FR7: Tab Creation Integration** — Modify tab creation flow to use profile settings when available. Apply shell_path, shell_args, env_vars, and working_directory to PTY spawning.
- **FR8: Environment Variable Parsing** — Parse env_vars text (KEY=VALUE per line) into a key-value map for PTY environment setup.
- **FR9: New Keybind for Selector** — Add a configurable keybind (e.g., `profile_selector`) to open the profile selector modal.
- **FR10: Settings UI Launch Button** — Add a launch (▶) button per profile in the settings UI to open a new tab with that profile.

### Non-Functional Requirements
- **NFR1 - Performance:** Selector modal must appear within 100ms of trigger.
- **NFR2 - Compatibility:** Existing settings.json files without `profiles` field must load without error using `serde(default)`.
- **NFR3 - Consistency:** Follow existing settings patterns: `serde(default)` + `deserialize_null_default` in Rust, mirrored `AppSettings` interface in TypeScript.

## Implementation Approach

### Architecture

**Data Flow:**
```
User Action (+ button / keybind)
  → TabManager.createTab(profileId?)
    → If profile specified:
        → Read profile from AppSettings
        → Pass shell config to PTY spawn
    → Else:
        → Use global shell_path/shell_args (current behavior)
  → PtyClient.spawn(shellPath, shellArgs, envVars, workingDir)
    → Rust PTY command receives profile-specific parameters
```

**Component Diagram:**
```
┌─────────────────────────────────────────────────┐
│ Settings Panel (Terminal > Profiles section)     │
│  ├─ Profile List (drag-reorderable)             │
│  ├─ Profile Edit Dialog (modal)                 │
│  └─ Launch Button (▶) per profile               │
├─────────────────────────────────────────────────┤
│ Profile Selector Modal                          │
│  └─ Simple list with keyboard navigation        │
├─────────────────────────────────────────────────┤
│ Tab Manager                                     │
│  └─ createTab() extended with profile support   │
├─────────────────────────────────────────────────┤
│ PTY Client (TypeScript)                         │
│  └─ spawn() extended with env_vars, working_dir │
├─────────────────────────────────────────────────┤
│ PTY Commands (Rust)                             │
│  └─ pty_create extended with env/cwd params     │
└─────────────────────────────────────────────────┘
```

### Data Model

**Rust (`AppSettings` extension):**
```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Profile {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_path: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_args: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub env_vars: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub working_directory: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub is_default: bool,
}

// In AppSettings:
pub profiles: Vec<Profile>,  // serde(default)
```

**TypeScript (`AppSettings` extension):**
```typescript
interface Profile {
  name: string;
  shell_path: string;
  shell_args: string[];
  env_vars: string;
  working_directory: string;
  is_default: boolean;
}

// In AppSettings:
profiles: Profile[];
```

**Storage format in settings.json:**
```json
{
  "profiles": [
    {
      "name": "Default Shell",
      "shell_path": "",
      "shell_args": [],
      "env_vars": "",
      "working_directory": "",
      "is_default": true
    },
    {
      "name": "Fish Shell",
      "shell_path": "/usr/bin/fish",
      "shell_args": [],
      "env_vars": "TERM=xterm-256color\nFISH_FEATURES=qmark-noglob",
      "working_directory": "~/projects",
      "is_default": false
    }
  ]
}
```

### Tab Creation Logic

**+ Button behavior:**
```
if profiles.length === 0:
    createTab()  // current behavior, global settings
else:
    showSelectionDialog()  // MD3 select with "Global Settings" + profiles
    // Pre-selects default profile if set, otherwise "Global Settings"
    // User confirms with "Open" button
```

**Ctrl+Shift+T behavior:**
```
if profiles.some(p => p.is_default):
    createTab(defaultProfile)
else:
    createTab()  // global settings
```

**Profile selector keybind:**
```
if profiles.length > 0:
    showProfileSelectorModal()
// else: no-op
```

### PTY Spawn Extension

The existing `pty_create` Tauri command needs to accept optional profile parameters:

```rust
#[tauri::command]
pub fn pty_create(
    // existing params...
    shell_path: Option<String>,
    shell_args: Option<Vec<String>>,
    env_vars: Option<HashMap<String, String>>,
    working_directory: Option<String>,
) -> Result<String, String>
```

When profile parameters are provided, they override global settings for that specific PTY session.

### Dependencies

**Internal Dependencies:**
- Settings system (`config.rs`, `settings-service.ts`, `settings-applier.ts`)
- Tab management (`tab-manager.ts`, `tab-bar-ui.ts`)
- PTY commands (`src-tauri/src/commands/pty.rs`)
- Keybind system (`define_keybinds!` macro, `keyboard-handler.ts`)

**External Dependencies:**
- None (uses existing crate/package dependencies)

### File Structure

**Modified files:**
```
src-tauri/src/commands/config.rs    # Profile struct, AppSettings extension
src-tauri/src/commands/pty.rs       # PTY spawn with profile params
src/settings/types.ts               # Profile interface, AppSettings extension
src/settings/settings-sections.ts   # Profile management UI section
src/settings/settings-panel.ts      # Profile category registration
src/tab-bar/tab-manager.ts          # createTab with profile support
src/tab-bar/tab-bar-ui.ts           # + button behavior change
src/tab-bar/types.ts                # CreateTabOptions extension
src/i18n/locales/en.json            # English labels
src/i18n/locales/ja.json            # Japanese labels
src-tauri/locales/en.json           # Rust-side i18n
src-tauri/locales/ja.json           # Rust-side i18n
```

**New files:**
```
src/profile/profile-selector.ts     # Profile selector modal component
src/profile/profile-editor.ts       # Profile edit dialog component
src/profile/types.ts                # Profile-specific types (if needed)
```

## Test Scenarios

### Unit Tests
- [ ] Profile struct serialization/deserialization (Rust)
- [ ] Default profile resolution (empty profiles → null, one default → that profile)
- [ ] Environment variable parsing (valid KEY=VALUE, empty lines, malformed lines)
- [ ] Default flag exclusivity (setting new default clears old one)
- [ ] Profile validation (empty name rejection)

### Integration Tests
- [ ] Settings load with profiles field present
- [ ] Settings load without profiles field (backward compatibility)
- [ ] PTY spawn with profile-specific shell_path and shell_args
- [ ] PTY spawn with profile-specific environment variables
- [ ] PTY spawn with profile-specific working directory

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with `./scripts/run-e2e-docker.sh`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression
- [ ] Profile CRUD operations in settings UI
- [ ] Profile selector modal appears and allows selection
- [ ] Tab created with correct profile settings

### Edge Cases
- [ ] Zero profiles: all creation paths use global settings
- [ ] Single profile marked as default: + button dialog pre-selects it
- [ ] All profiles deleted while selector is open: modal closes gracefully
- [ ] Profile with empty shell_path: uses system default shell
- [ ] Profile with non-existent shell_path: PTY spawn error handling
- [ ] Profile with non-existent working_directory: fallback to home
- [ ] env_vars with empty lines or comments: ignored during parsing
- [ ] Duplicate profile names: allowed (no uniqueness constraint)

## Error Handling

### Error Cases

| Error | Condition | Handling |
|-------|-----------|----------|
| Invalid shell path | Profile specifies non-existent shell | Show error notification, same as current shell error behavior |
| Invalid working directory | Directory does not exist | Fall back to home directory |
| Malformed env_vars | Lines without `=` separator | Skip malformed lines, log warning |
| Empty profile name | User tries to save without name | Disable save button, show validation message |

## Security Considerations

- **Input Validation:** Profile name length limit. Shell path validated on PTY spawn (not on save).
- **Environment Variables:** No special sanitization beyond KEY=VALUE parsing. Users are trusted with their own environment.
- **XSS Prevention:** Profile names rendered as text content, not innerHTML.

## Success Criteria

- [ ] All functional requirements (FR1-FR10) are implemented and tested
- [ ] All test scenarios pass
- [ ] Selector modal appears within 100ms
- [ ] Backward compatibility with existing settings.json
- [ ] No regression in existing E2E tests
- [ ] Code follows existing project patterns and conventions

## Open Questions

> **Note**: Unresolved requirements are tracked as `status: tbd` in sdd.yaml.
> Resolve before running `/sdd.2-create-plan`.

- [ ] FR9: Default keybind for profile selector (to be decided during implementation planning)

## References

- Requirements document: `doc/tasks/terminal-profiles/要件定義書.md`
- Tabby terminal profile system (reference implementation)
- Existing settings pattern: `src-tauri/src/commands/config.rs`
