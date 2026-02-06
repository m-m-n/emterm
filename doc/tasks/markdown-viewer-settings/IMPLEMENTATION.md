# Implementation Plan: Markdown Viewer Settings

## Overview

Add a "Markdown Viewer" settings category to the settings panel with three configurable options: body font family, code block font family, and body font size. Settings are persisted via Rust backend and applied to the Markdown fullscreen overlay via CSS variables.

## Objectives

- Add 3 new fields to Rust AppSettings and TypeScript AppSettings interface
- Add "Markdown Viewer" as the 5th settings navigation category
- Apply settings to Markdown fullscreen overlay via CSS variables
- Maintain backward compatibility with existing settings.json

## Prerequisites

### Development Environment
- Rust toolchain (for backend changes)
- Bun (for frontend build and test)
- Docker (for test execution per CLAUDE.md)

### Dependencies
- No new external dependencies required
- All UI components already exist (font picker, number input, subsection header)

### Knowledge Requirements
- Existing settings management pattern: serde(default) + deserialize_null + validate
- CSS variable application pattern in settings-applier.ts
- Section rendering pattern in settings-sections.ts

## Architecture Overview

### Technology Stack
- **Backend**: Rust (Tauri commands, serde serialization)
- **Frontend**: Vanilla TypeScript (settings UI, CSS variable application)
- **Styling**: CSS variables with fallback values

### Design Approach

Follow the established settings pattern exactly:
1. Rust: Add fields to AppSettings with serde defaults, null-safe deserialization, and validation
2. TypeScript types: Mirror Rust struct fields
3. Settings sections: Render UI controls using existing components
4. Settings applier: Set CSS variables on document root
5. CSS: Replace hardcoded values with `var()` references

### Component Interaction

```
AppSettings (Rust) ←→ settings.json ←→ AppSettings (TypeScript)
                                              ↓
                                    settings-panel.ts (category nav)
                                              ↓
                                    settings-sections.ts (render UI)
                                              ↓
                                    settings-applier.ts (CSS variables)
                                              ↓
                                    fullscreen.css / styles.css (var() usage)
```

## Implementation Phases

### Phase 1: Backend - Rust Settings Extension

**Goal**: Add three new fields to the Rust settings structure with defaults, null-safe deserialization, and validation. All existing Rust tests continue to pass.

**Files to Modify**:
- `src-tauri/src/commands/config.rs`:
  - Add validation constant `MIN_MARKDOWN_FONT_SIZE` / `MAX_MARKDOWN_FONT_SIZE` (reuse existing MIN/MAX_FONT_SIZE since same range)
  - Add default value functions for three new fields
  - Add `deserialize_null_with!` entries for `markdown_font_size`
  - Add three fields to `AppSettings` struct with serde annotations
  - Add three fields to `Default for AppSettings` impl
  - Add `markdown_font_size` validation to `validate_settings()`
  - Add tests for new fields (defaults, null, missing, validation, round-trip)
- `src-tauri/locales/en.json`: Add validation message for `markdownFontSize`
- `src-tauri/locales/ja.json`: Add validation message for `markdownFontSize`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `default_markdown_body_font_family()` | Return empty string default | None | Returns `""` |
| `default_markdown_code_font_family()` | Return empty string default | None | Returns `""` |
| `default_markdown_font_size()` | Return 14 as default | None | Returns `14` |
| `validate_settings()` (extension) | Validate markdown_font_size range | Settings loaded | Error if outside 8-32 |

**Processing Flow**:
```
1. Deserialize settings.json
   ├─ Field present with value → use value
   ├─ Field present with null → use default function
   └─ Field missing → use serde(default) function
2. Validate markdown_font_size
   ├─ In range [8, 32] → Ok
   └─ Out of range → Err with i18n message
```

**Implementation Steps**:

1. **Add default functions and null deserializers**
   - Three default functions following existing naming pattern
   - One `deserialize_null_with!` for markdown_font_size (u32 with custom default)
   - Two font family fields use `deserialize_null_default` (String with Default::default)

