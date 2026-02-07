# Implementation Plan: Markdown Viewer Color Theme Settings

## Overview

Add configurable color theme settings to the Markdown viewer with 8 color palettes (light/dark x 4 presets), a toggle to follow or detach from UI theme settings, and migrate missing element styles to `fullscreen.css` while removing dead code.

## Objectives

- Define 8 Markdown color palettes (light/dark x purple/blue/green/orange)
- Provide a toggle to follow or detach from the UI theme settings
- Apply the selected palette to `--markdown-*` CSS variables for fullscreen display
- Remove dead code: `theme.ts` file, `theme.test.ts` file, dead exports from `index.ts`
- Migrate missing element styles from `styles.css` to `fullscreen.css`
- Persist settings with full backward compatibility

## Prerequisites

### Development Environment

- Rust toolchain (for Tauri backend)
- Bun (package manager and bundler)
- Docker (for testing)

### Dependencies

- No new external dependencies required
- Uses existing `UiThemePreset` and `UiTheme` types

### Knowledge Requirements

- Existing settings pattern (Rust serde + TypeScript AppSettings + CSS variables)
- Settings UI rendering pattern (`settings-sections.ts`, `settings-components.ts`)
- Settings applier pattern (`settings-applier.ts`)
- CSS variable application for theme management

## Architecture Overview

### Technology Stack

- **Language**: Rust (backend), TypeScript (frontend)
- **Framework**: Tauri
- **Key Libraries**:
  - serde - Rust settings serialization/deserialization
  - CSS variables - Theme application

### Design Approach

The feature follows the existing settings pattern: Rust struct defines persistence, TypeScript interface mirrors it, CSS variables drive visual application. A new `markdown-theme-presets.ts` file defines the 8 color palettes. The settings applier resolves the effective theme/preset (follow UI or independent) and maps palette colors to `--markdown-*` CSS variables.

### Component Interaction

```
Settings UI (settings-sections.ts)
    |
    v
Settings Applier (settings-applier.ts)
    |
    v
Markdown Theme Presets (markdown-theme-presets.ts)
    |
    v
CSS Variables (--markdown-*)
    |
    v
Fullscreen View (fullscreen.css)
```

## Implementation Phases

### Phase 1: Data Layer and Color Palettes

**Goal**: Define the 8 Markdown color palettes and add settings fields to both Rust and TypeScript types.

**Files to Create**:

- `src/settings/markdown-theme-presets.ts` - 8 color palette definitions (11 colors each) and CSS variable mapping
- `src/settings/markdown-theme-presets.test.ts` - Tests for palette structure and color validity

**Files to Modify**:

- `src-tauri/src/commands/config.rs` - Add 3 new fields to AppSettings struct and Default impl
- `src/settings/types.ts` - Add 3 new fields to AppSettings interface

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| MarkdownThemeColors | Interface defining 11 color properties per palette | None | Type available for palette definitions |
| MarkdownPresetDefinition | Interface grouping dark and light variants | MarkdownThemeColors defined | Type available for preset map |
| MARKDOWN_THEME_PRESETS | Constant map from UiThemePreset to dark/light palettes | Both interfaces defined | 8 palettes accessible by preset + mode |
| MARKDOWN_COLOR_TO_CSS_VAR | Mapping from palette key to `--markdown-*` CSS variable name | None | CSS variable names centralized |
| AppSettings (Rust) | 3 new fields: markdown_theme_follow_ui (bool), markdown_theme (UiTheme), markdown_theme_preset (UiThemePreset) | Existing struct | New fields with defaults persisted |
| AppSettings (TS) | Mirror of Rust fields | Existing interface | TypeScript type safety |

**Processing Flow**:

```
1. Define MarkdownThemeColors interface (11 color properties)
2. Define MARKDOWN_COLOR_TO_CSS_VAR mapping (palette key -> --markdown-* CSS var)
3. Define MARKDOWN_THEME_PRESETS constant
   ├── For each preset (purple/blue/green/orange):
   │   ├── Derive dark palette from UI_THEME_PRESETS colors
   │   └── Derive light palette from UI_THEME_PRESETS colors
   └── Result: 4 presets x 2 modes = 8 palettes
4. Add Rust settings fields with serde defaults
5. Add TypeScript settings fields
```

**Implementation Steps**:

