# Feature: Markdown Viewer Color Theme Settings

## Overview

Add configurable color theme settings to the Markdown viewer. Users can either follow the UI theme (light/dark + preset) or independently select a theme/preset for the Markdown viewer. Each combination (light/dark x purple/blue/green/orange) has a dedicated color palette for Markdown rendering.

As part of this feature, remove dead code (unused theme functions in `theme.ts`) and migrate missing element styles to `fullscreen.css`.

## Objectives

- Define 8 Markdown color palettes (light/dark x 4 presets)
- Provide a toggle to follow or detach from the UI theme settings
- Apply the selected palette to `--markdown-*` CSS variables (fullscreen display)
- Remove dead code: `generateMarkdownTheme()`, `applyMarkdownTheme()`, `theme.ts` file
- Migrate missing element styles from `styles.css` to `fullscreen.css`
- Persist settings with full backward compatibility

## User Stories

### US1: Follow UI Theme
As a user, I want the Markdown viewer to automatically match my UI theme settings, so that the app looks visually consistent.

**Acceptance Criteria:**
- [ ] Toggle ON makes Markdown colors follow ui_theme + ui_theme_preset
- [ ] Changing UI theme/preset immediately updates Markdown colors
- [ ] System theme changes are reflected when ui_theme is "system"

### US2: Independent Theme Selection
As a user, I want to choose a different theme for the Markdown viewer, so that I can optimize readability independently of the UI theme.

**Acceptance Criteria:**
- [ ] Toggle OFF shows theme (system/light/dark) and preset (purple/blue/green/orange) selectors
- [ ] Selecting a combination immediately applies the corresponding palette
- [ ] Settings are persisted and restored on restart

## Technical Requirements

### Functional Requirements
- **FR1:** Define `MARKDOWN_THEME_PRESETS` constant with 8 palettes (dark/light x 4 presets), each containing 11 colors matching the `MarkdownThemeColors` interface
- **FR2:** Add `markdown_theme_follow_ui` (bool, default: true), `markdown_theme` (UiTheme enum, default: system), `markdown_theme_preset` (UiThemePreset enum, default: purple) to settings
- **FR3:** Add settings UI: toggle + conditional theme/preset selectors in the Markdown Viewer section
- **FR4:** Apply the resolved palette to `--markdown-*` CSS variables
- **FR5:** When `markdown_theme_follow_ui` is true and UI theme/preset changes, re-apply Markdown colors
- **FR6:** When theme is "system", listen to `prefers-color-scheme` media query and auto-switch
- **FR7:** Migrate missing element styles (h3-h6, p, ul/ol/li, inline code, pre code, tables, hr, img, strong/em) from `styles.css` to `fullscreen.css` using `--markdown-*` variables. Note: `.markdown-block` and `.markdown-content` styles in `styles.css` are retained as they are still used by `renderer.ts` for inline Markdown display and E2E tests.
- **FR8:** Remove dead code: `theme.ts` file (contains `generateMarkdownTheme()`, `applyMarkdownTheme()`, `getDarkTheme()`, `getLightTheme()`, `MarkdownTheme` interface, `DARK_THEME`/`LIGHT_THEME` constants, helper functions), `theme.test.ts` file, dead exports from `index.ts`

### Non-Functional Requirements
- **NFR1 - Backward Compatibility:** Missing or null fields in settings.json use defaults
- **NFR2 - Performance:** Theme switching applies instantly via CSS variables (no DOM re-rendering)

## Implementation Approach

### Architecture

The feature touches the following layers:

```
Settings UI (settings-sections.ts)
    |
    v
Settings Applier (settings-applier.ts)
    |
    v
Markdown Theme Presets (new: markdown-theme-presets.ts)
    |
    v
CSS Variables (--markdown-*)
    |
    v
Fullscreen View (fullscreen.css)
```

### New File: `src/settings/markdown-theme-presets.ts`

Defines the 8 Markdown color palettes.

