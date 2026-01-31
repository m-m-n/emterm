# Implementation Plan: Internationalization (i18n)

## Overview

Add multi-language support (English and Japanese) to eMterm by creating a frontend i18n module with JSON translation files and a `t()` function, integrating the `rust-i18n` crate on the backend, adding a language setting to `AppSettings`, and replacing all hardcoded UI strings across 10 components.

## Objectives

- Replace all user-visible hardcoded strings with translation function calls
- Provide language auto-detection from OS settings with manual override
- Synchronize language state between frontend and backend via Tauri command
- Maintain backward compatibility with existing settings files

## Prerequisites

### Development Environment

- Rust 1.85+ with edition 2024
- Bun (package manager and bundler)
- Tauri 2.x development environment

### Dependencies

- `rust-i18n` crate v3 (backend translations)
- `sys-locale` crate v0.3 (OS locale detection for CLI mode)
- No additional frontend dependencies (custom lightweight module)

### Knowledge Requirements

- Tauri command invocation pattern (frontend `invoke()` / backend `#[tauri::command]`)
- Existing `AppSettings` serialization pattern with `serde(default)` + null-safe deserialization
- Existing `SettingsPanel` rendering pattern (`renderSelect`, `renderSubsectionHeader`, etc.)

## Architecture Overview

### Technology Stack

- **Frontend**: Vanilla TypeScript, custom i18n module (JSON + `t()` function)
- **Backend**: Rust, `rust-i18n` crate with `t!()` macro
- **Sync**: Tauri `set_language` command
- **Translation files**: Nested JSON with dot-separated key access

### Design Approach

Frontend and backend maintain independent translation files and translation functions. The language setting is stored in `AppSettings` and synchronized through a Tauri command. The frontend resolves "auto" using `navigator.language`; the backend resolves "auto" using `sys-locale` (CLI mode only).

### Component Interaction

```
App Startup
  |
  +-- SettingsService.load() --> AppSettings.language
  |
  +-- resolveLocale(language) --> resolved locale
  |
  +-- initI18n(resolved) --> Frontend locale set
  |
  +-- invoke("set_language") --> Backend locale set

Settings Change
  |
  +-- saveSetting("language", value) --> Persist
  |
  +-- resolveLocale(value) --> resolved locale
  |
  +-- setLocale(resolved) --> Frontend update
  |
  +-- invoke("set_language") --> Backend update
  |
  +-- Re-render settings panel --> UI update
```

## Implementation Phases

### Phase 1: Core i18n Infrastructure

**Goal**: Establish the frontend i18n module and backend i18n crate integration so that `t()` and `t!()` calls are functional, without modifying any existing components yet.

**Files to Create**:

- `src/i18n/index.ts` - Frontend i18n API module
- `src/i18n/locales/en.json` - English translation file
- `src/i18n/locales/ja.json` - Japanese translation file
- `src-tauri/locales/en.json` - Backend English translation file
- `src-tauri/locales/ja.json` - Backend Japanese translation file

**Files to Modify**:

- `src-tauri/Cargo.toml` - Add `rust-i18n` and `sys-locale` dependencies
- `src-tauri/src/lib.rs` - Add `rust_i18n::i18n!()` macro invocation

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `initI18n(locale)` | Load translation data and set active locale | Valid locale string | Translations loaded, active locale set |
| `t(key, params?)` | Return translated string for given key | Module initialized | Translated string or key as fallback |
| `setLocale(locale)` | Change active locale at runtime | Module initialized | Active locale updated |
| `getLocale()` | Return current active locale | Module initialized | Locale string returned |
| `resolveLocale(locale)` | Resolve "auto" to concrete locale code | None | Concrete locale code returned |
| `SUPPORTED_LOCALES` | Define supported locale codes | None | Constant array of locale codes |

**Processing Flow**:

```
initI18n(locale):
  1. Import en.json and ja.json statically
  2. Store translations in module-level map
  3. Set active locale to provided value
  4. Set fallback locale to "en"

t(key, params?):
  1. Resolve key in current locale translations
     +-- Found --> use value
     +-- Not found --> resolve key in fallback locale
         +-- Found --> use fallback value
         +-- Not found --> use key string as-is
  2. If params provided, replace {paramName} placeholders
  3. Return final string

resolveLocale(locale):
  1. If locale is not "auto" --> return as-is
  2. Read navigator.language
  3. Extract base tag (split on "-")
  4. If base tag in SUPPORTED_LOCALES --> return base tag
  5. Otherwise --> return "en"
```

**Implementation Steps**:

1. **Add Rust dependencies**
   - Add `rust-i18n = "3"` and `sys-locale = "0.3"` to `Cargo.toml` dependencies section
   - Key considerations:
     - `rust-i18n` v3 supports nested JSON format
     - `sys-locale` is only used in CLI mode for OS language detection

2. **Create frontend i18n module**
   - Implement `src/i18n/index.ts` with exported functions: `initI18n`, `t`, `setLocale`, `getLocale`, `resolveLocale`
   - Use static imports for JSON files (Bun bundler supports JSON imports)
   - Key considerations:
     - `t()` must traverse nested objects using dot-separated keys
     - Fallback chain: current locale -> "en" -> key string
     - Parameter replacement uses `{paramName}` syntax

3. **Create translation JSON files**
   - Create `src/i18n/locales/en.json` and `ja.json` with full key structure from SPEC.md
   - Create `src-tauri/locales/en.json` and `ja.json` with backend keys from SPEC.md
   - Key considerations:
     - Frontend uses `{paramName}` placeholder syntax
     - Backend uses `%{paramName}` placeholder syntax
     - All keys in en.json must exist in ja.json

4. **Integrate rust-i18n in lib.rs**
   - Add `rust_i18n::i18n!("locales", fallback = "en")` macro invocation
   - Key considerations:
     - The macro must be at crate root level
     - The path "locales" is relative to the crate root (src-tauri/)

5. **Write unit tests for i18n module**
   - Test `t()` for existing key, missing key, fallback, parameter replacement
   - Test `resolveLocale()` for "auto", supported locales, unsupported locales
   - Test `setLocale()` and `getLocale()`

**Dependencies**:

- Requires: Nothing (first phase)
- Blocks: Phase 2, Phase 3, Phase 4

**Testing Approach**:

*Unit Tests*:

| Scenario | Expected Result |
|----------|-----------------|
| `t("settings.appearance.title")` with locale "en" | "Appearance" |
| `t("settings.appearance.title")` with locale "ja" | "外観" |
| `t("nonexistent.key")` with locale "ja" | Falls back to en, then returns key string |
| `t("paste.message", { count: 5 })` | "You are about to paste 5 lines of text into the terminal." |
| `resolveLocale("auto")` with navigator.language="ja-JP" | "ja" |
| `resolveLocale("auto")` with navigator.language="fr-FR" | "en" |
| `resolveLocale("ja")` | "ja" |
| `setLocale("ja")` then `getLocale()` | "ja" |

*Rust Unit Tests*:

| Scenario | Expected Result |
|----------|-----------------|
| `t!("cli.about")` with locale "en" | English about string |
| `t!("error.fileNotFound", path = "test.txt")` | "File not found: test.txt" |
| `t!("cli.about")` with locale "ja" | Japanese about string |

**Acceptance Criteria**:

- [ ] `t()` returns correct translation for all existing keys in en.json
- [ ] `t()` returns correct translation for all existing keys in ja.json
- [ ] `t()` falls back to English when key is missing in current locale
- [ ] `t()` returns key string when key is missing in all locales
- [ ] `t()` replaces `{paramName}` placeholders correctly
- [ ] `resolveLocale("auto")` correctly detects browser language
- [ ] Backend `t!()` macro compiles and returns translations
- [ ] All translation JSON files parse without errors
- [ ] en.json and ja.json have identical key structures (both frontend and backend)

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:

- **Risk**: JSON import may need bundler configuration
  - **Mitigation**: Bun natively supports JSON imports; verify in dev build

---

### Phase 2: AppSettings Extension and Language Sync

**Goal**: Add the `language` field to `AppSettings` on both frontend and backend, implement the `set_language` Tauri command, and wire up the startup initialization flow so that language is properly synced.

