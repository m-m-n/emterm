# Implementation Plan: Phase 2 - Line Height

## Overview

Line Height 設定を実際に機能させる。現在は `settings-applier.ts` が `notifyRenderers("lineHeight", ...)` を呼んでいるが、`CanvasRenderer` に対応ケースがなく、`measureCharacterSize()` は fontSize から固定計算で行高を算出している。

## Objectives

- `CanvasRenderer` が `lineHeight` 通知を受け取り、行間を更新して再描画する
- `measureCharacterSize()` が設定値の行高乗数を使用する

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/terminal/canvas-renderer.ts` | `lineHeight` プロパティ追加、`applySetting()` にケース追加、`measureCharacterSize()` を設定値対応に更新 |

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/canvas-renderer.test.ts` に `applySetting("lineHeight", ...)` のテストケースを追加

2. **`CanvasRenderer` に `lineHeight` プロパティを追加**
   - デフォルト値は現在の計算式から導出される値と同等

3. **`measureCharacterSize()` を更新**
   - 固定の `fontSize + 2` 計算式ではなく、格納された `lineHeight` 乗数を使用する

4. **`applySetting()` に `lineHeight` ケースを追加**
   - `lineHeight` を受け取ったら内部プロパティを更新
   - 文字寸法を再計測して再描画する

## Component Contracts

### `CanvasRenderer.applySetting("lineHeight", value)`

| Item | Description |
|------|-------------|
| Precondition | `value` は `number` 型 (0.8 - 3.0) |
| Postcondition | `this.lineHeight` が更新され、`measureCharacterSize()` と `forceRender()` が実行される |

### `measureCharacterSize()` (updated)

| Item | Description |
|------|-------------|
| Precondition | `this.lineHeight` と `this.fontSize` が有効な値 |
| Postcondition | `this.charHeight` が `fontSize * lineHeight` に基づく値になる |

## Processing Flow

```
1. applySetting("lineHeight", value) が呼ばれる
2. 内部の lineHeight プロパティを更新
3. measureCharacterSize() を呼び出し
   +-- charHeight を fontSize * lineHeight 乗数で計算
4. pendingState が存在するか判定
   +-- 存在する --> forceRender() で全画面再描画
   +-- 存在しない --> 何もしない
```

## Test Strategy

### Test File: `src/terminal/canvas-renderer.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applySetting("lineHeight", 1.5)` updates character height | 行高が更新されること |
| Line height affects `getCharHeight()` return value | charHeight が lineHeight に基づいて計算されること |

## Acceptance Criteria

- [ ] 設定で行の高さを変更すると、ターミナルの行間が変わる
- [ ] 行の高さ変更後、文字高さが再計測される