```typescript
import type { UiThemePreset } from "./types";

export interface MarkdownThemeColors {
  /** Background color */
  bg: string;
  /** Foreground/text color */
  fg: string;
  /** Heading color */
  heading: string;
  /** Link color */
  link: string;
  /** Border color */
  border: string;
  /** Blockquote text color */
  blockquote: string;
  /** Inline code background */
  codeBg: string;
  /** Code text color */
  codeFg: string;
  /** Code block (pre) background */
  preBg: string;
  /** Table background */
  tableBg: string;
  /** Table stripe (alternating row) background */
  tableStripe: string;
}

export interface MarkdownPresetDefinition {
  dark: MarkdownThemeColors;
  light: MarkdownThemeColors;
}

export const MARKDOWN_THEME_PRESETS: Record<UiThemePreset, MarkdownPresetDefinition> = {
  purple: {
    dark: {
      bg: "...",
      fg: "...",
      heading: "...",
      link: "...",
      border: "...",
      blockquote: "...",
      codeBg: "...",
      codeFg: "...",
      preBg: "...",
      tableBg: "...",
      tableStripe: "...",
    },
    light: { /* ... */ },
  },
  blue: { dark: { /* ... */ }, light: { /* ... */ } },
  green: { dark: { /* ... */ }, light: { /* ... */ } },
  orange: { dark: { /* ... */ }, light: { /* ... */ } },
};
```

Color values should be derived from the corresponding UI theme preset colors (`UI_THEME_PRESETS` in `ui-theme-presets.ts`) to maintain visual harmony. For example, for the purple/dark preset:

- `bg`: Based on surface color (`#141218`)
- `fg`: Based on onSurface color (`#E6E0E9`)
- `heading`: Brighter variant of onSurface
- `link`: Based on primary color (`#D0BCFF`)
- `border`: Based on outlineVariant (`#49454F`)
- `blockquote`: Based on onSurfaceVariant (`#CAC4D0`)
- `codeBg`: Semi-transparent secondary container
- `codeFg`: Same as fg
- `preBg`: Darker variant of surface (code block background)
- `tableBg`: Transparent or very subtle surface variant
- `tableStripe`: Semi-transparent secondary container (alternating rows)

### Settings Changes

#### Rust: `src-tauri/src/commands/config.rs`

Add three fields to `AppSettings`:

```rust
// Markdown Viewer Theme
#[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
pub markdown_theme_follow_ui: bool,
#[serde(default, deserialize_with = "deserialize_null_default")]
pub markdown_theme: UiTheme,
#[serde(default, deserialize_with = "deserialize_null_default")]
pub markdown_theme_preset: UiThemePreset,
```

Update `Default for AppSettings`:
```rust
markdown_theme_follow_ui: default_true(),  // true
markdown_theme: UiTheme::default(),        // System
markdown_theme_preset: UiThemePreset::default(), // Purple
```

No additional validation needed (enum validation is handled by serde).

#### TypeScript: `src/settings/types.ts`

Add to `AppSettings` interface:

```typescript
// Markdown Viewer Theme
markdown_theme_follow_ui: boolean;
markdown_theme: UiTheme;
markdown_theme_preset: UiThemePreset;
```

### Settings UI: `src/settings/settings-sections.ts`

Add a "Color Theme" subsection to `renderMarkdownViewerSection()`:

```
[Subsection: Color Theme]
  Toggle: "Follow UI Theme" (markdown_theme_follow_ui)
  [If toggle OFF:]
    Select: Theme (system/light/dark) (markdown_theme)
    Select: Preset (purple/blue/green/orange) (markdown_theme_preset)
```

When the toggle changes:
- ON: Re-apply using UI settings values, hide theme/preset selectors, re-render section
- OFF: Show theme/preset selectors, apply using markdown-specific values, re-render section

When theme or preset changes (toggle OFF):
- Apply the new palette immediately

### Settings Applier: `src/settings/settings-applier.ts`

Add new function `applyMarkdownColorTheme()`:

```typescript
export function applyMarkdownColorTheme(
  followUi: boolean,
  mdTheme: UiTheme,
  mdPreset: UiThemePreset,
  uiTheme: UiTheme,
  uiPreset: UiThemePreset,
): void {
  const effectiveTheme = followUi ? uiTheme : mdTheme;
  const effectivePreset = followUi ? uiPreset : mdPreset;

  // Resolve "system" to actual light/dark
  const resolved = resolveTheme(effectiveTheme);

  // Get palette
  const palette = MARKDOWN_THEME_PRESETS[effectivePreset][resolved];

  // Apply to --markdown-* CSS variables
  applyMarkdownCssVars(palette);
}
```

The `resolveTheme()` function handles the "system" case by checking `window.matchMedia("(prefers-color-scheme: dark)")`.

For `system` theme, a media query listener must be registered (similar to existing `applyUiTheme()`).

