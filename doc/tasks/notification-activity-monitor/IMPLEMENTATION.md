# Implementation Plan: Notification / Activity Monitor

## Overview

Add activity monitoring to eMterm that displays dot indicators on inactive tabs when activity occurs, and sends OS desktop notifications when the window is not focused.

## Objectives

- Display accent-colored dot indicators on inactive tabs for process exit, new output, and BEL events
- Send OS desktop notifications via tauri-plugin-notification when eMterm is not focused
- Provide settings UI to customize notification triggers and enable/disable features
- Implement throttling to prevent performance degradation during high-frequency output

## Prerequisites

### Development Environment
- Rust 1.85+ with Tauri 2.x
- Bun (package manager and bundler)
- Docker (for testing)

### Dependencies
- tauri-plugin-notification v2 (new external dependency)
- @tauri-apps/plugin-notification (new npm dependency)

### Knowledge Requirements
- Existing TabManager event system (TypedEventEmitter pattern)
- Existing PtyClient event model (session-based filtering)
- Existing settings pattern (Rust serde + TS AppSettings + settings-sections renderers)
- BEL handling via `state.onBell` callback in TerminalApp

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), Rust (backend plugin registration)
- **Framework**: Tauri 2.x
- **Key Libraries**:
  - tauri-plugin-notification - OS native desktop notifications
  - Existing TabManager/PtyClient - Tab lifecycle and PTY events

### Design Approach

Three new components with clear separation of concerns:

1. **TabActivityTracker** - Monitors events, manages per-tab activity state, emits activity callbacks
2. **NotificationManager** - Tracks window focus, dispatches desktop notifications with throttling
3. **TabBarUI modifications** - Renders/clears dot indicators on tab elements

The components are loosely coupled via callbacks, following the existing pattern where TabManager emits events and consumers subscribe independently.

### Component Interaction

```
TabActivityTracker
  ├── listens to: TabManager events (tab:activated, tab:closed)
  ├── listens to: TerminalApp callbacks (onBell, onOutput, onSessionExit)
  ├── reads: SettingsService for trigger configuration
  ├── notifies: TabBarUI (via callback) to show/hide dot
  └── notifies: NotificationManager (via callback) to send desktop notification

NotificationManager
  ├── tracks: window blur/focus events (or Tauri onFocusChanged) for window focus state
  ├── calls: @tauri-apps/plugin-notification API
  └── reads: SettingsService for notification_enabled flag
```

## Implementation Phases

### Phase 1: Backend Infrastructure (Rust + Tauri Plugin)

**Goal**: Register tauri-plugin-notification and add notification settings fields to the backend.

**Files to Modify**:
- `src-tauri/Cargo.toml` - Add `tauri-plugin-notification = "2"` dependency
- `src-tauri/src/lib.rs` - Register `.plugin(tauri_plugin_notification::init())`
- `src-tauri/capabilities/default.json` - Add notification permission identifiers
- `src-tauri/src/commands/config.rs` - Add 5 new boolean fields with serde defaults to AppSettings
- `src-tauri/locales/en.json` - Add validation/error message keys
- `src-tauri/locales/ja.json` - Add validation/error message keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Cargo.toml | Declare notification plugin dependency | Plugin not present | Plugin compiles with project |
| lib.rs plugin registration | Initialize notification plugin at app startup | Plugin in Cargo.toml | Plugin available to frontend JS API |
| capabilities/default.json | Grant notification permissions to webview | Plugin registered | Frontend can call notification API |
| AppSettings fields (config.rs) | Store notification preferences persistently | Settings struct exists | 5 new bool fields with defaults serialize/deserialize correctly |

**Processing Flow**:
```
1. Add dependency and plugin registration
   └─ Build and verify compilation succeeds
2. Add capability permissions
   └─ Verify schema generation includes new permissions
3. Add settings fields with serde defaults
   ├─ notification_enabled: default true
   ├─ tab_activity_indicator: default true
   ├─ notify_on_process_exit: default true
   ├─ notify_on_output: default false
   └─ notify_on_bell: default true
```

**Implementation Steps**:

