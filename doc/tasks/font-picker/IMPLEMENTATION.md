# Implementation Plan: Font Picker

## Overview

Replace manual text inputs for font family settings with a font picker UI that enumerates system fonts via the font-kit crate, displays them with preview rendering, and allows selection through an in-place transition within the settings panel.

## Objectives

- Enumerate system fonts from Rust backend using font-kit
- Classify fonts into monospace, all, and emoji categories
- Provide an in-place font picker UI with search and preview
- Replace text inputs with readonly input + "Change" button

## Prerequisites

### Development Environment
- Rust toolchain (1.85+)
- Bun (package manager)
- Docker (for testing)

### Dependencies
- `font-kit` crate (new dependency for Rust backend)

### Knowledge Requirements
- Tauri command registration pattern (existing `commands/mod.rs` and `lib.rs`)
- Settings panel architecture (`renderContent`, `contentElement`, `detachContentListeners`)
- Content listener management pattern (`addContentListener`, `detachContentListeners`)
- Existing settings applier pattern (`applyFontFamily`, `saveSetting`)
- i18n key structure (`src/i18n/locales/`)

## Architecture Overview

### Technology Stack
- **Backend**: Rust + font-kit (system font enumeration)
- **Frontend**: Vanilla TypeScript (font picker UI)
- **Styling**: CSS (Material Design 3 design system)

### Design Approach
- Backend enumerates and classifies system fonts once, caches result in `OnceLock`
- Frontend requests font list via Tauri `invoke("list_fonts")`, caches locally
- Font picker replaces the content area of settings panel in-place (not a modal/overlay)
- Navigation tabs remain visible but are disabled during font picker view
- Font selection triggers existing save and apply flow

### Component Interaction

```
Settings Panel
  |
  +-- "Change" button click
  |     |
  |     v
  +-- showFontPicker(category, currentValue, onSelect)
  |     |
  |     +-- FontService.list() --> invoke("list_fonts") --> Rust OnceLock cache
  |     |                                                      |
  |     |                    FontListResponse  <---------------+
  |     |
  |     +-- Replace contentElement with font picker UI
  |     +-- Disable navigation tabs
  |
  +-- User selects font
  |     |
  |     +-- onSelect(fontName) callback
  |     +-- hideFontPicker() --> restore settings view
  |     +-- applyFontFamily() + saveSetting()
  |
  +-- User clicks "Back" or presses Escape
        |
        +-- hideFontPicker() --> restore settings view (no changes)
```

## Implementation Phases

### Phase 1: Backend - Font Enumeration Command

**Goal**: Add `font-kit` dependency and create `list_fonts` Tauri command that returns categorized, sorted, deduplicated font family names with OnceLock caching.

**Files to Create**:
- `src-tauri/src/commands/font.rs` - Font enumeration command and caching logic

**Files to Modify**:
- `src-tauri/Cargo.toml` - Add `font-kit` dependency
- `src-tauri/src/commands/mod.rs` - Register `font` module
- `src-tauri/src/lib.rs` - Register `list_fonts` in Tauri invoke handler

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| FontListResponse | Data structure for categorized font lists | N/A | Serializable to JSON with three Vec<String> fields |
| FONT_CACHE (OnceLock) | One-time initialization cache for font enumeration | N/A | First call triggers enumeration; subsequent calls return cached data |
| list_fonts command | Tauri command entry point | Frontend invokes command | Returns FontListResponse (Ok) or error string (Err) |
| enumerate_fonts | Core enumeration and classification logic | SystemSource available | Returns FontListResponse with sorted, deduplicated lists |

**Processing Flow**:
```
1. Frontend invokes "list_fonts"
2. Check OnceLock cache
   +-- Cache hit -> return cloned cached data
   +-- Cache miss -> proceed to enumeration
3. Enumerate all font families from system source
4. For each font family:
   +-- Add to all_fonts list
   +-- Check if name contains "emoji" (case-insensitive) -> add to emoji_fonts
   +-- Load font and check is_monospace property -> add to monospace_fonts
   +-- If font fails to load -> skip monospace check, continue
5. Sort all three lists (case-insensitive alphabetical)
6. Deduplicate all three lists
7. Store in OnceLock and return
```

