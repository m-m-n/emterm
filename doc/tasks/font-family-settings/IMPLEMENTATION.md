# Implementation Plan: Font Family Settings - 3-field Split

## Overview

Replace the single `font_family` setting with three separate fields (`font_family_primary`, `font_family_secondary`, `font_family_emoji`) and build a CSS font-family fallback chain from them.

## Objectives

- Allow independent configuration of primary (alphanumeric), secondary (CJK), and emoji fonts
- Build font-family fallback chain: `{primary}, {emoji}, {secondary}, monospace`
- Maintain backward compatibility with existing `font_family` field in config files
- Provide three separate text inputs in the settings UI

## Prerequisites

### Development Environment
- Rust toolchain (for Tauri backend)
- Bun (for TypeScript frontend)

### Dependencies
- No new external dependencies required

### Knowledge Requirements
- serde deserialization patterns in `config.rs` (null-safe defaults, migration)
- Settings applier pattern (CSS variables + renderer notification)
- Settings panel text input rendering (`renderTextInput` method)
- i18n key structure

## Architecture Overview

### Technology Stack
- **Backend**: Rust (serde serialization/deserialization)
- **Frontend**: Vanilla TypeScript
- **Styling**: CSS custom properties

### Design Approach
- Data flows from config file -> Rust struct -> JSON -> TypeScript interface -> settings-applier -> CSS/renderer
- The font-family chain is built in the frontend (`settings-applier.ts`), not the backend
- `RendererSettings.fontFamily` remains a single string; the chain is assembled before passing to renderers
- Backward compatibility is handled at the Rust deserialization level

### Component Interaction

```
Config File (JSON)
  |
  v
config.rs (AppSettings) -- migration: font_family -> font_family_primary
  |
  v
Frontend (AppSettings interface in types.ts)
  |
  v
settings-applier.ts -- buildFontFamilyChain() assembles chain
  |
  +-> CSS variable: --terminal-font-family
  +-> notifyRenderers("fontFamily", chain)
        |
        v
      CanvasRenderer.setFontFamily(chain) -- no changes needed
```

## Implementation Phases

### Phase 1: Data Structure and Backend Migration

**Goal**: Replace `font_family` with three new fields in both TypeScript and Rust, with backward-compatible migration for existing config files.

**Files to Create**: None

**Files to Modify**:
- `src/settings/types.ts`:
  - Remove `font_family` field from `AppSettings`
  - Add `font_family_primary`, `font_family_secondary`, `font_family_emoji` (all `string`)
- `src-tauri/src/commands/config.rs`:
  - Remove `font_family` field from `AppSettings` struct (replace with three new fields)
  - Add `font_family_primary`, `font_family_secondary`, `font_family_emoji` with `#[serde(default, deserialize_with = "deserialize_null_default")]`
  - Add a legacy `font_family` field with `#[serde(default)]` that is deserialized but not serialized (`#[serde(skip_serializing)]`)
  - Implement post-deserialization migration: if `font_family` is non-empty and `font_family_primary` is empty, copy `font_family` into `font_family_primary`
  - Update `Default for AppSettings` to use three empty strings instead of one
  - Update existing tests that reference `font_family`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings (TS) | Hold three font family fields | N/A | Interface matches Rust struct |
| AppSettings (Rust) | Serialize/deserialize three fields + migration | Config file may have old `font_family` | Three fields populated; `font_family` migrated if present |
| Migration logic | Copy legacy `font_family` to `font_family_primary` | Old config loaded | `font_family_primary` contains the legacy value |

**Processing Flow**:
```
1. Deserialize JSON into AppSettings
   +-- font_family present AND font_family_primary empty -> copy font_family to font_family_primary
   +-- font_family absent OR font_family_primary non-empty -> no migration needed
2. Serialize AppSettings to JSON
   +-- font_family field is skipped (not serialized)
   +-- Three new fields are always written
```

**Implementation Steps**:

1. **Update TypeScript AppSettings interface**
   - Replace single `font_family` with three fields
   - All three default to empty string

2. **Update Rust AppSettings struct**
   - Add three new fields with serde attributes
   - Add legacy `font_family` with skip_serializing
   - Implement migration in `load_settings` (after deserialization, before returning)

3. **Update Rust tests**
   - Modify default value test to check three fields
   - Add migration test: JSON with `font_family` only -> `font_family_primary` populated
   - Add migration test: JSON with both `font_family` and `font_family_primary` -> `font_family_primary` wins
   - Update round-trip test for three fields

