# Feature: Custom Terminal Color Scheme

## Overview

Add user-customizable terminal color schemes to eMterm. Users can edit preset color schemes through an inline color palette editor in the settings panel. Editing a preset automatically creates a user scheme copy, which can be renamed, duplicated, and deleted. Changes preview in real-time on the terminal.

## Objectives

- Allow users to create custom terminal color schemes based on existing presets
- Provide inline color palette editing with native color pickers and HEX text input
- Real-time terminal preview of color changes
- Persistent storage of user color schemes in settings.json

## User Stories

### US1: Create Custom Color Scheme from Preset
As a terminal user, I want to edit a preset color scheme and have it automatically saved as my own custom scheme, so that I can personalize my terminal colors.

**Acceptance Criteria:**
- [ ] Selecting a preset displays its color palette inline below the select box
- [ ] Changing any color in a preset automatically creates a user scheme copy
- [ ] The copy is named `{preset_name}_copy_N` with auto-incrementing N
- [ ] The select box switches to the new user scheme
- [ ] Color changes reflect immediately on the terminal

### US2: Manage User Color Schemes
As a terminal user, I want to rename, duplicate, and delete my custom color schemes, so that I can organize my themes.

**Acceptance Criteria:**
- [ ] User schemes appear in select box as `{name} [User]`
- [ ] User schemes can be renamed via inline text field
- [ ] User schemes can be deleted (reverts to "emterm" preset)
- [ ] Any scheme (preset or user) can be duplicated
- [ ] Presets appear first in the select box, user schemes appear after

## Technical Requirements

### Functional Requirements

- **FR1:** Color palette editor renders inline in the settings panel below the Terminal Color Scheme select box
- **FR2:** Each color is editable via `input type="color"` (native color picker) and `#RRGGBB` HEX text input
- **FR3:** Editing a preset's color triggers auto-copy to a new user scheme named `{original}_copy_N`
- **FR4:** Editing a user scheme modifies it in place without creating copies
- **FR5:** Color changes apply to the terminal in real-time via CSS variables and renderer notifications
- **FR6:** User schemes are stored in `settings.json` under `custom_color_schemes` array
- **FR7:** Select box displays presets first (fixed order), then user schemes (creation order)
- **FR8:** User scheme entries in select box show `{name} [User]` suffix
- **FR9:** Delete button visible only when a user scheme is selected
- **FR10:** Duplicate button visible for both presets and user schemes
- **FR11:** Rename is available only for user schemes via inline editable text field

### Non-Functional Requirements

- **NFR1 - Performance:** Color change to terminal render < 16ms (single frame)
- **NFR2 - Backward Compatibility:** Existing settings.json without `custom_color_schemes` field loads without error (serde default)
- **NFR3 - Usability:** Color picker uses native browser `input type="color"` for platform consistency

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────┐
│   Settings Panel (settings-sections.ts)     │
│   ├── Terminal Color Scheme <select>        │
│   ├── Action Buttons (Duplicate/Delete)     │
│   ├── Rename Field (user schemes only)      │
│   └── Color Palette Editor (inline)         │
│       ├── Foreground / Background / Cursor / Selection
│       └── ANSI Colors 0-15 (grid)           │
├─────────────────────────────────────────────┤
│   Color Scheme Manager (new module)         │
│   ├── getUserSchemes()                      │
│   ├── createUserScheme()                    │
│   ├── updateUserScheme()                    │
│   ├── deleteUserScheme()                    │
│   ├── duplicateScheme()                     │
│   └── renameUserScheme()                    │
├─────────────────────────────────────────────┤
│   Settings Applier (settings-applier.ts)    │
│   └── applyTerminalColorScheme() - extended │
├─────────────────────────────────────────────┤
│   Rust Backend (config.rs)                  │
│   └── AppSettings.custom_color_schemes      │
└─────────────────────────────────────────────┘
```

### Data Flow

```
User edits color in palette
  → If preset selected: auto-copy to user scheme → update select box
  → Update CSS variables (real-time preview)
  → Notify terminal renderers
  → Save to settings.json via Tauri command
```

### Data Model

#### TypeScript Types

```typescript
// User color scheme stored in settings.json
interface UserColorScheme {
  name: string;
  foreground: string;      // "#RRGGBB"
  background: string;      // "#RRGGBB"
  cursor: string;          // "#RRGGBB"
  selection: string;       // "#RRGGBB"
  ansi_colors: string[];   // 16 "#RRGGBB" strings
}

// Extended AppSettings
interface AppSettings {
  // ... existing fields ...
  terminal_color_scheme: string;
  custom_color_schemes: UserColorScheme[];  // NEW
}
```

#### Rust Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserColorScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub selection: String,
    pub ansi_colors: Vec<String>,
}

pub struct AppSettings {
    // ... existing fields ...
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub custom_color_schemes: Vec<UserColorScheme>,
}
```

### Color Format

