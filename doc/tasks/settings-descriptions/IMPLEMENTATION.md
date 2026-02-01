# Implementation Plan: Settings Item Description Texts

## Overview

Add description texts (MD3 supporting text) to all 20 settings items in the Appearance and Terminal categories, providing users with a brief explanation of what each setting does when changed.

## Objectives

- Add a `.settings-description` CSS class following MD3 Body Small specifications
- Add 20 description i18n keys to both `en.json` and `ja.json`
- Extend 5 render methods to accept an optional `description` parameter
- Wire up description texts to all 20 settings items via caller sites

## Prerequisites

### Development Environment
- Bun (package manager and test runner)
- TypeScript (type checking)

### Dependencies
- Existing i18n system (`src/i18n/index.ts` with `t()` function)
- Existing settings panel (`src/settings/settings-panel.ts`)
- Existing CSS variables for MD3 tokens (already defined in the project)

### Knowledge Requirements
- Material Design 3 supporting text pattern
- Existing render method structure in `settings-panel.ts`
- Project i18n key structure

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (Vanilla, no framework)
- **Styling**: CSS with MD3 design tokens (CSS custom properties)
- **i18n**: Custom i18n system with JSON locale files

### Design Approach

The feature adds an optional description layer to the existing settings row layout. Each render method gains an optional `description` parameter. When provided, a description span is inserted between the label and input control. The description is also linked to the input via `aria-describedby` for accessibility.

### Component Interaction

```
Caller sites (renderAppearanceSection, renderTerminalSection)
  │
  ├─ Call render methods with new `description` property
  │   │
  │   └─ Render methods create description span when provided
  │       │
  │       ├─ CSS class `.settings-description` styles the span
  │       └─ `aria-describedby` links span to input control
  │
  └─ i18n `t()` function resolves description text from locale files
```

## Implementation Phases

### Phase 1: CSS - Add Description Style

**Goal**: Define the `.settings-description` CSS class so description elements are styled correctly once rendered.

**Files to Modify**:
- `src/styles/settings-panel.css`:
  - Add `.settings-description` class after the existing `.settings-hint` rule

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `.settings-description` | Style description text per MD3 Body Small | MD3 design token CSS variables available | Description spans render with 12px font, 16px line-height, 0.4px letter-spacing, on-surface-variant color |

**Processing Flow**:
```
1. Add new CSS rule `.settings-description` in settings-panel.css
   └─ Place after `.settings-hint` rule block
```

**Implementation Steps**:

1. **Add `.settings-description` rule**
   - Define font-size, line-height, letter-spacing, and color properties
   - Uses the same typographic values as `.settings-hint` (MD3 Body Small)
   - Key considerations:
     - No `margin-top` needed (unlike `.settings-hint` which has `margin-top: 4px`) because `.settings-row` flex gap of 8px handles spacing naturally
     - The class is positionally distinct from `.settings-hint`: description appears above the input, hint appears below

2. **Add `.settings-toggle-label-group` rule**
   - Wraps the label and description inside toggle rows into a vertical flex container
   - Properties: `display: flex`, `flex-direction: column`, `gap: 4px`
   - This wrapper prevents the description from becoming a third flex child of the toggle row, which would break the `justify-content: space-between` two-column layout

**Dependencies**:
- Requires: None (CSS can be added independently)
- Blocks: Phase 3, Phase 4 (description elements need styling)

**Testing Approach**:

*Manual Testing*:
- [ ] Verify `.settings-description` class is syntactically valid CSS
- [ ] Verify the rule uses `var(--md-sys-color-on-surface-variant)` for color

**Acceptance Criteria**:
- [ ] `.settings-description` class is defined in `settings-panel.css`
- [ ] Properties match MD3 Body Small: 12px font-size, 16px line-height, 0.4px letter-spacing, on-surface-variant color

**Estimated Effort**: Small (< 1 day)

---

### Phase 2: i18n - Add Description Keys

**Goal**: Add all 20 `*Desc` i18n keys to both English and Japanese locale files so that description texts are resolvable via `t()`.

**Files to Modify**:
- `src/i18n/locales/en.json`:
  - Add `labelDesc` under `settings.language`
  - Add 11 `*Desc` keys under `settings.appearance`
  - Add 8 `*Desc` keys under `settings.terminal`
- `src/i18n/locales/ja.json`:
  - Add the same 20 keys with Japanese translations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `en.json` description keys | Provide English description texts | Valid JSON structure | 20 new `*Desc` keys accessible via `t()` |
