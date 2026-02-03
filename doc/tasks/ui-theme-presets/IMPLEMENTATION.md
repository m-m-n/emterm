# Implementation Plan: UI Theme Color Presets

## Overview

UIテーマ（タブバー・設定パネル等のMaterial Design 3カラー）に、ダーク/ライトそれぞれ4種類の色相バリエーションプリセット（Purple, Blue, Green, Orange）を追加し、2段階選択UIで適用する。

## Objectives

- ダーク/ライトそれぞれ4種類のMD3カラープリセットを定義・適用可能にする
- `applyUiTheme()` をプリセット対応に拡張し、CSS変数を動的に設定する
- 設定UIにプリセット選択セレクトボックスを追加する
- 設定の永続化と後方互換性を維持する

## Prerequisites

### Development Environment
- Bun (package manager / test runner)
- Rust toolchain (for backend changes)
- Tauri CLI

### Dependencies
- 既存のMD3カラートークンCSS変数システム（`styles.css`）
- 設定パネルのコンポーネントシステム（`settings-components.ts`）
- Rust側の設定モデル（`config.rs`）

### Knowledge Requirements
- eMterm設定パネルのアーキテクチャ（sections / applier / types の分離）
- serde のカスタムデシリアライズパターン（`deserialize_null_default`）
- MD3カラートークン体系

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), Rust (backend)
- **Framework**: Tauri
- **Key Libraries**:
  - serde (Rust serialization)
  - vanilla TypeScript DOM API (UI rendering)

### Design Approach

プリセット定義を単一モジュール（`ui-theme-presets.ts`）に集約し、CSS変数の動的設定で配色を切り替える。CSSの `:root` にはPurple Darkのフォールバック値を残し、`:root[data-theme="light"]` を削除してJS側からの動的設定に移行する。

### Component Interaction

```
Settings UI (settings-sections.ts)
  |-- onSave --> applyUiTheme(theme, preset) --> settings-applier.ts
                    |-- resolves dark/light
                    |-- looks up preset colors --> ui-theme-presets.ts
                    |-- sets CSS variables on :root
  |-- onSave --> saveSetting("ui_theme_preset", value) --> Rust config.rs
```

## Implementation Phases

### Phase 1: Data Layer (Rust + TypeScript Type Definitions)

**Goal**: `ui_theme_preset` フィールドをRust/TypeScript両方の設定モデルに追加し、既存設定ファイルとの後方互換性を確保する。

**Files to Create**:
- なし

**Files to Modify**:
- `src-tauri/src/commands/config.rs` - `UiThemePreset` enum と `AppSettings` フィールド追加
- `src/settings/types.ts` - `UiThemePreset` 型と `AppSettings` フィールド追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| UiThemePreset (Rust) | プリセット値のenum定義・シリアライズ | なし | Purple/Blue/Green/Orangeの4値を持つenum |
| UiThemePreset (TS) | プリセット値のUnion型定義 | なし | 4つのstring literalのUnion型 |
| AppSettings.ui_theme_preset (Rust) | 設定フィールド | デフォルトPurple, null安全 | JSONから正常にデシリアライズ |
| AppSettings.ui_theme_preset (TS) | 設定フィールド | Rustモデルと一致 | 型チェック通過 |

**Processing Flow**:
```
1. Rust: UiThemePreset enum を定義
   ├─ derive: Debug, Clone, Default, Serialize, Deserialize, PartialEq
   └─ serde rename_all = "lowercase"
2. Rust: AppSettings に ui_theme_preset フィールドを追加
   ├─ serde(default) で未設定時にPurpleデフォルト
   └─ deserialize_null_default でnull値にPurpleデフォルト
3. Rust: Default impl を更新
4. TypeScript: UiThemePreset 型を追加
5. TypeScript: AppSettings に ui_theme_preset フィールドを追加
```

**Implementation Steps**:

1. **Rust enum と AppSettings フィールドの追加**
   - 既存の `UiTheme` enum と同じパターンで `UiThemePreset` を定義
   - `AppSettings` struct に `ui_theme_preset` フィールドを追加
   - `Default` impl に `UiThemePreset::Purple` を追加
   - Key considerations:
     - `deserialize_null_default` を使い、null値に対するデフォルト処理を既存パターンに合わせる
     - `#[serde(rename_all = "lowercase")]` でJSON値を小文字に統一

