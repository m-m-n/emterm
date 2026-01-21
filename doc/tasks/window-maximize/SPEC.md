# Feature: Window Maximize on Startup

## Overview

Configure the application window to start in maximized state by default. Users can resize or restore the window after startup.

## Objectives

- Start the application with a maximized window
- Allow users to resize and restore the window
- Implement via hardcoded configuration (settings file support planned for future)

## User Stories

### US1: Maximized Window on Startup
As a user, I want the terminal to start maximized, so that I can begin work with maximum screen space immediately.

**Acceptance Criteria:**
- [ ] Window opens in maximized state on application launch
- [ ] Terminal content fills the maximized window

### US2: Window Resize
As a user, I want to resize the window after startup, so that I can adjust the layout to my needs.

**Acceptance Criteria:**
- [ ] User can restore (un-maximize) the window
- [ ] User can resize the window by dragging edges
- [ ] User can re-maximize the window

## Technical Requirements

### Functional Requirements
- **FR1:** Window must open in maximized state on startup
- **FR2:** Window must remain resizable (resizable: true)
- **FR3:** Fullscreen mode is not used (fullscreen: false)

### Non-Functional Requirements
- **NFR1 - Performance:** No measurable impact on startup time
- **NFR2 - Compatibility:** Works on Linux (primary), Windows and macOS (future)

## Implementation Approach

### Configuration Change

Modify `src-tauri/tauri.conf.json` to add `maximized: true` to the window configuration.

**Current Configuration:**
```json
{
  "app": {
    "windows": [
      {
        "title": "eMterm",
        "width": 800,
        "height": 600,
        "resizable": true,
        "fullscreen": false
      }
    ]
  }
}
```

**Target Configuration:**
```json
{
  "app": {
    "windows": [
      {
        "title": "eMterm",
        "width": 800,
        "height": 600,
        "resizable": true,
        "fullscreen": false,
        "maximized": true
      }
    ]
  }
}
```

### Tauri Window Configuration Options

| Property | Value | Description |
|----------|-------|-------------|
| maximized | true | Start window in maximized state |
| resizable | true | Allow user to resize window |
| fullscreen | false | Not fullscreen mode |
| width | 800 | Default width (used when restored) |
| height | 600 | Default height (used when restored) |

### File Changes

| File | Change |
|------|--------|
| `src-tauri/tauri.conf.json` | Add `"maximized": true` to window config |

## Test Scenarios

### Manual Tests
- [ ] Launch application and verify window is maximized
- [ ] Click restore button and verify window becomes 800x600
- [ ] Drag window edges and verify resize works
- [ ] Click maximize button and verify window maximizes again
- [ ] Minimize and restore window

### Platform Tests
- [ ] Linux: Verify maximized startup works
- [ ] (Future) Windows: Verify maximized startup works
- [ ] (Future) macOS: Verify maximized startup works

## Success Criteria

- [ ] Application starts with maximized window
- [ ] User can restore and resize window
- [ ] User can re-maximize window
- [ ] No regression in existing functionality

## Future Enhancements

- Settings file support for window state configuration
- Remember last window position and size
- Multiple window support with independent states

## References

- Tauri Window Configuration: https://tauri.app/reference/config/#windowconfig
- Requirements Document: `doc/tasks/window-maximize/要件定義書.md`
