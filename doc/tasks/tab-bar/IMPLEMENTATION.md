# Implementation Plan: Tab Bar

## Overview

Implement a tab bar feature for eMterm that allows users to manage multiple terminal sessions within a single window. The tab bar appears below the OS native title bar and provides navigation, creation, and management of terminal tabs with full keyboard shortcut support.

## Objectives

- Enable multiple terminal sessions in a single window
- Provide intuitive tab management through mouse and keyboard
- Support tab reordering via drag and drop
- Display settings panel as a special tab
- Maintain low-latency performance for tab operations

## Prerequisites

### Development Environment
- Node.js with Bun package manager
- Rust toolchain with Cargo
- Tauri CLI

### Dependencies
- Existing `PtyClient` (`src/pty/client.ts`) for PTY communication
- Existing `TerminalApp` (`src/terminal-app/`) for terminal instances
- Existing `PtyManager` (`src-tauri/src/pty/manager.rs`) for backend session management
- `@tauri-apps/api` (already installed)

### Knowledge Requirements
- Tauri event system and IPC commands
- HTML5 Drag and Drop API
- TypeScript DOM manipulation without frameworks

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), Rust (backend - existing)
- **Framework**: Vanilla TypeScript (no UI framework)
- **Key Libraries**:
  - `@tauri-apps/api` - Tauri frontend API
  - HTML5 Drag and Drop API - Tab reordering

### Design Approach

The implementation follows a separation of concerns pattern:
- `TabManager` - State management and business logic
- `TabBarUI` - DOM rendering and user interactions
- `TabKeyboardHandler` - Keyboard shortcut handling
- `TabDragHandler` - Drag and drop reordering

TabManager owns all tab-related resources centrally:
- `tabs: Tab[]` - Tab data only (no resource references)
- `terminalApps: Map<tabId, TerminalApp>` - TerminalApp instances
- `eventUnlistens: Map<tabId, UnlistenFn>` - Event cleanup functions
- `eventEmitter: TabEventEmitter` - Typed event emitter for UI notifications

This centralized ownership ensures single-point cleanup when closing tabs.

**Event Emitter Pattern:**
TabManager uses a typed EventEmitter to notify UI components of state changes:
- `tab:created` - Emitted after tab creation completes
- `tab:closed` - Emitted after tab cleanup completes
- `tab:activated` - Emitted when a tab becomes active
- `tab:deactivated` - Emitted when a tab loses focus
- `tab:reordered` - Emitted after drag-drop reorder completes
- `tab:titleChanged` - Emitted when OSC sequence updates title

UI components subscribe to these events for rendering updates, ensuring consistent event emission across all state changes.

### Component Interaction

```
User Input (Click/Keyboard/Drag)
        |
        v
+------------------+
|   TabBarUI       |  <-- Renders tab bar, handles clicks/drag
+------------------+
        |
        v
+------------------+
|   TabManager     |  <-- State management, tab lifecycle
+------------------+
        |
        +----------+----------+
        |                     |
        v                     v
+------------------+  +------------------+
|   TerminalApp    |  |   PtyClient      |
| (per tab)        |  | (PTY commands)   |
+------------------+  +------------------+
        |                     |
        v                     v
+------------------+  +------------------+
|   Tauri Backend  |  |   PtyManager     |
|   (Events)       |  |   (Sessions)     |
+------------------+  +------------------+
```

## Implementation Phases

### Phase 1: Core Tab Infrastructure

**Goal**: Basic tab management with create/switch/close operations and keyboard shortcuts (Ctrl+T, Ctrl+W, Ctrl+Tab)

**Files to Create**:
- `src/tab-bar/types.ts` - Type definitions for tabs
- `src/tab-bar/tab-manager.ts` - Tab state management
- `src/tab-bar/tab-manager.test.ts` - Unit tests
- `src/tab-bar/tab-bar-ui.ts` - Tab bar DOM rendering
- `src/tab-bar/tab-bar-ui.test.ts` - UI tests
- `src/tab-bar/keyboard-handler.ts` - Tab keyboard shortcuts
- `src/tab-bar/keyboard-handler.test.ts` - Keyboard tests
- `src/tab-bar/index.ts` - Module entry point
- `src/styles/tab-bar.css` - Tab bar styles

