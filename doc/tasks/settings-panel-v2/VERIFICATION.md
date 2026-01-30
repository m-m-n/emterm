# Verification Document: Settings Panel

## Overview

**Feature**: Settings Panel Extension (Phases 1-3)
**SPEC.md**: `doc/tasks/settings-panel-v2/SPEC.md`
**Requirements**: `doc/tasks/settings-panel-v2/要件定義書.md`
**Implementation Plans**:
- `doc/tasks/settings-panel-v2/IMPLEMENTATION-Phase1.md`
- `doc/tasks/settings-panel-v2/IMPLEMENTATION-Phase2.md`
- `doc/tasks/settings-panel-v2/IMPLEMENTATION-Phase3.md`

## Build Verification

### Rust Build

```bash
cargo build --manifest-path src-tauri/Cargo.toml
```

**Expected Result**: Exit code 0, no errors.

### TypeScript Type Check

```bash
bun run typecheck
```

**Expected Result**: Exit code 0, no type errors.

### Full Build

```bash
bun tauri build
```

**Expected Result**: Exit code 0, application binary produced.

## Test Verification

### Rust Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

### TypeScript Tests

```bash
bun test
```

### Coverage Target

- **Minimum**: 80%
- **Target**: 90% for settings-related modules

## Test Scenarios from SPEC.md

### Unit Tests (Rust)

| ID | Scenario | Expected Result | Phase |
|----|----------|-----------------|-------|
| RU-01 | `AppSettings::default()` returns correct defaults for all fields | All defaults match SPEC values | 1 |
| RU-02 | Deserialization of `{}` produces all defaults | All fields have default values | 1 |
| RU-03 | Deserialization of `{"font_size": 13}` (old format) produces defaults for new fields | font_size=13, all others default | 1 |
| RU-04 | Deserialization ignores unknown fields | No error on unknown JSON keys | 1 |
| RU-05 | `save_settings` rejects `font_size` outside 8-32 | Error returned | 1 |
| RU-06 | `save_settings` rejects `line_height` outside 0.8-3.0 | Error returned | 1 |
| RU-07 | `save_settings` rejects invalid `ui_theme` value | Error returned | 1 |
| RU-08 | `save_settings` rejects invalid `cursor_style` value | Error returned | 1 |
| RU-09 | `save_settings` rejects `opacity` outside 0.3-1.0 | Error returned | 2 |
| RU-10 | `save_settings` rejects `scroll_speed` outside 1-10 | Error returned | 2 |
| RU-11 | `save_settings` rejects invalid `bell_action` value | Error returned | 2 |
| RU-12 | `save_settings` rejects invalid `show_scrollbar` value | Error returned | 2 |
| RU-13 | `save_settings` accepts valid complete settings | Success, file written | 1 |
| RU-14 | `KeybindSettings::default()` returns correct defaults | All keybind defaults match SPEC | 1 |
| RU-15 | Settings round-trip: serialize then deserialize preserves all fields | All fields preserved | 1 |
| RU-16 | `shell_args` (Vec<String>) serialization/deserialization round-trip | Array preserved correctly | 2 |
| RU-17 | Deserialization of `{"font_size": null}` produces default font_size | font_size=13 (default) | 1 |
| RU-18 | Deserialization of invalid enum value (e.g., `{"ui_theme": "invalid"}`) | Deserialization error | 1 |

### Unit Tests (TypeScript)

| ID | Scenario | Expected Result | Phase |
|----|----------|-----------------|-------|
| TU-01 | `applyFontFamily` sets `--terminal-font-family` CSS variable | CSS variable updated | 1 |
| TU-02 | `applyLineHeight` sets `--terminal-line-height` CSS variable | CSS variable updated | 1 |
| TU-03 | `applyUiTheme("light")` sets `data-theme="light"` | Attribute set | 1 |
| TU-04 | `applyUiTheme("dark")` sets `data-theme="dark"` | Attribute set | 1 |
| TU-05 | `applyUiTheme("system")` respects `prefers-color-scheme` | Attribute matches OS preference | 1 |
| TU-06 | `applyPadding` sets `--terminal-padding` CSS variable | CSS variable updated | 2 |
| TU-07 | `applyCursorStyle` notifies renderers | Renderer notification sent | 1 |
| TU-08 | `applyCursorBlink` notifies renderers | Renderer notification sent | 1 |
| TU-09 | `applyOpacity` calls Tauri window API with correct value | Window opacity updated | 2 |
| TU-10 | `applyTerminalColorScheme` sets terminal color CSS variables | CSS variables updated | 2 |
| TU-11 | `applyTerminalColorScheme("default")` removes custom overrides | Custom CSS variables removed | 2 |
| TU-12 | `applyScrollbar` updates scrollbar visibility class | CSS class updated | 2 |
| TU-13 | Rich Content section renders both toggles in Appearance | Both toggles displayed | 3 |
| TU-14 | Toggle `inline_images_enabled` saves setting correctly | Setting persisted | 3 |
| TU-15 | Toggle `markdown_rendering` saves setting correctly | Setting persisted | 3 |