**Implementation Steps**:

1. **Add font-kit dependency to Cargo.toml**
   - Add `font-kit = "0.14"` to dependencies section

2. **Create font.rs with data structures and command**
   - Define `FontListResponse` struct (Serialize, Clone)
   - Define `FONT_CACHE` as `OnceLock<FontListResponse>`
   - Implement `list_fonts` Tauri command
   - Implement `enumerate_fonts` helper function
   - Key considerations:
     - Handle `all_families()` failure gracefully (return empty lists)
     - Handle individual font load failures (skip font, continue)
     - Sort case-insensitively, then dedup (note: `dedup()` uses exact string match, which is sufficient since font-kit returns consistent casing per family; if needed, use `dedup_by` for case-insensitive dedup)

3. **Register module and command**
   - Add `pub mod font;` to `commands/mod.rs`
   - Add `commands::font::list_fonts` to `generate_handler!` in `lib.rs`

**Dependencies**:
- Requires: Nothing (foundational phase)
- Blocks: Phase 3 (frontend needs backend command)

**Testing Approach**:

*Unit Tests (Rust)*:

| Scenario | Expected Result |
|----------|-----------------|
| `list_fonts` returns non-empty `all_fonts` | At least one system font exists |
| `list_fonts` returns sorted results | Case-insensitive alphabetical order |
| `list_fonts` returns no duplicate entries | No two identical family names |
| `monospace_fonts` is a subset of `all_fonts` | Every monospace font appears in all_fonts |
| `emoji_fonts` contains only fonts with "emoji" in name | Case-insensitive name check |
| Cache returns same result on second call | Pointer equality or value equality |
| `list_fonts` succeeds with empty system (returns empty lists) | No panic, empty lists returned |

*Note*: Tests that enumerate system fonts are integration-like (depend on system state). Consider both a direct unit test for `enumerate_fonts` and a cached test for `list_fonts`.

**Acceptance Criteria**:
- [ ] `font-kit` compiles on Linux, macOS, and Windows
- [ ] `list_fonts` command registered and callable from frontend
- [ ] Returns three categorized lists (monospace, all, emoji)
- [ ] Lists are sorted case-insensitively
- [ ] No duplicates in any list
- [ ] OnceLock cache works (second call does not re-enumerate)
- [ ] Graceful handling when font loading fails
- [ ] Rust tests pass

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: font-kit compilation on different platforms (requires system libraries)
  - **Mitigation**: font-kit is widely used and supports all major platforms; CI will catch platform issues
- **Risk**: Font enumeration is slow on systems with many fonts
  - **Mitigation**: OnceLock ensures enumeration happens only once; NFR1 allows up to 5 seconds

---

### Phase 2: Frontend Types and Font Service

**Goal**: Add TypeScript types for `FontListResponse` and `FontCategory`, and create `FontService` with frontend-side caching.

**Files to Create**:
- `src/settings/font-service.ts` - Font service with invoke and caching

**Files to Modify**:
- `src/settings/types.ts` - Add `FontListResponse` and `FontCategory` types

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| FontListResponse (TS) | Type definition matching Rust struct | N/A | Interface with three string array fields |
| FontCategory | Type alias for category discriminant | N/A | Union type: "primary" / "secondary" / "emoji" |
| FontService | Frontend font list provider with cache | Backend `list_fonts` command available | Returns cached FontListResponse |

**Processing Flow**:
```
1. FontService.list() called
   +-- Cached data exists -> return immediately
   +-- No cache -> invoke("list_fonts")
2. Store response in static cache
3. Return response
```

**Implementation Steps**:

1. **Add types to types.ts**
   - Add `FontListResponse` interface with `monospace_fonts`, `all_fonts`, `emoji_fonts` (all `string[]`)
   - Add `FontCategory` type alias: `"primary" | "secondary" | "emoji"`