**Files to Modify**:
- `src/index.html` - Add tab bar container element
- `src/main.ts` - Initialize TabManager instead of single TerminalApp
- `src/styles.css` - Import tab-bar.css

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TabManager | Manage tab lifecycle, state, and all resources | None | Tabs array updated, resources in Maps, TabEvents emitted via EventEmitter |
| Tab | Data-only representation of tab (no resource ownership) | TabManager exists | Tab has unique ID and sessionId |
| TabBarUI | Render tab bar DOM elements | TabManager exists | DOM reflects tab state |
| TabKeyboardHandler | Handle tab keyboard shortcuts | TabManager exists | Shortcut triggers appropriate action |

**Processing Flow** (Tab Creation with Centralized Management):
```
1. User triggers tab creation (Ctrl+T or [+] button)
   |-- Check operationState === { status: 'idle' }
   |-- If not idle: ignore request (prevent race condition)
   |-- Set operationState = { status: 'creating' }
2. TabManager.createTab() called
   |-- Generate unique tab ID
   |-- Create new TerminalApp instance
   |-- TerminalApp.init() spawns PTY session
   |-- PTY session ID obtained
3. Register resources in TabManager
   |-- Create Tab data object (id, sessionId, title, type)
   |-- Store TerminalApp in terminalApps Map
   |-- Store event unlisten in eventUnlistens Map
   |-- Add Tab to tabs array
   |-- activeTabId updated
4. Set operationState = { status: 'idle' }
5. Emit events via TabEventEmitter
   |-- emit('tab:created', { tab })
   |-- emit('tab:activated', { tab, previousTabId })
6. TabBarUI responds to events
   |-- New tab element added to DOM
   |-- Previous tab container hidden
   |-- New tab container shown
```

**Processing Flow** (Tab Close on Shell Exit with Centralized Cleanup):
```
1. PTY session exits (pty-exit event)
   |-- Event contains session_id
2. TabManager.handleSessionExit(sessionId) called
   |-- Check operationState === { status: 'idle' }
   |-- If not idle: queue for later processing
   |-- Set operationState = { status: 'closing', tabId }
3. Find tab by sessionId, then cleanup all resources
   |-- Get TerminalApp from terminalApps Map
   |-- Call terminalApp.dispose()
   |-- Delete from terminalApps Map
   |-- Get unlisten from eventUnlistens Map
   |-- Call unlisten()
   |-- Delete from eventUnlistens Map
4. Tab removed from tabs array
   |-- If tabs.length > 0: activate adjacent tab
   |-- If tabs.length === 0: exit application
5. Set operationState = { status: 'idle' }
   |-- Process queued operations if any
6. Emit events via TabEventEmitter
   |-- emit('tab:closed', { tabId, wasActive })
   |-- emit('tab:activated', { tab, previousTabId }) if new tab activated
7. TabBarUI responds to events
   |-- Tab element removed from DOM
   |-- Adjacent tab container shown (if any)
```

**Implementation Steps**:

1. **Define Type System**
   - Create Tab, TabType, TabBarState interfaces
   - Define CreateTabOptions for tab creation

2. **Implement TabManager (Centralized Resource Management)**
   - State management for tabs array and activeTabId
   - Centralized Maps: terminalApps, eventUnlistens
   - TabEventEmitter for typed event emission
   - Methods: createTab, closeTab, switchTab, getActiveTab, getTerminalApp
   - Methods: on(event, handler), off(event, handler) for event subscription
   - closeTab() handles all resource cleanup (dispose, unlisten, remove from Maps)
   - All state changes emit corresponding events (tab:created, tab:closed, etc.)
   - Event handling for PTY exit events
   - Application exit when last tab closes

3. **Implement TabBarUI**
   - DOM rendering for tab bar structure
   - Tab elements with click handlers
   - Fixed area with [+] and settings buttons
   - Visual state for active/inactive tabs

4. **Implement TabKeyboardHandler**
   - Ctrl+T: create new tab
   - Ctrl+W: close active tab (send SIGTERM)
   - Ctrl+Tab: next tab
   - Ctrl+Shift+Tab: previous tab

5. **Update Entry Point**
   - Modify main.ts to use TabManager
   - Add tab bar container to index.html
   - Initialize with single tab on startup