### Integration Tests

| ID | Scenario | Expected Result | Phase |
|----|----------|-----------------|-------|
| IT-01 | Category navigation switches between Appearance, Terminal, Keybinds | Correct panel displayed for each tab | 1 |
| IT-02 | All settings render with correct current values | Values match loaded settings | 1-3 |
| IT-03 | Number input changes save and apply | Setting saved and visually applied | 1 |
| IT-04 | Select dropdown changes save and apply | Setting saved and visually applied | 1 |
| IT-05 | Toggle switch changes save and apply | Setting saved and applied | 1 |
| IT-06 | Slider changes save and apply | Setting saved and applied | 2 |
| IT-07 | Keybind capture records and saves key combinations | Keybind updated in settings | 1 |
| IT-08 | Settings persist after panel close and reopen | Values unchanged | 1 |
| IT-09 | Image rendering with `inline_images_enabled=true` | Image rendered in terminal | 3 |
| IT-10 | Image rendering with `inline_images_enabled=false` | Image data processed without rendering | 3 |
| IT-11 | Markdown rendering with `markdown_rendering=true` | Markdown block rendered | 3 |
| IT-12 | Markdown rendering with `markdown_rendering=false` | Markdown OSC consumed without rendering | 3 |

### Edge Cases

| ID | Scenario | Expected Result | Phase |
|----|----------|-----------------|-------|
| EC-01 | Old settings file with only `font_size` loads without error | All new fields use defaults | 1 |
| EC-02 | Corrupted JSON file falls back to all defaults | App starts with default settings | 1 |
| EC-03 | Empty string `font_family` uses system monospace | Terminal uses default monospace font | 1 |
| EC-04 | Empty string `shell_path` uses system default shell | New tab opens with default shell | 2 |
| EC-05 | `opacity` at minimum (0.3) | Window at 30% opacity | 2 |
| EC-06 | `opacity` at maximum (1.0) | Window fully opaque | 2 |
| EC-07 | `scrollback_lines` at 0 | No scrollback buffer | 2 |
| EC-08 | `scrollback_lines` at 100000 | Large scrollback buffer | 2 |
| EC-09 | Settings file with null values for optional fields | Defaults applied for null values | 1 |
| EC-10 | Image rendering with `inline_images_enabled=false` | No image rendered | 3 |
| EC-11 | Markdown rendering with `markdown_rendering=false` | No markdown block rendered | 3 |

## Code Quality Verification

### Format Check (Rust)

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

### Lint (Rust)

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

### Static Analysis (TypeScript)

```bash
bun run typecheck
```

## File Structure Verification

### Phase 1: Files to Modify

| File | Changes |
|------|---------|
| `src-tauri/src/commands/config.rs` | Extended AppSettings + KeybindSettings structs, validation constants, default functions, save validation, unit tests |
| `src/settings/types.ts` | Extended AppSettings interface, KeybindSettings interface, type aliases, validation constants |
| `src/settings/settings-applier.ts` | New apply functions: applyFontFamily, applyLineHeight, applyUiTheme, applyCursorStyle, applyCursorBlink; extended RendererSettings |
| `src/settings/settings-panel.ts` | Enable all category tabs, renderTerminalSection, renderKeybindsSection, new UI controls (text input, select, toggle, keybind capture) |
| `src/styles/settings-panel.css` | New styles: text input, select, toggle switch, keybind capture, subsection header |
| `src/settings/index.ts` | Updated exports |

### Phase 2: Files to Modify

| File | Changes |
|------|---------|
| `src/settings/settings-applier.ts` | New apply functions: applyTerminalColorScheme, applyOpacity, applyPadding, applyScrollbar |
| `src/settings/settings-panel.ts` | New settings rows in Appearance (Theme & Color, Layout), Terminal (Shell, Behavior), Keybinds (Tab Management); slider control |
| `src/styles/settings-panel.css` | Slider styles |
| Backend PTY spawn code | Read shell settings for new tab creation |

### Phase 3: Files to Modify