1. **Add notification plugin to Cargo.toml and register in lib.rs**
   - Add crate dependency
   - Register plugin in the Tauri builder chain
   - Key considerations:
     - Plugin must be registered before `.run()`
     - Order relative to other plugins does not matter

2. **Add notification permissions to capabilities**
   - Add `notification:default`, `notification:allow-is-permission-granted`, `notification:allow-request-permission`, `notification:allow-notify`

3. **Add notification settings fields to Rust AppSettings (`src-tauri/src/commands/config.rs`)**
   - Follow existing pattern: `serde(default = "fn_name")` for each field
   - Add `deserialize_null_default` where appropriate
   - Key considerations:
     - Must match TypeScript AppSettings exactly
     - Defaults must be applied when field is missing from existing settings.json

**Dependencies**:
- Requires: Nothing (first phase)
- Blocks: Phase 2 (frontend depends on backend plugin), Phase 3 (settings UI depends on fields)

**Testing Approach**:

*Unit Tests*:
- Test settings serialization/deserialization with and without notification fields
- Test default values are applied for missing fields

*Integration Tests*:
- Verify build succeeds with new plugin

**Acceptance Criteria**:
- [ ] `cargo build` succeeds with notification plugin
- [ ] New settings fields serialize/deserialize with correct defaults
- [ ] Existing settings.json without notification fields loads successfully

**Estimated Effort**: 小

---

### Phase 2: Frontend Core (TabActivityTracker + NotificationManager)

**Goal**: Implement activity tracking for inactive tabs and desktop notification dispatch with throttling.

**Files to Create**:
- `src/tab-bar/tab-activity-tracker.ts` - Central activity monitoring coordinator
- `src/notification/notification-manager.ts` - Desktop notification dispatch with window focus tracking

**Files to Modify**:
- `src/tab-bar/types.ts` - Add `ActivityType` type and activity-related event types
- `src/settings/types.ts` - Add 5 notification setting fields to AppSettings interface
- `src/main.ts` - Initialize TabActivityTracker and NotificationManager, wire up to TabManager
- `src/terminal-app/index.ts` - Add `onBell` and `onOutput` callback registration for activity tracking
- `package.json` - Add `@tauri-apps/plugin-notification` dependency

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TabActivityTracker | Monitor inactive tab events, manage per-tab activity flags | TabManager initialized | Activity state tracked per tab, callbacks invoked on activity |
| NotificationManager | Track window focus, dispatch throttled desktop notifications | Plugin available | OS notification sent when window inactive and activity occurs |
| ActivityType | Type definition for activity event classification | None | Shared type used by tracker and notification manager |

**Processing Flow (Activity Detection)**:
```
1. Event occurs (pty_exit / output / BEL) for a specific session
2. TabActivityTracker resolves session → tabId
3. Check if tabId is the active tab
   ├─ Active tab → ignore (no indicator needed)
   └─ Inactive tab → continue
4. Check settings for this trigger type (notify_on_process_exit, etc.)
   ├─ Disabled → ignore
   └─ Enabled → continue
5. For output events, apply throttle (max 1/sec per tab)
   ├─ Throttled → skip
   └─ Not throttled → continue
6. Mark activity on tab (set hasActivity flag)
7. Invoke activity callback (TabBarUI will show dot)
8. Invoke notification callback (NotificationManager will evaluate)
```

**Processing Flow (Desktop Notification)**:
```
1. NotificationManager receives activity notification
2. Check notification_enabled setting
   ├─ Disabled → return
   └─ Enabled → continue
3. Check window focus state (isWindowActive flag, tracked via window blur/focus events)
   ├─ Window active → return (no notification)
   └─ Window inactive → continue
4. Apply per-tab throttle (max 1/5sec, process_exit bypasses)
   ├─ Throttled → skip
   └─ Not throttled → send notification
5. Compose notification (title: "eMterm", body: "{tab title}: {event description}")
6. Call notification API to send
```

**Processing Flow (Indicator Clear)**:
```
1. TabManager emits "tab:activated" with tab info
2. TabActivityTracker receives event
3. Clear hasActivity flag for activated tab
4. Invoke clear callback (TabBarUI will hide dot)
```

