# Verification Document: Markdown Viewer Color Theme Settings

## Overview

**Feature**: Markdown Viewer Color Theme Settings
**SPEC.md**: `doc/tasks/markdown-color-theme/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/markdown-color-theme/IMPLEMENTATION.md`

## Build Verification

### Build Command

```bash
# TypeScript type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Expected Result

- Exit code: 0
- No error messages

## Test Verification

### Test Commands

```bash
# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### Coverage Target

- **Minimum**: 80%
- **Target**: 90%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Each preset has both dark and light variants | 4 presets x 2 variants | Unit |
| TS-2 | Each variant has all 11 required color properties | All properties defined, non-empty | Unit |
| TS-3 | All color values are valid CSS color strings | All values match hex or rgba pattern | Unit |
| TS-4 | applyMarkdownColorTheme() with followUi=true uses UI theme/preset | UI values used for lookup | Unit |
| TS-5 | applyMarkdownColorTheme() with followUi=false uses markdown theme/preset | Markdown-specific values used | Unit |
| TS-6 | applyMarkdownColorTheme() sets all --markdown-* color CSS variables | 11 CSS variables set on :root | Unit |
| TS-7 | System theme resolves correctly based on media query | dark/light resolved from matchMedia | Unit |
| TS-8 | Default settings have markdown_theme_follow_ui: true | Field value is true | Unit (Rust) |
| TS-9 | Default settings have markdown_theme: System | Field value is System | Unit (Rust) |
| TS-10 | Default settings have markdown_theme_preset: Purple | Field value is Purple | Unit (Rust) |
| TS-11 | Missing fields in JSON use defaults | Deserialized correctly | Unit (Rust) |
| TS-12 | Null fields in JSON use defaults | Deserialized correctly | Unit (Rust) |
| TS-13 | Round-trip serialization preserves values | All fields match after round-trip | Unit (Rust) |
| TS-14 | Invalid enum values are rejected by serde | Deserialization error | Unit (Rust) |

## Code Quality Verification

### Format Check

```bash
# Rust
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml -- --check"
```

### Static Analysis

```bash
# TypeScript type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"

# Rust
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"
```

## File Structure Verification

### Files to Create

- `src/settings/markdown-theme-presets.ts` - 8 color palette definitions (11 colors each) + CSS variable mapping
- `src/settings/markdown-theme-presets.test.ts` - Tests for palette structure

### Files to Modify

- `src-tauri/src/commands/config.rs` - Add 3 fields to AppSettings
- `src/settings/types.ts` - Add 3 fields to AppSettings interface
- `src/settings/settings-applier.ts` - Add applyMarkdownColorTheme()
- `src/settings/settings-applier.test.ts` - Add tests
- `src/settings/settings-sections.ts` - Add color theme subsection
- `src/markdown/fullscreen.css` - Add migrated element styles with --markdown-* vars
- `src/markdown/index.ts` - Remove dead exports from theme.ts
- `src/i18n/locales/en.json` - Add color theme i18n keys
- `src/i18n/locales/ja.json` - Add color theme i18n keys

### Files to Delete

- `src/markdown/theme.ts` - Dead code (replaced by markdown-theme-presets.ts)
- `src/markdown/theme.test.ts` - Tests for removed functions

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | All 8 Markdown color palettes defined and visually coherent with UI presets | Unit test: validate 4 presets x 2 modes; Manual: visual inspection |
| SC-2 | Toggle ON/OFF works correctly in settings UI | Manual: test toggle behavior |
| SC-3 | --markdown-* CSS variables receive palette colors | Unit test: verify CSS variable assignment |
| SC-4 | System theme auto-switching works for Markdown viewer | Unit test: mock matchMedia; Manual: change OS theme |
| SC-5 | UI theme changes propagate to Markdown when follow mode is on | Manual: change UI theme with follow ON |
| SC-6 | All settings persisted and backward compatible | Rust tests: default/null/missing field handling |
| SC-7 | Dead code removed (theme.ts, theme.test.ts, dead exports) | Grep: no generateMarkdownTheme/applyMarkdownTheme references in src/ |
| SC-8 | All test scenarios pass | CI: bun test + cargo test |
| SC-9 | TypeScript type check passes | CI: bun run typecheck |
| SC-10 | Rust tests pass | CI: cargo test |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: Define MARKDOWN_THEME_PRESETS with 8 palettes | Phase 1 | Unit test: validate structure |
| FR2: Add settings fields (follow_ui, theme, preset) | Phase 1 | Rust tests: defaults, null, missing |
| FR3: Add settings UI (toggle + selectors) | Phase 3 | Manual: settings panel inspection |
| FR4: Apply resolved palette to --markdown-* CSS variables | Phase 2 | Unit test: CSS variable check |
| FR5: When followUi and UI changes, re-apply markdown | Phase 3 | Manual: change UI theme with follow ON |
| FR6: System theme media query listener | Phase 2 | Unit test: mock matchMedia |
| FR7: Migrate missing element styles to fullscreen.css | Phase 4 | Manual: visual verification of all element types |
| FR8: Remove dead code | Phase 4 | Grep: no references to removed functions |

