# Verification Document: Default Font Adjustment

## Overview
**Feature**: Default Font Adjustment
**SPEC.md**: `doc/tasks/default-font-adjustment/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/default-font-adjustment/IMPLEMENTATION.md`

## Build Verification
- Command: `bun tauri build`
- Expected: exit code 0, no errors

## Test Verification

### TypeScript Tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Coverage target: All existing tests pass, new tests added for modified functions

### Rust Tests
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- Coverage target: All existing tests pass, new tests for serde/validation of `markdown_emoji_font_family`

### Type Check
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Expected: exit code 0, no type errors

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `buildFontFamilyChain("", "", "")` with no user fonts | Returns system monospace stack | Unit |
| TS-2 | `buildFontFamilyChain("Fira Code", "", "")` with user primary font | Prepends user font before system monospace stack | Unit |
| TS-3 | `applyFontFamily("", "", "")` removes CSS variable | `--terminal-font-family` removed from root style | Unit |
| TS-4 | Font picker clear button calls onSelect with empty string | onSelect("") invoked | Unit |
| TS-5 | `applyMarkdownSettings()` with emoji font | Emoji font appears in both body and code CSS variable chains | Unit |
| TS-6 | `applyMarkdownSettings()` with empty emoji | Emoji font omitted from chains | Unit |
| TS-7 | Settings round-trip: set font, clear, verify default restored | CSS variable removed, system font stack fallback activates | Integration |
| TS-8 | Font picker clear button hidden when value is empty | Button not visible | Unit |
| TS-9 | Rust: `markdown_emoji_font_family` serde default | Defaults to empty string | Unit (Rust) |
| TS-10 | Rust: `markdown_emoji_font_family` null deserialization | Uses default (empty string) | Unit (Rust) |
| TS-11 | Rust: `markdown_emoji_font_family` explicit value round-trip | Value preserved through serialize/deserialize | Unit (Rust) |

