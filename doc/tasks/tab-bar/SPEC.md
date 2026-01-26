# Feature: Tab Bar

## Overview

Implement a tab bar feature for eMterm that allows users to manage multiple terminal sessions within a single window. The tab bar appears below the OS native title bar and provides navigation, creation, and management of terminal tabs with full keyboard shortcut support.

## Objectives

- Enable multiple terminal sessions in a single window
- Provide intuitive tab management through mouse and keyboard
- Support tab reordering via drag and drop
- Display settings panel as a special tab
- Maintain low-latency performance for tab operations

## User Stories

### US1: Create New Tab
As a user, I want to create a new terminal tab, so that I can run multiple shell sessions simultaneously.

**Acceptance Criteria:**
- [ ] Clicking [+] button creates a new tab with a fresh PTY session
- [ ] Pressing Ctrl+T creates a new tab
- [ ] New tab becomes active immediately
- [ ] Default shell is launched in the new session

### US2: Switch Between Tabs
As a user, I want to switch between tabs, so that I can access different terminal sessions.

**Acceptance Criteria:**
- [ ] Clicking a tab makes it active
- [ ] Ctrl+Tab moves to the next tab
- [ ] Ctrl+Shift+Tab moves to the previous tab
- [ ] Ctrl+1 through Ctrl+9 jump to specific tabs

### US3: Close Tab
As a user, I want tabs to close automatically when the shell exits, so that terminated sessions are cleaned up.

**Acceptance Criteria:**
- [ ] Tab closes when shell process exits (exit, logout, etc.)
- [ ] Ctrl+W sends SIGTERM and closes the active tab
- [ ] Adjacent tab becomes active after closing
- [ ] Application exits when last tab closes

### US4: Reorder Tabs
As a user, I want to reorder tabs by dragging, so that I can organize my sessions.

**Acceptance Criteria:**
- [ ] Tabs can be dragged and dropped to new positions
- [ ] Visual indicator shows insertion point during drag
- [ ] Tab order persists after reordering
- [ ] Settings tab cannot be reordered

### US5: Access Settings
As a user, I want to access settings through a dedicated tab, so that I can configure the application.

**Acceptance Criteria:**
- [ ] Settings button (gear icon) opens settings tab
- [ ] Only one settings tab can exist at a time
- [ ] Settings tab can be closed with Ctrl+W

## Technical Requirements

### Functional Requirements
- **FR1:** Tab bar displays below OS native title bar
- **FR2:** Each tab corresponds to one PTY session
- **FR3:** Tab title reflects shell-provided title (OSC 0/2 sequences)
- **FR4:** Fixed tab width with ellipsis truncation for long titles
- **FR5:** No close button on tabs (auto-close on shell exit only)
- **FR6:** [+] button and settings button in fixed position (right side)
- **FR7:** Horizontal scroll when tabs overflow
- **FR8:** All keyboard shortcuts operational
- **FR9:** Drag and drop reordering
- **FR10:** Settings displayed as special tab type

### Non-Functional Requirements
- **NFR1 - Performance:** Tab switch completes within 50ms
- **NFR2 - Performance:** New tab creation within 200ms
- **NFR3 - Performance:** UI remains responsive at 60fps
- **NFR4 - Security:** PTY session isolation between tabs
- **NFR5 - Compatibility:** Works on Linux, Windows, macOS

## Implementation Approach

### Architecture

**Component Architecture:**
```
┌─────────────────────────────────────────────────────────────┐
│                     Window (Tauri)                          │
├─────────────────────────────────────────────────────────────┤
│  OS Native Title Bar: "eMterm ver.x.x.x"                    │
├─────────────────────────────────────────────────────────────┤
│                     TabBar Component                        │
│  ┌────────────────────────────────┬─────────────────────┐   │
│  │  TabScrollArea                 │  FixedButtonArea    │   │
│  │  [Tab1] [Tab2] [Tab3] ...     │  [+] [Settings]     │   │
│  └────────────────────────────────┴─────────────────────┘   │
├─────────────────────────────────────────────────────────────┤
│                  TabContent Container                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  TerminalApp (per tab) or SettingsPanel             │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

**State Management (Centralized Ownership):**
```
TabManager (owns all tab-related resources)
├── tabs: Tab[]                           // Tab data only (no TerminalApp reference)
├── terminalApps: Map<tabId, TerminalApp> // Centralized TerminalApp instances
├── eventUnlistens: Map<tabId, UnlistenFn> // Centralized event cleanup functions
├── activeTabId: string | null
├── operationState: TabOperationState
├── eventEmitter: TabEventEmitter         // Typed event emitter for state changes
├── createTab() -> Promise<Tab | null>    // Creates Tab, TerminalApp, registers cleanup
├── closeTab(id) -> Promise<boolean>      // Cleans up all resources for tab
├── switchTab(id)
├── reorderTabs(draggedTabId: string, targetTabId: string, position: 'before' | 'after')
├── getActiveTab() -> Tab | null
├── getTerminalApp(tabId) -> TerminalApp | null
├── isOperationInProgress() -> boolean
├── on(event, handler) -> UnsubscribeFn   // Subscribe to tab events
└── off(event, handler)                   // Unsubscribe from tab events

