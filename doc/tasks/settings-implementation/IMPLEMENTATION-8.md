# Implementation Plan: Phase 8 - Terminal Color Scheme

## Overview

Terminal Color Scheme 設定を実際に機能させる。現在はドロップダウンに "default" のみが表示され、プリセットの検索・適用処理がない。6 種のカラースキームプリセットを定義し、選択時にターミナルの色パレットを変更する。

## Objectives

- 6 種のカラースキームプリセットを定義する
- ドロップダウンにプリセットを追加し、"eMterm" を先頭に表示する
- スキーム選択時に Canvas レンダラーの色パレットを更新して再描画する

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/terminal/colors.ts` | カラースキームプリセットのデータ定義を追加 |
| `src/settings/settings-applier.ts` | `RendererSettings` に `colorScheme` を追加、`applyTerminalColorScheme()` でプリセット検索・`notifyRenderers` 呼び出し |
| `src/settings/settings-panel.ts` | ドロップダウンにプリセット選択肢を追加 |
| `src/terminal/canvas-renderer.ts` | 動的カラーパレットをサポート。`applySetting("colorScheme", ...)` ケースを追加。`renderLine()` と `renderCursorArea()` で可変色を使用 |

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/colors.test.ts` にプリセットデータの存在・構造テストを追加
   - `src/settings/settings-applier.test.ts` にスキーム適用テストを追加

2. **カラースキームプリセットを定義**
   - `src/terminal/colors.ts` に 6 スキームのデータを追加
   - 各スキーム: foreground, background, cursor, selection, 16 ANSI colors
   - プリセット名: "emterm" (デフォルト), "solarized-dark", "solarized-light", "monokai", "dracula", "nord"

3. **`settings-panel.ts` のドロップダウン更新**
   - "default" の代わりにプリセット名のリストを表示
   - "eMterm" を先頭に配置

4. **`settings-applier.ts` のスキーム適用処理**
   - プリセット名からプリセットデータを検索
   - レンダラーに色パレット変更を通知

5. **`canvas-renderer.ts` の動的パレット対応**
   - `DEFAULT_BACKGROUND` / `DEFAULT_FOREGROUND` の直接参照を、可変のパレットプロパティに置換
   - `applySetting()` に `colorScheme` ケースを追加
   - パレット変更後に全画面再描画

## Component Contracts

### Color Scheme Preset (data)

| Item | Description |
|------|-------------|
| Structure | `{ name, foreground, background, cursor, selection, ansiColors[16] }` |
| Constraint | 全 6 プリセットが定義されている。"emterm" がデフォルト |

### `applyTerminalColorScheme(scheme)` (updated)

| Item | Description |
|------|-------------|
| Precondition | `scheme` はプリセット名またはデフォルトを表す文字列 |
| Postcondition (default/emterm) | カスタム色をクリアし、デフォルトパレットに戻る |
| Postcondition (preset) | 指定プリセットの色パレットがレンダラーに適用される |

### `CanvasRenderer` dynamic palette

| Item | Description |
|------|-------------|
| Precondition | カラーパレットがプロパティとして保持されている |
| Postcondition | `renderLine()`, `renderCursorArea()`, `forceRender()` が現在のパレットを使用する |

## Processing Flow

```
1. ユーザーがカラースキームを選択
2. applyTerminalColorScheme(scheme) が呼ばれる
3. プリセット名でプリセットデータを検索
   +-- "emterm" or empty --> デフォルトパレットに戻す
   +-- 他のプリセット --> 対応するプリセットデータを取得
4. レンダラーに色パレット変更を通知
5. CanvasRenderer がパレットを更新
6. 全画面を再描画
```

## Test Strategy

### Test File: `src/terminal/colors.test.ts`

| Test Case | Description |
|-----------|-------------|
| All 6 presets exist | 6 つのプリセットが定義されていること |
| Each preset has required fields | 各プリセットに foreground, background, 16 ANSI colors があること |
| "emterm" preset matches DEFAULT values | デフォルトプリセットが既存定数と一致すること |

### Test File: `src/settings/settings-applier.test.ts`

| Test Case | Description |
|-----------|-------------|
| Selecting a scheme updates color variables | スキーム選択で色が更新されること |
| "default" clears custom overrides | デフォルトでカスタム色がクリアされること |

## Acceptance Criteria

- [ ] 6 種のカラースキームプリセットが選択可能
- [ ] "eMterm" がドロップダウンの先頭に表示される
- [ ] スキーム変更時にターミナルの色が変わる
- [ ] "eMterm" でデフォルトカラーに戻る
- [ ] Canvas レンダラーが新しい色パレットを使って再描画する
