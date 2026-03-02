# Feature: Right-Click Context Menu

## Overview

Add native context menus to three distinct areas of the eMterm UI: terminal viewport, tab elements, and tab bar empty space. Each area displays a tailored set of menu items with dynamic enable/disable states based on the current context (selection state, URL presence, profile availability). Uses Tauri v2 native Menu API for OS-standard appearance.

## Objectives

- Provide right-click context menu for terminal area (copy, paste, URL operations)
- Provide right-click context menu for tabs (close)
- Provide right-click context menu for tab bar empty area (new tab, open profile)
- Use Tauri native menus for consistent OS look and feel
- Support i18n for menu item labels

## User Stories

### US1: Terminal Context Menu
As a terminal user, I want to right-click in the terminal area to access copy, paste, and URL operations, so that I can perform common actions without keyboard shortcuts.

**Acceptance Criteria:**
- [ ] Right-click shows menu with Copy, Paste, Copy URL, Open URL
- [ ] Copy is disabled when no text is selected
- [ ] Copy URL and Open URL are disabled when cursor is not on a URL
- [ ] Paste triggers existing paste flow (with confirmation dialog for multi-line)
- [ ] Menu appears even when PTY mouse tracking is enabled

### US2: Tab Context Menu
As a user, I want to right-click on a tab to close it, so that I can manage tabs with the mouse.

**Acceptance Criteria:**
- [ ] Right-click on tab shows menu with Close
- [ ] Close removes the target tab

### US3: Tab Bar Context Menu
As a user, I want to right-click on the tab bar empty area to create new tabs, so that I can quickly open terminals.

**Acceptance Criteria:**
- [ ] Right-click on empty tab bar area shows menu with New and Open Profile
- [ ] New creates a terminal tab with default shell
- [ ] Open Profile shows the profile selector modal
- [ ] Open Profile is disabled when no profiles are configured

## Technical Requirements

### Functional Requirements

- **FR1: Terminal context menu** - Display native context menu with Copy, Paste, separator, Copy URL, Open URL on right-click in the terminal viewport.
- **FR2: Menu item dynamic states** - Enable/disable menu items based on context: Copy requires active selection; Copy URL and Open URL require URL at click position (detected via existing `findUrlAtPosition()`).
- **FR3: Copy action** - Copy selected text to clipboard using existing `SelectionController.copy()` / `ClipboardManager`.
- **FR4: Paste action** - Read clipboard and send to PTY. For multi-line content, show existing Paste Dialog for confirmation. Reuse existing paste flow.
- **FR5: Copy URL action** - Copy the detected URL string to clipboard using `ClipboardManager.copyToClipboard()`.
- **FR6: Open URL action** - Open the detected URL in the default browser using `@tauri-apps/plugin-shell` `open()`.
- **FR7: Tab context menu** - Display native context menu with Close on right-click on a tab element.
- **FR8: Tab close action** - Close the right-clicked tab via `TabManager.closeTab()`.
- **FR9: Tab bar context menu** - Display native context menu with New and Open Profile on right-click in the tab bar empty area (not on a tab element).
- **FR10: New tab action** - Create a new terminal tab with default shell via `TabManager.createTab()`.
- **FR11: Open profile action** - Show the profile selector modal. Disabled when `AppSettings.profiles` is empty.
- **FR12: Mouse tracking override** - When PTY mouse tracking is enabled, right-click shows the context menu instead of sending the mouse event to the PTY.
- **FR13: i18n support** - All menu item labels are localized via existing i18n system (en/ja).

### Non-Functional Requirements

- **NFR1 - Performance:** Menu must appear instantly on right-click (native menus have no render delay).
- **NFR2 - Platform:** Must work on both Linux (GTK) and Windows (Win32).
- **NFR3 - Maintainability:** Menu definitions should be structured for easy addition of future items.

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│  Frontend (TypeScript)                          │
│                                                 │
│  contextmenu event → detect context             │
│  → determine enabled/disabled states            │
│  → call Tauri Menu API (JS)                     │
│  → handle menu item action callback             │
│                                                 │
│  Three trigger zones:                           │
│  1. Terminal container (terminal-root)           │
│  2. Tab element (.tab)                          │
│  3. Tab bar empty area (.tab-scroll-area)       │
└─────────────────────────────────────────────────┘
```

**Menu creation approach:**

Menus are created dynamically on each right-click to reflect current state (selection, URL, profiles). The `@tauri-apps/api/menu` JS API is used to build and show the popup menu. No Rust-side menu creation is needed since all context (selection state, URL detection, profiles) lives in the frontend.

### Data Flow

**Terminal context menu:**
```
Right-click on terminal
  → contextmenu event (preventDefault)
  → Check selection state (SelectionController)
  → Build logical line, detect URL at click position (url-detector.ts)
  → Create Menu with enabled/disabled items
  → menu.popup()
  → User clicks item
  → Action callback executes (copy/paste/url-copy/url-open)
