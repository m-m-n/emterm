# Implementation Plan: Settings Panel - Phase 2

## Overview

Add remaining Appearance settings (color scheme, opacity, padding, scrollback, scrollbar), all Terminal behavior settings (shell, scroll speed, bell, URL detection, copy-on-select), and tab management keybinds. Phase 2 introduces the slider UI control and integrates with Tauri window opacity and PTY shell spawn APIs.

## Objectives

- Implement Appearance settings: terminal_color_scheme, opacity, padding, scrollback_lines, show_scrollbar
- Implement Terminal settings: shell_path, shell_args, scroll_speed, bell_action, url_detection, copy_on_select
- Implement Keybind settings: new_tab, close_tab, next_tab, prev_tab
- Implement slider UI control for opacity and scroll_speed
- Integrate opacity setting with Tauri window API
- Integrate shell settings with PTY spawn configuration

## Prerequisites

### Development Environment
- Phase 1 completed and merged
- All Phase 1 tests passing

### Dependencies
- Phase 1 type definitions (all fields already defined in Phase 1)
- Phase 1 UI controls (text input, select, toggle, keybind capture)
- Tauri window API for opacity control
- Existing PTY spawn logic for shell path integration

## Architecture Overview

### Design Approach

Phase 2 extends the settings panel with additional settings items in each category. Since all type definitions were added in Phase 1, this phase focuses on:
1. Rendering new settings rows in the UI
2. Adding apply functions for new visual settings
3. Integrating non-visual settings with their respective subsystems (window opacity, PTY shell)

### Key Integration Points

```
opacity setting --> SettingsApplier --> Tauri window API (set_decorations / set_effects)
shell_path/args --> SettingsService --> PTY spawn reads from saved settings
terminal_color_scheme --> SettingsApplier --> CSS variables for terminal colors
padding --> SettingsApplier --> CSS variable --terminal-padding
```

## Implementation Steps

### Step 1: Extend Settings Applier with Phase 2 Apply Functions

**Goal**: Apply functions exist for all Phase 2 visual settings.

**Files to Modify**:
- `src/settings/settings-applier.ts`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| applySettings | Call all Phase 2 apply functions in addition to Phase 1 | Full AppSettings loaded | All visual settings applied |
| applyTerminalColorScheme | Apply terminal color scheme preset as CSS variables | Valid scheme name | Terminal color CSS variables updated |
| applyOpacity | Set window opacity via Tauri API | Valid opacity 0.3-1.0 | Window transparency updated |
| applyPadding | Set --terminal-padding CSS variable | Valid padding 0-32 | CSS variable updated |
| applyScrollbar | Configure scrollbar visibility behavior | Valid mode string | Scrollbar CSS class updated |

**Processing Flow**:
```
applySettings(settings)
    +-- [Phase 1 functions]
    +-- applyTerminalColorScheme(settings.terminal_color_scheme)  [new]
    +-- applyOpacity(settings.opacity)                             [new]
    +-- applyPadding(settings.padding)                             [new]
    +-- applyScrollbar(settings.show_scrollbar)                    [new]
```

**applyTerminalColorScheme behavior**:
```
1. Receive scheme name ("default" or preset name)
   +-- "default" --> remove custom terminal color overrides
   +-- Other --> look up preset color definitions
       +-- Apply as CSS custom properties for terminal foreground/background/ANSI colors
```

**applyOpacity behavior**:
```
1. Receive opacity value (0.3-1.0)
2. Invoke Tauri window API to set window opacity
   +-- Success --> opacity applied
   +-- Failure --> log warning, keep current opacity
```

**Key Considerations**:
- Terminal color scheme presets need a definition structure (map of scheme name to color values). The specific presets are an open question per SPEC; implement the mechanism and a "default" scheme.
- Opacity requires Tauri window API; this is an async operation.
- scrollback_lines, scroll_speed, bell_action, url_detection, copy_on_select do not have visual apply functions -- they are read from settings when the relevant feature is invoked.

**Acceptance Criteria**:
- [ ] applyTerminalColorScheme updates terminal color CSS variables
- [ ] applyOpacity changes window transparency
- [ ] applyPadding sets --terminal-padding CSS variable
- [ ] applyScrollbar configures scrollbar visibility

---

### Step 2: Render Phase 2 Appearance Settings

**Goal**: Appearance category shows all Phase 2 settings in their respective subsections.

**Files to Modify**:
- `src/settings/settings-panel.ts`
- `src/styles/settings-panel.css`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderAppearanceSection | Add Theme & Color subsection items and Layout subsection | Settings loaded | terminal_color_scheme, opacity, padding, scrollback_lines, show_scrollbar displayed |
| Slider control | New UI element for opacity and scroll_speed | CSS styles defined | Range input with value display |

**Settings to Add to Appearance**:

| Setting | Subsection | Control | Behavior |
|---------|-----------|---------|----------|
| terminal_color_scheme | Theme & Color | Select dropdown | Apply on change |
| opacity | Theme & Color | Slider (0.3-1.0, step 0.05) | Real-time preview on input; save on change |
| padding | Layout | Number input + "px" | Real-time preview on input; save on blur/Enter |
| scrollback_lines | Layout | Number input | Save on blur/Enter (no real-time preview) |
| show_scrollbar | Layout | Select dropdown | Apply on change |

**Slider Control Structure**:
```
settings-row
  +-- label
  +-- settings-slider-group
  |     +-- input[type=range] with min/max/step
  |     +-- settings-slider-value (displays current numeric value)
  +-- hint (optional)
```

**Slider Behavior**:
```
1. User drags slider or clicks track
   +-- input event fires on every position change
   +-- Update value display text
   +-- Apply setting for real-time preview
2. User releases slider
   +-- change event fires
   +-- Save setting to backend
```

**Key Considerations**:
- Slider needs CSS styling to match MD3 design (custom appearance for track and thumb)
- opacity slider shows value as percentage or decimal
- scrollback_lines does not need real-time preview (only affects terminal buffer size)
- show_scrollbar applies to terminal content area, not settings panel

**Acceptance Criteria**:
- [ ] Theme & Color subsection shows terminal_color_scheme and opacity
- [ ] Layout subsection shows padding, scrollback_lines, show_scrollbar
- [ ] Slider control renders correctly with value display
- [ ] opacity slider provides real-time preview
- [ ] All controls save correctly

---

### Step 3: Render Phase 2 Terminal Settings

**Goal**: Terminal category shows Shell, Cursor (already from Phase 1), and Behavior subsections.

**Files to Modify**:
- `src/settings/settings-panel.ts`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderTerminalSection | Add Shell and Behavior subsections alongside existing Cursor | Settings loaded | All Phase 2 Terminal controls displayed |

**Settings to Add to Terminal**:

| Setting | Subsection | Control | Behavior |
|---------|-----------|---------|----------|
| shell_path | Shell | Text input | Save on blur/Enter; no preview (applies to new tabs) |
| shell_args | Shell | Text input (comma-separated) | Save on blur/Enter; no preview |
| scroll_speed | Behavior | Slider (1-10, step 1) | Save on change |
| bell_action | Behavior | Select dropdown | Save on change |
| url_detection | Behavior | Toggle switch | Save on toggle |
| copy_on_select | Behavior | Toggle switch | Save on toggle |

**shell_args Input Behavior**:
```
1. Display as comma-separated string in text input
   +-- Stored as string[] in AppSettings
   +-- On display: join array with ", "
   +-- On save: split by comma, trim whitespace
2. Empty string produces empty array []
```

**Key Considerations**:
- shell_path and shell_args changes apply to new tabs only -- hint text should communicate this
- shell_args requires conversion between string[] (stored) and comma-separated string (displayed)
- scroll_speed, bell_action, url_detection, copy_on_select are behavioral settings that do not require real-time visual preview

**Acceptance Criteria**:
- [ ] Shell subsection shows shell_path and shell_args
- [ ] Behavior subsection shows scroll_speed, bell_action, url_detection, copy_on_select
- [ ] shell_args displays as comma-separated and saves as array
- [ ] Shell settings hint mentions "applies to new tabs"

---

### Step 4: Render Phase 2 Keybind Settings

**Goal**: Tab Management subsection added to Keybinds category.

**Files to Modify**:
- `src/settings/settings-panel.ts`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderKeybindsSection | Add Tab Management subsection with 4 keybinds | Settings loaded | new_tab, close_tab, next_tab, prev_tab keybind inputs displayed |

**Settings to Add to Keybinds**:

| Setting | Subsection | Control |
|---------|-----------|---------|
| new_tab | Tab Management | Keybind capture |
| close_tab | Tab Management | Keybind capture |
| next_tab | Tab Management | Keybind capture |
| prev_tab | Tab Management | Keybind capture |

**Key Considerations**:
- Reuse the keybind capture control from Phase 1
- Tab Management subsection is inserted between Basic and Display subsections

**Acceptance Criteria**:
- [ ] Tab Management subsection visible in Keybinds category
- [ ] All 4 tab management keybinds render with current values
- [ ] Keybind capture works the same as Phase 1 keybinds

---

### Step 5: Add Slider CSS Styles

**Goal**: Slider control styled with MD3 design tokens.

**Files to Modify**:
- `src/styles/settings-panel.css`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| .settings-slider-group | Container layout for slider + value display | None | Horizontal layout with gap |
| .settings-slider | Style range input track and thumb (MD3 slider) | None | Custom track/thumb appearance matching MD3 |
| .settings-slider-value | Style numeric value display | None | MD3 Body Medium typography |

**Key Considerations**:
- Range input requires WebKit pseudo-elements for custom styling in WebView
- Slider track uses MD3 primary-container for filled portion, outline-variant for unfilled
- Slider thumb uses MD3 primary color
- Value display positioned to the right of the slider

**Acceptance Criteria**:
- [ ] Slider track and thumb styled with MD3 tokens
- [ ] Value display shows current numeric value
- [ ] Slider has focus-visible indicator
- [ ] Slider responds to keyboard input (arrow keys)

