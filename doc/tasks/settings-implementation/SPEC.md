# Feature: Implement All Non-functional Settings

## Overview

eMterm has settings items visible in the UI that have no actual effect. This specification covers implementing all 15 phases to make every settings item functional. Each phase is independently committable.

## Objectives

- Make all settings items in the UI functional
- Maintain backward compatibility with existing settings files
- Each phase is independently committable and testable

## Phase Dependencies

```
Phase 9 (Scrollback) --> Phase 6 (Scrollbar)
Phase 9 (Scrollback) --> Phase 11 (Scroll Speed)
All other phases are independent.
```

---

## Phase 1: Font Family

### Current Problem

- `src/settings/settings-applier.ts:62-71`: Sets CSS variable `--terminal-font-family` and calls `notifyRenderers("fontFamily", fontFamily)`
- `src/terminal/canvas-renderer.ts:849-857`: `applySetting()` switch statement has no `fontFamily` case
- No CSS rules reference `var(--terminal-font-family)`

### Expected Behavior

When the user changes the font family setting:
1. `CanvasRenderer` receives the notification and updates its internal `fontFamily` property
2. Character dimensions are re-measured with the new font
3. The terminal is re-rendered with the new font

### Implementation Approach

**Files to modify:**
- `src/terminal/canvas-renderer.ts`: Add `fontFamily` case to `applySetting()`, create `setFontFamily()` method

**Changes:**
1. Add `setFontFamily(fontFamily: string)` method to `CanvasRenderer`:
   - Update `this.fontFamily` (use default monospace if empty string)
   - Call `this.measureCharacterSize()`
   - Call `this.forceRender()` if state exists
2. Add `fontFamily` case to `applySetting()` switch

### Test Approach

- Unit test: `CanvasRenderer.applySetting("fontFamily", "Fira Code")` updates the font
- Unit test: Empty string falls back to default monospace

### Acceptance Criteria

- [ ] Changing font family in settings updates terminal display font
- [ ] Character width/height are re-measured after font change
- [ ] Empty string falls back to default monospace font

---

## Phase 2: Line Height

### Current Problem

- `src/settings/settings-applier.ts:77-82`: Sets CSS variable `--terminal-line-height` and calls `notifyRenderers("lineHeight", lineHeight)`
- `src/terminal/canvas-renderer.ts:849-857`: `applySetting()` switch statement has no `lineHeight` case
- `src/terminal/canvas-renderer.ts:388-395`: `measureCharacterSize()` calculates line height from `fontSize` instead of using the settings value

### Expected Behavior

When the user changes the line height setting:
1. `CanvasRenderer` receives the notification and stores the line height multiplier
2. Character height is recalculated using the new multiplier
3. The terminal is re-rendered with the new line spacing

### Implementation Approach

**Files to modify:**
- `src/terminal/canvas-renderer.ts`: Add `lineHeight` property, `lineHeight` case to `applySetting()`, create `setLineHeight()` method, update `measureCharacterSize()` to use stored multiplier

**Changes:**
1. Add `private lineHeight: number` property (default 1.2)
2. Update `measureCharacterSize()` to use `this.lineHeight` multiplier instead of hardcoded `fontSize + 2` formula
3. Add `setLineHeight(lineHeight: number)` method:
   - Update `this.lineHeight`
   - Call `this.measureCharacterSize()`
   - Call `this.forceRender()` if state exists
4. Add `lineHeight` case to `applySetting()` switch

### Test Approach

- Unit test: `CanvasRenderer.applySetting("lineHeight", 1.5)` updates character height
- Unit test: Line height affects `getCharHeight()` return value

### Acceptance Criteria

- [ ] Changing line height in settings updates terminal line spacing
- [ ] Character height is recalculated after line height change

---

## Phase 3: UI Theme

### Current Problem

- `src/settings/settings-applier.ts:88-111`: Sets `data-theme` attribute on `<html>` element
- No CSS rules use `[data-theme]` selectors
- Theme switching has no visual effect

### Expected Behavior