| File | Changes |
|------|---------|
| `src/settings/settings-panel.ts` | Rich Content subsection with two toggles |
| Image rendering code | Feature flag check for inline_images_enabled |
| Markdown rendering code | Feature flag check for markdown_rendering |

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify | Phase |
|----|------------------------|---------------|-------|
| SC-01 | All Phase 1 settings items implemented and functional | Manual testing of each Phase 1 setting | 1 |
| SC-02 | All three category tabs navigable | Click each tab, verify content switches | 1 |
| SC-03 | Real-time preview for visual settings | Change font/theme/cursor, observe immediate change | 1 |
| SC-04 | Settings persist across app restarts | Change settings, restart, verify values | 1 |
| SC-05 | Old settings files (font_size only) load correctly | Write `{"font_size": 16}` to settings.json, launch app | 1 |
| SC-06 | All unit tests pass | `cargo test` and `bun test` exit 0 | 1-3 |
| SC-07 | Build succeeds | `cargo test`, `bun test`, `bun run typecheck` all exit 0 | 1-3 |

### Functional Requirements Coverage

| Requirement | Description | Phase | Verification |
|-------------|-------------|-------|--------------|
| FR1 | Extend AppSettings with all fields | 1 | Type check passes; deserialization tests |
| FR2 | All fields use #[serde(default)] | 1 | RU-02, RU-03 tests |
| FR3 | Enable Terminal and Keybinds tabs | 1 | IT-01 test |
| FR4 | Render appropriate UI controls | 1-3 | Manual testing per category |
| FR5 | Real-time preview for visual settings | 1-2 | Manual testing: font, theme, cursor, opacity, padding |
| FR6 | Auto-save on blur/Enter/change | 1-3 | IT-03, IT-04, IT-05, IT-06, IT-07 tests |
| FR7 | Validate all inputs | 1-2 | RU-05 through RU-12 tests |
| FR8 | Shell settings apply to new tabs only | 2 | Manual testing: change shell, verify new vs existing tabs |

### Non-Functional Requirements Coverage

| Requirement | Description | Verification |
|-------------|-------------|--------------|
| NFR1 | Settings preview within 16ms (60fps) | Performance profiling; no visible lag on input |
| NFR2 | Backward compatible with font_size-only files | EC-01 test |
| NFR3 | Extensible settings structure | All fields use serde(default); new fields can be added |
| NFR4 | All controls keyboard-navigable with ARIA | Manual keyboard-only navigation test |

### User Story Coverage

| User Story | Description | Phase | Verification |
|------------|-------------|-------|--------------|
| US1 | Customize Appearance | 1-2 | Font family, line height, theme all functional |
| US2 | Configure Cursor | 1 | Cursor style and blink controls work |
| US3 | Customize Keybinds | 1-2 | Keybind capture and save works for all keybinds |
| US4 | Configure Shell | 2 | Shell path and args applied to new tabs |
| US5 | Backward-Compatible Settings | 1 | Old files load, missing fields get defaults |

## Manual Testing Checklist

### Phase 1: Basic Functionality

- [ ] Open settings panel via keyboard shortcut
- [ ] Three category tabs visible: Appearance, Terminal, Keybinds
- [ ] Click each tab -- content switches correctly
- [ ] Keyboard navigation between tabs (arrow keys, Enter/Space)
- [ ] **Appearance > Font**: Font Size input works (existing)
- [ ] **Appearance > Font**: Font Family text input -- type value, blur saves
- [ ] **Appearance > Font**: Line Height number input -- change value, preview updates
- [ ] **Appearance > Theme**: UI Theme dropdown -- select Light/Dark/System
- [ ] **Terminal > Cursor**: Cursor Style dropdown -- select Block/Underline/Bar
- [ ] **Terminal > Cursor**: Cursor Blink toggle -- toggle on/off
- [ ] **Keybinds > Basic**: Click Copy keybind -- capture mode activates
- [ ] **Keybinds > Basic**: Press new key combination -- keybind updates
- [ ] **Keybinds > Basic**: Press Escape during capture -- cancels
- [ ] All 9 Phase 1 keybinds render with correct default values

### Phase 1: Persistence

- [ ] Change settings, close panel, reopen -- values preserved
- [ ] Change settings, restart app -- values preserved
- [ ] Delete settings.json, restart -- all defaults applied
- [ ] Write `{"font_size": 16}` to settings.json, restart -- font_size=16, others default

### Phase 1: Edge Cases

- [ ] Empty font_family -- terminal uses system monospace
- [ ] Line height at minimum (0.8) -- renders without overlap
- [ ] Line height at maximum (3.0) -- renders with large spacing
- [ ] UI Theme "System" -- follows OS dark/light preference
- [ ] Change OS theme while app running with "System" -- theme follows

### Phase 2: Settings Items

