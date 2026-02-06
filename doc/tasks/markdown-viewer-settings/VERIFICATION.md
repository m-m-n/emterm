# Verification Document: Markdown Viewer Settings

## Overview
**Feature**: Markdown Viewer Settings
**SPEC.md**: `doc/tasks/markdown-viewer-settings/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/markdown-viewer-settings/IMPLEMENTATION.md`

## Build Verification

### Rust Build
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml --no-run"
```

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Expected Result
- Exit code: 0
- No error messages

## Test Verification

### Rust Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### TypeScript Test Command
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### Coverage Target
- **Minimum**: 80%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Default values: markdown_font_size=14, font families="" | Fields have correct defaults | Unit (Rust) |
| TS-2 | Validation: markdown_font_size < 8 | Returns error | Unit (Rust) |
| TS-3 | Validation: markdown_font_size > 32 | Returns error | Unit (Rust) |
| TS-4 | Validation: markdown_font_size = 8 | Passes validation | Unit (Rust) |
| TS-5 | Validation: markdown_font_size = 32 | Passes validation | Unit (Rust) |
| TS-6 | Deserialization: missing fields use defaults | All 3 fields have defaults | Unit (Rust) |
| TS-7 | Deserialization: null fields use defaults | All 3 fields have defaults | Unit (Rust) |
| TS-8 | Deserialization: valid values preserved | Explicit values kept | Unit (Rust) |
| TS-9 | Round-trip: serialize then deserialize | All values preserved | Unit (Rust) |
| TS-10 | applyMarkdownSettings sets CSS variables | 3 CSS vars on :root | Unit (TS) |
| TS-11 | Empty font family: CSS fallback chain used | CSS var removed, fallback active | Unit (TS) |
| TS-12 | Settings without new fields: defaults applied | Backward compatible | Unit (Rust) |
| TS-13 | Settings with null values: defaults applied | Null-safe | Unit (Rust) |

## Code Quality Verification

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Rust Clippy (if available)
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"
```

## File Structure Verification

### Files to Modify
- `src-tauri/src/commands/config.rs` - Add 3 fields, defaults, null deser, validation, tests
- `src-tauri/locales/en.json` - Add markdownFontSize validation message
- `src-tauri/locales/ja.json` - Add markdownFontSize validation message
- `src/settings/types.ts` - Add 3 fields to AppSettings interface, extend FontCategory type
- `src/settings/font-picker.ts` - Add markdown-body/markdown-code to titleMap and font list switch
- `src/settings/settings-panel.ts` - Add markdown-viewer category and switch case
- `src/settings/settings-sections.ts` - Add renderMarkdownViewerSection()
- `src/settings/settings-applier.ts` - Add applyMarkdownSettings(), wire into applySettings()
- `src/styles.css` - Replace hardcoded font values with CSS var()
- `src/markdown/fullscreen.css` - Replace hardcoded font values with CSS var()
- `src/i18n/locales/en.json` - Add category and field labels
- `src/i18n/locales/ja.json` - Add Japanese translations

### Files NOT to Create
- No new files needed. All changes are additions to existing files.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | Settings panel shows "Markdown Viewer" as 5th category | Manual: open settings, verify 5th nav item |
| SC-2 | Body font family configurable via font picker | Manual: click picker, select font, verify saved |
| SC-3 | Code font family configurable via font picker | Manual: click picker, select font, verify saved |
| SC-4 | Font size configurable via number input (8-32pt) | Manual: change value, verify range enforcement |
| SC-5 | Changes apply immediately to Markdown fullscreen overlay | Manual: change setting, open markdown, verify |
| SC-6 | Settings persist in settings.json | Manual: change setting, restart app, verify |
| SC-7 | Backward compatible with existing settings.json | Unit test: deserialize old JSON with no new fields |
| SC-8 | Rust validation works correctly | Unit test: boundary values |
| SC-9 | All tests pass | Automated: cargo test + bun test |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: Add markdown-viewer as 5th category | Phase 2 | Manual: settings nav shows 5 categories |
| FR2: Font picker for markdown_body_font_family | Phase 2 | Manual: font picker opens and saves |
| FR3: Font picker for markdown_code_font_family | Phase 2 | Manual: font picker opens and saves |
| FR4: Number input for markdown_font_size (8-32) | Phase 2 | Manual: input enforces range |
| FR5: Apply via CSS variables | Phase 3 | Unit test + Manual |
| FR6: Apply immediately on change | Phase 3 | Manual: change setting, verify overlay |
| FR7: Apply saved settings on app startup | Phase 3 | Manual: restart, verify settings applied |

## Manual Testing (Tauri WebView - E2E Not Possible)

Chrome-devtools MCP cannot work with Tauri's native WebViews. All UI verification must be manual.

### Settings UI
- [ ] Open settings panel
- [ ] "Markdown Viewer" appears as 5th navigation category
- [ ] Click "Markdown Viewer" → content panel shows font settings
- [ ] Body font picker opens and displays available fonts
- [ ] Code font picker opens and displays available fonts
- [ ] Font size input accepts values 8-32
- [ ] Font size input rejects values outside range
- [ ] Keyboard navigation (arrow keys) includes new category

### Settings Persistence
- [ ] Change body font → setting saved to settings.json
- [ ] Change code font → setting saved to settings.json
- [ ] Change font size → setting saved to settings.json
- [ ] Restart app → all three settings restored
- [ ] Delete new fields from settings.json → defaults used on load

### Markdown Overlay Display
- [ ] Run `emterm markdown <file>` to display Markdown
- [ ] Default display: system fonts, 14pt body text
- [ ] Change body font → overlay text updates immediately
- [ ] Change code font → code blocks update immediately
- [ ] Change font size → all text scales proportionally
- [ ] Headings maintain relative sizes (h1=2em, h2=1.5em, etc.)
- [ ] Code blocks maintain 85% relative size
- [ ] Empty font value → CSS fallback fonts used

### Backward Compatibility
- [ ] Existing settings.json without new fields loads successfully
- [ ] No errors in console on startup with old settings

## Performance Verification

### Requirements
- CSS variable changes must be instant (no perceptible delay)

### How to Verify
- Manual: change font settings, observe that Markdown overlay updates without visible lag

## Security Verification

- [ ] Font size validated on backend (8-32 range) - verified via unit test
- [ ] Font family strings applied via CSS variables only (no innerHTML) - verified via code review
- [ ] No new XSS vectors introduced - verified via code review

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 2 | ✅ | - |
| Tests | 13 | ✅ | - |
| Code Quality | 2 | ✅ | - |
| File Structure | 11 | ✅ | - |
| SPEC Compliance | 9 | Partial (3) | ✅ (6) |
| UI Testing | 8 | - | ✅ |
| Persistence | 5 | - | ✅ |
| Display | 8 | - | ✅ |
| Compatibility | 2 | - | ✅ |
| Performance | 1 | - | ✅ |
| Security | 3 | Partial (1) | ✅ (2) |

**Total**: 28 automated items, 30 manual items