2. **TypeScript 型定義の追加**
   - `UiThemePreset` Union型を定義
   - `AppSettings` インターフェースに `ui_theme_preset` を追加
   - Key considerations:
     - Rust側と値が一致すること

3. **Rust テストの追加**
   - Key considerations:
     - 既存テストパターン（`test_deserialize_null_*`, `test_deserialize_empty_json`, round-trip）に合わせる

**Dependencies**:
- Requires: なし
- Blocks: Phase 2, Phase 3

**Testing Approach**:

*Unit Tests (Rust)*:
- `UiThemePreset` のデフォルト値が Purple であること
- 各値（"purple", "blue", "green", "orange"）の正常デシリアライズ
- null 値のデシリアライズでデフォルト（Purple）
- 未設定フィールドのデシリアライズでデフォルト
- 不正値のデシリアライズでエラー
- ラウンドトリップ（serialize -> deserialize）

*Unit Tests (TypeScript)*:
- `makeSettings()` ヘルパーに `ui_theme_preset` が含まれること（テストファイル更新）

**Acceptance Criteria**:
- [ ] `cargo test` が全テスト通過
- [ ] `bun run typecheck` がエラーなし
- [ ] 既存の設定ファイル（`ui_theme_preset` 未設定）がデフォルト値で読み込める

**Estimated Effort**: 小 (1-2 days)

**Risks and Mitigation**:
- **Risk**: 既存設定ファイルの後方互換性
  - **Mitigation**: `serde(default)` + `deserialize_null_default` パターンで確実に対応

---

### Phase 2: Preset Data and Theme Application Logic

**Goal**: プリセットの色定義データモジュールを作成し、`applyUiTheme()` をプリセット対応に拡張する。CSSの `:root[data-theme="light"]` を削除し、`:root` にはPurple Darkフォールバック値を残す。

**Files to Create**:
- `src/settings/ui-theme-presets.ts` - プリセット定義と CSS変数適用ヘルパー

**Files to Modify**:
- `src/settings/settings-applier.ts` - `applyUiTheme()` のシグネチャ変更とプリセット適用ロジック
- `src/styles.css` - `:root` と `:root[data-theme="light"]` のMD3カラートークン削除
- `src/settings/settings-applier.test.ts` - テスト更新

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| UI_THEME_PRESETS | 全プリセットのダーク/ライト色定義を保持 | なし | 4プリセット x 2モード x 19トークン |
| applyPresetColors | ThemeColors オブジェクトからCSS変数を一括設定 | document.documentElement が存在 | 全19個のCSS変数が設定される |
| applyUiTheme (拡張) | テーマモードとプリセットの組み合わせでUI色を適用 | プリセットデータが利用可能 | data-theme属性設定 + CSS変数設定 |

**Processing Flow**:
```
applyUiTheme(theme, preset)
  1. 既存のシステムテーマリスナーをクリーンアップ
  2. テーマモード解決
     ├─ "system" → OS設定からdark/lightを判定
     ├─ "dark" → "dark"
     └─ "light" → "light"
  3. data-theme 属性を設定
  4. プリセットデータから色定義を取得
     ├─ UI_THEME_PRESETS[preset] が undefined → "purple" にフォールバック
     └─ UI_THEME_PRESETS[preset][resolved]
  5. 全CSS変数をプリセットの色値で設定
  6. "system" の場合、メディアクエリリスナーを登録
     └─ リスナー内: data-theme切替 + プリセット色の再適用
```

**Implementation Steps**:

1. **`ui-theme-presets.ts` の作成**
   - `ThemeColors` インターフェース（19個のMD3カラートークン）を定義
   - `PresetDefinition` インターフェース（dark/light の ThemeColors ペア）を定義
   - `UI_THEME_PRESETS` 定数マップ（4プリセット分のデータ）を定義
   - `applyPresetColors()` ヘルパー関数を定義
   - Key considerations:
     - SPEC.md のカラーテーブルの値をそのまま使用
     - CSS変数名は既存の `--md-sys-color-*` パターンに合わせる

2. **`applyUiTheme()` の拡張**
   - シグネチャに `preset` 引数を追加
   - テーマ解決後にプリセット色を CSS変数に適用するロジックを追加
   - システムテーマリスナー内でもプリセット色を再適用
   - Key considerations:
     - `applySettings()` の呼び出し箇所も更新が必要
     - 既存の `data-theme` 属性設定は維持（他CSSルールが参照している可能性）
     - `UI_THEME_PRESETS[preset]` が undefined の場合は `"purple"` にフォールバックする（設定ファイルからの不正値に対する防御）

