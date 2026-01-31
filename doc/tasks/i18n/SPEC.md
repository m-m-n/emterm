# Feature: Internationalization (i18n)

## Overview

Add multi-language support to eMterm. The frontend uses a lightweight custom i18n module (JSON translation files + `t()` function). The backend uses the `rust-i18n` crate. Language preference is stored in `AppSettings` and synced between frontend and backend via a Tauri command.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    Frontend (TypeScript)                  │
│                                                          │
│  src/i18n/index.ts          src/i18n/locales/en.json    │
│  ├── initI18n(locale)       src/i18n/locales/ja.json    │
│  ├── t(key, params?)                                     │
│  ├── setLocale(locale)                                   │
│  └── getLocale()                                         │
│                                                          │
│           │ invoke("set_language", { language })          │
│           ▼                                              │
├─────────────────────────────────────────────────────────┤
│                    Backend (Rust)                         │
│                                                          │
│  rust-i18n crate            src-tauri/locales/en.json   │
│  ├── t!("key")              src-tauri/locales/ja.json   │
│  ├── rust_i18n::set_locale()                             │
│  └── rust_i18n::locale()                                 │
│                                                          │
│  Tauri command: set_language(language: String)            │
└─────────────────────────────────────────────────────────┘
```

## Frontend i18n Module

### File Structure

```
src/i18n/
├── index.ts              # i18n API: initI18n, t, setLocale, getLocale
└── locales/
    ├── en.json           # English translations (base)
    └── ja.json           # Japanese translations
```

### `t()` Function API

```typescript
/**
 * Returns the translated string for the given key.
 *
 * @param key - Dot-separated key (e.g., "settings.appearance.fontSize")
 * @param params - Optional parameter map for placeholder replacement
 * @returns Translated string, or the key itself if not found
 */
function t(key: string, params?: Record<string, string | number>): string;
```

Lookup order:
1. Current locale translations
2. English (`en`) translations (fallback)
3. Return the key string as-is

Parameter replacement: `{paramName}` placeholders in the translated string are replaced with values from the `params` object.

```typescript
// Example
t("paste.message", { count: 5 });
// en.json: "You are about to paste {count} lines"
// Result:  "You are about to paste 5 lines"
```

### Other Exported Functions

```typescript
/**
 * Initializes the i18n module with the given locale.
 * Loads the translation files and sets the active locale.
 *
 * @param locale - Language code ("en", "ja") or "auto"
 */
function initI18n(locale: string): void;

/**
 * Changes the active locale and triggers re-rendering.
 *
 * @param locale - Language code ("en", "ja")
 */
function setLocale(locale: string): void;

/**
 * Returns the current active locale code.
 *
 * @returns Language code (e.g., "en", "ja")
 */
function getLocale(): string;

/**
 * Resolves "auto" to a concrete locale code using navigator.language.
 * Returns "en" if the detected language is not supported.
 *
 * @param locale - "auto" or a specific language code
 * @returns Resolved language code
 */
function resolveLocale(locale: string): string;
```

Supported locales list:

```typescript
const SUPPORTED_LOCALES = ["en", "ja"] as const;
```

`resolveLocale` logic:
1. If `locale` is not `"auto"`, return it as-is (assumes valid)
2. Read `navigator.language` (e.g., `"ja-JP"`, `"en-US"`)
3. Extract base tag: `"ja-JP"` → `"ja"`, `"en-US"` → `"en"`
4. If base tag is in `SUPPORTED_LOCALES`, return it
5. Otherwise return `"en"`

### Initialization Flow

```
App startup
  │
  ├── SettingsService.load() → AppSettings.language ("auto" | "en" | "ja")
  │
  ├── resolveLocale(language)
  │     └── "auto" → navigator.language → "en" or "ja"
  │
  ├── initI18n(resolvedLocale)
  │     └── Load en.json and ja.json
  │     └── Set active locale
  │
  └── invoke("set_language", { language: resolvedLocale })
        └── Backend: rust_i18n::set_locale(resolvedLocale)