- [ ] **Appearance > Theme & Color**: Terminal Color Scheme dropdown
- [ ] **Appearance > Theme & Color**: Opacity slider -- window transparency changes
- [ ] **Appearance > Layout**: Padding number input -- terminal padding updates
- [ ] **Appearance > Layout**: Scrollback Lines number input -- saves correctly
- [ ] **Appearance > Layout**: Scrollbar dropdown -- auto/always/never behavior
- [ ] **Terminal > Shell**: Shell Path text input -- hint mentions "new tabs"
- [ ] **Terminal > Shell**: Shell Args text input -- comma-separated display
- [ ] **Terminal > Behavior**: Scroll Speed slider
- [ ] **Terminal > Behavior**: Bell Action dropdown
- [ ] **Terminal > Behavior**: URL Detection toggle
- [ ] **Terminal > Behavior**: Copy on Select toggle
- [ ] **Keybinds > Tab Management**: new_tab keybind capture
- [ ] **Keybinds > Tab Management**: close_tab keybind capture
- [ ] **Keybinds > Tab Management**: next_tab keybind capture
- [ ] **Keybinds > Tab Management**: prev_tab keybind capture

### Phase 2: Integration

- [ ] Set shell_path to valid shell -- new tab uses it
- [ ] Set shell_path to empty -- new tab uses system default
- [ ] Set shell_args -- arguments visible in shell (e.g., `--login`)
- [ ] Opacity at 0.3 -- window very transparent
- [ ] Opacity at 1.0 -- window fully opaque

### Phase 3: Rich Content

- [ ] **Appearance > Rich Content**: Inline Images toggle visible
- [ ] **Appearance > Rich Content**: Markdown Rendering toggle visible
- [ ] Disable Inline Images -- send Kitty image -- no image appears
- [ ] Enable Inline Images -- send Kitty image -- image appears
- [ ] Disable Markdown Rendering -- send markdown OSC -- no markdown block
- [ ] Enable Markdown Rendering -- send markdown OSC -- markdown block appears
- [ ] Both settings default to true when not in settings file

### Accessibility

- [ ] All controls have associated labels
- [ ] Tab navigation through all controls works
- [ ] Toggle switches have role="switch" and aria-checked
- [ ] Category tabs follow ARIA tablist/tab/tabpanel pattern
- [ ] Focus-visible indicators on all interactive elements
- [ ] Keybind capture button announces state to screen readers

### Error Handling

- [ ] Settings file not found -- app starts with defaults
- [ ] Settings file corrupted JSON -- app starts with defaults, log warning
- [ ] Missing fields in settings JSON -- defaults applied per field
- [ ] Unknown fields in settings JSON -- ignored, no error
- [ ] Invalid font_family -- browser fallback to monospace
- [ ] Invalid shell_path -- error reported on tab creation

## Performance Verification

### Real-time Preview Latency

- **Requirement**: Settings preview within 16ms (60fps)
- **Method**: Use Chrome DevTools Performance panel during settings changes
- **Settings to Test**: font_size, font_family, line_height, ui_theme, opacity, padding, cursor_style, cursor_blink
- **Criterion**: No frame drops during real-time preview

### Settings File I/O

- **Requirement**: File read < 10ms, file write < 50ms
- **Method**: Console timing in SettingsService.load() and SettingsService.save()
- **Criterion**: Timing within requirements

## Security Verification

- [ ] Keybind capture does not execute arbitrary code
- [ ] shell_path is passed to backend for validation, not executed in frontend
- [ ] Settings panel does not expose filesystem paths beyond config directory
- [ ] JSON deserialization does not execute code (serde is data-only)

## Verification Commands Summary

```bash
# Full automated verification sequence
cargo build --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
bun run typecheck
bun test

# Run application for manual testing
bun tauri dev

# Backward compatibility test
echo '{"font_size": 16}' > ~/.config/emterm/settings.json
bun tauri dev
# Verify: font_size=16, all other fields use defaults

# Check settings file
cat ~/.config/emterm/settings.json
```

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 3 | Yes | - |
| Rust Unit Tests | 18 | Yes | - |
| TypeScript Unit Tests | 15 | Yes | - |
| Integration Tests | 12 | Yes | - |
| Edge Cases | 11 | Partial | Yes |
| SPEC Success Criteria | 7 | Partial | Yes |
| Functional Requirements | 8 | Partial | Yes |
| Non-Functional Requirements | 4 | - | Yes |
| User Stories | 5 | - | Yes |
| Manual Testing (Phase 1) | 18 | - | Yes |
| Manual Testing (Phase 2) | 19 | - | Yes |
| Manual Testing (Phase 3) | 7 | - | Yes |
| Accessibility | 6 | - | Yes |
| Error Handling | 6 | Partial | Yes |
| Performance | 2 | - | Yes |
| Security | 4 | - | Yes |

**Total**: ~45 automated test scenarios, ~62 manual verification items