## Code Quality Verification
- Type check: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`
- Grep audit: Verify no remaining `"Inconsolata"` or `"Noto Sans JP"` in production source

## File Structure Verification

### Files to Modify
- `src-tauri/src/commands/config.rs` - Add `markdown_emoji_font_family` field with serde defaults
- `src/terminal-app/config.ts` - Update `DEFAULT_FONT_FAMILY` to system monospace stack
- `src/settings/settings-applier.ts` - Add `SYSTEM_MONO_STACK`, update `buildFontFamilyChain()`, `applyFontFamily()`, `applyMarkdownSettings()`
- `src/settings/settings-applier.test.ts` - Update expectations, add new tests
- `src/settings/settings-sections.ts` - Add markdown emoji font picker
- `src/settings/types.ts` - Add `markdown_emoji_font_family` field, `"markdown-emoji"` category
- `src/settings/font-picker.ts` - Add clear button, add `"markdown-emoji"` category support
- `src/styles.css` - Replace hardcoded fonts in body, markdown-content, code, fullscreen-content, link-confirm-url, image-viewer-info
- `src/styles/settings-panel.css` - Add emoji fallback to UI font stack
- `src/styles/tab-bar.css` - Add emoji fallback to UI font stack
- `src/image-viewer/styles.css` - Replace monospace font stack
- `src/image-viewer/index.ts` - Replace monospace font stack in STYLES constant
- `src/image-viewer/display-mode-styles.ts` - Replace monospace font stack
- `src/shared/zoom-styles.ts` - Replace monospace font stack
- `src/clipboard/dialog.ts` - Replace both monospace and sans-serif font stacks
- `src/markdown/link-dialog.css` - Replace monospace font stack
- `src/markdown/fullscreen.css` - Replace body (serif) and code (monospace) font stacks
- `src/i18n/locales/en.json` - Add fontPickerClear and markdown emoji labels
- `src/i18n/locales/ja.json` - Add fontPickerClear and markdown emoji labels

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements FR1-FR7 are implemented | Review each FR against implementation; run test suite |
| SC-2 | All unit and integration tests pass | `bun test` and `cargo test` in Docker |
| SC-3 | No hardcoded "Inconsolata", "Noto Sans JP", "Noto Color Emoji" font references remain | `grep -r "Inconsolata\|Noto Sans JP\|Noto Color Emoji" src/ src-tauri/src/` returns no results (excluding test fixtures) |
| SC-4 | Font rendering works correctly on Linux | Manual visual verification |
| SC-5 | Existing E2E tests pass without regression | `./scripts/run-e2e-docker.sh test` |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: Terminal monospace font stack | Phase 1 | Unit test: `buildFontFamilyChain` returns system monospace stack |
| FR2: Font picker clear button | Phase 3 | Unit test: onSelect("") called; manual: visual check |
| FR3: Hardcoded font replacement | Phase 2 | Grep audit: no remaining hardcoded font names |
| FR4: Markdown body font stack (serif) | Phase 2 | Visual: Markdown body uses serif fallback; inspect CSS |
| FR5: Markdown code font stack | Phase 2 | Visual: Markdown code uses monospace stack; inspect CSS |
| FR6: UI font stack emoji support | Phase 2 | Inspect CSS: emoji fonts in settings-panel.css and tab-bar.css |
| FR7: Markdown emoji font setting | Phase 4 | Unit test: emoji in both chains; Rust: serde round-trip |

### Non-Functional Requirements Coverage

| Requirement | Verification |
|-------------|--------------|
| NFR1: Compatibility | System font stacks use generic CSS keywords (`ui-monospace`, `ui-serif`) that resolve per-platform |
| NFR2: Backward Compatibility | Non-empty user fonts are prepended before system stack; behavior unchanged. Verify by setting a custom font, then upgrading. |
| NFR3: Maintainability | Font stacks defined as constants (`SYSTEM_MONO_STACK`); CSS uses `var()` fallback chains. Code review. |

## E2E Testing (Docker)

- Command: `./scripts/run-e2e-docker.sh test`
- [ ] All existing E2E tests pass without modification
- [ ] Terminal renders text correctly with default fonts
- [ ] Settings panel opens and displays font options

## Manual Testing (E2E Not Possible)

- [ ] Visual: Terminal text renders clearly with system monospace font on Linux
- [ ] Visual: Markdown body text uses serif font when no user font is set
- [ ] Visual: Markdown code blocks use monospace font when no user font is set
- [ ] Visual: Emoji characters render in terminal, Markdown, settings panel, and tab bar
- [ ] Visual: Font picker clear button appears when font is set, disappears when cleared
- [ ] Visual: After clearing font, placeholder text is visible in input
- [ ] Functional: Set a custom font -> clear it -> verify system default activates
- [ ] Functional: Markdown emoji font picker shows emoji font list
- [ ] Functional: Setting markdown emoji font affects both body and code rendering
- [ ] UX: Clear button is keyboard accessible (Tab to focus, Enter/Space to activate)

## Performance Verification

- Font rendering latency: No measurable degradation (system fonts are locally resolved)
- Settings panel responsiveness: No delay from additional font picker

## Security Verification

- [ ] No user input is injected into CSS without sanitization (existing pattern: values are set via DOM API `style.setProperty()`, not string interpolation)

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 1 | 1 | 0 | 0 |
| TypeScript unit tests | 8 | 8 | 0 | 0 |
| Rust unit tests | 3 | 3 | 0 | 0 |
| Type check | 1 | 1 | 0 | 0 |
| Code quality (grep) | 1 | 1 | 0 | 0 |
| E2E regression | 3 | 0 | 3 | 0 |
| Visual / UX | 10 | 0 | 0 | 10 |
| Security | 1 | 0 | 0 | 1 |
| **Total** | **28** | **14** | **3** | **11** |