1. **Create markdown-theme-presets.ts**
   - Define `MarkdownThemeColors` interface with 11 color properties (bg, fg, heading, link, border, blockquote, codeBg, codeFg, preBg, tableBg, tableStripe)
   - Define `MARKDOWN_COLOR_TO_CSS_VAR` mapping object (e.g., `bg` -> `"--markdown-bg"`, `codeBg` -> `"--markdown-code-bg"`)
   - Define `MarkdownPresetDefinition` with dark/light variants
   - Define `MARKDOWN_THEME_PRESETS` constant with all 8 palettes
   - Color values derived from `UI_THEME_PRESETS` to maintain visual harmony
   - Key considerations:
     - bg: based on surface color
     - fg: based on onSurface color
     - heading: brighter variant of onSurface
     - link: based on primary color
     - border: based on outlineVariant
     - blockquote: based on onSurfaceVariant
     - codeBg: semi-transparent secondary container (inline code)
     - codeFg: same as fg
     - preBg: darker variant of surface (code block background)
     - tableBg: transparent or very subtle surface variant
     - tableStripe: semi-transparent secondary container (alternating rows)

2. **Add Rust settings fields to config.rs**
   - Add `markdown_theme_follow_ui` (bool, default: true, `deserialize_null_true`)
   - Add `markdown_theme` (UiTheme, default: System, `deserialize_null_default`)
   - Add `markdown_theme_preset` (UiThemePreset, default: Purple, `deserialize_null_default`)
   - Update `Default for AppSettings` impl

3. **Add TypeScript settings fields to types.ts**
   - Add `markdown_theme_follow_ui: boolean`
   - Add `markdown_theme: UiTheme`
   - Add `markdown_theme_preset: UiThemePreset`

**Dependencies**:

- Requires: None (foundational phase)
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests*:
- Each preset (purple/blue/green/orange) has both dark and light variants
- Each variant has all 11 required color properties
- All color values are valid CSS color strings (hex or rgba format)
- Rust: Default settings have correct values for new fields
- Rust: Missing fields in JSON use defaults
- Rust: Null fields in JSON use defaults
- Rust: Round-trip serialization preserves values
- Rust: Invalid enum values are rejected

*Manual Testing (E2E Not Possible)*:
- [ ] Visual inspection of palette harmony with UI theme presets

**Acceptance Criteria**:

- [ ] `MARKDOWN_THEME_PRESETS` constant exports 4 presets, each with dark and light variants
- [ ] Each palette has 11 color properties
- [ ] `MARKDOWN_COLOR_TO_CSS_VAR` maps all 11 keys to `--markdown-*` CSS variable names
- [ ] Rust AppSettings includes 3 new fields with correct defaults
- [ ] TypeScript AppSettings mirrors Rust struct
- [ ] All existing tests still pass
- [ ] New Rust and TypeScript tests pass

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:

- **Risk**: Color values don't look good visually
  - **Mitigation**: Derive from existing UI_THEME_PRESETS colors for consistency; can be tuned later

---

### Phase 2: Settings Applier and System Theme Integration

**Goal**: Implement the `applyMarkdownColorTheme()` function in the settings applier, integrate with UI theme changes and system theme media query listener.

**Files to Modify**:

- `src/settings/settings-applier.ts` - Add `applyMarkdownColorTheme()` function, call from `applySettings()`
- `src/settings/settings-applier.test.ts` - Add tests for new function

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| applyMarkdownColorTheme | Resolve effective theme/preset and apply 11 palette colors to CSS variables | MARKDOWN_THEME_PRESETS available | 11 `--markdown-*` color CSS variables set on :root |
| resolveTheme (internal) | Convert "system" theme to actual "light" or "dark" | Media query available | Concrete theme mode returned |
| System theme listener | Re-apply markdown colors on OS theme change when effective theme is "system" | Listener registered | Colors update on OS change |

**Processing Flow**:

```
1. Determine effective theme and preset
   ├── followUi = true → use uiTheme + uiPreset
   └── followUi = false → use mdTheme + mdPreset
2. Resolve "system" to actual light/dark
   └── Check prefers-color-scheme media query
3. Look up palette from MARKDOWN_THEME_PRESETS[effectivePreset][resolved]
4. Apply palette to --markdown-* CSS variables on :root
   └── Iterate MARKDOWN_COLOR_TO_CSS_VAR mapping to set each variable
5. If effective theme is "system"
   └── Register media query change listener to re-apply on OS theme change
6. Clean up previous listener if exists
```

