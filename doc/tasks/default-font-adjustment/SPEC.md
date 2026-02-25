# Feature: Default Font Adjustment

## Overview

Update eMterm's default font configuration to use simple CSS generic font families instead of hardcoded font names. Add a clear button to font picker inputs so users can reset font settings to their default state. Fix terminal resize behavior when font settings change.

## Objectives

- Replace hardcoded font names (Inconsolata, Noto Sans JP, etc.) with simple generic font families (`monospace`, `serif`, `sans-serif`) plus emoji fallback
- Add clear (reset) functionality to all font picker inputs
- When a user sets a specific font, use ONLY that font (no generic family appended)
- Recalculate terminal dimensions (cols/rows) and notify PTY when font changes

## User Stories

### US1: Default Font Experience
As a user, I want the terminal to use the browser/OS default monospace font without specifying individual font names, so that text renders well without manual configuration.

**Acceptance Criteria:**
- [ ] Terminal uses `monospace` generic family by default
- [ ] Markdown body uses `serif` generic family by default
- [ ] Markdown code uses `monospace` generic family by default
- [ ] UI elements use `sans-serif` generic family by default
- [ ] All contexts include emoji font fallback
- [ ] No specific font installation is required for a good default experience

### US2: Reset Font to Default
As a user who customized fonts, I want to easily reset any font setting back to the default, so that I can undo my changes.

**Acceptance Criteria:**
- [ ] Each font picker input has a clear (x) button when a value is set
- [ ] Clicking the clear button resets the font to empty string (default)
- [ ] The clear button is hidden when the value is already empty
- [ ] The input shows the placeholder text after clearing

### US3: Custom Font Without Fallback
As a user who sets a specific font (e.g., "Inconsolata"), I want only that font to be used, so that the rendering matches my expectation exactly.

**Acceptance Criteria:**
- [ ] When user sets primary font "Inconsolata", the renderer receives only "Inconsolata"
- [ ] No generic family (monospace, serif, etc.) is appended to the user's font
- [ ] When the user clears the setting, the renderer falls back to `monospace`

### US4: Terminal Resize on Font Change
As a user who changes the terminal font, I want the terminal to automatically recalculate the number of columns and rows, so that the display is correct after the change.

**Acceptance Criteria:**
- [ ] Changing font family recalculates cols/rows and resizes PTY
- [ ] Changing font size recalculates cols/rows and resizes PTY
- [ ] Changing from a wide font to a narrow font increases the column count
- [ ] The ResizeObserver is reconnected with the new character dimensions

## Technical Requirements

### Functional Requirements

- **FR1: Simple generic font defaults** - Use `monospace` as the terminal default font family (via `DEFAULT_FONT_FAMILY` constant). No system-specific font stack.
- **FR2: Font picker clear button** - Add an x button to all font picker inputs that resets the value to empty string. Button is visible only when value is non-empty.
- **FR3: Hardcoded font replacement** - Replace all instances of `"Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace` with `monospace` plus emoji fallback
- **FR4: Markdown body font default** - CSS fallback for Markdown body is `serif` plus emoji fallback
- **FR5: Markdown code font default** - CSS fallback for Markdown code is `monospace` plus emoji fallback
- **FR6: UI font emoji support** - Add emoji font fallback to UI font stack (`sans-serif` based)
- **FR7: Markdown emoji font setting** - Add `markdown_emoji_font_family` setting for Markdown viewer. Applied as a separate CSS variable `--markdown-emoji-font-family`.
- **FR8: User-only font chain** - `buildFontFamilyChain()` returns only user-specified fonts. No generic family appended. Returns empty string when no fonts configured.
- **FR9: PTY resize on font change** - When font family or font size changes, recalculate terminal cols/rows from new character dimensions and resize state, renderer, selection, and PTY.

### Non-Functional Requirements

- **NFR1 - Compatibility:** Font rendering must work correctly on macOS, Windows, and Linux without requiring specific font installations
- **NFR2 - Backward Compatibility:** Existing user font settings (non-empty values) must continue to work unchanged
- **NFR3 - Simplicity:** Use CSS generic font families (`monospace`, `serif`, `sans-serif`) instead of verbose system font stacks

