# Feature: Font Family Settings - 3-field Split

## Overview

Replace the single `font_family` setting with three separate fields: `font_family_primary`, `font_family_secondary`, and `font_family_emoji`. Build a CSS font-family fallback chain from these fields.

## Objectives

- Allow independent configuration of primary (alphanumeric), secondary (CJK), and emoji fonts
- Build font-family fallback chain: `{primary}, {emoji}, {secondary}, monospace`
- Maintain backward compatibility with existing `font_family` field
- Provide three separate text inputs in the settings UI

---

## Data Structure Changes

### TypeScript (`src/settings/types.ts`)

Replace:
```typescript
font_family: string;
```

With:
```typescript
font_family_primary: string;
font_family_secondary: string;
font_family_emoji: string;
```

### Rust (`src-tauri/src/commands/config.rs`)

Replace:
```rust
pub font_family: String,
```

With:
```rust
pub font_family_primary: String,
pub font_family_secondary: String,
pub font_family_emoji: String,
```

All three fields use `#[serde(default, deserialize_with = "deserialize_null_default")]` (empty string default).

### Backward Compatibility

Add a `font_family` field with `#[serde(default)]` that is read during deserialization but not serialized. In a post-deserialization step or via a custom deserializer, if `font_family` is non-empty and `font_family_primary` is empty, copy `font_family` to `font_family_primary`.

---

## Font Family Chain Builder

### Location

`src/settings/settings-applier.ts`

### Function

```typescript
function buildFontFamilyChain(primary: string, emoji: string, secondary: string): string
```

### Logic

1. Start with empty array
2. If `primary` is non-empty, push `primary`
3. If `emoji` is non-empty, push `emoji`
4. If `secondary` is non-empty, push `secondary`
5. Push `"monospace"`
6. Join with `, `
7. Return the result

### Examples

| primary | emoji | secondary | Result |
|---------|-------|-----------|--------|
| `""` | `""` | `""` | `monospace` |
| `Fira Code` | `""` | `""` | `Fira Code, monospace` |
| `Fira Code` | `""` | `Noto Sans JP` | `Fira Code, Noto Sans JP, monospace` |
| `JetBrains Mono` | `Noto Color Emoji` | `Noto Sans JP` | `JetBrains Mono, Noto Color Emoji, Noto Sans JP, monospace` |
| `""` | `Noto Color Emoji` | `Noto Sans JP` | `Noto Color Emoji, Noto Sans JP, monospace` |

---

## Settings Applier Changes

### File: `src/settings/settings-applier.ts`

Replace `applyFontFamily(fontFamily: string)` with:

```typescript
export function applyFontFamily(primary: string, emoji: string, secondary: string): void {
  const chain = buildFontFamilyChain(primary, emoji, secondary);
  const root = document.documentElement;
  if (chain !== "monospace") {
    root.style.setProperty("--terminal-font-family", chain);
  } else {
    root.style.removeProperty("--terminal-font-family");
  }
  notifyRenderers("fontFamily", chain);
}
```

Update `applySettings()` call:
```typescript
applyFontFamily(settings.font_family_primary, settings.font_family_emoji, settings.font_family_secondary);
```

### RendererSettings Interface

No change needed. `fontFamily` remains a single `string` in `RendererSettings` - the chain is built before passing to renderers.

---

## Settings Panel UI Changes

### File: `src/settings/settings-panel.ts`

Replace the single font-family text input with three inputs:

1. **Primary Font**
   - key: `font-family-primary`
   - value: `this.currentSettings.font_family_primary`
   - placeholder: `monospace (default)`
   - onSave: rebuild chain and apply

2. **Secondary Font**
   - key: `font-family-secondary`
   - value: `this.currentSettings.font_family_secondary`
   - placeholder: (empty)
   - hint: "Optional"
   - onSave: rebuild chain and apply

3. **Emoji Font**
   - key: `font-family-emoji`
   - value: `this.currentSettings.font_family_emoji`
   - placeholder: (empty)
   - hint: "Optional"
   - onSave: rebuild chain and apply

