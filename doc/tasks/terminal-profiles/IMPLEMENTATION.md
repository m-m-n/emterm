# Implementation Plan: Terminal Profiles

## Overview

Add named terminal profiles (shell path, arguments, environment variables, working directory) to eMterm, enabling users to define multiple shell configurations and select a profile when creating new tabs.

## Objectives

- Enable users to define, edit, duplicate, reorder, and delete named shell profiles
- Integrate profile selection into the tab creation workflow (+ button, keybind, settings launch)
- Extend PTY spawn to accept per-session environment variables and working directory
- Maintain full backward compatibility with existing settings.json files

## Prerequisites

### Development Environment

- Rust toolchain (stable)
- Bun (package manager / bundler)
- Tauri CLI
- Docker (for testing)

### Dependencies

- No new external dependencies required
- Internal: settings system, tab management, PTY subsystem, keybind system, i18n

## Architecture Overview

### Technology Stack

- **Language**: Rust (backend) + TypeScript (frontend)
- **Framework**: Tauri v2
- **Key Libraries**: serde (serialization), portable-pty (PTY management)

### Design Approach

Bottom-up, incremental delivery:
1. Data model and persistence (Rust + TypeScript)
2. PTY spawn extension (Rust backend + TypeScript client)
3. Settings UI for profile management
4. Tab creation integration and profile selector modal
5. Keybind registration

Each phase produces a working, testable increment. Profiles are stored as a flat array within the existing `AppSettings` structure, following the established `serde(default)` + `deserialize_null_default` pattern.

### Component Interaction

```
Settings UI (profile CRUD) --> AppSettings (profiles array) --> settings.json
                                       |
Tab Bar UI (+ button) -----> TabManager.createTab(profileId?)
                                       |
                              Profile resolution logic
                                       |
                              PtyClient.spawn(options) --> pty_spawn command
                                       |
                              PtySession::new(shell, args, env, cwd)
```

The profile selector modal is a standalone DOM component, triggered by the + button (when no default exists) or by the `Ctrl+Shift+P` keybind. It communicates the selected profile ID back to TabManager for tab creation.

## Implementation Phases

### Phase 1: Data Model and Persistence

**Goal**: Define the Profile data structure in Rust and TypeScript, persist profiles in settings.json, and verify backward compatibility with existing settings files.

**Files to Modify**:
- `src-tauri/src/commands/config.rs` - Add Profile struct and profiles field to AppSettings
- `src/settings/types.ts` - Add Profile interface and profiles field to AppSettings
- `src-tauri/locales/en.json` - Add validation messages for profiles
- `src-tauri/locales/ja.json` - Add validation messages for profiles

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Profile (Rust) | Serializable profile data with null-safe defaults | None | All fields have serde defaults; missing profiles field yields empty array |
| Profile (TypeScript) | Mirror of Rust struct for frontend use | None | Interface matches Rust serialization exactly |
| validate_settings | Validate profile names are non-empty | AppSettings loaded | Invalid profiles rejected with i18n error message |

**Processing Flow**:
1. User saves settings containing profiles
2. Settings serialized to JSON
   - profiles array present -> deserialized as-is
   - profiles field absent -> defaults to empty array (backward compatibility)
   - individual profile field null -> defaults via deserialize_null_default
3. Validation rejects empty profile names

**Implementation Steps**:
1. **Define Profile struct in Rust** - Add struct with name, shell_path, shell_args, env_vars, working_directory, is_default fields, all with serde(default) + deserialize_null_default
2. **Add profiles field to AppSettings** - Add `profiles: Vec<Profile>` with serde(default) + deserialize_null_default
3. **Mirror in TypeScript** - Add Profile interface and profiles field to AppSettings interface
4. **Add profile validation** - Validate non-empty profile names in validate_settings using t!() macro
5. **Add i18n entries for validation** - Add profile-related validation messages to Rust locale files

**Dependencies**: None (foundation phase)