2. **Create font-service.ts**
   - Static class with `list()` method
   - Private static cache field
   - Invokes `list_fonts` Tauri command on cache miss
   - Returns cached data on cache hit

**Dependencies**:
- Requires: Phase 1 (backend command must exist)
- Blocks: Phase 3 (font picker UI needs font data)

**Testing Approach**:

*Unit Tests (TypeScript)*:

| Scenario | Expected Result |
|----------|-----------------|
| `FontService.list()` calls `invoke("list_fonts")` on first call | Invoke called once |
| `FontService.list()` returns cached result on second call | Invoke not called again |

**Acceptance Criteria**:
- [ ] `FontListResponse` type matches Rust struct
- [ ] `FontCategory` type defined
- [ ] `FontService.list()` returns font data
- [ ] Cache prevents redundant backend calls
- [ ] TypeScript type check passes

**Estimated Effort**: Small (0.5 day)

---

### Phase 3: Settings Panel UI Changes and Font Picker

**Goal**: Replace font text inputs with readonly input + "Change" button, implement in-place font picker with search and preview, add CSS styles, and update i18n.

**Files to Create**: None (all changes in existing files)

**Files to Modify**:
- `src/settings/settings-panel.ts` - Add `renderFontPickerInput`, `showFontPicker`, `hideFontPicker`, `filterFontList`; modify `renderAppearanceSection`
- `src/styles/settings-panel.css` - Add font picker styles and font picker input styles
- `src/i18n/locales/en.json` - Add font picker i18n keys
- `src/i18n/locales/ja.json` - Add font picker i18n keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderFontPickerInput | Render readonly input + "Change" button for a font field | Settings loaded, currentSettings available | Readonly input shows current value; button triggers showFontPicker |
| showFontPicker | Replace content area with font picker view | FontService available, category known | Content area shows font picker; navigation disabled |
| hideFontPicker | Restore settings content area | Font picker is currently showing | Settings view restored; navigation re-enabled |
| filterFontList | Filter font names by search text | Font list loaded | Returns subset matching case-insensitive search |
| Font picker keyboard handler | Handle Arrow/Enter/Escape in font list | Font picker showing | Arrow keys move focus; Enter selects; Escape closes |

**Processing Flow - Font Picker Open**:
```
1. User clicks "Change" button on a font field
2. Detach current content listeners
3. Clear contentElement
4. Disable navigation tabs (add disabled class, set aria-disabled)
5. Load fonts via FontService.list()
6. Select font list based on category:
   +-- "primary" -> monospace_fonts
   +-- "secondary" -> all_fonts
   +-- "emoji" -> emoji_fonts
7. Render font picker UI into contentElement:
   +-- Header: back button + category title
   +-- Search bar: text input
   +-- Font list: scrollable list with each item in its own font-family
8. Mark currently selected font with aria-selected="true"
9. Attach new content listeners (search input, list click, keyboard)
```

**Processing Flow - Font Selection**:
```
1. User clicks a font item (or presses Enter on focused item)
2. Call onSelect callback with font name
3. Call hideFontPicker()
```

**Processing Flow - Font Picker Close (Back/Escape)**:
```
1. User clicks back button or presses Escape
2. Call hideFontPicker():
   a. Re-enable navigation tabs
   b. Detach font picker content listeners
   c. Call renderContent() to restore settings view
```

**Processing Flow - Search**:
```
1. User types in search input
2. filterFontList(searchText, fullFontList)
   +-- Empty search -> return full list
   +-- Non-empty -> filter by case-insensitive substring match
3. Re-render only the font list portion
4. If no matches -> show "No fonts found" message
```

**Implementation Steps**:

1. **Add i18n keys**
   - Add 7 new keys to `settings.appearance` section in both en.json and ja.json:
     - `fontPickerBack`, `fontPickerSearch`, `fontPickerNoResults`, `fontPickerChange`
     - `fontPickerPrimaryTitle`, `fontPickerSecondaryTitle`, `fontPickerEmojiTitle`