When the user switches the UI theme:
1. The `data-theme` attribute is set (already working)
2. CSS rules respond to `[data-theme="light"]` and `[data-theme="dark"]` selectors
3. Tab bar, settings panel, and all UI elements change to the appropriate color scheme

### Implementation Approach

**Files to modify:**
- `src/styles.css`: Add `[data-theme="light"]` CSS rules with light-mode MD3 color tokens
- The existing `[data-theme="dark"]` is the current default (all existing CSS uses dark colors)

**Changes:**
1. The current `:root` colors serve as the dark theme (default)
2. Add `[data-theme="light"] :root` or `:root[data-theme="light"]` block overriding MD3 color tokens with light theme values:
   - `--md-sys-color-primary`, `--md-sys-color-surface`, etc.
3. Terminal background color should also respond to theme

### Test Approach

- Visual test: Switch to "light" theme and verify UI colors change
- Visual test: Switch to "system" and verify it follows OS preference
- Unit test: `applyUiTheme("dark")` sets `data-theme="dark"` attribute

### Acceptance Criteria

- [ ] "dark" theme applies dark color scheme
- [ ] "light" theme applies light color scheme
- [ ] "system" theme follows OS preference
- [ ] Tab bar, settings panel colors change with theme

---

## Phase 4: Opacity

### Current Problem

- `src/settings/settings-applier.ts:134-137`: Sets CSS variable `--terminal-opacity`
- No CSS rules reference `var(--terminal-opacity)`

### Expected Behavior

When the user changes the opacity setting:
1. The terminal background becomes semi-transparent
2. Text remains fully opaque for readability

### Implementation Approach

**Files to modify:**
- `src/settings/settings-applier.ts`: `RendererSettings` に `opacity` を追加、`applyOpacity()` で `notifyRenderers` 呼び出し
- `src/terminal/canvas-renderer.ts`: `applySetting()` に `opacity` case を追加、背景描画で alpha チャンネルに反映

**Changes:**
1. `RendererSettings` に `opacity: number` を追加し、既存の通知パターンで Canvas レンダラーに反映
2. Canvas 背景描画時にアルファチャンネルとして opacity を適用
3. CSS の `opacity` プロパティは Canvas 内のテキストも透明にするため使用しない

### Test Approach

- Unit test: `applyOpacity(0.5)` sets the CSS variable and notifies renderers
- Unit test: `applySetting("opacity", 0.5)` updates CanvasRenderer opacity
- Visual test: Opacity slider changes terminal transparency

### Acceptance Criteria

- [ ] Changing opacity setting updates terminal background transparency
- [ ] Content remains readable at minimum opacity (0.3)

---

## Phase 5: Padding

### Current Problem

- `src/settings/settings-applier.ts:117-120`: Sets CSS variable `--terminal-padding`
- No CSS rules reference `var(--terminal-padding)`

### Expected Behavior

When the user changes the padding setting:
1. Padding is applied around the terminal content area
2. Terminal column/row count is recalculated to account for reduced available space

### Implementation Approach

**Files to modify:**
- `src/styles.css`: Add `padding: var(--terminal-padding)` to `.terminal-root`
- `src/terminal-app/index.ts`: Account for padding when calculating terminal size

**Changes:**
1. Apply `padding: var(--terminal-padding)` to terminal container
2. When measuring available space for cols/rows, subtract padding from container dimensions
3. Notify renderer to resize when padding changes

### Test Approach

- Unit test: `applyPadding(8)` sets the CSS variable
- Visual test: Padding slider adds space around terminal content

### Acceptance Criteria

- [ ] Changing padding adds margin around terminal content
- [ ] Terminal col/row count recalculated after padding change

---

## Phase 6: Show Scrollbar

### Current Problem

- `src/settings/settings-applier.ts:125-128`: Sets CSS variable `--terminal-scrollbar-mode`
- No CSS rules reference this variable
- No scrollbar UI component exists

### Expected Behavior

