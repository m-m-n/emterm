# Feature: Settings Item Description Texts

## Overview

Add description texts to each settings item in the Appearance and Terminal categories. Descriptions explain what happens when the user changes a setting, displayed as MD3 supporting text between the label and the input control.

## Objectives

- Provide clear, concise descriptions for all settings in Appearance and Terminal categories
- Follow Material Design 3 supporting text pattern
- Support bilingual display (English and Japanese) via the existing i18n system
- Maintain visual distinction from existing hint texts (range/format info)

## User Stories

### US1: Understand Setting Effect
As a user, I want to see a brief description of each setting, so that I know what will change when I modify it.

**Acceptance Criteria:**
- [ ] Each setting item in Appearance category shows a description below the label
- [ ] Each setting item in Terminal category shows a description below the label
- [ ] Descriptions explain the effect of changing the setting in one sentence
- [ ] Descriptions are displayed in the current locale language (en/ja)

## Technical Requirements

### Functional Requirements
- **FR1:** Add an optional `description` parameter to all render methods (`renderNumberInput`, `renderTextInput`, `renderSelect`, `renderToggle`, `renderSlider`)
- **FR2:** Render description text between label and input control
- **FR3:** Associate description with input via `aria-describedby`
- **FR4:** Add i18n keys for all description texts in both `en.json` and `ja.json`

### Non-Functional Requirements
- **NFR1 - Design:** Follow MD3 supporting text pattern (Body Small, on-surface-variant)
- **NFR2 - Accessibility:** Description text linked via `aria-describedby`
- **NFR3 - Backward Compatibility:** Description is optional; existing calls without description continue to work

## Implementation Approach

### Display Pattern

The description uses MD3's "supporting text" pattern. It is visually distinct from the existing "hint" text:

- **Description** (new): Explains what the setting does. Placed between label and input.
- **Hint** (existing): Shows range/format info. Placed below the input.

```
┌─────────────────────────────────────────────┐
│ Font Size                          ← label  │
│ Changes the text size in the terminal       │
│                                    ← desc   │
│ [  13  ] pt                        ← input  │
│ Range: 8-32pt                      ← hint   │
└─────────────────────────────────────────────┘
```

For toggle rows (horizontal layout with wrapper element):
```
┌─────────────────────────────────────────────┐
│ ┌─ label-group ──────────┐                  │
│ │ Inline Images          │         [toggle] │
│ │ Displays images inline │                  │
│ │ in the terminal        │                  │
│ └────────────────────────┘                  │
└─────────────────────────────────────────────┘
```

Toggle rows use `flex-direction: row` with `justify-content: space-between`. To support a description without breaking the two-column layout, the label and description are wrapped in a `.settings-toggle-label-group` container. This wrapper is the first flex child, and the toggle button is the second.

For keybind rows: No description (self-explanatory).

### CSS

New `.settings-description` class:

```css
/* Description - MD3 Body Small (supporting text) */
.settings-description {
  font-size: 12px;
  line-height: 16px;
  letter-spacing: 0.4px;
  color: var(--md-sys-color-on-surface-variant);
}

/* Toggle row label group wrapper */
.settings-toggle-label-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
```

The `.settings-description` class uses identical typographic properties to `.settings-hint` but is placed in a different position (above the input, not below). The `.settings-row` flex gap of 8px naturally spaces it.

The `.settings-toggle-label-group` class wraps the label and description inside toggle rows. It uses a vertical flex layout with 4px gap to stack the label above the description, while the parent `.settings-row-toggle` keeps the wrapper and toggle button in a horizontal row via `justify-content: space-between`.

### Render Method Changes

Each render method gains an optional `description?: string` parameter. When provided, a `<span class="settings-description">` element is inserted after the label element.

#### renderNumberInput

```typescript
private renderNumberInput(panel: HTMLElement, opts: {
  key: string;
  label: string;
  description?: string;  // NEW
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  hint: string;
  onInput: (value: number) => void;
  onSave: (value: number) => void;
}): void {
  const row = document.createElement("div");
  row.className = "settings-row";

  const label = document.createElement("label");
  label.className = "settings-label";
  label.htmlFor = `settings-${opts.key}`;
  label.textContent = opts.label;
  row.appendChild(label);

  // NEW: Description
  if (opts.description) {
    const desc = document.createElement("span");
    desc.className = "settings-description";
    desc.id = `settings-${opts.key}-desc`;
    desc.textContent = opts.description;
    row.appendChild(desc);
  }

  // ... rest unchanged, but add aria-describedby to input:
  // if (opts.description) {
  //   input.setAttribute("aria-describedby", `settings-${opts.key}-desc`);
  // }
}
```

The same pattern applies to all render methods:
- `renderTextInput`: Add description span after label, `aria-describedby` on input
- `renderSelect`: Add description span after label, `aria-describedby` on select
- `renderToggle`: When description is provided, create a wrapper `div.settings-toggle-label-group` containing the label and description span. The wrapper becomes the first flex child and the toggle button the second. When description is not provided, append label directly to row (backward compatible). `aria-describedby` on button.
- `renderSlider`: Add description span after label, `aria-describedby` on input