**Testing Approach**:
- Unit: Profile serialization/deserialization roundtrip, backward compatibility with missing profiles field, null field handling, validation of empty names
- Integration: Settings load/save with profiles present and absent

**Acceptance Criteria**:
- [ ] Profile struct serializes/deserializes correctly
- [ ] Existing settings.json without profiles field loads without error
- [ ] Empty profile name rejected by validation
- [ ] TypeScript interface matches Rust struct serialization

**Estimated Effort**: small

---

### Phase 2: PTY Spawn Extension

**Goal**: Extend PTY session creation to accept optional environment variables and working directory, enabling per-profile shell configuration.

**Files to Modify**:
- `src-tauri/src/pty/session.rs` - Accept env_vars and working_directory parameters
- `src-tauri/src/pty/manager.rs` - Pass new parameters through create_session_atomic
- `src-tauri/src/lib.rs` - Add env_vars and working_directory params to pty_spawn command
- `src/types/pty.ts` - Extend PtySpawnOptions with env_vars and working_directory
- `src/pty/client.ts` - Pass new options to pty_spawn invoke call

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| PtySession::new | Create PTY with optional env vars and cwd | Valid shell path | Shell spawned with specified env vars merged and cwd set |
| PtyManager::create_session_atomic | Forward env/cwd params to PtySession | None | Session created with profile-specific configuration |
| pty_spawn command | Accept optional env_vars (key-value map) and working_directory | None | Parameters forwarded to PtyManager |
| PtyClient.spawn | Send extended options to backend | None | pty_spawn called with env_vars and working_directory if provided |

**Processing Flow**:
1. PtyClient.spawn called with optional env_vars (key-value map) and working_directory
2. Backend pty_spawn receives parameters
3. PtyManager.create_session_atomic forwards to PtySession::new
4. PtySession::new applies configuration:
   - env_vars present -> merge each key-value pair into command environment
   - working_directory present and non-empty -> set command working directory
   - working_directory absent or invalid -> use default behavior (inherit parent)

**Implementation Steps**:
1. **Extend PtySession::new signature** - Add optional env_vars (key-value map) and working_directory parameters
2. **Apply env vars to command builder** - Iterate env_vars map and set each key-value pair on the command
3. **Apply working directory** - Set command working directory if provided and non-empty
4. **Update PtyManager** - Pass new parameters through create_session_atomic
5. **Update pty_spawn Tauri command** - Accept new optional parameters
6. **Update TypeScript types and client** - Extend PtySpawnOptions and PtyClient.spawn

**Dependencies**: Requires Phase 1 (Profile struct defined)

**Testing Approach**:
- Unit: PtySession creation with env vars set, PtySession creation with working directory set, PtySession creation with neither (backward compatible)
- Integration: Spawn PTY with profile-specific parameters, verify env vars visible in shell

**Acceptance Criteria**:
- [ ] PTY spawns with custom environment variables
- [ ] PTY spawns with custom working directory
- [ ] PTY spawns correctly with no profile parameters (backward compatible)
- [ ] Invalid working directory handled gracefully (falls back to default)

**Estimated Effort**: medium

---

### Phase 3: Profile Management Settings UI

**Goal**: Add a "Profiles" section to the settings panel where users can create, edit, duplicate, reorder, and delete profiles, and launch tabs from profiles.

**Files to Create**:
- `src/profile/profile-editor.ts` - Profile edit dialog (modal form for profile fields)
- `src/profile/types.ts` - Profile-related frontend types and helpers (env var parsing, default flag management)

