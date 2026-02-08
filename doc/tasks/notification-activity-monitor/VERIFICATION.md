# Verification Document: Notification / Activity Monitor

## Overview
**Feature**: Notification / Activity Monitor
**SPEC.md**: `doc/tasks/notification-activity-monitor/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/notification-activity-monitor/IMPLEMENTATION.md`

## Build Verification

### Rust Build
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --manifest-path src-tauri/Cargo.toml"
```

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Expected Result
- Exit code: 0
- No error messages

## Test Verification

### Rust Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### TypeScript Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### Coverage Target
- **Minimum**: 70%
- **Target**: 80%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | TabActivityTracker.markActivity() sets flag for correct tab | hasActivity returns true for marked tab only | Unit |
| TS-02 | TabActivityTracker.clearActivity() resets flag | hasActivity returns false after clear | Unit |
| TS-03 | TabActivityTracker does not mark activity on active tab | Active tab's hasActivity remains false | Unit |
| TS-04 | Output throttling limits marks to 1/sec per tab | Second markActivity within 1s is suppressed | Unit |
| TS-05 | Settings flags enable/disable triggers | Disabled trigger type does not invoke callback | Unit |
| TS-06 | NotificationManager respects window focus state | No notification when window is active | Unit |
| TS-07 | Notification throttle limits to 1/5sec per tab | Second notify within 5s is suppressed | Unit |
| TS-08 | Process exit bypasses notification throttle | Notification sent even within throttle window | Unit |
| TS-09 | pty_exit triggers indicator on inactive tab | Dot shows on correct tab element | Integration |
| TS-09a | pty_output triggers indicator on inactive tab (with throttle) | Dot shows, subsequent outputs within 1s suppressed | Integration |
| TS-10 | Tab activation clears indicator | Dot hidden after tab switch | Integration |
| TS-11 | BEL triggers indicator on inactive tab | Dot shows on correct tab | Integration |
| TS-11a | Desktop notification sent when window is inactive | Notification API called when window not focused | Integration |
| TS-11b | Desktop notification NOT sent when window is active | No notification when window is focused | Integration |
| TS-11c | Settings changes take effect immediately | Toggling setting changes behavior on next event | Integration |
| TS-12 | Rapid tab switching during activity | No stale indicators after switch | Edge |
| TS-13 | Multiple tabs receiving activity simultaneously | Each tab shows independent indicator | Edge |
| TS-14 | Tab closed while having activity indicator | No errors, state cleaned up | Edge |
| TS-15 | Settings round-trip (Rust serde) | New fields serialize/deserialize with defaults | Unit |
| TS-16 | Settings round-trip (missing fields) | Existing settings.json loads with defaults for new fields | Unit |

## Code Quality Verification

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Rust Static Analysis
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"
```

## File Structure Verification

### Files to Create
- `src/tab-bar/tab-activity-tracker.ts` - Activity monitoring coordinator
- `src/notification/notification-manager.ts` - Desktop notification with window focus tracking

