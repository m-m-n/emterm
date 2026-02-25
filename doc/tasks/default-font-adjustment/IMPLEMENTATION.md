# Implementation Plan: Default Font Adjustment

## Overview

Replace hardcoded font names with cross-platform system font stacks across the entire application, add a clear button to font picker inputs, and introduce a markdown emoji font setting. This ensures optimal font rendering on all platforms without requiring specific font installations.

## Objectives

- Replace all hardcoded `"Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace` references with the system monospace font stack
- Update Markdown body CSS fallback to a serif font stack
- Add emoji font fallback to UI font stacks
- Add clear (reset) button to all font picker inputs
- Add `markdown_emoji_font_family` setting with full backend/frontend integration

## Prerequisites

### Development Environment
- Bun (package manager and test runner)
- Rust toolchain (for backend changes)
- Docker (for test execution)

### Dependencies
- No new external dependencies required
- Existing project dependencies are sufficient

## Architecture Overview

### Technology Stack
- **Backend**: Rust (Tauri) - settings persistence
- **Frontend**: Vanilla TypeScript - UI, settings, CSS
- **Testing**: Bun test (TypeScript), cargo test (Rust), Docker E2E

### Design Approach
- Define font stacks as constants for single-source management (NFR3)
- CSS `var()` fallback chains provide graceful degradation when user fonts are empty
- Font picker clear button resets value to empty string, allowing CSS fallback to activate
- Markdown emoji font is inserted into both body and code font chains

### Component Interaction

```
Settings UI (font picker)
  --> onSelect("") via clear button
  --> saveSetting() persists to backend
  --> applySettings() updates CSS variables
  --> CSS fallback chain activates system fonts
```

## Implementation Phases

### Phase 1: Font Stack Constants and Core Logic

**Goal**: Define system font stacks as constants and update `buildFontFamilyChain()` to use the monospace stack as its fallback instead of bare `monospace`. Update `DEFAULT_FONT_FAMILY` in config. This phase establishes the foundation all subsequent phases depend on.

**Files to Modify**:
- `src/terminal-app/config.ts` - Update `DEFAULT_FONT_FAMILY` to system monospace stack
- `src/settings/settings-applier.ts` - Define `SYSTEM_MONO_STACK` constant, update `buildFontFamilyChain()` fallback, update `applyFontFamily()` comparison
- `src/settings/settings-applier.test.ts` - Update test expectations for new fallback string

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `SYSTEM_MONO_STACK` | Single-source monospace font stack constant | N/A | Available for import throughout the app |
| `DEFAULT_FONT_FAMILY` | Default terminal font family | N/A | Uses system monospace stack |
| `buildFontFamilyChain()` | Build CSS font-family chain with system fallback | Accepts primary, emoji, secondary strings | Returns chain ending with system monospace stack |
| `applyFontFamily()` | Apply font chain to CSS variable | Receives three font strings | Removes CSS variable when chain equals system monospace stack only |

**Implementation Steps**:
1. **Define monospace stack constant** - Add `SYSTEM_MONO_STACK` in `settings-applier.ts` with the full system monospace font list (without emoji suffix, as emoji is handled by the emoji font field)
2. **Update `buildFontFamilyChain()`** - Replace `monospace` tail with `SYSTEM_MONO_STACK`
3. **Update `applyFontFamily()`** - Change the CSS variable removal condition to compare against `SYSTEM_MONO_STACK` instead of `"monospace"`
4. **Update `DEFAULT_FONT_FAMILY`** - Set to the system monospace stack string in `config.ts`
5. **Update unit tests** - Adjust all `buildFontFamilyChain` and `applyFontFamily` test expectations to match new fallback string

**Dependencies**: None (foundational phase)

**Testing Approach**:
- Unit: `buildFontFamilyChain("", "", "")` returns system monospace stack
- Unit: `buildFontFamilyChain("Fira Code", "", "")` prepends before system monospace stack
- Unit: `applyFontFamily("", "", "")` removes CSS variable (comparison against new stack)

