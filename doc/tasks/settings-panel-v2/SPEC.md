# Feature: Settings Panel

## Overview

Extend the eMterm settings panel from its current font_size-only implementation to support all configuration items across three categories: Appearance, Terminal, and Keybinds. Settings are persisted as JSON and applied in real-time.

## Objectives

- Add all settings items across three categories (Appearance, Terminal, Keybinds)
- Extend Rust and TypeScript type definitions to match new settings structure
- Enable all three category tabs in the settings panel navigation
- Implement real-time preview for applicable settings
- Maintain backward compatibility with existing `font_size`-only settings files

## User Stories

### US1: Customize Appearance
As a user, I want to change font family, line height, and UI theme, so that the terminal matches my visual preferences.

**Acceptance Criteria:**
- [ ] Font family can be set via text input
- [ ] Line height can be adjusted (0.8-3.0)
- [ ] UI theme can be switched between Light, Dark, and System
- [ ] Changes are previewed in real-time

### US2: Configure Cursor
As a user, I want to change cursor style and blink behavior, so that the cursor matches my editing preferences.

**Acceptance Criteria:**
- [ ] Cursor style selectable: Block, Underline, Bar
- [ ] Cursor blink togglable ON/OFF
- [ ] Changes apply immediately to all terminal tabs

### US3: Customize Keybinds
As a user, I want to remap keyboard shortcuts, so that I can use key combinations I'm familiar with.

**Acceptance Criteria:**
- [ ] Keybind capture input records key combinations
- [ ] Default keybinds are provided
- [ ] Changed keybinds persist across restarts

### US4: Configure Shell
As a user, I want to set a custom shell path and arguments, so that new tabs use my preferred shell.

**Acceptance Criteria:**
- [ ] Shell path configurable via text input
- [ ] Shell arguments configurable
- [ ] Empty shell path falls back to system default
- [ ] Changes apply to new tabs only

### US5: Backward-Compatible Settings
As a user, I want my existing settings file (with only font_size) to work after the update.

**Acceptance Criteria:**
- [ ] Old settings files load without errors
- [ ] Missing fields are filled with defaults
- [ ] Unknown fields are ignored

## Technical Requirements

### Functional Requirements

- **FR1:** Extend `AppSettings` struct/interface with all settings fields
- **FR2:** All fields use `#[serde(default)]` for backward compatibility
- **FR3:** Enable Terminal and Keybinds category tabs in navigation
- **FR4:** Render appropriate UI controls for each setting type
- **FR5:** Real-time preview for visual settings (font, theme, cursor)
- **FR6:** Auto-save on blur/Enter for text/number inputs, on change for toggles/dropdowns
- **FR7:** Validate all inputs (clamp numeric values, reject invalid enums)
- **FR8:** Shell settings apply to new tabs only (not existing tabs)

### Non-Functional Requirements

- **NFR1 - Performance:** Settings preview within 16ms (60fps)
- **NFR2 - Compatibility:** Backward compatible with existing font_size-only settings files
- **NFR3 - Extensibility:** Settings structure supports future additions without breaking changes
- **NFR4 - Accessibility:** All controls keyboard-navigable with proper ARIA attributes

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Frontend (TypeScript)                     │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌───────────────┐  ┌──────────────────┐ │
│  │SettingsPanel │  │SettingsService│  │SettingsApplier   │ │
│  │  (UI)        │──│ (Load/Save)   │──│ (CSS/Renderer)   │ │
│  └──────────────┘  └───────────────┘  └──────────────────┘ │
│         │                 │                                  │
│         │                 │ invoke                           │
├─────────┴─────────────────┴──────────────────────────────────┤
│                    Backend (Rust/Tauri)                       │
├──────────────────────────────────────────────────────────────┤
│  ┌────────────────────────────────────────────────────────┐  │
│  │              config.rs                                  │  │
│  │  load_settings / save_settings commands                 │  │
│  │  AppSettings struct with all fields                     │  │
│  └────────────────────────────────────────────────────────┘  │
│                          │                                    │
│                          ▼                                    │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         ~/.config/emterm/settings.json                  │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