### Files to Modify
- `src-tauri/Cargo.toml` - Add tauri-plugin-notification dependency
- `src-tauri/src/lib.rs` - Register notification plugin
- `src-tauri/src/commands/config.rs` - Add notification settings fields to AppSettings
- `src-tauri/capabilities/default.json` - Add notification permissions
- `src/tab-bar/types.ts` - Add ActivityType
- `src/tab-bar/tab-bar-ui.ts` - Add dot indicator element and show/hide
- `src/terminal-app/index.ts` - Add activity callbacks (onBell, onOutput)
- `src/settings/types.ts` - Add 5 notification fields to AppSettings
- `src/settings/settings-sections.ts` - Add notification settings section
- `src/styles/tab-bar.css` - Add .tab-activity-dot styles
- `src/main.ts` - Initialize tracker and manager
- `src/i18n/locales/en.json` - Add notification i18n keys
- `src/i18n/locales/ja.json` - Add notification i18n keys
- `package.json` - Add @tauri-apps/plugin-notification

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | All functional requirements implemented and tested | Run test suite, verify all TS scenarios pass |
| SC-2 | All test scenarios pass | `bun test` and `cargo test` exit 0 |
| SC-3 | Throttling prevents performance degradation | TS-04 (output 1/sec) and TS-07 (notification 1/5sec) pass |
| SC-4 | Desktop notifications work on Linux | Manual test: run eMterm, switch to another window, trigger process exit |
| SC-5 | Settings UI correctly controls behavior | Manual test: toggle settings, verify behavior changes |
| SC-6 | No terminal output content in notifications | Code review: verify notification body format is tab title + event type only |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: Track activity on inactive tabs | Phase 2 | TS-01, TS-03, TS-09, TS-11 |
| FR2: Display dot indicator | Phase 3 | Manual: visual confirmation |
| FR3: Clear indicator on tab activation | Phase 2 + 3 | TS-02, TS-10 |
| FR4: Track window focus state | Phase 2 | TS-06 |
| FR5: Send desktop notification when inactive | Phase 2 | TS-06, TS-11a, TS-11b, Manual: desktop notification appears |
| FR6: Throttle notifications | Phase 2 | TS-04, TS-07, TS-08 |
| FR7: Settings customization | Phase 1 + 4 | TS-05, TS-15, TS-16, Manual: settings UI |
| FR8: Notification content = tab title + event type | Phase 2 | Code review, manual inspection |

## E2E Testing (Docker)

### Build Verification
- [ ] Rust build succeeds: `cargo build --manifest-path src-tauri/Cargo.toml`
- [ ] TypeScript type check passes: `bun run typecheck`
- [ ] Rust tests pass: `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] Bun tests pass: `bun test`

### Settings Persistence
- [ ] Settings with new notification fields save and load correctly
- [ ] Existing settings without notification fields load with correct defaults

## Manual Testing (E2E Not Possible)

Items requiring actual desktop environment:

### Tab Indicator
- [ ] Dot indicator appears on inactive tab when process exits in background tab
- [ ] Dot indicator appears on inactive tab when BEL character received
- [ ] Dot indicator appears on inactive tab when output occurs (if notify_on_output = true)
- [ ] Dot indicator uses theme accent color
- [ ] Dot clears when tab is activated (clicked)
- [ ] No dot appears on active tab
- [ ] No dot appears on settings tab

### Desktop Notifications
- [ ] Desktop notification appears when eMterm window is not focused and process exits
- [ ] Desktop notification does NOT appear when window is focused
- [ ] Notification shows tab title and event type (not terminal content)
- [ ] Notification throttling: rapid events produce at most 1 notification per 5 seconds
- [ ] Process exit always produces notification (bypasses throttle)
- [ ] OS notification permission request handled correctly

### Settings UI
- [ ] Notification section appears in settings panel
- [ ] 5 toggle switches display correctly
- [ ] Default values: notification=ON, indicator=ON, process_exit=ON, output=OFF, bell=ON
- [ ] Toggling notification_enabled disables desktop notifications
- [ ] Toggling tab_activity_indicator disables dot display
- [ ] Toggling individual triggers (process_exit, output, bell) controls which events trigger
- [ ] Settings persist after application restart
- [ ] Labels display correctly in English
- [ ] Labels display correctly in Japanese

### Performance
- [ ] High-frequency output (e.g., `yes | head -10000`) does not cause UI lag
- [ ] Dot indicator updates at most 1/sec during rapid output
- [ ] No notification spam during rapid output

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | - | ✅ | - |
| Unit Tests | 10 | ✅ | - | - |
| Integration Tests | 7 | ✅ | - | - |
| Edge Case Tests | 3 | ✅ | - | - |
| Code Quality | 2 | - | ✅ | - |
| Settings Persistence | 2 | - | ✅ | - |
| File Structure | 15 | ✅ | - | - |
| SPEC Compliance | 6 | Partial | - | ✅ |
| Tab Indicator | 7 | - | - | ✅ |
| Desktop Notification | 6 | - | - | ✅ |
| Settings UI | 10 | - | - | ✅ |
| Performance | 3 | - | - | ✅ |

**Total**: 20 automated unit/integration test items, 6 E2E (Docker) items, 26 manual items
