# Feature: Notification / Activity Monitor

## Overview

Add activity monitoring to eMterm that notifies users when background tabs have new activity. This includes visual indicators (dots) on inactive tabs and OS desktop notifications when the eMterm window is not focused. The feature targets users running long-running commands (e.g., Claude Code tasks, cargo build) who switch to other tabs or applications while waiting.

## Objectives

- Display activity indicator dots on inactive tabs when events occur
- Send OS desktop notifications when eMterm window is not focused
- Allow users to customize notification behavior through settings
- Maintain performance with throttling for high-frequency output

## User Stories

### US1: See Activity on Inactive Tabs
As a terminal user, I want to see which background tabs have new activity, so that I know when a long-running command has finished.

**Acceptance Criteria:**
- [ ] A dot indicator appears on inactive tabs when activity occurs
- [ ] The dot uses the theme's accent color
- [ ] The dot clears automatically when the tab is activated

### US2: Receive Desktop Notifications
As a terminal user, I want to receive OS notifications when processes complete while I'm in another app, so that I don't miss task completions.

**Acceptance Criteria:**
- [ ] Desktop notification is sent when eMterm window is not active
- [ ] Notification shows tab title and event type
- [ ] Notifications are throttled (max 1 per 5 seconds per tab)

### US3: Customize Notification Behavior
As a terminal user, I want to configure which events trigger notifications, so that I only get notified about events I care about.

**Acceptance Criteria:**
- [ ] Desktop notifications can be enabled/disabled
- [ ] Tab indicator can be enabled/disabled
- [ ] Individual trigger conditions (process exit, output, bell) can be toggled

## Technical Requirements

### Functional Requirements

- **FR1:** Track activity on inactive terminal tabs (process exit, new output, BEL character)
- **FR2:** Display dot indicator on tab element when activity occurs on inactive tab
- **FR3:** Clear indicator when tab becomes active
- **FR4:** Track window focus state (active/inactive)
- **FR5:** Send OS desktop notification when window is inactive and activity occurs
- **FR6:** Throttle output-based notifications (indicator: max 1/sec, desktop: max 1/5sec)
- **FR7:** Provide settings for notification customization
- **FR8:** Notification content shows tab title and event type only (no terminal output content)

### Non-Functional Requirements

- **NFR1 - Performance:** High-frequency output must not cause excessive DOM updates or notification spam; throttling required
- **NFR2 - Security:** Notification body must not include terminal output content (only tab title + event type)
- **NFR3 - Platform:** Linux notification via libnotify/D-Bus; macOS/Windows via Tauri plugin native support

## Implementation Approach

### Architecture

```
┌──────────────────────────────────────────────────────┐
│                    Frontend (TS)                      │
│                                                       │
│  ┌─────────────────┐  ┌──────────────────────────┐   │
│  │  TabBarUI        │  │  NotificationManager     │   │
│  │  - dot indicator │  │  - window focus tracking  │   │
│  │  - clear on      │  │  - throttling             │   │
│  │    activate      │  │  - settings integration   │   │
│  └────────┬─────────┘  └──────────┬───────────────┘   │
│           │                       │                    │
│  ┌────────┴───────────────────────┴───────────────┐   │
│  │            TabActivityTracker                   │   │
│  │  - monitors inactive tab events                 │   │
│  │  - emits activity notifications                 │   │
│  │  - manages per-tab activity state               │   │
│  └────────────────────┬───────────────────────────┘   │
│                       │                                │
│  ┌────────────────────┴───────────────────────────┐   │
│  │   Existing: TabManager + PtyClient + OSC        │   │
│  │   (pty_exit, pty_output, BEL from terminal)     │   │
│  └────────────────────────────────────────────────┘   │
│                                                       │
├───────────────────────────────────────────────────────┤
│                    Backend (Rust)                      │
│  ┌────────────────────────────────────────────────┐   │
│  │  tauri-plugin-notification                      │   │
│  │  (OS native notification API)                   │   │
│  └────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────┘
```

### Component Design

#### TabActivityTracker (new: `src/tab-bar/tab-activity-tracker.ts`)

Central coordinator for tab activity monitoring.

