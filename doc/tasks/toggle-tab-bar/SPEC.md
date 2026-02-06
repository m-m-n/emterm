# Feature: Toggle Tab Bar Visibility

## Overview

Add a keyboard shortcut (`Ctrl+Shift+B` by default) to toggle the tab bar visibility. When hidden, the tab content area expands to fill the full window height. The visibility state is persisted in settings and restored on app restart. Transitions use MD3 Motion-compliant CSS animations.

## Objectives

- Allow users to hide/show the tab bar via a configurable keybinding
- Expand the terminal content area when the tab bar is hidden
- Persist the visibility state across app restarts
- Animate the transition with MD3-compliant smooth slide animation

## User Stories

### US1: Toggle Tab Bar with Keyboard Shortcut
As a terminal user, I want to hide the tab bar with a keyboard shortcut, so that I can maximize the terminal display area.

**Acceptance Criteria:**
- [ ] Pressing `Ctrl+Shift+B` toggles tab bar visibility
- [ ] The tab content area expands/shrinks accordingly
- [ ] The keybinding is configurable via `keybinds.toggle_tab_bar`

### US2: Persist Tab Bar Visibility
As a terminal user, I want the tab bar visibility to be remembered, so that I don't have to hide it every time I restart the app.

**Acceptance Criteria:**
- [ ] Tab bar visibility state is saved to settings on toggle
- [ ] On app startup, the saved state is restored
- [ ] Default value is `true` (tab bar visible)

### US3: Smooth Animation
As a terminal user, I want a smooth animation when toggling the tab bar, so that the UI feels polished.

**Acceptance Criteria:**
- [ ] Tab bar slides up/down smoothly when toggled
- [ ] Animation follows MD3 Motion specifications
- [ ] Animation runs at 60fps

## Technical Requirements

### Functional Requirements
- **FR1:** Add `toggle_tab_bar` field to `KeybindSettings` (default: `"Ctrl+Shift+B"`)
- **FR2:** Add `show_tab_bar` field to `AppSettings` (default: `true`)
- **FR3:** Handle the keybinding in `TabKeyboardHandler.handleKeyDown()`
- **FR4:** Toggle the tab bar visibility via CSS class and height transition
- **FR5:** Save the state to settings via `SettingsService` on each toggle
- **FR6:** Restore the state from settings on app initialization
- **FR7:** Tab operation keybindings continue to work when tab bar is hidden

### Non-Functional Requirements
- **NFR1 - Performance:** Animation must run at 60fps using CSS transitions (no JS animation)
- **NFR2 - Consistency:** Use existing keybinding system (`matchKeybindStr`) and settings infrastructure

## Implementation Approach

### Architecture

```
KeyboardEvent
  → TabKeyboardHandler.handleKeyDown()
    → matchKeybindStr(event, keybinds.toggle_tab_bar)
      → TabBarUI.toggleVisibility()
        → CSS class toggle on .tab-bar
        → SettingsService.update({ show_tab_bar: newValue })
```

### Changes Required

#### 1. TypeScript Settings Types (`src/settings/types.ts`)

Add to `AppSettings`:
```typescript
show_tab_bar: boolean;
```

Add to `KeybindSettings`:
```typescript
toggle_tab_bar: string;
```

#### 2. Rust Settings Struct (`src-tauri/`)

Add corresponding fields to the Rust `AppSettings` struct:
```rust
show_tab_bar: bool,        // default: true
```

Add to `KeybindSettings`:
```rust
toggle_tab_bar: String,    // default: "Ctrl+Shift+B"
```

#### 3. Tab Bar CSS (`src/styles/tab-bar.css`)

Add transition and hidden state:
```css
.tab-bar {
  /* existing styles */
  transition: height var(--md-motion-duration-short4) var(--md-motion-easing-standard),
              border-bottom-width var(--md-motion-duration-short4) var(--md-motion-easing-standard);
  overflow: hidden;
}

.tab-bar.hidden {
  height: 0;
  border-bottom-width: 0;
}
```

#### 4. Tab Bar UI (`src/tab-bar/tab-bar-ui.ts`)

Add method:
```typescript
setVisible(visible: boolean): void {
  if (visible) {
    this.container.classList.remove('hidden');
  } else {
    this.container.classList.add('hidden');
  }
}
```

#### 5. Keyboard Handler (`src/tab-bar/keyboard-handler.ts`)

Add toggle handler in `handleKeyDown()`:
```typescript
// Toggle tab bar
if (matchKeybindStr(event, keybinds?.toggle_tab_bar ?? "Ctrl+Shift+B")) {
  event.preventDefault();
  this.onToggleTabBar?.();
  return true;
}
```

#### 6. App Initialization (`src/main.ts`)

On startup, read `show_tab_bar` from settings and apply initial state.

### Dependencies

**Internal Dependencies:**
- `src/settings/types.ts`: AppSettings and KeybindSettings types
- `src/settings/settings-service.ts`: Settings persistence
- `src/tab-bar/tab-bar-ui.ts`: Tab bar DOM management
- `src/tab-bar/keyboard-handler.ts`: Keyboard shortcut handling
- `src/keybind/matcher.ts`: Keybind matching utility
- `src-tauri/src/settings/`: Rust settings struct and defaults

**External Dependencies:**
- None (pure CSS + existing infrastructure)

### File Structure

```
src/
├── settings/
│   └── types.ts                  # Add show_tab_bar, toggle_tab_bar
├── styles/
│   └── tab-bar.css               # Add .tab-bar.hidden, transition
├── tab-bar/
│   ├── tab-bar-ui.ts             # Add setVisible() method
│   └── keyboard-handler.ts       # Add toggle handler
├── main.ts                       # Apply initial state on startup
src-tauri/
└── src/
    └── settings/                 # Add Rust struct fields + defaults
```

## Test Scenarios

### Unit Tests
- [ ] `matchKeybindStr` matches `Ctrl+Shift+B` correctly
- [ ] `TabBarUI.setVisible(false)` adds `hidden` class
- [ ] `TabBarUI.setVisible(true)` removes `hidden` class
- [ ] Default `show_tab_bar` is `true` when setting is missing

### Integration Tests
- [ ] Toggle keybind triggers visibility change and settings save
- [ ] App startup restores saved visibility state

### Edge Cases
- [ ] Settings file missing `show_tab_bar` field: defaults to `true`
- [ ] Rapid toggling: animation handles interruption gracefully
- [ ] Tab bar hidden + new tab created: tab bar remains hidden, tab is functional
- [ ] Tab bar hidden + settings tab opened via keybind: tab bar remains hidden

## Security Considerations

- No new security concerns (purely UI feature, no external data)

## Error Handling

| Error | Condition | Handling |
|-------|-----------|---------|
| Settings read failure | Settings file corrupted | Default to `show_tab_bar: true` |
| Settings write failure | Disk error on save | Log warning, state still applies in-memory |

## Performance Optimization

### Animation Performance
- Use CSS `transition` on `height` property (GPU-compositable with `overflow: hidden`)
- Avoid JS-driven animation frames
- Use `will-change: height` only if needed for smoothness

## Success Criteria

- [ ] All functional requirements implemented and tested
- [ ] `Ctrl+Shift+B` toggles tab bar visibility with smooth animation
- [ ] Visibility state persists across app restarts
- [ ] Keybinding is configurable via settings
- [ ] Tab operations work regardless of tab bar visibility
- [ ] No regression in existing tab bar functionality
