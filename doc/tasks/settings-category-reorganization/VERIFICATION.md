# Verification Document: Settings Category Reorganization

## Overview

**Feature**: Settings Category Reorganization
**SPEC.md**: `doc/tasks/settings-category-reorganization/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/settings-category-reorganization/IMPLEMENTATION.md`

## Build Verification

### Build Command

```bash
# TypeScript
bun run typecheck

# Rust
cargo build --manifest-path src-tauri/Cargo.toml
```

### Expected Result
- Exit code: 0
- No error messages

## Test Verification

### Test Command

```bash
# TypeScript tests
bun test

# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml
```

### Coverage Target
- **Minimum**: 70%
- **Target**: 80%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Category array returns 4 items | Array length is 4 with correct IDs | Unit |
| TS-2 | UI font family saves correctly | Setting persisted to file | Integration |
| TS-3 | Default value when ui_font_family missing | "Roboto" used | Unit |
| TS-4 | Category navigation switches content | Correct section renders | Integration |
| TS-5 | Settings saved to correct keys | JSON contains expected keys | Integration |
| TS-6 | UI font change applies immediately | CSS variable updated | Integration |
| TS-7 | Navigate through all 4 categories | All categories accessible | E2E |
| TS-8 | Settings persist after restart | Values restored on reload | E2E |

## Code Quality Verification

### Format Check

```bash
# TypeScript
bun run lint
```

### Static Analysis

```bash
# TypeScript
bun run typecheck

# Rust
cargo clippy --manifest-path src-tauri/Cargo.toml
```

## File Structure Verification

### Files to Modify

| File | Changes |
|------|---------|
| `src/settings/types.ts` | Add ui_font_family field |
| `src/settings/settings-panel.ts` | Update categories (3→4), update switch statement |
| `src/settings/settings-sections.ts` | Add renderUiSection, renderTerminalAppearanceSection, renderTerminalBehaviorSection |
| `src/settings/settings-applier.ts` | Add applyUiFont function |
| `src/styles/settings-panel.css` | Add --ui-font-family CSS variable |
| `src/i18n/locales/en.json` | Add category and UI font translation keys |
| `src/i18n/locales/ja.json` | Add category and UI font translation keys |
| `src-tauri/src/settings/types.rs` | Add ui_font_family field with default |

### Files Unchanged

- `src/settings/settings-components.ts`
- `src/settings/settings-service.ts`
- `src/settings/keybind-editor.ts`
- `src/settings/font-picker.ts`
- `src/settings/color-scheme-editor.ts`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | All 4 categories display correctly | Visual inspection + automated test |
| SC-2 | Settings are in appropriate categories | Compare against requirements table |
| SC-3 | UI font family setting works | Change font, verify CSS variable |
| SC-4 | Backward compatible with existing configs | Load old settings file |
| SC-5 | i18n support (Japanese/English) | Switch language, verify labels |
| SC-6 | Keyboard navigation works | Tab/Arrow key navigation |
| SC-7 | All tests pass | Run test suite |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: 4 categories | Phase 2 | Check categories getter returns 4 items |
| FR2: Correct settings per category | Phase 2 | Compare rendered items to spec |
| FR3: UI font applies to .settings-panel | Phase 3 | Check computed style |
| FR4: Default UI font is "Roboto" | Phase 1, 3 | Check default value |

### Non-Functional Requirements

| Requirement | Verification |
|-------------|--------------|
| NFR1: Instant category switching | Measure render time < 100ms |
| NFR2: Backward compatibility | Load settings without ui_font_family |
| NFR3: ARIA accessibility | Inspect ARIA attributes |

## E2E Testing (Docker)

Docker environment ref: `~/.claude/skills/docker-e2e-testing/SKILL.md`

### Setup

```bash
# Run E2E tests
./scripts/run-e2e-docker.sh
```

### Basic Functionality

