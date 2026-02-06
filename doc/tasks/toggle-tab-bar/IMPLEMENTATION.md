# Implementation Plan: Toggle Tab Bar Visibility

## Overview

Implement a keyboard shortcut (`Ctrl+Shift+B` by default) to toggle tab bar visibility. When hidden, the terminal content area expands to fill the full window height. The visibility state is persisted in settings and restored on app restart, with MD3-compliant smooth slide animation.

## Objectives

- Add configurable keybinding to toggle tab bar visibility
- Expand content area when tab bar is hidden
- Persist visibility state across app restarts
- Animate transitions using MD3 Motion specifications

## Prerequisites

### Development Environment
- Rust toolchain (for Tauri backend)
- Bun (for TypeScript frontend)
- Running `bun install` to ensure dependencies

### Dependencies
- No new external dependencies required
- Uses existing `SettingsService`, `matchKeybindStr`, and CSS variable system

### Knowledge Requirements
- Understanding of existing keybind system (`src/keybind/matcher.ts`)
- Familiarity with settings flow (TypeScript → Rust → JSON file)
- CSS transitions and Material Design 3 Motion tokens

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), Rust (backend)
- **Framework**: Tauri
- **Key Libraries**:
  - Existing `SettingsService` for persistence
  - Existing `matchKeybindStr` for keybind matching

### Design Approach
Minimal changes to existing architecture. Add a new boolean setting (`show_tab_bar`) and keybind setting (`toggle_tab_bar`). The toggle logic is handled in `TabKeyboardHandler`, with visibility applied via CSS class toggle on `.tab-bar`.

### Component Interaction
```
KeyboardEvent
  → TabKeyboardHandler.handleKeyDown()
    → matchKeybindStr(event, keybinds.toggle_tab_bar)
      → TabBarUI.setVisible(newValue)
        → CSS class toggle on .tab-bar
      → SettingsService.update({ show_tab_bar: newValue })
```

## Implementation Phases

### Phase 1: Settings Infrastructure

**Goal**: Add `show_tab_bar` and `toggle_tab_bar` fields to both TypeScript and Rust settings structures.

**Files to Modify**:
- `src/settings/types.ts`:
  - Add `show_tab_bar: boolean` to `AppSettings`
  - Add `toggle_tab_bar: string` to `KeybindSettings`
- `src-tauri/src/commands/config.rs`:
  - Add `show_tab_bar: bool` field to `AppSettings` struct
  - Add `toggle_tab_bar: String` field to `KeybindSettings` struct
  - Add default value functions and deserializers

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `AppSettings.show_tab_bar` | Store tab bar visibility state | Settings loaded | Boolean value available |
| `KeybindSettings.toggle_tab_bar` | Store configurable keybind | Settings loaded | Keybind string available |

**Processing Flow**:
```
1. Settings file read
   ├─ Field exists → Use stored value
   └─ Field missing/null → Use default (true for show_tab_bar, "Ctrl+Shift+B" for toggle_tab_bar)
2. Settings returned to frontend
```

**Implementation Steps**:

1. **Add TypeScript type definitions**
   - Extend `AppSettings` interface with `show_tab_bar`
   - Extend `KeybindSettings` interface with `toggle_tab_bar`

2. **Add Rust struct fields**
   - Add `show_tab_bar` field with `#[serde(default = "default_true")]`
   - Add `toggle_tab_bar` field with custom default function
   - Add null deserializer for both fields

3. **Update Rust Default impl**
   - Add field initializations in `impl Default for AppSettings`
   - Add field initialization in `impl Default for KeybindSettings`

**Dependencies**:
- Requires: None (foundation layer)
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests*:
- Test `show_tab_bar` defaults to `true` when missing
- Test `toggle_tab_bar` defaults to `"Ctrl+Shift+B"` when missing
- Test null values fall back to defaults
- Test round-trip serialization

**Acceptance Criteria**:
- [ ] TypeScript types include new fields
- [ ] Rust struct includes new fields with serde attributes
- [ ] Default values are correct (true, "Ctrl+Shift+B")
- [ ] Null/missing fields deserialize to defaults
- [ ] Existing tests pass

**Estimated Effort**: 小 (1-2 days)

---

### Phase 2: CSS Animation

**Goal**: Add CSS classes and transitions for tab bar show/hide animation.

**Files to Modify**:
- `src/styles/tab-bar.css`:
  - Add transition properties to `.tab-bar`
  - Add `.tab-bar.hidden` class

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `.tab-bar` transition | Animate height changes | Element rendered | Smooth transition on height change |
| `.tab-bar.hidden` | Hide tab bar | Class applied | Height 0, border hidden |