Each input's `onSave` handler:
1. Update `this.currentSettings` with the new value
2. Call `applyFontFamily(primary, emoji, secondary)` with all three current values
3. Save the specific field via `this.saveSetting(fieldName, value)`

---

## i18n Changes

### File: `src/i18n/locales/en.json`

Replace:
```json
"fontFamily": "Font Family",
"fontFamilyPlaceholder": "monospace (default)",
"fontFamilyHint": "CSS font-family value",
"fontFamilyDesc": "Sets the font used for terminal text"
```

With:
```json
"fontFamilyPrimary": "Primary Font",
"fontFamilyPrimaryPlaceholder": "monospace (default)",
"fontFamilyPrimaryHint": "Alphanumeric font",
"fontFamilyPrimaryDesc": "Sets the font for alphanumeric characters",
"fontFamilySecondary": "Secondary Font",
"fontFamilySecondaryPlaceholder": "",
"fontFamilySecondaryHint": "Optional - CJK font",
"fontFamilySecondaryDesc": "Sets the font for CJK characters (Japanese, Chinese, Korean)",
"fontFamilyEmoji": "Emoji Font",
"fontFamilyEmojiPlaceholder": "",
"fontFamilyEmojiHint": "Optional - Emoji font",
"fontFamilyEmojiDesc": "Sets the font for emoji characters"
```

### File: `src/i18n/locales/ja.json`

Replace:
```json
"fontFamily": "フォントファミリー",
"fontFamilyPlaceholder": "monospace (デフォルト)",
"fontFamilyHint": "CSS font-family値",
"fontFamilyDesc": "ターミナルのテキストに使用するフォントを設定します"
```

With:
```json
"fontFamilyPrimary": "プライマリフォント",
"fontFamilyPrimaryPlaceholder": "monospace (デフォルト)",
"fontFamilyPrimaryHint": "英数字フォント",
"fontFamilyPrimaryDesc": "英数字に使用するフォントを設定します",
"fontFamilySecondary": "セカンダリフォント",
"fontFamilySecondaryPlaceholder": "",
"fontFamilySecondaryHint": "任意 - 日本語フォント",
"fontFamilySecondaryDesc": "日本語・中国語・韓国語の文字に使用するフォントを設定します",
"fontFamilyEmoji": "絵文字フォント",
"fontFamilyEmojiPlaceholder": "",
"fontFamilyEmojiHint": "任意 - 絵文字フォント",
"fontFamilyEmojiDesc": "絵文字に使用するフォントを設定します"
```

---

## Canvas Renderer

### File: `src/terminal/canvas-renderer.ts`

No changes to the renderer itself. The `setFontFamily()` method already accepts a complete font-family string and passes it to canvas context. The chain building happens in `settings-applier.ts` before reaching the renderer.

---

## Files to Modify

| File | Change |
|------|--------|
| `src/settings/types.ts` | Replace `font_family` with three fields |
| `src-tauri/src/commands/config.rs` | Replace `font_family` with three fields + migration |
| `src/settings/settings-applier.ts` | Add `buildFontFamilyChain()`, update `applyFontFamily()` |
| `src/settings/settings-panel.ts` | Replace single input with three inputs |
| `src/i18n/locales/en.json` | Replace font family labels |
| `src/i18n/locales/ja.json` | Replace font family labels |

---

## Test Approach

### Unit Tests

- `buildFontFamilyChain()`: all combinations of empty/non-empty fields
- Backward compatibility: `font_family` migration to `font_family_primary`

### Acceptance Criteria

- [ ] Setting primary font updates terminal display for alphanumeric characters
- [ ] Setting secondary font provides fallback for CJK characters
- [ ] Setting emoji font provides fallback for emoji
- [ ] Empty fields are omitted from the fallback chain
- [ ] All fields empty falls back to `monospace`
- [ ] Existing `font_family` setting migrates to `font_family_primary`
- [ ] Three text inputs appear in settings UI
- [ ] i18n labels display correctly in English and Japanese