2. **Add CSS styles to settings-panel.css**
   - Font picker container (`.font-picker`) - flex column, full height
   - Header (`.font-picker-header`) - flex row with back button and title
   - Back button (`.font-picker-back`) - transparent button with hover state
   - Search input (`.font-picker-search-input`) - MD3 outlined text field
   - Font list (`.font-picker-list`) - scrollable list with flex:1
   - Font item (`.font-picker-item`) - hover and selected states
   - No results (`.font-picker-no-results`) - centered message
   - Font picker input row (`.settings-font-picker-group`) - flex row
   - Readonly input (`.settings-font-picker-input`) - surface-variant background
   - Change button (`.settings-font-picker-button`) - outlined button style

3. **Add renderFontPickerInput method to SettingsPanel**
   - Renders label, description, readonly input + "Change" button, hint
   - Key considerations:
     - Readonly input uses `type="text" readonly`
     - Input displays current font name (or placeholder if empty)
     - Button click triggers showFontPicker with appropriate category

4. **Replace three renderTextInput calls in renderAppearanceSection**
   - Primary Font: category "primary", onSelect updates font_family_primary
   - Secondary Font: category "secondary", onSelect updates font_family_secondary
   - Emoji Font: category "emoji", onSelect updates font_family_emoji
   - Key considerations:
     - Each onSelect callback follows same pattern: update currentSettings, applyCurrentFontFamily, saveSetting

5. **Implement showFontPicker method**
   - Detach current content listeners, clear content, disable nav tabs
   - Fetch fonts, select list by category, render font picker UI
   - Key considerations:
     - Each font item rendered with inline `font-family` style for preview
     - Search input gets `input` event listener for filtering
     - Font list container uses `role="listbox"`, items use `role="option"`
     - Currently selected font marked with `aria-selected="true"`

6. **Implement hideFontPicker method**
   - Re-enable navigation tabs, detach font picker listeners, restore content
   - Key considerations:
     - Must re-enable tab buttons (remove disabled class, remove aria-disabled)
     - Uses existing renderContent() to restore settings view

7. **Implement keyboard navigation**
   - Arrow Down/Up: move focus between font list items
   - Enter: select focused font
   - Escape: close font picker
   - Key considerations:
     - Manage tabindex on font items (focused item gets tabindex="0", others "-1")
     - Tab key moves between search input, list, and back button

**Dependencies**:
- Requires: Phase 1 (backend command), Phase 2 (FontService and types)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests (TypeScript)*:

| Scenario | Expected Result |
|----------|-----------------|
| filterFontList with empty search | Returns all fonts |
| filterFontList with search text | Returns case-insensitive matches |
| filterFontList with non-matching text | Returns empty array |
| renderFontPickerInput renders readonly input | Input element present with readonly attribute |
| renderFontPickerInput renders change button | Button element present with correct text |

*Integration Tests (TypeScript - Settings Panel)*:

| Scenario | Expected Result |
|----------|-----------------|
| Font picker input displays current font name | Readonly input value matches currentSettings |
| Clicking "Change" transitions to font picker view | Content area contains font picker elements |
| Font picker contains back button, search bar, font list | Three key elements present |
| Back button restores settings view | Content area returns to settings |
| Navigation tabs disabled during font picker | Tabs have disabled class and aria-disabled |
| Selecting a font restores settings with updated value | Settings view shows new font name |
| Search filters the list | Font items filtered by search text |
| "No fonts found" shows for no matches | No-results message visible |
| Font list items have `role="option"` | ARIA attributes present |
| Selected font has `aria-selected="true"` | Currently selected font highlighted |

*Manual Testing*:
- [ ] Open settings, click "Change" on Primary Font, see monospace font list
- [ ] Each font name rendered in its own typeface
- [ ] Search for a font, select it, verify terminal updates
- [ ] Click "Change" on Secondary Font, see all fonts
- [ ] Click "Change" on Emoji Font, see emoji fonts
- [ ] Click back button, verify settings panel restored
- [ ] Press Escape, verify settings panel restored
- [ ] Arrow keys navigate font list
- [ ] Enter selects focused font
- [ ] Navigation tabs disabled during font picker, re-enabled after