**Files to Modify**:

- `src/settings/types.ts` - Add `language` field to `AppSettings` interface
- `src-tauri/src/commands/config.rs` - Add `language` field to `AppSettings` struct with serde defaults
- `src-tauri/src/lib.rs` - Add `set_language` command and register in invoke_handler
- `src-tauri/src/main.rs` - Add CLI locale resolution before clap parsing
- `src/main.ts` - Add i18n initialization to startup flow

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `AppSettings.language` (TS) | Store language preference | None | Field available in settings interface |
| `AppSettings.language` (Rust) | Store and deserialize language preference | None | Field with "auto" default, null-safe |
| `set_language` command | Set backend locale at runtime | Valid locale string | `rust_i18n::set_locale()` called |
| `resolve_system_locale()` | Resolve OS locale for CLI mode | None | Concrete locale code for backend |
| Startup flow in `main.ts` | Initialize i18n and sync backend | Settings loaded | Both frontend and backend locales set |

**Processing Flow**:

```
Startup (main.ts):
  1. SettingsService.load() --> settings.language
  2. resolveLocale(settings.language) --> resolvedLocale
  3. initI18n(resolvedLocale) --> frontend i18n ready
  4. invoke("set_language", { language: resolvedLocale }) --> backend synced

CLI Mode (main.rs):
  1. resolve_system_locale() --> locale from OS
  2. rust_i18n::set_locale(&locale) --> backend locale set
  3. Cli::parse() --> clap uses translated strings
```

**Implementation Steps**:

1. **Extend AppSettings on backend**
   - Add `language` field with `serde(default = "default_language")` and null-safe deserializer
   - Add `default_language()` function returning `"auto"`
   - Follow existing patterns: `deserialize_null_with!` macro for the language field
   - Update `Default` implementation
   - Key considerations:
     - Must maintain backward compatibility (missing field defaults to "auto")
     - Must handle null JSON value (defaults to "auto")

2. **Extend AppSettings on frontend**
   - Add `Language` type (`"auto" | "en" | "ja"`) to `types.ts`
   - Add `language: Language` field to the `AppSettings` interface in `types.ts`

3. **Implement set_language command**
   - Add `set_language(language: String) -> Result<(), String>` Tauri command to `lib.rs`
   - Validate input against `SUPPORTED_LOCALES` before calling `rust_i18n::set_locale()`
   - Return `Err` for unsupported locale strings
   - Register the command in the `invoke_handler` list

4. **Implement CLI locale resolution**
   - Add `resolve_system_locale()` function in `main.rs`
   - Call it before `Cli::parse()` and set locale with `rust_i18n::set_locale()`
   - Key considerations:
     - Uses `sys_locale::get_locale()` for OS detection
     - Splits by multiple separators (`-`, `_`, `.`) to handle formats like `ja_JP`, `ja_JP.UTF-8`, `ja-JP`
     - Extracts base tag and checks against `SUPPORTED_LOCALES`
     - Falls back to "en" for unsupported locales

5. **Wire up startup flow**
   - Modify `main.ts` to call `resolveLocale()`, `initI18n()`, and `invoke("set_language")` after settings load
   - Key considerations:
     - i18n initialization must complete before any UI rendering
     - Backend sync is fire-and-forget (UI does not wait for it)

**Dependencies**:

- Requires: Phase 1
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests (Rust)*:

| Scenario | Expected Result |
|----------|-----------------|
| Deserialize `{}` | `language` defaults to "auto" |
| Deserialize `{"language": null}` | `language` defaults to "auto" |
| Deserialize `{"language": "ja"}` | `language` is "ja" |
| Round-trip with `language: "ja"` | Preserved through serialize/deserialize |
| `resolve_system_locale()` with "ja-JP" | "ja" |
| `resolve_system_locale()` with "ja_JP" | "ja" |
| `resolve_system_locale()` with "ja_JP.UTF-8" | "ja" |
| `resolve_system_locale()` with "fr-FR" | "en" |

*Integration Tests*:

| Scenario | Expected Result |
|----------|-----------------|
| `set_language("ja")` command | Backend locale becomes "ja" |
| App startup with `language: "auto"` | Locale resolved from OS |
| Existing settings file without `language` field | Loads successfully with "auto" default |