**Implementation Steps**:

1. **Add applyMarkdownColorTheme() to settings-applier.ts**
   - Accept parameters: followUi, mdTheme, mdPreset, uiTheme, uiPreset
   - Resolve effective theme (follow UI or independent)
   - Handle "system" theme via media query check
   - Look up palette from presets
   - Apply each color to corresponding CSS variable using `MARKDOWN_COLOR_TO_CSS_VAR` mapping
   - Manage a dedicated media query listener for markdown system theme
   - Key considerations:
     - Separate listener from the existing UI theme listener
     - Clean up listener before registering new one

2. **Integrate into applySettings()**
   - Call applyMarkdownColorTheme() after applyUiTheme()
   - Pass all relevant settings values

3. **Add tests to settings-applier.test.ts**
   - Test followUi=true uses UI theme/preset
   - Test followUi=false uses markdown theme/preset
   - Test all 11 --markdown-* color CSS variables are set
   - Test system theme resolution

**Dependencies**:

- Requires: Phase 1 (MARKDOWN_THEME_PRESETS, MARKDOWN_COLOR_TO_CSS_VAR, settings fields)
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests*:
- `applyMarkdownColorTheme()` with followUi=true uses UI theme/preset values
- `applyMarkdownColorTheme()` with followUi=false uses markdown-specific values
- All 11 `--markdown-*` color CSS variables are set on document.documentElement
- System theme resolves correctly based on media query mock

*E2E Testing (Docker)*:
- [ ] TypeScript type check passes with new function signatures

**Acceptance Criteria**:

- [ ] `applyMarkdownColorTheme()` correctly resolves follow/independent mode
- [ ] CSS variables `--markdown-*` receive palette colors
- [ ] System theme listener registered when effective theme is "system"
- [ ] Previous listener cleaned up on re-apply
- [ ] `applySettings()` calls `applyMarkdownColorTheme()`
- [ ] All tests pass

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:

- **Risk**: Multiple media query listeners for system theme (UI + markdown) conflict
  - **Mitigation**: Use separate listener variable for markdown; independent lifecycle management

---

### Phase 3: Settings UI and i18n

**Goal**: Add color theme subsection to the Markdown Viewer settings section with toggle, conditional theme/preset selectors, and i18n keys. Integrate callbacks with settings applier.

**Files to Modify**:

- `src/settings/settings-sections.ts` - Add color theme subsection to `renderMarkdownViewerSection()`
- `src/i18n/locales/en.json` - Add i18n keys for color theme subsection
- `src/i18n/locales/ja.json` - Add i18n keys for color theme subsection

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Color Theme Subsection | Render toggle + conditional selectors in settings UI | Settings fields exist, applier ready | UI controls visible and functional |
| Toggle callback | Switch between follow/independent mode, re-render section | applyMarkdownColorTheme available | Theme applied, UI updated |
| Theme/Preset select callbacks | Apply new palette on change | Toggle OFF | CSS variables updated immediately |
| UI theme change integration | Re-apply markdown colors when UI theme/preset changes while follow mode is on | applyMarkdownColorTheme available | Markdown colors follow UI changes |

**Processing Flow**:

```
1. Render subsection header "Color Theme"
2. Render "Follow UI Theme" toggle
3. If toggle is OFF:
   ├── Render Theme selector (system/light/dark)
   └── Render Preset selector (purple/blue/green/orange)
4. Toggle change callback:
   ├── Save setting
   ├── Apply markdown color theme
   └── Re-render section (show/hide selectors)
5. Theme/Preset change callbacks:
   ├── Save setting
   └── Apply markdown color theme
6. UI theme/preset change callbacks (existing):
   └── If followUi is true → also apply markdown color theme
```

**Implementation Steps**:

1. **Add i18n keys to en.json and ja.json**
   - Keys under `settings.markdownViewer`: colorTheme, followUiTheme, followUiThemeDesc, theme, themeDesc, themeSystem, themeLight, themeDark, preset, presetDesc
   - Preset labels reuse existing `settings.appearance.preset*` keys

2. **Add Color Theme subsection to renderMarkdownViewerSection()**
   - Render subsection header with i18n key
   - Render toggle for `markdown_theme_follow_ui`
   - Conditionally render theme and preset selectors when toggle is OFF
   - Toggle callback: save, apply, re-render section
   - Theme/Preset callbacks: save, apply immediately
   - Key considerations:
     - Re-rendering the section when toggle changes (to show/hide selectors)
     - Passing all necessary parameters to applyMarkdownColorTheme

