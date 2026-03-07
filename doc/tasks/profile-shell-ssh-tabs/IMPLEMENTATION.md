# Implementation Plan: Profile Editor SHELL/SSH Tab UI

## Overview

Add a SHELL | SSH tab switcher inside the profile editor modal to separate local shell configuration from SSH connection selection, enforcing mutual exclusivity between the two modes.

## Objectives

- Provide clear visual separation between shell and SSH profile types via tab UI
- Enforce mutual exclusivity: a profile uses either a local shell or an SSH connection
- Disable the SSH tab when no SSH connections are registered
- Maintain accessibility with ARIA roles and keyboard navigation

## Prerequisites

### Development Environment

- Bun (package manager and bundler)
- Docker (for test execution)

### Dependencies

- No new external dependencies
- Existing internal dependency: `SettingsService` for loading SSH connections

## Architecture Overview

### Technology Stack

- **Language**: TypeScript (frontend only)
- **Framework**: Vanilla TypeScript with DOM manipulation
- **Styling**: CSS with MD3 design tokens from `UI-DESIGN-GUIDELINES.yaml`

### Design Approach

The profile editor modal currently renders all fields (shell fields + SSH dropdown) linearly in one form. This change introduces a tab bar between the name field and the form fields, grouping shell-related fields under a "SHELL" tab panel and the SSH connection dropdown under an "SSH" tab panel. Only one panel is visible at a time.

### Component Interaction

```
Profile Editor Modal
  |-- Name field (always visible, outside tabs)
  |-- Tab bar [SHELL | SSH]
  |     |-- SHELL panel: shell_path, shell_args, env_vars, working_directory
  |     |-- SSH panel: ssh_connection dropdown
  |-- Button row [Cancel | Save]
```

Tab state determines which panel is visible and which fields contribute to the saved profile. On save, only the active tab's values are included; the inactive tab's values are cleared.

## Implementation Phases

### Phase 1: Tab UI and State Management in Profile Editor

**Goal**: Replace linear field layout with tabbed interface, implementing all tab switching logic, ARIA accessibility, and value clearing behavior.

**Files to Modify**:

- `src/profile/profile-editor.ts` - Restructure form to use tab bar with two panels, add tab state management and switching logic
- `src/styles/settings-panel.css` - Add tab bar and tab panel styles for profile editor
- `src/i18n/locales/en.json` - Add `settings.profiles.tabShell` and `settings.profiles.tabSsh` keys
- `src/i18n/locales/ja.json` - Add `settings.profiles.tabShell` and `settings.profiles.tabSsh` keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Tab bar | Render SHELL/SSH tab buttons with ARIA tablist role | Profile editor modal is open | Tab bar is visible between name field and form fields |
| Tab switching | Toggle active panel visibility and clear inactive values | User clicks a tab or uses arrow keys | Active panel shown, inactive panel hidden, inactive values cleared |
| Initial tab selection | Determine which tab to select on modal open | Profile data is available, SSH connections loaded | SSH tab selected if profile has ssh_connection_name; SHELL tab otherwise |
| SSH tab disable logic | Disable SSH tab when no SSH connections exist | SSH connections list loaded from settings | SSH tab has disabled state and cannot be activated |

**Processing Flow** (diagram-convertible):

1. Modal opens
   - Load SSH connections from settings
   - If ssh_connections is empty -> mark SSH tab as disabled
   - If editing profile with non-empty ssh_connection_name -> select SSH tab
   - Otherwise -> select SHELL tab
2. User clicks tab (or arrow key navigation)
   - If target tab is disabled -> do nothing
   - Update active tab state
   - Show target panel, hide other panel
   - Clear values of the hidden panel's fields
3. User submits form
   - Read name from name field (always visible)
   - Read values only from active panel's fields
   - Inactive panel's fields already cleared by tab switch

**Implementation Steps**:

1. **Add i18n keys** - Add `tabShell` and `tabSsh` translation entries to both locale files
2. **Add CSS styles** - Define `.profile-editor-tabs`, `.profile-editor-tab`, `.profile-editor-tab.active`, `.profile-editor-tab.disabled`, `.profile-editor-tab-panel` styles following MD3 tokens
3. **Restructure form layout** - Insert tab bar after name field; wrap shell fields in SHELL tab panel; wrap SSH dropdown in SSH tab panel
4. **Implement tab state and switching** - Track active tab, toggle panel visibility, clear inactive values on switch
5. **Implement initial tab selection** - After SSH connections load, determine initial tab based on profile data and SSH availability
6. **Add ARIA attributes and keyboard navigation** - Apply tablist/tab/tabpanel roles, manage aria-selected, support arrow key navigation between tabs

**Dependencies**: None (single phase)

**Testing Approach**:

- Unit: Verify tab state initialization for new profile (SHELL selected), for SSH profile (SSH selected), for profile without SSH connections (SSH disabled)
- Integration: Verify save output has correct field clearing based on active tab
- E2E (Docker): Verify tab bar renders, tabs are clickable, existing E2E tests pass
- Manual: Visual appearance matches MD3 design, keyboard navigation feels natural

**Acceptance Criteria**:

- [ ] SHELL/SSH tabs displayed in profile editor between name field and form fields
- [ ] SHELL tab selected by default for new profiles
- [ ] SSH tab auto-selected when editing profile with ssh_connection_name
- [ ] SSH tab disabled when no SSH connections registered
- [ ] Tab switching shows/hides correct panel and clears inactive values
- [ ] Save from SHELL tab produces profile with empty ssh_connection_name
- [ ] Save from SSH tab produces profile with empty shell_path, shell_args, env_vars, working_directory
- [ ] ARIA tablist/tab/tabpanel roles applied
- [ ] Arrow key navigation between tabs works
- [ ] i18n labels render in both English and Japanese
- [ ] TypeScript typecheck passes
- [ ] Existing E2E tests pass

**Estimated Effort**: small

---

## Complete File Structure

```
src/
  profile/
    profile-editor.ts    # Modified: add tab UI, restructure form into tab panels
    types.ts             # No changes
  styles/
    settings-panel.css   # Modified: add .profile-editor-tab* styles
  i18n/
    locales/
      en.json            # Modified: add tabShell, tabSsh keys
      ja.json            # Modified: add tabShell, tabSsh keys
```

## Testing Strategy

- **Unit**: Tab state initialization logic, value clearing on tab switch
- **Integration**: Save behavior produces correct Profile object based on active tab
- **E2E (Docker)**: Tab bar renders in profile editor modal, SSH tab disabled state, no regression in existing tests
- **Manual**: Visual consistency with MD3 design, keyboard navigation, i18n label display

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none)  | -       | No new dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| SSH connections load fails asynchronously after tab rendered | Low | Low | SSH tab defaults to disabled; enabled only after successful load |
| Rapid tab switching causes inconsistent field values | Low | Low | Clear values synchronously on each tab switch |

## Open Questions

- (none - all requirements resolved)

## Success Metrics

- [ ] All acceptance criteria from Phase 1 pass
- [ ] TypeScript typecheck: 0 errors
- [ ] Existing E2E tests: no regression
- [ ] Tab switching latency: imperceptible (< 16ms, single DOM operation)