### i18n Keys

Key pattern: `settings.{category}.{settingKey}Desc`

#### English (`en.json`)

Add the following keys under `settings.appearance`:

```json
{
  "settings": {
    "language": {
      "labelDesc": "Changes the display language of the application"
    },
    "appearance": {
      "fontSizeDesc": "Changes the text size in the terminal",
      "fontFamilyDesc": "Sets the font used for terminal text",
      "lineHeightDesc": "Adjusts the vertical spacing between lines",
      "uiThemeDesc": "Switches the application color scheme between light and dark",
      "colorSchemeDesc": "Changes the color palette used for terminal text and background",
      "opacityDesc": "Sets the background transparency of the terminal window",
      "paddingDesc": "Adjusts the space between the terminal text and the window edge",
      "scrollbackLinesDesc": "Sets how many lines of output are kept in the scroll history",
      "showScrollbarDesc": "Controls when the scrollbar is visible",
      "inlineImagesDesc": "Displays images inline in the terminal output",
      "markdownRenderingDesc": "Renders Markdown content in the terminal output"
    },
    "terminal": {
      "cursorStyleDesc": "Changes the visual shape of the text cursor",
      "cursorBlinkDesc": "Makes the cursor blink on and off",
      "shellPathDesc": "Sets the shell program to launch in new tabs",
      "shellArgsDesc": "Sets the command-line arguments passed to the shell",
      "scrollSpeedDesc": "Adjusts how fast the terminal scrolls",
      "bellActionDesc": "Controls how the terminal notifies you on a bell character",
      "urlDetectionDesc": "Highlights clickable URLs in the terminal output",
      "copyOnSelectDesc": "Automatically copies text to clipboard when selected"
    }
  }
}
```

#### Japanese (`ja.json`)

Add the following keys under `settings.appearance`:

```json
{
  "settings": {
    "language": {
      "labelDesc": "アプリケーションの表示言語を変更します"
    },
    "appearance": {
      "fontSizeDesc": "ターミナル内のテキストサイズを変更します",
      "fontFamilyDesc": "ターミナルのテキストに使用するフォントを設定します",
      "lineHeightDesc": "行間の縦幅を調整します",
      "uiThemeDesc": "アプリケーションのカラースキームをライト・ダークで切り替えます",
      "colorSchemeDesc": "ターミナルのテキストと背景の配色を変更します",
      "opacityDesc": "ターミナルウィンドウの背景の透明度を設定します",
      "paddingDesc": "ターミナルのテキストとウィンドウ端の余白を調整します",
      "scrollbackLinesDesc": "スクロール履歴に保持する出力行数を設定します",
      "showScrollbarDesc": "スクロールバーの表示タイミングを制御します",
      "inlineImagesDesc": "ターミナル出力内に画像をインライン表示します",
      "markdownRenderingDesc": "ターミナル出力内のMarkdownコンテンツを描画します"
    },
    "terminal": {
      "cursorStyleDesc": "テキストカーソルの形状を変更します",
      "cursorBlinkDesc": "カーソルを点滅させます",
      "shellPathDesc": "新しいタブで起動するシェルプログラムを設定します",
      "shellArgsDesc": "シェルに渡すコマンドライン引数を設定します",
      "scrollSpeedDesc": "ターミナルのスクロール速度を調整します",
      "bellActionDesc": "ベル文字受信時の通知方法を制御します",
      "urlDetectionDesc": "ターミナル出力内のURLをクリック可能にハイライトします",
      "copyOnSelectDesc": "テキスト選択時に自動的にクリップボードにコピーします"
    }
  }
}
```

### Caller Site Changes

Each `render*` call in `renderAppearanceSection` and `renderTerminalSection` adds the `description` property. Example:

```typescript
// Font Size (number input)
this.renderNumberInput(panel, {
  key: "font-size",
  label: t("settings.appearance.fontSize"),
  description: t("settings.appearance.fontSizeDesc"),  // NEW
  value: this.currentSettings.font_size,
  min: MIN_FONT_SIZE,
  max: MAX_FONT_SIZE,
  step: 1,
  unit: "pt",
  hint: t("settings.appearance.fontSizeHint", { min: MIN_FONT_SIZE, max: MAX_FONT_SIZE }),
  onInput: (v) => applyFontSize(v),
  onSave: (v) => this.saveSetting("font_size", v),
});
```

### Complete Description Text Reference