**Acceptance Criteria**:

- [ ] `AppSettings` serializes and deserializes the `language` field correctly
- [ ] Missing `language` field defaults to "auto"
- [ ] Null `language` field defaults to "auto"
- [ ] `set_language` Tauri command changes backend locale
- [ ] CLI mode resolves system locale before argument parsing
- [ ] Frontend startup initializes i18n with resolved locale
- [ ] Backend startup syncs locale from frontend

**Estimated Effort**: Small (1-2 days)

**Risks and Mitigation**:

- **Risk**: Existing settings files may fail to parse with new field
  - **Mitigation**: Use `serde(default)` pattern already established in the codebase

---

### Phase 3: Frontend String Migration

**Goal**: Replace all hardcoded user-visible strings in frontend components with `t()` calls, and add the Language selector to the Settings panel.

**Files to Modify**:

- `src/settings/settings-panel.ts` - Replace ~50 strings with `t()` calls; add Language selector
- `src/clipboard/dialog.ts` - Replace ~5 strings (title, message, button labels, "more lines")
- `src/markdown/link-dialog.ts` - Replace ~3 strings (title, button labels)
- `src/tab-bar/tab-bar-ui.ts` - Replace ~5 strings (aria-labels, titles, tooltips)
- `src/image-viewer/index.ts` - Replace ~3 strings (aria-label, info text, error message)
- `src/markdown/fullscreen.ts` - Replace ~4 strings (aria-label, copy button text, feedback)
- `src/shared/zoom-controller.ts` - Replace ~4 strings (aria-labels for buttons)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Settings panel Language selector | Allow user to change language | i18n module initialized | Language change triggers re-render and backend sync |
| `settings-panel.ts` migration | Display all labels via `t()` | Translation keys exist | All visible text comes from translation files |
| Dialog migrations | Display dialog text via `t()` | Translation keys exist | Dialog text is localized |
| Tab bar migration | Display accessible labels via `t()` | Translation keys exist | ARIA labels and tooltips are localized |
| Viewer migrations | Display viewer text via `t()` | Translation keys exist | Viewer text is localized |

**Processing Flow**:

```
Language Change in Settings:
  1. User selects new language from dropdown
  2. saveSetting("language", selectedValue)
  3. resolveLocale(selectedValue) --> resolvedLocale
  4. setLocale(resolvedLocale) --> frontend locale updated
  5. invoke("set_language", { language: resolvedLocale }) --> backend synced
  6. Re-render settings panel (this.render() + this.attachEventListeners())
  7. Settings panel now shows all labels in new language
```

**Implementation Steps**:

1. **Add Language selector to Settings panel**
   - Add Language subsection before the Font subsection in `renderAppearanceSection()`
   - Use `renderSubsectionHeader()` and `renderSelect()` with existing patterns
   - On change: save setting, resolve locale, update frontend, sync backend, re-render
   - Key considerations:
     - The `categories` array labels must also use `t()` calls
     - The re-render must call both `render()` and `attachEventListeners()`
     - Language option labels: "Auto (System)", "English", "日本語"

2. **Migrate settings-panel.ts strings**
   - Import `t` from i18n module
   - Replace all string literals for labels, hints, placeholders, option labels
   - Convert hint strings with dynamic ranges to use `t()` with parameters
   - Key considerations:
     - Parameterized hints: `t("settings.appearance.fontSizeHint", { min: ..., max: ... })`
     - Section headers, subsection headers, labels, hints, placeholders, select options
     - The categories array should derive labels from `t()` at render time

3. **Migrate clipboard/dialog.ts**
   - Replace "Confirm Paste", message template, "Cancel", "Paste", "more lines" text
   - Key considerations:
     - Message uses `{count}` parameter
     - "more lines" text uses `{count}` parameter

4. **Migrate markdown/link-dialog.ts**
   - Replace hardcoded Japanese strings with `t()` calls
   - Key considerations:
     - Currently has Japanese hardcoded ("外部リンクを開きますか?", "キャンセル", "開く")
     - Must switch to `t("link.title")`, `t("link.cancel")`, `t("link.open")`