**Implementation Steps**:

1. **Add TypeScript types and npm dependency**
   - Add `ActivityType` to tab-bar types
   - Add notification settings to AppSettings interface
   - Install @tauri-apps/plugin-notification via bun
   - Key considerations:
     - AppSettings fields must exactly match Rust struct field names

2. **Implement TabActivityTracker**
   - Manages `Map<string, TabActivityState>` keyed by tabId
   - Subscribes to TabManager events for tab lifecycle
   - Provides methods: `markActivity(tabId, type)`, `clearActivity(tabId)`, `hasActivity(tabId)`
   - Output throttling via per-tab timer (1 second window)
   - Invokes registered callbacks on activity/clear
   - Key considerations:
     - Must resolve sessionId → tabId (TabManager maintains this mapping)
     - Must handle tab close (cleanup activity state and clear per-tab throttle timers)
     - Throttle only for "output" type; "process_exit" and "bell" are always immediate

3. **Implement NotificationManager**
   - Tracks window focus via `window blur/focus` events (or Tauri `getCurrentWebviewWindow().onFocusChanged()`)
   - `notify(tabTitle, activityType)` respects throttle and settings
   - Per-tab throttle timer (5 second window, process_exit bypasses)
   - Notification body format: "{tabTitle}: Process exited" / "New output" / "Bell"
   - Key considerations:
     - Must request OS notification permission on first use
     - Must handle permission denied gracefully (log warning, skip)
     - Notification content: tab title + event type only (never terminal output)

4. **Wire up in main.ts**
   - Create TabActivityTracker after TabManager initialization
   - Create NotificationManager
   - Connect TabActivityTracker callbacks to TabBarUI and NotificationManager
   - Key considerations:
     - Initialization order: TabManager → TabBarUI → TabActivityTracker → NotificationManager
     - Cleanup in `cleanup()` function

5. **Add activity callbacks to TerminalApp**
   - Add `onBellActivity` and `onOutputActivity` callback setters (public methods, following `onSessionExit`/`onTitleChange` pattern)
   - Store callbacks as private fields: `private bellActivityCallback` and `private outputActivityCallback`
   - In `handleBell()` (private): invoke `this.bellActivityCallback?.()` after existing bell action
   - In `setupPtyHandlers()` (private): invoke `this.outputActivityCallback?.()` inside the `onTerminalActions` handler after processing actions
   - Key considerations:
     - Bell callback already exists (`state.onBell`), extend `handleBell()` to also notify activity
     - Output callback: trigger on each `terminal_actions` event batch (throttled by tracker)
     - No need to change method visibility; only add public callback registration methods

**Dependencies**:
- Requires: Phase 1 (Rust plugin and settings fields)
- Blocks: Phase 3 (TabBarUI dot requires TabActivityTracker), Phase 4 (settings UI)

**Testing Approach**:

*Unit Tests*:
- Test TabActivityTracker marks activity only for inactive tabs
- Test TabActivityTracker clears activity on tab activation
- Test output throttling (second mark within 1 sec is suppressed)
- Test TabActivityTracker ignores active tab events
- Test NotificationManager respects window focus state
- Test NotificationManager per-tab throttle (5 sec window)
- Test process_exit bypasses notification throttle
- Test settings flags enable/disable individual triggers

*Integration Tests*:
- Test full flow: event → tracker → notification callback invoked
- Test full flow: tab activated → tracker → clear callback invoked

**Acceptance Criteria**:
- [ ] TabActivityTracker correctly identifies inactive tabs and marks activity
- [ ] Activity is cleared when tab is activated
- [ ] Output events are throttled (max 1/sec per tab)
- [ ] NotificationManager sends notification only when window is inactive
- [ ] Desktop notification throttling works (max 1/5sec per tab, process_exit bypasses)
- [ ] Settings flags correctly control behavior
- [ ] TypeScript type check passes

**Estimated Effort**: 中

**Risks and Mitigation**:
- **Risk**: Notification permission denied by OS
  - **Mitigation**: Log warning and gracefully skip; feature degrades to tab indicator only

---

