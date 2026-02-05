# Implementation Plan: Settings Category Reorganization

## Overview

Reorganize the settings panel from 3 categories (Appearance, Terminal, Keybinds) to 4 categories (UI Settings, Keybinds, Terminal Appearance, Terminal Behavior) and add a new UI font family setting.

## Objectives

- Restructure settings into 4 logical categories
- Add UI font family customization
- Maintain backward compatibility with existing settings files
- Provide i18n support (Japanese/English)

## Prerequisites

### Development Environment
- Node.js / Bun for TypeScript development
- Tauri development environment

### Dependencies
- No new external dependencies required

### Knowledge Requirements
- Understanding of existing settings-panel.ts architecture
- Familiarity with i18n system in src/i18n/

## Architecture Overview

### Technology Stack
- **Language**: TypeScript
- **Framework**: Tauri (frontend)
- **Styling**: CSS with MD3 design tokens

### Design Approach
- Modify category definitions in settings-panel.ts
- Split existing section renderers into new functions
- Add new ui_font_family setting with CSS variable application

### Component Interaction

```
settings-panel.ts (categories)
       ↓
settings-sections.ts (section renderers)
       ↓
settings-applier.ts (apply changes)
       ↓
CSS variables (visual update)
```

## Implementation Phases

### Phase 1: Type Definition & Backend Update

**Goal**: Add ui_font_family to settings type and ensure backend compatibility

**Files to Modify**:
- `src/settings/types.ts`: Add ui_font_family field
- `src-tauri/src/settings/types.rs`: Add ui_font_family field (Rust backend)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings interface | Define UI font family property | Type exists | New field added with string type |
| Rust AppSettings | Define matching field in backend | Struct exists | Field added with default value |

**Implementation Steps**:

1. **Add TypeScript type**
   - Add `ui_font_family: string` to AppSettings interface
   - Position after theme/color related fields for logical grouping
   - Extend `FontCategory` type: `"primary" | "secondary" | "emoji" | "ui"`

2. **Update Rust backend**
   - Add `ui_font_family` field to Rust struct
   - Set default value to "Roboto"
   - Ensure serde serialization compatibility

**Dependencies**:
- Requires: None
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests*:
- Verify TypeScript compilation succeeds
- Verify Rust build succeeds

**Acceptance Criteria**:
- [ ] TypeScript types compile without errors
- [ ] Rust backend compiles without errors
- [ ] Settings file with missing ui_font_family loads with default

**Estimated Effort**: 小 (1-2 hours)

---

### Phase 2: Category Reorganization

**Goal**: Change from 3 categories to 4 categories with proper navigation

**Files to Modify**:
- `src/settings/settings-panel.ts`: Update categories getter
- `src/settings/settings-sections.ts`: Create new section renderers, reorganize existing code

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| categories getter | Return 4 category definitions | Returns 3 categories | Returns 4 categories |
| renderUiSection | Render UI settings (language, theme, font) | Does not exist | Renders language, themes, UI font |
| renderTerminalAppearanceSection | Render terminal visual settings | Does not exist | Renders fonts, colors, layout |
| renderTerminalBehaviorSection | Render terminal behavior settings | Does not exist | Renders cursor, shell, scroll, etc. |

**Processing Flow**:
```
1. User opens settings
2. Navigation displays 4 categories (UI, Keybinds, Terminal Appearance, Terminal Behavior)
3. User selects category
   ├─ "ui" → renderUiSection()
   ├─ "keybinds" → renderKeybindsSection() (unchanged)
   ├─ "terminal-appearance" → renderTerminalAppearanceSection()
   └─ "terminal-behavior" → renderTerminalBehaviorSection()
4. Section content renders in right panel
```

**Implementation Steps**:

1. **Update categories getter**
   - Change array to include 4 categories with new IDs
   - Order: ui, keybinds, terminal-appearance, terminal-behavior
   - Update default activeCategory to "ui"

2. **Create renderUiSection**
   - Extract language, ui_theme, ui_theme_preset from renderAppearanceSection
   - Add placeholder for ui_font_family (implemented in Phase 3)

3. **Create renderTerminalAppearanceSection**
   - Move font settings, line_height, color scheme, padding, scrollbar settings
   - Rename from renderAppearanceSection content

4. **Create renderTerminalBehaviorSection**
   - Move cursor, shell, scroll, bell, url, copy settings
   - Rename from renderTerminalSection content

5. **Update renderContent switch**
   - Add cases for new category IDs

**Dependencies**:
- Requires: Phase 1
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests*:
- Verify categories getter returns 4 items
- Verify each category ID is unique

*Integration Tests*:
- Category switching works correctly
- Settings render in correct categories

**Acceptance Criteria**:
- [ ] 4 categories display in navigation
- [ ] Each category renders correct settings
- [ ] Keyboard navigation works across all categories
- [ ] No TypeScript compilation errors

**Estimated Effort**: 中 (3-4 hours)

---

### Phase 3: UI Font Setting & i18n

**Goal**: Add UI font family setting with immediate visual feedback and complete i18n

**Files to Modify**:
- `src/settings/settings-sections.ts`: Add UI font setting to renderUiSection
- `src/settings/settings-applier.ts`: Add applyUiFont function
- `src/styles/settings-panel.css`: Add CSS variable for UI font
- `src/i18n/locales/en.json`: Add translation keys
- `src/i18n/locales/ja.json`: Add translation keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| UI font picker | Allow user to select UI font | Does not exist | Font picker available in UI section |
| applyUiFont | Apply font to UI elements via CSS variable | Does not exist | CSS variable updated on change and on app load |
| applySettings | Apply all settings on load | Exists | Calls applyUiFont for initial font application |
| CSS variable | Control UI font rendering | Fixed font | Variable-based font |