**Acceptance Criteria**:
- [ ] `buildFontFamilyChain` uses system monospace stack as fallback
- [ ] `applyFontFamily` removes CSS variable when all inputs are empty
- [ ] All existing unit tests pass with updated expectations
- [ ] `DEFAULT_FONT_FAMILY` updated in config

**Estimated Effort**: small

---

### Phase 2: CSS Font Stack Replacements

**Goal**: Replace all hardcoded font names across CSS files and TypeScript template strings. Update Markdown body fallback to serif stack. Add emoji fallback to UI font stacks. After this phase, no `"Inconsolata"`, `"Noto Sans JP"`, or `"Noto Color Emoji"` references remain in production code.

**Files to Modify**:
- `src/styles.css` - Body font, markdown-content fallback, markdown-code fallback, markdown-fullscreen-content fallback, link-confirm-url, image-viewer-info
- `src/image-viewer/styles.css` - image-viewer-info font
- `src/image-viewer/index.ts` - STYLES constant (image-viewer-info font)
- `src/image-viewer/display-mode-styles.ts` - viewer-mode-button, viewer-mode-toggle font
- `src/shared/zoom-styles.ts` - viewer-zoom-level font
- `src/markdown/link-dialog.css` - link-confirm-url font
- `src/markdown/fullscreen.css` - fullscreen-content font (body: serif stack), code font (monospace stack)
- `src/clipboard/dialog.ts` - preview font (monospace stack), dialog container font (sans-serif with emoji)
- `src/styles/settings-panel.css` - settings-panel font (add emoji fallback)
- `src/styles/tab-bar.css` - tab font, settings-tab-content font (add emoji fallback)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Monospace locations | Use system monospace stack | Currently use hardcoded `"Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace` | Use `ui-monospace, SFMono-Regular, ...` stack with emoji suffix |
| Markdown body locations | Use serif font stack | Currently use sans-serif fallback | Use `ui-serif, Georgia, ...` stack with emoji suffix |
| Markdown code locations | Use monospace font stack | Currently use hardcoded fonts | Use system monospace stack with emoji suffix |
| UI locations | Add emoji fallback | Currently no emoji fonts in stack | Include emoji font suffix in sans-serif stack |

**Implementation Steps**:
1. **Replace monospace hardcoded fonts** - In all files listed, replace `"Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace` with the monospace font stack (including emoji suffix)
2. **Update Markdown body fallbacks** - In `styles.css` (`.markdown-content` and `.markdown-fullscreen-content`) and `fullscreen.css`, update `var()` fallback from sans-serif to serif stack with emoji suffix
3. **Update Markdown code fallbacks** - In `styles.css` (`.markdown-content code`) and `fullscreen.css` (`.markdown-fullscreen-content code`), update `var()` fallback to monospace stack with emoji suffix
4. **Add emoji to UI font stacks** - In `settings-panel.css` and `tab-bar.css`, append emoji font suffix to the existing sans-serif font stacks
5. **Update clipboard dialog** - Replace both the monospace portion (preview) and sans-serif portion (dialog container) with respective system stacks

**Dependencies**: Phase 1 (for constant definition reference, though CSS changes are string literals)

**Testing Approach**:
- E2E: Existing E2E tests pass without regression
- Manual: Visual verification of font rendering on Linux
- Grep audit: No remaining `"Inconsolata"` or `"Noto Sans JP"` in production source files

**Acceptance Criteria**:
- [ ] No hardcoded `"Inconsolata"`, `"Noto Sans JP"`, `"Noto Color Emoji"` in production code
- [ ] Markdown body uses serif fallback stack
- [ ] Markdown code uses monospace system stack
- [ ] UI font stacks include emoji fallback
- [ ] Type check passes

**Estimated Effort**: medium

---

### Phase 3: Font Picker Clear Button

**Goal**: Add a clear (x) button to all font picker inputs that resets the value to empty string. The button is visible only when the current value is non-empty.

**Files to Modify**:
- `src/settings/font-picker.ts` - Add clear button to `renderFontPickerInput()`
- `src/styles/settings-panel.css` - Add styles for `.settings-font-picker-clear`
- `src/i18n/locales/en.json` - Add `fontPickerClear` label
- `src/i18n/locales/ja.json` - Add `fontPickerClear` label

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Clear button element | Reset font value to empty string | Font picker input has a non-empty value | Value set to empty, input shows placeholder |
| Visibility toggle | Show/hide clear button | N/A | Button visible when value non-empty, hidden when empty |

