# Feature: Markdown Viewer Settings

## Overview

Add a "Markdown Viewer" settings category to the settings panel, allowing users to configure font family and font size for the Markdown fullscreen overlay. Currently, these values are hardcoded in CSS. This feature introduces three user-configurable settings: body font family, code block font family, and body font size.

## Objectives

- Allow users to customize Markdown viewer typography through the settings panel
- Add a new "Markdown Viewer" navigation category as the 5th settings category
- Maintain backward compatibility with existing settings.json files

## User Stories

### US1: Configure Markdown Body Font

As a user, I want to change the font used for Markdown body text, so that I can read Markdown content in my preferred font.

**Acceptance Criteria:**
- [ ] Font picker dialog is available for body font selection
- [ ] Selected font is applied to Markdown fullscreen overlay text
- [ ] Empty value falls back to CSS default font chain
- [ ] Setting persists across app restarts

### US2: Configure Markdown Code Font

As a user, I want to change the font used for code blocks in Markdown, so that code is displayed in my preferred monospace font.

**Acceptance Criteria:**
- [ ] Font picker dialog is available for code font selection
- [ ] Selected font is applied to `<code>` and `<pre>` elements in Markdown overlay
- [ ] Empty value falls back to CSS default font chain
- [ ] Setting persists across app restarts

### US3: Configure Markdown Font Size

As a user, I want to adjust the base font size for Markdown content, so that I can control readability.

**Acceptance Criteria:**
- [ ] Number input allows setting font size in pt (8-32 range)
- [ ] Heading sizes scale proportionally (h1: 2em, h2: 1.5em, etc.)
- [ ] Code block size scales proportionally (85% of body size)
- [ ] Setting persists across app restarts

## Technical Requirements

### Functional Requirements

- **FR1:** Add `markdown-viewer` as the 5th navigation category in settings panel
- **FR2:** Render font picker for `markdown_body_font_family` (default: empty string)
- **FR3:** Render font picker for `markdown_code_font_family` (default: empty string)
- **FR4:** Render number input for `markdown_font_size` (default: 14, range: 8-32, step: 1, unit: pt)
- **FR5:** Apply settings via CSS variables on the Markdown fullscreen overlay
- **FR6:** Apply settings immediately on change (no restart required)
- **FR7:** Apply saved settings on app startup

### Non-Functional Requirements

- **NFR1 - Performance:** CSS variable changes must be instant (no perceptible delay)
- **NFR2 - Compatibility:** Existing settings.json without new fields must load with defaults
- **NFR3 - Consistency:** Follow existing settings management patterns (load/save/validate/apply)

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────────┐
│  Settings Panel (settings-panel.ts)                 │
│  └── New category: "markdown-viewer"                │
│      └── renderMarkdownViewerSection()              │
│          ├── Font Picker (body font)                │
│          ├── Font Picker (code font)                │
│          └── Number Input (font size)               │
├─────────────────────────────────────────────────────┤
│  Settings Service (settings-service.ts)             │
│  └── load/save with new fields                      │
├─────────────────────────────────────────────────────┤
│  Settings Applier (settings-applier.ts)             │
│  └── applyMarkdownSettings()                        │
│      └── Set CSS variables on document root         │
├─────────────────────────────────────────────────────┤
│  Rust Backend (config.rs)                           │
│  └── AppSettings + 3 new fields                     │
│      └── validate_settings() updated                │
├─────────────────────────────────────────────────────┤
│  Markdown Fullscreen (fullscreen.css, styles.css)   │
│  └── Use CSS variables with fallback values         │
└─────────────────────────────────────────────────────┘
```

### Data Flow

```
Settings Panel
    ↓ User changes value
    ↓ invoke("save_settings", {settings})
Rust Backend
    ↓ validate_settings() (includes markdown_font_size: 8-32)
    ↓ write settings.json
    ↓ returns success
Frontend
    ↓ applyMarkdownSettings()
    ↓ sets CSS variables:
    ↓   --markdown-body-font-family
    ↓   --markdown-code-font-family
    ↓   --markdown-body-font-size
CSS
    ↓ .markdown-fullscreen-content uses var(--markdown-body-font-family, <fallback>)
    ↓ .markdown-content code uses var(--markdown-code-font-family, <fallback>)
    ↓ .markdown-fullscreen-content uses var(--markdown-body-font-size, 14pt)
Markdown Overlay
    ↓ Renders with updated fonts/size
```

### Backend Changes (config.rs)

#### New fields in AppSettings

```rust
#[serde(default = "default_markdown_body_font_family")]
pub markdown_body_font_family: String,

#[serde(default = "default_markdown_code_font_family")]
pub markdown_code_font_family: String,