#### CSS Variable Mapping

| Palette Key | CSS Variable |
|------------|-------------|
| bg | `--markdown-bg` |
| fg | `--markdown-fg` |
| heading | `--markdown-heading` |
| link | `--markdown-link` |
| border | `--markdown-border` |
| blockquote | `--markdown-blockquote` |
| codeBg | `--markdown-code-bg` |
| codeFg | `--markdown-code-fg` |
| preBg | `--markdown-pre-bg` |
| tableBg | `--markdown-table-bg` |
| tableStripe | `--markdown-table-stripe` |

Note: Some `--markdown-*` variables may need to be added to `fullscreen.css` where they aren't yet used (e.g., `--markdown-code-fg`, `--markdown-pre-bg`, `--markdown-table-bg`, `--markdown-table-stripe`).

#### Integration with UI Theme Changes

When `applyUiTheme()` is called and `markdown_theme_follow_ui` is true, `applyMarkdownColorTheme()` must also be called. The simplest approach:

`applySettings()` calls `applyMarkdownColorTheme()` after `applyUiTheme()`.

For live UI theme changes in the settings panel (where individual apply functions are called), the settings section callback for ui_theme/ui_theme_preset must also trigger `applyMarkdownColorTheme()` when `markdown_theme_follow_ui` is true.

### CSS Migration (FR7)

#### Migrate to `fullscreen.css`

Add styles for elements not yet covered in `fullscreen.css` under `.markdown-fullscreen-content` selectors using `--markdown-*` variables:

| Element | Currently in styles.css | Action |
|---------|------------------------|--------|
| h3-h6 headings | `.markdown-content h3`-`h6` | Add to `fullscreen.css` |
| paragraphs | `.markdown-content p` | Add to `fullscreen.css` |
| lists (ul/ol/li) | `.markdown-content ul/ol/li` | Add to `fullscreen.css` |
| inline code | `.markdown-content code` | Add to `fullscreen.css` with `--markdown-code-bg`, `--markdown-code-fg` |
| pre > code | `.markdown-content pre code` | Add to `fullscreen.css` |
| table striping | `.markdown-content table tr:nth-child(2n)` | Add to `fullscreen.css` with `--markdown-table-bg`, `--markdown-table-stripe` |
| horizontal rule | `.markdown-content hr` | Add to `fullscreen.css` |
| images | `.markdown-content img` | Add to `fullscreen.css` |
| strong/em | `.markdown-content strong/em` | Add to `fullscreen.css` |

Note: `.markdown-block` and `.markdown-content` styles in `styles.css` are **retained** because `renderer.ts` generates `<div class="markdown-content">` for inline Markdown display and E2E tests reference `.markdown-block`.

#### Remove dead code

- Delete `src/markdown/theme.ts` entirely (all functions replaced by `markdown-theme-presets.ts`)
- Delete `src/markdown/theme.test.ts` entirely
- Update `src/markdown/index.ts` to remove dead exports from `theme.ts`

### File Structure

```
src/settings/
├── markdown-theme-presets.ts      # NEW: 8 markdown color palettes
├── markdown-theme-presets.test.ts # NEW: tests for palettes
├── settings-sections.ts           # MODIFIED: add color theme subsection
├── settings-applier.ts            # MODIFIED: add applyMarkdownColorTheme()
├── settings-applier.test.ts       # MODIFIED: add tests
├── types.ts                       # MODIFIED: add 3 fields to AppSettings
├── ...

src/markdown/
├── fullscreen.css                 # MODIFIED: add migrated styles with --markdown-* vars
├── theme.ts                       # DELETED: dead code replaced by markdown-theme-presets.ts
├── theme.test.ts                  # DELETED: tests for removed functions
├── index.ts                       # MODIFIED: remove dead exports
├── ...

src-tauri/src/commands/
├── config.rs                      # MODIFIED: add 3 fields to AppSettings

src/i18n/locales/
├── en.json                        # MODIFIED: add color theme i18n keys
├── ja.json                        # MODIFIED: add color theme i18n keys
```

### i18n Keys

**English (`src/i18n/locales/en.json`):**
```json
{
  "settings": {
    "markdownViewer": {
      "colorTheme": "Color Theme",
      "followUiTheme": "Follow UI Theme",
      "followUiThemeDesc": "Use the same theme and preset as the UI settings",
      "theme": "Theme",
      "themeDesc": "Color theme for the Markdown viewer",
      "themeSystem": "System",
      "themeLight": "Light",
      "themeDark": "Dark",
      "preset": "Preset",
      "presetDesc": "Color preset for the Markdown viewer"
    }
  }
}
```

