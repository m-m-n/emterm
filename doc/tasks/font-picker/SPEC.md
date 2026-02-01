# Feature: Font Picker

## Overview

Replace manual text inputs for font family settings with a font picker UI that enumerates system fonts via the font-kit crate, displays them with preview rendering, and allows selection through an in-place transition within the settings panel.

## Objectives

- Enumerate system fonts from Rust backend using font-kit
- Classify fonts into monospace, all, and emoji categories
- Provide an in-place font picker UI with search and preview
- Replace text inputs with readonly input + "Change" button

## User Stories

### US1: Select Primary Font
As a user, I want to browse and select a monospace font from a list, so that I can configure the terminal's primary font without memorizing font names.

**Acceptance Criteria:**
- [ ] Font picker shows only monospace fonts
- [ ] Each font name is rendered in its own typeface
- [ ] Selecting a font saves it and applies to the terminal immediately
- [ ] Back button returns to settings without changes

### US2: Select Secondary Font
As a user, I want to browse all installed fonts to select a CJK font, so that I can configure the secondary font for Japanese/Chinese/Korean characters.

**Acceptance Criteria:**
- [ ] Font picker shows all fonts
- [ ] Selecting a font saves it and applies to the terminal immediately

### US3: Select Emoji Font
As a user, I want to select an emoji font from a filtered list, so that I can configure emoji rendering in the terminal.

**Acceptance Criteria:**
- [ ] Font picker shows only emoji fonts (name-based heuristic)
- [ ] Selecting a font saves it and applies to the terminal immediately

### US4: Search Fonts
As a user, I want to search fonts by name, so that I can quickly find a specific font in a large list.

**Acceptance Criteria:**
- [ ] Search input filters the font list in real-time
- [ ] Search is case-insensitive
- [ ] Clearing search shows all fonts again
- [ ] "No fonts found" message when search has no matches

## Technical Requirements

### Functional Requirements
- **FR1:** Add `font-kit` crate dependency to `src-tauri/Cargo.toml`
- **FR2:** Create `list_fonts` Tauri command that returns categorized font lists
- **FR3:** Cache font enumeration result using `OnceLock` (one-time initialization)
- **FR4:** Create `FontPicker` UI component in TypeScript
- **FR5:** Replace font text inputs with readonly input + change button
- **FR6:** Implement in-place transition within settings panel content area
- **FR7:** Render each font list item in its own typeface for preview
- **FR8:** Implement case-insensitive search filtering
- **FR9:** Add i18n keys for font picker UI elements

### Non-Functional Requirements
- **NFR1 - Performance:** Font enumeration completes within 5 seconds on first call
- **NFR2 - Performance:** Font picker view renders within 100ms
- **NFR3 - Performance:** Search filtering completes within 16ms per keystroke
- **NFR4 - Accessibility:** Font list uses `role="listbox"` / `role="option"` ARIA pattern
- **NFR5 - Accessibility:** Keyboard navigation: Arrow keys, Enter, Escape
- **NFR6 - Platform:** Works on Linux, macOS, and Windows

---

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│  Settings Panel (TypeScript)                    │
│  ┌──────────────┐  ┌─────────────────────────┐  │
│  │ Nav (left)    │  │ Content (right)         │  │
│  │               │  │ ┌─────────────────────┐ │  │
│  │ [Appearance]  │  │ │ Settings View       │ │  │
│  │ [Terminal]    │  │ │ (readonly + Change)  │ │  │
│  │ [Keybinds]    │  │ └─────────────────────┘ │  │
│  │               │  │         ↕ transition     │  │
│  │               │  │ ┌─────────────────────┐ │  │
│  │               │  │ │ Font Picker View    │ │  │
│  │               │  │ │ (← Back, Search,    │ │  │
│  │               │  │ │  Font List)         │ │  │
│  │               │  │ └─────────────────────┘ │  │
│  └──────────────┘  └─────────────────────────┘  │
└─────────────────────────────────────────────────┘
         │
         │ invoke("list_fonts")
         ▼
┌─────────────────────────────────────────────────┐
│  Rust Backend (Tauri)                           │
│  ┌────────────────────┐  ┌────────────────────┐ │
│  │ commands/font.rs   │  │ OnceLock Cache     │ │
│  │ list_fonts()       │→ │ FontListResponse   │ │
│  └────────────────────┘  └────────────────────┘ │
│                                    ↑             │
│                          font-kit enumeration    │
└─────────────────────────────────────────────────┘
```

### Data Flow

**Font Enumeration:**
```
Frontend (init)  →  invoke("list_fonts")  →  Rust: OnceLock check
                                                  ↓ (cache miss)
                                              font-kit enumerate
                                              classify & sort
                                              store in OnceLock
                 ←  FontListResponse       ←  return cached data
