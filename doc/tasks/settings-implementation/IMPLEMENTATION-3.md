# Implementation Plan: Phase 3 - UI Theme

## Overview

UI Theme 設定を実際に機能させる。現在は `settings-applier.ts` が `data-theme` 属性を `<html>` 要素に設定しているが、`[data-theme]` セレクタを持つ CSS ルールが存在せず、テーマ切替に視覚的効果がない。

## Objectives

- ライトテーマ用の CSS ルールを追加し、テーマ切替で UI 配色が変わるようにする
- タブバー、設定パネルの色がテーマに応じて変わる
- "system" テーマで OS 設定に追従する

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/styles.css` | `:root[data-theme="light"]` ブロックで MD3 ライトテーマカラートークンを上書き |
| `src/styles/settings-panel.css` | 必要に応じてテーマ対応の色調整 |
| `src/styles/tab-bar.css` | 必要に応じてテーマ対応の色調整 |

## Implementation Steps

1. **テストを先に書く**
   - `src/settings/settings-applier.test.ts` に `applyUiTheme()` で `data-theme` 属性が正しく設定されることの確認（既存テストを確認し不足分を追加）

2. **ライトテーマの CSS カラートークン定義**
   - `src/styles.css` に `:root[data-theme="light"]` ブロックを追加
   - MD3 ライトテーマの色トークンで `:root` のダーク値を上書き
   - 対象トークン: `--md-sys-color-primary`, `--md-sys-color-surface`, `--md-sys-color-on-surface` 等

3. **ターミナル背景のテーマ対応**
   - `#terminal` と `.tab-content` の `background-color` がテーマトークンを参照するよう調整

4. **タブバー・設定パネルの確認**
   - 既に MD3 トークンを使用している場合は追加変更不要
   - ハードコードされた色がある場合はトークン参照に置換

## Component Contracts

### CSS: `:root[data-theme="light"]`

| Item | Description |
|------|-------------|
| Precondition | `<html>` 要素に `data-theme="light"` 属性が設定されている |
| Postcondition | MD3 カラートークンがライトテーマの値に上書きされる |

### CSS: `:root[data-theme="dark"]` (default)

| Item | Description |
|------|-------------|
| Precondition | `data-theme="dark"` または属性なし |
| Postcondition | 既存のダークテーマ色がそのまま使用される |

## Processing Flow

```
1. ユーザーがテーマを選択
2. applyUiTheme() が data-theme 属性を設定（既存実装）
   +-- "light" --> data-theme="light" を設定
   +-- "dark" --> data-theme="dark" を設定
   +-- "system" --> OS設定を検出して "light" or "dark" を設定、変更リスナー登録
3. CSS セレクタ [data-theme="light"] が一致
4. MD3 カラートークンがライト値に上書き
5. タブバー、設定パネル、ターミナル背景が新しい色を反映
```

## Test Strategy

### Test File: `src/settings/settings-applier.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applyUiTheme("dark")` sets `data-theme="dark"` | ダークテーマの属性設定 |
| `applyUiTheme("light")` sets `data-theme="light"` | ライトテーマの属性設定 |
| `applyUiTheme("system")` respects OS preference | システムテーマの追従 |

### Manual Tests

| Test Case | Description |
|-----------|-------------|
| Visual: light theme | ライトテーマで UI 配色がライトになること |
| Visual: dark theme | ダークテーマで UI 配色がダークになること |
| Visual: system theme | OS 設定変更に追従すること |

## Acceptance Criteria

- [ ] "dark" テーマでダーク配色が適用される
- [ ] "light" テーマでライト配色が適用される
- [ ] "system" テーマで OS 設定に追従する
- [ ] テーマ切替時にタブバー、設定パネルの色が変わる
