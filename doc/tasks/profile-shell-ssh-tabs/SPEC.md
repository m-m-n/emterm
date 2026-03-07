# Feature: Profile Editor SHELL/SSH Tab UI

## Overview

Add a SHELL | SSH tab switcher inside the profile editor modal to clearly separate local shell configuration from SSH connection selection. The two modes are mutually exclusive: a profile uses either a local shell or an SSH connection.

## Objectives

- Provide clear visual separation between shell and SSH profile types
- Enforce mutual exclusivity between shell settings and SSH connection
- Disable the SSH tab when no SSH connections are registered

## User Stories

### US1: Create a local shell profile
As a user, I want to configure shell path, args, env vars, and working directory under the SHELL tab, so that I can create a local shell profile.

**Acceptance Criteria:**
- [ ] SHELL tab is selected by default for new profiles
- [ ] Shell fields (path, args, env vars, working directory) are displayed under SHELL tab
- [ ] Saving clears ssh_connection_name

### US2: Create an SSH profile
As a user, I want to select a registered SSH connection from a dropdown under the SSH tab, so that I can create an SSH-based profile.

**Acceptance Criteria:**
- [ ] SSH tab shows a dropdown of registered SSH connections
- [ ] Saving sets ssh_connection_name and clears shell_path, shell_args, env_vars, working_directory

### US3: Edit an existing SSH profile
As a user, I want the SSH tab to be automatically selected when editing a profile that has ssh_connection_name set.

**Acceptance Criteria:**
- [ ] SSH tab is pre-selected when editing a profile with non-empty ssh_connection_name
- [ ] The correct SSH connection is pre-selected in the dropdown

## Technical Requirements

### Functional Requirements
- **FR1:** Add SHELL/SSH tab bar inside profile editor modal, below the name field and above the form fields
- **FR2:** SHELL tab displays: shell_path, shell_args, env_vars, working_directory fields (existing fields)
- **FR3:** SSH tab displays: a single dropdown to select from registered SSH connections
- **FR4:** Tab switching clears the other mode's values (SHELL fields or ssh_connection_name)
- **FR5:** Disable SSH tab when ssh_connections array is empty
- **FR6:** Auto-select tab based on existing profile's ssh_connection_name (non-empty = SSH tab, empty = SHELL tab)

### Non-Functional Requirements
- **NFR1 - Accessibility:** Tab bar uses ARIA tablist/tab/tabpanel roles with keyboard navigation (arrow keys)
- **NFR2 - Design consistency:** Tab styling follows `doc/UI-DESIGN-GUIDELINES.yaml` tokens
- **NFR3 - i18n:** All new labels have en/ja translations

## Implementation Approach

### Architecture

The change is scoped entirely to the frontend. No backend (Rust) changes are needed.

```
profile-editor.ts  ── Modified: add tab UI, restructure form fields
settings-panel.css ── Modified: add tab styles for profile editor modal
i18n/locales/      ── Modified: add new translation keys
```

### Component Structure

```
Profile Editor Modal
├── Title (h2)
├── Error message area
├── Name field (always visible)
├── Tab bar [SHELL | SSH]          ← NEW
│   ├── SHELL tab panel            ← Contains existing fields
│   │   ├── Shell path
│   │   ├── Shell args
│   │   ├── Env vars
│   │   └── Working directory
│   └── SSH tab panel              ← Contains dropdown
│       └── SSH connection dropdown
└── Button row [Cancel | Save]
```

### Tab State Logic

```
On modal open:
  1. Load SSH connections from settings
  2. If ssh_connections.length === 0 → disable SSH tab
  3. If editing profile with ssh_connection_name → select SSH tab
  4. Otherwise → select SHELL tab

On tab switch to SHELL:
  - Show shell fields, hide SSH dropdown
  - Clear ssh_connection_name value

On tab switch to SSH:
  - Show SSH dropdown, hide shell fields
  - Clear shell_path, shell_args, env_vars, working_directory values

On save:
  - Read values only from the active tab
  - Other mode's fields are already cleared by tab switch
```

### Data Flow

```
User clicks tab → Update active tab state → Show/hide panels → Clear inactive values
User clicks Save → Read profile name + active tab fields → Create Profile object → onSave callback
```

### CSS Classes (new)

| Class | Purpose |
|-------|---------|
| `.profile-editor-tabs` | Tab bar container (flexbox) |
| `.profile-editor-tab` | Individual tab button |
| `.profile-editor-tab.active` | Active tab state |
| `.profile-editor-tab.disabled` | Disabled tab state |
| `.profile-editor-tab-panel` | Tab panel container |

### i18n Keys (new)

| Key | EN | JA |
|-----|----|----|
| `settings.profiles.tabShell` | Shell | Shell |
| `settings.profiles.tabSsh` | SSH | SSH |

### Dependencies

**Internal Dependencies:**
- `src/profile/profile-editor.ts`: Primary file to modify
- `src/styles/settings-panel.css`: Add tab styles
- `src/i18n/locales/en.json`: Add English translations
- `src/i18n/locales/ja.json`: Add Japanese translations
- `src/settings/settings-service.ts`: Used to load SSH connections (existing usage)

**No new external dependencies.**

### File Structure

```
src/
├── profile/
│   └── profile-editor.ts    # Modified: add tab UI
├── styles/
│   └── settings-panel.css   # Modified: add .profile-editor-tab* styles
└── i18n/
    └── locales/
        ├── en.json           # Modified: add tab labels
        └── ja.json           # Modified: add tab labels
```

## Test Scenarios

### Unit Tests
- [ ] New profile defaults to SHELL tab
- [ ] Profile with ssh_connection_name opens with SSH tab selected
- [ ] Profile without ssh_connection_name opens with SHELL tab selected

### Integration Tests
- [ ] Save from SHELL tab produces profile with empty ssh_connection_name
- [ ] Save from SSH tab produces profile with empty shell_path and shell_args
- [ ] Tab switch from SSH to SHELL clears ssh_connection_name
- [ ] Tab switch from SHELL to SSH clears shell fields

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with WebdriverIO + tauri-driver
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression
- [ ] Profile editor shows SHELL/SSH tabs
- [ ] SSH tab is disabled when no SSH connections exist

### Edge Cases
- [ ] SSH connections list is empty: SSH tab is disabled, cannot be selected
- [ ] Editing SSH profile when the referenced SSH connection has been deleted: SSH tab selected, dropdown shows no matching selection
- [ ] Rapid tab switching does not cause state inconsistency

## Error Handling

No new error states are introduced. Existing validation (profile name required) remains unchanged.

## Success Criteria

- [ ] SHELL/SSH tabs are displayed in profile editor modal
- [ ] Tab switching correctly shows/hides fields and clears values
- [ ] SSH tab is disabled when no SSH connections exist
- [ ] Auto-selection works for existing profiles
- [ ] ARIA roles and keyboard navigation are implemented
- [ ] i18n works for both English and Japanese
- [ ] Existing E2E tests pass without regression
- [ ] TypeScript typecheck passes

## Open Questions

> **Note**: No unresolved requirements.

## References

- Requirements document: `doc/tasks/profile-shell-ssh-tabs/要件定義書.md`
- UI Design Guidelines: `doc/UI-DESIGN-GUIDELINES.yaml`
- Existing SSH connection task: `doc/tasks/ssh-connection/`