#[serde(default = "default_markdown_font_size")]
pub markdown_font_size: u32,
```

#### Default values

```rust
fn default_markdown_body_font_family() -> String { String::new() }
fn default_markdown_code_font_family() -> String { String::new() }
fn default_markdown_font_size() -> u32 { 14 }
```

#### Validation addition

```rust
// In validate_settings():
if settings.markdown_font_size < 8 || settings.markdown_font_size > 32 {
    return Err("markdown_font_size must be between 8 and 32".to_string());
}
```

#### Null-safe deserialization

Add `deserialize_null_with!` entries for the three new fields, following the existing pattern.

### Frontend Changes

#### settings-panel.ts

Add `markdown-viewer` to the categories array as the 5th item.

#### settings-sections.ts

Add `renderMarkdownViewerSection()` function:
- Subsection: "Font"
  - Font picker for body font family
  - Font picker for code font family
  - Number input for font size (8-32, step 1, unit "pt")

#### settings-applier.ts

Add `applyMarkdownSettings()` function:
- Set `--markdown-body-font-family` CSS variable on document root
- Set `--markdown-code-font-family` CSS variable on document root
- Set `--markdown-body-font-size` CSS variable on document root (with "pt" unit suffix)
- Call from `applySettings()`

#### CSS Changes (styles.css, fullscreen.css)

Replace hardcoded font values with CSS variables:

```css
/* .markdown-fullscreen-content (fullscreen.css) */
font-family: var(--markdown-body-font-family, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif);
font-size: var(--markdown-body-font-size, 14pt);

/* .markdown-content (styles.css) */
font-family: var(--markdown-body-font-family, -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif);
font-size: var(--markdown-body-font-size, 14pt);

/* .markdown-content code (styles.css) */
font-family: var(--markdown-code-font-family, "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace);
```

### Dependencies

**Internal Dependencies:**
- Settings panel component system (settings-panel.ts, settings-sections.ts, settings-components.ts)
- Font picker dialog (existing component)
- Settings applier (settings-applier.ts)
- Settings service (settings-service.ts)
- Rust config module (config.rs)

**External Dependencies:**
- None (all existing)

### FontCategory Extension

The existing `FontCategory` type (`"primary" | "secondary" | "emoji" | "ui"`) determines which font list is displayed and the picker title. Two new categories are needed:

- `"markdown-body"` → `all_fonts` (sans-serif fonts for body text)
- `"markdown-code"` → `monospace_fonts` (monospace fonts for code blocks)

This requires extending `FontCategory` in `types.ts` and adding corresponding entries in `font-picker.ts` (title map and font list switch).

### File Structure

```
src-tauri/src/commands/config.rs     # Add 3 fields, validation, null-safe deser
src/settings/settings-panel.ts       # Add markdown-viewer category
src/settings/settings-sections.ts    # Add renderMarkdownViewerSection()
src/settings/settings-applier.ts     # Add applyMarkdownSettings()
src/settings/types.ts                # Add 3 fields + extend FontCategory
src/settings/font-picker.ts          # Add markdown-body/markdown-code to titleMap and switch
src/styles.css                       # Replace hardcoded values with CSS vars
src/markdown/fullscreen.css          # Replace hardcoded values with CSS vars
```

## Test Scenarios

### Unit Tests (Rust)

- [ ] Default values: markdown_font_size defaults to 14, font families default to empty string
- [ ] Validation: markdown_font_size < 8 returns error
- [ ] Validation: markdown_font_size > 32 returns error
- [ ] Validation: markdown_font_size = 8 passes
- [ ] Validation: markdown_font_size = 32 passes
- [ ] Deserialization: missing fields use defaults
- [ ] Deserialization: null fields use defaults
- [ ] Deserialization: valid values are preserved
- [ ] Round-trip: serialize then deserialize preserves values

### Unit Tests (TypeScript)

- [ ] renderMarkdownViewerSection renders font pickers and number input
- [ ] applyMarkdownSettings sets correct CSS variables

### Edge Cases

- [ ] Empty font family string: CSS fallback chain is used
- [ ] Settings file without new fields: defaults applied (backward compatibility)
- [ ] Settings file with null values for new fields: defaults applied

## Security Considerations

- **Input Validation:** Font size validated on Rust backend (8-32 range)
- **XSS Prevention:** Font family strings are applied via CSS variables (not innerHTML), no XSS risk

## Error Handling

| Error | Condition | Handling |
|-------|-----------|---------|
| Invalid font size | Value outside 8-32 | Backend returns validation error, frontend shows error |
| Invalid settings.json | Corrupt or missing fields | Defaults used via serde(default) |

## Success Criteria

- [ ] Settings panel shows "Markdown Viewer" as 5th category
- [ ] Body font family configurable via font picker
- [ ] Code font family configurable via font picker
- [ ] Font size configurable via number input (8-32pt)
- [ ] Changes apply immediately to Markdown fullscreen overlay
- [ ] Settings persist in settings.json
- [ ] Backward compatible with existing settings.json
- [ ] Rust validation works correctly
- [ ] All tests pass

## Open Questions

None - all questions resolved during requirements gathering.