**Dependencies**:
- Requires: Nothing (foundational phase)
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests (Rust)*:
- Default value assertions for three new fields (all empty strings)
- Deserialization of `{}` -> three empty strings
- Deserialization of `{"font_family": "Fira Code"}` -> `font_family_primary` = "Fira Code"
- Deserialization of `{"font_family": "X", "font_family_primary": "Y"}` -> `font_family_primary` = "Y"
- Round-trip serialization: `font_family` is NOT in the output JSON
- Null handling: `{"font_family_primary": null}` -> empty string

*Type Check*:
- `bun run typecheck` passes with no errors

**Acceptance Criteria**:
- [ ] TypeScript `AppSettings` has three font family fields, no single `font_family`
- [ ] Rust `AppSettings` has three font family fields with proper serde attributes
- [ ] Legacy `font_family` migrates to `font_family_primary` on load
- [ ] `font_family` is not serialized when saving
- [ ] All existing Rust tests pass (updated for new structure)
- [ ] TypeScript type check passes

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:
- **Risk**: Existing test helpers (`makeSettings`) in TypeScript tests reference `font_family`
  - **Mitigation**: Update all test helper functions in Phase 1 to prevent cascading failures
  - **Note**: `makeSettings()` in `settings-applier.test.ts` also lacks the `language` field (existing bug); fix together with the three-field migration

---

### Phase 2: Font Family Chain Builder and Settings Applier

**Goal**: Implement `buildFontFamilyChain()` function and update `applyFontFamily()` to accept three parameters, building a CSS font-family chain before applying.

**Files to Create**: None

**Files to Modify**:
- `src/settings/settings-applier.ts`:
  - Add exported `buildFontFamilyChain(primary, emoji, secondary)` function
  - Update `applyFontFamily()` signature to accept three parameters
  - Update `applySettings()` call to pass three fields
- `src/settings/settings-applier.test.ts`:
  - Add unit tests for `buildFontFamilyChain()`
  - Update existing `applyFontFamily` tests for new signature
  - Update `makeSettings` helper and `applySettings` tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| buildFontFamilyChain | Assemble CSS font-family string from three fields | Three string inputs (may be empty) | Returns comma-separated chain ending with "monospace" |
| applyFontFamily | Apply chain to CSS variable and notify renderers | Three string inputs | CSS variable set (or removed if chain = "monospace"); renderers notified |

**Processing Flow**:
```
1. buildFontFamilyChain receives (primary, emoji, secondary)
2. Build array of non-empty values in order: [primary, emoji, secondary]
3. Append "monospace" as final fallback
4. Join with ", " and return
5. applyFontFamily calls buildFontFamilyChain
6. If result is "monospace" -> remove CSS variable
7. Otherwise -> set CSS variable to the chain
8. Notify renderers with the chain string
```

**Implementation Steps**:

1. **Add buildFontFamilyChain function**
   - Pure function, no side effects
   - Filters empty strings, appends "monospace", joins

2. **Update applyFontFamily signature and logic**
   - Accept three string parameters instead of one
   - Build chain, then apply via CSS variable and renderer notification
   - Condition for removing CSS variable: chain equals "monospace" (all fields empty)

3. **Update applySettings call**
   - Pass `settings.font_family_primary`, `settings.font_family_emoji`, `settings.font_family_secondary`

4. **Write unit tests for buildFontFamilyChain**
   - Cover all combinations from SPEC.md examples table

**Dependencies**:
- Requires: Phase 1 (TypeScript types must have three fields)
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests (TypeScript)*:

Test table for `buildFontFamilyChain`:

| primary | emoji | secondary | Expected |
|---------|-------|-----------|----------|
| `""` | `""` | `""` | `"monospace"` |
| `"Fira Code"` | `""` | `""` | `"Fira Code, monospace"` |
| `"Fira Code"` | `""` | `"Noto Sans JP"` | `"Fira Code, Noto Sans JP, monospace"` |
| `"JetBrains Mono"` | `"Noto Color Emoji"` | `"Noto Sans JP"` | `"JetBrains Mono, Noto Color Emoji, Noto Sans JP, monospace"` |
| `""` | `"Noto Color Emoji"` | `"Noto Sans JP"` | `"Noto Color Emoji, Noto Sans JP, monospace"` |
| `""` | `""` | `"Noto Sans JP"` | `"Noto Sans JP, monospace"` |
| `""` | `"Noto Color Emoji"` | `""` | `"Noto Color Emoji, monospace"` |

Test items for updated `applyFontFamily`:
- Three empty strings -> CSS variable removed, renderers notified with "monospace" (note: previously notified with empty string "")
- Primary only -> CSS variable set to "Fira Code, monospace", renderers notified
- All three filled -> CSS variable set to full chain, renderers notified
- **Note**: Existing tests that expected empty string `""` in renderer notification must be updated to expect `"monospace"`