5. **Migrate tab-bar/tab-bar-ui.ts**
   - Replace aria-label, title, and tooltip strings
   - Key considerations:
     - `aria-label="Terminal tabs"`, `title="New Tab (Ctrl+T)"`, `aria-label="Create new tab"`, `title="Settings"`, `aria-label="Open settings"`

6. **Migrate image-viewer/index.ts**
   - Replace aria-label, info display text, error message text
   - Key considerations:
     - Info display uses `{width}`, `{height}`, `{mode}`, `{help}` parameters
     - Mode text: "Fit" for fit mode (100% is a number, not translated)
     - Error text: "Failed to decode image"

7. **Migrate markdown/fullscreen.ts**
   - Replace aria-label, copy button text, feedback text
   - Key considerations:
     - `aria-label="Markdown Document"` -> `t("markdown.label")`
     - Copy button: "Copy" -> `t("markdown.copyCode")`
     - Feedback: "Copied!" -> `t("markdown.copySuccess")`, "Failed" -> `t("markdown.copyFailed")`

8. **Migrate shared/zoom-controller.ts**
   - Replace aria-label strings for close, zoom-in, zoom-out, reset buttons
   - Key considerations:
     - Reset button aria-label uses `{level}` parameter: `t("zoom.resetZoom", { level: ... })`

**Dependencies**:

- Requires: Phase 1, Phase 2
- Blocks: Nothing (can run in parallel with Phase 4)

**Testing Approach**:

*Unit Tests*:

| Scenario | Expected Result |
|----------|-----------------|
| Settings panel renders with locale "en" | All labels in English |
| Settings panel renders with locale "ja" | All labels in Japanese |
| Language change triggers re-render | Panel updates to new language |
| Paste dialog with locale "ja" | Title and buttons in Japanese |
| Link dialog with locale "en" | Title and buttons in English |

*Manual Testing*:

- [ ] All settings labels display correctly in English
- [ ] All settings labels display correctly in Japanese
- [ ] Language selector appears in Appearance category
- [ ] Changing language immediately updates settings panel
- [ ] Paste dialog shows localized text
- [ ] Link dialog shows localized text (no longer hardcoded Japanese)
- [ ] Tab bar tooltips and aria-labels are localized
- [ ] Image viewer info bar shows localized text
- [ ] Markdown fullscreen copy buttons show localized text
- [ ] Zoom controller buttons have localized aria-labels

**Acceptance Criteria**:

- [ ] No hardcoded user-visible strings remain in the 7 modified frontend files
- [ ] Language selector is functional in Settings panel
- [ ] Switching language updates all visible UI in the settings panel
- [ ] All `t()` keys used in code have corresponding entries in both en.json and ja.json
- [ ] Link dialog no longer contains hardcoded Japanese text

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:

- **Risk**: Missing translation keys cause raw key strings to appear
  - **Mitigation**: Verify all keys exist in both en.json and ja.json before merge; write a key-parity check test
- **Risk**: Re-render on language change may break event listeners
  - **Mitigation**: Follow existing pattern of `render()` + `attachEventListeners()`

---

### Phase 4: Backend String Migration

**Goal**: Replace all hardcoded user-visible strings in backend Rust code with `t!()` macro calls.

**Files to Modify**:

- `src-tauri/src/error.rs` - Replace `#[error("...")]` messages with manual `Display` impl using `t!()`
- `src-tauri/src/main.rs` - Replace clap `about` and help strings with `t!()` calls
- `src-tauri/src/commands/config.rs` - Replace `validate_settings()` error messages with `t!()`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `CommandError` Display impl | Return localized error messages | Backend locale set | Error messages use active locale |
| Clap CLI strings | Display localized help text | Backend locale set before parsing | CLI help in active locale |
| Validation messages | Return localized validation errors | Backend locale set | Validation errors use active locale |

**Processing Flow**:

```
Error Display:
  1. CommandError variant is created
  2. Display::fmt is called (for user-facing output)
  3. t!() macro resolves translation for current locale
  4. Parameters are interpolated
  5. Formatted string is returned

CLI Help:
  1. resolve_system_locale() sets locale
  2. Cli::parse() triggers help rendering
  3. about/help strings call t!() at evaluation time
  4. Localized help text is displayed
```