### Phase 3: Tab Bar UI (Dot Indicator)

**Goal**: Display and clear accent-colored dot indicators on tab elements.

**Files to Modify**:
- `src/tab-bar/tab-bar-ui.ts` - Add dot element to tab, show/hide via TabActivityTracker callbacks
- `src/styles/tab-bar.css` - Add `.tab-activity-dot` styles

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Tab dot element | Visual indicator DOM element within each tab | Tab element exists | Dot visible/hidden based on activity state |
| CSS styles | Styling for dot (size, color, position) | Tab CSS exists | 8px accent-colored circle positioned left of title |

**Processing Flow**:
```
1. TabActivityTracker invokes activity callback with tabId
2. TabBarUI finds tab element for tabId
3. Show dot indicator element (remove hidden class)
---
1. TabManager emits "tab:activated"
2. TabBarUI receives event (existing handler)
3. TabActivityTracker invokes clear callback
4. TabBarUI hides dot indicator element (add hidden class)
```

**Implementation Steps**:

1. **Add dot element to tab creation**
   - In `addTabElement()`, create a `<span class="tab-activity-dot">` element
   - Insert before the title element
   - Initially hidden
   - Key considerations:
     - Only add to terminal tabs (settings tab does not need activity indicator)
     - Store reference in a Map for quick access

2. **Add show/hide methods**
   - `showActivityDot(tabId)` - removes hidden class from dot element
   - `hideActivityDot(tabId)` - adds hidden class to dot element
   - Connected to TabActivityTracker callbacks in main.ts
   - Key considerations:
     - Check `tab_activity_indicator` setting before showing

3. **Add CSS styles**
   - 8px circle using `--md-sys-color-primary` (matches accent/active tab color)
   - Positioned left of tab title with `margin-right: 8px`
   - Hidden by default (`.tab-activity-dot { display: none; }`)
   - Visible state: `.tab-activity-dot.active { display: inline-block; }`

**Dependencies**:
- Requires: Phase 2 (TabActivityTracker provides activity callbacks)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests*:
- Test dot element is created for terminal tabs but not settings tabs
- Test showActivityDot adds visible class
- Test hideActivityDot removes visible class
- Test dot is cleaned up when tab element is removed

*Manual Testing*:
- [ ] Visual confirmation: dot appears on inactive tab when activity occurs
- [ ] Visual confirmation: dot disappears when tab is clicked/activated
- [ ] Visual confirmation: dot color matches theme accent color
- [ ] Visual confirmation: dot does not appear on active tab

**Acceptance Criteria**:
- [ ] Dot element rendered in correct position on inactive tabs with activity
- [ ] Dot uses theme accent color (`--md-sys-color-primary`)
- [ ] Dot clears on tab activation
- [ ] Settings toggle `tab_activity_indicator` controls visibility
- [ ] No dot on settings tabs

**Estimated Effort**: 小

---

### Phase 4: Settings UI + i18n

**Goal**: Add notification settings section to the settings panel with i18n support.

**Files to Modify**:
- `src/settings/settings-sections.ts` - Add notification settings section with toggles
- `src/i18n/locales/en.json` - Add notification-related i18n keys
- `src/i18n/locales/ja.json` - Add notification-related i18n keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Notification settings section | Render 5 toggle switches for notification preferences | Settings panel exists | Users can configure notification behavior |
| i18n keys | Localized labels and descriptions for notification settings | i18n system exists | Labels display in user's language |

**Processing Flow**:
```
1. User opens settings panel
2. Settings sections renderer creates "Notification" category
3. Render 5 toggle switches:
   ├─ Desktop Notification (notification_enabled)
   ├─ Tab Activity Indicator (tab_activity_indicator)
   ├─ Notify on Process Exit (notify_on_process_exit)
   ├─ Notify on Output (notify_on_output)
   └─ Notify on Bell (notify_on_bell)
4. User toggles a switch
5. saveSetting() persists change
6. TabActivityTracker/NotificationManager read setting on next event
```

**Implementation Steps**:

1. **Add i18n keys for both locales**
   - English and Japanese labels, descriptions for each setting
   - Section title: "Notification" / "通知"
   - Key considerations:
     - Follow existing naming pattern: `settings.notification.{key}`

2. **Add notification section to settings-sections.ts**
   - Create new section renderer following existing pattern
   - Use `renderToggle()` for each of the 5 boolean settings
   - Position: after "Terminal Behavior" section (or as a new category)
   - Key considerations:
     - Follow existing `renderToggle()` pattern used for `url_detection`, `copy_on_select`, etc.
     - Settings are read-on-demand by tracker/manager, no need for applier notification

3. **Register new section in settings panel category list**
   - Add "notification" to category list in settings-panel.ts
   - Key considerations:
     - Match i18n key pattern for category title

**Dependencies**:
- Requires: Phase 1 (Rust settings fields), Phase 2 (TypeScript settings types)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests*:
- Test i18n keys exist in both en.json and ja.json
- Test settings save/load round-trip for notification fields

*Manual Testing*:
- [ ] Settings panel shows notification section with 5 toggles
- [ ] Toggle state matches saved settings
- [ ] Changing toggles immediately affects notification behavior
- [ ] Labels display correctly in English and Japanese

**Acceptance Criteria**:
- [ ] All 5 notification settings are configurable in the UI
- [ ] Settings persist across application restarts
- [ ] i18n labels display correctly in both languages
- [ ] Default values are correct (enabled: true, indicator: true, process_exit: true, output: false, bell: true)

**Estimated Effort**: 小

---

## Complete File Structure

```
src/
├── tab-bar/
│   ├── tab-activity-tracker.ts    # NEW: Activity monitoring coordinator
│   ├── tab-bar-ui.ts              # MODIFIED: Add dot indicator element + show/hide methods
│   ├── tab-manager.ts             # EXISTING (no changes needed)
│   └── types.ts                   # MODIFIED: Add ActivityType
├── notification/
│   └── notification-manager.ts    # NEW: Desktop notification with window focus tracking
├── terminal-app/
│   └── index.ts                   # MODIFIED: Add onBellActivity/onOutputActivity callbacks
├── settings/
│   ├── types.ts                   # MODIFIED: Add 5 notification fields to AppSettings
│   └── settings-sections.ts       # MODIFIED: Add notification settings section
├── styles/
│   └── tab-bar.css                # MODIFIED: Add .tab-activity-dot styles
├── main.ts                        # MODIFIED: Initialize tracker/manager, wire callbacks
├── i18n/locales/
│   ├── en.json                    # MODIFIED: Add notification i18n keys
│   └── ja.json                    # MODIFIED: Add notification i18n keys
src-tauri/
├── src/
│   ├── lib.rs                     # MODIFIED: Register notification plugin
│   └── commands/config.rs         # MODIFIED: Add notification settings fields to AppSettings
├── Cargo.toml                     # MODIFIED: Add tauri-plugin-notification
├── capabilities/default.json      # MODIFIED: Add notification permissions
└── locales/
    ├── en.json                    # MODIFIED: Add validation keys (if needed)
    └── ja.json                    # MODIFIED: Add validation keys (if needed)
```

## Testing Strategy

### Unit Testing

**Approach**:
- Bun test for TypeScript (tab-activity-tracker, notification-manager)
- Cargo test for Rust (settings serialization)

**Test Coverage Goals**:
- TabActivityTracker logic: 80%+
- NotificationManager logic: 60% (OS API difficult to mock fully)
- Settings: 90%+

**Key Test Areas**:
1. **TabActivityTracker** - Activity marking, clearing, throttling, settings-based filtering
2. **NotificationManager** - Window focus tracking, throttle logic, settings check
3. **Settings** - Rust serde round-trip, TypeScript type matching

### Integration Testing

**Scenarios**:
1. Full event flow: PTY exit → tracker → dot shown
2. Full event flow: tab activated → tracker → dot cleared
3. Settings change → behavior change on next event

### E2E Testing (Docker)

- [ ] Build succeeds with notification plugin: `cargo build`
- [ ] TypeScript type check passes: `bun run typecheck`
- [ ] Bun tests pass: `bun test`
- [ ] Rust tests pass: `cargo test`

