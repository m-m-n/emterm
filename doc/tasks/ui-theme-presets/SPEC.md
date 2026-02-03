# Feature: UIテーマカラープリセット

## Overview

UIテーマ（タブバー・設定パネル等のMaterial Design 3カラー）に、ダーク/ライトそれぞれ4種類の色相バリエーションプリセットを追加する。2段階選択UIで、まずライト/ダーク/システムを選び、次にプリセットを選ぶ。

## Objectives

- ダークテーマに4種類の色相プリセットを追加（Purple, Blue, Green, Orange）
- ライトテーマに4種類の色相プリセットを追加（Purple, Blue, Green, Orange）
- 2段階選択UI（テーマモード → プリセット）を実装
- システムテーマ選択時もプリセット連動（OSのダーク/ライトに応じたプリセットを適用）

## User Stories

### US1: ダークテーマのプリセット選択
ユーザーとして、ダークテーマを選んだ際に4種類の色相プリセットから好みの配色を選べるようにしたい。

**Acceptance Criteria:**
- [ ] UIテーマで「ダーク」選択後、プリセット選択セレクトボックスが表示される
- [ ] Purple, Blue, Green, Orange の4種類から選択できる
- [ ] 選択したプリセットが即座にUIに反映される
- [ ] 選択は永続化される

### US2: ライトテーマのプリセット選択
ユーザーとして、ライトテーマを選んだ際に4種類の色相プリセットから好みの配色を選べるようにしたい。

**Acceptance Criteria:**
- [ ] UIテーマで「ライト」選択後、プリセット選択セレクトボックスが表示される
- [ ] Purple, Blue, Green, Orange の4種類から選択できる
- [ ] 選択したプリセットが即座にUIに反映される
- [ ] 選択は永続化される

### US3: システムテーマでのプリセット連動
ユーザーとして、「システム」テーマ選択時にも好みのプリセットを適用したい。

**Acceptance Criteria:**
- [ ] 「システム」テーマ選択時もプリセット選択が可能
- [ ] OSがダークモードの場合、選択プリセットのダーク版が適用される
- [ ] OSがライトモードの場合、選択プリセットのライト版が適用される
- [ ] OS設定変更時にプリセットの明暗が自動切替される

## Technical Requirements

### Functional Requirements

- **FR1:** 新しい設定フィールド `ui_theme_preset` を追加（値: `"purple"` | `"blue"` | `"green"` | `"orange"`、デフォルト: `"purple"`）
- **FR2:** 各プリセットはダーク/ライト両方のMD3カラートークン一式を定義
- **FR3:** `applyUiTheme()` を拡張し、テーマモードとプリセットの両方を適用
- **FR4:** 設定UIに2段階選択を実装（UIテーマセレクト + プリセットセレクト）
- **FR5:** プリセット変更時のリアルタイムプレビュー
- **FR6:** 後方互換性の維持（`ui_theme_preset` 未設定時は `"purple"` をデフォルト適用）

### Non-Functional Requirements

- **NFR1 - Performance:** プリセット切替は即座に反映（CSS変数の書き換えのみ）
- **NFR2 - Compatibility:** 既存の `ui_theme` 設定との後方互換性を維持
- **NFR3 - Maintainability:** プリセット定義を1ファイルに集約し、新プリセット追加を容易にする

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────┐
│       Settings UI (settings-sections.ts)    │
│  - UIテーマ選択（system / light / dark）     │
│  - プリセット選択（purple / blue / green /   │
│    orange）                                  │
├─────────────────────────────────────────────┤
│       Applier (settings-applier.ts)         │
│  - applyUiTheme(theme, preset)              │
│  - CSS変数をプリセットデータから一括設定       │
├─────────────────────────────────────────────┤
│       Preset Data (ui-theme-presets.ts)      │
│  - 各プリセットのダーク/ライト色定義          │
│  - MD3カラートークンのマッピング              │
├─────────────────────────────────────────────┤
│       CSS (styles.css)                       │
│  - :root のハードコード色定義を削除           │
│  - CSS変数はJS側から動的に設定                │
└─────────────────────────────────────────────┘
```

### Data Model

#### 設定フィールド追加

**TypeScript (types.ts):**
```typescript
export type UiThemePreset = "purple" | "blue" | "green" | "orange";
```

`AppSettings` に追加:
```typescript
ui_theme_preset: UiThemePreset;
```

**Rust (config.rs):**
```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UiThemePreset {
    #[default]
    Purple,
    Blue,
    Green,
    Orange,
}
```

`AppSettings` に追加:
```rust
#[serde(default, deserialize_with = "deserialize_null_default")]
pub ui_theme_preset: UiThemePreset,
```

#### プリセット定義構造

```typescript
interface ThemeColors {
  primary: string;
  onPrimary: string;
  primaryContainer: string;
  onPrimaryContainer: string;
  secondary: string;
  onSecondary: string;
  secondaryContainer: string;
  onSecondaryContainer: string;
  surface: string;
  surfaceContainer: string;
  surfaceContainerLow: string;
  surfaceContainerHigh: string;
  surfaceContainerHighest: string;
  onSurface: string;
  onSurfaceVariant: string;
  outline: string;
  outlineVariant: string;
  error: string;
  onError: string;
}