## Font Patterns

Three font patterns are used across the application. All include emoji font fallback.

| Context | CSS Font Family | Usage |
|---------|----------------|-------|
| Monospace | `monospace, <emoji fallback>` | Terminal body, image-viewer, clipboard dialog code, link dialog |
| UI | `var(--ui-font-family, sans-serif), <emoji fallback>` | Settings panel, tab bar |
| Markdown body | `var(--markdown-body-font-family, serif), var(--markdown-emoji-font-family, <emoji fallback>)` | Markdown viewer body, fullscreen |
| Markdown code | `var(--markdown-code-font-family, monospace), var(--markdown-emoji-font-family, <emoji fallback>)` | Markdown viewer code blocks |

### Emoji Font Fallback

All patterns include the same emoji suffix:
```css
'Apple Color Emoji', 'Segoe UI Emoji', 'Segoe UI Symbol', 'Noto Color Emoji'
```

| OS | Resolved Emoji Font |
|----|---------------------|
| macOS/iOS | Apple Color Emoji |
| Windows | Segoe UI Emoji |
| Linux | Noto Color Emoji |

### Font Chain Logic

`buildFontFamilyChain(primary, emoji, secondary)`:
- Returns only user-specified fonts, joined by `, `
- Returns empty string when all inputs are empty
- No generic family is appended

`applyFontFamily(primary, emoji, secondary)`:
- Sets `--terminal-font-family` CSS variable when user chain is non-empty
- Removes CSS variable when chain is empty (CSS fallback activates)
- Passes `chain || "monospace"` to Canvas renderer

## Implementation Approach

### Component Changes

#### 1. TypeScript Constants (`src/terminal-app/config.ts`)

`DEFAULT_FONT_FAMILY` is `"monospace"`.

#### 2. Font Chain Builder (`src/settings/settings-applier.ts`)

```typescript
export function buildFontFamilyChain(primary: string, emoji: string, secondary: string): string {
  const parts: string[] = [];
  if (primary) parts.push(primary);
  if (emoji) parts.push(emoji);
  if (secondary) parts.push(secondary);
  return parts.join(", ");
}
```

#### 3. Font Picker Clear Button (`src/settings/font-picker.ts`)

Add clear button to `renderFontPickerInput()`:
```
[input field] [x button] [Change button]
```

- Clear button element: `<button class="settings-font-picker-clear" aria-label="Clear">x</button>`
- Visible only when `opts.value` is non-empty
- On click: calls `opts.onSelect("")` and updates input display

#### 4. CSS Files - Monospace Replacement

Replace hardcoded font stacks with `monospace, <emoji fallback>` in:
- `src/styles.css` (body)
- `src/image-viewer/styles.css`
- `src/image-viewer/index.ts`
- `src/image-viewer/display-mode-styles.ts`
- `src/shared/zoom-styles.ts`
- `src/markdown/link-dialog.css`
- `src/clipboard/dialog.ts` (monospace portion)

#### 5. CSS Files - Markdown Body (Serif)

Update Markdown body font fallback to `serif, <emoji>` in:
- `src/styles.css` (Markdown body fallback in CSS variable)
- `src/markdown/fullscreen.css` (Markdown body fallback)

#### 6. CSS Files - Markdown Code

Update code font fallbacks to `monospace, <emoji>` in:
- `src/styles.css` (Markdown code fallback in CSS variable)
- `src/markdown/fullscreen.css` (Markdown code fallback)

#### 7. CSS Files - UI Font Stack

Update UI font stack to `var(--ui-font-family, sans-serif), <emoji>` in:
- `src/styles/settings-panel.css`
- `src/styles/tab-bar.css`
- `src/clipboard/dialog.ts` (sans-serif portion)

#### 8. Markdown Emoji Font Setting (FR7)

Add `markdown_emoji_font_family` setting:

**Rust** (`src-tauri/src/commands/config.rs`):
- Add `markdown_emoji_font_family: String` field to `UiSettings` (default: empty string)

**TypeScript** (`src/settings/types.ts`):
- Add `markdown_emoji_font_family: string` to `AppSettings`
- Add `"markdown-emoji"` to `FontCategory`