When the user changes the scrollbar mode:
- "always": Scrollbar is always visible
- "never": Scrollbar is hidden
- "auto": Scrollbar appears only when content overflows (scrollback available)

### Implementation Approach

**Depends on Phase 9 (Scrollback Lines) for full implementation.**

**Files to modify:**
- `src/settings/settings-applier.ts`: `applyScrollbar()` で CSS 変数値マッピング
- `src/styles.css`: `overflow-y: var(--terminal-scrollbar-overflow)` を適用

**Changes:**
1. `applyScrollbar()` で設定値を CSS `overflow-y` 値にマッピング:
   - `"always"` -> `"scroll"`
   - `"never"` -> `"hidden"`
   - `"auto"` -> `"auto"`
2. マッピング後の値を `--terminal-scrollbar-overflow` CSS 変数に設定
3. スクロールコンテナで `overflow-y: var(--terminal-scrollbar-overflow)` で参照
4. Custom scrollbar styling with `::-webkit-scrollbar`

### Test Approach

- Unit test: `applyScrollbar("always")` maps to "scroll", "never" to "hidden"
- Visual test: Scrollbar mode changes scrollbar visibility

### Acceptance Criteria

- [ ] "always" shows scrollbar permanently
- [ ] "never" hides scrollbar
- [ ] "auto" shows scrollbar only when scrollable

---

## Phase 7: Cursor Style / Cursor Blink

### Current Problem

- `src/settings/settings-applier.ts:188-197`: Calls `notifyRenderers("cursorStyle", ...)` and `notifyRenderers("cursorBlink", ...)`
- `src/terminal/canvas-renderer.ts:849-857`: `applySetting()` switch has no `cursorStyle` or `cursorBlink` cases
- The renderer receives notifications but ignores them

### Expected Behavior

When the user changes cursor style or blink:
1. `CanvasRenderer` updates the terminal state's cursor configuration
2. The cursor is immediately re-rendered with the new style
3. Blink timer is started/stopped based on the blink setting

### Implementation Approach

**Files to modify:**
- `src/terminal/canvas-renderer.ts`: Add `cursorStyle` and `cursorBlink` cases to `applySetting()`

**Note:** `CursorState.style` is a public property, so no setter is needed on `TerminalState`. `modes.cursorBlink` is also directly writable.

**Changes:**
1. Add `cursorStyle` case to `applySetting()`:
   - Update the terminal state's cursor style
   - Force re-render cursor area
2. Add `cursorBlink` case to `applySetting()`:
   - Update the terminal state's cursor blink mode
   - Start/stop cursor blink timer accordingly
   - Force re-render cursor area

### Test Approach

- Unit test: `applySetting("cursorStyle", "bar")` changes cursor rendering
- Unit test: `applySetting("cursorBlink", false)` stops blink timer

### Acceptance Criteria

- [ ] Changing cursor style updates cursor shape in real-time
- [ ] Turning off cursor blink stops blinking
- [ ] Turning on cursor blink starts blinking

---

## Phase 8: Terminal Color Scheme

### Current Problem

- `src/settings/settings-panel.ts:288-301`: Select dropdown has only "default" option
- `src/settings/settings-applier.ts:168-183`: Comment says "Future: look up preset by name and set CSS variables"
- `src/terminal/canvas-renderer.ts`: Uses hardcoded `DEFAULT_BACKGROUND` and `DEFAULT_FOREGROUND` from `colors.ts`

### Expected Behavior

When the user selects a color scheme:
1. The 16-color ANSI palette, foreground, background, and cursor colors update
2. The Canvas renderer uses the new colors for rendering
3. "eMterm" (default) restores the built-in colors

### Implementation Approach

**Files to modify:**
- `src/terminal/colors.ts`: Add color scheme presets (data)
- `src/settings/settings-applier.ts`: Look up preset and set CSS variables / notify renderer
- `src/settings/settings-panel.ts`: Add preset options to the select dropdown
- `src/terminal/canvas-renderer.ts`: Support dynamic color palette