| `ja.json` description keys | Provide Japanese description texts | Valid JSON structure | 20 new `*Desc` keys accessible via `t()` |

**Processing Flow**:
```
1. Add keys to en.json
   ├─ settings.language.labelDesc
   ├─ settings.appearance.{fontSizeDesc, fontFamilyDesc, lineHeightDesc, uiThemeDesc, colorSchemeDesc, opacityDesc, paddingDesc, scrollbackLinesDesc, showScrollbarDesc, inlineImagesDesc, markdownRenderingDesc}
   └─ settings.terminal.{cursorStyleDesc, cursorBlinkDesc, shellPathDesc, shellArgsDesc, scrollSpeedDesc, bellActionDesc, urlDetectionDesc, copyOnSelectDesc}
2. Add equivalent keys to ja.json with Japanese translations
```

**Implementation Steps**:

1. **Add English description keys to `en.json`**
   - Add `labelDesc` to `settings.language` section
   - Add 11 `*Desc` keys to `settings.appearance` section
   - Add 8 `*Desc` keys to `settings.terminal` section
   - Key considerations:
     - Each description explains what happens when the setting is changed
     - Descriptions are one sentence, concise
     - Refer to the complete text reference table in SPEC.md for exact wording

2. **Add Japanese description keys to `ja.json`**
   - Mirror the same key structure as `en.json`
   - Key considerations:
     - Japanese descriptions end with "...します" form
     - Refer to the complete text reference table in SPEC.md for exact wording

**Complete Key Reference**:

| i18n Key | English | Japanese |
|----------|---------|----------|
| `settings.language.labelDesc` | Changes the display language of the application | アプリケーションの表示言語を変更します |
| `settings.appearance.fontSizeDesc` | Changes the text size in the terminal | ターミナル内のテキストサイズを変更します |
| `settings.appearance.fontFamilyDesc` | Sets the font used for terminal text | ターミナルのテキストに使用するフォントを設定します |
| `settings.appearance.lineHeightDesc` | Adjusts the vertical spacing between lines | 行間の縦幅を調整します |
| `settings.appearance.uiThemeDesc` | Switches the application color scheme between light and dark | アプリケーションのカラースキームをライト・ダークで切り替えます |
| `settings.appearance.colorSchemeDesc` | Changes the color palette used for terminal text and background | ターミナルのテキストと背景の配色を変更します |
| `settings.appearance.opacityDesc` | Sets the background transparency of the terminal window | ターミナルウィンドウの背景の透明度を設定します |
| `settings.appearance.paddingDesc` | Adjusts the space between the terminal text and the window edge | ターミナルのテキストとウィンドウ端の余白を調整します |
| `settings.appearance.scrollbackLinesDesc` | Sets how many lines of output are kept in the scroll history | スクロール履歴に保持する出力行数を設定します |
| `settings.appearance.showScrollbarDesc` | Controls when the scrollbar is visible | スクロールバーの表示タイミングを制御します |
| `settings.appearance.inlineImagesDesc` | Displays images inline in the terminal output | ターミナル出力内に画像をインライン表示します |
| `settings.appearance.markdownRenderingDesc` | Renders Markdown content in the terminal output | ターミナル出力内のMarkdownコンテンツを描画します |
| `settings.terminal.cursorStyleDesc` | Changes the visual shape of the text cursor | テキストカーソルの形状を変更します |
| `settings.terminal.cursorBlinkDesc` | Makes the cursor blink on and off | カーソルを点滅させます |
| `settings.terminal.shellPathDesc` | Sets the shell program to launch in new tabs | 新しいタブで起動するシェルプログラムを設定します |
| `settings.terminal.shellArgsDesc` | Sets the command-line arguments passed to the shell | シェルに渡すコマンドライン引数を設定します |
| `settings.terminal.scrollSpeedDesc` | Adjusts how fast the terminal scrolls | ターミナルのスクロール速度を調整します |
| `settings.terminal.bellActionDesc` | Controls how the terminal notifies you on a bell character | ベル文字受信時の通知方法を制御します |
| `settings.terminal.urlDetectionDesc` | Highlights clickable URLs in the terminal output | ターミナル出力内のURLをクリック可能にハイライトします |
| `settings.terminal.copyOnSelectDesc` | Automatically copies text to clipboard when selected | テキスト選択時に自動的にクリップボードにコピーします |