3. **`styles.css` のMD3カラートークン整理**
   - `:root` ブロックのMD3カラートークン（`--md-sys-color-*`）をPurple Darkの値に統一（フォールバック用）
   - `:root[data-theme="light"]` ブロック全体を削除（ライト色はJS側から動的に設定）
   - Shape / Motion トークンは維持
   - Key considerations:
     - JS実行前のFOUC防止として、`:root` にデフォルトプリセット（Purple Dark）のフォールバック値を残す
     - JS実行後はプリセットに応じた色で上書きされるため、フォールバック値は初期表示時のみ使用される

4. **テストの更新**
   - Key considerations:
     - `applyUiTheme` のテストに preset 引数を追加
     - CSS変数設定の検証を追加

**Dependencies**:
- Requires: Phase 1（`UiThemePreset` 型が必要）
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests*:
- `UI_THEME_PRESETS` が4つのプリセットを含むこと
- 各プリセットが dark と light の両方を持つこと
- 各色定義が19個のMD3トークンすべてを含むこと
- `applyPresetColors()` が全CSS変数を正しく設定すること
- `applyUiTheme("dark", "blue")` で Blue ダークの色が適用されること
- `applyUiTheme("light", "green")` で Green ライトの色が適用されること
- `applyUiTheme("system", "orange")` で OS設定に応じた色が適用されること
- システムテーマ変更リスナーでプリセット色が再適用されること

**Acceptance Criteria**:
- [ ] プリセット定義が全4種のダーク/ライト色を含む
- [ ] `applyUiTheme()` にプリセット引数を渡して色適用が機能する
- [ ] CSSの `:root[data-theme="light"]` が削除され、`:root` にPurple Darkフォールバック値が残っている
- [ ] `bun test` のsettings-applier テストが全通過
- [ ] `bun run typecheck` がエラーなし

**Estimated Effort**: 中 (3-5 days)

**Risks and Mitigation**:
- **Risk**: CSS変更により、JS実行前に色が未設定の瞬間が発生する可能性
  - **Mitigation**: `:root` にPurple Darkのフォールバック値を残すことでFOUCを防止。JS実行後はプリセットに応じた値で上書きされる

---

### Phase 3: Settings UI and i18n

**Goal**: 設定パネルにプリセット選択セレクトボックスを追加し、i18n対応を完了する。

**Files to Modify**:
- `src/settings/settings-sections.ts` - プリセットセレクト追加
- `src/i18n/locales/en.json` - プリセット関連ラベル追加
- `src/i18n/locales/ja.json` - プリセット関連ラベル追加
- `src/settings/settings-panel.test.ts` - `makeSettings()` ヘルパー更新

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Preset Select UI | プリセット選択セレクトボックスの描画 | settings-sections.ts のレンダリングパターン | UIテーマ選択の直下にプリセット選択が表示される |
| i18n keys | プリセットラベルの多言語対応 | i18nシステムが稼働 | en/ja の両方でラベル表示 |

**Processing Flow**:
```
Settings Panel Rendering (Appearance > Theme & Color)
  1. UIテーマセレクト描画（既存）
  2. プリセットセレクト描画（新規）
     ├─ key: "ui-theme-preset"
     ├─ options: purple / blue / green / orange
     └─ onSave: applyUiTheme(currentTheme, newPreset) + saveSetting
  3. Terminal Color Schemeセレクト描画（既存）
```

**Implementation Steps**:

1. **i18n キーの追加**
   - `en.json` / `ja.json` に6つのキーを追加
     - `uiThemePreset` (ラベル)
     - `uiThemePresetDesc` (説明)
     - `presetPurple`, `presetBlue`, `presetGreen`, `presetOrange` (選択肢)
   - Key considerations:
     - 既存の `settings.appearance` ネスト構造に追加

2. **プリセットセレクトの追加**
   - `renderAppearanceSection()` 内の UIテーマセレクト直下に追加
   - 既存の `renderSelect()` コンポーネントを使用
   - `onSave` コールバックで `applyUiTheme()` と `saveSetting()` を実行
   - Key considerations:
     - `applyUiTheme()` に現在のテーマ値とプリセット値の両方を渡す必要がある
     - UIテーマの `onSave` も更新して、テーマ変更時にプリセットを維持する