**Implementation Steps**:

1. **Migrate error.rs**
   - Remove `#[error("...")]` derive attributes from `CommandError` variants
   - Implement `fmt::Display` manually for `CommandError`
   - Each variant calls `t!()` with appropriate key and parameters
   - Keep `thiserror::Error` derive for the `Error` trait but provide manual `Display`
   - Key considerations:
     - `FileNotFound(PathBuf)` -> `t!("error.fileNotFound", path = path.display())`
     - `FileTooLarge { size, max_size }` -> `t!("error.fileTooLarge", size = size, maxSize = max_size)`
     - `FileReadError(std::io::Error)` -> `t!("error.fileReadError", error = source)`
     - Backend parameter syntax is `%{paramName}`
     - Existing tests for error display messages must be updated

2. **Migrate main.rs CLI strings**
   - Replace `#[command(about = "...")]` with dynamic about strings
   - Replace doc comments on variants (which clap uses for help) with explicit `about` attributes using `t!()`
   - Key considerations:
     - Clap derive macro attributes require `&'static str` at compile time; runtime `t!()` values cannot be used directly in derive attributes
     - **Recommended approach**: Use clap builder API to construct `Command` with `t!()` calls at runtime:
       ```rust
       let cli = Command::new("emterm")
           .about(t!("cli.about").to_string())
           .subcommand(
               Command::new("markdown")
                   .about(t!("cli.markdown.about").to_string())
           )
           .subcommand(
               Command::new("image")
                   .about(t!("cli.image.about").to_string())
           );
       let matches = cli.get_matches();
       ```
     - Alternative: Keep derive for argument parsing but override `about`/`long_about` using `.mut_cmd()` or `Command::augment_args`
     - Locale must be set before `Cli::parse()` / `cli.get_matches()` is called

3. **Migrate config.rs validation messages**
   - Replace `format!("font_size must be between...")` with `t!("validation.fontSize", min = ..., max = ...)`
   - Apply to all 6 validation error messages
   - Key considerations:
     - Each validation call returns `Err(t!(...).to_string())`
     - Parameter names must match the JSON keys (`%{min}`, `%{max}`)

**Dependencies**:

- Requires: Phase 1, Phase 2
- Blocks: Nothing

**Testing Approach**:

*Unit Tests (Rust)*:

| Scenario | Expected Result |
|----------|-----------------|
| `CommandError::FileNotFound` display with locale "en" | Contains "File not found" |
| `CommandError::FileNotFound` display with locale "ja" | Contains "ファイルが見つかりません" |
| `CommandError::FileTooLarge` display | Contains size and limit values |
| Validation error for font_size with locale "en" | Contains "font_size must be between" |
| Validation error for font_size with locale "ja" | Contains "font_sizeは" and "範囲" |

*Integration Tests*:

| Scenario | Expected Result |
|----------|-----------------|
| `emterm --help` with LANG=ja_JP | Japanese help text |
| `emterm markdown nonexistent.md` with LANG=ja_JP | Japanese error message |

**Acceptance Criteria**:

- [ ] No hardcoded user-visible strings remain in `error.rs`
- [ ] No hardcoded user-visible strings remain in `main.rs` CLI definitions
- [ ] No hardcoded user-visible strings remain in `validate_settings()`
- [ ] Error messages display correctly in English
- [ ] Error messages display correctly in Japanese
- [ ] CLI help text displays in the detected OS language
- [ ] All existing tests pass (updated for new message format)

**Estimated Effort**: Medium (3-5 days)

**Risks and Mitigation**:

- **Risk**: Clap may not support dynamic strings for all attributes
  - **Mitigation**: Use clap builder API if derive macro attributes do not support runtime values
- **Risk**: Removing `#[error("...")]` changes `thiserror` behavior
  - **Mitigation**: Implement `Display` manually while keeping `#[derive(Error)]` for `Error` trait

---

## Complete File Structure