**Dependencies**:
- Requires: None (i18n keys can be added independently)
- Blocks: Phase 4 (caller sites need keys to resolve)

**Testing Approach**:

*Automated*:
- Type check (`bun run typecheck`) verifies JSON is valid

*Manual Testing*:
- [ ] Verify `en.json` is valid JSON after changes
- [ ] Verify `ja.json` is valid JSON after changes
- [ ] Verify key count: 20 new `*Desc` keys in each file

**Acceptance Criteria**:
- [ ] 20 `*Desc` keys added to `en.json` (1 language + 11 appearance + 8 terminal)
- [ ] 20 `*Desc` keys added to `ja.json` with Japanese translations
- [ ] Both files remain valid JSON
- [ ] Key pattern follows `settings.{category}.{settingKey}Desc`

**Estimated Effort**: Small (< 1 day)

---

### Phase 3: Render Methods - Add Description Parameter

**Goal**: Extend the 5 render methods to accept an optional `description` parameter, render a description span when provided, and set `aria-describedby` on the input element.

**Files to Modify**:
- `src/settings/settings-panel.ts`:
  - Modify `renderNumberInput` method
  - Modify `renderTextInput` method
  - Modify `renderSelect` method
  - Modify `renderToggle` method
  - Modify `renderSlider` method

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `renderNumberInput` | Render number input with optional description | Accepts `opts` with optional `description` | Description span inserted after label when provided; `aria-describedby` set on input |
| `renderTextInput` | Render text input with optional description | Accepts `opts` with optional `description` | Description span inserted after label when provided; `aria-describedby` set on input |
| `renderSelect` | Render select dropdown with optional description | Accepts `opts` with optional `description` | Description span inserted after label when provided; `aria-describedby` set on select |
| `renderToggle` | Render toggle switch with optional description | Accepts `opts` with optional `description` | Description span inserted after label when provided; `aria-describedby` set on button |
| `renderSlider` | Render slider with optional description | Accepts `opts` with optional `description` | Description span inserted after label when provided; `aria-describedby` set on input |

**Processing Flow**:
```
For each render method:
1. Add optional `description?: string` to opts type
2. After creating and appending the label element
   ├─ If description is provided:
   │   ├─ Create span element with class "settings-description"
   │   ├─ Set span id to "settings-{key}-desc"
   │   ├─ Set span text content to description value
   │   └─ Append span to row (after label, before input)
   └─ If description is not provided:
       └─ No change (backward compatible)
3. On the input/select/button element
   ├─ If description is provided:
   │   └─ Set aria-describedby to "settings-{key}-desc"
   └─ If description is not provided:
       └─ No aria-describedby attribute
```

**Implementation Steps**:

1. **Extend `renderNumberInput`**
   - Add `description?: string` to opts type signature
   - Insert description span creation after label appending, before input group creation
   - Add `aria-describedby` on the number input element when description is provided
   - Key considerations:
     - Description span goes between label and inputGroup in the DOM order
     - The span uses `textContent` (not `innerHTML`) for XSS safety

2. **Extend `renderTextInput`**
   - Same pattern as renderNumberInput
   - Insert description span after label, before text input element
   - Add `aria-describedby` on the text input element

3. **Extend `renderSelect`**
   - Same pattern
   - Insert description span after label, before select element
   - Add `aria-describedby` on the select element

4. **Extend `renderToggle`**
   - Toggle rows use horizontal layout (`settings-row-toggle` with `flex-direction: row` and `justify-content: space-between`)
   - When description is provided:
     - Create a wrapper `div` with class `settings-toggle-label-group`
     - Append label to wrapper
     - Create description span (`settings-description` class, `settings-{key}-desc` id)
     - Append description span to wrapper
     - Append wrapper to row (instead of appending label directly to row)
   - When description is NOT provided:
     - Append label directly to row (backward compatible, no wrapper needed)
   - Add `aria-describedby` on the toggle button element when description is provided
   - Key considerations:
     - The wrapper element approach keeps the toggle row as a two-child flex layout (wrapper + toggle button), preserving the `space-between` alignment
     - The wrapper uses `flex-direction: column` with `gap: 4px` to stack label and description vertically

5. **Extend `renderSlider`**
   - Same pattern as renderNumberInput
   - Insert description span after label, before slider group
   - Add `aria-describedby` on the range input element

**Dependencies**:
- Requires: Phase 1 (CSS class must exist for styling)
- Blocks: Phase 4 (caller sites need the new parameter)