```

**Font Selection:**
```
User clicks "Change"  →  Replace content with FontPicker
User selects font     →  Update currentSettings
                      →  applyFontFamily(primary, emoji, secondary)
                      →  saveSetting(field, value)
                      →  Restore settings content
```

---

## Backend Implementation

### New File: `src-tauri/src/commands/font.rs`

#### Tauri Command: `list_fonts`

```rust
use font_kit::source::SystemSource;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize)]
pub struct FontListResponse {
    pub monospace_fonts: Vec<String>,
    pub all_fonts: Vec<String>,
    pub emoji_fonts: Vec<String>,
}

static FONT_CACHE: OnceLock<FontListResponse> = OnceLock::new();

#[tauri::command]
pub fn list_fonts() -> Result<FontListResponse, String> {
    let response = FONT_CACHE.get_or_init(|| enumerate_fonts());
    Ok(response.clone())
}
```

#### Font Enumeration Logic

```rust
fn enumerate_fonts() -> FontListResponse {
    let source = SystemSource::new();
    let families = source.all_families().unwrap_or_default();

    let mut monospace_fonts = Vec::new();
    let mut all_fonts = Vec::new();
    let mut emoji_fonts = Vec::new();

    for family_name in &families {
        all_fonts.push(family_name.clone());

        // Emoji detection: name-based heuristic
        if family_name.to_lowercase().contains("emoji") {
            emoji_fonts.push(family_name.clone());
        }

        // Monospace detection: load font and check property
        if let Ok(family) = source.select_family_by_name(family_name) {
            if let Some(font) = family.fonts().first() {
                if let Ok(font) = font.load() {
                    if font.is_monospace() {
                        monospace_fonts.push(family_name.clone());
                    }
                }
            }
        }
    }

    monospace_fonts.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    all_fonts.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    emoji_fonts.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    monospace_fonts.dedup();
    all_fonts.dedup();
    emoji_fonts.dedup();

    FontListResponse {
        monospace_fonts,
        all_fonts,
        emoji_fonts,
    }
}
```

### Register Command

**File: `src-tauri/src/commands/mod.rs`**

Add:
```rust
pub mod font;
```

**File: `src-tauri/src/lib.rs`** (or wherever commands are registered)

Add `commands::font::list_fonts` to the Tauri invoke handler.

### Cargo.toml Changes

**File: `src-tauri/Cargo.toml`**

Add dependency:
```toml
font-kit = "0.14"
```

---

## Frontend Implementation

### TypeScript Types

**File: `src/settings/types.ts`**

Add:
```typescript
export interface FontListResponse {
  monospace_fonts: string[];
  all_fonts: string[];
  emoji_fonts: string[];
}

export type FontCategory = "primary" | "secondary" | "emoji";
```

### Font Service

**File: `src/settings/font-service.ts`**

```typescript
import { invoke } from "@tauri-apps/api/core";
import type { FontListResponse } from "./types";

export class FontService {
  private static cachedFonts: FontListResponse | null = null;

