# Feature: Default Font Settings

## Overview

Configure the default font stack for eMterm terminal emulator to use Inconsolata as the primary monospace font, Noto Sans JP for Japanese characters, and Noto Color Emoji for emoji rendering, with a font size of 13pt.

## Objectives

- Set Inconsolata as the primary terminal font for better ASCII character readability
- Ensure consistent Japanese text rendering with Noto Sans JP
- Enable proper color emoji display with Noto Color Emoji
- Set font size to 13pt (approximately 17.33px) for optimal readability
- Maintain proper fallback chain for systems without these fonts installed

## User Stories

### US1: ASCII Character Display
As a developer, I want ASCII characters to be displayed in a clear, readable monospace font, so that I can efficiently work with code and terminal output.

**Acceptance Criteria:**
- [ ] ASCII characters render using Inconsolata font when installed
- [ ] Characters maintain equal width (monospace)
- [ ] Font falls back to system monospace if Inconsolata is unavailable

### US2: Japanese Text Display
As a Japanese-speaking developer, I want Japanese characters to display consistently, so that I can work with Japanese filenames, comments, and output.

**Acceptance Criteria:**
- [ ] Japanese hiragana, katakana, and kanji render using Noto Sans JP
- [ ] Japanese text aligns properly with ASCII text
- [ ] Font falls back appropriately if Noto Sans JP is unavailable

### US3: Emoji Display
As a user, I want emoji characters to display in color, so that I can see them as intended in terminal output.

**Acceptance Criteria:**
- [ ] Emoji characters render using Noto Color Emoji
- [ ] Emojis display in full color
- [ ] Font falls back appropriately if Noto Color Emoji is unavailable

## Technical Requirements

### Functional Requirements
- **FR1:** Update CSS `font-family` property to use the specified font stack
- **FR2:** Change `--terminal-font-size` CSS variable to 13pt (approximately 17.33px)
- **FR3:** Adjust `--terminal-line-height` CSS variable to 15pt to maintain proper line spacing
- **FR4:** Maintain the existing font-family in code blocks within Markdown content

### Non-Functional Requirements
- **NFR1 - Performance:** No degradation in text rendering performance
- **NFR2 - Compatibility:** Graceful fallback on systems without specified fonts
- **NFR3 - Maintainability:** Font settings centralized for future configuration file support

## Implementation Approach

### Architecture

The font configuration is managed through CSS, with the terminal container inheriting font settings from the body element.

```
┌─────────────────────────────────────┐
│           styles.css                │
├─────────────────────────────────────┤
│  :root {                            │
│    --terminal-font-size: 13pt;      │
│    --terminal-line-height: 15pt;    │
│  }                                  │
│                                     │
│  body {                             │
│    font-family: "Inconsolata", ...  │
│  }                                  │
└─────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────┐
│         #terminal                   │
│   (inherits font-family from body)  │
│   (uses CSS variables for size)     │
└─────────────────────────────────────┘
```

### CSS Changes

#### 1. Body Font Family (Line 15 in styles.css)

**Current:**
```css
body {
  font-family: "Menlo", "Monaco", "Courier New", monospace;
  color: #40ff40;
}
```

**New:**
```css
body {
  font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace;
  color: #40ff40;
}
```

#### 2. CSS Variables (Lines 19-22 in styles.css)

**Current:**
```css
:root {
  --terminal-font-size: 14px;
  --terminal-line-height: 16px;
}
```

**New:**
```css
:root {
  --terminal-font-size: 13pt;  /* approximately 17.33px */
  --terminal-line-height: 15pt;
}
```

#### 3. Markdown Code Blocks (Line 155 in styles.css)

**Current:**
```css
.markdown-content code {
  /* ... */
  font-family: "Menlo", "Monaco", "Courier New", monospace;
  /* ... */
}
```

**New:**
```css
.markdown-content code {
  /* ... */
  font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace;
  /* ... */
}
```

