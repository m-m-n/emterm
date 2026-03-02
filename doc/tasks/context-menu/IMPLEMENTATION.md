# Implementation Plan: Right-Click Context Menu

## Overview

Add native right-click context menus to three areas of the eMterm UI: terminal viewport, tab elements, and tab bar empty space. Each menu displays contextually appropriate items with dynamic enable/disable states, using Tauri v2 native Menu API for OS-standard appearance.

## Objectives

- Provide context-specific right-click menus for terminal area, tab elements, and tab bar
- Dynamically enable/disable menu items based on current context (selection, URL, profiles)
- Integrate with existing subsystems (clipboard, URL detection, paste flow, tab management)
- Override PTY mouse tracking to always show context menu on right-click
- Localize all menu labels (en/ja)

## Prerequisites

### Development Environment
- Bun (package manager / bundler)
- Tauri v2 CLI
- Rust toolchain

### Dependencies
- `@tauri-apps/api/menu` - Already available via `@tauri-apps/api ^2.0.0` (devDependency)
- `@tauri-apps/plugin-shell` - `open()` for URL opening (already installed)
- `@tauri-apps/plugin-clipboard-manager` - Clipboard access (already installed)

### Internal Dependencies
- `SelectionController` - Selection state check and copy
- `ClipboardManager` - Clipboard read/write
- `url-detector.ts` - URL detection at click position
- `TabManager` - Tab creation and closure
- `TabBarUI` - Tab bar DOM structure
- `TerminalApp` - Terminal container, state access
- `ProfileSelector` - Profile selector modal
- Existing paste flow (dialog, chunked sending)

## Architecture Overview

### Technology Stack
- **Frontend**: Vanilla TypeScript
- **Menu system**: Tauri v2 native Menu API (`@tauri-apps/api/menu`)
- **Backend**: No Rust changes needed (all context lives in frontend)

### Design Approach

Menus are created dynamically on each right-click to reflect current state. A single context menu module provides builder functions for each of the three menu types. Each builder receives the necessary context (selection state, URL, profiles) and returns a configured native menu. No persistent menu objects are retained.

### Component Interaction

```
contextmenu event (DOM)
  → ContextMenuBuilder (determines zone: terminal / tab / tab-bar)
  → Gathers context from existing subsystems
  → Creates native Menu via Tauri JS API
  → menu.popup()
  → Action callback → delegates to existing subsystem
```

Terminal context menu needs access to:
- `SelectionController` for selection state and copy
- `url-detector` functions for URL detection at click position
- `TerminalState` for active buffer and viewport info
- `charSize` for pixel-to-cell coordinate conversion
- Existing paste flow for paste action

Tab context menu needs access to:
- `TabManager` for tab closure
- Target tab ID from DOM event

Tab bar context menu needs access to:
- `TabManager` for new tab creation
- `TabBarUI` for profile selector
- Settings for profile availability check

## Implementation Phases

### Phase 1: Infrastructure and Context Menu Module

**Goal**: Create the context menu builder module, add Tauri menu capability, add i18n keys, and expose necessary internal state from TerminalApp.

**Files to Create**:
- `src/context-menu/index.ts` - Context menu builder and action handlers

**Files to Modify**:
- `src-tauri/capabilities/default.json` - Add `menu:default` permission
- `src/i18n/locales/en.json` - Add `context_menu.*` keys
- `src/i18n/locales/ja.json` - Add `context_menu.*` keys
- `src/terminal-app/index.ts` - Expose selection controller, terminal root, char size, and state as public accessors

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ContextMenuBuilder | Build and show native menus for each zone | Valid context (zone, state) | Menu displayed at cursor position |
| Terminal accessors (TerminalApp) | Expose internal state needed by menu builder | TerminalApp initialized | Selection, root element, char size, state accessible |

**Processing Flow**:
1. Right-click event fires on a target element
2. Determine zone: terminal viewport, tab, or tab bar empty area
   - Target closest `.tab` → tab zone
   - Target inside `.tab-scroll-area` but not a tab → tab bar zone
   - Target inside `.terminal-root` → terminal zone
3. Build menu for the determined zone (see Phase 2-4)
4. Show native popup menu
5. User selects item → execute action callback

**Implementation Steps**:
1. **Add menu capability** - Add `menu:default` to Tauri capabilities
2. **Add i18n keys** - Add all 7 context menu label keys to en.json and ja.json
3. **Expose TerminalApp internals** - Add public getters for `selectionController`, `terminalRoot`, `charSize`, and `state`
4. **Create context menu module** - Create `src/context-menu/index.ts` with builder functions for each zone and action handlers

**Dependencies**: None (foundation phase)

**Testing Approach**:
- Unit: Verify i18n keys exist in both locales
- Manual: Confirm `menu:default` capability doesn't break existing functionality