  static async list(): Promise<FontListResponse> {
    if (FontService.cachedFonts) {
      return FontService.cachedFonts;
    }
    const fonts = await invoke<FontListResponse>("list_fonts");
    FontService.cachedFonts = fonts;
    return fonts;
  }
}
```

### Settings Panel Changes

**File: `src/settings/settings-panel.ts`**

#### New Method: `renderFontPickerInput`

Replace the three `renderTextInput` calls for font families with a new `renderFontPickerInput` method:

```typescript
private renderFontPickerInput(panel: HTMLElement, opts: {
  key: string;
  label: string;
  value: string;
  placeholder: string;
  hint: string;
  description?: string;
  category: FontCategory;
  onSelect: (value: string) => void;
}): void
```

This renders:
1. Label and description (same as existing)
2. A horizontal group:
   - `<input type="text" readonly>` displaying current font name
   - `<button>` with text from `t("settings.appearance.fontPickerChange")`
3. Hint text

The button click triggers `showFontPicker(category, currentValue, onSelect)`.

#### New Method: `showFontPicker`

```typescript
private async showFontPicker(
  category: FontCategory,
  currentValue: string,
  onSelect: (value: string) => void,
): Promise<void>
```

This method:
1. Detaches current content listeners
2. Clears `contentElement`
3. Disables navigation tabs (set `aria-disabled`, add `disabled` class)
4. Renders the font picker UI into `contentElement`
5. Loads fonts via `FontService.list()`
6. Selects the appropriate font list based on `category`:
   - `"primary"` → `monospace_fonts`
   - `"secondary"` → `all_fonts`
   - `"emoji"` → `emoji_fonts`
7. Renders the font list with preview

#### Font Picker UI Structure

```html
<div class="font-picker">
  <div class="font-picker-header">
    <button class="font-picker-back" aria-label="{t:fontPickerBack}">←</button>
    <h3 class="font-picker-title">{category title}</h3>
  </div>
  <div class="font-picker-search">
    <input type="text"
           class="font-picker-search-input"
           placeholder="{t:fontPickerSearch}"
           aria-label="{t:fontPickerSearch}">
  </div>
  <div class="font-picker-list" role="listbox" aria-label="{category title}">
    <div class="font-picker-item" role="option"
         style="font-family: '{fontName}', sans-serif"
         aria-selected="true|false">
      {fontName}
    </div>
    <!-- ... more items -->
  </div>
</div>
```

#### New Method: `hideFontPicker`

```typescript
private hideFontPicker(): void
```

1. Re-enables navigation tabs
2. Detaches font picker content listeners
3. Calls `renderContent()` to restore the settings view

#### Search Implementation

```typescript
private filterFontList(searchText: string, fonts: string[]): string[] {
  if (!searchText) return fonts;
  const lower = searchText.toLowerCase();
  return fonts.filter(name => name.toLowerCase().includes(lower));
}
```

Attach an `input` event listener on the search field that:
1. Calls `filterFontList` with the current search text and full font list
2. Re-renders only the list portion of the font picker

#### Font Selection

When a font list item is clicked:
1. Call `onSelect(fontName)` callback
2. Call `hideFontPicker()`

#### Keyboard Navigation

- **Arrow Down/Up**: Move focus between font list items
- **Enter**: Select the focused font
- **Escape**: Close font picker (same as back button)

### Appearance Section Changes

**File: `src/settings/settings-panel.ts` - `renderAppearanceSection`**

Replace the three `renderTextInput` calls for font families:

```typescript
// Primary Font (font picker)
this.renderFontPickerInput(panel, {
  key: "font-family-primary",
  label: t("settings.appearance.fontFamilyPrimary"),
  value: this.currentSettings.font_family_primary,
  placeholder: t("settings.appearance.fontFamilyPrimaryPlaceholder"),
  hint: t("settings.appearance.fontFamilyPrimaryHint"),
  description: t("settings.appearance.fontFamilyPrimaryDesc"),
  category: "primary",
  onSelect: (v) => {
    this.currentSettings!.font_family_primary = v;
    this.applyCurrentFontFamily();
    this.saveSetting("font_family_primary", v);
  },
});

// Secondary Font (font picker)
this.renderFontPickerInput(panel, {
  key: "font-family-secondary",
  label: t("settings.appearance.fontFamilySecondary"),
  value: this.currentSettings.font_family_secondary,
  placeholder: t("settings.appearance.fontFamilySecondaryPlaceholder"),
  hint: t("settings.appearance.fontFamilySecondaryHint"),
  description: t("settings.appearance.fontFamilySecondaryDesc"),
  category: "secondary",
  onSelect: (v) => {
    this.currentSettings!.font_family_secondary = v;
    this.applyCurrentFontFamily();
    this.saveSetting("font_family_secondary", v);
  },
});

// Emoji Font (font picker)
this.renderFontPickerInput(panel, {
  key: "font-family-emoji",
  label: t("settings.appearance.fontFamilyEmoji"),
  value: this.currentSettings.font_family_emoji,
  placeholder: t("settings.appearance.fontFamilyEmojiPlaceholder"),
  hint: t("settings.appearance.fontFamilyEmojiHint"),
  description: t("settings.appearance.fontFamilyEmojiDesc"),
  category: "emoji",
  onSelect: (v) => {
    this.currentSettings!.font_family_emoji = v;
    this.applyCurrentFontFamily();
    this.saveSetting("font_family_emoji", v);
  },
});
```

---

## CSS Styles

### New File: `src/styles/font-picker.css`

Or add to `src/styles/settings-panel.css`:

```css
/* Font Picker - In-place transition view */
.font-picker {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

/* Header: back button + title */
.font-picker-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 16px;
}