```

### Translation JSON Structure

Key naming convention: `{component}.{section}.{item}`

```json
// src/i18n/locales/en.json
{
  "settings": {
    "categories": {
      "appearance": "Appearance",
      "terminal": "Terminal",
      "keybinds": "Keybinds"
    },
    "appearance": {
      "title": "Appearance",
      "font": "Font",
      "fontSize": "Font Size",
      "fontSizeHint": "Range: {min}-{max}pt",
      "fontFamily": "Font Family",
      "fontFamilyPlaceholder": "monospace (default)",
      "fontFamilyHint": "CSS font-family value",
      "lineHeight": "Line Height",
      "lineHeightHint": "Range: {min}-{max}",
      "themeColor": "Theme & Color",
      "uiTheme": "UI Theme",
      "uiThemeSystem": "System",
      "uiThemeLight": "Light",
      "uiThemeDark": "Dark",
      "colorScheme": "Terminal Color Scheme",
      "colorSchemeDefault": "Default",
      "opacity": "Opacity",
      "opacityHint": "Range: {min}-{max}",
      "layout": "Layout",
      "padding": "Padding",
      "paddingHint": "Range: {min}-{max}px",
      "scrollbackLines": "Scrollback Lines",
      "scrollbackLinesHint": "Range: {min}-{max}",
      "showScrollbar": "Show Scrollbar",
      "scrollbarAuto": "Auto",
      "scrollbarAlways": "Always",
      "scrollbarNever": "Never",
      "richContent": "Rich Content",
      "inlineImages": "Inline Images",
      "markdownRendering": "Markdown Rendering"
    },
    "terminal": {
      "title": "Terminal",
      "cursor": "Cursor",
      "cursorStyle": "Cursor Style",
      "cursorBlock": "Block",
      "cursorUnderline": "Underline",
      "cursorBar": "Bar",
      "cursorBlink": "Cursor Blink",
      "shell": "Shell",
      "shellPath": "Shell Path",
      "shellPathPlaceholder": "System default",
      "shellPathHint": "Applies to new tabs only",
      "shellArgs": "Shell Arguments",
      "shellArgsPlaceholder": "e.g. --login, -i",
      "shellArgsHint": "Comma-separated. Applies to new tabs only",
      "behavior": "Behavior",
      "scrollSpeed": "Scroll Speed",
      "scrollSpeedHint": "Range: {min}-{max}",
      "bellAction": "Bell Action",
      "bellVisual": "Visual",
      "bellSound": "Sound",
      "bellNone": "None",
      "urlDetection": "URL Detection",
      "copyOnSelect": "Copy on Select"
    },
    "keybinds": {
      "title": "Keybinds",
      "basic": "Basic",
      "copy": "Copy",
      "paste": "Paste",
      "selectAll": "Select All",
      "search": "Search",
      "tabManagement": "Tab Management",
      "newTab": "New Tab",
      "closeTab": "Close Tab",
      "nextTab": "Next Tab",
      "prevTab": "Previous Tab",
      "display": "Display",
      "zoomIn": "Zoom In",
      "zoomOut": "Zoom Out",
      "zoomReset": "Zoom Reset",
      "toggleFullscreen": "Toggle Fullscreen",
      "settingsSection": "Settings",
      "openSettings": "Open Settings",
      "pressKey": "Press a key..."
    },
    "language": {
      "title": "Language",
      "label": "Language",
      "auto": "Auto (System)",
      "en": "English",
      "ja": "日本語"
    }
  },
  "tabBar": {
    "terminalTabs": "Terminal tabs",
    "newTab": "New Tab",
    "newTabShortcut": "New Tab (Ctrl+T)",
    "settings": "Settings",
    "openSettings": "Open settings",
    "createNewTab": "Create new tab"
  },
  "paste": {
    "title": "Confirm Paste",
    "message": "You are about to paste {count} lines of text into the terminal.",
    "moreLines": "... and {count} more lines",
    "cancel": "Cancel",
    "paste": "Paste"
  },
  "link": {
    "title": "Open external link?",
    "cancel": "Cancel",
    "open": "Open"
  },
  "imageViewer": {
    "label": "Image Viewer",
    "info": "{width} x {height} | {mode} | {help}",
    "modeFit": "Fit",
    "helpText": "f:toggle 1:100% 0:fit Esc:close",
    "decodeError": "Failed to decode image"
  },
  "markdown": {
    "label": "Markdown Document",
    "copyCode": "Copy code",
    "copySuccess": "Copied!",
    "copyFailed": "Failed"
  },
  "zoom": {
    "closeViewer": "Close viewer",
    "zoomOut": "Zoom out",
    "zoomIn": "Zoom in",
    "resetZoom": "Reset zoom to {level}%"
  }
}
```

```json
// src/i18n/locales/ja.json
{
  "settings": {
    "categories": {
      "appearance": "外観",
      "terminal": "ターミナル",
      "keybinds": "キーバインド"
    },
    "appearance": {
      "title": "外観",
      "font": "フォント",
      "fontSize": "フォントサイズ",
      "fontSizeHint": "範囲: {min}-{max}pt",
      "fontFamily": "フォントファミリー",
      "fontFamilyPlaceholder": "monospace (デフォルト)",
      "fontFamilyHint": "CSS font-family値",
      "lineHeight": "行の高さ",
      "lineHeightHint": "範囲: {min}-{max}",
      "themeColor": "テーマとカラー",
      "uiTheme": "UIテーマ",
      "uiThemeSystem": "システム",
      "uiThemeLight": "ライト",
      "uiThemeDark": "ダーク",
      "colorScheme": "ターミナル配色",
      "colorSchemeDefault": "デフォルト",
      "opacity": "不透明度",
      "opacityHint": "範囲: {min}-{max}",
      "layout": "レイアウト",
      "padding": "パディング",
      "paddingHint": "範囲: {min}-{max}px",
      "scrollbackLines": "スクロールバック行数",
      "scrollbackLinesHint": "範囲: {min}-{max}",
      "showScrollbar": "スクロールバー表示",
      "scrollbarAuto": "自動",
      "scrollbarAlways": "常に表示",
      "scrollbarNever": "非表示",
      "richContent": "リッチコンテンツ",
      "inlineImages": "インライン画像",
      "markdownRendering": "Markdownレンダリング"
    },
    "terminal": {
      "title": "ターミナル",
      "cursor": "カーソル",
      "cursorStyle": "カーソルスタイル",
      "cursorBlock": "ブロック",
      "cursorUnderline": "アンダーライン",
      "cursorBar": "バー",
      "cursorBlink": "カーソル点滅",
      "shell": "シェル",
      "shellPath": "シェルパス",
      "shellPathPlaceholder": "システムデフォルト",
      "shellPathHint": "新しいタブにのみ適用",
      "shellArgs": "シェル引数",
      "shellArgsPlaceholder": "例: --login, -i",
      "shellArgsHint": "カンマ区切り。新しいタブにのみ適用",
      "behavior": "動作",
      "scrollSpeed": "スクロール速度",
      "scrollSpeedHint": "範囲: {min}-{max}",
      "bellAction": "ベルアクション",
      "bellVisual": "ビジュアル",
      "bellSound": "サウンド",
      "bellNone": "なし",
      "urlDetection": "URL検出",
      "copyOnSelect": "選択時にコピー"
    },
    "keybinds": {
      "title": "キーバインド",
      "basic": "基本",
      "copy": "コピー",
      "paste": "ペースト",
      "selectAll": "全選択",
      "search": "検索",
      "tabManagement": "タブ管理",
      "newTab": "新しいタブ",
      "closeTab": "タブを閉じる",
      "nextTab": "次のタブ",
      "prevTab": "前のタブ",
      "display": "表示",
      "zoomIn": "拡大",
      "zoomOut": "縮小",
      "zoomReset": "ズームリセット",
      "toggleFullscreen": "全画面切替",
      "settingsSection": "設定",
      "openSettings": "設定を開く",
      "pressKey": "キーを押してください..."
    },
    "language": {
      "title": "言語",
      "label": "言語",
      "auto": "自動 (システム)",
      "en": "English",
      "ja": "日本語"
    }
  },
  "tabBar": {
    "terminalTabs": "ターミナルタブ",
    "newTab": "新しいタブ",
    "newTabShortcut": "新しいタブ (Ctrl+T)",
    "settings": "設定",
    "openSettings": "設定を開く",
    "createNewTab": "新しいタブを作成"
  },
  "paste": {
    "title": "ペーストの確認",
    "message": "{count}行のテキストをターミナルにペーストしようとしています。",
    "moreLines": "... 他{count}行",
    "cancel": "キャンセル",
    "paste": "ペースト"
  },
  "link": {
    "title": "外部リンクを開きますか?",
    "cancel": "キャンセル",
    "open": "開く"
  },
  "imageViewer": {
    "label": "画像ビューア",
    "info": "{width} x {height} | {mode} | {help}",
    "modeFit": "フィット",
    "helpText": "f:切替 1:100% 0:フィット Esc:閉じる",
    "decodeError": "画像のデコードに失敗しました"
  },
  "markdown": {
    "label": "Markdownドキュメント",
    "copyCode": "コードをコピー",
    "copySuccess": "コピーしました!",
    "copyFailed": "失敗"
  },
  "zoom": {
    "closeViewer": "ビューアを閉じる",
    "zoomOut": "縮小",
    "zoomIn": "拡大",
    "resetZoom": "ズームを{level}%にリセット"
  }
}
```

## Backend i18n (rust-i18n)

### Crate Configuration

Add to `Cargo.toml`:

```toml
rust-i18n = "3"
sys-locale = "0.3"
```

Add to `src-tauri/src/lib.rs`:

```rust
rust_i18n::i18n!("locales", fallback = "en");
```

### Translation JSON Structure

```json
// src-tauri/locales/en.json
{
  "cli": {
    "about": "eMterm - Modern terminal emulator with rich rendering",
    "markdownAbout": "Display Markdown file in eMterm",
    "markdownFile": "Path to Markdown file",
    "imageAbout": "Display image file in eMterm",
    "imageFile": "Path to image file",
    "imageProtocol": "Image protocol to use"
  },
  "error": {
    "fileNotFound": "File not found: %{path}",
    "notAFile": "Path is not a file: %{path}",
    "fileReadError": "Failed to read file: %{error}",
    "fileTooLarge": "File size (%{size} bytes) exceeds %{maxSize} bytes limit",
    "unsupportedImageFormat": "Unsupported image format: %{format}",
    "imageDecodeError": "Failed to decode image: %{error}",
    "invalidProtocol": "Invalid protocol: %{protocol}",
    "encodingError": "Encoding error: %{error}"
  },
  "validation": {
    "fontSize": "font_size must be between %{min} and %{max}",
    "lineHeight": "line_height must be between %{min} and %{max}",
    "opacity": "opacity must be between %{min} and %{max}",
    "padding": "padding must be between %{min} and %{max}",
    "scrollbackLines": "scrollback_lines must be between %{min} and %{max}",
    "scrollSpeed": "scroll_speed must be between %{min} and %{max}"
  }
}
```

```json
// src-tauri/locales/ja.json
{
  "cli": {
    "about": "eMterm - リッチレンダリング対応モダンターミナルエミュレータ",
    "markdownAbout": "eMtermでMarkdownファイルを表示",
    "markdownFile": "Markdownファイルのパス",
    "imageAbout": "eMtermで画像ファイルを表示",
    "imageFile": "画像ファイルのパス",
    "imageProtocol": "使用する画像プロトコル"
  },
  "error": {
    "fileNotFound": "ファイルが見つかりません: %{path}",
    "notAFile": "ファイルではありません: %{path}",
    "fileReadError": "ファイルの読み込みに失敗しました: %{error}",
    "fileTooLarge": "ファイルサイズ (%{size}バイト) が%{maxSize}バイトの制限を超えています",
    "unsupportedImageFormat": "サポートされていない画像形式: %{format}",
    "imageDecodeError": "画像のデコードに失敗しました: %{error}",
    "invalidProtocol": "無効なプロトコル: %{protocol}",
    "encodingError": "エンコードエラー: %{error}"
  },
  "validation": {
    "fontSize": "font_sizeは%{min}から%{max}の範囲である必要があります",
    "lineHeight": "line_heightは%{min}から%{max}の範囲である必要があります",
    "opacity": "opacityは%{min}から%{max}の範囲である必要があります",
    "padding": "paddingは%{min}から%{max}の範囲である必要があります",
    "scrollbackLines": "scrollback_linesは%{min}から%{max}の範囲である必要があります",
    "scrollSpeed": "scroll_speedは%{min}から%{max}の範囲である必要があります"
  }
}
```

### `t!()` Macro Usage

```rust
use rust_i18n::t;