Test items for updated `applySettings`:
- Full settings object with three font fields -> correct chain applied

**Acceptance Criteria**:
- [ ] `buildFontFamilyChain` returns correct chain for all combinations
- [ ] Empty fields are omitted from the chain
- [ ] "monospace" is always the final fallback
- [ ] All fields empty results in CSS variable being removed
- [ ] Renderers receive the assembled chain string
- [ ] All existing tests pass (updated for new signature)

**Estimated Effort**: Small (1 day)

---

### Phase 3: Settings Panel UI and i18n

**Goal**: Replace the single font-family text input with three separate inputs in the settings panel, and update i18n labels for both English and Japanese.

**Files to Create**: None

**Files to Modify**:
- `src/settings/settings-panel.ts`:
  - Remove single `font-family` text input
  - Add three text inputs: `font-family-primary`, `font-family-secondary`, `font-family-emoji`
  - Each input's `onSave` handler updates `currentSettings`, calls `applyFontFamily` with all three current values, and saves the specific field
- `src/i18n/locales/en.json`:
  - Remove `fontFamily`, `fontFamilyPlaceholder`, `fontFamilyHint`, `fontFamilyDesc`
  - Add keys for primary, secondary, emoji (label, placeholder, hint, description)
- `src/i18n/locales/ja.json`:
  - Same replacements as English, with Japanese translations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Primary Font input | Edit `font_family_primary` | Settings loaded | Value saved, chain rebuilt and applied |
| Secondary Font input | Edit `font_family_secondary` | Settings loaded | Value saved, chain rebuilt and applied |
| Emoji Font input | Edit `font_family_emoji` | Settings loaded | Value saved, chain rebuilt and applied |
| i18n labels (en/ja) | Provide localized labels for three inputs | N/A | All input labels, hints, descriptions available in both languages |

**Processing Flow**:
```
1. User modifies any of the three font inputs
2. On save (blur/Enter):
   a. Update this.currentSettings with the changed field
   b. Call applyFontFamily(primary, emoji, secondary) with ALL three current values
   c. Save the specific changed field via saveSetting()
3. Settings panel displays three inputs in the Font subsection
   +-- Primary Font (placeholder: "monospace (default)")
   +-- Secondary Font (placeholder: empty, hint: "Optional")
   +-- Emoji Font (placeholder: empty, hint: "Optional")
```

**Implementation Steps**:

1. **Update i18n files**
   - Replace four fontFamily keys with twelve keys (four per font field)
   - English and Japanese

2. **Update settings panel**
   - Replace single `renderTextInput` call for font-family with three calls
   - Each call uses the corresponding i18n keys and settings field
   - Each `onSave` handler: update settings, call `applyFontFamily` with three fields, save via `saveSetting`

3. **Update settings-panel.test.ts**
   - Update `makeSettings` helper for three font fields
   - Update `#settings-font-family` selector references to `#settings-font-family-primary`, `#settings-font-family-secondary`, `#settings-font-family-emoji`
   - Verify three input elements are rendered in the font subsection

**Dependencies**:
- Requires: Phase 1 (TypeScript types), Phase 2 (applyFontFamily signature)
- Blocks: Nothing

**Testing Approach**:

*Unit Tests (TypeScript)*:
- Settings panel renders three text inputs for font families
- Each input has correct placeholder and hint
- `makeSettings` helper updated and tests pass

*Manual Testing*:
- [ ] Three font inputs visible in the Font subsection of Appearance settings
- [ ] Labels display correctly in English
- [ ] Labels display correctly in Japanese
- [ ] Changing primary font updates terminal display immediately
- [ ] Changing secondary font updates terminal display immediately
- [ ] Changing emoji font updates terminal display immediately
- [ ] Values persist after closing and reopening settings

**Acceptance Criteria**:
- [ ] Three text inputs appear in settings UI under the Font subsection
- [ ] i18n labels display correctly in English and Japanese
- [ ] Changing any font field rebuilds and applies the complete chain
- [ ] Each field saves independently via `saveSetting`
- [ ] All existing settings panel tests pass (updated for new structure)

**Estimated Effort**: Small (1 day)

---

## Complete File Structure