3. **`applySettings()` の呼び出し更新**
   - `applySettings()` 内の `applyUiTheme()` 呼び出しにプリセット引数を追加
   - Key considerations:
     - `applySettings()` のシグネチャは変更不要（`AppSettings` から直接取得）

4. **テストヘルパーの更新**
   - `makeSettings()` に `ui_theme_preset: "purple"` を追加
   - 更新対象ファイル（全2箇所）:
     - `src/settings/settings-applier.test.ts`
     - `src/settings/settings-panel.test.ts`
   - Key considerations:
     - 上記が `makeSettings()` を持つ全テストファイル。他のテストファイルには `AppSettings` のリテラルは存在しない
     - typecheckで更新漏れは型エラーとして検出される

**Dependencies**:
- Requires: Phase 1, Phase 2
- Blocks: なし

**Testing Approach**:

*Unit Tests*:
- プリセットセレクト要素がDOMに存在すること
- セレクト変更時に `applyUiTheme()` が正しい引数で呼ばれること
- i18nキーが en / ja 両方に存在すること

*Integration Tests (Manual)*:
- 設定パネルでプリセット変更 -> UI色が即座に反映
- 設定保存 -> 再読込でプリセットが保持される
- UIテーマ切替時にプリセットが維持される
- システムテーマ + プリセット選択 -> OS設定変更で明暗自動切替

**Acceptance Criteria**:
- [ ] 設定パネルの "Theme & Color" セクションにプリセットセレクトが表示される
- [ ] Purple / Blue / Green / Orange の4選択肢が表示される
- [ ] プリセット変更が即座にUIに反映される
- [ ] 設定が永続化され、アプリ再起動後も保持される
- [ ] en/ja 両方のi18nキーが正しく表示される
- [ ] `bun test` 全通過
- [ ] `bun run typecheck` エラーなし

**Estimated Effort**: 小 (1-2 days)

**Risks and Mitigation**:
- **Risk**: UIテーマ変更時のプリセット連動漏れ
  - **Mitigation**: UIテーマの `onSave` を明示的に更新し、テーマ変更時に現在のプリセットを維持して再適用

---

## Complete File Structure

```
src/
├── settings/
│   ├── types.ts                  # UiThemePreset 型追加
│   ├── ui-theme-presets.ts       # [NEW] プリセット定義 + CSS変数適用ヘルパー
│   ├── settings-applier.ts       # applyUiTheme() 拡張
│   ├── settings-applier.test.ts  # テスト更新
│   ├── settings-sections.ts      # プリセットセレクト追加
│   └── settings-panel.test.ts    # makeSettings() 更新
├── styles.css                    # MD3カラートークンのハードコード削除
├── i18n/locales/
│   ├── en.json                   # プリセットラベル追加
│   └── ja.json                   # プリセットラベル追加
src-tauri/
├── src/commands/config.rs        # UiThemePreset enum & フィールド追加
```

**File Descriptions**:
- `ui-theme-presets.ts`: プリセットの色定義データとCSS変数適用関数を集約。新プリセット追加時はこのファイルのみ変更
- `settings-applier.ts`: `applyUiTheme()` を拡張し、テーマモードとプリセットの組み合わせでCSS変数を設定
- `types.ts`: `UiThemePreset` Union型と `AppSettings` フィールドの型定義
- `config.rs`: `UiThemePreset` Rust enum と `AppSettings` フィールド。serde設定で後方互換性を確保
- `settings-sections.ts`: UIテーマセレクト直下にプリセットセレクトを配置
- `styles.css`: `:root` にPurple Darkフォールバック値を維持し、`:root[data-theme="light"]` を削除（ライト色はJS動的設定に移行）

## Testing Strategy

### Unit Testing

**Approach**:
- Bun の `bun:test` を使用（TypeScript）
- Rust の `#[cfg(test)]` を使用
- 既存のモックパターン（`mockStyle`, `mockAttributes`, `mockMediaQueryList`）を踏襲

**Test Coverage Goals**:
- プリセットデータ完全性: 100%
- CSS変数適用ロジック: 90%+
- Rust enum デシリアライズ: 100%

**Key Test Areas**:

1. **プリセットデータ** (`ui-theme-presets.ts`)
   - 全プリセット存在確認
   - dark/light 両方の定義存在
   - 全19トークンの存在

2. **テーマ適用ロジック** (`settings-applier.ts`)
   - dark + 各プリセット -> 正しいCSS変数
   - light + 各プリセット -> 正しいCSS変数
   - system + プリセット -> OS設定に応じた色
   - システムテーマリスナー内でのプリセット再適用

