# Verification Document: Font Family Settings - 3-field Split

## Overview
**Feature**: Font Family Settings - 3-field Split
**SPEC.md**: `doc/tasks/font-family-settings/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/font-family-settings/IMPLEMENTATION.md`

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

### Rust Tests
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### TypeScript Tests
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### Coverage Target
- **buildFontFamilyChain**: 100% (pure function, all combinations)
- **Migration logic (Rust)**: 100% (all migration paths)
- **Overall settings-applier**: Maintain existing coverage level

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | All three fields empty | Chain = `"monospace"` | Unit |
| TS-2 | Primary only: `"Fira Code"` | Chain = `"Fira Code, monospace"` | Unit |
| TS-3 | Primary + secondary: `"Fira Code"` + `"Noto Sans JP"` | Chain = `"Fira Code, Noto Sans JP, monospace"` | Unit |
| TS-4 | All three: `"JetBrains Mono"` + `"Noto Color Emoji"` + `"Noto Sans JP"` | Chain = `"JetBrains Mono, Noto Color Emoji, Noto Sans JP, monospace"` | Unit |
| TS-5 | Emoji + secondary only: `"Noto Color Emoji"` + `"Noto Sans JP"` | Chain = `"Noto Color Emoji, Noto Sans JP, monospace"` | Unit |
| TS-6 | Secondary only: `"Noto Sans JP"` | Chain = `"Noto Sans JP, monospace"` | Unit |
| TS-7 | Emoji only: `"Noto Color Emoji"` | Chain = `"Noto Color Emoji, monospace"` | Unit |
| TS-8 | Legacy migration: `{"font_family": "Fira Code"}` | `font_family_primary` = `"Fira Code"` | Unit (Rust) |
| TS-9 | Legacy + new: `{"font_family": "X", "font_family_primary": "Y"}` | `font_family_primary` = `"Y"` (new field wins) | Unit (Rust) |
| TS-10 | Serialization: `font_family` not in output | JSON output has no `font_family` key | Unit (Rust) |
| TS-11 | Null handling: `{"font_family_primary": null}` | `font_family_primary` = `""` | Unit (Rust) |
| TS-12 | All fields empty -> CSS variable removed | `--terminal-font-family` removed from root | Unit (TS) |
| TS-13 | Non-empty chain -> CSS variable set | `--terminal-font-family` set to chain value | Unit (TS) |
| TS-14 | Renderer notification | `notifyRenderers("fontFamily", chain)` called | Unit (TS) |

### Unit Test Items: `buildFontFamilyChain()`

Located in `src/settings/settings-applier.test.ts`:

| # | Input (primary, emoji, secondary) | Expected Output |
|---|-----------------------------------|-----------------|
| 1 | `("", "", "")` | `"monospace"` |
| 2 | `("Fira Code", "", "")` | `"Fira Code, monospace"` |
| 3 | `("Fira Code", "", "Noto Sans JP")` | `"Fira Code, Noto Sans JP, monospace"` |
| 4 | `("JetBrains Mono", "Noto Color Emoji", "Noto Sans JP")` | `"JetBrains Mono, Noto Color Emoji, Noto Sans JP, monospace"` |
| 5 | `("", "Noto Color Emoji", "Noto Sans JP")` | `"Noto Color Emoji, Noto Sans JP, monospace"` |
| 6 | `("", "", "Noto Sans JP")` | `"Noto Sans JP, monospace"` |
| 7 | `("", "Noto Color Emoji", "")` | `"Noto Color Emoji, monospace"` |

### Unit Test Items: Rust Migration

Located in `src-tauri/src/commands/config.rs` (tests module):

| # | JSON Input | Assertion |
|---|-----------|-----------|
| 1 | `{}` | `font_family_primary == ""`, `font_family_secondary == ""`, `font_family_emoji == ""` |
| 2 | `{"font_family": "Fira Code"}` | `font_family_primary == "Fira Code"` |
| 3 | `{"font_family": "X", "font_family_primary": "Y"}` | `font_family_primary == "Y"` |
| 4 | `{"font_family_primary": "A", "font_family_secondary": "B", "font_family_emoji": "C"}` | All three fields match input |
| 5 | `{"font_family_primary": null}` | `font_family_primary == ""` |
| 6 | Serialize defaults | JSON does not contain `"font_family"` key |
| 7 | Round-trip: set three fields, serialize, deserialize | All three fields preserved |

### Unit Test Items: Updated `applyFontFamily()`

Located in `src/settings/settings-applier.test.ts`:

| # | Input (primary, emoji, secondary) | CSS Variable | Renderer Value |
|---|-----------------------------------|-------------|----------------|
| 1 | `("", "", "")` | removed | `"monospace"` |
| 2 | `("Fira Code", "", "")` | `"Fira Code, monospace"` | `"Fira Code, monospace"` |
| 3 | `("JetBrains Mono", "Noto Color Emoji", "Noto Sans JP")` | `"JetBrains Mono, Noto Color Emoji, Noto Sans JP, monospace"` | same |

### Unit Test Items: Updated `applySettings()`

| # | Scenario | Assertion |
|---|----------|-----------|
| 1 | Full settings with three font fields | CSS variable set to correct chain |
| 2 | Full settings with all font fields empty | CSS variable removed |

## Code Quality Verification