**Acceptance Criteria**:
- [ ] `menu:default` permission added to capabilities
- [ ] All 7 i18n keys present in en.json and ja.json
- [ ] TerminalApp exposes necessary accessors
- [ ] Context menu module structure in place

**Estimated Effort**: small

---

### Phase 2: Terminal Context Menu

**Goal**: Implement the terminal viewport context menu with Copy, Paste, separator, Copy URL, and Open URL items, all with dynamic enable/disable states.

**Files to Modify**:
- `src/context-menu/index.ts` - Implement terminal menu builder and action handlers
- `src/terminal-app/index.ts` - Attach contextmenu event listener on terminal root
- `src/terminal-app/handlers/mouse.ts` - Allow context menu to propagate during PTY mouse tracking

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| buildTerminalMenu | Create menu with dynamic item states | MouseEvent on terminal area | Native menu with correct enabled states |
| Terminal actions | Execute copy/paste/URL operations | Menu item selected | Operation completed via existing subsystem |
| MouseHandler patch | Stop blocking contextmenu during mouse tracking | Mouse tracking enabled | Right-click shows context menu instead of sending to PTY |

**Processing Flow**:
1. `contextmenu` event on terminal root
   - `preventDefault()` to suppress browser default
2. Detect context:
   - Check `selectionController.hasSelection()` → determines Copy enabled state
   - Convert click pixel coordinates to cell (row, col) using `charSize` and `terminalRoot.getBoundingClientRect()`
   - Build logical line from active buffer at the row
   - Convert physical position to logical column
   - Call `findUrlAtPosition(logicalLine.text, logicalCol)` → determines URL item states
3. Build native menu:
   - Copy (enabled if selection exists)
   - Paste (always enabled)
   - Separator
   - Copy URL (enabled if URL detected)
   - Open URL (enabled if URL detected)
4. Show popup menu
5. Action callbacks:
   - Copy → `selectionController.copy()`
   - Paste → read clipboard, check multi-line, show dialog if needed, send to PTY (reuse existing paste pattern)
   - Copy URL → write detected URL to clipboard
   - Open URL → open detected URL in default browser via shell plugin

**Mouse tracking override** (FR12):
- Modify `MouseHandler.onContextMenu` to not call `preventDefault()` for right-click
- The contextmenu handler on terminal root will handle all right-clicks regardless of mouse tracking mode

**Implementation Steps**:
1. **Modify MouseHandler** - Remove `preventDefault()` from `onContextMenu` so the event propagates to the terminal root's contextmenu handler
2. **Attach contextmenu listener** - Register on terminal root in TerminalApp, calling the terminal menu builder
3. **Implement terminal menu builder** - Gather context (selection, URL), create native menu items with correct enabled states, show popup
4. **Implement action handlers** - Copy, paste (reusing existing paste flow), copy URL, open URL

**Dependencies**: Requires Phase 1

**Testing Approach**:
- Unit: Menu item enabled-state logic (selection present/absent, URL present/absent)
- E2E (Docker): Right-click shows native menu (limited testability for native menus)
- Manual: Verify all 4 actions work correctly, verify mouse tracking override

**Acceptance Criteria**:
- [ ] Right-click on terminal shows 5-item native menu
- [ ] Copy disabled when no selection, enabled when selection exists
- [ ] Paste always enabled, triggers existing paste flow (with dialog for multi-line)
- [ ] Copy URL / Open URL disabled when no URL at position, enabled when URL present
- [ ] Menu appears even when PTY mouse tracking is active
- [ ] FR1, FR2, FR3, FR4, FR5, FR6, FR12 satisfied

**Estimated Effort**: medium

---

### Phase 3: Tab and Tab Bar Context Menus

**Goal**: Implement context menus for tab elements (Close) and tab bar empty area (New Tab, Open Profile).

**Files to Modify**:
- `src/context-menu/index.ts` - Implement tab and tab bar menu builders and action handlers
- `src/tab-bar/tab-bar-ui.ts` - Attach contextmenu event listeners on scroll area

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| buildTabMenu | Create Close menu for a specific tab | Right-click on `.tab` element | Native menu with Close item |
| buildTabBarMenu | Create New Tab / Open Profile menu | Right-click on empty tab bar area | Native menu with correct Open Profile state |
| Tab bar event routing | Distinguish tab vs. empty area clicks | contextmenu on `.tab-scroll-area` | Correct menu shown for target |

**Processing Flow**:

**Tab context menu:**
1. `contextmenu` event on `.tab-scroll-area`
2. Check `event.target.closest('.tab')`
   - If found → tab context menu:
     - Extract tab ID from element's `dataset.tabId`
     - Build menu with single Close item
     - Action: `TabManager.closeTab(tabId)`
   - If not found → tab bar context menu (see below)