interface PresetDefinition {
  dark: ThemeColors;
  light: ThemeColors;
}
```

### カラープリセット定義

#### Purple（現在のデフォルト）

**Dark:**
| Token | Value |
|-------|-------|
| primary | #D0BCFF |
| on-primary | #381E72 |
| primary-container | #4F378B |
| on-primary-container | #EADDFF |
| secondary | #CCC2DC |
| on-secondary | #332D41 |
| secondary-container | #4A4458 |
| on-secondary-container | #E8DEF8 |
| surface | #141218 |
| surface-container | #211F26 |
| surface-container-low | #1D1B20 |
| surface-container-high | #2B2930 |
| surface-container-highest | #36343B |
| on-surface | #E6E0E9 |
| on-surface-variant | #CAC4D0 |
| outline | #938F99 |
| outline-variant | #49454F |
| error | #F2B8B5 |
| on-error | #601410 |

**Light:**
| Token | Value |
|-------|-------|
| primary | #6750A4 |
| on-primary | #FFFFFF |
| primary-container | #EADDFF |
| on-primary-container | #21005D |
| secondary | #625B71 |
| on-secondary | #FFFFFF |
| secondary-container | #E8DEF8 |
| on-secondary-container | #1D192B |
| surface | #FEF7FF |
| surface-container | #F3EDF7 |
| surface-container-low | #F7F2FA |
| surface-container-high | #ECE6F0 |
| surface-container-highest | #E6E0E9 |
| on-surface | #1D1B20 |
| on-surface-variant | #49454F |
| outline | #79747E |
| outline-variant | #CAC4D0 |
| error | #B3261E |
| on-error | #FFFFFF |

#### Blue

**Dark:**
| Token | Value |
|-------|-------|
| primary | #A8C7FA |
| on-primary | #062E6F |
| primary-container | #0842A0 |
| on-primary-container | #D3E3FD |
| secondary | #C2C6DC |
| on-secondary | #2C3041 |
| secondary-container | #434659 |
| on-secondary-container | #DEE2F9 |
| surface | #111318 |
| surface-container | #1F2126 |
| surface-container-low | #1A1C20 |
| surface-container-high | #292B30 |
| surface-container-highest | #34363B |
| on-surface | #E2E2E9 |
| on-surface-variant | #C4C6D0 |
| outline | #8E909A |
| outline-variant | #44464F |
| error | #F2B8B5 |
| on-error | #601410 |

**Light:**
| Token | Value |
|-------|-------|
| primary | #0B57D0 |
| on-primary | #FFFFFF |
| primary-container | #D3E3FD |
| on-primary-container | #041E49 |
| secondary | #5A5E71 |
| on-secondary | #FFFFFF |
| secondary-container | #DEE2F9 |
| on-secondary-container | #171B2C |
| surface | #F9F9FF |
| surface-container | #EFF0F6 |
| surface-container-low | #F3F3FA |
| surface-container-high | #E8E9EF |
| surface-container-highest | #E2E2E9 |
| on-surface | #1A1C20 |
| on-surface-variant | #44464F |
| outline | #75767F |
| outline-variant | #C4C6D0 |
| error | #B3261E |
| on-error | #FFFFFF |

#### Green

**Dark:**
| Token | Value |
|-------|-------|
| primary | #7DD3A8 |
| on-primary | #003823 |
| primary-container | #005234 |
| on-primary-container | #98F0C3 |
| secondary | #B4CCB8 |
| on-secondary | #213528 |
| secondary-container | #374B3E |
| on-secondary-container | #D0E8D4 |
| surface | #101412 |
| surface-container | #1C201E |
| surface-container-low | #181C1A |
| surface-container-high | #262B28 |
| surface-container-highest | #313633 |
| on-surface | #DEE4DF |
| on-surface-variant | #BFC9C1 |
| outline | #8A938C |
| outline-variant | #404943 |
| error | #F2B8B5 |
| on-error | #601410 |

**Light:**
| Token | Value |
|-------|-------|
| primary | #006D3E |
| on-primary | #FFFFFF |
| primary-container | #98F0C3 |
| on-primary-container | #002110 |
| secondary | #4E6354 |
| on-secondary | #FFFFFF |
| secondary-container | #D0E8D4 |
| on-secondary-container | #0B1F13 |
| surface | #F5FBF5 |
| surface-container | #EBF1EB |
| surface-container-low | #EFF5EF |
| surface-container-high | #E5EBE5 |
| surface-container-highest | #DEE4DF |
| on-surface | #181C1A |
| on-surface-variant | #404943 |
| outline | #717972 |
| outline-variant | #BFC9C1 |
| error | #B3261E |
| on-error | #FFFFFF |

#### Orange

**Dark:**
| Token | Value |
|-------|-------|
| primary | #FFB877 |
| on-primary | #4C2700 |
| primary-container | #6C3A00 |
| on-primary-container | #FFDCBE |
| secondary | #DDC2A1 |
| on-secondary | #3E2D16 |
| secondary-container | #56432B |
| on-secondary-container | #FADEBB |
| surface | #18120B |
| surface-container | #261F18 |
| surface-container-low | #211A13 |
| surface-container-high | #302922 |
| surface-container-highest | #3B342D |
| on-surface | #EFE0CF |
| on-surface-variant | #D4C4B1 |
| outline | #9D8E7D |
| outline-variant | #524436 |
| error | #F2B8B5 |
| on-error | #601410 |

**Light:**
| Token | Value |
|-------|-------|
| primary | #8B5000 |
| on-primary | #FFFFFF |
| primary-container | #FFDCBE |
| on-primary-container | #2D1600 |
| secondary | #6F5B40 |
| on-secondary | #FFFFFF |
| secondary-container | #FADEBB |
| on-secondary-container | #271904 |
| surface | #FFF8F4 |
| surface-container | #F5EDEA |
| surface-container-low | #FAF2EE |
| surface-container-high | #EEE6E3 |
| surface-container-highest | #E9E1DD |
| on-surface | #211A13 |
| on-surface-variant | #524436 |
| outline | #847465 |
| outline-variant | #D4C4B1 |
| error | #B3261E |
| on-error | #FFFFFF |

### File Changes

#### 1. 新規ファイル: `src/settings/ui-theme-presets.ts`

プリセット定義を集約するモジュール。`ThemeColors` インターフェース、`PresetDefinition` インターフェース、および全プリセットの色定義マップを含む。

```typescript
export const UI_THEME_PRESETS: Record<UiThemePreset, PresetDefinition> = {
  purple: { dark: { ... }, light: { ... } },
  blue: { dark: { ... }, light: { ... } },
  green: { dark: { ... }, light: { ... } },
  orange: { dark: { ... }, light: { ... } },
};
```

CSS変数への適用ヘルパー関数:
```typescript
export function applyPresetColors(colors: ThemeColors): void {
  const root = document.documentElement;
  root.style.setProperty("--md-sys-color-primary", colors.primary);
  root.style.setProperty("--md-sys-color-on-primary", colors.onPrimary);
  // ... 全トークンを設定
}
```

#### 2. 変更: `src/settings/types.ts`

- `UiThemePreset` 型を追加
- `AppSettings` に `ui_theme_preset: UiThemePreset` を追加

#### 3. 変更: `src-tauri/src/commands/config.rs`

- `UiThemePreset` enum を追加（Purple, Blue, Green, Orange）
- `AppSettings` に `ui_theme_preset` フィールドを追加
- null デシリアライザを設定
- `Default` 実装を更新
- テストを追加

#### 4. 変更: `src/settings/settings-applier.ts`

- `applyUiTheme()` のシグネチャを変更: `applyUiTheme(theme: UiTheme, preset: UiThemePreset)` に拡張
- テーマモード解決後、プリセットデータからCSS変数を一括設定
- システムテーマのリスナーもプリセット対応に更新

#### 5. 変更: `src/styles.css`

- `:root` のMD3カラートークンのハードコード値を削除
- 初期値はJS側の `applyUiTheme()` で設定されるため、CSSにはフォールバック値のみ残す（またはJS初期化前の表示崩れ防止のためデフォルトのPurple Darkを残す）

#### 6. 変更: `src/settings/settings-sections.ts`

- UIテーマセレクト直下にプリセットセレクトを追加
- UIテーマ変更時にプリセットセレクトの表示を更新

#### 7. 変更: i18nファイル（4ファイル）

**`src/i18n/locales/en.json` / `src/i18n/locales/ja.json`:**
```json
"uiThemePreset": "Color Preset" / "カラープリセット",
"uiThemePresetDesc": "Changes the accent color of the UI" / "UIのアクセントカラーを変更します",
"presetPurple": "Purple" / "パープル",
"presetBlue": "Blue" / "ブルー",
"presetGreen": "Green" / "グリーン",
"presetOrange": "Orange" / "オレンジ"
```

**`src-tauri/locales/en.json` / `src-tauri/locales/ja.json`:**
変更不要（バリデーションメッセージの追加なし）

### File Structure

```
src/
├── settings/
│   ├── types.ts                  # UiThemePreset 型追加
│   ├── ui-theme-presets.ts       # 【新規】プリセット定義
│   ├── settings-applier.ts       # applyUiTheme() 拡張
│   └── settings-sections.ts      # プリセットセレクト追加
├── styles.css                    # ハードコードMD3色を調整
├── i18n/locales/
│   ├── en.json                   # プリセットラベル追加
│   └── ja.json                   # プリセットラベル追加
src-tauri/
├── src/commands/config.rs        # UiThemePreset enum & フィールド追加
├── locales/
│   ├── en.json                   # 変更なし
│   └── ja.json                   # 変更なし
```

### 設定UIフロー

```
Settings → Appearance → Theme & Color
├── UI Theme: [System ▼]        ← 既存セレクト（system / light / dark）
├── Color Preset: [Purple ▼]    ← 新規セレクト（purple / blue / green / orange）
└── Terminal Color Scheme: ...   ← 既存セレクト（変更なし）
```

プリセットセレクトは `ui_theme` の値に関わらず常に表示する。`system` 選択時はOSの設定に応じてプリセットのダーク/ライト版が自動適用される。

### テーマ適用フロー

```
applyUiTheme(theme, preset)
  │
  ├─ theme === "system"
  │    ├─ OS がダーク → resolved = "dark"
  │    └─ OS がライト → resolved = "light"
  │
  ├─ theme === "dark" → resolved = "dark"
  └─ theme === "light" → resolved = "light"
       │
       └─ colors = UI_THEME_PRESETS[preset][resolved]
            │
            └─ 全 CSS 変数を colors から設定
               root.style.setProperty("--md-sys-color-primary", colors.primary)
               root.style.setProperty("--md-sys-color-on-primary", colors.onPrimary)
               ...
