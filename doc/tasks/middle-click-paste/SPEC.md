# Feature: Middle-Click Clipboard Paste

## Overview

Add middle mouse button (wheel click) paste support to the terminal. When the user middle-clicks anywhere in the terminal area, the system clipboard contents are pasted into the PTY. This follows the conventional behavior of Linux/X11 terminals.

## Objectives

- Paste clipboard contents on middle mouse button click
- Reuse existing paste logic (multi-line confirmation dialog, chunked sending)
- Provide a settings toggle to enable/disable the feature

## User Stories

### US1: Middle-Click Paste

As a terminal user, I want to paste clipboard contents by middle-clicking in the terminal, so that I can quickly paste without keyboard shortcuts.

**Acceptance Criteria:**
- [ ] Middle-clicking in the terminal area reads from the system clipboard
- [ ] Single-line text is pasted immediately
- [ ] Multi-line text shows the existing confirmation dialog before pasting
- [ ] Text is sent via the existing chunked paste mechanism

### US2: Settings Control

As a user, I want to enable or disable middle-click paste via settings, so that I can control this behavior based on my preference.

**Acceptance Criteria:**
- [ ] A `middle_click_paste` boolean setting exists (default: `true`)
- [ ] When disabled, middle-click does not trigger paste
- [ ] Setting is persisted in the config file

## Technical Requirements

### Functional Requirements

- **FR1:** Middle mouse button click (button === 1) in the terminal area triggers clipboard paste
- **FR2:** Paste behavior is identical to Ctrl+Shift+V: multi-line confirmation dialog, chunked sending via `sendTextInChunks`
- **FR3:** Middle-click paste takes priority over PTY mouse tracking (always pastes regardless of mouse tracking mode)
- **FR4:** `middle_click_paste` boolean setting with default value `true`

### Non-Functional Requirements

- **NFR1 - Responsiveness:** Paste should feel instant for single-line text (no perceptible delay)
- **NFR2 - Consistency:** Behavior must be identical to existing keyboard paste (Ctrl+Shift+V)

## Implementation Approach

### Event Handling

The middle-click event should be intercepted before PTY mouse tracking. The `auxclick` event (button === 1) or `mousedown` with `event.button === 1` should be used.

### Data Flow

```
Middle-Click (button === 1)
  → Check middle_click_paste setting
  → Read clipboard via ClipboardBridge.read()
  → If multi-line: show paste confirmation dialog
  → Send text via sendTextInChunks() to PTY
```

### Settings

Add to `AppSettings`:
- Rust: `middle_click_paste: bool` with `default = "default_true"`
- TypeScript: `middle_click_paste: boolean` in `AppSettings` interface

### File Structure

```
src/terminal-app/handlers/mouse.ts  # Add middle-click handler
src/settings/types.ts               # Add setting type
src-tauri/src/commands/config.rs     # Add Rust setting field
```

## Test Scenarios

### Unit Tests
- [ ] Middle-click with empty clipboard does nothing
- [ ] Middle-click with single-line text pastes directly
- [ ] Middle-click with multi-line text triggers confirmation dialog
- [ ] Middle-click when setting is disabled does nothing
- [ ] Middle-click pastes even when mouse tracking is enabled

## Success Criteria

- [ ] Middle-click pastes clipboard text in the terminal
- [ ] Multi-line confirmation dialog works the same as Ctrl+Shift+V
- [ ] Setting toggle works correctly
- [ ] No regression in existing mouse handling (selection, PTY tracking, wheel scroll)