2. **Extend AppSettings struct and Default impl**
   - Three new fields with `#[serde(default = "...", deserialize_with = "...")]`
   - Place after existing Custom Color Schemes section as new "Markdown Viewer" section

3. **Extend validation**
   - Add markdown_font_size range check using i18n validation message

4. **Add i18n validation messages**
   - Add `markdownFontSize` key to both locale files

5. **Add comprehensive tests**
   - Default values, null handling, missing fields, validation boundaries, round-trip

**Dependencies**:
- Requires: Nothing (independent backend work)
- Blocks: Phase 2 (frontend types must match), Phase 3 (CSS needs backend to work)

**Testing Approach**:

*Unit Tests (Rust)*:
- Default value assertions for 3 new fields
- Null deserialization for 3 new fields
- Missing field deserialization (empty JSON still works)
- Validation: markdown_font_size boundaries (7=error, 8=ok, 32=ok, 33=error)
- Round-trip serialization/deserialization
- Existing tests remain unaffected

**Acceptance Criteria**:
- [ ] Three new fields exist in AppSettings with correct defaults
- [ ] Null values in JSON produce defaults
- [ ] Missing fields in JSON produce defaults
- [ ] markdown_font_size outside 8-32 fails validation
- [ ] Round-trip preserves all three values
- [ ] All existing tests still pass

**Estimated Effort**: 小

---

### Phase 2: Frontend - TypeScript Types and Settings UI

**Goal**: Add settings fields to TypeScript types, add "Markdown Viewer" navigation category, and render the settings section with font pickers and number input.

**Files to Modify**:
- `src/settings/types.ts`:
  - Add 3 new fields to `AppSettings` interface
  - Extend `FontCategory` type with `"markdown-body"` and `"markdown-code"`
- `src/settings/font-picker.ts`:
  - Add `"markdown-body"` and `"markdown-code"` to `titleMap` (Record<FontCategory, string>)
  - Add cases to font list switch: `"markdown-body"` → `all_fonts`, `"markdown-code"` → `monospace_fonts`
- `src/settings/settings-panel.ts`:
  - Add `markdown-viewer` to categories array
  - Add case in `renderContent()` switch
  - Import `renderMarkdownViewerSection`
- `src/settings/settings-sections.ts`:
  - Add `renderMarkdownViewerSection()` function
  - Import `applyMarkdownSettings` from settings-applier
- `src/i18n/locales/en.json`:
  - Add `settings.categories.markdownViewer` label
  - Add `settings.markdownViewer.*` labels for subsection, fields, and font picker titles
- `src/i18n/locales/ja.json`:
  - Add corresponding Japanese translations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `AppSettings` interface (extension) | Type definition for 3 new fields | None | Types match Rust struct |
| `FontCategory` type (extension) | Add "markdown-body" and "markdown-code" | None | New categories available for font picker |
| font-picker.ts (extension) | Map new categories to font lists and titles | FontCategory extended | Correct font lists shown for Markdown |
| categories array (extension) | Add 5th nav item | Settings panel renders | "Markdown Viewer" tab visible |
| `renderMarkdownViewerSection()` | Render font pickers and number input | Settings loaded, panel created | UI controls visible and interactive |

**Processing Flow**:
```
1. User selects "Markdown Viewer" category
2. renderMarkdownViewerSection() called with section context
3. Render subsection header "Font"
4. Render font picker for body font family
   └─ On select → saveSetting + applyMarkdownSettings
5. Render font picker for code font family
   └─ On select → saveSetting + applyMarkdownSettings
6. Render number input for font size (8-32, step 1, unit "pt")
   └─ On change → saveSetting + applyMarkdownSettings
```

**Implementation Steps**:

1. **Extend TypeScript AppSettings interface and FontCategory**
   - Add `markdown_body_font_family: string`
   - Add `markdown_code_font_family: string`
   - Add `markdown_font_size: number`
   - Extend `FontCategory` with `"markdown-body"` and `"markdown-code"`

2. **Extend font-picker.ts for new categories**
   - Add `"markdown-body"` and `"markdown-code"` to `titleMap` with i18n keys
   - Add switch cases: `"markdown-body"` → `all_fonts`, `"markdown-code"` → `monospace_fonts`