**Responsibilities:**
- Listen to PtyClient events (pty_exit, pty_output) for all sessions
- Listen to terminal BEL events
- Determine if the event's tab is inactive
- Emit activity events for inactive tabs
- Manage per-tab activity state (has activity flag)
- Clear activity state when tab becomes active

**Interface:**
```typescript
interface TabActivityState {
  hasActivity: boolean;
  lastActivityTime: number;
}

class TabActivityTracker {
  private activityStates: Map<string, TabActivityState>;  // keyed by tabId
  private outputThrottleTimers: Map<string, number>;       // keyed by tabId

  constructor(tabManager: TabManager);

  /** Check if a tab has unread activity */
  hasActivity(tabId: string): boolean;

  /** Mark activity on a tab (called internally) */
  markActivity(tabId: string): void;

  /** Clear activity for a tab (called on tab activation) */
  clearActivity(tabId: string): void;

  /** Register activity callback */
  onActivity(callback: (tabId: string, type: ActivityType) => void): UnsubscribeFn;

  dispose(): void;
}

type ActivityType = "process_exit" | "output" | "bell";
```

#### NotificationManager (new: `src/notification/notification-manager.ts`)

Handles desktop notification dispatch.

**Responsibilities:**
- Track window focus state via `window blur/focus` events (or Tauri `onFocusChanged`)
- Send OS notifications via tauri-plugin-notification when window is inactive
- Throttle notifications per tab (max 1 per 5 seconds)
- Read notification settings to determine if notifications should be sent

**Interface:**
```typescript
class NotificationManager {
  private isWindowActive: boolean;
  private notificationThrottleTimers: Map<string, number>;  // keyed by tabId

  constructor();

  /** Whether the window is currently active/focused */
  get windowActive(): boolean;

  /** Send a desktop notification (respects throttle and settings) */
  notify(tabTitle: string, activityType: ActivityType): void;

  dispose(): void;
}
```

#### TabBarUI Changes (existing: `src/tab-bar/tab-bar-ui.ts`)

**Additions:**
- Add dot indicator element to each tab element in `addTabElement()`
- Show/hide dot based on activity state
- Clear dot on `tab:activated` event
- CSS class: `.tab-activity-dot`

#### Settings Additions

**Rust (`src-tauri/src/commands/config.rs`):**
```rust
// New fields in AppSettings
notification_enabled: bool,        // default: true
tab_activity_indicator: bool,      // default: true
notify_on_process_exit: bool,      // default: true
notify_on_output: bool,            // default: false
notify_on_bell: bool,              // default: true
```

**TypeScript (`src/settings/types.ts`):**
```typescript
// New fields in AppSettings
notification_enabled: boolean;
tab_activity_indicator: boolean;
notify_on_process_exit: boolean;
notify_on_output: boolean;
notify_on_bell: boolean;
```

### Data Flow

#### Activity Detection Flow
```
PTY Process Exit → pty_exit event → TabActivityTracker.markActivity()
                                  → TabBarUI shows dot
                                  → NotificationManager.notify() (if window inactive)

PTY Output       → pty_output event → TabActivityTracker (throttled)
                                     → TabBarUI shows dot
                                     → NotificationManager.notify() (if window inactive)

BEL Character    → Terminal parser → TabActivityTracker.markActivity()
                                   → TabBarUI shows dot
                                   → NotificationManager.notify() (if window inactive)
```

#### Indicator Clear Flow
```
User clicks tab → TabManager.switchTab()
               → tab:activated event
               → TabActivityTracker.clearActivity()
               → TabBarUI hides dot
```

### Dependencies

**Internal Dependencies:**
- TabManager: For tab state, active tab tracking, event subscription
- PtyClient: For pty_exit, pty_output events (with session_id for tab mapping)
- Terminal parser: For BEL character detection
- Settings system: For notification preferences
- Settings UI: For notification settings section

**External Dependencies:**
- tauri-plugin-notification: v2 (Tauri 2.x compatible) - OS native notifications

### Plugin Setup (tauri-plugin-notification)

**Cargo.toml:**
```toml
tauri-plugin-notification = "2"
```