#### 4. Link Confirm URL Display (Line 467 in styles.css)

**Current:**
```css
.link-confirm-url {
  /* ... */
  font-family: monospace;
  /* ... */
}
```

**New:**
```css
.link-confirm-url {
  /* ... */
  font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace;
  /* ... */
}
```

#### 5. Image Viewer Info (Line 553 in styles.css)

**Current:**
```css
.image-viewer-info {
  /* ... */
  font-family: monospace;
  /* ... */
}
```

**New:**
```css
.image-viewer-info {
  /* ... */
  font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace;
  /* ... */
}
```

### Font Stack Explanation

| Priority | Font | Purpose |
|----------|------|---------|
| 1 | Inconsolata | Primary monospace font for ASCII characters |
| 2 | Noto Sans JP | Japanese characters (hiragana, katakana, kanji) |
| 3 | Noto Color Emoji | Color emoji rendering |
| 4 | monospace | System fallback for any missing characters |

### Files to Modify

```
src/
└── styles.css    # Font family and size configuration
```

### Specific Line Changes

| File | Line | Change |
|------|------|--------|
| src/styles.css | 15 | Update font-family in body selector |
| src/styles.css | 20 | Change --terminal-font-size: 14px to 13pt |
| src/styles.css | 21 | Change --terminal-line-height: 16px to 15pt |
| src/styles.css | 155 | Update font-family in .markdown-content code |
| src/styles.css | 467 | Update font-family in .link-confirm-url |
| src/styles.css | 553 | Update font-family in .image-viewer-info |

## Test Scenarios

### Visual Tests
- [ ] ASCII text displays correctly with Inconsolata
- [ ] Japanese text (ひらがな、カタカナ、漢字) displays correctly
- [ ] Emoji characters display in color
- [ ] Mixed text (ASCII + Japanese + Emoji) displays correctly
- [ ] Font size appears as 13pt (approximately 17.33px)
- [ ] Line spacing is appropriate at 15pt (no overlap, no excessive gaps)

### Fallback Tests
- [ ] Terminal displays correctly when Inconsolata is not installed
- [ ] Terminal displays correctly when Noto Sans JP is not installed
- [ ] Terminal displays correctly when Noto Color Emoji is not installed

### Regression Tests
- [ ] Markdown inline code uses the updated font
- [ ] IME composition view inherits correct font
- [ ] Existing terminal functionality is not affected

### Test Commands

```bash
# Display ASCII characters
echo "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
echo "abcdefghijklmnopqrstuvwxyz"
echo "0123456789"
echo '!"#$%&'\''()*+,-./:;<=>?@[\]^_`{|}~'

# Display Japanese characters
echo "あいうえお かきくけこ"
echo "アイウエオ カキクケコ"
echo "日本語表示テスト 漢字"

# Display emoji
echo "Hello 🎉 World 🌍 Test 💻"

# Mixed content
echo "Hello 世界 🌍"
echo "ファイル名: test.txt 📄"
```

## Success Criteria

- [ ] All font-family properties updated to use Inconsolata, Noto Sans JP, Noto Color Emoji
- [ ] Font size changed to 13pt (approximately 17.33px)
- [ ] Line height changed to 15pt
- [ ] All visual tests pass
- [ ] All fallback tests pass
- [ ] All regression tests pass
- [ ] No performance degradation observed

## Future Considerations

- Configuration file support for user-customizable fonts
- Font weight customization
- Support for additional font fallbacks
- Per-character-class font selection (more granular control)

## References

- Requirements Document: `doc/tasks/default-font-settings/要件定義書.md`
- Current CSS: `src/styles.css`
- Inconsolata: https://fonts.google.com/specimen/Inconsolata
- Noto Sans JP: https://fonts.google.com/specimen/Noto+Sans+JP
- Noto Color Emoji: https://fonts.google.com/noto/specimen/Noto+Color+Emoji