3. **Add i18n translation keys**
   - Category label: `settings.categories.markdownViewer`
   - Section labels: subsection header, field labels for both locales
   - Font picker titles: `settings.markdownViewer.fontPickerBodyTitle`, `settings.markdownViewer.fontPickerCodeTitle`

5. **Add category to settings panel**
   - Add 5th entry to `categories` getter
   - Add case `"markdown-viewer"` to switch in `renderContent()`

6. **Implement renderMarkdownViewerSection()**
   - Follow same pattern as existing section renderers
   - Use `renderSubsectionHeader` for "Font" subsection
   - Use `showFontPicker` for both font family fields (reuse existing font picker)
   - Use `renderNumberInput` for font size (min: 8, max: 32, step: 1, unit: "pt")
   - On change: call `saveSetting()` and apply CSS variables

**Dependencies**:
- Requires: Phase 1 (backend must accept new fields)
- Blocks: Phase 3 (CSS changes need applier function)

**Testing Approach**:

*Unit Tests (TypeScript)*:
- Verify renderMarkdownViewerSection creates expected DOM elements
- Verify applyMarkdownSettings sets CSS variables correctly (Phase 3)

**Acceptance Criteria**:
- [ ] "Markdown Viewer" appears as 5th category in settings navigation
- [ ] Category shows font pickers for body and code fonts
- [ ] Category shows number input for font size (8-32pt)
- [ ] Changing values triggers save and CSS application
- [ ] i18n labels display in both English and Japanese

**Estimated Effort**: 小

---

### Phase 3: CSS Variable Application and CSS Updates

**Goal**: Implement the CSS variable application function and replace hardcoded CSS values with CSS variable references. Settings changes apply immediately to the Markdown fullscreen overlay.

**Files to Modify**:
- `src/settings/settings-applier.ts`:
  - Add `applyMarkdownSettings()` function
  - Call it from `applySettings()`
- `src/styles.css`:
  - Replace hardcoded font-family/font-size in `.markdown-content` with CSS variable references
  - Replace hardcoded font-family in `.markdown-content code` with CSS variable reference
- `src/markdown/fullscreen.css`:
  - Replace hardcoded font-family/font-size in `.markdown-fullscreen-content` with CSS variable references

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `applyMarkdownSettings()` | Set 3 CSS variables on document root | Settings values available | CSS variables set on :root |
| CSS variable usage | `.markdown-fullscreen-content` and `.markdown-content` use `var()` | CSS variables defined | Fonts/size reflect settings |

**Processing Flow**:
```
1. applyMarkdownSettings(bodyFont, codeFont, fontSize) called
2. Set --markdown-body-font-family on root
   ├─ Non-empty value → set property with value
   └─ Empty value → remove property (CSS fallback used)
3. Set --markdown-code-font-family on root
   ├─ Non-empty value → set property with value
   └─ Empty value → remove property (CSS fallback used)
4. Set --markdown-body-font-size on root
   └─ Always set with "pt" suffix
```

**Implementation Steps**:

1. **Add applyMarkdownSettings() to settings-applier.ts**
   - Accept body font, code font, and font size parameters
   - Set/remove CSS variables on document root following applyUiFont pattern
   - For empty font strings, remove property so CSS fallback chain applies

2. **Wire into applySettings()**
   - Add call to applyMarkdownSettings with the three new settings fields

3. **Update CSS files to use variables**
   - `.markdown-fullscreen-content` font-family → `var(--markdown-body-font-family, ...fallback...)`
   - `.markdown-fullscreen-content` font-size → `var(--markdown-body-font-size, 14pt)`
   - `.markdown-content` font-family → `var(--markdown-body-font-family, ...fallback...)`
   - `.markdown-content` font-size → `var(--markdown-body-font-size, 14pt)`
   - `.markdown-content code` font-family → `var(--markdown-code-font-family, ...fallback...)`

**Dependencies**:
- Requires: Phase 2 (section renderer calls applyMarkdownSettings)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests*:
- applyMarkdownSettings sets correct CSS variables on document root
- Empty font string removes CSS variable property
- Non-empty font string sets CSS variable property