**src-tauri/src/lib.rs:**
```rust
.plugin(tauri_plugin_notification::init())
```

**src-tauri/capabilities/default.json:**
```json
{
  "permissions": [
    "notification:default",
    "notification:allow-is-permission-granted",
    "notification:allow-request-permission",
    "notification:allow-notify"
  ]
}
```

**Frontend (npm):**
```bash
bun add @tauri-apps/plugin-notification
```

### File Structure

```
src/
├── tab-bar/
│   ├── tab-activity-tracker.ts    # NEW: Activity monitoring coordinator
│   ├── tab-bar-ui.ts              # MODIFIED: Add dot indicator
│   ├── tab-manager.ts             # MODIFIED: Integrate activity tracker
│   └── types.ts                   # MODIFIED: Add activity types
├── notification/
│   └── notification-manager.ts    # NEW: Desktop notification handling
├── settings/
│   ├── types.ts                   # MODIFIED: Add notification settings
│   └── settings-sections.ts       # MODIFIED: Add notification section
├── styles/
│   └── tab-bar.css                # MODIFIED: Add dot indicator styles
src-tauri/
├── src/
│   ├── commands/config.rs          # MODIFIED: Add notification settings fields
│   └── lib.rs                     # MODIFIED: Add notification plugin
├── Cargo.toml                     # MODIFIED: Add notification dependency
├── capabilities/default.json      # MODIFIED: Add notification permissions
src-tauri/locales/
├── en.json                        # MODIFIED: Add notification i18n keys
├── ja.json                        # MODIFIED: Add notification i18n keys
src/i18n/locales/
├── en.json                        # MODIFIED: Add notification UI i18n keys
├── ja.json                        # MODIFIED: Add notification UI i18n keys
```

## Throttling Strategy

### Tab Indicator Throttling
- For `output` type only (process_exit and bell are always immediate)
- Max 1 indicator update per second per tab
- Implementation: debounce timer per tab in TabActivityTracker

### Desktop Notification Throttling
- For all activity types
- Max 1 notification per 5 seconds per tab
- Implementation: throttle timer per tab in NotificationManager
- Process exit notifications bypass throttle (always notify)

## Test Scenarios

### Unit Tests
- [ ] TabActivityTracker.markActivity() sets activity flag for correct tab
- [ ] TabActivityTracker.clearActivity() resets activity flag
- [ ] TabActivityTracker does not mark activity on active tab
- [ ] Output throttling limits activity marks to 1/sec
- [ ] Settings flags correctly enable/disable individual triggers
- [ ] NotificationManager respects window focus state
- [ ] Notification throttling limits to 1/5sec per tab

### Integration Tests
- [ ] pty_exit event triggers indicator on inactive tab
- [ ] pty_output event triggers indicator on inactive tab (with throttle)
- [ ] BEL character triggers indicator on inactive tab
- [ ] Tab activation clears indicator
- [ ] Desktop notification sent when window is inactive
- [ ] Desktop notification not sent when window is active
- [ ] Settings changes take effect immediately

### Edge Cases
- [ ] Rapid tab switching during activity
- [ ] Multiple tabs receiving activity simultaneously
- [ ] Tab closed while having activity indicator
- [ ] Notification sent for tab that is subsequently closed
- [ ] Very high output rate (stress test throttling)

## Security Considerations

- **Notification Content:** Only tab title and event type in notification body; never include terminal output content
- **Permission:** tauri-plugin-notification handles OS permission requests
- **Input Validation:** Settings values are validated (boolean only, no free-text)

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Throttling prevents performance degradation during high-frequency output
- [ ] Desktop notifications work on Linux (primary target)
- [ ] Settings UI correctly controls notification behavior
- [ ] No terminal output content leaks into notifications

## Open Questions

- None (all questions resolved in requirements gathering)

## References

- Feature proposal: `tmp/AI-features.md` (item 4)
- Existing tab system: `src/tab-bar/`
- Settings pattern: `src/settings/types.ts`, `src-tauri/src/commands/config.rs`
- tauri-plugin-notification docs: https://v2.tauri.app/plugin/notification/