**Processing Flow**:
```
1. User opens UI Settings
2. UI font picker shows current value
3. User selects new font
   ├─ Font picker opens (reuse existing font-picker)
   └─ User selects font
4. applyUiFont() called
   └─ Sets CSS variable on document root
5. UI immediately reflects new font
6. Setting saved to file
```

**Implementation Steps**:

1. **Add CSS variable support**
   - Add --ui-font-family CSS variable to settings-panel.css
   - Apply to .settings-panel font-family

2. **Create applyUiFont function**
   - Set CSS variable on document.documentElement
   - Handle empty value (use default)
   - Call from applySettings() for initial load application

3. **Add UI font picker to renderUiSection**
   - Reuse existing renderFontPickerInput component
   - Use "all" font category (not just monospace)
   - Connect to saveSetting and applyUiFont

4. **Add i18n keys**
   - Add category labels for all 4 categories
   - Add UI section title and font setting labels
   - Both English and Japanese

**Dependencies**:
- Requires: Phase 1, Phase 2
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- applyUiFont sets correct CSS variable

*Integration Tests*:
- Font change reflects immediately in UI
- Setting persists after reload

*E2E Testing (Docker)*:
- [ ] Change UI font and verify visual change
- [ ] Verify setting loads on restart

*Manual Testing (E2E Not Possible)*:
- [ ] Verify font rendering quality across different fonts

**Acceptance Criteria**:
- [ ] UI font setting appears in UI Settings category
- [ ] Font change applies immediately
- [ ] Setting persists after restart
- [ ] All i18n keys present in en.json and ja.json
- [ ] Category names display correctly in both languages

**Estimated Effort**: 中 (3-4 hours)

---

## Complete File Structure

```
src/settings/
├── settings-panel.ts       # Categories: 3 → 4
├── settings-sections.ts    # Sections: 2 → 4 + modifications
├── settings-applier.ts     # Add applyUiFont()
├── types.ts                # Add ui_font_family
├── settings-components.ts  # No changes
├── settings-service.ts     # No changes
├── keybind-editor.ts       # No changes
├── font-picker.ts          # No changes
└── color-scheme-editor.ts  # No changes

src/i18n/locales/
├── en.json                 # Add keys for 4 categories + UI font
└── ja.json                 # Add keys for 4 categories + UI font

src/styles/
└── settings-panel.css      # Add --ui-font-family variable

src-tauri/src/settings/
└── types.rs                # Add ui_font_family field
```

## Testing Strategy

### Unit Testing

**Test Coverage Goals**:
- Core logic: 80%+ coverage
- Type definitions: Compile-time verification

**Key Test Areas**:
1. **Settings Types** - TypeScript compilation
2. **Category Configuration** - Returns correct array
3. **UI Font Applier** - CSS variable manipulation

### Integration Testing

**Scenarios**:
1. Navigate through all 4 categories
2. Modify settings in each category
3. UI font changes reflect immediately

### E2E Testing (Docker)

- [ ] Open settings and verify 4 categories visible
- [ ] Switch between all categories
- [ ] Change UI font and verify CSS variable applied
- [ ] Save and reload - verify persistence

### Manual Testing (E2E Not Possible)

- [ ] Visual quality of different font choices
- [ ] Subjective UX evaluation of category organization

## Dependencies

### External Dependencies

None required.

### Internal Dependencies

**Implementation Order**:
1. Phase 1 (type definitions)
2. Phase 2 (category reorganization)
3. Phase 3 (UI font + i18n)

**Component Dependencies**:
- renderUiSection depends on applyUiFont
- applyUiFont depends on CSS variable definition
- All sections depend on type definitions

## Risk Assessment

### Technical Risks

1. **Rust/TypeScript Type Mismatch**
   - **Risk**: Types out of sync between frontend and backend
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: Test settings load/save cycle thoroughly

2. **Font Availability**
   - **Risk**: User-selected font not available
   - **Likelihood**: Low
   - **Impact**: Low
   - **Mitigation**: CSS fallback chain (Roboto, system-ui, sans-serif)

### Implementation Risks

1. **Breaking Existing Settings**
   - **Risk**: Existing user settings files fail to load
   - **Likelihood**: Low
   - **Impact**: High
   - **Mitigation**: Default value handling for new field

## Performance Considerations

1. **Category Switching**
   - No performance concerns (DOM is rebuilt per category anyway)

2. **Font Application**
   - CSS variable change is instant (no reflow needed beyond text)

## Security Considerations

1. **Font Name Sanitization**
   - Font names come from system font list (trusted source)
   - CSS font-family property is safe against injection

## Open Questions

None - all requirements clarified in specification phase.

## Future Enhancements

Not in current scope:
- UI font size customization (intentionally excluded per MD3 guidelines)
- Per-category font settings

## Success Metrics

### Functional Completeness
- [ ] All 4 categories implemented
- [ ] UI font setting works
- [ ] Backward compatible

### Quality Metrics
- [ ] All tests pass
- [ ] No TypeScript errors
- [ ] No Rust compilation errors

### User Experience
- [ ] Categories logically organized
- [ ] Settings easy to find
- [ ] Immediate visual feedback for UI font

## References

- **Specification**: `doc/tasks/settings-category-reorganization/SPEC.md`
- **Requirements**: `doc/tasks/settings-category-reorganization/要件定義書.md`
- **Existing Code**: `src/settings/settings-sections.ts`
- **Material Design 3**: Typography guidelines

## Next Steps

1. Review and approve this implementation plan
2. Begin Phase 1: Type definitions
3. Continue with Phase 2: Category reorganization
4. Complete with Phase 3: UI font + i18n