**Processing Flow**:
```
1. CSS class change triggers
   ├─ Add .hidden → Height transitions to 0px
   └─ Remove .hidden → Height transitions to 48px
2. Content area automatically fills space (flexbox)
```

**Implementation Steps**:

1. **Add transition to .tab-bar**
   - Add `transition` property for `height` and `border-bottom-width`
   - Use MD3 Motion tokens: `--md-motion-duration-short4` and `--md-motion-easing-standard`
   - Add `overflow: hidden` to prevent content flash during transition

2. **Add .tab-bar.hidden class**
   - Set `height: 0`
   - Set `border-bottom-width: 0`

**Dependencies**:
- Requires: None (independent CSS changes)
- Blocks: Phase 3 (requires CSS for visual effect)

**Testing Approach**:

*Manual Testing (E2E Not Possible)*:
- Verify animation runs at 60fps
- Verify animation duration feels appropriate
- Verify no visual glitches during transition

**Acceptance Criteria**:
- [ ] Tab bar animates smoothly when `.hidden` class is toggled
- [ ] Animation uses MD3 Motion tokens
- [ ] No layout shift or flash during animation
- [ ] Content area expands/contracts smoothly

**Estimated Effort**: 小 (1-2 days)

---

### Phase 3: Toggle Logic and Integration

**Goal**: Implement keyboard handler, UI toggle method, and app initialization integration.

**Files to Modify**:
- `src/tab-bar/tab-bar-ui.ts`:
  - Add `setVisible(visible: boolean)` method
  - Add `isVisible(): boolean` getter
- `src/tab-bar/keyboard-handler.ts`:
  - Add `onToggleTabBar` callback option
  - Add toggle keybind handling in `handleKeyDown()`
- `src/main.ts`:
  - Apply initial visibility from settings on startup
  - Wire up toggle callback to save settings

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `TabBarUI.setVisible()` | Toggle CSS class | TabBarUI initialized | `.hidden` class added/removed |
| `TabBarUI.isVisible()` | Return current state | TabBarUI initialized | Boolean visibility state |
| `TabKeyboardHandler` toggle | Handle keybind, invoke callback | Handler attached | Callback invoked on match |
| App initialization | Apply saved state | Settings loaded | Tab bar visibility matches setting |

**Processing Flow**:
```
1. User presses Ctrl+Shift+B
2. TabKeyboardHandler.handleKeyDown() matches keybind
3. onToggleTabBar callback invoked
4. TabBarUI.setVisible(!currentState) called
5. SettingsService.update({ show_tab_bar: newState })
6. Settings saved to disk
```

**Implementation Steps**:

1. **Add TabBarUI visibility methods**
   - Implement `setVisible(visible: boolean)` to toggle `.hidden` class
   - Implement `isVisible()` getter based on class presence

2. **Extend TabKeyboardHandler**
   - Add `onToggleTabBar?: () => void` callback to constructor options
   - Add toggle keybind check in `handleKeyDown()` before other checks
   - Call callback when toggle keybind matches

3. **Update main.ts initialization**
   - After loading settings, apply `show_tab_bar` to TabBarUI
   - Create toggle callback that:
     - Gets current visibility from TabBarUI
     - Inverts it and calls setVisible()
     - Updates settings via SettingsService
   - Pass callback to TabKeyboardHandler constructor

**Dependencies**:
- Requires: Phase 1 (settings fields), Phase 2 (CSS classes)
- Blocks: None (final phase)

**Testing Approach**:

*Unit Tests*:
- Test `TabBarUI.setVisible(false)` adds `.hidden` class
- Test `TabBarUI.setVisible(true)` removes `.hidden` class
- Test `TabBarUI.isVisible()` returns correct state
- Test keybind matcher recognizes `Ctrl+Shift+B`

*Integration Tests*:
- Test toggle keybind triggers callback
- Test settings are updated on toggle

*E2E Testing (Docker)*:
- [ ] Press Ctrl+Shift+B and verify tab bar hides
- [ ] Press Ctrl+Shift+B again and verify tab bar shows
- [ ] Verify settings file contains updated `show_tab_bar` value

**Acceptance Criteria**:
- [ ] `Ctrl+Shift+B` toggles tab bar visibility
- [ ] Tab bar animates with CSS transition
- [ ] Settings are saved on each toggle
- [ ] Initial state is restored from settings on app start
- [ ] Tab operations (Ctrl+Shift+T, etc.) work when tab bar is hidden
- [ ] Custom keybind can be configured

**Estimated Effort**: 小 (1-2 days)

---

## Complete File Structure

```
src/
├── settings/
│   └── types.ts                  # Add show_tab_bar, toggle_tab_bar
├── styles/
│   └── tab-bar.css               # Add .tab-bar.hidden, transition
├── tab-bar/
│   ├── tab-bar-ui.ts             # Add setVisible(), isVisible()
│   └── keyboard-handler.ts       # Add toggle handler, callback
├── main.ts                       # Apply initial state, wire callback
src-tauri/
└── src/
    └── commands/
        └── config.rs             # Add Rust struct fields + defaults
```