### Manual Testing (E2E Not Possible)

- [ ] Desktop notification appears when window is not focused and process exits
- [ ] Desktop notification does not appear when window is focused
- [ ] Dot indicator visible on inactive tab when process exits in background
- [ ] Dot clears when switching to that tab
- [ ] BEL character triggers indicator on inactive tab
- [ ] High-frequency output does not spam notifications (throttle verification)
- [ ] Settings toggles control notification behavior correctly
- [ ] Notification content shows tab title and event type only

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| tauri-plugin-notification | 2 | OS desktop notifications (Rust) | Cargo.toml |
| @tauri-apps/plugin-notification | latest | Notification JS API (frontend) | bun add |

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1 (Rust backend: plugin + settings) - no dependencies
2. Phase 2 (Frontend core: tracker + manager) - depends on Phase 1
3. Phase 3 (Tab bar UI: dot indicator) - depends on Phase 2
4. Phase 4 (Settings UI + i18n) - depends on Phases 1 and 2

**Component Dependencies**:
- TabActivityTracker depends on TabManager (event subscription)
- TabActivityTracker depends on SettingsService (trigger settings)
- NotificationManager depends on @tauri-apps/plugin-notification
- NotificationManager depends on SettingsService (enabled flag)
- TabBarUI dot depends on TabActivityTracker callbacks
- Settings section depends on Rust AppSettings fields

## Risk Assessment

### Technical Risks

1. **Notification Permission Denied**
   - **Risk**: OS may block notification permission
   - **Likelihood**: Low (Tauri handles permission flow)
   - **Impact**: Low (tab indicator still works)
   - **Mitigation**: Graceful degradation to tab indicator only; log warning

2. **Output Throttle Tuning**
   - **Risk**: 1 second throttle may feel too slow or too fast
   - **Likelihood**: Medium
   - **Impact**: Low (UX preference)
   - **Mitigation**: Constants are easily tunable; can make configurable later if needed

3. **Session-to-Tab Mapping**
   - **Risk**: Race conditions during tab creation/closing while events arrive
   - **Likelihood**: Low
   - **Impact**: Medium (missed or misrouted indicator)
   - **Mitigation**: TabManager already handles session lifecycle; tracker subscribes to tab:closed for cleanup

4. **Process Exit Tab Auto-Close Conflict**
   - **Risk**: Current `TabManager.handleSessionExit()` closes the tab immediately on process exit, making it impossible to show a dot indicator for process_exit events on inactive tabs
   - **Likelihood**: High (this is current behavior)
   - **Impact**: High (FR1 partially broken for process_exit)
   - **Mitigation**: Modify the process exit flow to send notification and show indicator BEFORE closing the tab, or decouple process exit from auto-close (e.g., keep tab open with "exited" state)

## Performance Considerations

1. **Output Event Throttling**
   - Tab indicator: max 1 DOM update per second per inactive tab
   - Desktop notification: max 1 per 5 seconds per tab
   - Process exit bypasses desktop throttle (always important to notify)

2. **Window Focus Tracking**
   - Single `blur/focus` event listener pair (minimal overhead)
   - Boolean flag checked on each notification request

3. **Settings Lookup**
   - Use `SettingsService.getCached()` (already in-memory, no async)
   - Checked on each event (fast path: single boolean read)

## Security Considerations

1. **Notification Content** - Only tab title and event type in notification body; never include terminal output content
2. **Permission** - tauri-plugin-notification handles OS permission flow
3. **Settings Validation** - All notification settings are boolean only (no free-text input)

## References

- **Specification**: `doc/tasks/notification-activity-monitor/SPEC.md`
- **Existing tab system**: `src/tab-bar/`
- **Settings pattern**: `src/settings/types.ts`, Rust AppSettings in `src-tauri/src/commands/config.rs`
- **tauri-plugin-notification**: https://v2.tauri.app/plugin/notification/
- **BEL handler**: `src/terminal/handlers/c0_handlers.ts` → `state.onBell` callback
- **PTY events**: `src/pty/client.ts` → `onExit()`, `onTerminalActions()`