### Data Flow

```
User Input → SettingsPanel → SettingsService.save() → Tauri Command → File
                  │
                  └→ SettingsApplier.apply*() → CSS Variables / Renderer Notification

Startup → SettingsService.load() → Tauri Command → File
                  │
                  └→ SettingsApplier.applySettings() → CSS Variables / Renderer Notification
```

### Type Definitions

#### Rust (src-tauri/src/commands/config.rs)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    // Font
    #[serde(default = "default_font_size", deserialize_with = "deserialize_null_default")]
    pub font_size: u32,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub font_family: String,
    #[serde(default = "default_line_height", deserialize_with = "deserialize_null_default")]
    pub line_height: f32,

    // Theme / Color
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ui_theme: UiTheme,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub terminal_color_scheme: String,
    #[serde(default = "default_opacity", deserialize_with = "deserialize_null_default")]
    pub opacity: f32,

    // Layout
    #[serde(default = "default_padding", deserialize_with = "deserialize_null_default")]
    pub padding: u32,
    #[serde(default = "default_scrollback_lines", deserialize_with = "deserialize_null_default")]
    pub scrollback_lines: u32,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub show_scrollbar: ScrollbarMode,

    // Rich Content
    #[serde(default = "default_true", deserialize_with = "deserialize_null_default")]
    pub inline_images_enabled: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_default")]
    pub markdown_rendering: bool,

    // Terminal
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_path: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_args: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub cursor_style: CursorStyle,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_default")]
    pub cursor_blink: bool,
    #[serde(default = "default_scroll_speed", deserialize_with = "deserialize_null_default")]
    pub scroll_speed: u32,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub bell_action: BellAction,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_default")]
    pub url_detection: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub copy_on_select: bool,

    // Keybinds
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub keybinds: KeybindSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BellAction {
    Sound,
    #[default]
    Visual,
    None,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScrollbarMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindSettings {
    #[serde(default = "default_keybind_copy", deserialize_with = "deserialize_null_default")]
    pub copy: String,
    #[serde(default = "default_keybind_paste", deserialize_with = "deserialize_null_default")]
    pub paste: String,
    #[serde(default = "default_keybind_select_all", deserialize_with = "deserialize_null_default")]
    pub select_all: String,
    #[serde(default = "default_keybind_search", deserialize_with = "deserialize_null_default")]
    pub search: String,
    #[serde(default = "default_keybind_new_tab", deserialize_with = "deserialize_null_default")]
    pub new_tab: String,
    #[serde(default = "default_keybind_close_tab", deserialize_with = "deserialize_null_default")]
    pub close_tab: String,
    #[serde(default = "default_keybind_next_tab", deserialize_with = "deserialize_null_default")]
    pub next_tab: String,
    #[serde(default = "default_keybind_prev_tab", deserialize_with = "deserialize_null_default")]
    pub prev_tab: String,
    #[serde(default = "default_keybind_zoom_in", deserialize_with = "deserialize_null_default")]
    pub zoom_in: String,
    #[serde(default = "default_keybind_zoom_out", deserialize_with = "deserialize_null_default")]
    pub zoom_out: String,
    #[serde(default = "default_keybind_zoom_reset", deserialize_with = "deserialize_null_default")]
    pub zoom_reset: String,
    #[serde(default = "default_keybind_toggle_fullscreen", deserialize_with = "deserialize_null_default")]
    pub toggle_fullscreen: String,
    #[serde(default = "default_keybind_open_settings", deserialize_with = "deserialize_null_default")]
    pub open_settings: String,
}
```

#### Null-safe Deserialization Helper (Rust)

```rust
/// Deserializes a value, treating JSON null as the type's default value.
/// Used with #[serde(deserialize_with = "deserialize_null_default")]
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}
```

#### Default Value Functions (Rust)

```rust
fn default_font_size() -> u32 { 13 }
fn default_line_height() -> f32 { 1.2 }
fn default_opacity() -> f32 { 1.0 }
fn default_padding() -> u32 { 4 }
fn default_scrollback_lines() -> u32 { 10000 }
fn default_scroll_speed() -> u32 { 3 }
fn default_true() -> bool { true }

