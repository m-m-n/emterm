# Feature: UI Pink (Sakura) Color Preset

## Overview

Add a "Pink" color preset to the UI theme system. The preset uses a cherry blossom (sakura) pink palette, providing a warm and soft aesthetic. It follows the same Material Design 3 token structure as existing presets (Purple, Blue, Green, Orange).

## Objectives

- Add a fifth color preset option to both the UI theme and Markdown viewer theme
- Use a cherry blossom (sakura) pink color palette
- Maintain consistency with the existing MD3 token-based preset system

## Technical Requirements

### Functional Requirements

- **FR1: TypeScript UiThemePreset type** - Add `"pink"` to the `UiThemePreset` union type.
- **FR2: UI theme preset colors** - Define dark and light MD3 color tokens (19 tokens each) for the pink preset.
- **FR3: Markdown theme preset colors** - Define dark and light Markdown color palettes (11 colors each) for the pink preset.
- **FR4: Rust UiThemePreset enum** - Add `Pink` variant to the Rust `UiThemePreset` enum with serde support.
- **FR5: Settings UI dropdown** - Add the pink option to both UI theme preset and Markdown theme preset dropdown menus.
- **FR6: i18n labels** - Add translation keys for the pink preset label in English ("Pink") and Japanese ("ピンク").

### Non-Functional Requirements

- **NFR1 - Visual harmony:** The sakura pink palette must provide sufficient contrast ratios for readability in both dark and light modes.
- **NFR2 - Consistency:** The pink preset follows the same MD3 token structure and naming conventions as existing presets.

## Implementation Approach

### Files to Modify

| File | Change |
|------|--------|
| `src/settings/types.ts` | Add `"pink"` to `UiThemePreset` union type |
| `src/settings/ui-theme-presets.ts` | Add `pink` entry to `UI_THEME_PRESETS` record |
| `src/settings/markdown-theme-presets.ts` | Add `pink` entry to `MARKDOWN_THEME_PRESETS` record |
| `src-tauri/src/commands/config.rs` | Add `Pink` variant to `UiThemePreset` enum; update tests |
| `src/settings/settings-sections.ts` | Add pink option to 2 preset dropdown menus |
| `src/i18n/locales/en.json` | Add `presetPink` keys (appearance + markdownViewer sections) |
| `src/i18n/locales/ja.json` | Add `presetPink` keys (appearance + markdownViewer sections) |

### Color Palette (Sakura Pink)

#### UI Theme Tokens (MD3)

**Dark:**

| Token | Value | Purpose |
|-------|-------|---------|
| primary | `#FFB1C8` | Primary sakura pink |
| onPrimary | `#5E1133` | Text on primary |
| primaryContainer | `#7B2949` | Primary container |
| onPrimaryContainer | `#FFD9E3` | Text on primary container |
| secondary | `#E3BDC6` | Muted pink secondary |
| onSecondary | `#422931` | Text on secondary |
| secondaryContainer | `#5B3F47` | Secondary container |
| onSecondaryContainer | `#FFD9E2` | Text on secondary container |
| surface | `#1A1114` | Dark surface with pink tint |
| surfaceContainer | `#271D21` | Container surface |
| surfaceContainerLow | `#221820` | Low container |
| surfaceContainerHigh | `#322830` | High container |
| surfaceContainerHighest | `#3D333A` | Highest container |
| onSurface | `#F0DEE2` | Text on surface |
| onSurfaceVariant | `#D4BFC5` | Variant text |
| outline | `#9D8A90` | Outline |
| outlineVariant | `#514349` | Variant outline |
| error | `#F2B8B5` | Error (standard) |
| onError | `#601410` | Text on error (standard) |

**Light:**

| Token | Value | Purpose |
|-------|-------|---------|
| primary | `#984061` | Sakura-toned primary |
| onPrimary | `#FFFFFF` | Text on primary |
| primaryContainer | `#FFD9E3` | Light pink container |
| onPrimaryContainer | `#3E001D` | Text on container |
| secondary | `#74565F` | Muted secondary |
| onSecondary | `#FFFFFF` | Text on secondary |
| secondaryContainer | `#FFD9E2` | Secondary container |
| onSecondaryContainer | `#2B151C` | Text on secondary container |
| surface | `#FFF8F8` | Light surface with pink tint |
| surfaceContainer | `#FAECEF` | Container surface |
| surfaceContainerLow | `#FDF0F2` | Low container |
| surfaceContainerHigh | `#F2E4E8` | High container |
| surfaceContainerHighest | `#EBDEE2` | Highest container |
| onSurface | `#22191C` | Text on surface |
| onSurfaceVariant | `#514349` | Variant text |
| outline | `#837379` | Outline |
| outlineVariant | `#D4BFC5` | Variant outline |
| error | `#B3261E` | Error (standard) |
| onError | `#FFFFFF` | Text on error (standard) |

#### Markdown Theme Colors

**Dark:**

| Property | Value | Derivation |
|----------|-------|------------|
| bg | `#1A1114` | = surface |
| fg | `#F0DEE2` | = onSurface |
| heading | `#FFFFFF` | White (same as other presets) |
| link | `#FFB1C8` | = primary |
| border | `#514349` | = outlineVariant |
| blockquote | `#D4BFC5` | = onSurfaceVariant |
| codeBg | `#322830` | = surfaceContainerHigh |
| codeFg | `#F0DEE2` | = onSurface |
| preBg | `#221820` | = surfaceContainerLow |
| tableBg | `#271D21` | = surfaceContainer |
| tableStripe | `#322830` | = surfaceContainerHigh |

**Light:**

| Property | Value | Derivation |
|----------|-------|------------|
| bg | `#FFF8F8` | = surface |
| fg | `#22191C` | = onSurface |
| heading | `#3E001D` | = onPrimaryContainer |
| link | `#984061` | = primary |
| border | `#D4BFC5` | = outlineVariant |
| blockquote | `#514349` | = onSurfaceVariant |
| codeBg | `#F2E4E8` | = surfaceContainerHigh |
| codeFg | `#22191C` | = onSurface |
| preBg | `#FAECEF` | = surfaceContainer |
| tableBg | `#FDF0F2` | = surfaceContainerLow |
| tableStripe | `#F2E4E8` | = surfaceContainerHigh |

### i18n Keys

| Key | en | ja |
|-----|----|----|
| `settings.appearance.presetPink` | Pink | ピンク |
| `settings.markdownViewer.presetPink` | Pink | ピンク |

## Test Scenarios

### Rust Tests

- [ ] `UiThemePreset::Pink` deserializes from `"pink"`
- [ ] `UiThemePreset::Pink` serializes to `"pink"`
- [ ] `UiThemePreset::Pink` round-trips correctly
- [ ] Default preset remains `Purple` (no regression)

### TypeScript Type Check

- [ ] `tsc --noEmit` passes with new `"pink"` union member
- [ ] All preset records include pink key (enforced by `Record<UiThemePreset, ...>`)

### Manual Verification

- [ ] Settings panel shows "Pink" / "ピンク" in UI theme preset dropdown
- [ ] Settings panel shows "Pink" / "ピンク" in Markdown theme preset dropdown
- [ ] Dark mode: pink UI has readable text and sufficient contrast
- [ ] Light mode: pink UI has readable text and sufficient contrast
- [ ] Dark mode: Markdown viewer renders correctly with pink preset
- [ ] Light mode: Markdown viewer renders correctly with pink preset

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented
- [ ] All Rust and TypeScript tests pass
- [ ] Pink preset is selectable in both UI theme and Markdown theme settings
- [ ] Both dark and light modes render correctly with the pink preset