**Dependencies**:
- Requires: None (first phase)
- Blocks: Phase 2 (advanced features)

**Testing Approach**:

*Unit Tests*:
- TabManager.createTab() creates unique ID and sessionId
- TabManager.closeTab() removes correct tab
- TabManager.switchTab() updates active states
- Closing last tab signals application exit

*Manual Testing*:
- [ ] Ctrl+T creates new terminal tab
- [ ] New tab shows working shell prompt
- [ ] Clicking tab switches to it
- [ ] Ctrl+W closes active tab
- [ ] Shell `exit` command closes tab
- [ ] Last tab close exits application

**Acceptance Criteria**:
- [ ] Multiple tabs can be created with [+] button or Ctrl+T
- [ ] Each tab has independent PTY session
- [ ] Tabs can be switched by clicking
- [ ] Ctrl+Tab cycles through tabs
- [ ] Shell exit automatically closes tab
- [ ] Ctrl+W sends SIGTERM and closes tab
- [ ] Application exits when last tab closes

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:
- **Risk**: PtyClient assumes single session model
  - **Mitigation**: Each tab creates its own PtyClient instance
- **Risk**: Keyboard shortcuts conflict with terminal applications
  - **Mitigation**: Tab shortcuts use Ctrl+Shift where needed to avoid conflicts

---

### Phase 2: Advanced Features

**Goal**: Complete feature set with drag-drop reordering, Ctrl+1-9 shortcuts, tab scroll, and settings tab

**Files to Create**:
- `src/tab-bar/drag-handler.ts` - Drag and drop logic
- `src/tab-bar/drag-handler.test.ts` - Drag tests
- `src/settings/index.ts` - Settings panel entry
- `src/settings/settings-panel.ts` - Settings UI placeholder

**Files to Modify**:
- `src/tab-bar/tab-manager.ts` - Add reorderTabs, settings tab support
- `src/tab-bar/tab-bar-ui.ts` - Add drag listeners, scroll behavior
- `src/tab-bar/keyboard-handler.ts` - Add Ctrl+1-9 shortcuts
- `src/styles/tab-bar.css` - Add drag styles, scroll styles

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TabDragHandler | Manage drag and drop operations | TabBarUI exists | Tabs reordered on drop |
| SettingsPanel | Display settings UI | Settings tab active | Settings content shown |
| TabScrollArea | Handle overflow with scroll | Many tabs exist | All tabs accessible via scroll |

**Processing Flow** (Drag and Drop):
```
1. User starts dragging a tab
   |-- dragstart event captured
   |-- Source tab index stored
   |-- Dragging visual state applied
2. User drags over other tabs
   |-- dragover events update insertion indicator
   |-- Insertion position calculated from mouse position
3. User drops tab
   |-- drop event captured
   |-- Determine position ('before' or 'after') based on mouse position
   |-- TabManager.reorderTabs(draggedTabId, targetTabId, position)
   |-- TabBarUI.render() updates DOM order
```

**Processing Flow** (Settings Tab):
```
1. User clicks settings button
   |-- Check if settings tab exists
2. If settings tab exists
   |-- switchTab(settingsTabId)
3. If no settings tab
   |-- Create new tab with type 'settings'
   |-- No PTY session created (sessionId empty)
   |-- Show SettingsPanel content
```

**Implementation Steps**:

1. **Implement Ctrl+1-9 Shortcuts**
   - Ctrl+1 through Ctrl+8: jump to tab by index
   - Ctrl+9: jump to last tab
   - Handle index out of bounds gracefully

2. **Implement Tab Drag and Drop**
   - HTML5 Drag and Drop API integration
   - Visual feedback during drag (opacity, cursor)
   - Drop indicator showing insertion point
   - Settings tab excluded from dragging

3. **Implement Tab Scroll**
   - Overflow-x: auto on tab scroll area
   - Hidden scrollbar (wheel scroll only)
   - Auto-scroll to active tab when switching

4. **Implement Settings Tab**
   - Settings button opens/activates settings tab
   - Only one settings tab allowed
   - Settings tab can be closed with Ctrl+W
   - Placeholder content for settings panel

**Dependencies**:
- Requires: Phase 1 (core infrastructure)
- Blocks: Phase 3 (polish)

**Testing Approach**:

*Unit Tests*:
- TabManager.reorderTabs() moves tab correctly
- Settings tab singleton enforcement
- Ctrl+1-9 activates correct tab index
- Drag handler calculates correct insertion index

*Manual Testing*:
- [ ] Ctrl+1 through Ctrl+9 switch to correct tabs
- [ ] Tab can be dragged to new position
- [ ] Drop indicator shows during drag
- [ ] Settings tab cannot be dragged
- [ ] Many tabs show horizontal scroll
- [ ] Mouse wheel scrolls tab bar

**Acceptance Criteria**:
- [ ] Ctrl+1-9 shortcuts work correctly
- [ ] Tabs can be reordered by drag and drop
- [ ] Visual indicator shows during drag
- [ ] Settings tab opens with gear button
- [ ] Only one settings tab allowed
- [ ] Tab overflow enables horizontal scroll

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:
- **Risk**: Drag and drop behavior varies across platforms
  - **Mitigation**: Use standard HTML5 API, test on all platforms
- **Risk**: Scroll behavior inconsistent with trackpad vs mouse
  - **Mitigation**: Use CSS scroll-behavior for smooth scrolling

---

### Phase 3: Polish and Testing

**Goal**: Production quality with full test coverage, performance optimization, and edge case handling

**Files to Modify**:
- All test files - Add comprehensive test coverage
- `src/tab-bar/tab-manager.ts` - Performance optimizations
- `src/tab-bar/tab-bar-ui.ts` - Accessibility improvements
- `src/styles/tab-bar.css` - Visual polish

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Error Handling | Handle PTY failures gracefully | Error occurs | User informed, state consistent |
| Performance Monitor | Track tab switch latency | Tabs exist | Metrics logged |

**Processing Flow** (Error Handling):
```
1. PTY creation fails
   |-- Error logged with [ERROR][FRONTEND]
   |-- No tab created
   |-- User notification shown
2. Tab in inconsistent state detected
   |-- Force cleanup of partial resources
   |-- Re-render TabBarUI
```

**Implementation Steps**:

1. **Complete Test Coverage**
   - Unit tests for all components
   - Integration tests for tab workflows
   - Edge case tests (rapid switching, many tabs)

2. **Performance Optimization**
   - Lazy terminal initialization
   - DOM recycling for tab switches
   - Throttle rapid operations

3. **Error Handling**
   - PTY creation failure recovery
   - Graceful degradation on errors
   - Consistent state maintenance

4. **Accessibility**
   - ARIA labels for tab elements
   - Keyboard navigation within tab bar
   - Focus management on tab switch

**Dependencies**:
- Requires: Phase 2 (all features)
- Blocks: None (final phase)

**Testing Approach**:

*Unit Tests*:
- Error recovery paths
- Edge cases (20+ tabs, rapid switching)
- Memory cleanup verification

*Performance Tests*:
- Tab switch < 50ms
- Tab creation < 200ms
- 60fps during animations

*Manual Testing*:
- [ ] 20 tabs work without lag
- [ ] Rapid tab switching is smooth
- [ ] Error messages are clear
- [ ] Works on Linux, Windows, macOS

**Acceptance Criteria**:
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] Tab switch < 50ms (performance requirement)
- [ ] Tab creation < 200ms (performance requirement)
- [ ] Error handling works correctly
- [ ] Works on all supported platforms

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:
- **Risk**: Performance regression with many tabs
  - **Mitigation**: Lazy initialization, DOM recycling
- **Risk**: Platform-specific rendering issues
  - **Mitigation**: Test on all platforms during development

---

## Complete File Structure

```
src/
├── tab-bar/
│   ├── index.ts              # Module entry, re-exports
│   ├── types.ts              # Tab, TabType, TabBarState interfaces
│   ├── tab-manager.ts        # Tab lifecycle and state management
│   ├── tab-manager.test.ts   # TabManager unit tests
│   ├── tab-bar-ui.ts         # DOM rendering and click handlers
│   ├── tab-bar-ui.test.ts    # UI component tests
│   ├── keyboard-handler.ts   # Tab keyboard shortcuts (Ctrl+T, etc.)
│   ├── keyboard-handler.test.ts
│   ├── drag-handler.ts       # Drag and drop reordering
│   └── drag-handler.test.ts
├── settings/
│   ├── index.ts              # Settings module entry
│   └── settings-panel.ts     # Settings UI (placeholder)
├── terminal-app/
│   └── index.ts              # (existing) Terminal instance per tab
├── pty/
│   └── client.ts             # (existing) PTY communication
├── main.ts                   # Updated to use TabManager
├── index.html                # Add tab bar container
├── styles.css                # (existing) Import tab-bar.css
└── styles/
    └── tab-bar.css           # Tab bar styles
```

