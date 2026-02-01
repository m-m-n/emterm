# Implementation Plan: Phase 1 - Font Family

## Overview

Font Family 設定を実際に機能させる。現在は `settings-applier.ts` が CSS 変数を設定し `notifyRenderers("fontFamily", ...)` を呼んでいるが、`CanvasRenderer.applySetting()` に `fontFamily` ケースがなく無視される。

## Objectives

- `CanvasRenderer` が `fontFamily` 通知を受け取り、フォントを切り替えて再描画する
- フォント変更後に文字幅・高さが再計測される
- 空文字列の場合はデフォルトのモノスペースフォントにフォールバックする

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/terminal/canvas-renderer.ts` | `applySetting()` に `fontFamily` ケースを追加 |

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/canvas-renderer.test.ts` に `applySetting("fontFamily", ...)` のテストケースを追加

2. **`applySetting()` に `fontFamily` ケースを追加**
   - `fontFamily` を受け取ったら、内部のフォントファミリーを更新する
   - 空文字列の場合はデフォルトモノスペース (例: `"monospace"`) にフォールバック

3. **フォント変更後の再計測と再描画**
   - フォントファミリー更新後、文字寸法を再計測する
   - 状態が存在する場合は全画面を再描画する

## Component Contracts

### `CanvasRenderer.applySetting("fontFamily", value)`

| Item | Description |
|------|-------------|
| Precondition | `value` は `string` 型 |
| Postcondition (normal) | `this.fontFamily` が更新され、`measureCharacterSize()` と `forceRender()` が実行される |
| Postcondition (empty) | 空文字列の場合、デフォルトモノスペースが使用される |

## Processing Flow

```
1. applySetting("fontFamily", value) が呼ばれる
2. value が空文字列か判定
   +-- 空 --> デフォルトモノスペースを使用
   +-- 非空 --> value をそのまま使用
3. 内部の fontFamily プロパティを更新
4. measureCharacterSize() を呼び出して文字寸法を再計測
5. pendingState が存在するか判定
   +-- 存在する --> forceRender() で全画面再描画
   +-- 存在しない --> 何もしない
```

## Test Strategy

### Test File: `src/terminal/canvas-renderer.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applySetting("fontFamily", "Fira Code")` updates font | フォントファミリーが更新されること |
| Empty string falls back to default monospace | 空文字列でデフォルトにフォールバック |

## Acceptance Criteria

- [ ] 設定でフォントファミリーを変更すると、ターミナルの表示フォントが変わる
- [ ] フォント変更後、文字幅・高さが再計測される
- [ ] 空文字列の場合はデフォルトのモノスペースフォントにフォールバックする