```
src/
├── i18n/
│   ├── index.ts                     # i18n API: initI18n, t, setLocale, getLocale, resolveLocale
│   └── locales/
│       ├── en.json                  # English translations (~80 keys)
│       └── ja.json                  # Japanese translations (~80 keys)
├── settings/
│   ├── settings-panel.ts            # Modified: all labels via t(), Language selector added
│   ├── settings-service.ts          # No changes
│   ├── settings-applier.ts          # No changes
│   └── types.ts                     # Modified: language field added to AppSettings
├── clipboard/
│   └── dialog.ts                    # Modified: title, message, buttons via t()
├── markdown/
│   ├── link-dialog.ts               # Modified: title, buttons via t()
│   └── fullscreen.ts                # Modified: aria-label, copy button text via t()
├── tab-bar/
│   └── tab-bar-ui.ts                # Modified: aria-labels, titles via t()
├── image-viewer/
│   └── index.ts                     # Modified: aria-label, info display, error via t()
├── shared/
│   └── zoom-controller.ts           # Modified: button aria-labels via t()
└── main.ts                          # Modified: i18n initialization in startup flow

src-tauri/
├── locales/
│   ├── en.json                      # Backend English translations (~25 keys)
│   └── ja.json                      # Backend Japanese translations (~25 keys)
├── src/
│   ├── lib.rs                       # Modified: rust_i18n::i18n!(), set_language command
│   ├── main.rs                      # Modified: CLI locale resolution, t!() for clap strings
│   ├── error.rs                     # Modified: manual Display impl with t!()
│   └── commands/
│       └── config.rs                # Modified: language field, t!() for validation
└── Cargo.toml                       # Modified: rust-i18n, sys-locale dependencies
```

**File Descriptions**:

| File | Purpose |
|------|---------|
| `src/i18n/index.ts` | Core i18n module: translation lookup, locale management, parameter interpolation |
| `src/i18n/locales/en.json` | English translations for all frontend UI strings |
| `src/i18n/locales/ja.json` | Japanese translations for all frontend UI strings |
| `src-tauri/locales/en.json` | English translations for CLI help, errors, and validation |
| `src-tauri/locales/ja.json` | Japanese translations for CLI help, errors, and validation |

## Testing Strategy

### Unit Testing

**Frontend** (Bun test):

- i18n module: `t()` lookup, fallback, parameter replacement, locale management
- Translation file parity: verify en.json and ja.json have identical key structures

**Backend** (Cargo test):

- `AppSettings` deserialization with new `language` field (missing, null, valid)
- `resolve_system_locale()` for various OS locale strings
- `CommandError` display messages in English and Japanese
- Validation error messages in English and Japanese
- `t!()` macro returns correct translations

### Integration Testing

- Language setting persists across app restart (save and reload)
- `set_language` Tauri command changes backend locale
- CLI subcommands display localized help and error messages

### Manual Testing Checklist

- [ ] Switch language from Auto to Japanese: all settings labels update
- [ ] Switch language from Japanese to English: all settings labels update
- [ ] Paste dialog shows translated text in both languages
- [ ] Link dialog shows translated text (no hardcoded Japanese)
- [ ] Image viewer info bar shows translated mode and help text
- [ ] Markdown fullscreen copy buttons show translated text
- [ ] Tab bar tooltips show translated text
- [ ] Zoom controller buttons have translated aria-labels
- [ ] Existing settings file without `language` loads correctly
- [ ] `emterm --help` shows localized text based on OS language
- [ ] `emterm markdown nonexistent.md` shows localized error

## Dependencies

### External Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `rust-i18n` | 3 | Backend translation management via `t!()` macro |
| `sys-locale` | 0.3 | OS locale detection for CLI mode |

### Internal Dependencies

**Implementation Order** (respecting dependencies):

1. Phase 1 - Core i18n infrastructure (no dependencies)
2. Phase 2 - AppSettings extension and language sync (depends on Phase 1)
3. Phase 3 - Frontend string migration (depends on Phase 1 + 2)
4. Phase 4 - Backend string migration (depends on Phase 1 + 2; parallel with Phase 3)

**Component Dependencies**:

- All `t()` calls in frontend depend on `src/i18n/index.ts`
- All `t!()` calls in backend depend on `rust_i18n::i18n!()` macro in `lib.rs`
- Language selector in settings depends on `set_language` Tauri command
- CLI locale depends on `sys-locale` crate and `resolve_system_locale()`
- Settings panel re-render depends on `setLocale()` + `invoke("set_language")`

## Risk Assessment

### Technical Risks

1. **Clap Dynamic String Support**
   - **Risk**: Clap derive macros require `&'static str` and cannot use runtime `t!()` values
   - **Likelihood**: High (confirmed limitation of derive macros)
   - **Impact**: Medium (CLI construction needs builder API)
   - **Mitigation**: Use clap builder API for `Command` construction with runtime `t!()` calls (see Phase 4 Step 2)

2. **thiserror Compatibility with Manual Display**
   - **Risk**: Removing `#[error("...")]` may conflict with `thiserror` derive
   - **Likelihood**: Low
   - **Impact**: Low (can remove thiserror derive and implement Error manually)
   - **Mitigation**: Test with manual Display impl; thiserror supports `#[derive(Error)]` without `#[error]` if `Display` is implemented

3. **Translation File Sync**
   - **Risk**: Translation keys may get out of sync between en.json and ja.json
   - **Likelihood**: Medium (ongoing risk during development)
   - **Impact**: Low (fallback to English or key string)
   - **Mitigation**: Write a key-parity check test that fails if keys diverge

### Implementation Risks

1. **Settings Panel Re-render**
   - **Risk**: Language change re-render may break event listeners or UI state
   - **Likelihood**: Low (existing pattern handles this)
   - **Impact**: Medium (broken settings UI)
   - **Mitigation**: Follow existing `switchCategory()` pattern which already does `detachContentListeners()` + re-render

## Performance Considerations

1. **Translation Lookup**
   - `t()` performs object property traversal (3 levels max)
   - No network requests; translations are statically imported at build time
   - Negligible impact on rendering performance

2. **Language Switch**
   - Re-renders settings panel only (not the entire application)
   - Other components (dialogs, viewers) pick up new locale on next open
   - No DOM-wide re-render needed

3. **Backend Translation**
   - `t!()` macro resolves at call time with string matching
   - Negligible impact compared to I/O-bound operations

## Security Considerations

1. **No User-Provided Translation Keys**
   - All translation keys are hardcoded in source code
   - No user input flows into `t()` key parameter
   - No risk of key injection or path traversal

2. **HTML Escaping in Dialogs**
   - Link dialog already uses `escapeHtml()` for URL display
   - Translation strings do not contain HTML; parameter values are text-only
   - No XSS risk from translation content

## Open Questions

None. All decisions have been documented in the specification.

## Future Enhancements

Items beyond the current specification:

- Additional languages beyond English and Japanese
- Type-safe translation keys (generated TypeScript types from en.json)
- Translation validation script in CI pipeline
- RTL language support

## Success Metrics

### Functional Completeness

- [ ] All 15 functional requirements (F01-F15) implemented
- [ ] All test scenarios from SPEC.md pass
- [ ] No hardcoded user-visible strings remain in modified files

### Quality Metrics

- [ ] Frontend i18n module: 90%+ test coverage
- [ ] Backend config tests: all new deserialization cases pass
- [ ] Translation key parity: en.json and ja.json have identical structures

### User Experience

- [ ] Language change is immediate (no restart required)
- [ ] Settings panel displays correctly in both languages
- [ ] All dialogs and viewers display correctly in both languages
- [ ] CLI shows localized help and error messages

## References

- **Specification**: `doc/tasks/i18n/SPEC.md`
- **Requirements**: `doc/tasks/i18n/要件定義書.md`
- **rust-i18n crate**: https://crates.io/crates/rust-i18n
- **sys-locale crate**: https://crates.io/crates/sys-locale
- **i18n Guidelines Skill**: `~/.claude/skills/i18n-guidelines/`

## Next Steps

After reviewing this implementation plan:

1. `/sdd.3-verify-plan` to run consistency verification and design review
2. Resolve any open questions
3. `/sdd.4-implement` to begin implementation starting with Phase 1