---

### Step 6: Integrate Shell Settings with PTY Spawn

**Goal**: New tabs use shell_path and shell_args from settings when spawning PTY.

**Files to Modify**:
- Backend PTY spawn code (reads settings at tab creation time)

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| PTY spawn logic | Read shell_path from saved settings | Settings file accessible | New tab uses configured shell |
| Fallback behavior | Use system default shell when shell_path is empty | Empty string in settings | System default shell used |

**Processing Flow**:
```
1. New tab requested
2. Load current settings from file
3. Check shell_path
   +-- Non-empty --> use as shell executable
   +-- Empty --> use system default shell
4. Check shell_args
   +-- Non-empty --> pass as arguments to shell
   +-- Empty --> no additional arguments
5. Spawn PTY with resolved shell and arguments
```

**Key Considerations**:
- Shell path validation (existence, executable) happens at PTY spawn time, not at save time
- Invalid shell path results in tab creation failure -- error should be reported to user
- Existing tabs are unaffected by shell_path changes

**Acceptance Criteria**:
- [ ] New tabs use configured shell_path when non-empty
- [ ] Empty shell_path falls back to system default
- [ ] shell_args passed correctly to spawned shell
- [ ] Existing tabs unaffected by shell setting changes

---

## Dependencies Between Steps

```
Step 1 (Applier) ──> Step 2 (Appearance UI) ──> Step 5 (Slider CSS)
                 \                                      ^
                  +-> Step 3 (Terminal UI) ─────────────+
                  \
                   +-> Step 4 (Keybinds UI)

Step 6 (PTY integration) -- independent, can proceed in parallel
```

- Step 1 should be completed first (apply functions needed by UI)
- Steps 2, 3, 4 can proceed in parallel after Step 1
- Step 5 (CSS) should be done before or alongside Steps 2/3
- Step 6 is independent and can proceed in parallel

## Complete File Changes

**Files to Modify**:
- `src/settings/settings-applier.ts` - New apply functions for Phase 2 settings
- `src/settings/settings-panel.ts` - New settings rows, slider control, subsections
- `src/styles/settings-panel.css` - Slider styles, any additional layout styles
- Backend PTY spawn code - Read shell settings

## Testing Strategy

### Unit Tests (TypeScript)

| Test | Description |
|------|-------------|
| applyTerminalColorScheme | Sets terminal color CSS variables for a preset |
| applyTerminalColorScheme default | Removes custom overrides for "default" |
| applyOpacity | Calls Tauri window API with correct value |
| applyPadding | Sets --terminal-padding CSS variable |
| applyScrollbar | Updates scrollbar visibility class |

### Unit Tests (Rust)

| Test | Description |
|------|-------------|
| shell_args serialization | Vec<String> serializes/deserializes correctly |
| Validation: opacity range | save_settings rejects opacity outside 0.3-1.0 |
| Validation: scroll_speed range | save_settings rejects scroll_speed outside 1-10 |
| Validation: bell_action enum | save_settings rejects invalid bell_action |
| Validation: show_scrollbar enum | save_settings rejects invalid show_scrollbar |

### Manual Testing

- [ ] Appearance: change terminal color scheme -- terminal colors update
- [ ] Appearance: drag opacity slider -- window transparency changes in real-time
- [ ] Appearance: change padding -- terminal content padding updates
- [ ] Appearance: change scrollback lines -- save without visual change
- [ ] Appearance: change scrollbar mode -- scrollbar visibility changes
- [ ] Terminal: set shell_path -- new tab uses configured shell
- [ ] Terminal: set shell_args -- arguments passed to shell
- [ ] Terminal: clear shell_path -- new tab uses system default
- [ ] Terminal: drag scroll speed slider -- value display updates
- [ ] Terminal: change bell action -- appropriate bell behavior on BEL character
- [ ] Terminal: toggle URL detection -- URL highlighting on/off
- [ ] Terminal: toggle copy on select -- text selection behavior changes
- [ ] Keybinds: capture new_tab keybind -- new shortcut works
- [ ] Keybinds: capture close_tab keybind -- new shortcut works
- [ ] All Phase 2 settings persist after restart

## Estimated Effort

Medium (3-5 days)

## Risks and Mitigation

- **Risk**: Terminal color scheme preset definitions not finalized (open question in SPEC)
  - **Mitigation**: Implement the mechanism (preset lookup, CSS variable application) with a "default" scheme. Additional presets can be added without structural changes.

- **Risk**: Tauri window opacity API may differ across platforms
  - **Mitigation**: Test on target platform (Linux). Handle API errors gracefully with fallback to full opacity.

- **Risk**: shell_args parsing from comma-separated string may be ambiguous (args containing commas)
  - **Mitigation**: Document the comma-separation behavior in the hint text. This matches the SPEC requirement of comma-separated input.

- **Risk**: Slider styling inconsistencies across WebView engines
  - **Mitigation**: Use WebKit-specific pseudo-elements; test on the Tauri WebView. Fall back to native appearance if custom styling fails.