**Japanese (`src/i18n/locales/ja.json`):**
```json
{
  "settings": {
    "markdownViewer": {
      "colorTheme": "カラーテーマ",
      "followUiTheme": "UIテーマに合わせる",
      "followUiThemeDesc": "UI設定と同じテーマ・プリセットを使用します",
      "theme": "テーマ",
      "themeDesc": "Markdownビューアーのカラーテーマ",
      "themeSystem": "システム",
      "themeLight": "ライト",
      "themeDark": "ダーク",
      "preset": "プリセット",
      "presetDesc": "Markdownビューアーのカラープリセット"
    }
  }
}
```

Note: Preset labels (Purple, Blue, Green, Orange) reuse existing i18n keys from `settings.appearance.preset*`.

### Rust i18n Keys

No new Rust validation messages needed (enum validation is handled by serde deserialization).

## Test Scenarios

### Unit Tests

#### `markdown-theme-presets.test.ts`
- [ ] Each preset (purple/blue/green/orange) has both dark and light variants
- [ ] Each variant has all 11 required color properties
- [ ] All color values are valid CSS color strings (hex or rgba format)

#### `settings-applier.test.ts` (additions)
- [ ] `applyMarkdownColorTheme()` with followUi=true uses UI theme/preset
- [ ] `applyMarkdownColorTheme()` with followUi=false uses markdown theme/preset
- [ ] `applyMarkdownColorTheme()` sets all 11 `--markdown-*` color CSS variables
- [ ] System theme resolves correctly based on media query

#### Rust `config.rs` tests (additions)
- [ ] Default settings have `markdown_theme_follow_ui: true`
- [ ] Default settings have `markdown_theme: System`
- [ ] Default settings have `markdown_theme_preset: Purple`
- [ ] Missing fields in JSON use defaults
- [ ] Null fields in JSON use defaults
- [ ] Round-trip serialization preserves values
- [ ] Invalid enum values are rejected by serde

### Integration Tests
- [ ] Settings panel renders color theme subsection correctly
- [ ] Toggle ON hides theme/preset selectors
- [ ] Toggle OFF shows theme/preset selectors
- [ ] Changing toggle applies correct theme immediately

### Edge Cases
- [ ] Toggle ON -> change UI theme -> Markdown theme updates
- [ ] Toggle OFF -> change UI theme -> Markdown theme does NOT change
- [ ] Theme "system" + OS theme change -> Markdown colors update
- [ ] Settings file with only `markdown_theme_follow_ui: false` (other fields use defaults)

### CSS Cleanup Verification
- [ ] `generateMarkdownTheme` and `applyMarkdownTheme` are not exported from `index.ts`
- [ ] No imports of removed functions exist in the codebase
- [ ] All fullscreen element styles use `--markdown-*` CSS variables

## Security Considerations

- **Input Validation:** Theme and preset values are constrained to enum variants by serde (Rust) and TypeScript types
- **CSS Injection:** Color values are hardcoded constants, not user-provided

## Error Handling

### Error Codes

| Code | Description | HTTP Status | User Message |
|------|-------------|-------------|--------------|
| N/A | Invalid enum in settings JSON | N/A | Settings file parse error (falls back to defaults) |

## Success Criteria

- [ ] All 8 Markdown color palettes defined and visually coherent with UI presets
- [ ] Toggle ON/OFF works correctly in settings UI
- [ ] `--markdown-*` CSS variables receive palette colors
- [ ] System theme auto-switching works for Markdown viewer
- [ ] UI theme changes propagate to Markdown when follow mode is on
- [ ] All settings persisted and backward compatible
- [ ] Dead code removed (`theme.ts`, `theme.test.ts`, dead exports from `index.ts`)
- [ ] All test scenarios pass
- [ ] TypeScript type check passes
- [ ] Rust tests pass

## Open Questions

- None

## References

- UI Theme Presets: `src/settings/ui-theme-presets.ts`
- Markdown Fullscreen: `src/markdown/fullscreen.ts`
- Settings Applier: `src/settings/settings-applier.ts`
- Rust Settings: `src-tauri/src/commands/config.rs`
- Fullscreen Display CSS: `src/markdown/fullscreen.css`