**Changes:**
1. Define 6 color scheme presets:
   - eMterm (default, first in dropdown)
   - Solarized Dark
   - Solarized Light
   - Monokai
   - Dracula
   - Nord
2. Each preset: foreground, background, cursor, selection, 16 ANSI colors
3. When scheme changes, update the renderer's color palette
4. Add scheme options to settings panel dropdown ("eMterm" at top)
5. Renderer reads colors from a mutable palette instead of constants

### Test Approach

- Unit test: Selecting a scheme updates color variables
- Unit test: "default" clears custom color overrides
- Visual test: Terminal colors change with scheme selection

### Acceptance Criteria

- [ ] 6 color scheme presets are selectable
- [ ] "eMterm" appears first in dropdown
- [ ] Scheme change updates terminal colors
- [ ] "eMterm" restores built-in colors
- [ ] Canvas renderer re-renders with new palette

---

## Phase 9: Scrollback Lines

### Current Problem

- `src/settings/settings-panel.ts:346`: `onInput: () => {}` (empty function)
- Scrollback buffer is not implemented
- `src/terminal/canvas-renderer.ts:119-133`: `getVisibleLines()` comment says "Scrollback buffer support will be added later"

### Expected Behavior

When scrollback lines is configured:
1. Lines that scroll off the top of the screen are saved in a scrollback buffer
2. The user can scroll up to view saved lines using mouse wheel
3. The buffer size is limited to the configured number of lines
4. When scrolled up, new output maintains the current scroll position (does not auto-scroll to bottom)

### Implementation Approach

**Files to modify:**
- `src/terminal/state.ts`: Add scrollback buffer to terminal state
- `src/terminal/canvas-renderer.ts`: Support rendering from scrollback buffer, handle scroll offset
- Terminal container: Add mouse wheel event handler for scrolling

**Changes:**
1. Add scrollback buffer to `TerminalState`:
   - Store lines that scroll off the top
   - Limit buffer size to `scrollback_lines` setting
2. Add scroll offset tracking
3. Update `getVisibleLines()` to return lines from scrollback when scrolled
4. Add mouse wheel handler for scrolling through buffer
5. On new output while scrolled up, maintain current scroll position (do not auto-scroll)

### Test Approach

- Unit test: Lines pushed off screen are saved to scrollback
- Unit test: Buffer respects size limit
- Unit test: Scroll offset changes visible lines
- Integration test: Mouse wheel scrolls through buffer

### Acceptance Criteria

- [ ] Configured number of scrollback lines are retained
- [ ] Mouse wheel scrolls through history
- [ ] Scroll position is maintained when new output arrives while scrolled up
- [ ] Setting change applies to next session

---

## Phase 10: Shell Path / Shell Args

### Current Problem

- `src/settings/settings-panel.ts:417-438`: Values saved but not used
- `src/pty/client.ts:83-99`: `spawn()` passes `options.shell` but ignores args
- `src/terminal-app/index.ts:199`: `this.ptyClient.spawn({ cols, rows })` does not pass shell path
- `src/types/pty.ts:50-68`: `PtySpawnOptions` has no `shell_args` field
- `src-tauri/src/lib.rs:120-126`: `pty_spawn` command has no `args` parameter

### Expected Behavior

When shell path/args are configured:
1. New tabs spawn the configured shell with the specified arguments
2. Empty shell path uses the platform default shell
3. Settings changes apply to new tabs only (existing tabs unaffected)

### Implementation Approach

**Files to modify:**
- `src/types/pty.ts`: Add `args` field to `PtySpawnOptions`
- `src/pty/client.ts`: Pass `args` to spawn command
- `src/terminal-app/index.ts`: Read settings and pass shell/args to PTY spawn
- `src-tauri/src/lib.rs`: Add `args` parameter to `pty_spawn` command
- `src-tauri/src/pty/manager.rs`: Pass args to session creation
- PTY session creation: Use args when spawning the shell process