**Files to Modify**:
- `src/settings/settings-sections.ts` - Add renderProfilesSection function
- `src/settings/settings-panel.ts` - Register "profiles" category in navigation
- `src/i18n/locales/en.json` - Add profile-related UI labels
- `src/i18n/locales/ja.json` - Add profile-related UI labels

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderProfilesSection | Render profile list with action buttons (edit, duplicate, delete, launch, set default) | Settings loaded with profiles array | Profile list displayed with drag-reorder support |
| ProfileEditor | Modal dialog for creating/editing a profile | Profile data (or empty for new) | User can fill in name, shell_path, shell_args, env_vars, working_directory |
| parseEnvVars | Parse multi-line KEY=VALUE text into key-value map | Raw env_vars string | Map of valid key-value pairs; malformed lines skipped |
| ensureSingleDefault | Enforce at most one default profile | Profiles array | Exactly zero or one profile has is_default=true |

**Processing Flow**:
1. User navigates to Profiles category in settings
2. Profile list rendered from settings.profiles array
   - Each item shows name, shell path summary, default badge, action buttons
   - Drag handle enables reordering
3. User clicks "Add Profile" button
   - Profile editor modal opens with empty fields
   - On save: validate name non-empty, add to profiles array, save settings
4. User clicks edit button on existing profile
   - Profile editor modal opens pre-filled
   - On save: update profile in array, save settings
5. User clicks duplicate button
   - New profile created with "(Copy)" appended to name
   - Immediately added to array and saved
6. User clicks delete button
   - Profile removed from array, settings saved
7. User clicks default toggle
   - Previous default cleared, new default set, settings saved
8. User clicks launch button
   - Trigger tab creation with this profile (delegates to TabManager)

**Implementation Steps**:
1. **Create profile types module** - Define env var parser, default flag enforcer, profile duplication helper
2. **Create profile editor component** - Modal form with fields for name, shell_path, shell_args (comma-separated), env_vars (textarea), working_directory; save/cancel buttons
3. **Add profiles section renderer** - Profile list with per-item action buttons (edit, duplicate, delete, set default, launch), "Add Profile" button at top
4. **Implement drag-and-drop reorder** - Drag handle on each profile item, reorder profiles array on drop, save settings
5. **Register profiles category** - Add "profiles" to settings panel categories array, wire up rendering in switch statement
6. **Add i18n labels** - Add all profile-related labels to en.json and ja.json

**Dependencies**: Requires Phase 1 (Profile data model)

**Testing Approach**:
- Unit: Env var parsing (valid pairs, empty lines, malformed lines, lines with = in value), default flag exclusivity, profile duplication naming
- E2E (Docker): Profile CRUD operations in settings UI, drag reorder
- Manual: Visual appearance, UX flow smoothness

**Acceptance Criteria**:
- [ ] Can create, edit, duplicate, and delete profiles in settings UI
- [ ] Drag-and-drop reordering works and persists
- [ ] Default flag toggle enforces single-default constraint
- [ ] Launch button opens new tab with profile settings
- [ ] Profile editor validates non-empty name before save

**Estimated Effort**: large

---

### Phase 4: Tab Creation Integration and Profile Selector

**Goal**: Modify tab creation flow to use profiles when available, add profile selector modal, and wire up the + button behavior change.

**Files to Create**:
- `src/profile/profile-selector.ts` - Profile selector modal overlay with keyboard navigation

**Files to Modify**:
- `src/tab-bar/tab-manager.ts` - Extend createTab to accept profile configuration
- `src/tab-bar/tab-bar-ui.ts` - Change + button behavior based on profile existence
- `src/tab-bar/types.ts` - Extend CreateTabOptions with profile fields
- `src/terminal-app/index.ts` - Accept profile parameters for PTY spawn
- `src/main.ts` - Wire profile resolution into createTerminalApp factory

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ProfileSelector | Modal overlay listing profiles with keyboard nav | Profiles array non-empty | User selects a profile or cancels; returns selected profile or null |
| TabManager.createTab | Accept optional profile config (shell, args, env, cwd) | None | Tab created with profile-specific PTY configuration |
| TabBarUI.handleNewTabClick | Determine + button behavior based on profiles | Settings loaded | Correct action taken: direct create, show selector, or global settings |
| TerminalApp.init | Accept optional spawn options for profile | None | PTY spawned with provided profile parameters or global settings |