- [ ] Open settings panel
- [ ] Verify 4 categories visible in navigation
- [ ] Click each category and verify content changes
- [ ] Verify correct settings appear in each category:
  - UI Settings: language, ui_theme, ui_theme_preset, ui_font_family
  - Keybinds: all keyboard shortcuts
  - Terminal Appearance: fonts, colors, padding, scrollbar
  - Terminal Behavior: cursor, shell, scroll, bell, url, copy

### UI Font Setting

- [ ] Navigate to UI Settings category
- [ ] Locate UI Font setting
- [ ] Click to open font picker
- [ ] Select a different font
- [ ] Verify settings panel font changes immediately
- [ ] Verify setting is saved (check settings file or reload)

### Category Navigation

- [ ] Use keyboard (Tab, Arrow keys) to navigate categories
- [ ] Verify focus indicators visible
- [ ] Verify Enter/Space activates category
- [ ] Verify correct ARIA attributes (role="tab", aria-selected)

### Persistence

- [ ] Change UI font setting
- [ ] Close and reopen settings panel
- [ ] Verify UI font setting retained
- [ ] Restart application (if possible in E2E)
- [ ] Verify setting persists

### Edge Cases

- [ ] Load settings file without ui_font_family → should use default
- [ ] Set empty ui_font_family → should use default fallback
- [ ] Select font not installed → CSS fallback should work

### i18n

- [ ] Change language to Japanese
- [ ] Verify all 4 category names in Japanese
- [ ] Verify UI font setting labels in Japanese
- [ ] Change language to English
- [ ] Verify all labels in English

## Manual Testing (E2E Not Possible)

Items requiring human judgment:

- [ ] Visual quality of UI font rendering at different fonts
- [ ] Subjective assessment of category organization clarity
- [ ] Readability of settings with different UI fonts
- [ ] Overall UX improvement compared to previous 3-category layout

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | ✅ | - | - |
| Tests | 8 | ✅ | - | - |
| Code Quality | 2 | ✅ | - | - |
| File Structure | 8 | ✅ | - | - |
| SPEC Compliance | 7 | Partial | ✅ | - |
| E2E Testing | 16 | - | ✅ | - |
| Manual Testing | 4 | - | - | ✅ |

**Total**: 12 automated items, 16 E2E items, 4 manual items

## Verification Checklist by Phase

### Phase 1: Type Definition

- [ ] TypeScript compiles without errors
- [ ] Rust compiles without errors
- [ ] AppSettings interface includes ui_font_family: string
- [ ] Rust struct includes ui_font_family with default "Roboto"

### Phase 2: Category Reorganization

- [ ] categories getter returns array of 4 items
- [ ] Category IDs: "ui", "keybinds", "terminal-appearance", "terminal-behavior"
- [ ] renderUiSection exists and renders: language, ui_theme, ui_theme_preset (ui_font_family added in Phase 3)
- [ ] renderTerminalAppearanceSection renders: font_size, font_family_*, line_height, terminal_color_scheme, padding, scrollback_lines, show_scrollbar
- [ ] renderTerminalBehaviorSection renders: cursor_style, cursor_blink, shell_path, shell_args, scroll_speed, bell_action, url_detection, copy_on_select
- [ ] renderKeybindsSection unchanged
- [ ] Switch statement in renderContent handles all 4 categories

### Phase 3: UI Font + i18n

- [ ] applyUiFont function exists
- [ ] applyUiFont called from applySettings for initial load
- [ ] CSS variable --ui-font-family defined
- [ ] UI font picker added to renderUiSection
- [ ] Font change triggers applyUiFont
- [ ] FontCategory type includes "ui"

**i18n Keys Verification (en.json & ja.json):**
- [ ] `settings.categories.ui` exists
- [ ] `settings.categories.keybinds` exists
- [ ] `settings.categories.terminalAppearance` exists
- [ ] `settings.categories.terminalBehavior` exists
- [ ] `settings.ui.title` exists
- [ ] `settings.ui.fontFamily` exists
- [ ] `settings.ui.fontFamilyDesc` exists
- [ ] `settings.ui.fontFamilyPlaceholder` exists (optional)
- [ ] `settings.appearance.fontPickerUiTitle` exists (for font picker title)
- [ ] Category labels display correctly in both languages