**Changes (Frontend):**
1. Add `args?: string[]` to `PtySpawnOptions`
2. Update `PtyClient.spawn()` to pass `args` to the invoke call
3. In `TerminalApp.init()`, load settings and pass `shell_path` and `shell_args` to spawn

**Changes (Backend):**
1. Add `args: Option<Vec<String>>` parameter to `pty_spawn` command
2. Pass args through manager to `PtySession::new()`
3. Use args when spawning the child process

### Test Approach

- Unit test (Rust): `pty_spawn` with custom shell path
- Unit test (Rust): `pty_spawn` with args
- Unit test (TS): `PtyClient.spawn()` passes shell and args
- Integration test: New tab uses configured shell

### Acceptance Criteria

- [ ] Configured shell path is used for new tabs
- [ ] Shell args are passed to the shell process
- [ ] Empty shell path falls back to default shell
- [ ] Existing tabs are unaffected by setting change

---

## Phase 11: Scroll Speed

### Current Problem

- `src/settings/settings-panel.ts:454`: `onInput: () => {}` (empty function)
- No scroll handling code references scroll speed setting

### Expected Behavior

When scroll speed is configured:
1. Mouse wheel scroll amount is multiplied by the speed factor
2. Higher values scroll more lines per wheel tick

### Implementation Approach

**Depends on Phase 9 (Scrollback Lines).**

**Files to modify:**
- Mouse wheel scroll handler (created in Phase 9): Apply scroll speed multiplier

**Changes:**
1. Read `scroll_speed` from settings
2. Multiply wheel delta by scroll speed value
3. Apply resulting offset to scrollback position

### Test Approach

- Unit test: Scroll speed multiplier affects scroll amount
- Visual test: Higher speed scrolls faster

### Acceptance Criteria

- [ ] Scroll speed setting affects scroll amount
- [ ] Higher values produce faster scrolling

---

## Phase 12: Bell Action

### Current Problem

- `src/settings/settings-panel.ts:459-470`: Value saved but not used
- No BEL character (0x07) handling code exists in the frontend

### Expected Behavior

When a BEL character is received:
- "visual": Terminal briefly flashes (CSS animation)
- "sound": System beep sound plays
- "none": No action

### Implementation Approach

**Files to modify:**
- `src/terminal/handlers/types.ts`: `TerminalStateAccessor` に `onBell` コールバック追加
- `src/terminal/state.ts`: `TerminalState` に `onBell` プロパティ実装
- `src/terminal/handlers/c0_handlers.ts`: `handleBel()` から `state.onBell?.()` を呼び出し
- `src/terminal-app/index.ts`: `onBell` コールバック登録、設定に応じたアクション実行
- `src/styles.css`: Add bell flash animation CSS

**Changes:**
1. `TerminalStateAccessor` に `onBell?: () => void` コールバックを追加
2. `handleBel()` で `state.onBell?.()` を呼び出す
3. `TerminalApp` が `TerminalState` 作成後に `onBell` コールバックを登録
4. コールバック内で `bell_action` 設定に応じて分岐:
   - "visual": CSS class で flash animation
   - "sound": Web Audio API or `<audio>` element
   - "none": Do nothing

### Test Approach

- Unit test: `handleBel()` calls `state.onBell` callback
- Unit test: `handleBel()` without callback does not throw
- Unit test: BEL action fires correct handler based on setting
- Visual test: "visual" produces visible flash
- Audio test: "sound" produces audible beep

### Acceptance Criteria

- [ ] "visual" flashes the screen on BEL
- [ ] "sound" plays a beep on BEL
- [ ] "none" does nothing on BEL

---

## Phase 13: URL Detection

### Current Problem

- `src/settings/settings-panel.ts:472-479`: Value saved but not used
- No URL detection/highlighting functionality exists

### Expected Behavior

When URL detection is enabled:
1. URLs in terminal output are detected using regex
2. Detected URLs are visually highlighted (underline + color)
3. Ctrl+clicking a URL opens it in the default browser

### Implementation Approach