```

**Tab context menu:**
```
Right-click on tab
  → contextmenu event (preventDefault)
  → Identify target tab from event.target closest('.tab')
  → Create Menu with Close item
  → menu.popup()
  → User clicks Close
  → TabManager.closeTab(tabId)
```

**Tab bar context menu:**
```
Right-click on tab bar empty area
  → contextmenu event (preventDefault)
  → Check if profiles exist (AppSettings.profiles.length > 0)
  → Create Menu with New and Open Profile items
  → menu.popup()
  → User clicks item
  → New: TabManager.createTab() / Open Profile: showProfileSelector()
```

### Dependencies

**Internal Dependencies:**
- `SelectionController` - Check if text is selected, perform copy
- `ClipboardManager` - Write URL to clipboard, read for paste
- `url-detector.ts` - `findUrlAtPosition()`, `getLogicalLine()`
- `TabManager` - `createTab()`, `closeTab()`
- `TabBarUI` - Tab bar DOM structure, event delegation
- `TerminalApp` - Terminal container, access to state and charSize
- `ProfileSelector` - `showProfileSelector()`
- Existing paste flow (paste dialog, chunked sending)

**External Dependencies:**
- `@tauri-apps/api/menu` - Menu, MenuItem, PredefinedMenuItem (already in `@tauri-apps/api`)
- `@tauri-apps/plugin-shell` - `open()` for URL opening (already installed)
- `@tauri-apps/plugin-clipboard-manager` - Clipboard access (already installed)

### File Structure

```
src/
├── context-menu/
│   └── index.ts               # Context menu builder and handlers
├── terminal-app/
│   ├── index.ts               # Add contextmenu handler for terminal
│   └── handlers/
│       └── mouse.ts           # Modify to allow context menu during mouse tracking
├── tab-bar/
│   └── tab-bar-ui.ts          # Add contextmenu handlers for tab and tab bar
├── i18n/
│   └── locales/
│       ├── en.json            # Add menu item labels
│       └── ja.json            # Add menu item labels
└── ...
```

### Tauri Configuration

**Capabilities** (`src-tauri/capabilities/default.json`):

Add menu-related permissions:
```json
"menu:default"
```

### i18n Keys

| Key | English | Japanese |
|-----|---------|----------|
| `context_menu.copy` | Copy | コピー |
| `context_menu.paste` | Paste | 貼り付け |
| `context_menu.copy_url` | Copy URL | URLをコピー |
| `context_menu.open_url` | Open URL | URLを開く |
| `context_menu.close_tab` | Close | 閉じる |
| `context_menu.new_tab` | New Tab | 新規タブ |
| `context_menu.open_profile` | Open Profile | プロファイルを開く |

## Test Scenarios

### Unit Tests
- [ ] URL detection at click position returns correct URL or null
- [ ] Menu item enabled state logic: copy disabled when no selection
- [ ] Menu item enabled state logic: URL items disabled when no URL at position

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` (30 files)
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression
- [ ] Right-click on terminal shows context menu (native menu testing may be limited)
- [ ] Right-click on tab shows close menu
- [ ] Right-click on tab bar empty area shows new/profile menu

### Edge Cases
- [ ] Right-click during mouse tracking mode: menu shown, no PTY event
- [ ] Right-click with empty clipboard: paste still enabled (clipboard read happens on action)
- [ ] Right-click on URL that spans wrapped lines: URL correctly detected via logical line assembly
- [ ] Profile menu with 0 profiles: Open Profile is disabled
- [ ] Very long URL: full URL is copied/opened without truncation

## Security Considerations

- **URL Validation:** URLs opened via `open()` are detected by regex; only http/https/ftp/file protocols are matched
- **Clipboard Access:** Uses Tauri's sandboxed clipboard API (already permitted)
- **Shell Open:** Uses `@tauri-apps/plugin-shell` `open()` which delegates to the OS (already permitted)

## Success Criteria

- [ ] All functional requirements (FR1-FR13) are implemented
- [ ] All test scenarios pass
- [ ] Works on both Linux and Windows
- [ ] Menu items are localized (en/ja)
- [ ] Existing keyboard shortcuts and mouse behavior remain unchanged
- [ ] No regression in existing E2E tests

## Open Questions

> **Note**: No unresolved requirements.

## References

- Tauri v2 Menu API: `@tauri-apps/api/menu`
- Tauri v2 Window Menu Guide: https://v2.tauri.app/learn/window-menu/
- Existing URL detector: `src/terminal/url-detector.ts`
- Existing clipboard: `src/clipboard/manager.ts`
- Existing paste flow: `src/clipboard/paste.ts`, `src/clipboard/dialog.ts`