.font-picker-back {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: var(--md-sys-shape-corner-full);
  color: var(--md-sys-color-on-surface);
  font-size: 20px;
  cursor: pointer;
  transition: background-color var(--md-motion-duration-short4) var(--md-motion-easing-standard);
}

.font-picker-back:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.font-picker-back:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

.font-picker-title {
  font-size: 16px;
  font-weight: 500;
  line-height: 24px;
  color: var(--md-sys-color-on-surface);
  margin: 0;
}

/* Search bar */
.font-picker-search {
  margin-bottom: 12px;
}

.font-picker-search-input {
  width: 100%;
  padding: 12px 16px;
  background-color: transparent;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: var(--md-sys-shape-corner-extra-small);
  color: var(--md-sys-color-on-surface);
  font-size: 16px;
  line-height: 24px;
  font-family: inherit;
  transition: border-color var(--md-motion-duration-short4) var(--md-motion-easing-standard);
}

.font-picker-search-input::placeholder {
  color: var(--md-sys-color-on-surface-variant);
}

.font-picker-search-input:focus {
  outline: none;
  border-color: var(--md-sys-color-primary);
  border-width: 2px;
  padding: 11px 15px;
}

/* Font list */
.font-picker-list {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
}

.font-picker-item {
  display: flex;
  align-items: center;
  padding: 12px 16px;
  border-radius: var(--md-sys-shape-corner-small);
  cursor: pointer;
  font-size: 16px;
  line-height: 24px;
  color: var(--md-sys-color-on-surface);
  transition: background-color var(--md-motion-duration-short4) var(--md-motion-easing-standard);
}

.font-picker-item:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.font-picker-item:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

.font-picker-item[aria-selected="true"] {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

/* No results message */
.font-picker-no-results {
  padding: 24px 16px;
  text-align: center;
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
}

/* Font picker input row (readonly + button) */
.settings-font-picker-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.settings-font-picker-input {
  flex: 1;
  max-width: 260px;
  padding: 12px 16px;
  background-color: var(--md-sys-color-surface-variant);
  border: 1px solid var(--md-sys-color-outline-variant);
  border-radius: var(--md-sys-shape-corner-extra-small);
  color: var(--md-sys-color-on-surface);
  font-size: 16px;
  line-height: 24px;
  font-family: inherit;
  cursor: default;
}

.settings-font-picker-input::placeholder {
  color: var(--md-sys-color-on-surface-variant);
}

.settings-font-picker-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 40px;
  padding: 0 24px;
  background-color: transparent;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: var(--md-sys-shape-corner-full);
  color: var(--md-sys-color-primary);
  font-size: 14px;
  font-weight: 500;
  font-family: inherit;
  cursor: pointer;
  transition:
    background-color var(--md-motion-duration-short4) var(--md-motion-easing-standard),
    border-color var(--md-motion-duration-short4) var(--md-motion-easing-standard);
}