```
src/settings/
  types.ts                  # AppSettings: font_family -> 3 fields
  settings-applier.ts       # +buildFontFamilyChain(), updated applyFontFamily()
  settings-applier.test.ts  # +buildFontFamilyChain tests, updated applyFontFamily tests
  settings-panel.ts         # 1 text input -> 3 text inputs
  settings-panel.test.ts    # Updated makeSettings, new input assertions

src/i18n/locales/
  en.json                   # Replaced fontFamily keys with 3-field keys
  ja.json                   # Replaced fontFamily keys with 3-field keys

src-tauri/src/commands/
  config.rs                 # font_family -> 3 fields + migration logic

src/terminal/
  canvas-renderer.ts        # NO CHANGES (receives assembled chain)
  renderer-interface.ts     # NO CHANGES (fontFamily remains string)
```

## Testing Strategy

### Unit Testing

**Approach**:
- Bun test runner for TypeScript tests
- Cargo test for Rust tests
- Table-driven tests for `buildFontFamilyChain` (all combinations)

**Test Coverage Goals**:
- `buildFontFamilyChain`: 100% (pure function, all input combinations)
- Migration logic: 100% (three scenarios: legacy only, both fields, no legacy)
- Settings applier: Existing + updated tests

**Key Test Areas**:

1. **buildFontFamilyChain** (`src/settings/settings-applier.test.ts`)
   - All 7 combinations from SPEC.md examples
   - Edge: all empty -> "monospace"
   - Edge: only secondary -> "secondary, monospace"
   - Edge: only emoji -> "emoji, monospace"

2. **Rust migration** (`src-tauri/src/commands/config.rs`)
   - Legacy `font_family` -> `font_family_primary`
   - Both present -> `font_family_primary` takes precedence
   - Neither present -> all empty
   - `font_family` not in serialized output

3. **Settings applier integration** (`src/settings/settings-applier.test.ts`)
   - `applyFontFamily` with three parameters
   - CSS variable set/removed correctly
   - Renderer notification with chain string

### Manual Testing Checklist

- [ ] Set primary font to "Fira Code" -> alphanumeric text renders in Fira Code
- [ ] Set secondary font to "Noto Sans JP" -> Japanese text renders in Noto Sans JP
- [ ] Set emoji font to "Noto Color Emoji" -> emoji renders with specified font
- [ ] Leave all three empty -> terminal uses browser default monospace
- [ ] Set primary only -> chain is "PrimaryFont, monospace"
- [ ] Old config with `font_family: "X"` -> loads as primary font
- [ ] Save and reload -> all three values persist
- [ ] Switch language to Japanese -> labels display in Japanese
- [ ] Switch language to English -> labels display in English

## Dependencies

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Data structures (TypeScript + Rust)
2. Phase 2: Chain builder and applier (depends on Phase 1)
3. Phase 3: UI and i18n (depends on Phase 1 + Phase 2)

**Component Dependencies**:
- `settings-panel.ts` depends on `settings-applier.ts` (applyFontFamily signature)
- `settings-applier.ts` depends on `types.ts` (AppSettings interface)
- `config.rs` and TypeScript changes must be applied atomically to maintain JSON serialization compatibility

## Risk Assessment

### Technical Risks

1. **Migration edge case: malformed font_family value**
   - **Risk**: Old `font_family` might contain comma-separated values (user manually edited CSS chain)
   - **Likelihood**: Low
   - **Impact**: Low (chain still works, just all in primary)
   - **Mitigation**: Migration copies the value as-is; the chain builder handles it transparently

2. **Test helper breakage across test files**
   - **Risk**: Multiple test files have `makeSettings()` helpers that reference `font_family`
   - **Likelihood**: High (confirmed by reading test files)
   - **Impact**: Medium (tests fail until updated)
   - **Mitigation**: Update all `makeSettings` helpers in Phase 1 before proceeding

## Open Questions

### From Specification
- None (specification is complete and unambiguous)

### Implementation-Specific
- None

## Future Enhancements

Items NOT in the current specification (do not implement):
- Font preview in settings panel
- Font availability detection / validation
- Per-font-field size or weight settings

## Success Metrics

### Functional Completeness
- [ ] All three font fields configurable in settings UI
- [ ] Chain correctly assembled from three fields
- [ ] Legacy migration works for existing config files
- [ ] i18n labels in English and Japanese

### Quality Metrics
- [ ] All Rust tests pass (`cargo test`)
- [ ] All TypeScript tests pass (`bun test`)
- [ ] Type check passes (`bun run typecheck`)
- [ ] No regressions in existing font rendering

## References

- **Specification**: `doc/tasks/font-family-settings/SPEC.md`
- **Requirements**: `doc/tasks/font-family-settings/要件定義書.md`

## Next Steps

1. `/sdd.4-implement` で Phase 1 から順に実装を開始
2. 各フェーズ完了後にテストを実行して検証