*Manual Testing*:
- Open Markdown overlay, verify font matches setting
- Change body font in settings, verify overlay updates
- Change code font in settings, verify code blocks update
- Change font size, verify proportional heading sizes

**Acceptance Criteria**:
- [ ] applyMarkdownSettings() sets/removes 3 CSS variables correctly
- [ ] applySettings() calls applyMarkdownSettings()
- [ ] CSS files use `var()` with appropriate fallbacks
- [ ] Default display matches current appearance (14pt, system fonts)
- [ ] Custom font/size applied immediately to Markdown overlay

**Estimated Effort**: 小

---

## Complete File Structure

```
src-tauri/
├── src/commands/config.rs         # +3 fields, +defaults, +null deser, +validation, +tests
├── locales/en.json                # +markdownFontSize validation message
└── locales/ja.json                # +markdownFontSize validation message

src/
├── settings/
│   ├── types.ts                   # +3 fields in AppSettings, +FontCategory extension
│   ├── font-picker.ts             # +titleMap entries, +switch cases for new categories
│   ├── settings-panel.ts          # +markdown-viewer category, +switch case
│   ├── settings-sections.ts       # +renderMarkdownViewerSection()
│   └── settings-applier.ts        # +applyMarkdownSettings(), +call in applySettings()
├── styles.css                     # Replace hardcoded values with CSS var()
├── markdown/
│   └── fullscreen.css             # Replace hardcoded values with CSS var()
└── i18n/locales/
    ├── en.json                    # +settings.categories.markdownViewer, +field labels, +picker titles
    └── ja.json                    # +Japanese translations
```

## Testing Strategy

### Unit Testing

**Rust (cargo test)**:
- Default values for 3 new fields
- Null deserialization for 3 new fields
- Missing field deserialization
- Validation boundary tests (7, 8, 32, 33 for markdown_font_size)
- Round-trip serialization
- Existing tests unaffected

**TypeScript (bun test)**:
- applyMarkdownSettings CSS variable behavior
- renderMarkdownViewerSection DOM output (if test infrastructure supports it)

### Manual Testing (E2E Not Possible)

Tauri WebView cannot be tested via chrome-devtools MCP:
- [ ] Open settings → "Markdown Viewer" category visible
- [ ] Font pickers open and allow font selection
- [ ] Number input allows 8-32pt range
- [ ] Display Markdown via `emterm markdown` command
- [ ] Verify body font matches setting in fullscreen overlay
- [ ] Verify code font matches setting in code blocks
- [ ] Verify font size matches setting
- [ ] Change settings → overlay updates immediately
- [ ] Restart app → settings persist

## Dependencies

### External Dependencies

None required.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Rust backend (independent)
2. Phase 2: TypeScript types and UI (depends on Phase 1)
3. Phase 3: CSS variable application (depends on Phase 2)

## Risk Assessment

### Technical Risks

1. **CSS variable fallback chain with font-family**
   - **Risk**: `var(--markdown-body-font-family, ...)` may not work correctly with multi-value font fallback chains in all browsers
   - **Likelihood**: Low (WebView is Chromium-based, well-supported)
   - **Mitigation**: Test with empty and non-empty font values; use same pattern as existing `--terminal-font-family`

2. **Font picker category for Markdown fonts**
   - **Risk**: Existing font picker categories (primary/secondary/emoji/ui) may not fit Markdown body/code use case
   - **Likelihood**: Low (font picker accepts any FontCategory)
   - **Mitigation**: Use "ui" category for body font (sans-serif), "primary" for code font (monospace), or add new categories if needed

## Security Considerations

- Font family strings applied via CSS variables (no innerHTML injection risk)
- Font size validated server-side (8-32 range)
- No new attack surface introduced

## Open Questions

None - all questions resolved during requirements gathering.

## References

- **Specification**: `doc/tasks/markdown-viewer-settings/SPEC.md`
- **Requirements**: `doc/tasks/markdown-viewer-settings/要件定義書.md`
- **Existing patterns**: `src-tauri/src/commands/config.rs` (settings management)
- **Existing patterns**: `src/settings/settings-applier.ts` (CSS variable application)