// Simple key
t!("cli.about");

// With parameters
t!("error.fileNotFound", path = path.display());
t!("validation.fontSize", min = MIN_FONT_SIZE, max = MAX_FONT_SIZE);
```

## Language Settings

### AppSettings Extension

Add to `AppSettings` struct in `src-tauri/src/commands/config.rs`:

```rust
#[serde(default = "default_language", deserialize_with = "deserialize_null_language")]
pub language: String,
```

Default value function:

```rust
fn default_language() -> String {
    "auto".to_string()
}
```

Valid values: `"auto"`, `"en"`, `"ja"`

Add to `AppSettings` interface in `src/settings/types.ts`:

```typescript
language: Language;
```

Language type definition (add to `src/settings/types.ts`):

```typescript
export type Language = "auto" | "en" | "ja";
```

### Settings Panel UI

Add a Language select in the Appearance category, under a "Language" subsection:

```typescript
// In renderAppearanceSection(), before the Font subsection:
this.renderSubsectionHeader(panel, t("settings.language.title"));

this.renderSelect(panel, {
  key: "language",
  label: t("settings.language.label"),
  value: this.currentSettings.language,
  options: [
    { value: "auto", label: t("settings.language.auto") },
    { value: "en", label: t("settings.language.en") },
    { value: "ja", label: t("settings.language.ja") },
  ],
  onSave: (v) => {
    this.saveSetting("language", v);
    // Apply language change
    const resolved = resolveLocale(v);
    setLocale(resolved);
    invoke("set_language", { language: resolved });
    // Re-render settings panel with new language
    this.render();
    this.attachEventListeners();
  },
});
```

## Language Sync Flow

### Startup Flow

```
┌─────────────┐    load_settings    ┌──────────────┐
│  Frontend    │ ◄──────────────── │   Backend     │
│  (main.ts)  │                    │  (config.rs)  │
└──────┬──────┘                    └───────────────┘
       │
       │  settings.language = "auto" | "en" | "ja"
       │
       ▼