**Tab bar context menu:**
1. Right-click on empty area within `.tab-scroll-area`
2. Check if profiles exist in settings
3. Build menu:
   - New Tab (always enabled)
   - Open Profile (disabled if no profiles configured)
4. Action callbacks:
   - New Tab → `TabManager.createTab()`
   - Open Profile → `TabBarUI.showProfileSelector(profiles)`

**Implementation Steps**:
1. **Implement tab menu builder** - Create single-item Close menu, action delegates to TabManager
2. **Implement tab bar menu builder** - Create New Tab and Open Profile items, check profile availability for enabled state
3. **Attach event listeners** - Register contextmenu on `.tab-scroll-area` in TabBarUI, route to correct builder based on click target
4. **Implement action handlers** - Close tab, create new tab, show profile selector

**Dependencies**: Requires Phase 1

**Testing Approach**:
- Unit: Tab target detection logic (tab vs. empty area)
- E2E (Docker): Right-click on tab area and tab bar area (limited native menu testability)
- Manual: Verify Close removes correct tab, New Tab creates tab, Open Profile shows modal

**Acceptance Criteria**:
- [ ] Right-click on tab shows Close menu
- [ ] Close removes the targeted tab (not active tab if different)
- [ ] Right-click on empty tab bar area shows New Tab / Open Profile menu
- [ ] New Tab creates a default terminal tab
- [ ] Open Profile shows profile selector modal
- [ ] Open Profile disabled when no profiles configured
- [ ] FR7, FR8, FR9, FR10, FR11 satisfied

**Estimated Effort**: small

---

### Phase 4: Integration and Polish

**Goal**: Ensure all context menus work together, verify i18n, test cross-platform behavior, confirm no regressions.

**Files to Modify**:
- None expected (bug fixes only if discovered)

**Implementation Steps**:
1. **Verify i18n** - Test both English and Japanese locale menu labels
2. **Run existing E2E tests** - Confirm no regressions from mouse handler changes
3. **Cross-platform check** - Verify native menu appearance on Linux (GTK) and Windows (Win32)
4. **Edge case testing** - Mouse tracking modes, empty clipboard, wrapped URLs, zero profiles

**Dependencies**: Requires Phase 2, Phase 3

**Testing Approach**:
- E2E (Docker): Run full E2E suite for regression
- Manual: Cross-locale testing, edge case scenarios

**Acceptance Criteria**:
- [ ] All existing E2E tests pass
- [ ] Menu labels correct in English and Japanese
- [ ] FR13, NFR1, NFR2, NFR3 satisfied
- [ ] All edge cases handled

**Estimated Effort**: small

---

## Complete File Structure

```
src/
├── context-menu/
│   └── index.ts               # Context menu builders (terminal, tab, tab bar) and action handlers
├── terminal-app/
│   ├── index.ts               # Add public accessors, attach terminal contextmenu listener
│   └── handlers/
│       └── mouse.ts           # Remove preventDefault from onContextMenu
├── tab-bar/
│   └── tab-bar-ui.ts          # Attach contextmenu listener on tab scroll area
├── i18n/
│   └── locales/
│       ├── en.json            # Add context_menu.* keys
│       └── ja.json            # Add context_menu.* keys
src-tauri/
└── capabilities/
    └── default.json           # Add menu:default permission
```

## Testing Strategy

- **Unit**: Menu item enabled-state logic, zone detection, URL detection integration
- **E2E (Docker)**: Regression testing via existing test suite; native menu popup testability is limited
- **Manual**: Action execution (copy, paste, URL operations, tab operations), i18n label verification, mouse tracking override, cross-platform appearance

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `@tauri-apps/api` | ^2.0.0 | Menu API (`@tauri-apps/api/menu`) - already installed |
| `@tauri-apps/plugin-shell` | existing | URL opening via `open()` - already installed |
| `@tauri-apps/plugin-clipboard-manager` | existing | Clipboard access - already installed |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Native menu API differences between Linux/Windows | Low | Medium | Use Tauri's cross-platform menu abstraction |
| Mouse handler change breaks existing mouse behavior | Low | High | Targeted change: only remove preventDefault from onContextMenu |
| Native menus not testable in E2E | High | Low | Rely on manual testing for menu popup; unit test enabled-state logic |

## Open Questions

- None. All requirements are clear from SPEC.md.

## Success Metrics

- [ ] All 13 functional requirements (FR1-FR13) implemented
- [ ] All 3 non-functional requirements (NFR1-NFR3) satisfied
- [ ] No regression in existing E2E tests
- [ ] Menu labels localized in en/ja
- [ ] Works on Linux and Windows