## E2E Testing (Docker)

Docker environment ref: ~/.claude/skills/docker-e2e-testing/SKILL.md

### Setup

- Run: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "..."`

### Build and Type Check

- [ ] `bun run typecheck` passes with no errors
- [ ] No broken imports after dead code removal

### Unit Tests

- [ ] `bun test` passes with all tests green
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes with all tests green

### Code Quality

- [ ] `cargo fmt -- --check` shows no formatting issues
- [ ] No imports of `generateMarkdownTheme`, `applyMarkdownTheme`, `getDarkTheme`, `getLightTheme` in src/

### Dead Code Verification

- [ ] `grep -r "generateMarkdownTheme" src/` returns no matches
- [ ] `grep -r "applyMarkdownTheme" src/` returns no matches
- [ ] `src/markdown/theme.ts` does not exist (deleted)
- [ ] `src/markdown/theme.test.ts` does not exist (deleted)

## Manual Testing (E2E Not Possible)

Items that cannot be automated via Docker E2E:

### Settings UI

- [ ] Color theme subsection appears in Markdown Viewer settings
- [ ] "Follow UI Theme" toggle renders correctly (default: ON)
- [ ] Toggle ON: theme/preset selectors are hidden
- [ ] Toggle OFF: theme/preset selectors are visible
- [ ] Toggle change applies theme immediately

### Theme Application

- [ ] followUi=true + change UI theme -> Markdown colors update
- [ ] followUi=true + change UI preset -> Markdown colors update
- [ ] followUi=false + change UI theme -> Markdown colors do NOT change
- [ ] followUi=false + change markdown theme -> colors update immediately
- [ ] followUi=false + change markdown preset -> colors update immediately
- [ ] Theme "system" + OS dark mode -> dark palette applied
- [ ] Theme "system" + OS light mode -> light palette applied
- [ ] Theme "system" + change OS theme -> Markdown colors auto-update

### Visual Verification

- [ ] All 8 palettes visually coherent (dark/light x purple/blue/green/orange)
- [ ] Fullscreen markdown display: headings colored correctly
- [ ] Fullscreen markdown display: links colored correctly
- [ ] Fullscreen markdown display: code blocks have distinct background
- [ ] Fullscreen markdown display: blockquotes have border and muted text
- [ ] Fullscreen markdown display: tables render with borders and striping
- [ ] Fullscreen markdown display: horizontal rules visible
- [ ] Fullscreen markdown display: lists properly indented
- [ ] Fullscreen markdown display: images render with max-width
- [ ] No visual regressions after CSS migration

### Persistence

- [ ] Settings persist after app restart
- [ ] Settings file with only markdown_theme_follow_ui: false (other fields use defaults)
- [ ] Old settings files (without new fields) load with correct defaults

### i18n

- [ ] English labels display correctly
- [ ] Japanese labels display correctly
- [ ] Switching language updates color theme section labels

## Performance Verification

### Theme Switching

- Theme change should be instant (< 100ms)
- No visible flickering during theme switch
- Verification: Manual observation during settings changes

## Security Verification

### Input Validation

- [ ] Invalid theme values rejected by serde (Rust test)
- [ ] Invalid preset values rejected by serde (Rust test)
- [ ] CSS color values are hardcoded constants, not user-provided

## Verification Summary

| Category | Items | Automated (Unit) | E2E (Docker) | Manual |
|----------|-------|-------------------|--------------|--------|
| Build | 2 | - | 2 | - |
| Unit Tests | 14 | 14 | - | - |
| Code Quality | 2 | - | 2 | - |
| File Structure | 13 | - | 13 | - |
| Dead Code | 4 | - | 4 | - |
| SPEC Compliance | 10 | 5 | 2 | 3 |
| Settings UI | 5 | - | - | 5 |
| Theme Application | 8 | - | - | 8 |
| Visual | 10 | - | - | 10 |
| Persistence | 3 | - | - | 3 |
| i18n | 3 | - | - | 3 |
| Performance | 1 | - | - | 1 |
| Security | 3 | 2 | - | 1 |

**Total**: 21 automated items (unit tests), 23 E2E items (Docker), 34 manual items
