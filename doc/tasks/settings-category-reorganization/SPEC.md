# Feature: Settings Category Reorganization

## Overview

Reorganize the settings panel from 3 categories to 4 categories, separating UI-level settings from terminal-specific settings. Additionally, add a new UI font family setting.

## Objectives

- Reorganize settings categories for better logical grouping
- Separate UI settings from terminal appearance settings
- Add UI font family customization
- Maintain backward compatibility with existing settings files

## User Stories

### US1: Category Navigation
As a user, I want to find settings in logically organized categories, so that I can quickly locate and modify the settings I need.

**Acceptance Criteria:**
- [ ] Four categories are displayed in the navigation panel
- [ ] Category names clearly indicate their contents
- [ ] Keyboard navigation works correctly

### US2: UI Font Customization
As a user, I want to customize the font used in the settings panel and other UI elements, so that I can personalize the application appearance.

**Acceptance Criteria:**
- [ ] UI font family setting is available in "UI Settings" category
- [ ] Font change is applied immediately to the UI
- [ ] Setting persists after application restart

## Technical Requirements

### Functional Requirements

- **FR1:** Settings panel displays 4 categories: UI Settings, Keybinds, Terminal Appearance, Terminal Behavior
- **FR2:** Each category contains the appropriate settings as defined in the requirements
- **FR3:** UI font family setting applies to `.settings-panel` and related UI elements
- **FR4:** Default UI font is "Roboto" or system fallback

### Non-Functional Requirements

- **NFR1 - Performance:** Category switching should be instantaneous
- **NFR2 - Compatibility:** Existing settings files without `ui_font_family` should work with default value
- **NFR3 - Accessibility:** ARIA attributes maintained for all navigation elements

## Implementation Approach

### Architecture

**Component Changes:**

```
settings-panel.ts     → Category definitions (3 → 4)
settings-sections.ts  → Section renderers (reorganize + add new)
settings-applier.ts   → Add applyUiFont function
types.ts              → Add ui_font_family to AppSettings
locales/*.json        → Add translation keys
settings-panel.css    → Add CSS variable for UI font
```

### File Structure Changes

```
src/settings/
├── settings-panel.ts       # Update categories array
├── settings-sections.ts    # Add renderUiSection, rename others
├── settings-applier.ts     # Add applyUiFont()
├── types.ts                # Add ui_font_family
├── settings-components.ts  # No changes
└── settings-service.ts     # No changes (default handling)

src/i18n/locales/
├── en.json                 # Add new translation keys
└── ja.json                 # Add new translation keys

src/styles/
└── settings-panel.css      # Add --ui-font-family CSS variable
```

### Category Configuration

```typescript
// settings-panel.ts
private get categories(): Category[] {
  return [
    { id: "ui", label: t("settings.categories.ui"), enabled: true },
    { id: "keybinds", label: t("settings.categories.keybinds"), enabled: true },
    { id: "terminal-appearance", label: t("settings.categories.terminalAppearance"), enabled: true },
    { id: "terminal-behavior", label: t("settings.categories.terminalBehavior"), enabled: true },
  ];
}
```

### Settings Type Update

```typescript
// types.ts
export interface AppSettings {
  // Existing fields...

  // New field
  ui_font_family: string;
}

// Extend FontCategory for UI font picker
export type FontCategory = "primary" | "secondary" | "emoji" | "ui";

// Default value
export const DEFAULT_SETTINGS: AppSettings = {
  // ...existing defaults
  ui_font_family: "Roboto",
};
```

### CSS Variable

```css
/* settings-panel.css */
/* Current scope: Settings panel only (v1) */
/* Future scope: May extend to menus, dialogs */
.settings-panel {
  --ui-font-family: var(--settings-ui-font, "Roboto", system-ui, sans-serif);
  font-family: var(--ui-font-family);
}
```

**Application Scope (v1)**:
- Settings panel (`.settings-panel`)

**Future Expansion** (not in current scope):
- Application menus
- Dialog boxes
- Other UI elements

### Section Renderers

| Function | Category | Contents |
|----------|----------|----------|
| `renderUiSection` | ui | language, ui_theme, ui_theme_preset, ui_font_family |
| `renderKeybindsSection` | keybinds | (unchanged) |
| `renderTerminalAppearanceSection` | terminal-appearance | font_size, font_family_*, line_height, terminal_color_scheme, padding, scrollback_lines, show_scrollbar |
| `renderTerminalBehaviorSection` | terminal-behavior | cursor_style, cursor_blink, shell_path, shell_args, scroll_speed, bell_action, url_detection, copy_on_select |

### i18n Keys

**English (en.json)**:
```json
{
  "settings": {
    "categories": {
      "ui": "UI Settings",
      "keybinds": "Keybinds",
      "terminalAppearance": "Terminal Appearance",
      "terminalBehavior": "Terminal Behavior"
    },
    "ui": {
      "title": "UI Settings",
      "fontFamily": "UI Font",
      "fontFamilyDesc": "Font used in settings panel"
    },
    "appearance": {
      "fontPickerUiTitle": "UI Font"
    }
  }
}
```

**Japanese (ja.json)**:
```json
{
  "settings": {
    "categories": {
      "ui": "UI設定",
      "keybinds": "キーバインド",
      "terminalAppearance": "ターミナル表示",
      "terminalBehavior": "ターミナル動作"
    },
    "ui": {
      "title": "UI設定",
      "fontFamily": "UIフォント",
      "fontFamilyDesc": "設定画面で使用するフォント"
    },
    "appearance": {
      "fontPickerUiTitle": "UIフォント"
    }
  }
}
```

## Test Scenarios

### Unit Tests

- [ ] Category array returns 4 items with correct IDs
- [ ] UI font family setting saves and loads correctly
- [ ] Default value is used when ui_font_family is missing

### Integration Tests

- [ ] Category navigation switches content correctly
- [ ] Settings are saved to correct keys
- [ ] UI font change applies to settings panel

### E2E Tests

- [ ] Navigate through all 4 categories
- [ ] Change UI font and verify visual change
- [ ] Verify settings persist after restart

### Edge Cases

- [ ] Missing ui_font_family in existing config → use default
- [ ] Empty ui_font_family string → use default
- [ ] Invalid font name → fallback to system font

## Security Considerations

- **Input Validation:** Font family names should be sanitized (no script injection via CSS)

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| Invalid font | Font not available on system | Use CSS fallback chain |
| Missing setting | Old config file | Use default value |

## Success Criteria

- [ ] All 4 categories display correctly
- [ ] Settings are in appropriate categories
- [ ] UI font family setting works
- [ ] Backward compatible with existing configs
- [ ] i18n support (Japanese/English)
- [ ] Keyboard navigation works
- [ ] All tests pass

## Implementation Phases

### Phase 1: Category Reorganization
**Goals:** Restructure existing settings into 4 categories
**Deliverables:**
- Update settings-panel.ts categories
- Create new section renderers
- Move existing settings to new categories

### Phase 2: UI Font Setting
**Goals:** Add UI font family customization
**Deliverables:**
- Add type definition
- Add setting renderer
- Add applier function
- Add CSS variable support

### Phase 3: i18n & Polish
**Goals:** Finalize translations and UI
**Deliverables:**
- Add all translation keys
- Test all languages
- Verify accessibility

## References

- Requirements: `doc/tasks/settings-category-reorganization/要件定義書.md`
- Material Design 3: Typography guidelines
- Existing settings: `src/settings/settings-sections.ts`