┌──────────────┐
│ resolveLocale │
│ ("auto" →    │
│  "en"/"ja")  │
└──────┬───────┘
       │
       ├──► initI18n(resolvedLocale)     [Frontend locale set]
       │
       └──► invoke("set_language",       [Backend locale set]
              { language: resolvedLocale })
```

### Settings Change Flow

```
User selects "日本語" in Settings
       │
       ▼
saveSetting("language", "ja")
       │
       ├──► setLocale("ja")              [Frontend locale updated]
       │
       ├──► invoke("set_language",       [Backend locale updated]
       │      { language: "ja" })
       │
       └──► Re-render settings panel     [UI updated]
```

### Tauri Command

```rust
#[tauri::command]
fn set_language(language: String) -> Result<(), String> {
    const SUPPORTED: &[&str] = &["en", "ja"];
    if SUPPORTED.contains(&language.as_str()) {
        rust_i18n::set_locale(&language);
        Ok(())
    } else {
        Err(format!("Unsupported language: {}", language))
    }
}
```

Register in `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    set_language,
])
```

### CLI Mode Language Resolution

When running CLI subcommands (`emterm markdown`, `emterm image`), there is no frontend. The backend resolves the locale independently using `sys-locale`.

```
CLI startup (emterm markdown file.md)
  │
  ├── sys_locale::get_locale() → "ja-JP" / "en-US" / etc.
  │
  ├── Normalize to base tag: "ja-JP" → "ja", "ja_JP" → "ja", "ja_JP.UTF-8" → "ja", "en-US" → "en"
  │
  ├── Check if supported (SUPPORTED_LOCALES: ["en", "ja"])
  │     └── Unsupported → fallback to "en"
  │
  └── rust_i18n::set_locale(&resolved)