3. **Integrate UI theme/preset callbacks with markdown color theme**
   - In existing UI theme select callback: if followUi is true, also call applyMarkdownColorTheme
   - In existing UI preset select callback: if followUi is true, also call applyMarkdownColorTheme

**Dependencies**:

- Requires: Phase 1 (settings fields), Phase 2 (applier function)
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- Settings panel renders color theme subsection
- Toggle ON hides theme/preset selectors
- Toggle OFF shows theme/preset selectors

*E2E Testing (Docker)*:
- [ ] TypeScript type check passes
- [ ] Bun test passes

*Manual Testing (E2E Not Possible)*:
- [ ] Settings UI renders correctly with toggle ON/OFF
- [ ] Theme/preset changes apply immediately
- [ ] UI theme changes propagate to markdown when follow mode is on

**Acceptance Criteria**:

- [ ] Color theme subsection appears in Markdown Viewer settings
- [ ] Toggle ON hides theme/preset selectors
- [ ] Toggle OFF shows theme/preset selectors
- [ ] Changing toggle applies correct theme immediately
- [ ] UI theme/preset changes trigger markdown re-apply when followUi is true
- [ ] i18n keys work for both English and Japanese

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:

- **Risk**: Re-rendering section on toggle change may cause flickering
  - **Mitigation**: Use the existing `ctx.reRender()` pattern which is already used for language changes

---

### Phase 4: CSS Migration and Dead Code Removal

**Goal**: Migrate missing element styles to `fullscreen.css` using `--markdown-*` variables, and remove dead TypeScript code.

**Files to Modify**:

- `src/markdown/fullscreen.css` - Add migrated styles for elements not yet covered
- `src/markdown/index.ts` - Remove dead exports

**Files to Delete**:

- `src/markdown/theme.ts` - All dead code (replaced by `markdown-theme-presets.ts`)
- `src/markdown/theme.test.ts` - Tests for removed functions

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| fullscreen.css migration | Add styles for h3-h6, p, ul/ol/li, inline code, pre code, table striping, hr, img, strong/em | Palette CSS vars applied | All markdown elements styled |
| theme.ts cleanup | Remove dead functions and types | New preset system in use | File deleted |
| index.ts cleanup | Remove dead exports | theme.ts deleted | No import errors |

**Processing Flow**:

```
1. Audit fullscreen.css for missing element styles
2. Migrate missing styles from styles.css to fullscreen.css
   ├── Convert .markdown-content selectors to .markdown-fullscreen-content
   └── Use --markdown-* color variables
3. Delete theme.ts entirely
4. Delete theme.test.ts entirely
5. Update index.ts exports (remove dead theme.ts exports)
6. Verify no remaining references to deleted functions
```

**Implementation Steps**:

1. **Migrate styles to fullscreen.css**
   - Add styles for elements missing from fullscreen.css:
     - h3-h6 headings (font sizes)
     - Paragraphs (margins)
     - Lists (ul/ol/li with padding and margins)
     - Inline code (with `--markdown-code-bg`, `--markdown-code-fg`, `--markdown-code-font-family`)
     - pre > code reset (transparent bg, no padding)
     - Table styling (with `--markdown-table-bg`, `--markdown-table-stripe` for header bg and alternating rows)
     - Horizontal rule
     - Images (max-width, background)
     - Strong/em
     - Blockquote nested margins
     - Task list checkboxes
   - All styles use `.markdown-fullscreen-content` selector and `--markdown-*` variables
   - Note: `renderer.ts` outputs `<div class="markdown-content">` which nests inside `.markdown-fullscreen-content` in fullscreen mode. Since `.markdown-fullscreen-content` selectors are descendant selectors, styles cascade through the nested div correctly.

2. **Delete theme.ts and theme.test.ts**
   - Delete `src/markdown/theme.ts` entirely
   - Delete `src/markdown/theme.test.ts` entirely

3. **Update index.ts exports**
   - Remove `MarkdownTheme` type export
   - Remove `applyMarkdownTheme`, `generateMarkdownTheme`, `getDarkTheme`, `getLightTheme` exports

**Dependencies**:

- Requires: Phase 1, Phase 2 (new `--markdown-*` variable system must be working)
- Blocks: None (can be done in parallel with Phase 3 if careful)

**Testing Approach**:

*Unit Tests*:
- No imports of removed functions exist in the codebase
- `generateMarkdownTheme` and `applyMarkdownTheme` are not exported from `index.ts`

*E2E Testing (Docker)*:
- [ ] TypeScript type check passes (no broken imports)
- [ ] Bun test passes (no test failures from removed code)
- [ ] Rust tests pass

*Manual Testing (E2E Not Possible)*:
- [ ] Fullscreen markdown display renders all element types correctly
- [ ] No visual regressions in markdown rendering

**Acceptance Criteria**:

- [ ] All `.markdown-content` child element styles covered in `fullscreen.css` under `.markdown-fullscreen-content`
- [ ] `theme.ts` deleted
- [ ] `theme.test.ts` deleted
- [ ] `index.ts` exports updated (no dead exports)
- [ ] No broken imports in the codebase
- [ ] TypeScript type check passes

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:

- **Risk**: Missing styles during migration causes visual regressions
  - **Mitigation**: Systematic comparison of styles.css vs fullscreen.css element coverage before removal

---

## Complete File Structure

```
src/settings/
├── markdown-theme-presets.ts      # NEW: 8 markdown color palettes (11 colors each) + CSS var mapping
├── markdown-theme-presets.test.ts # NEW: tests for palette structure
├── settings-sections.ts           # MODIFIED: add color theme subsection
├── settings-applier.ts            # MODIFIED: add applyMarkdownColorTheme()
├── settings-applier.test.ts       # MODIFIED: add tests
├── types.ts                       # MODIFIED: add 3 fields to AppSettings
├── ui-theme-presets.ts            # UNCHANGED: reference for palette derivation

src/markdown/
├── fullscreen.css                 # MODIFIED: add migrated styles with --markdown-* vars
├── theme.ts                       # DELETED: dead code replaced by markdown-theme-presets.ts
├── theme.test.ts                  # DELETED: tests for removed functions
├── index.ts                       # MODIFIED: remove dead exports

src-tauri/src/commands/
├── config.rs                      # MODIFIED: add 3 fields to AppSettings

src/i18n/locales/
├── en.json                        # MODIFIED: add color theme i18n keys
├── ja.json                        # MODIFIED: add color theme i18n keys
```

**File Descriptions**:

- `markdown-theme-presets.ts`: Central definition of all 8 Markdown color palettes (11 colors each), `MarkdownThemeColors` interface, `MARKDOWN_COLOR_TO_CSS_VAR` mapping, and preset definitions.
- `settings-applier.ts`: `applyMarkdownColorTheme()` resolves follow/independent mode, resolves system theme, and applies the correct palette via `MARKDOWN_COLOR_TO_CSS_VAR`.
- `settings-sections.ts`: `renderMarkdownViewerSection()` gains a "Color Theme" subsection with toggle and conditional selectors.
- `config.rs`: `AppSettings` gains `markdown_theme_follow_ui`, `markdown_theme`, `markdown_theme_preset` with serde defaults.
- `types.ts`: TypeScript mirror of Rust struct additions.
- `fullscreen.css`: Complete set of markdown element styles using `--markdown-*` CSS variables.

## Testing Strategy

### Unit Testing

**Approach**:

- TypeScript: Bun test runner
- Rust: built-in `#[cfg(test)]` module
- Table-driven tests for palette validation
- Mock document.documentElement for CSS variable tests

**Test Coverage Goals**:

- Palette definitions: 100% coverage (all 8 palettes validated)
- Settings applier: 80%+ coverage (new function + integration)
- Rust config: 100% coverage (defaults, null, missing, round-trip)

**Key Test Areas**:

1. **Palette Definitions** (`markdown-theme-presets.test.ts`)
   - All 4 presets have both dark and light variants
   - All 11 color properties present in each variant
   - All color values are valid CSS color strings (hex or rgba format)

2. **Settings Applier** (`settings-applier.test.ts`)
   - Follow UI mode applies UI theme/preset
   - Independent mode applies markdown-specific theme/preset
   - System theme resolution via media query
   - All CSS variables set correctly

3. **Rust Settings** (`config.rs` tests)
   - Default values correct
   - Missing field handling
   - Null field handling
   - Round-trip serialization
   - Invalid enum rejection