fn default_keybind_copy() -> String { "Ctrl+Shift+C".to_string() }
fn default_keybind_paste() -> String { "Ctrl+Shift+V".to_string() }
fn default_keybind_select_all() -> String { "Ctrl+Shift+A".to_string() }
fn default_keybind_search() -> String { "Ctrl+Shift+F".to_string() }
fn default_keybind_new_tab() -> String { "Ctrl+Shift+T".to_string() }
fn default_keybind_close_tab() -> String { "Ctrl+Shift+W".to_string() }
fn default_keybind_next_tab() -> String { "Ctrl+Tab".to_string() }
fn default_keybind_prev_tab() -> String { "Ctrl+Shift+Tab".to_string() }
fn default_keybind_zoom_in() -> String { "Ctrl+Plus".to_string() }
fn default_keybind_zoom_out() -> String { "Ctrl+Minus".to_string() }
fn default_keybind_zoom_reset() -> String { "Ctrl+0".to_string() }
fn default_keybind_toggle_fullscreen() -> String { "F11".to_string() }
fn default_keybind_open_settings() -> String { "Ctrl+Comma".to_string() }
```

Note: `UiTheme`, `CursorStyle`, `BellAction`, `ScrollbarMode` use `#[default]` attribute on their variants, so they implement `Default` trait. No separate default functions needed for these enum types.

#### Manual Default Implementations (Rust)

`AppSettings` and `KeybindSettings` require manual `impl Default` because their default values differ from Rust's zero-value defaults (e.g., `font_size` defaults to 13, not 0). This also ensures `deserialize_null_default` returns the correct custom defaults when encountering JSON `null`.

```rust
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            font_family: String::new(),
            line_height: default_line_height(),
            ui_theme: UiTheme::default(),
            terminal_color_scheme: String::new(),
            opacity: default_opacity(),
            padding: default_padding(),
            scrollback_lines: default_scrollback_lines(),
            show_scrollbar: ScrollbarMode::default(),
            inline_images_enabled: default_true(),
            markdown_rendering: default_true(),
            shell_path: String::new(),
            shell_args: Vec::new(),
            cursor_style: CursorStyle::default(),
            cursor_blink: default_true(),
            scroll_speed: default_scroll_speed(),
            bell_action: BellAction::default(),
            url_detection: default_true(),
            copy_on_select: false,
            keybinds: KeybindSettings::default(),
        }
    }
}

impl Default for KeybindSettings {
    fn default() -> Self {
        Self {
            copy: default_keybind_copy(),
            paste: default_keybind_paste(),
            select_all: default_keybind_select_all(),
            search: default_keybind_search(),
            new_tab: default_keybind_new_tab(),
            close_tab: default_keybind_close_tab(),
            next_tab: default_keybind_next_tab(),
            prev_tab: default_keybind_prev_tab(),
            zoom_in: default_keybind_zoom_in(),
            zoom_out: default_keybind_zoom_out(),
            zoom_reset: default_keybind_zoom_reset(),
            toggle_fullscreen: default_keybind_toggle_fullscreen(),
            open_settings: default_keybind_open_settings(),
        }
    }
}
```

This ensures:
- `AppSettings::default()` and `KeybindSettings::default()` return correct custom defaults (used in tests RU-01, RU-14)
- `deserialize_null_default` returns correct values for null fields (e.g., `{"font_size": null}` → 13, not 0)

#### Validation Constants (Rust)