**Testing Approach**:

*Unit Tests*:
- Test each render method with `description` provided: verify description span is created with correct class, id, and text
- Test each render method without `description`: verify no description span is created
- Test `aria-describedby` attribute is present on input when description is provided
- Test `aria-describedby` attribute is absent when description is not provided

*Manual Testing*:
- [ ] Verify backward compatibility: existing calls without description still work

**Acceptance Criteria**:
- [ ] All 5 render methods accept optional `description` parameter
- [ ] Description span has class `settings-description` and id `settings-{key}-desc`
- [ ] Description text is set via `textContent`
- [ ] `aria-describedby` attribute references the description span id
- [ ] Omitting `description` produces identical output to current behavior
- [ ] Type check passes (`bun run typecheck`)

**Estimated Effort**: Small (< 1 day)

---

### Phase 4: Caller Sites - Wire Up Descriptions

**Goal**: Add `description: t("settings.*.{key}Desc")` to all render method calls in `renderAppearanceSection` and `renderTerminalSection`.

**Files to Modify**:
- `src/settings/settings-panel.ts`:
  - Modify all render calls in `renderAppearanceSection` (12 items)
  - Modify all render calls in `renderTerminalSection` (8 items)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `renderAppearanceSection` caller sites | Pass description text to each render call | i18n keys exist; render methods accept description | All 12 appearance settings display description text |
| `renderTerminalSection` caller sites | Pass description text to each render call | i18n keys exist; render methods accept description | All 8 terminal settings display description text |

**Processing Flow**:
```
1. For each render call in renderAppearanceSection (12 items):
   └─ Add description property with t() call using the corresponding *Desc key
2. For each render call in renderTerminalSection (8 items):
   └─ Add description property with t() call using the corresponding *Desc key
```

**Implementation Steps**:

1. **Add description to Appearance render calls**
   - Add `description: t("settings.language.labelDesc")` to language select
   - Add `description: t("settings.appearance.fontSizeDesc")` to font size number input
   - Add `description: t("settings.appearance.fontFamilyDesc")` to font family text input
   - Add `description: t("settings.appearance.lineHeightDesc")` to line height number input
   - Add `description: t("settings.appearance.uiThemeDesc")` to UI theme select
   - Add `description: t("settings.appearance.colorSchemeDesc")` to terminal color scheme select
   - Add `description: t("settings.appearance.opacityDesc")` to opacity slider
   - Add `description: t("settings.appearance.paddingDesc")` to padding number input
   - Add `description: t("settings.appearance.scrollbackLinesDesc")` to scrollback lines number input
   - Add `description: t("settings.appearance.showScrollbarDesc")` to show scrollbar select
   - Add `description: t("settings.appearance.inlineImagesDesc")` to inline images toggle
   - Add `description: t("settings.appearance.markdownRenderingDesc")` to markdown rendering toggle

2. **Add description to Terminal render calls**
   - Add `description: t("settings.terminal.cursorStyleDesc")` to cursor style select
   - Add `description: t("settings.terminal.cursorBlinkDesc")` to cursor blink toggle
   - Add `description: t("settings.terminal.shellPathDesc")` to shell path text input
   - Add `description: t("settings.terminal.shellArgsDesc")` to shell args text input
   - Add `description: t("settings.terminal.scrollSpeedDesc")` to scroll speed slider
   - Add `description: t("settings.terminal.bellActionDesc")` to bell action select
   - Add `description: t("settings.terminal.urlDetectionDesc")` to URL detection toggle
   - Add `description: t("settings.terminal.copyOnSelectDesc")` to copy on select toggle

**Dependencies**:
- Requires: Phase 2 (i18n keys must exist) and Phase 3 (render methods must accept description)
- Blocks: None (this is the final phase)

**Testing Approach**:

*Automated*:
- Type check passes (`bun run typecheck`)
- Existing tests pass (`bun test`)

*Manual Testing*:
- [ ] Open settings panel, verify all 12 Appearance descriptions are visible
- [ ] Switch to Terminal category, verify all 8 descriptions are visible
- [ ] Switch language from English to Japanese, verify descriptions update
- [ ] Switch language from Japanese to English, verify descriptions update
- [ ] Verify description text appears between label and input control
- [ ] Verify description text is visually distinct from hint text