**File Descriptions**:
- `tab-bar/types.ts`: Type definitions shared across tab bar modules
- `tab-bar/tab-manager.ts`: Core state management, handles tab lifecycle
- `tab-bar/tab-bar-ui.ts`: Creates and updates DOM elements for tab bar
- `tab-bar/keyboard-handler.ts`: Intercepts keyboard shortcuts for tab operations
- `tab-bar/drag-handler.ts`: Implements HTML5 drag and drop for reordering
- `settings/settings-panel.ts`: Placeholder for future settings implementation

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner
- Mock Tauri APIs and DOM where needed
- Table-driven tests for multiple scenarios

**Test Coverage Goals**:
- Core logic (TabManager): 90%+ coverage
- UI components (TabBarUI): 70%+ coverage
- Handlers (keyboard, drag): 80%+ coverage

**Key Test Areas**:

1. **TabManager** (`src/tab-bar/tab-manager.ts`)
   - createTab creates unique IDs
   - createTab emits 'tab:created' and 'tab:activated' events
   - closeTab removes correct tab
   - closeTab emits 'tab:closed' event
   - switchTab updates active states
   - switchTab emits 'tab:activated' and 'tab:deactivated' events
   - reorderTabs(draggedTabId, targetTabId, position) moves tabs correctly
   - reorderTabs emits 'tab:reordered' event
   - handleSessionExit cleans up properly
   - Last tab close triggers exit signal
   - on()/off() correctly manage event subscriptions

2. **TabKeyboardHandler** (`src/tab-bar/keyboard-handler.ts`)
   - Ctrl+T calls createTab
   - Ctrl+W calls closeActiveTab
   - Ctrl+Tab cycles forward
   - Ctrl+Shift+Tab cycles backward
   - Ctrl+1-9 activates by index
   - Non-tab shortcuts pass through

3. **TabDragHandler** (`src/tab-bar/drag-handler.ts`)
   - Drag start sets data transfer
   - Drag over calculates insertion index
   - Drop triggers reorder
   - Settings tab not draggable

### Integration Testing

**Scenarios**:
1. Create tab -> verify PTY session exists
2. Switch tabs -> verify correct content shown
3. Shell exit -> verify tab closes automatically
4. Multiple tabs -> verify isolation

### Manual Testing Checklist

**From Specification Test Scenarios**:
- [ ] Create 3 tabs, switch between them, close middle tab
- [ ] Open settings, close settings, reopen settings
- [ ] Drag tab to reorder, verify new order persists
- [ ] Exit shell with `exit` command, verify tab closes
- [ ] Close all tabs, verify application exits

**Edge Cases**:
- [ ] PTY creation fails - error shown, no tab created
- [ ] Very long tab title - truncated with ellipsis
- [ ] 20+ tabs - horizontal scroll works
- [ ] Rapid tab switching - no race conditions
- [ ] Rapid tab creation - blocked while creating (state machine)
- [ ] Close during create - queued until create completes
- [ ] Create during close - blocked until close completes

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| @tauri-apps/api | (existing) | Tauri IPC | Already installed |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: types.ts -> tab-manager.ts -> tab-bar-ui.ts -> keyboard-handler.ts
2. Phase 2: drag-handler.ts, settings-panel.ts (depends on Phase 1)
3. Phase 3: Testing and polish (depends on Phase 2)

**Component Dependencies**:
- `TabBarUI` depends on `TabManager`
- `TabKeyboardHandler` depends on `TabManager`
- `TabDragHandler` depends on `TabBarUI` and `TabManager`
- `TabManager` uses `TerminalApp` (existing)
- `TerminalApp` uses `PtyClient` (existing)

## Risk Assessment

### Technical Risks