**Files to modify:**
- New module: `src/terminal/url-detector.ts`
- `src/terminal/canvas-renderer.ts`: Render URL highlights
- `src/terminal-app/index.ts` or mouse handler: Handle URL clicks
- Tauri: Use `shell.open()` API to open URLs

**Changes:**
1. URL regex pattern for detecting http(s)://, ftp://, file:// URLs
2. After rendering text, scan visible lines for URLs
3. Overlay URL highlights (underline + distinct color)
4. On Ctrl+click, check if click position is over a URL
5. If URL Ctrl+clicked, open with Tauri `shell.open()` API
6. Read `url_detection` setting to enable/disable

### Test Approach

- Unit test: URL regex detects various URL formats
- Unit test: URL detection disabled when setting is off
- Integration test: URL click opens browser

### Acceptance Criteria

- [ ] URLs in terminal output are detected and highlighted
- [ ] Ctrl+clicking a URL opens the default browser
- [ ] Setting OFF disables URL detection

---

## Phase 14: Copy on Select

### Current Problem

- `src/settings/settings-panel.ts:482-488`: Value saved but not used
- `src/selection-v2/SelectionController.ts`: Does not reference `copy_on_select` setting

### Expected Behavior

When copy_on_select is enabled:
1. After a text selection is completed (mouseup), the selected text is automatically copied to clipboard
2. When disabled, selection alone does not trigger copy

### Implementation Approach

**Files to modify:**
- `src/selection-v2/SelectionController.ts`: Check `copy_on_select` setting on selection completion

**Changes:**
1. On selection completion (mouseup after drag), check `copy_on_select` from cached settings
2. If enabled, call the existing clipboard copy method with the selected text
3. Read setting from `SettingsService.getCached()`

### Test Approach

- Unit test: Selection completion triggers copy when setting is ON
- Unit test: Selection completion does not copy when setting is OFF

### Acceptance Criteria

- [ ] Setting ON: text selection auto-copies to clipboard
- [ ] Setting OFF: selection does not auto-copy

---

## Phase 15: Keybinds

### Current Problem

- `src/tab-bar/keyboard-handler.ts`: Hardcoded keybinds (Ctrl+T, Ctrl+W, Ctrl+Tab, etc.)
- `src/terminal-app/handlers/keyboard.ts`: Hardcoded clipboard shortcuts
- Settings-saved keybind values are never read

### Expected Behavior

When keybinds are configured:
1. All keyboard shortcuts use the configured keybind values
2. Default keybinds work as initial values
3. Changed keybinds take effect immediately

### Implementation Approach

**Files to modify:**
- `src/tab-bar/keyboard-handler.ts`: Read keybinds from settings instead of hardcoding
- `src/terminal-app/handlers/keyboard.ts`: Read clipboard keybinds from settings
- New utility: Keybind matching function to compare key events with keybind strings

**Changes:**
1. Create keybind matching utility:
   - Parse keybind string (e.g., "Ctrl+Shift+T") into components
   - Match against `KeyboardEvent` properties
2. Update `TabKeyboardHandler.handleKeyDown()`:
   - Load keybinds from `SettingsService.getCached()`
   - Match each key event against configured keybinds
3. Update `KeyboardHandler` (terminal-app):
   - Load clipboard keybinds (copy, paste, select_all) from settings
   - Match against configured values
4. Handle keybind changes at runtime:
   - On settings save, handlers pick up new values from cache

### Test Approach

- Unit test: Keybind matcher correctly parses and matches key combinations
- Unit test: Custom keybind triggers the correct action
- Unit test: Default keybinds work when no custom values set
- Integration test: Changing a keybind in settings panel updates the actual shortcut

### Acceptance Criteria

- [ ] Custom keybinds function as configured shortcuts
- [ ] Default keybinds work as initial values
- [ ] Keybind conflicts are handled (later setting wins)

---

## File Structure

