# Implementation Plan: Phase 7 - Cursor Style / Cursor Blink

## Overview

Cursor Style と Cursor Blink 設定を実際に機能させる。現在は `settings-applier.ts` が `notifyRenderers("cursorStyle", ...)` と `notifyRenderers("cursorBlink", ...)` を呼んでいるが、`CanvasRenderer.applySetting()` に対応ケースがなく無視される。

## Objectives

- `CanvasRenderer` が `cursorStyle` 通知を受け取り、カーソル形状を変更する
- `CanvasRenderer` が `cursorBlink` 通知を受け取り、点滅の有無を制御する

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/terminal/canvas-renderer.ts` | `applySetting()` に `cursorStyle` と `cursorBlink` ケースを追加 |

**Note:** `CursorState.style` と `TerminalModes.cursorBlink` はともに public プロパティのため、`state.ts` へのセッター追加は不要。

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/canvas-renderer.test.ts` に `applySetting("cursorStyle", ...)` と `applySetting("cursorBlink", ...)` のテストケースを追加

2. **`TerminalState` のカーソル設定更新メカニズムの確認**
   - `state.cursorStyle` は `cursor.style` から取得（読み取り専用の getter）
   - 設定変更で更新するためのセッターが必要かどうかを確認
   - `modes.cursorBlink` は既に存在する

3. **`applySetting()` に `cursorStyle` ケースを追加**
   - ターミナル状態のカーソルスタイルを更新
   - カーソル領域を再描画

4. **`applySetting()` に `cursorBlink` ケースを追加**
   - ターミナル状態のカーソル点滅モードを更新
   - 点滅 ON: カーソル点滅タイマーを開始
   - 点滅 OFF: カーソル点滅タイマーを停止、カーソルを可視状態にして再描画

## Component Contracts

### `CanvasRenderer.applySetting("cursorStyle", value)`

| Item | Description |
|------|-------------|
| Precondition | `value` は `"block"`, `"underline"`, `"bar"` のいずれか |
| Postcondition | カーソルスタイルが更新され、カーソル領域が再描画される |

### `CanvasRenderer.applySetting("cursorBlink", value)`

| Item | Description |
|------|-------------|
| Precondition | `value` は `boolean` 型 |
| Postcondition (true) | カーソル点滅タイマーが開始される |
| Postcondition (false) | カーソル点滅タイマーが停止され、カーソルが可視状態になる |

## Processing Flow

```
1. applySetting("cursorStyle", value) が呼ばれる
   +-- TerminalState のカーソルスタイルを更新
   +-- pendingState が存在する場合、カーソル領域を再描画

2. applySetting("cursorBlink", value) が呼ばれる
   +-- value が true の場合
   |   +-- startCursorBlink() を呼び出し
   +-- value が false の場合
   |   +-- stopCursorBlink() を呼び出し
   |   +-- カーソルを可視状態にして再描画
   +-- TerminalState のカーソル点滅モードを更新
```

## Test Strategy

### Test File: `src/terminal/canvas-renderer.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applySetting("cursorStyle", "bar")` changes cursor style | カーソルスタイルが変更されること |
| `applySetting("cursorStyle", "underline")` changes cursor style | underline スタイルへの変更 |
| `applySetting("cursorBlink", false)` stops blink timer | 点滅タイマーが停止されること |
| `applySetting("cursorBlink", true)` starts blink timer | 点滅タイマーが開始されること |

## Acceptance Criteria

- [ ] 設定でカーソルスタイルを変更すると、カーソル形状がリアルタイムに変わる
- [ ] 設定でカーソル点滅を OFF にすると、カーソルが点滅しなくなる
- [ ] 設定でカーソル点滅を ON にすると、カーソルが点滅する