```rust
// Font
pub const MIN_FONT_SIZE: u32 = 8;
pub const MAX_FONT_SIZE: u32 = 32;
pub const DEFAULT_FONT_SIZE: u32 = 13;
pub const MIN_LINE_HEIGHT: f32 = 0.8;
pub const MAX_LINE_HEIGHT: f32 = 3.0;

// Layout
pub const MIN_PADDING: u32 = 0;
pub const MAX_PADDING: u32 = 32;
pub const MIN_SCROLLBACK_LINES: u32 = 0;
pub const MAX_SCROLLBACK_LINES: u32 = 100000;

// Opacity
pub const MIN_OPACITY: f32 = 0.3;
pub const MAX_OPACITY: f32 = 1.0;

// Scroll
pub const MIN_SCROLL_SPEED: u32 = 1;
pub const MAX_SCROLL_SPEED: u32 = 10;

// Note: Enum validation is handled by serde deserialization of UiTheme,
// CursorStyle, BellAction, ScrollbarMode types. Invalid values cause
// deserialization errors, which are caught and handled as validation failures.
```

#### TypeScript (src/settings/types.ts)

```typescript
export interface AppSettings {
  // Font
  font_size: number;
  font_family: string;
  line_height: number;

  // Theme / Color
  ui_theme: UiTheme;
  terminal_color_scheme: string;
  opacity: number;

  // Layout
  padding: number;
  scrollback_lines: number;
  show_scrollbar: ScrollbarMode;

  // Rich Content
  inline_images_enabled: boolean;
  markdown_rendering: boolean;

  // Terminal
  shell_path: string;
  shell_args: string[];
  cursor_style: CursorStyle;
  cursor_blink: boolean;
  scroll_speed: number;
  bell_action: BellAction;
  url_detection: boolean;
  copy_on_select: boolean;

  // Keybinds
  keybinds: KeybindSettings;
}

export type UiTheme = "light" | "dark" | "system";
export type CursorStyle = "block" | "underline" | "bar";
export type BellAction = "sound" | "visual" | "none";
export type ScrollbarMode = "auto" | "always" | "never";

export interface KeybindSettings {
  copy: string;
  paste: string;
  select_all: string;
  search: string;
  new_tab: string;
  close_tab: string;
  next_tab: string;
  prev_tab: string;
  zoom_in: string;
  zoom_out: string;
  zoom_reset: string;
  toggle_fullscreen: string;
  open_settings: string;
}

// Validation constants
export const MIN_FONT_SIZE = 8;
export const MAX_FONT_SIZE = 32;
export const MIN_LINE_HEIGHT = 0.8;
export const MAX_LINE_HEIGHT = 3.0;
export const LINE_HEIGHT_STEP = 0.1;
export const MIN_OPACITY = 0.3;
export const MAX_OPACITY = 1.0;
export const OPACITY_STEP = 0.05;
export const MIN_PADDING = 0;
export const MAX_PADDING = 32;
export const MIN_SCROLLBACK_LINES = 0;
export const MAX_SCROLLBACK_LINES = 100000;
export const MIN_SCROLL_SPEED = 1;
export const MAX_SCROLL_SPEED = 10;
```

### Settings File Format

**Path:** `~/.config/emterm/settings.json`

```json
{
  "font_size": 13,
  "font_family": "",
  "line_height": 1.2,
  "ui_theme": "system",
  "terminal_color_scheme": "default",
  "opacity": 1.0,
  "padding": 4,
  "scrollback_lines": 10000,
  "show_scrollbar": "auto",
  "inline_images_enabled": true,
  "markdown_rendering": true,
  "shell_path": "",
  "shell_args": [],
  "cursor_style": "block",
  "cursor_blink": true,
  "scroll_speed": 3,
  "bell_action": "visual",
  "url_detection": true,
  "copy_on_select": false,
  "keybinds": {
    "copy": "Ctrl+Shift+C",
    "paste": "Ctrl+Shift+V",
    "select_all": "Ctrl+Shift+A",
    "search": "Ctrl+Shift+F",
    "new_tab": "Ctrl+Shift+T",
    "close_tab": "Ctrl+Shift+W",
    "next_tab": "Ctrl+Tab",
    "prev_tab": "Ctrl+Shift+Tab",
    "zoom_in": "Ctrl+Plus",
    "zoom_out": "Ctrl+Minus",
    "zoom_reset": "Ctrl+0",
    "toggle_fullscreen": "F11",
    "open_settings": "Ctrl+Comma"
  }
}
```

### Validation Rules

**Backend validation in `save_settings`:**

