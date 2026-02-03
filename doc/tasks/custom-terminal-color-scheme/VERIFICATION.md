# Verification Document: Custom Terminal Color Scheme

## Overview
**Feature**: Custom Terminal Color Scheme
**SPEC.md**: `doc/tasks/custom-terminal-color-scheme/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/custom-terminal-color-scheme/IMPLEMENTATION.md`

## Build Verification

### Build Command (TypeScript)
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Build Command (Rust)
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --manifest-path src-tauri/Cargo.toml"
```

### Expected Result
- Exit code: 0
- No type errors (TypeScript)
- No compilation errors (Rust)

## Test Verification

### Test Command (TypeScript)
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### Test Command (Rust)
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `generateCopyName()` with no existing copies | Returns `{name}_copy_1` | Unit |
| TS-2 | `generateCopyName()` with existing copies | Increments N correctly | Unit |
| TS-3 | `hexToRgb()` parses valid `#RRGGBB` | Correct Rgb values | Unit |
| TS-4 | `rgbToHex()` formats Rgb to `#rrggbb` | Correct hex string | Unit |
| TS-5 | Hex conversion round-trip | `rgbToHex(hexToRgb(hex)) === hex` | Unit |
| TS-6 | User scheme CRUD: create from preset | New scheme with preset colors + auto name | Unit |
| TS-7 | User scheme CRUD: update color | Scheme color updated in place | Unit |
| TS-8 | User scheme CRUD: delete | Scheme removed from array | Unit |
| TS-9 | User scheme CRUD: duplicate | New scheme with source colors | Unit |
| TS-10 | Select options: presets first, users second | Correct order | Unit |
| TS-11 | Select options: user schemes have `[User]` suffix | Labels formatted correctly | Unit |
| TS-12 | Auto-copy triggers only for presets | No copy when editing user scheme | Unit |
| TS-13 | Rename: valid name updates scheme | Name changed | Unit |
| TS-14 | Rename: empty string rejected | Operation fails | Unit |
| TS-15 | Rename: duplicate name rejected | Operation fails | Unit |
| TS-16 | `validateHexColor` accepts valid hex | Returns true | Unit |
| TS-17 | `validateHexColor` rejects invalid | Returns false | Unit |
| TS-18 | Apply user scheme sets CSS variables | All 20 vars set | Unit |
| TS-19 | Apply user scheme notifies renderers | Notification sent | Unit |
| TS-20 | Apply unknown scheme falls back to preset | Existing behavior | Unit |
| TS-21 | Apply emterm/default clears CSS vars | Existing behavior preserved | Unit |
| TS-22 | Rust: missing `custom_color_schemes` → empty vec | Deserialization succeeds | Unit (Rust) |
| TS-23 | Rust: null `custom_color_schemes` → empty vec | Deserialization succeeds | Unit (Rust) |
| TS-24 | Rust: `UserColorScheme` round-trip | Serialize → deserialize matches | Unit (Rust) |
| TS-25 | Rust: settings with schemes save/load correctly | Data preserved | Unit (Rust) |

## Code Quality Verification

### Format Check (TypeScript)
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Build Check (Rust)
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"
```

## File Structure Verification

### Files to Create
- `src/settings/color-scheme-editor.ts` - Palette editor UI + scheme CRUD logic
- `src/settings/color-scheme-editor.test.ts` - Tests for CRUD logic and utilities

### Files to Modify
- `src-tauri/src/commands/config.rs` - Add UserColorScheme struct + custom_color_schemes field
- `src/settings/types.ts` - Add UserColorScheme interface + field to AppSettings
- `src/terminal/colors.ts` - Add hexToRgb, rgbToHex utilities
- `src/settings/settings-applier.ts` - Extend to support user schemes
- `src/settings/settings-applier.test.ts` - Add user scheme tests
- `src/settings/settings-sections.ts` - Integrate color editor
- `src/i18n/locales/en.json` - Add color editor labels
- `src/i18n/locales/ja.json` - Add color editor labels

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | All functional requirements implemented and tested | Run all unit tests, verify all pass |
| SC-2 | All test scenarios pass | `bun test` + `cargo test` exit 0 |
| SC-3 | Backward compatibility with existing settings.json | TS-22, TS-23 tests pass |
| SC-4 | Color changes render in real-time on terminal | Manual: edit color, observe terminal |
| SC-5 | User schemes persist across app restarts | Manual: save scheme, restart app, verify |
| SC-6 | Code review completed | PR review process |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1: Inline palette editor | Phase 4 | Manual: verify palette renders below select |
| FR2: Color picker + HEX input | Phase 4 | Manual: verify both input types work |
| FR3: Auto-copy on preset edit | Phase 2 + 4 | TS-6, TS-12 tests + manual |
| FR4: Edit user scheme in place | Phase 2 + 4 | TS-7 test + manual |
| FR5: Real-time preview | Phase 3 + 4 | TS-18, TS-19 tests + manual |
| FR6: Storage in settings.json | Phase 1 + 2 | TS-22–TS-25 tests |
| FR7: Presets first, users second | Phase 2 | TS-10 test |
| FR8: `[User]` suffix display | Phase 2 | TS-11 test |
| FR9: Delete button for user schemes only | Phase 4 | Manual: verify visibility |
| FR10: Duplicate button for all | Phase 4 | Manual: verify visibility |
| FR11: Rename for user schemes only | Phase 4 | TS-13–TS-15 tests + manual |

## E2E Testing (Docker)

### Setup
- Docker compose: `docker-compose.e2e.yml`
- Run TypeScript tests: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- Run Rust tests: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`