**Acceptance Criteria**:
- [ ] Three font fields show readonly input + "Change" button
- [ ] Font picker shows correct category of fonts for each field
- [ ] Each font rendered in its own typeface (CSS font-family preview)
- [ ] Search filtering works case-insensitively in real-time
- [ ] "No fonts found" message shown when search has no matches
- [ ] Font selection saves to settings and applies to terminal
- [ ] Back button and Escape close font picker without changes
- [ ] Navigation tabs disabled during picker, re-enabled after
- [ ] ARIA roles (listbox/option) and keyboard navigation implemented
- [ ] i18n labels display correctly in English and Japanese
- [ ] CSS styles follow MD3 design system

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:
- **Risk**: Large number of fonts (1000+) may cause slow rendering of font list
  - **Mitigation**: Spec notes this as an open question; initial implementation uses plain list. Virtual scrolling can be added later if needed.
- **Risk**: Some font names may not work as CSS font-family values
  - **Mitigation**: Use font name as-is; browser falls back to next font in chain
- **Risk**: Font preview may not render if font name doesn't match CSS font-family name
  - **Mitigation**: font-kit returns family names that generally match CSS expectations; fallback font handles mismatches

---

## Complete File Structure

```
src-tauri/
  Cargo.toml                          # + font-kit dependency
  src/commands/
    mod.rs                            # + pub mod font;
    font.rs                           # NEW: list_fonts command, OnceLock cache
  src/lib.rs                          # + commands::font::list_fonts in handler

src/settings/
  types.ts                            # + FontListResponse, FontCategory
  font-service.ts                     # NEW: FontService with caching
  settings-panel.ts                   # + renderFontPickerInput, showFontPicker,
                                      #   hideFontPicker, filterFontList;
                                      #   modified renderAppearanceSection

src/styles/
  settings-panel.css                  # + font picker styles

src/i18n/locales/
  en.json                             # + 7 font picker keys
  ja.json                             # + 7 font picker keys
```

## Testing Strategy

### Unit Testing

**Approach**:
- Bun test runner for TypeScript tests
- Cargo test for Rust tests
- Docker-based test execution (per project convention)

**Test Coverage Goals**:
- Font enumeration (Rust): Functional coverage (system-dependent)
- Font service (TypeScript): 100% (simple cache logic)
- filterFontList: 100% (pure function)
- Settings panel integration: Key user flows covered

**Key Test Areas**:

1. **Font Enumeration** (`src-tauri/src/commands/font.rs`)
   - Returns categorized lists
   - Lists are sorted and deduplicated
   - Monospace is subset of all_fonts
   - Emoji fonts match name heuristic
   - Cache works correctly

2. **Font Service** (`src/settings/font-service.ts`)
   - Invokes backend on first call
   - Returns cache on subsequent calls

3. **Filter Logic** (`src/settings/settings-panel.ts`)
   - Empty search returns all
   - Case-insensitive matching
   - No matches returns empty

4. **Settings Panel Integration** (`src/settings/settings-panel.test.ts`)
   - Font picker input rendering
   - Transition to/from font picker view
   - ARIA attributes

### Manual Testing Checklist

Based on spec test scenarios:

- [ ] Open settings, click Change on Primary Font, see monospace font list
- [ ] Search for a font, select it, verify terminal updates
- [ ] Open settings, click Change on Secondary Font, see all fonts
- [ ] Open settings, click Change on Emoji Font, see emoji fonts
- [ ] Click back button, verify settings panel is restored
- [ ] Font list is empty for a category: "No fonts found" message
- [ ] Very long font name: text display without layout breakage
- [ ] Rapid clicking Change/Back: no UI corruption

## Dependencies

### External Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| font-kit | 0.14 | System font enumeration and property detection |

### Internal Dependencies

**Implementation Order**:
1. Phase 1: Backend (no dependencies)
2. Phase 2: Frontend types and service (depends on Phase 1)
3. Phase 3: UI, CSS, i18n (depends on Phase 1 + Phase 2)