| Field | Rule | Error handling |
|-------|------|----------------|
| `font_size` | 8..=32 | Return error |
| `line_height` | 0.8..=3.0 | Return error |
| `ui_theme` | `UiTheme` enum (light, dark, system) | Serde deserialization error |
| `terminal_color_scheme` | Must be a valid preset name | Return error |
| `opacity` | 0.3..=1.0 | Return error |
| `padding` | 0..=32 | Return error |
| `scrollback_lines` | 0..=100000 | Return error |
| `cursor_style` | `CursorStyle` enum (block, underline, bar) | Serde deserialization error |
| `scroll_speed` | 1..=10 | Return error |
| `bell_action` | `BellAction` enum (sound, visual, none) | Serde deserialization error |
| `show_scrollbar` | `ScrollbarMode` enum (auto, always, never) | Serde deserialization error |

**Frontend validation (clamp before save):**

Numeric inputs are clamped to valid range in the UI before calling `save_settings`. Enum fields use `<select>` elements that only allow valid values.

### Settings Application

#### CSS Variables

```css
:root {
  --terminal-font-size: 13pt;
  --terminal-font-family: monospace;
  --terminal-line-height: 1.2;
  --terminal-padding: 4px;
}
```

#### SettingsApplier Extensions

```typescript
export function applySettings(settings: AppSettings): void {
  applyFontSize(settings.font_size);
  applyFontFamily(settings.font_family);
  applyLineHeight(settings.line_height);
  applyUiTheme(settings.ui_theme);
  applyTerminalColorScheme(settings.terminal_color_scheme);
  applyOpacity(settings.opacity);
  applyPadding(settings.padding);
  applyScrollbar(settings.show_scrollbar);
  applyCursorStyle(settings.cursor_style);
  applyCursorBlink(settings.cursor_blink);
}
```

Each `apply*` function updates CSS variables and/or notifies renderers via `notifyRenderers()`.

#### UI Theme Application

`applyUiTheme()` sets a `data-theme` attribute on `document.documentElement`:
- `"light"` → `data-theme="light"`
- `"dark"` → `data-theme="dark"`
- `"system"` → `data-theme` is set based on `window.matchMedia("(prefers-color-scheme: dark)")`, and a media query listener is registered for changes

### UI Controls

#### Number Input (existing pattern)

Used for: `font_size`, `line_height`, `padding`, `scrollback_lines`

```html
<div class="settings-row">
  <label class="settings-label" for="settings-{key}">{Label}</label>
  <div class="settings-input-group">
    <input type="number" id="settings-{key}" class="settings-number-input"
           min="{min}" max="{max}" step="{step}" value="{value}">
    <span class="settings-unit">{unit}</span>
  </div>
  <span class="settings-hint">{hint}</span>
</div>
```

#### Text Input

Used for: `font_family`, `shell_path`, `shell_args`

```html
<div class="settings-row">
  <label class="settings-label" for="settings-{key}">{Label}</label>
  <input type="text" id="settings-{key}" class="settings-text-input"
         value="{value}" placeholder="{placeholder}">
  <span class="settings-hint">{hint}</span>
</div>
```

**`shell_args` conversion**: The `shell_args` field is stored as `Vec<String>` / `string[]` but displayed as a comma-separated text input.
- **Display**: `["--login", "-i"]` → `"--login, -i"`
- **Save**: `"--login, -i"` → split by comma, trim whitespace → `["--login", "-i"]`
- **Empty string**: `""` → `[]`

#### Select Dropdown

Used for: `ui_theme`, `terminal_color_scheme`, `show_scrollbar`, `cursor_style`, `bell_action`

```html
<div class="settings-row">
  <label class="settings-label" for="settings-{key}">{Label}</label>
  <select id="settings-{key}" class="settings-select">
    <option value="{value1}">{label1}</option>
    <option value="{value2}">{label2}</option>
  </select>
  <span class="settings-hint">{hint}</span>
</div>
```

#### Toggle Switch

Used for: `cursor_blink`, `url_detection`, `copy_on_select`, `inline_images_enabled`, `markdown_rendering`