**Processing Flow**:
1. `renderFontPickerInput()` creates clear button element between input and change button
   - Value non-empty -> button visible
   - Value empty -> button hidden
2. User clicks clear button
   - Call `opts.onSelect("")`
   - Update input display to empty
   - Hide clear button
3. User selects font from picker
   - Input shows font name
   - Clear button becomes visible

**Implementation Steps**:
1. **Add clear button element** - Create button with class `settings-font-picker-clear` and aria-label, insert between input and change button in the input group
2. **Implement visibility logic** - Show button when `opts.value` is non-empty, hide when empty
3. **Add click handler** - On click, call `opts.onSelect("")`, clear input value, hide button
4. **Add CSS styles** - Style the clear button to match MD3 design, compact size, appropriate hover/focus states
5. **Add i18n labels** - Add clear button label text in both en.json and ja.json

**Dependencies**: None (independent of font stack changes)

**Testing Approach**:
- Unit: Clear button calls onSelect with empty string
- Unit: Clear button hidden when value is already empty
- Manual: Visual verification of button layout and interaction

**Acceptance Criteria**:
- [ ] Clear button appears on all font picker inputs when value is non-empty
- [ ] Clicking clear resets value to empty string
- [ ] Clear button is hidden when value is empty
- [ ] Input shows placeholder text after clearing
- [ ] Keyboard accessible (focusable, activatable)

**Estimated Effort**: small

---

### Phase 4: Markdown Emoji Font Setting

**Goal**: Add `markdown_emoji_font_family` setting that allows users to specify an emoji font for the Markdown viewer. The emoji font is inserted into both body and code CSS variable chains.

**Files to Modify**:
- `src-tauri/src/commands/config.rs` - Add `markdown_emoji_font_family` field to `AppSettings`
- `src/settings/types.ts` - Add `markdown_emoji_font_family` to `AppSettings` interface
- `src/settings/settings-sections.ts` - Add emoji font picker to Markdown section
- `src/settings/settings-applier.ts` - Update `applyMarkdownSettings()` signature and logic
- `src/settings/settings-applier.test.ts` - Add/update tests for emoji parameter
- `src/i18n/locales/en.json` - Add emoji font picker labels for Markdown section
- `src/i18n/locales/ja.json` - Add emoji font picker labels for Markdown section
- `src/settings/font-picker.ts` - Add `"markdown-emoji"` to titleMap and font list switch

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `markdown_emoji_font_family` (Rust) | Persist emoji font setting | Default: empty string | Serializable, nullable with default |
| `markdown_emoji_font_family` (TS) | Type definition for frontend | N/A | Mirrors Rust struct |
| `applyMarkdownSettings()` | Apply markdown fonts including emoji | Receives body, code, emoji, size | Sets CSS variables with emoji in chain |
| Markdown emoji font picker | UI for selecting emoji font | Settings panel rendered | Emoji font picker between body and code pickers |

**Processing Flow**:
1. `applyMarkdownSettings()` receives body, code, emoji, and fontSize
2. Build body CSS value: user-body font, then user-emoji font, then serif stack (all non-empty parts)
3. Build code CSS value: user-code font, then user-emoji font, then monospace stack (all non-empty parts)
4. Set `--markdown-body-font-family` and `--markdown-code-font-family` CSS variables
   - If the built value equals just the default stack -> remove property (let CSS fallback handle it)
   - Otherwise -> set property to the built value

**Implementation Steps**:
1. **Add Rust setting field** - Add `markdown_emoji_font_family: String` with serde default empty string, null deserializer, and Default impl
2. **Update TypeScript type** - Add field to `AppSettings` interface
3. **Add font category** - Add `"markdown-emoji"` to `FontCategory` type, titleMap, and font list switch in font-picker
4. **Update `applyMarkdownSettings()`** - Add emoji parameter, build font chains that include emoji font between user font and default stack
5. **Update settings UI** - Add emoji font picker in Markdown section between body and code pickers
6. **Update all callers** - Update `applySettings()`, `renderMarkdownViewerSection()`, and test helper to pass emoji parameter
7. **Add i18n labels** - Add Markdown emoji font picker labels in both locale files