**Component Dependencies**:
- `font-service.ts` depends on `types.ts` (FontListResponse type)
- `settings-panel.ts` depends on `font-service.ts` (FontService.list)
- `settings-panel.ts` depends on `types.ts` (FontCategory type)
- `commands/font.rs` depends on `font-kit` crate
- `lib.rs` depends on `commands/font.rs` (command registration)

## Risk Assessment

### Technical Risks

1. **font-kit Platform Compilation**
   - **Risk**: font-kit requires platform-specific libraries (fontconfig on Linux, Core Text on macOS, DirectWrite on Windows)
   - **Likelihood**: Low (font-kit is mature and widely used)
   - **Impact**: High (blocks entire feature)
   - **Mitigation**: Test compilation on all CI platforms early

2. **Font List Performance**
   - **Risk**: Systems with many fonts (1000+) may experience slow list rendering
   - **Likelihood**: Medium
   - **Impact**: Medium (degraded UX but functional)
   - **Mitigation**: Start with plain list; virtual scrolling is a documented open question for future enhancement

3. **CSS Font-Family Name Mismatch**
   - **Risk**: font-kit family names may not match CSS font-family expectations
   - **Likelihood**: Low (most systems use consistent naming)
   - **Impact**: Low (preview won't show correct font, but selection still works)
   - **Mitigation**: Use font names as-is; browser handles fallback

### Implementation Risks

1. **Content Area Transition State**
   - **Risk**: In-place transition may leave state inconsistent if interrupted
   - **Likelihood**: Low
   - **Impact**: Medium (UI corruption)
   - **Mitigation**: hideFontPicker always restores state via renderContent(); cancel any active font picker on category switch

## Performance Considerations

1. **Font Enumeration**
   - OnceLock ensures single enumeration
   - NFR1: Must complete within 5 seconds on first call
   - Subsequent calls return cached data immediately

2. **Font Picker Rendering**
   - NFR2: Font picker view must render within 100ms
   - Each font item sets inline `font-family` style for preview

3. **Search Filtering**
   - NFR3: Must complete within 16ms per keystroke (60fps)
   - Simple string matching on cached array; re-render only list portion

## Security Considerations

1. **Font Name Handling**
   - Font names are set as CSS font-family values via inline style
   - No HTML injection risk since values are set via DOM `style.fontFamily` property, not innerHTML

## Open Questions

### From Specification
- [ ] Should virtual scrolling be implemented if font count exceeds a threshold (e.g., 1000)?
- [ ] Should font-kit version be pinned to a specific version or use a version range?

## Future Enhancements

Items NOT in the current specification (do not implement):
- Virtual scrolling for large font lists
- Font weight/style selection within a family
- Font file path display
- Custom font loading from file

## Success Metrics

### Functional Completeness
- [ ] All 9 functional requirements (FR1-FR9) implemented
- [ ] All user stories (US1-US4) acceptance criteria met
- [ ] All test scenarios pass

### Quality Metrics
- [ ] Rust tests pass
- [ ] TypeScript tests pass
- [ ] TypeScript type check passes
- [ ] No regressions in existing settings functionality

### Performance Metrics
- [ ] Font enumeration < 5 seconds (first call)
- [ ] Font picker renders < 100ms
- [ ] Search filtering < 16ms per keystroke

### User Experience
- [ ] ARIA roles and keyboard navigation functional
- [ ] i18n labels correct in English and Japanese
- [ ] Font preview renders each font in its own typeface

## References

- **Specification**: `doc/tasks/font-picker/SPEC.md`
- **Requirements**: `doc/tasks/font-picker/要件定義書.md`
- **Previous Implementation**: `doc/tasks/font-family-settings/IMPLEMENTATION.md`
- **font-kit Documentation**: https://docs.rs/font-kit

## Next Steps

1. `/sdd.3-verify-plan` で整合性検証と設計レビューを実行
2. 不明点を確認・解決してください
3. `/sdd.4-implement` で Phase 1 から順に実装を開始