| Category | Setting Key | i18n Key | English | Japanese |
|----------|------------|----------|---------|----------|
| Language | language | `settings.language.labelDesc` | Changes the display language of the application | アプリケーションの表示言語を変更します |
| Appearance | fontSize | `settings.appearance.fontSizeDesc` | Changes the text size in the terminal | ターミナル内のテキストサイズを変更します |
| Appearance | fontFamily | `settings.appearance.fontFamilyDesc` | Sets the font used for terminal text | ターミナルのテキストに使用するフォントを設定します |
| Appearance | lineHeight | `settings.appearance.lineHeightDesc` | Adjusts the vertical spacing between lines | 行間の縦幅を調整します |
| Appearance | uiTheme | `settings.appearance.uiThemeDesc` | Switches the application color scheme between light and dark | アプリケーションのカラースキームをライト・ダークで切り替えます |
| Appearance | colorScheme | `settings.appearance.colorSchemeDesc` | Changes the color palette used for terminal text and background | ターミナルのテキストと背景の配色を変更します |
| Appearance | opacity | `settings.appearance.opacityDesc` | Sets the background transparency of the terminal window | ターミナルウィンドウの背景の透明度を設定します |
| Appearance | padding | `settings.appearance.paddingDesc` | Adjusts the space between the terminal text and the window edge | ターミナルのテキストとウィンドウ端の余白を調整します |
| Appearance | scrollbackLines | `settings.appearance.scrollbackLinesDesc` | Sets how many lines of output are kept in the scroll history | スクロール履歴に保持する出力行数を設定します |
| Appearance | showScrollbar | `settings.appearance.showScrollbarDesc` | Controls when the scrollbar is visible | スクロールバーの表示タイミングを制御します |
| Appearance | inlineImages | `settings.appearance.inlineImagesDesc` | Displays images inline in the terminal output | ターミナル出力内に画像をインライン表示します |
| Appearance | markdownRendering | `settings.appearance.markdownRenderingDesc` | Renders Markdown content in the terminal output | ターミナル出力内のMarkdownコンテンツを描画します |
| Terminal | cursorStyle | `settings.terminal.cursorStyleDesc` | Changes the visual shape of the text cursor | テキストカーソルの形状を変更します |
| Terminal | cursorBlink | `settings.terminal.cursorBlinkDesc` | Makes the cursor blink on and off | カーソルを点滅させます |
| Terminal | shellPath | `settings.terminal.shellPathDesc` | Sets the shell program to launch in new tabs | 新しいタブで起動するシェルプログラムを設定します |
| Terminal | shellArgs | `settings.terminal.shellArgsDesc` | Sets the command-line arguments passed to the shell | シェルに渡すコマンドライン引数を設定します |
| Terminal | scrollSpeed | `settings.terminal.scrollSpeedDesc` | Adjusts how fast the terminal scrolls | ターミナルのスクロール速度を調整します |
| Terminal | bellAction | `settings.terminal.bellActionDesc` | Controls how the terminal notifies you on a bell character | ベル文字受信時の通知方法を制御します |
| Terminal | urlDetection | `settings.terminal.urlDetectionDesc` | Highlights clickable URLs in the terminal output | ターミナル出力内のURLをクリック可能にハイライトします |
| Terminal | copyOnSelect | `settings.terminal.copyOnSelectDesc` | Automatically copies text to clipboard when selected | テキスト選択時に自動的にクリップボードにコピーします |

### File Structure

Files to modify:

```
src/
├── i18n/locales/
│   ├── en.json                  # Add *Desc keys
│   └── ja.json                  # Add *Desc keys
├── settings/
│   └── settings-panel.ts        # Add description param to render methods, add description to all calls
└── styles/
    └── settings-panel.css       # Add .settings-description class
```

### Dependencies

**Internal Dependencies:**
- i18n system (`src/i18n/index.ts`): Uses `t()` function for description text lookup
- Settings panel (`src/settings/settings-panel.ts`): Render methods to be extended

**External Dependencies:**
- None

## Test Scenarios

### Unit Tests
- [ ] Each render method renders description span when `description` is provided
- [ ] Each render method omits description span when `description` is undefined
- [ ] `aria-describedby` attribute is set on input when description is provided
- [ ] `aria-describedby` attribute is absent when description is not provided

### Integration Tests
- [ ] Settings panel renders with description texts visible
- [ ] Language switch updates all description texts

### Visual Verification
- [ ] Description text appears between label and input for all Appearance settings
- [ ] Description text appears between label and input for all Terminal settings
- [ ] Description text uses correct MD3 Body Small styling
- [ ] Description text is visually distinct from hint text (different position)
- [ ] Toggle rows display description correctly within the horizontal layout

## Security Considerations

- **Input Validation:** Description texts are static i18n strings, no user input involved
- **XSS Prevention:** Text set via `textContent`, not `innerHTML`

## Error Handling

No new error cases. If a description key is missing from locale files, the `t()` function returns the key itself, which is acceptable degraded behavior.

## Success Criteria

- [ ] All 20 settings items (12 Appearance + 8 Terminal) display description texts
- [ ] Both English and Japanese descriptions are complete and accurate
- [ ] MD3 supporting text pattern is correctly implemented
- [ ] Existing hint texts remain unchanged and functional
- [ ] `aria-describedby` accessibility is implemented
- [ ] All existing tests continue to pass
- [ ] Visual appearance is consistent with MD3 guidelines

## Open Questions

### Future Improvement
- Consider extending `aria-describedby` to reference both description and hint text (space-separated IDs) for inputs that have both. Currently only description is referenced, which is an improvement over the current state (no `aria-describedby` at all).