```

## Test Scenarios

### Unit Tests

- [ ] `UI_THEME_PRESETS` が4つのプリセットを含む
- [ ] 各プリセットが `dark` と `light` の両方の色定義を持つ
- [ ] 各色定義が19個のMD3トークンすべてを含む
- [ ] `applyPresetColors()` が全CSS変数を正しく設定する
- [ ] `applyUiTheme("dark", "blue")` でBlueダークの色が適用される
- [ ] `applyUiTheme("light", "green")` でGreenライトの色が適用される
- [ ] `applyUiTheme("system", "orange")` でOS設定に応じた色が適用される

### Rust Tests

- [ ] `UiThemePreset` のデシリアライズ: `"purple"`, `"blue"`, `"green"`, `"orange"`
- [ ] `UiThemePreset` のデフォルト値が `Purple`
- [ ] `ui_theme_preset: null` のデシリアライズでデフォルト値
- [ ] `ui_theme_preset` 未設定のJSONから正常デシリアライズ
- [ ] 不正値のデシリアライズでエラー
- [ ] ラウンドトリップ（serialize → deserialize）

### Integration Tests

- [ ] 設定パネルでプリセット変更 → UI色が即座に反映
- [ ] 設定保存 → 再読込でプリセットが保持される
- [ ] UIテーマ切替時にプリセットが維持される
- [ ] システムテーマ + プリセット選択 → OS設定変更で明暗自動切替

### 後方互換性テスト

- [ ] `ui_theme_preset` フィールドが無い既存設定ファイル → Purple がデフォルト適用
- [ ] `ui_theme_preset: null` → Purple がデフォルト適用
- [ ] 既存の `ui_theme` 設定のみの場合 → 現在と同じ表示

## Error Handling

- 不正なプリセット値 → Rustデシリアライズエラー（`load_settings` でデフォルト設定にフォールバック）
- CSS変数設定失敗 → フォールバック値がCSSに残っているため表示崩れなし

## Security Considerations

- 入力バリデーション: プリセット値はenum型で制約されるため、不正値の注入リスクなし
- CSS変数: プリセットの色値はハードコード定数のみ使用（ユーザー入力なし）

## Success Criteria

- [ ] ダーク/ライトそれぞれ4種類のプリセットが選択可能
- [ ] プリセット変更が即座にUIに反映される
- [ ] システムテーマ選択時にOS設定に応じたプリセット明暗が適用される
- [ ] 設定が永続化され、アプリ再起動後も保持される
- [ ] 既存設定ファイルとの後方互換性が維持される
- [ ] 型チェック通過（`bun run typecheck`）
- [ ] 全テスト通過（`bun test` and `cargo test`）