**Acceptance Criteria**:
- [ ] All 12 Appearance settings display description text
- [ ] All 8 Terminal settings display description text
- [ ] Descriptions display correctly in both English and Japanese
- [ ] Language switching updates descriptions
- [ ] Existing tests continue to pass
- [ ] Type check passes

**Estimated Effort**: Small (< 1 day)

---

## Complete File Structure

```
src/
├── i18n/locales/
│   ├── en.json                  # Add 20 *Desc keys
│   └── ja.json                  # Add 20 *Desc keys
├── settings/
│   └── settings-panel.ts        # Modify 5 render methods + 20 caller sites
└── styles/
    └── settings-panel.css       # Add .settings-description class
```

**File Descriptions**:
- `settings-panel.css`: Add one new CSS rule (`.settings-description`) after the existing `.settings-hint` rule
- `en.json`: Add 20 new keys following the `*Desc` naming pattern with English description texts
- `ja.json`: Add 20 new keys following the `*Desc` naming pattern with Japanese description texts
- `settings-panel.ts`: Extend 5 render methods with optional `description` parameter; add `description` property to all 20 render calls

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner
- DOM testing via jsdom (or similar) for render method output verification

**Key Test Areas**:
1. **Render methods with description**: Verify description span is created, has correct class/id/text, and `aria-describedby` is set
2. **Render methods without description**: Verify no description span, no `aria-describedby` (backward compatibility)

### Manual Testing Checklist

- [ ] Appearance category: All 12 items show description text
- [ ] Terminal category: All 8 items show description text
- [ ] Description text positioned between label and input control
- [ ] Description text styled correctly (12px, on-surface-variant color)
- [ ] Description visually distinct from hint text (above input vs. below input)
- [ ] Toggle rows display description within horizontal layout correctly
- [ ] Language switch English to Japanese updates descriptions
- [ ] Language switch Japanese to English updates descriptions
- [ ] Existing hint texts unchanged and functional

## Dependencies

### External Dependencies

None. All dependencies are already in the project.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1 (CSS) - no dependencies
2. Phase 2 (i18n) - no dependencies (can be parallel with Phase 1)
3. Phase 3 (Render methods) - depends on Phase 1
4. Phase 4 (Caller sites) - depends on Phase 2 and Phase 3

**Parallel Execution**: Phases 1 and 2 can be implemented simultaneously.

## Risk Assessment

### Technical Risks

1. **Toggle Row Layout with Description**
   - **Risk**: Toggle rows use horizontal `flex-direction: row` layout; adding a description span as a third flex child would break `justify-content: space-between` alignment
   - **Likelihood**: Low (mitigated)
   - **Impact**: Medium (visual misalignment)
   - **Mitigation**: Resolved by wrapper element approach. Label and description are wrapped in `.settings-toggle-label-group` container, keeping the toggle row as a two-child flex layout (wrapper + toggle button). The wrapper uses `flex-direction: column` to stack label and description vertically.

### Implementation Risks

1. **JSON Syntax Errors**
   - **Risk**: Adding 20 keys to JSON files introduces risk of missing commas or brackets
   - **Likelihood**: Low
   - **Impact**: High (app fails to load locale)
   - **Mitigation**: Run `bun run typecheck` and validate JSON after editing

## Security Considerations

1. **XSS Prevention**: Description text must be set via `textContent`, not `innerHTML`. All description values come from static i18n strings, not user input.

## Open Questions

None. All requirements are fully specified in SPEC.md.

## Success Metrics

### Functional Completeness
- [ ] All 20 settings items display description texts
- [ ] Both English and Japanese descriptions are complete
- [ ] MD3 supporting text pattern correctly implemented

### Quality Metrics
- [ ] Type check passes (`bun run typecheck`)
- [ ] All existing tests pass (`bun test`)
- [ ] `aria-describedby` accessibility implemented on all 20 items

### User Experience
- [ ] Descriptions are concise and helpful
- [ ] Visual hierarchy is clear: label > description > input > hint
- [ ] Language switching works correctly

## References

- **Specification**: `doc/tasks/settings-descriptions/SPEC.md`
- **Requirements**: `doc/tasks/settings-descriptions/要件定義書.md`
- **Material Design 3 - Supporting Text**: Typography scale Body Small (12sp)

## Next Steps

1. Review this implementation plan
2. Begin with Phase 1 (CSS) and Phase 2 (i18n) in parallel
3. Proceed to Phase 3 (render methods)
4. Complete with Phase 4 (caller sites)
5. Run verification per `VERIFICATION.md`
