# Feature: ANSI Color Palette Resolution

## Overview

Fix ANSI indexed color resolution to use the active color scheme's palette instead of a hardcoded static palette, and implement "bold brightens ANSI colors" behavior where bold text with standard ANSI foreground colors (indices 0-7) displays using their bright variants (indices 8-15).

## Objectives

- Indexed colors (SGR 30-37, 40-47, 90-97, 100-107) resolve against the dynamic color scheme palette
- Bold attribute + standard foreground color (0-7) automatically uses the bright variant (8-15)
- Bold-brightens behavior is configurable via settings (default: ON)

## Technical Requirements

### Functional Requirements

- **FR1:** `colorToRgb()` accepts an optional palette parameter for dynamic indexed color lookup
- **FR2:** `getEffectiveForeground()` and `getEffectiveBackground()` accept an optional palette parameter and forward it to `colorToRgb()`
- **FR3:** `CanvasRenderer` maintains a full 256-color palette (`currentPalette256`) built from `currentPalette16` + static entries 16-255, rebuilt on color scheme change
- **FR4:** All `getEffectiveForeground()` / `getEffectiveBackground()` call sites in `CanvasRenderer` pass `currentPalette256`
- **FR5:** `getEffectiveForeground()` implements bold-brightens logic: when `attrs.bold` is true and the resolved foreground is an indexed color with index 0-7, substitute index 8-15 from the palette
- **FR6:** Bold-brightens applies to foreground color only (not background)
- **FR7:** Bold-brightens applies after reverse attribute processing (i.e., on the effective foreground, not the raw `attrs.fg`)
- **FR8:** New setting `bold_brightens_ansi_colors: bool` (default: `true`) controls FR5 behavior
- **FR9:** `StyleCache` methods accept an optional palette parameter for correct indexed color resolution

### Non-Functional Requirements

- **NFR1 - Performance:** No measurable rendering performance regression. The palette lookup is O(1) array access.
- **NFR2 - Compatibility:** Existing callers without palette parameter fall back to the static `PALETTE_256` (backward compatible via optional parameter)

## Implementation Approach

### Data Flow

```
SGR 31 (red fg, bold)
  -> WASM: PackedColor::indexed(1), flags |= STYLE_BOLD
  -> Binary transfer: { type: "indexed", index: 1 }, bold: true
  -> getEffectiveForeground(attrs, defaultFg, defaultBg, palette)
     -> bold + indexed(1) + bold_brightens ON -> palette[1+8] = palette[9]
  -> Result: bright red from active color scheme
```

### Affected Files

| File | Change |
|------|--------|
| `src/terminal/colors.ts` | Add `buildPalette256()` helper |
| `src/terminal/attributes.ts` | Add palette param to `colorToRgb`, `getEffectiveForeground`, `getEffectiveBackground`; add bold-brightens logic |
| `src/terminal/canvas-renderer.ts` | Add `currentPalette256` field, rebuild on scheme change, pass palette to all color resolution calls |
| `src/terminal/style-cache.ts` | Add palette param to `getClass`, `hashAttributes`, `generateCSSRule` |
| `src-tauri/src/settings.rs` | Add `bold_brightens_ansi_colors: bool` field |
| `src/settings/types.ts` | Add `bold_brightens_ansi_colors` to `AppSettings` |
| `src/settings/settings-applier.ts` | Apply bold-brightens setting to renderer |
| `src-tauri/locales/{en,ja}.json` | Add i18n key for setting label |
| `src/i18n/locales/{en,ja}.json` | Add frontend i18n key |
| `src/settings/settings-sections.ts` | Add toggle to appearance section |

### Bold-Brightens Logic (FR5/FR7)

In `getEffectiveForeground()`, after resolving the effective color (accounting for reverse):

```
if bold_brightens_enabled AND color.type == "indexed" AND color.index < 8:
    use palette[color.index + 8] instead of palette[color.index]
```

This must happen BEFORE the `colorToRgb()` call resolves the index to RGB, so the bright index is looked up in the palette.

### Settings Integration

Follow existing settings pattern:
- Rust: `serde(default = "default_true")` + `deserialize_null_with!` macro
- TypeScript: `bold_brightens_ansi_colors: boolean` in `AppSettings`
- Setting category: Appearance

## Test Scenarios

### Unit Tests

- [ ] `colorToRgb` with palette returns palette color for indexed(1) instead of static PALETTE_256[1]
- [ ] `colorToRgb` without palette falls back to static PALETTE_256
- [ ] `getEffectiveForeground` with bold + indexed(1) + bold_brightens returns palette[9]
- [ ] `getEffectiveForeground` with bold + indexed(1) + bold_brightens OFF returns palette[1]
- [ ] `getEffectiveForeground` with bold + indexed(8) (already bright) does NOT double-brighten to 16
- [ ] `getEffectiveForeground` with bold + rgb color is unaffected by bold-brightens
- [ ] `getEffectiveForeground` with bold + indexed(1) + reverse uses effective fg (was bg) for brightening
- [ ] `buildPalette256` produces 256 entries with first 16 from input, rest from static

## Success Criteria

- [ ] tmux `status-style fg=red` with bold shows bright red from active color scheme
- [ ] Changing color scheme updates indexed colors in rendered output
- [ ] Setting `bold_brightens_ansi_colors` to false disables bold-to-bright conversion
- [ ] All existing tests pass without modification (backward compatible)