**File Descriptions**:
- `types.ts`: TypeScript interface definitions for settings
- `config.rs`: Rust settings struct with serde serialization
- `tab-bar.css`: Tab bar styling including hidden state
- `tab-bar-ui.ts`: Tab bar DOM management and visibility control
- `keyboard-handler.ts`: Keyboard shortcut handling for tabs
- `main.ts`: Application entry point and initialization

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's test runner for TypeScript tests
- Use Rust's built-in test framework for Rust tests
- Table-driven tests for settings deserialization

**Test Coverage Goals**:
- Settings deserialization: 90%+ coverage
- UI visibility methods: 80%+ coverage

**Key Test Areas**:
1. **Settings Deserialization** (`src-tauri/src/commands/config.rs`)
   - Default values when fields missing
   - Null handling with deserialize_null_* functions
   - Round-trip serialization

2. **TabBarUI Visibility** (`src/tab-bar/tab-bar-ui.ts`)
   - setVisible(true/false) class manipulation
   - isVisible() state reading

3. **Keybind Matching** (`src/keybind/matcher.ts`)
   - Already tested, verify Ctrl+Shift+B works

### Integration Testing

**Scenarios**:
1. Toggle keybind → callback → settings update
2. App startup → settings load → visibility applied

### E2E Testing (Docker)

- [ ] Tab bar visibility toggles with Ctrl+Shift+B
- [ ] Settings file updated after toggle
- [ ] Visibility state persisted across restart

### Manual Testing (E2E Not Possible)

- [ ] Animation feels smooth at 60fps
- [ ] No visual glitches during transition
- [ ] Keybind works in various input contexts

## Dependencies

### External Dependencies

None required - uses existing infrastructure.

### Internal Dependencies

| Package | Purpose |
|---------|---------|
| `src/settings/settings-service.ts` | Settings persistence |
| `src/keybind/matcher.ts` | Keybind matching utility |
| CSS variables (`--md-motion-*`) | MD3 Motion tokens |

**Implementation Order** (respecting dependencies):
1. Phase 1 (no dependencies)
2. Phase 2 (no dependencies, can be parallel with Phase 1)
3. Phase 3 (depends on Phase 1 and Phase 2)

## Risk Assessment

### Technical Risks

1. **CSS height transition performance**
   - **Risk**: Animating `height` property may cause layout thrashing
   - **Likelihood**: Low (single element, simple layout)
   - **Impact**: Medium (janky animation)
   - **Mitigation**: Use `overflow: hidden` to enable GPU compositing, test on target platforms

2. **Settings save timing**
   - **Risk**: Rapid toggling may cause excessive disk writes
   - **Likelihood**: Low (user action frequency limited)
   - **Impact**: Low (minor performance impact)
   - **Mitigation**: Could debounce if needed (not implemented initially)

### Implementation Risks

1. **Keybind conflict**
   - **Risk**: `Ctrl+Shift+B` may conflict with other shortcuts
   - **Mitigation**: Keybind is configurable, user can change if needed

## Performance Considerations

1. **Animation**
   - CSS transitions are GPU-accelerated when possible
   - Single property (`height`) animation is lightweight
   - `overflow: hidden` helps browser optimize

2. **Settings Save**
   - Existing `SettingsService` handles async save
   - No blocking UI operations

## Security Considerations

- No new security concerns (purely UI feature)
- No external data input
- No network operations

## Success Metrics

### Functional Completeness
- [ ] All three phases implemented
- [ ] All acceptance criteria met

### Quality Metrics
- [ ] TypeScript compiles without errors
- [ ] Rust compiles without errors
- [ ] All existing tests pass
- [ ] New tests pass

### Performance Metrics
- [ ] Animation runs at 60fps
- [ ] No perceptible delay on keybind press

### User Experience
- [ ] Toggle is intuitive and discoverable (via settings keybind customization)
- [ ] Animation feels polished

## References

- **Specification**: `doc/tasks/toggle-tab-bar/SPEC.md`
- **Settings Implementation**: `src-tauri/src/commands/config.rs`
- **Keybind Matcher**: `src/keybind/matcher.ts`
- **MD3 Motion**: CSS variables defined in theme

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Confirm approach and phase order
   - Address any open questions

2. **Begin Implementation**
   - Start with Phase 1 (settings infrastructure)
   - Phase 2 can be done in parallel
   - Phase 3 requires both Phase 1 and 2

3. **Testing**
   - Run existing tests to ensure no regression
   - Add new tests as specified