**Dependencies**: Phase 1 (for font stack constants), Phase 3 (for clear button on new picker)

**Testing Approach**:
- Unit: `applyMarkdownSettings()` with emoji inserts emoji into both body and code chains
- Unit: `applyMarkdownSettings()` with empty emoji omits it from chains
- Unit: Rust serde default, null handling, and round-trip for new field
- Integration: Settings save/load round-trip with emoji font
- Type check: TypeScript interface matches Rust struct

**Acceptance Criteria**:
- [ ] `markdown_emoji_font_family` persisted in settings
- [ ] Emoji font appears in both body and code CSS variable chains
- [ ] Font picker uses emoji font category (same list as terminal emoji)
- [ ] Empty emoji font is omitted from chains
- [ ] Existing tests updated and pass
- [ ] New Rust tests for serde and validation pass

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/commands/
  config.rs                      # + markdown_emoji_font_family field

src/terminal-app/
  config.ts                      # Update DEFAULT_FONT_FAMILY

src/settings/
  settings-applier.ts            # + SYSTEM_MONO_STACK constant, update buildFontFamilyChain(), applyFontFamily(), applyMarkdownSettings()
  settings-applier.test.ts       # Update existing tests, add new tests
  settings-sections.ts           # Add markdown emoji font picker
  types.ts                       # + markdown_emoji_font_family field, + "markdown-emoji" category
  font-picker.ts                 # + clear button, + "markdown-emoji" in titleMap/switch

src/
  styles.css                     # Replace hardcoded fonts (body, markdown-content, code, fullscreen-content, link-confirm-url, image-viewer-info)

src/styles/
  settings-panel.css             # Add emoji fallback to UI font stack
  tab-bar.css                    # Add emoji fallback to UI font stack

src/image-viewer/
  styles.css                     # Replace monospace font stack
  index.ts                       # Replace monospace font stack in STYLES constant
  display-mode-styles.ts         # Replace monospace font stack

src/shared/
  zoom-styles.ts                 # Replace monospace font stack

src/clipboard/
  dialog.ts                      # Replace both monospace and sans-serif font stacks

src/markdown/
  link-dialog.css                # Replace monospace font stack
  fullscreen.css                 # Replace body (serif) and code (monospace) font stacks

src/i18n/locales/
  en.json                        # + fontPickerClear, markdown emoji labels
  ja.json                        # + fontPickerClear, markdown emoji labels
```

## Testing Strategy

- **Unit**: Core logic (buildFontFamilyChain, applyFontFamily, applyMarkdownSettings) - update existing tests and add new ones
- **Rust unit**: Serde serialization, null handling, validation for new field
- **Type check**: `bun run typecheck` ensures TypeScript interface matches usage
- **E2E (Docker)**: Run existing E2E suite to verify no regression
- **Manual**: Visual font rendering verification on Linux
- **Grep audit**: Verify no hardcoded font names remain

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none) | - | No new dependencies required |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Canvas 2D rendering uses `DEFAULT_FONT_FAMILY` directly, may not resolve `ui-monospace` | Low | Medium | `ui-monospace` is a CSS generic; Canvas may fall back to next in chain. Verify with manual testing. |
| Existing user settings (non-empty fonts) break | Very Low | High | Non-empty user fonts are prepended before the system stack; behavior unchanged (NFR2) |
| Missing i18n keys cause runtime errors | Low | Medium | Add keys in both locale files; type system catches missing keys at build |

## Open Questions

- (none - all requirements are status: ok)

## Success Metrics

- [ ] All FR1-FR7 functional requirements implemented
- [ ] All unit and integration tests pass (TypeScript and Rust)
- [ ] No hardcoded "Inconsolata", "Noto Sans JP", "Noto Color Emoji" in production code
- [ ] Font rendering works correctly on Linux
- [ ] Existing E2E tests pass without regression
- [ ] Type check passes