### Basic Functionality
- [ ] All TypeScript unit tests pass in Docker
- [ ] All Rust unit tests pass in Docker
- [ ] TypeScript type check passes in Docker
- [ ] Rust compilation succeeds in Docker

### Edge Cases
- [ ] Empty custom_color_schemes loads correctly (Rust test)
- [ ] Null custom_color_schemes loads correctly (Rust test)
- [ ] Duplicate name generation fills gaps correctly (TS test)
- [ ] Rename validation prevents empty and duplicate names (TS test)
- [ ] HEX validation rejects invalid formats (TS test)

## Manual Testing (E2E Not Possible)

Items requiring Tauri WebView runtime (cannot be automated in Docker):

### Color Palette UI
- [ ] Palette renders inline below Terminal Color Scheme select in settings panel
- [ ] 4 special colors (foreground, background, cursor, selection) display as labeled rows
- [ ] 16 ANSI colors display in two grid rows (8 standard + 8 bright)
- [ ] Color picker (`input type="color"`) opens native OS color dialog
- [ ] HEX text input shows current color as `#RRGGBB`
- [ ] Color picker and HEX input stay synchronized bidirectionally

### Auto-Copy Flow
- [ ] Select a preset (e.g., "Dracula") → palette shows preset colors
- [ ] Edit any color → select box auto-switches to `dracula_copy_1 [User]`
- [ ] Edit another color → stays on same user scheme (no second copy)
- [ ] Terminal updates in real-time during editing

### Scheme Management
- [ ] User scheme shows "Delete" and "Duplicate" buttons
- [ ] Preset shows only "Duplicate" button
- [ ] Delete removes user scheme and reverts to "emterm"
- [ ] Duplicate creates new `{name}_copy_N` scheme
- [ ] Rename field allows changing user scheme name
- [ ] Renamed scheme reflected in select box with `[User]` suffix

### Persistence
- [ ] Close and reopen settings → user schemes still present
- [ ] Restart application → user schemes still present
- [ ] User scheme colors match saved values after reload

### Select Box Display
- [ ] Presets listed first in fixed order (emterm, solarized-dark, etc.)
- [ ] User schemes listed after presets
- [ ] User schemes show `{name} [User]` suffix
- [ ] Selecting a user scheme shows its palette correctly

## Performance Verification

### Real-time Preview
- Color change to terminal render should be within a single frame (< 16ms)
- Verification: Manually observe no perceptible lag when changing colors via picker

## Security Verification

### Input Validation
- [ ] HEX color input validates against `#RRGGBB` pattern
- [ ] Invalid HEX values are rejected (not saved)
- [ ] No user-provided strings used in innerHTML (CSS setProperty only)

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | ✅ | ✅ | - |
| Tests | 25 | ✅ | ✅ | - |
| Code Quality | 2 | ✅ | ✅ | - |
| File Structure | 10 | ✅ | - | - |
| SPEC Compliance | 6 | Partial | - | ✅ |
| UI/UX | 17 | - | - | ✅ |
| Performance | 1 | - | - | ✅ |
| Security | 3 | Partial | ✅ | - |

**Total**: 29 automated items (Docker), 18 manual items