```
src/
├── settings/
│   ├── settings-applier.ts       # Phases 1-7: Already sends notifications, Phase 4: opacity notifyRenderers, Phase 6: scrollbar CSS mapping, Phase 8: color scheme lookup
│   ├── settings-panel.ts         # Phase 8: Add color scheme options
│   ├── settings-service.ts       # Used by Phases 14-15 for getCached()
│   └── types.ts                  # No changes needed
├── terminal/
│   ├── canvas-renderer.ts        # Phases 1-2, 4, 7-9: applySetting cases, opacity, scrollback rendering
│   ├── colors.ts                 # Phase 8: Color scheme presets
│   ├── state.ts                  # Phases 7, 9, 12: Cursor setters, scrollback buffer, onBell callback
│   ├── url-detector.ts           # Phase 13: New file
│   ├── handlers/
│   │   ├── c0_handlers.ts       # Phase 12: handleBel() calls state.onBell?.()
│   │   └── types.ts            # Phase 12: TerminalStateAccessor に onBell callback 追加
│   └── renderer-interface.ts     # No changes needed
├── terminal-app/
│   ├── index.ts                  # Phases 5, 10, 12: Padding resize, shell path, bell handler
│   └── handlers/
│       └── keyboard.ts           # Phase 15: Configurable keybinds
├── tab-bar/
│   └── keyboard-handler.ts       # Phase 15: Configurable keybinds
├── selection-v2/
│   └── SelectionController.ts    # Phase 14: Copy on select
├── pty/
│   └── client.ts                 # Phase 10: Pass shell args
├── types/
│   └── pty.ts                    # Phase 10: Add args to PtySpawnOptions
├── styles.css                    # Phases 3, 5-6: Theme, padding, scrollbar CSS
└── styles/
    ├── settings-panel.css        # Phase 3: Theme-aware colors
    └── tab-bar.css               # Phase 3: Theme-aware colors

src-tauri/src/
├── lib.rs                        # Phase 10: Add args to pty_spawn
└── pty/
    └── manager.rs                # Phase 10: Pass args to session creation
```

## Test Scenarios

### Unit Tests
- [ ] Phase 1: Font family change updates renderer font and re-measures
- [ ] Phase 2: Line height change updates renderer line spacing
- [ ] Phase 3: Theme attribute changes are applied correctly
- [ ] Phase 4: Opacity CSS variable is set and referenced
- [ ] Phase 5: Padding CSS variable is applied to terminal container
- [ ] Phase 6: Scrollbar mode CSS is applied
- [ ] Phase 7: Cursor style/blink settings reach renderer and update display
- [ ] Phase 8: Color scheme selection updates color palette
- [ ] Phase 9: Scrollback buffer stores and retrieves lines
- [ ] Phase 10: Shell path/args are passed to PTY spawn
- [ ] Phase 11: Scroll speed multiplies scroll delta
- [ ] Phase 12: Bell action triggers correct response
- [ ] Phase 13: URL regex detects various URL patterns
- [ ] Phase 14: Copy on select triggers clipboard on selection end
- [ ] Phase 15: Keybind matcher parses and matches correctly

### Integration Tests
- [ ] Phase 10: New tab spawns configured shell
- [ ] Phase 15: Custom keybinds work end-to-end

### Edge Cases
- [ ] Phase 1: Invalid font family falls back to monospace
- [ ] Phase 8: Unknown scheme name treated as "default"
- [ ] Phase 10: Invalid shell path shows error
- [ ] Phase 10: Empty shell_args passes no arguments
- [ ] Phase 15: Keybind string parsing handles edge cases (single key, multiple modifiers)

## Success Criteria

- [ ] All 15 phases are implemented and tested
- [ ] All existing tests continue to pass
- [ ] Each phase has new unit tests
- [ ] Settings changes are reflected immediately (< 100ms)
- [ ] Backward compatibility with existing settings.json files

## Resolved Questions

- [x] Phase 8: Color scheme presets → eMterm (default), Solarized Dark, Solarized Light, Monokai, Dracula, Nord
- [x] Phase 13: URL click behavior → Ctrl+click
- [x] Phase 9: Auto-scroll behavior → Maintain scroll position when scrolled up