```

```rust
const SUPPORTED_LOCALES: &[&str] = &["en", "ja"];

fn resolve_system_locale() -> String {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    let base = locale.split(&['-', '_', '.'][..]).next().unwrap_or("en");
    if SUPPORTED_LOCALES.contains(&base) {
        base.to_string()
    } else {
        "en".to_string()
    }
}
```

Call `resolve_system_locale()` and `rust_i18n::set_locale()` at the beginning of `main()`, before clap argument parsing.

## Translation File Format

### Key Naming Convention

| Level | Convention | Example |
|-------|-----------|---------|
| Component | camelCase | `settings`, `tabBar`, `imageViewer` |
| Section | camelCase | `appearance`, `terminal`, `keybinds` |
| Item | camelCase | `fontSize`, `fontFamily`, `shellPath` |

Full key format: `{component}.{section}.{item}`

Examples:
- `settings.appearance.fontSize`
- `settings.terminal.cursorStyle`
- `tabBar.newTabShortcut`
- `paste.title`

### Parameter Syntax

Frontend (`t()` function): `{paramName}`
```json
"message": "You are about to paste {count} lines"
```

Backend (`t!()` macro): `%{paramName}`
```json
"fileNotFound": "File not found: %{path}"
```

## Migration Guide

### Frontend: Replacing Hardcoded Strings

1. Import `t` from `src/i18n/index.ts`
2. Replace hardcoded strings with `t()` calls
3. Add corresponding keys to `en.json` and `ja.json`

Example migration for `settings-panel.ts`:

Before:
```typescript
header.textContent = "Appearance";
```

After:
```typescript
import { t } from "../i18n/index.ts";