**Settings UI** (`src/settings/settings-sections.ts`):
- Add emoji font picker between body and code font pickers in Markdown section

**Settings Applier** (`src/settings/settings-applier.ts`):
- `applyMarkdownSettings(bodyFont, codeFont, emojiFont, fontSize)`
- Sets `--markdown-emoji-font-family` CSS variable when emoji font is configured
- Removes CSS variable when empty (CSS fallback activates)

#### 9. PTY Resize on Font Change (FR9)

**`src/terminal-app/index.ts`**:
- `applySetting()`: After applying `fontSize` or `fontFamily` to renderer, calls `handleCharSizeChange()`
- New method `handleCharSizeChange()`:
  1. Gets new character dimensions from renderer (`getCharWidth()`/`getCharHeight()`)
  2. Updates `this.charSize`
  3. Recalculates cols/rows via `calculateTerminalSize()`
  4. Resizes state, renderer, selection controller, mouse handler
  5. Disconnects and reconnects ResizeObserver with new character dimensions
  6. Resizes PTY

### Data Flow

```
Font change in settings
  → applyFontFamily() sets CSS variable + notifies renderer
  → renderer.setFontFamily() updates font + re-measures character size
  → TerminalApp.applySetting() detects font change
  → handleCharSizeChange() reads new char dimensions from renderer
  → calculateTerminalSize() computes new cols/rows
  → state.resize() + renderer.resize() + pty.resize()
  → ResizeObserver reconnected with new char dimensions
```

### File Structure

```
src/
├── terminal-app/
│   ├── config.ts                    # DEFAULT_FONT_FAMILY = "monospace"
│   └── index.ts                     # handleCharSizeChange() for PTY resize
├── settings/
│   ├── settings-applier.ts          # buildFontFamilyChain() + applyMarkdownSettings()
│   ├── settings-applier.test.ts     # Test updates
│   ├── settings-sections.ts         # Markdown emoji font picker
│   ├── types.ts                     # markdown_emoji_font_family, FontCategory
│   └── font-picker.ts              # Clear button addition
├── styles.css                       # body + markdown fallbacks
├── styles/
│   ├── settings-panel.css           # UI font emoji fallback
│   └── tab-bar.css                  # UI font emoji fallback
├── image-viewer/
│   ├── styles.css                   # Monospace + emoji
│   ├── index.ts                     # Monospace + emoji
│   └── display-mode-styles.ts       # Monospace + emoji
├── shared/
│   └── zoom-styles.ts               # Monospace + emoji
├── clipboard/
│   └── dialog.ts                    # sans-serif + emoji / monospace + emoji
└── markdown/
    ├── link-dialog.css              # Monospace + emoji
    └── fullscreen.css               # Serif + emoji / monospace + emoji
```

## Test Scenarios

### Unit Tests
- [ ] `buildFontFamilyChain()` returns empty string when all inputs are empty
- [ ] `buildFontFamilyChain()` returns only user fonts (no generic fallback appended)
- [ ] `applyFontFamily()` removes CSS variable when chain is empty
- [ ] `applyFontFamily()` notifies renderer with `"monospace"` when chain is empty
- [ ] Font picker clear button calls onSelect with empty string
- [ ] `applyMarkdownSettings()` sets/removes emoji CSS variable correctly

### Integration Tests
- [ ] Settings round-trip: set font → clear → verify default restored
- [ ] CSS variables correctly set/removed based on font configuration

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Font picker clear button hidden when value is already empty
- [ ] Multiple rapid clear/set operations don't cause race conditions
- [ ] Font change triggers PTY resize with correct new dimensions
- [ ] ResizeObserver uses updated character dimensions after font change

## Success Criteria

- [ ] All functional requirements FR1-FR9 are implemented
- [ ] All unit and integration tests pass
- [ ] No hardcoded "Inconsolata", "Noto Sans JP" font references remain (except in test fixtures if needed)
- [ ] Font rendering works correctly on Linux (primary development platform)
- [ ] Existing E2E tests pass without regression
- [ ] Terminal correctly resizes when font family or size changes