**Processing Flow**:

**+ button click**:
1. Load current profiles from settings
2. No profiles exist -> create tab with global settings (current behavior)
3. Profiles exist with a default -> create tab with default profile
4. Profiles exist without a default -> show profile selector modal

**Ctrl+Shift+T (new tab keybind)**:
1. Load current profiles
2. Default profile exists -> create tab with default profile
3. No default -> create tab with global settings

**Ctrl+Shift+P (profile selector keybind)**:
1. Load current profiles
2. Profiles exist -> show profile selector modal
3. No profiles -> no-op

**Profile selector modal**:
1. Display overlay with profile name list
2. Keyboard: arrow keys navigate, Enter selects, Escape cancels
3. Mouse: click selects
4. On select -> create tab with selected profile
5. On cancel -> close modal, no action

**Implementation Steps**:
1. **Extend CreateTabOptions** - Add optional shell_path, shell_args, env_vars (key-value map), working_directory fields
2. **Update TabManager.createTab** - Pass profile config through to TerminalApp creation
3. **Update TerminalApp.init** - Accept and use optional spawn parameters, falling back to global settings
4. **Create profile selector modal** - Overlay component with accessible keyboard navigation (arrow keys, Enter, Escape), focus trap, profile list rendering
5. **Update + button behavior** - Implement three-way branching based on profile state
6. **Wire up main.ts** - Pass profile parameters through createTerminalApp factory callback

**Dependencies**: Requires Phase 2 (PTY spawn extension), Phase 3 (Profile UI for test data)

**Testing Approach**:
- Unit: Tab creation with profile config, + button behavior branching logic
- E2E (Docker): Profile selector modal appears, keyboard navigation works, tab created with correct profile
- Manual: Visual appearance of selector modal, UX responsiveness

**Acceptance Criteria**:
- [ ] + button opens default profile directly when default is set
- [ ] + button shows selector when profiles exist but no default
- [ ] + button uses global settings when no profiles exist
- [ ] Profile selector modal supports keyboard navigation
- [ ] Selector modal appears within 100ms (NFR1)
- [ ] Tab created with profile-specific shell, args, env vars, and working directory

**Estimated Effort**: large

---

### Phase 5: Keybind Registration

**Goal**: Add configurable keybind `profile_selector` (default: `Ctrl+Shift+P`) to open the profile selector modal, and update Ctrl+Shift+T to respect default profile.

**Files to Modify**:
- `src-tauri/src/commands/config.rs` - Add profile_selector to define_keybinds! macro
- `src/settings/types.ts` - Add profile_selector to KeybindSettings interface
- `src/tab-bar/keyboard-handler.ts` - Handle profile_selector keybind
- `src/settings/settings-sections.ts` - Add profile_selector to keybinds section UI
- `src/i18n/locales/en.json` - Add keybind label
- `src/i18n/locales/ja.json` - Add keybind label

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| KeybindSettings.profile_selector | Store configurable keybind for profile selector | None | Default "Ctrl+Shift+P", configurable via settings |
| TabKeyboardHandler | Handle profile_selector keybind | Settings loaded | Opens profile selector modal when pressed |
| Keybinds section UI | Display profile_selector in keybinds settings | None | User can customize the keybind |

**Processing Flow**:
1. User presses Ctrl+Shift+P (or custom keybind)
2. TabKeyboardHandler matches against profile_selector keybind
3. If profiles exist -> open profile selector modal
4. If no profiles -> no-op

**Ctrl+Shift+T behavior update**:
1. User presses Ctrl+Shift+T
2. TabKeyboardHandler matches against new_tab keybind
3. If default profile exists -> create tab with default profile
4. If no default profile -> create tab with global settings (current behavior preserved)