```html
<div class="settings-row settings-row-toggle">
  <label class="settings-label" for="settings-{key}">{Label}</label>
  <button id="settings-{key}" class="settings-toggle"
          role="switch" aria-checked="{value}">
    <span class="settings-toggle-track">
      <span class="settings-toggle-thumb"></span>
    </span>
  </button>
</div>
```

#### Slider

Used for: `opacity`, `scroll_speed`

```html
<div class="settings-row">
  <label class="settings-label" for="settings-{key}">{Label}</label>
  <div class="settings-slider-group">
    <input type="range" id="settings-{key}" class="settings-slider"
           min="{min}" max="{max}" step="{step}" value="{value}">
    <span class="settings-slider-value">{value}</span>
  </div>
  <span class="settings-hint">{hint}</span>
</div>
```

#### Keybind Capture Input

Used for: all `keybinds.*` fields

```html
<div class="settings-row settings-row-keybind">
  <label class="settings-label">{Label}</label>
  <button class="settings-keybind-input" data-key="{keybind_key}">
    {current_value}
  </button>
</div>
```

On click, enters capture mode. Next keydown event records the key combination and saves.

### Content Sections

#### Appearance Category

Sections: Font, Theme & Color, Layout, Rich Content

Each section has a subheader (`h3.settings-subsection-header`).

#### Terminal Category

Sections: Shell, Cursor, Behavior

#### Keybinds Category

Sections: Basic, Tab Management, Display, Settings

### File Structure

```
src-tauri/
└── src/
    └── commands/
        └── config.rs              # Extended AppSettings + KeybindSettings

src/
├── settings/
│   ├── types.ts                   # Extended AppSettings + enum types + constants
│   ├── settings-service.ts        # Unchanged interface
│   ├── settings-applier.ts        # Extended apply* functions
│   └── settings-panel.ts          # Extended with all categories and controls
├── styles/
│   └── settings-panel.css         # Extended with new control styles
```

### Dependencies

**Internal Dependencies:**
- `src/settings/settings-applier.ts`: Apply settings to CSS/renderers
- `src/settings/settings-service.ts`: Load/save via Tauri commands
- `src-tauri/src/commands/config.rs`: Backend settings persistence

**External Dependencies:**
- `@tauri-apps/api`: Tauri invoke for commands
- `serde` / `serde_json`: Rust serialization

## Test Scenarios

### Unit Tests (Rust)

- [ ] `AppSettings::default()` returns correct defaults for all fields
- [ ] Deserialization of `{}` produces all defaults
- [ ] Deserialization of `{"font_size": 13}` (old format) produces defaults for new fields
- [ ] Deserialization ignores unknown fields
- [ ] `save_settings` rejects `font_size` outside 8-32
- [ ] `save_settings` rejects `line_height` outside 0.8-3.0
- [ ] `save_settings` rejects invalid `ui_theme` value
- [ ] `save_settings` rejects invalid `cursor_style` value
- [ ] `save_settings` rejects `opacity` outside 0.3-1.0
- [ ] `save_settings` rejects `scroll_speed` outside 1-10
- [ ] `save_settings` rejects invalid `bell_action` value
- [ ] `save_settings` rejects invalid `show_scrollbar` value
- [ ] `save_settings` accepts valid complete settings
- [ ] `KeybindSettings::default()` returns correct defaults
- [ ] Settings round-trip: serialize then deserialize preserves all fields

### Unit Tests (TypeScript)

- [ ] `applyFontFamily` sets `--terminal-font-family` CSS variable
- [ ] `applyLineHeight` sets `--terminal-line-height` CSS variable
- [ ] `applyUiTheme("light")` sets `data-theme="light"`
- [ ] `applyUiTheme("dark")` sets `data-theme="dark"`
- [ ] `applyUiTheme("system")` respects `prefers-color-scheme`
- [ ] `applyPadding` sets `--terminal-padding` CSS variable
- [ ] `applyCursorStyle` notifies renderers
- [ ] `applyCursorBlink` notifies renderers

### Integration Tests