### E2E Testing (Docker)

- [ ] TypeScript type check: `bun run typecheck`
- [ ] TypeScript tests: `bun test`
- [ ] Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] No broken imports after dead code removal

### Manual Testing (E2E Not Possible)

- [ ] Settings UI: toggle ON hides selectors, toggle OFF shows selectors
- [ ] Theme/preset changes apply immediately in fullscreen view
- [ ] UI theme changes propagate to markdown when follow mode is on
- [ ] OS theme changes propagate when "system" theme selected
- [ ] All markdown elements render correctly (headings, code, tables, lists, etc.)
- [ ] Settings persist across app restart

## Dependencies

### External Dependencies

No new external dependencies.

### Internal Dependencies

**Implementation Order** (respecting dependencies):

1. Phase 1 (no dependencies) - Data layer and palettes
2. Phase 2 (depends on Phase 1) - Settings applier
3. Phase 3 (depends on Phases 1 and 2) - Settings UI
4. Phase 4 (depends on Phase 2) - CSS cleanup and dead code removal

Note: Phase 3 and Phase 4 can be done in parallel once Phase 2 is complete.

**Component Dependencies**:

- `settings-applier.ts` depends on `markdown-theme-presets.ts`
- `settings-sections.ts` depends on `settings-applier.ts` (applyMarkdownColorTheme)
- `fullscreen.css` depends on `--markdown-*` CSS variables being set

## Risk Assessment

### Technical Risks

1. **CSS Migration Missing Styles**
   - **Risk**: Some element styles not migrated to `fullscreen.css`
   - **Likelihood**: Low
   - **Impact**: Medium (visual regression in markdown rendering)
   - **Mitigation**: Systematic element-by-element comparison before removal

2. **System Theme Listener Conflicts**
   - **Risk**: Multiple media query listeners (UI + markdown) interfere
   - **Likelihood**: Low
   - **Impact**: Low (incorrect theme until next interaction)
   - **Mitigation**: Separate listener variables with independent lifecycle

### Implementation Risks

1. **Settings UI Re-render on Toggle**
   - **Risk**: Flickering or state loss when re-rendering section
   - **Likelihood**: Low
   - **Impact**: Low (cosmetic)
   - **Mitigation**: Use existing `ctx.reRender()` pattern

## Performance Considerations

1. **Theme Switching**: CSS variable changes via `setProperty()` are instant (no DOM re-rendering needed)
2. **Media Query Listeners**: At most 2 listeners active (UI + markdown); negligible overhead
3. **Palette Lookup**: Direct object property access; O(1) performance

## Security Considerations

1. **Input Validation**: Theme and preset values constrained to enum variants by serde (Rust) and TypeScript types
2. **CSS Injection**: Color values are hardcoded constants, not user-provided

## Open Questions

None - all questions resolved during specification.

## Future Enhancements

None planned beyond current specification scope.

## Success Metrics

### Functional Completeness

- [ ] All 8 Markdown color palettes defined and visually coherent with UI presets
- [ ] Toggle ON/OFF works correctly in settings UI
- [ ] `--markdown-*` CSS variables receive palette colors
- [ ] System theme auto-switching works for Markdown viewer
- [ ] UI theme changes propagate to Markdown when follow mode is on
- [ ] All settings persisted and backward compatible

### Quality Metrics

- [ ] All test scenarios pass
- [ ] TypeScript type check passes
- [ ] Rust tests pass
- [ ] Dead code removed (`theme.ts`, `theme.test.ts`, dead exports)

### User Experience

- [ ] Settings UI is intuitive (toggle + conditional selectors)
- [ ] Theme changes are instant

## References

- **Specification**: `doc/tasks/markdown-color-theme/SPEC.md`
- **Requirements**: `doc/tasks/markdown-color-theme/要件定義書.md`
- **UI Theme Presets**: `src/settings/ui-theme-presets.ts`
- **Markdown Fullscreen**: `src/markdown/fullscreen.ts`
- **Settings Applier**: `src/settings/settings-applier.ts`
- **Rust Settings**: `src-tauri/src/commands/config.rs`
- **Fullscreen Display CSS**: `src/markdown/fullscreen.css`

## Next Steps

After reviewing this implementation plan:

1. `/sdd.3-verify-plan` で整合性検証と設計レビューを実行
2. 不明点を確認・解決してください
3. `/sdd.4-implement` で実装を開始してください