**Implementation Steps**:
1. **Add keybind to Rust define_keybinds! macro** - Add profile_selector entry with default "Ctrl+Shift+P"
2. **Mirror in TypeScript KeybindSettings** - Add profile_selector field
3. **Handle in TabKeyboardHandler** - Add profile_selector match, trigger profile selector modal via callback
4. **Update new_tab handler** - Modify to check for default profile before creating tab
5. **Add to keybinds settings UI** - Render profile_selector in Tab Management subsection

**Dependencies**: Requires Phase 4 (Profile selector modal exists)

**Testing Approach**:
- Unit: Keybind matching for profile_selector, new_tab with default profile resolution
- E2E (Docker): Keybind opens selector modal
- Manual: Keybind customization in settings UI

**Acceptance Criteria**:
- [ ] Ctrl+Shift+P opens profile selector when profiles exist
- [ ] Ctrl+Shift+P is no-op when no profiles exist
- [ ] Ctrl+Shift+T creates tab with default profile when one exists
- [ ] Ctrl+Shift+T creates tab with global settings when no default profile
- [ ] profile_selector keybind is customizable in settings UI

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/
  src/
    commands/config.rs        # Profile struct, profiles field in AppSettings, validation
    pty/session.rs            # Extended PtySession::new with env_vars and working_directory
    pty/manager.rs            # Forward new params in create_session_atomic
    lib.rs                    # Extended pty_spawn command parameters
  locales/
    en.json                   # Profile validation messages
    ja.json                   # Profile validation messages

src/
  profile/                    # NEW directory
    profile-selector.ts       # Profile selector modal overlay
    profile-editor.ts         # Profile edit dialog component
    types.ts                  # Profile helpers (env var parser, default management)
  settings/
    types.ts                  # Profile interface, profiles field, profile_selector keybind
    settings-sections.ts      # Profile management section, profile_selector keybind UI
    settings-panel.ts         # "profiles" category registration
  tab-bar/
    tab-manager.ts            # createTab with profile config
    tab-bar-ui.ts             # + button behavior change
    types.ts                  # CreateTabOptions extension
    keyboard-handler.ts       # profile_selector keybind handler
  terminal-app/
    index.ts                  # Accept profile spawn options
  pty/
    client.ts                 # Extended spawn options
  types/
    pty.ts                    # PtySpawnOptions extension
  main.ts                     # Wire profile resolution into factory
  i18n/locales/
    en.json                   # Profile UI labels and keybind labels
    ja.json                   # Profile UI labels and keybind labels
```

## Testing Strategy

- **Unit**: Core logic (env var parsing, default flag management, profile validation, serialization) -- target 90%+ coverage
- **Integration**: Settings load/save with profiles, PTY spawn with profile parameters
- **E2E (Docker)**: Profile CRUD in settings, selector modal interaction, tab creation with profiles, keybind handling, backward compatibility (no profiles scenario)
- **Manual**: Visual appearance, drag-and-drop UX, selector modal responsiveness, overall workflow coherence

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none) | - | No new external dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Drag-and-drop reorder complexity | Medium | Medium | Use simple DOM-based drag events; no external library needed since tab bar already implements drag reorder |
| Profile selector modal focus management | Medium | Low | Follow existing modal patterns (paste dialog, image viewer) for focus trap and Escape handling |
| Backward compatibility regression | Low | High | Extensive serde default tests; existing settings.json roundtrip tests |
| Environment variable injection on Windows | Low | Low | Users are trusted with their own env; same trust model as shell_path |

## Open Questions

- (none -- all TBD items resolved)

## Success Metrics

- [ ] All functional requirements (FR1-FR10) implemented and tested
- [ ] All test scenarios pass (unit, integration, E2E)
- [ ] Profile selector modal appears within 100ms of trigger (NFR1)
- [ ] Existing settings.json without profiles field loads without error (NFR2)
- [ ] Code follows existing project patterns (NFR3)
- [ ] No regression in existing E2E tests