- [ ] Category navigation switches between Appearance, Terminal, Keybinds
- [ ] All settings render with correct current values
- [ ] Number input changes save and apply
- [ ] Select dropdown changes save and apply
- [ ] Toggle switch changes save and apply
- [ ] Slider changes save and apply
- [ ] Keybind capture records and saves key combinations
- [ ] Settings persist after panel close and reopen

### Edge Cases

- [ ] Old settings file with only `font_size` loads without error
- [ ] Corrupted JSON file falls back to all defaults
- [ ] Empty string `font_family` uses system monospace
- [ ] Empty string `shell_path` uses system default shell
- [ ] `opacity` at minimum (0.3) and maximum (1.0)
- [ ] `scrollback_lines` at 0 and 100000
- [ ] Settings file with null values for optional fields
- [ ] `inline_images_enabled=false`: image rendering is skipped
- [ ] `markdown_rendering=false`: markdown rendering is skipped

## Error Handling

| Scenario | Handling |
|----------|----------|
| Settings file not found | Use all defaults |
| Settings file corrupted JSON | Use all defaults, log warning |
| Missing fields in JSON | Use default for missing fields |
| Unknown fields in JSON | Ignore unknown fields |
| Validation failure on save | Return error, do not write file |
| File write failure | Return error, log error |
| Invalid font_family | Browser fallback to monospace |

## Implementation Phases

### Phase 1

**Scope:**
- Extend `AppSettings` struct/interface with Phase 1 fields
- Enable Terminal and Keybinds category tabs
- Appearance: `font_family`, `line_height`, `ui_theme`
- Terminal: `cursor_style`, `cursor_blink`
- Keybinds: `copy`, `paste`, `select_all`, `search`, `zoom_in`, `zoom_out`, `zoom_reset`, `toggle_fullscreen`, `open_settings`
- New UI controls: text input, select dropdown, toggle switch, keybind capture
- New CSS styles for added controls

**Deliverables:**
- Extended Rust `AppSettings` + `KeybindSettings` structs
- Extended TypeScript `AppSettings` interface + types
- Three functional category tabs
- Apply functions for new settings
- Unit tests for new fields

### Phase 2

**Scope:**
- Appearance: `terminal_color_scheme`, `opacity`, `padding`, `scrollback_lines`, `show_scrollbar`
- Terminal: `shell_path`, `shell_args`, `scroll_speed`, `bell_action`, `url_detection`, `copy_on_select`
- Keybinds: `new_tab`, `close_tab`, `next_tab`, `prev_tab`
- New UI control: slider

**Deliverables:**
- Color scheme preset system
- Slider UI component
- Opacity application via Tauri window API
- Shell path integration with PTY spawn
- Additional unit tests

### Phase 3

**Scope:**
- Appearance: `inline_images_enabled`, `markdown_rendering`

**Deliverables:**
- Toggle controls for rich content settings
- Integration with image/markdown rendering pipelines
- Feature flag checks in rendering code

## Success Criteria

- [ ] All Phase 1 settings items implemented and functional
- [ ] All three category tabs navigable
- [ ] Real-time preview for visual settings
- [ ] Settings persist across app restarts
- [ ] Old settings files (font_size only) load correctly
- [ ] All unit tests pass
- [ ] Build succeeds (`cargo test`, `bun test`, `bun run typecheck`)

## Verification

```bash
# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# TypeScript tests
bun test

# Type check
bun run typecheck

# Manual verification
bun tauri dev
# 1. Open settings → All three categories visible
# 2. Change font_family → Terminal font updates
# 3. Switch UI theme → Theme changes
# 4. Change cursor style → Cursor updates
# 5. Capture keybind → New keybind works
# 6. Restart app → All settings preserved
# 7. Delete settings.json → App starts with defaults

# Backward compatibility
echo '{"font_size": 16}' > ~/.config/emterm/settings.json
bun tauri dev
# Verify: font_size=16, all other fields use defaults
```

## Open Questions

- [ ] Terminal Color Scheme: specific preset names and color values (Phase 2)
- [ ] Keybind conflict resolution: what happens when two actions share the same keybind