- Storage format: `#RRGGBB` HEX string (JSON-friendly)
- Internal format: `Rgb { r, g, b }` struct (for rendering)
- Conversion utilities: `hexToRgb()` and `rgbToHex()`

### Select Box Display Format

```
-- Presets --
eMterm
Solarized Dark
Solarized Light
Monokai
Dracula
Nord
-- User --
dracula_copy_1 [User]
MyCustomTheme [User]
```

### Auto-Copy Naming Algorithm

```typescript
function generateCopyName(baseName: string, existingNames: string[]): string {
  let n = 1;
  while (existingNames.includes(`${baseName}_copy_${n}`)) {
    n++;
  }
  return `${baseName}_copy_${n}`;
}
```

### Color Palette Editor Layout

```
┌─────────────────────────────────────────┐
│ Foreground  [■ #40ff40]                 │
│ Background  [■ #000000]                 │
│ Cursor      [■ #008000]                 │
│ Selection   [■ #3296fa]                 │
│                                         │
│ Standard Colors                         │
│ [■][■][■][■][■][■][■][■]              │
│  0  1  2  3  4  5  6  7               │
│                                         │
│ Bright Colors                           │
│ [■][■][■][■][■][■][■][■]              │
│  8  9 10 11 12 13 14 15               │
└─────────────────────────────────────────┘

[■] = input type="color" + HEX text input
```

### Settings Applier Changes

The `applyTerminalColorScheme()` function needs to be extended to:
1. Check if the scheme name matches a user scheme in `custom_color_schemes`
2. If found, apply user scheme colors as CSS variables
3. If not found, fall back to preset lookup (existing behavior)

### Dependencies

**Internal Dependencies:**
- `src/terminal/colors.ts`: ColorSchemePreset interface, Rgb type, rgbToCSS()
- `src/settings/settings-applier.ts`: applyTerminalColorScheme()
- `src/settings/settings-sections.ts`: renderAppearanceSection()
- `src/settings/settings-components.ts`: UI component renderers
- `src/settings/types.ts`: AppSettings interface
- `src-tauri/src/commands/config.rs`: Rust AppSettings struct

**New Files:**
- `src/settings/color-scheme-editor.ts`: Color palette editor component and scheme management logic

### File Structure

```
src/settings/
├── color-scheme-editor.ts       # NEW: Color palette editor + scheme manager
├── color-scheme-editor.test.ts  # NEW: Tests
├── settings-sections.ts         # MODIFIED: Integrate color editor
├── settings-applier.ts          # MODIFIED: Support user schemes
├── types.ts                     # MODIFIED: Add UserColorScheme type
src/terminal/
├── colors.ts                    # MODIFIED: Add hex conversion utilities
src-tauri/src/commands/
├── config.rs                    # MODIFIED: Add custom_color_schemes field
src/i18n/locales/
├── en.json                      # MODIFIED: Add color editor labels
├── ja.json                      # MODIFIED: Add color editor labels
```

## Test Scenarios

### Unit Tests
- [ ] `generateCopyName()` returns `{name}_copy_1` when no copies exist
- [ ] `generateCopyName()` increments N when copies exist
- [ ] `hexToRgb()` correctly parses `#RRGGBB` strings
- [ ] `rgbToHex()` correctly formats Rgb to `#RRGGBB`
- [ ] User scheme CRUD operations (create, read, update, delete)
- [ ] Select box options list presets first, user schemes second
- [ ] User scheme names display with `[User]` suffix
- [ ] Auto-copy triggers only for preset schemes, not user schemes
- [ ] Rename updates the scheme name and select box display

### Integration Tests (Rust)
- [ ] `AppSettings` deserializes with missing `custom_color_schemes` (defaults to empty vec)
- [ ] `AppSettings` deserializes with null `custom_color_schemes` (defaults to empty vec)
- [ ] `UserColorScheme` round-trip serialization
- [ ] Validation passes with valid user color schemes
- [ ] Settings with custom_color_schemes save and load correctly

### Edge Cases
- [ ] Deleting the currently active user scheme reverts to "emterm"
- [ ] Duplicate names are prevented (auto-increment handles this)
- [ ] Empty custom_color_schemes array loads correctly
- [ ] Renaming to empty string is prevented
- [ ] HEX input validation rejects invalid formats

## Security Considerations

- **Input Validation:** HEX color values validated against `#RRGGBB` format before saving
- **XSS Prevention:** Color values are applied via CSS variables (not innerHTML), preventing injection

## Error Handling

| Error | Condition | Handling |
|-------|-----------|---------|
| Invalid HEX | User enters non-hex value | Revert to previous value, show validation hint |
| Duplicate name | Rename to existing name | Prevent rename, show hint |
| Empty name | Rename to empty string | Prevent rename, show hint |
| Save failure | settings.json write error | Show error notification |

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Backward compatibility with existing settings.json maintained
- [ ] Color changes render in real-time on the terminal
- [ ] User schemes persist across application restarts
- [ ] Code review is completed