header.textContent = t("settings.appearance.title");
```

Before:
```typescript
hint: `Range: ${MIN_FONT_SIZE}-${MAX_FONT_SIZE}pt`,
```

After:
```typescript
hint: t("settings.appearance.fontSizeHint", {
  min: String(MIN_FONT_SIZE),
  max: String(MAX_FONT_SIZE),
}),
```

### Frontend: Component-by-Component

| Component | Action |
|-----------|--------|
| `settings-panel.ts` | Replace all category labels, section headers, field labels, hints, placeholders, and option labels with `t()` calls |
| `clipboard/dialog.ts` | Replace dialog title, message template, button labels |
| `markdown/link-dialog.ts` | Replace dialog title, button labels (currently in Japanese) |
| `tab-bar/tab-bar-ui.ts` | Replace aria-labels, titles, tooltips |
| `image-viewer/index.ts` | Replace aria-label, info display text, error message |
| `markdown/fullscreen.ts` | Replace aria-label, copy button text, feedback text |
| `shared/zoom-controller.ts` | Replace aria-labels for close, zoom-in, zoom-out, reset |

### Backend: Replacing Hardcoded Strings

1. Add `rust_i18n::i18n!()` macro to `lib.rs`
2. Replace `#[error("...")]` messages with `t!()` calls in `Display` implementation
3. Replace clap `about` and `help` strings with `t!()` calls
4. Replace `validate_settings()` error messages with `t!()` calls

Example for `error.rs`:

Before:
```rust
#[error("File not found: {0}")]
FileNotFound(PathBuf),
```