Tab (data only, no resource ownership)
├── id: string
├── sessionId: string
├── title: string
└── type: 'terminal' | 'settings'
```

**Resource Lifecycle:**
- TabManager.createTab(): Creates Tab data, TerminalApp instance, stores in Maps
- TabManager.closeTab(): Calls terminalApp.dispose(), unlisten(), removes from all Maps
- Single point of cleanup ensures no orphaned resources

### Data Flow

**New Tab Creation:**
```
User Action (Click/Ctrl+T)
       ↓
  Check: operationState === 'idle'?
       ↓ (No: ignore request)
  Set operationState = { status: 'creating' }
       ↓
  TabManager.createTab()
       ↓
  Tauri Command: create_pty_session
       ↓
  PtyManager.create_session_atomic()
       ↓
  SessionCreatedResult { session_id, count }
       ↓
  Create Tab { id, sessionId, title: "shell" }
       ↓
  Create TerminalApp instance
       ↓
  Add to tabs[], set as active
       ↓
  Set operationState = { status: 'idle' }
       ↓
  Render TabBar, show new terminal
```

**Tab Close (Shell Exit):**
```
PTY Session Exit Event
       ↓
  Event: pty-exited { session_id }
       ↓
  TabManager.handleSessionExit(sessionId)
       ↓
  Check: operationState === 'idle'?
       ↓ (No: queue for later or ignore if same tab)
  Set operationState = { status: 'closing', tabId }
       ↓
  Find tab by sessionId
       ↓
  Cleanup TerminalApp instance
       ↓
  Remove tab from tabs[]
       ↓
  If tabs.length > 0: activate adjacent tab
  Else: exit application
       ↓
  Set operationState = { status: 'idle' }
       ↓
  Re-render TabBar
```

**Tab Switch:**
```
User Action (Click/Shortcut)
       ↓
  TabManager.switchTab(targetId)
       ↓
  Update activeTabId to targetId
       ↓
  Hide previous TerminalApp container
       ↓
  Show target TerminalApp container
       ↓
  Focus terminal input