3. **Rust 設定モデル** (`config.rs`)
   - enum デシリアライズ（正常値 / null / 未設定 / 不正値）
   - デフォルト値
   - ラウンドトリップ

### Manual Testing Checklist

- [ ] ダークテーマ + 各プリセット: タブバー・設定パネルの色が変わる
- [ ] ライトテーマ + 各プリセット: タブバー・設定パネルの色が変わる
- [ ] システムテーマ + プリセット: OS設定に応じた色が適用される
- [ ] プリセット変更後、アプリ再起動で設定が保持される
- [ ] 既存設定ファイル（`ui_theme_preset` なし）でアプリ起動: Purple がデフォルト適用
- [ ] テーマ切替時にプリセットが維持される

## Dependencies

### External Dependencies

変更なし（新規ライブラリの追加なし）

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Data Layer (Rust + TypeScript型定義)
2. Phase 2: Preset Data + Theme Application Logic
3. Phase 3: Settings UI + i18n

**Component Dependencies**:
- `ui-theme-presets.ts` -> `types.ts` (UiThemePreset 型)
- `settings-applier.ts` -> `ui-theme-presets.ts` (プリセットデータ + 適用関数)
- `settings-sections.ts` -> `settings-applier.ts` (applyUiTheme)
- `settings-sections.ts` -> `types.ts` (UiThemePreset 型)

## Risk Assessment

### Technical Risks

1. **CSS変更による初期表示の色なし状態**
   - **Risk**: JS実行前にMD3カラーが未設定になる瞬間
   - **Likelihood**: Low（`:root` にPurple Darkフォールバック値を残すため）
   - **Impact**: Low（フォールバック値で表示される）
   - **Mitigation**: `:root` にPurple Darkのフォールバック値を維持。`:root[data-theme="light"]` のみ削除

2. **システムテーマリスナーとプリセットの組み合わせ**
   - **Risk**: リスナー内でプリセット値を正しく参照できない（クロージャ問題）
   - **Likelihood**: Low
   - **Impact**: Medium（テーマ切替時に色が正しくない）
   - **Mitigation**: リスナー登録時にプリセット値をクロージャにキャプチャ

### Implementation Risks

1. **複数ファイルの makeSettings() 更新漏れ**
   - **Risk**: テストファイルの `makeSettings()` に `ui_theme_preset` を追加し忘れる
   - **Mitigation**: typecheck で型エラーとして検出される

## Performance Considerations

- プリセット切替はCSS変数の書き換えのみ（DOMリフロー不要）
- `UI_THEME_PRESETS` は静的定数オブジェクトのため、ランタイムコスト最小
- CSS変数の設定は19個のプロパティ設定（十分高速）

## Security Considerations

- プリセット値はenum/Union型で制約されるため、不正値注入リスクなし
- CSS変数の値はハードコード定数のみ使用（ユーザー入力を含まない）

## Open Questions

### From Specification:
- なし（仕様が十分に明確）

### Implementation-Specific:
- [x] CSSからMD3カラートークンを完全削除するか、フォールバックとしてPurple Darkを残すか → **決定: `:root` にPurple Darkフォールバック値を残す。`:root[data-theme="light"]` は削除（ライト色はJS側から動的設定）**

## Future Enhancements

- 新しい色相プリセットの追加（`ui-theme-presets.ts` にデータ追加のみ）
- カスタムカラープリセット（ユーザー定義色）

## Success Metrics

### Functional Completeness
- [ ] 全4プリセットがダーク/ライト両方で選択可能
- [ ] プリセット変更が即座にUIに反映
- [ ] システムテーマ選択時にOS設定に応じたプリセット明暗が適用
- [ ] 設定が永続化され、アプリ再起動後も保持

### Quality Metrics
- [ ] 全テスト通過（`bun test` + `cargo test`）
- [ ] 型チェック通過（`bun run typecheck`）
- [ ] 既存設定ファイルとの後方互換性維持

### User Experience
- [ ] 設定パネルでの選択操作が直感的
- [ ] プリセット切替がラグなく反映

## References

- **Specification**: `doc/tasks/ui-theme-presets/SPEC.md`
- **Material Design 3 Color System**: https://m3.material.io/styles/color
- **Existing Pattern**: `src/settings/settings-applier.ts` (applyUiTheme)
- **Existing Pattern**: `src-tauri/src/commands/config.rs` (UiTheme enum)