.settings-font-picker-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.settings-font-picker-button:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}
```

---

## i18n Changes

### File: `src/i18n/locales/en.json`

Add to `settings.appearance`:

```json
"fontPickerBack": "Back",
"fontPickerSearch": "Search fonts...",
"fontPickerNoResults": "No fonts found",
"fontPickerChange": "Change",
"fontPickerPrimaryTitle": "Primary Font",
"fontPickerSecondaryTitle": "Secondary Font",
"fontPickerEmojiTitle": "Emoji Font"
```

### File: `src/i18n/locales/ja.json`

Add to `settings.appearance`:

```json
"fontPickerBack": "戻る",
"fontPickerSearch": "フォントを検索...",
"fontPickerNoResults": "フォントが見つかりません",
"fontPickerChange": "変更",
"fontPickerPrimaryTitle": "プライマリフォント",
"fontPickerSecondaryTitle": "セカンダリフォント",
"fontPickerEmojiTitle": "絵文字フォント"
```

---

## Files to Modify

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `font-kit` dependency |
| `src-tauri/src/commands/mod.rs` | Add `pub mod font;` |
| `src-tauri/src/commands/font.rs` | **New file** - `list_fonts` command with OnceLock cache |
| `src-tauri/src/lib.rs` | Register `list_fonts` in Tauri invoke handler |
| `src/settings/types.ts` | Add `FontListResponse` and `FontCategory` types |
| `src/settings/font-service.ts` | **New file** - Frontend font service with cache |
| `src/settings/settings-panel.ts` | Add `renderFontPickerInput`, `showFontPicker`, `hideFontPicker`, modify `renderAppearanceSection` |
| `src/styles/settings-panel.css` | Add font picker styles |
| `src/i18n/locales/en.json` | Add font picker i18n keys |
| `src/i18n/locales/ja.json` | Add font picker i18n keys |

---

## Test Scenarios

### Unit Tests (Rust)

- [ ] `list_fonts` returns non-empty `all_fonts` list
- [ ] `list_fonts` returns sorted results (case-insensitive alphabetical)
- [ ] `list_fonts` returns no duplicate entries
- [ ] `monospace_fonts` is a subset of `all_fonts`
- [ ] `emoji_fonts` contains only fonts with "emoji" in the name (case-insensitive)
- [ ] OnceLock cache returns same result on second call
- [ ] `list_fonts` succeeds even if no fonts are installed (returns empty lists)

### Unit Tests (TypeScript)

- [ ] `FontService.list()` calls `invoke("list_fonts")` on first call
- [ ] `FontService.list()` returns cached result on subsequent calls
- [ ] `filterFontList` with empty search returns all fonts
- [ ] `filterFontList` with search text filters case-insensitively
- [ ] `filterFontList` with non-matching text returns empty array
- [ ] `renderFontPickerInput` renders readonly input with current value
- [ ] `renderFontPickerInput` renders change button
- [ ] Change button click calls `showFontPicker`

### Integration Tests (TypeScript - Settings Panel)

- [ ] Font picker input displays current font name in readonly field
- [ ] Clicking "Change" transitions to font picker view
- [ ] Font picker view contains back button, search bar, and font list
- [ ] Back button restores settings view
- [ ] Navigation tabs are disabled during font picker view
- [ ] Selecting a font restores settings view with updated value
- [ ] Font picker search filters the list
- [ ] "No fonts found" shows when search has no matches
- [ ] Selected font item has `aria-selected="true"`
- [ ] Font list items have `role="option"`

### E2E Tests

- [ ] Open settings, click Change on Primary Font, see monospace font list
- [ ] Search for a font, select it, verify terminal updates
- [ ] Open settings, click Change on Secondary Font, see all fonts
- [ ] Open settings, click Change on Emoji Font, see emoji fonts
- [ ] Click back button, verify settings panel is restored

### Edge Cases

- [ ] Font list is empty (no fonts match category): "No fonts found" message
- [ ] Font name contains special CSS characters: proper escaping
- [ ] Very long font name: text truncation in list item
- [ ] Rapid clicking Change/Back: no UI corruption
- [ ] Category switch during font picker view: picker closes, new category shows

---

## Error Handling

### Error Cases

| Condition | Handling |
|-----------|----------|
| `font-kit` fails to enumerate fonts | Return empty lists, log warning |
| `font-kit` fails to load a specific font | Skip that font, continue enumeration |
| `invoke("list_fonts")` fails | Show font picker with empty list and error state |
| Font name not valid as CSS font-family | Use as-is; browser falls back to next in chain |

---

## Accessibility

### ARIA Roles and Properties

| Element | Role / Property |
|---------|-----------------|
| Font list container | `role="listbox"`, `aria-label="{title}"` |
| Font list item | `role="option"`, `aria-selected="true\|false"`, `tabindex="-1\|0"` |
| Back button | `aria-label="{t:fontPickerBack}"` |
| Search input | `aria-label="{t:fontPickerSearch}"` |
| Readonly font input | `readonly`, `aria-describedby` |

### Keyboard Navigation

| Key | Action |
|-----|--------|
| Arrow Down | Move focus to next font in list |
| Arrow Up | Move focus to previous font in list |
| Enter | Select focused font |
| Escape | Close font picker (return to settings) |
| Tab | Move focus between search, list, back button |

---

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] Font picker displays correct category of fonts for each field
- [ ] Each font is rendered in its own typeface
- [ ] Search filtering works correctly
- [ ] Font selection saves to settings and applies to terminal
- [ ] In-place transition is smooth and doesn't corrupt UI state
- [ ] ARIA roles and keyboard navigation are implemented
- [ ] i18n labels display correctly in English and Japanese
- [ ] Works on Linux, macOS, and Windows
- [ ] Rust tests pass
- [ ] TypeScript tests pass
- [ ] Code review is completed

## Open Questions

- [ ] Should virtual scrolling be implemented if font count exceeds a threshold (e.g., 1000)?
- [ ] Should font-kit version be pinned to a specific version or use a version range?