```

### API Design

#### Tauri Commands (Existing)

The following commands already exist in `PtyManager`:

**Create Session:**
```rust
// src-tauri/src/pty/manager.rs
pub async fn create_session_atomic(
    &self,
    shell: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<SessionCreatedResult, PtyError>
```

**Remove Session:**
```rust
pub async fn remove_session_atomic(
    &self,
    id: &str,
) -> Option<(Arc<Mutex<PtySession>>, SessionRemovedResult)>
```

#### Frontend Events

**PTY Exit Event:**
```typescript
// Emitted from backend when PTY session ends
interface PtyExitedEvent {
  sessionId: string;
  exitCode: number | null;
}

// Listen in TabManager
listen<PtyExitedEvent>('pty-exited', (event) => {
  this.handleSessionExit(event.payload.sessionId);
});
```

**Title Change Event:**
```typescript
// Parsed from OSC sequences in terminal output
interface TitleChangeEvent {
  sessionId: string;
  title: string;
}
```

### Data Models

#### TypeScript Types

```typescript
// src/tab-bar/types.ts

/**
 * Base tab interface with common properties (data only, no resource ownership)
 */
interface BaseTab {
  /** Unique tab identifier */
  id: string;
  /** Display title */
  title: string;
}

/**
 * Terminal tab with PTY session (data only)
 */
export interface TerminalTab extends BaseTab {
  type: 'terminal';
  /** Associated PTY session ID */
  sessionId: string;
}

/**
 * Settings tab (singleton, no PTY)
 */
export interface SettingsTab extends BaseTab {
  type: 'settings';
}

/**
 * Tab type - discriminated union for type-safe handling
 * Note: Tab is data-only; TerminalApp instances are managed by TabManager
 */
export type Tab = TerminalTab | SettingsTab;

/**
 * Type guard for terminal tabs
 */
export function isTerminalTab(tab: Tab): tab is TerminalTab {
  return tab.type === 'terminal';
}

/**
 * Type guard for settings tab
 */
export function isSettingsTab(tab: Tab): tab is SettingsTab {
  return tab.type === 'settings';
}

/**
 * Operation state for preventing race conditions
 */
export type TabOperationState =
  | { status: 'idle' }
  | { status: 'creating' }
  | { status: 'closing'; tabId: string };

/**
 * Tab bar state (centralized resource management)
 */
export interface TabBarState {
  /** All tabs (data only) */
  tabs: Tab[];
  /** Currently active tab ID */
  activeTabId: string | null;
  /** Current operation state (prevents concurrent modifications) */
  operationState: TabOperationState;
  /** TerminalApp instances keyed by tab ID (centralized ownership) */
  terminalApps: Map<string, TerminalApp>;
  /** Event unlisten functions keyed by tab ID (centralized cleanup) */
  eventUnlistens: Map<string, UnlistenFn>;
}

/**
 * Tab creation options
 */
export interface CreateTabOptions {
  /** Tab type (default: 'terminal') */
  type?: 'terminal' | 'settings';
  /** Initial title */
  title?: string;
}

/**
 * Tab event types for EventEmitter pattern
 */
export type TabEventType =
  | 'tab:created'
  | 'tab:closed'
  | 'tab:activated'
  | 'tab:deactivated'
  | 'tab:reordered'
  | 'tab:titleChanged';

/**
 * Tab event payloads (typed event data)
 */
export interface TabEventPayloads {
  'tab:created': { tab: Tab };
  'tab:closed': { tabId: string; wasActive: boolean };
  'tab:activated': { tab: Tab; previousTabId: string | null };
  'tab:deactivated': { tab: Tab };
  'tab:reordered': { tabs: Tab[] };
  'tab:titleChanged': { tabId: string; title: string };
}

/**
 * Event handler type for tab events
 */
export type TabEventHandler<T extends TabEventType> =
  (payload: TabEventPayloads[T]) => void;

/**
 * Unsubscribe function returned by on()
 */
export type UnsubscribeFn = () => void;

/**
 * Tab event emitter interface
 */
export interface TabEventEmitter {
  on<T extends TabEventType>(event: T, handler: TabEventHandler<T>): UnsubscribeFn;
  off<T extends TabEventType>(event: T, handler: TabEventHandler<T>): void;
  emit<T extends TabEventType>(event: T, payload: TabEventPayloads[T]): void;
}
```

### Dependencies

**Internal Dependencies:**
- `PtyClient` (`src/pty/client.ts`): PTY session communication
- `TerminalApp` (`src/terminal-app/`): Terminal instance per tab
- `TerminalState` (`src/terminal/state.ts`): Terminal buffer state
- `ITerminalRenderer` (`src/terminal/`): Terminal rendering

**External Dependencies:**
- `@tauri-apps/api`: Tauri frontend API (existing)
- No additional external dependencies required

### File Structure

```
src/
├── tab-bar/
│   ├── index.ts              # TabBar component entry
│   ├── types.ts              # Type definitions
│   ├── tab-manager.ts        # Tab state management
│   ├── tab-manager.test.ts   # Unit tests
│   ├── tab-bar-ui.ts         # TabBar UI rendering
│   ├── tab-bar-ui.test.ts    # UI tests
│   ├── keyboard-handler.ts   # Tab keyboard shortcuts
│   ├── keyboard-handler.test.ts
│   ├── drag-handler.ts       # Drag and drop logic
│   └── drag-handler.test.ts
├── settings/
│   ├── index.ts              # Settings panel entry
│   └── settings-panel.ts     # Settings UI
├── main.ts                   # Updated entry point
└── styles/
    └── tab-bar.css           # Tab bar styles
```

### Keyboard Shortcut Implementation

```typescript
// src/tab-bar/keyboard-handler.ts

export interface TabKeyboardShortcuts {
  'Ctrl+T': () => void;      // New tab
  'Ctrl+W': () => void;      // Close tab
  'Ctrl+Tab': () => void;    // Next tab
  'Ctrl+Shift+Tab': () => void;  // Previous tab
  'Ctrl+1': () => void;      // Tab 1
  'Ctrl+2': () => void;      // Tab 2
  // ... Ctrl+3 through Ctrl+8
  'Ctrl+9': () => void;      // Last tab (or tab 9)
}

export class TabKeyboardHandler {
  constructor(private tabManager: TabManager) {}

  /**
   * Handle keydown event
   * @returns true if event was handled (should stop propagation)
   */
  handleKeyDown(event: KeyboardEvent): boolean {
    if (!event.ctrlKey) return false;

    switch (event.key) {
      case 't':
      case 'T':
        event.preventDefault();
        this.tabManager.createTab();
        return true;

      case 'w':
      case 'W':
        event.preventDefault();
        this.tabManager.closeActiveTab();
        return true;

      case 'Tab':
        event.preventDefault();
        if (event.shiftKey) {
          this.tabManager.activatePreviousTab();
        } else {
          this.tabManager.activateNextTab();
        }
        return true;

      case '1': case '2': case '3': case '4':
      case '5': case '6': case '7': case '8':
        event.preventDefault();
        this.tabManager.activateTabByIndex(parseInt(event.key) - 1);
        return true;

      case '9':
        event.preventDefault();
        this.tabManager.activateLastTab();
        return true;

      default:
        return false;
    }
  }
}
```

### UI Styling

```css
/* src/styles/tab-bar.css */

.tab-bar {
  display: flex;
  height: 32px;
  background: var(--tab-bar-bg, #1e1e1e);
  border-bottom: 1px solid var(--tab-bar-border, #333);
}

.tab-scroll-area {
  flex: 1;
  display: flex;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: none; /* Firefox */
}

.tab-scroll-area::-webkit-scrollbar {
  display: none; /* Chrome, Safari */
}

.tab {
  display: flex;
  align-items: center;
  padding: 0 16px;
  min-width: 120px;
  max-width: 180px;
  height: 100%;
  background: var(--tab-bg, #2d2d2d);
  border-right: 1px solid var(--tab-border, #333);
  cursor: pointer;
  user-select: none;
}

.tab.active {
  background: var(--tab-active-bg, #3c3c3c);
  border-bottom: 2px solid var(--tab-active-indicator, #007acc);
}

.tab:hover:not(.active) {
  background: var(--tab-hover-bg, #353535);
}

.tab-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tab-fixed-area {
  display: flex;
  align-items: center;
  padding: 0 8px;
  gap: 4px;
}

.tab-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  cursor: pointer;
  border-radius: 4px;
}

.tab-button:hover {
  background: var(--button-hover-bg, #3c3c3c);
}

/* Drag and drop */
.tab.dragging {
  opacity: 0.5;
}

.tab-drop-indicator {
  position: absolute;
  width: 2px;
  height: 100%;
  background: var(--drop-indicator, #007acc);
}
```

## Test Scenarios

### Unit Tests

**TabManager Tests:**
- [ ] `createTab()` creates tab with unique ID and sessionId
- [ ] `closeTab()` removes tab and updates activeTabId
- [ ] `switchTab()` updates activeTabId correctly
- [ ] `reorderTabs()` moves tab to correct position
- [ ] `getActiveTab()` returns current active tab
- [ ] Closing last tab triggers application exit signal

**TabKeyboardHandler Tests:**
- [ ] Ctrl+T calls `createTab()`
- [ ] Ctrl+W calls `closeActiveTab()`
- [ ] Ctrl+Tab calls `activateNextTab()`
- [ ] Ctrl+Shift+Tab calls `activatePreviousTab()`
- [ ] Ctrl+1-9 calls `activateTabByIndex()` with correct index
- [ ] Non-tab shortcuts are not intercepted

**DragHandler Tests:**
- [ ] Drag start sets correct data
- [ ] Drag over shows insertion indicator
- [ ] Drop reorders tabs correctly
- [ ] Settings tab is not draggable

### Integration Tests

- [ ] Create multiple tabs and verify each has separate PTY session
- [ ] Switch tabs and verify correct terminal content shown
- [ ] Shell exit in one tab closes only that tab
- [ ] All keyboard shortcuts work end-to-end
- [ ] Tab scroll appears when many tabs exist

### E2E Tests

- [ ] Scenario: Create 3 tabs, switch between them, close middle tab
- [ ] Scenario: Open settings, close settings, reopen settings
- [ ] Scenario: Drag tab to reorder, verify new order persists
- [ ] Scenario: Exit shell with `exit` command, verify tab closes
- [ ] Scenario: Close all tabs, verify application exits

### Edge Cases

- [ ] Creating tab when PTY creation fails - show error, no tab created
- [ ] Switching to non-existent tab - no-op
- [ ] Closing already-closed tab - no-op
- [ ] Very long tab title - truncated with ellipsis
- [ ] 20+ tabs - horizontal scroll works correctly
- [ ] Rapid tab switching - no race conditions
- [ ] Rapid tab creation - blocked while creating (state machine)
- [ ] Close during create - queued until create completes
- [ ] Create during close - blocked until close completes

### Performance Tests

- [ ] Tab switch: < 50ms (measured with performance.now())
- [ ] Tab creation: < 200ms including PTY spawn
- [ ] 10 tabs open: Memory usage reasonable, no leaks
- [ ] Drag reorder: Smooth animation at 60fps

## Security Considerations

- **PTY Isolation:** Each tab has its own PTY session; no cross-session data leakage
- **Input Validation:** Tab IDs and session IDs validated before operations
- **Settings XSS:** Settings panel content sanitized if user-provided data displayed
- **Event Source Verification:** Only handle events from trusted Tauri backend

## Error Handling

### Error Codes

| Code | Description | HTTP Status | User Message |
|------|-------------|-------------|--------------|
| TAB_CREATE_FAILED | PTY session creation failed | N/A | "Failed to create new terminal session" |
| TAB_NOT_FOUND | Tab ID not found in state | N/A | (silent, log only) |
| SESSION_NOT_FOUND | PTY session not found | N/A | (silent, log only) |

### Error Flow

```
Error Occurs
     ↓
Log error with [ERROR][FRONTEND] prefix
     ↓
If user-facing: Show notification
If internal: Continue gracefully
     ↓
Clean up partial state if needed
```

### Error Recovery

- **PTY Creation Failure:** Log error, no tab created, user can retry
- **Tab Switch Failure:** Log warning, stay on current tab
- **Cleanup Failure:** Log error, force remove from state

## Performance Optimization

### Performance Goals
- Tab switch latency: < 50ms
- New tab creation: < 200ms
- UI frame rate: 60fps during animations

### Optimization Strategies

1. **Lazy Terminal Initialization:** Only initialize TerminalApp when tab first becomes active
2. **DOM Recycling:** Reuse DOM elements when switching tabs (hide/show vs create/destroy)
3. **Efficient State Updates:** Batch state updates to minimize re-renders
4. **Event Throttling:** Throttle rapid tab switch requests

### Memory Management

- Dispose TerminalApp properly when tab closes
- Clear terminal buffers for closed tabs
- Limit scroll-back buffer per tab if memory constrained

## Success Criteria

- [ ] All functional requirements (FR1-FR10) implemented
- [ ] All keyboard shortcuts working
- [ ] Tab drag and drop working
- [ ] Settings tab opens and closes correctly
- [ ] PTY sessions properly isolated per tab
- [ ] Shell exit triggers tab close
- [ ] Last tab close exits application
- [ ] Performance meets NFR targets
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] E2E tests pass
- [ ] Code review completed

## Open Questions

- [ ] Should there be a maximum tab limit? (Currently: no limit)
- [ ] Settings panel content to be defined in separate specification

## Implementation Phases

### Phase 1: Core Tab Infrastructure
**Goals:** Basic tab management without drag support
**Deliverables:**
- TabManager class with create/close/switch
- TabBar UI with tabs and fixed buttons
- PTY session per tab
- Basic keyboard shortcuts (Ctrl+T, Ctrl+W, Ctrl+Tab)

### Phase 2: Advanced Features
**Goals:** Complete feature set
**Deliverables:**
- Drag and drop reordering
- Ctrl+1-9 shortcuts
- Tab scroll for overflow
- Settings tab implementation

### Phase 3: Polish and Testing
**Goals:** Production quality
**Deliverables:**
- Full test coverage
- Performance optimization
- Edge case handling
- Documentation

## References

- Requirements Document: `doc/tasks/tab-bar/要件定義書.md`
- Existing PTY Manager: `src-tauri/src/pty/manager.rs`
- Terminal App: `src/terminal-app/`
- Tauri Config: `src-tauri/tauri.conf.json`
- Test Guidelines: `test/README.md`