### TypeScript Type Check
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```
- Expected: No type errors
- Validates that all references to `font_family` have been updated to three fields

### Rust Compilation
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --manifest-path src-tauri/Cargo.toml"
```
- Expected: No compilation errors or warnings related to font_family fields

## File Structure Verification

### Files to Modify

| File | Changes |
|------|---------|
| `src/settings/types.ts` | `font_family` replaced with `font_family_primary`, `font_family_secondary`, `font_family_emoji` |
| `src-tauri/src/commands/config.rs` | Three new fields, migration logic, updated tests |
| `src/settings/settings-applier.ts` | `buildFontFamilyChain()` added, `applyFontFamily()` updated |
| `src/settings/settings-applier.test.ts` | Chain builder tests, updated applier tests, updated `makeSettings` |
| `src/settings/settings-panel.ts` | Single font input replaced with three inputs |
| `src/settings/settings-panel.test.ts` | Updated `makeSettings` helper |
| `src/i18n/locales/en.json` | Font family i18n keys replaced |
| `src/i18n/locales/ja.json` | Font family i18n keys replaced |

### Files NOT Modified (confirmed no changes needed)

| File | Reason |
|------|--------|
| `src/terminal/canvas-renderer.ts` | Receives assembled chain string; no changes |
| `src/terminal/renderer-interface.ts` | `RendererSettings.fontFamily` remains `string` |

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | Setting primary font updates terminal display for alphanumeric characters | Manual: set primary font, observe alphanumeric rendering |
| SC-2 | Setting secondary font provides fallback for CJK characters | Manual: set secondary font, type Japanese text |
| SC-3 | Setting emoji font provides fallback for emoji | Manual: set emoji font, display emoji |
| SC-4 | Empty fields are omitted from the fallback chain | Unit test: `buildFontFamilyChain` with empty fields |
| SC-5 | All fields empty falls back to `monospace` | Unit test: `buildFontFamilyChain("", "", "")` returns `"monospace"` |
| SC-6 | Existing `font_family` setting migrates to `font_family_primary` | Unit test (Rust): deserialize legacy JSON |
| SC-7 | Three text inputs appear in settings UI | Manual: open settings, verify Font subsection |
| SC-8 | i18n labels display correctly in English and Japanese | Manual: switch language, verify labels |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| Three separate font fields | Phase 1 | Type check + Rust tests |
| Fallback chain construction | Phase 2 | `buildFontFamilyChain` unit tests |
| CSS variable and renderer notification | Phase 2 | `applyFontFamily` unit tests |
| Three text inputs in settings UI | Phase 3 | Manual testing + panel tests |
| i18n labels (en/ja) | Phase 3 | Manual testing |
| Backward compatibility (migration) | Phase 1 | Rust deserialization tests |

## Manual Testing Checklist

### Basic Functionality
- [ ] Open settings panel -> Appearance -> Font subsection shows three text inputs
- [ ] Primary Font label displays correctly
- [ ] Secondary Font label displays correctly with "Optional" hint
- [ ] Emoji Font label displays correctly with "Optional" hint
- [ ] Primary Font placeholder shows "monospace (default)"
- [ ] Secondary Font and Emoji Font placeholders are empty

### Font Application
- [ ] Set primary font to "Fira Code" -> terminal alphanumeric text changes
- [ ] Set secondary font to "Noto Sans JP" -> terminal CJK text uses this font
- [ ] Set emoji font to "Noto Color Emoji" -> terminal emoji uses this font
- [ ] Clear all three fields -> terminal reverts to default monospace
- [ ] Set only secondary font -> CJK characters use it, alphanumeric uses monospace

### Persistence
- [ ] Set all three fonts, close settings, reopen -> values preserved
- [ ] Set all three fonts, restart application -> values preserved
- [ ] Set all three fonts, close and save -> `settings.json` contains three fields

### Migration
- [ ] Manually set `"font_family": "Fira Code"` in `settings.json` (remove new fields) -> on load, Primary Font shows "Fira Code"
- [ ] After migration load, save settings -> `font_family` removed from JSON, `font_family_primary` present

### i18n
- [ ] Switch language to English -> all three font labels in English
- [ ] Switch language to Japanese -> all three font labels in Japanese

### Edge Cases
- [ ] Set primary to a non-existent font name -> terminal falls back gracefully (no crash)
- [ ] Set very long font name -> UI displays without breaking layout
- [ ] Rapidly switch between inputs -> no double-save or state corruption

### Error Handling
- [ ] Invalid JSON in settings.json -> defaults loaded (no crash)
- [ ] `settings.json` with `"font_family_primary": null` -> treated as empty string

## Performance Verification

- [ ] Font change responsiveness: changing any font field applies within one frame (no visible delay)
- [ ] No extra re-renders: changing one font field does not trigger unnecessary layout recalculations

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 2 | Yes | - |
| Unit Tests (buildFontFamilyChain) | 7 | Yes | - |
| Unit Tests (Rust migration) | 7 | Yes | - |
| Unit Tests (applyFontFamily) | 3 | Yes | - |
| Unit Tests (applySettings) | 2 | Yes | - |
| Code Quality | 2 | Yes | - |
| File Structure | 8 files | Yes (type check) | - |
| SPEC Compliance | 8 | Partial (5 auto, 3 manual) | Yes |
| Manual Testing | 19 | - | Yes |

**Total**: 34 automated items, 22 manual items