After:
```rust
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::FileNotFound(path) => {
                write!(f, "{}", t!("error.fileNotFound", path = path.display()))
            }
            // ...
        }
    }
}
```

Example for `config.rs`:

Before:
```rust
return Err(format!(
    "font_size must be between {} and {}",
    MIN_FONT_SIZE, MAX_FONT_SIZE
));
```

After:
```rust
return Err(t!(
    "validation.fontSize",
    min = MIN_FONT_SIZE,
    max = MAX_FONT_SIZE
).to_string());
```

## Adding New Languages

### Step 1: Create Frontend Translation File

Create `src/i18n/locales/{code}.json` with the same structure as `en.json`.

### Step 2: Create Backend Translation File

Create `src-tauri/locales/{code}.json` with the same structure as `src-tauri/locales/en.json`.

### Step 3: Register in Frontend

Import the new locale in `src/i18n/index.ts` and add it to the locale map.

### Step 4: Register in Settings UI

Add the new language option to the Language select in `settings-panel.ts`:

```typescript
{ value: "{code}", label: "{native name}" }
```

### Step 5: Update AppSettings Validation (Optional)

If backend validation of the `language` field is desired, add the new code to the validation logic.

### Step 6: Update resolveLocale

Add the new language code to the supported languages list in `resolveLocale()`.

## File Structure

```
src/
├── i18n/
│   ├── index.ts                    # i18n API
│   └── locales/
│       ├── en.json                 # English translations
│       └── ja.json                 # Japanese translations
├── settings/
│   ├── settings-panel.ts           # Updated: t() calls
│   ├── settings-service.ts         # No changes
│   ├── settings-applier.ts         # No changes
│   └── types.ts                    # Updated: language field
├── clipboard/
│   └── dialog.ts                   # Updated: t() calls
├── markdown/
│   ├── link-dialog.ts              # Updated: t() calls
│   └── fullscreen.ts               # Updated: t() calls
├── tab-bar/
│   └── tab-bar-ui.ts               # Updated: t() calls
├── image-viewer/
│   └── index.ts                    # Updated: t() calls
└── shared/
    └── zoom-controller.ts          # Updated: t() calls

src-tauri/
├── locales/
│   ├── en.json                     # English translations
│   └── ja.json                     # Japanese translations
├── src/
│   ├── lib.rs                      # Updated: rust_i18n::i18n!(), set_language command
│   ├── main.rs                     # Updated: t!() for clap strings
│   ├── error.rs                    # Updated: t!() for error messages
│   └── commands/
│       └── config.rs               # Updated: language field, t!() for validation
└── Cargo.toml                      # Updated: rust-i18n, sys-locale dependencies
```

## Test Scenarios

### Unit Tests
- [ ] `t()` returns correct translation for existing key
- [ ] `t()` returns English fallback when key missing in current locale
- [ ] `t()` returns key string when key missing in all locales
- [ ] `t()` replaces `{param}` placeholders correctly
- [ ] `resolveLocale("auto")` returns "en" or "ja" based on `navigator.language`
- [ ] `resolveLocale("auto")` returns "en" for unsupported languages
- [ ] `setLocale()` changes the active locale
- [ ] `getLocale()` returns the current locale
- [ ] `AppSettings` deserializes with missing `language` field (defaults to "auto")
- [ ] `AppSettings` deserializes with `null` `language` field (defaults to "auto")
- [ ] Backend `t!()` returns correct translations
- [ ] Backend `t!()` falls back to English for missing keys

### Integration Tests
- [ ] Language setting persists across app restart
- [ ] Language change updates all visible UI strings
- [ ] `set_language` Tauri command changes backend locale
- [ ] Settings panel re-renders in new language after language change

### E2E Tests
- [ ] Switch language from Auto to Japanese: all settings labels update
- [ ] Switch language from Japanese to English: all settings labels update
- [ ] Paste dialog shows translated text
- [ ] Link dialog shows translated text

## Open Questions

None.