1. **PtyClient Single-Session Assumption**
   - **Risk**: Current PtyClient has NOTE about single-session model
   - **Likelihood**: Medium
   - **Impact**: High (architecture change needed)
   - **Mitigation**: Each tab creates its own PtyClient instance; verify no shared state issues

2. **Keyboard Shortcut Conflicts**
   - **Risk**: Tab shortcuts may conflict with terminal applications
   - **Likelihood**: Medium
   - **Impact**: Medium (UX degradation)
   - **Mitigation**: Use Ctrl+Shift variants where conflicts likely; document conflicts

3. **Memory Leaks on Tab Close**
   - **Risk**: TerminalApp resources not fully cleaned up
   - **Likelihood**: Low (existing dispose method)
   - **Impact**: High (memory growth over time)
   - **Mitigation**: Verify dispose() cleanup in tests; monitor memory in E2E tests

### Implementation Risks

1. **Scope Creep**
   - **Risk**: Adding features beyond specification
   - **Mitigation**: Strict adherence to SPEC.md requirements

2. **Integration Complexity**
   - **Risk**: Modifying main.ts entry point affects existing functionality
   - **Mitigation**: Incremental changes with backward compatibility

## Performance Considerations

1. **Tab Switch Latency**
   - Target: < 50ms
   - Strategy: Hide/show containers instead of create/destroy
   - Measurement: performance.now() around switch operation

2. **Tab Creation Latency**
   - Target: < 200ms including PTY spawn
   - Strategy: Non-blocking UI update, async PTY spawn
   - Measurement: Time from click to shell prompt

3. **Memory Management**
   - Strategy: Call TerminalApp.dispose() on tab close
   - Strategy: Clear terminal buffers for closed tabs
   - Monitoring: Track heap size with multiple tabs

## Security Considerations

1. **PTY Isolation**
   - Each tab has its own PTY session
   - No cross-session data leakage
   - Session IDs validated before operations

2. **Input Validation**
   - Tab IDs validated before use
   - Session IDs from events verified against known sessions
   - No user-controlled HTML in tab titles

3. **Settings XSS**
   - Settings panel content sanitized
   - User-provided data escaped before display

## Open Questions

### From Specification:
- [ ] Should there be a maximum tab limit? (Currently: no limit)
- [ ] Settings panel content to be defined in separate specification

### Implementation-Specific:
- [x] How to handle PTY spawn race condition with tab close?
  - Resolved: Use state machine (operationState: idle | creating | closing) to prevent concurrent operations
- [ ] Should inactive tabs pause terminal rendering for performance?

## Future Enhancements

Items deferred to later phases or releases:

### Phase 2 Features (from spec - included):
- File opening capabilities (not in current spec)
- Multi-file marking (not in current spec)

### Not in Current Spec:
- Tab detach to new window
- Tab groups/folders
- Session persistence across app restart
- Custom tab colors/icons

## Success Metrics

### Functional Completeness
- [ ] All functional requirements (FR1-FR10) implemented
- [ ] All keyboard shortcuts working
- [ ] Tab drag and drop working
- [ ] Settings tab opens and closes correctly
- [ ] PTY sessions properly isolated per tab
- [ ] Shell exit triggers tab close
- [ ] Last tab close exits application

### Quality Metrics
- [ ] Test coverage meets goals
- [ ] No critical bugs in manual testing
- [ ] Code follows project conventions

### Performance Metrics
- [ ] Tab switch < 50ms
- [ ] Tab creation < 200ms
- [ ] UI responsive at 60fps

## References

- **Specification**: `doc/tasks/tab-bar/SPEC.md`
- **Requirements**: `doc/tasks/tab-bar/要件定義書.md`
- **Existing PTY Client**: `src/pty/client.ts`
- **Existing PTY Manager**: `src-tauri/src/pty/manager.rs`
- **Existing Terminal App**: `src/terminal-app/index.ts`
- **Tauri Event API**: https://v2.tauri.app/develop/calling-frontend/#listening-to-events

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm approach and timeline
   - Address open questions
   - Verify no conflicts with other development

2. **Environment Setup**
   - Ensure development environment ready
   - Verify existing tests pass

3. **Begin Implementation**
   - Start with Phase 1
   - Follow TDD approach where practical
   - Commit incrementally

4. **Continuous Verification**
   - Run tests after each component
   - Manual testing at phase boundaries
   - Performance measurement before Phase 3
