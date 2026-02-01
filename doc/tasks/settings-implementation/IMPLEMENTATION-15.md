# Implementation Plan: Phase 15 - Keybinds

## Overview

Keybinds 設定を実際に機能させる。現在はキーバインドがハードコードされており（`tab-bar/keyboard-handler.ts` と `terminal-app/handlers/keyboard.ts`）、設定で変更した値が参照されない。

## Objectives

- キーバインド設定を変更すると、対応するショートカットキーが変わる
- デフォルトのキーバインドが初期値として機能する
- キーバインドの衝突がある場合、後に設定された方が優先される

## Target Files

### Files to Create

| File | Purpose |
|------|---------|
| `src/keybind/matcher.ts` | キーバインド文字列のパースとキーイベントとのマッチングユーティリティ |

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/tab-bar/keyboard-handler.ts` | ハードコードされたキーバインドを設定値からの読み取りに変更 |
| `src/terminal-app/handlers/keyboard.ts` | クリップボード関連のキーバインドを設定値からの読み取りに変更 |

## Implementation Steps

1. **テストを先に書く**
   - `src/keybind/matcher.test.ts` を新規作成
   - キーバインド文字列のパースとマッチングテスト

2. **キーバインドマッチングユーティリティの作成**
   - `src/keybind/matcher.ts` を新規作成
   - キーバインド文字列（例: "Ctrl+Shift+T"）をパースし、構成要素（修飾キー + メインキー）に分解
   - `KeyboardEvent` のプロパティと比較してマッチングを判定

3. **`TabKeyboardHandler` の更新**
   - `handleKeyDown()` 内のハードコードされた条件分岐を、設定値ベースのマッチングに変更
   - `SettingsService.getCached()` から `keybinds` オブジェクトを取得
   - 各キーイベントを設定されたキーバインドとマッチング

4. **`KeyboardHandler`（terminal-app）の更新**
   - クリップボードショートカット (copy, paste, select_all) を設定値からの読み取りに変更
   - `SettingsService.getCached()` から `keybinds` を取得してマッチング

5. **ランタイムでのキーバインド変更対応**
   - 設定保存時にキャッシュが更新されるため、ハンドラは毎回キャッシュから最新値を読み取る

## Component Contracts

### Keybind Matcher

| Item | Description |
|------|-------------|
| Input (parse) | キーバインド文字列（例: "Ctrl+Shift+T"） |
| Output (parse) | 構成要素オブジェクト `{ ctrlKey, shiftKey, altKey, metaKey, key }` |
| Input (match) | `KeyboardEvent` と構成要素オブジェクト |
| Output (match) | `boolean` (一致するか) |

### `TabKeyboardHandler.handleKeyDown()` (updated)

| Item | Description |
|------|-------------|
| Precondition | `KeyboardEvent` が発生し、`SettingsService.getCached()` が有効 |
| Postcondition | 設定されたキーバインドに基づいてアクションを実行 |
| Fallback | 設定が取得できない場合はデフォルトのキーバインドを使用 |

### `KeyboardHandler` clipboard shortcuts (updated)

| Item | Description |
|------|-------------|
| Precondition | `KeyboardEvent` が発生し、`SettingsService.getCached()` が有効 |
| Postcondition | 設定されたクリップボードキーバインドに基づいてコピー/ペーストを実行 |

## Processing Flow

```
1. キーダウンイベント発生
2. SettingsService.getCached() から keybinds を取得
   +-- 取得できない場合 --> デフォルトのキーバインドを使用
3. イベントを各設定されたキーバインドとマッチング
   +-- new_tab に一致 --> 新規タブ作成
   +-- close_tab に一致 --> アクティブタブを閉じる
   +-- next_tab に一致 --> 次のタブに切替
   +-- prev_tab に一致 --> 前のタブに切替
   +-- copy に一致 --> コピー実行
   +-- paste に一致 --> ペースト実行
   +-- ... (他のアクション)
4. 一致したアクションを実行
```

## Test Strategy

### Test File: `src/keybind/matcher.test.ts` (new)

| Test Case | Description |
|-----------|-------------|
| Parses "Ctrl+T" correctly | 修飾キー + メインキーが正しくパースされること |
| Parses "Ctrl+Shift+T" correctly | 複数修飾キーのパース |
| Parses single key "F11" correctly | 単独キーのパース |
| Matches KeyboardEvent against keybind | イベントとキーバインドの一致判定 |
| Non-matching event returns false | 不一致の場合 false |
| Case-insensitive key matching | 大文字小文字を区別しないマッチング |

### Test File: `src/tab-bar/keyboard-handler.test.ts`

| Test Case | Description |
|-----------|-------------|
| Custom keybind triggers correct action | カスタムキーバインドが正しいアクションをトリガーすること |
| Default keybinds work without custom settings | デフォルトキーバインドが機能すること |

## Acceptance Criteria

- [ ] 設定で変更したキーバインドが実際のショートカットとして動作する
- [ ] デフォルトのキーバインドが初期値として機能する
- [ ] キーバインドの衝突がある場合、後に設定された方が優先される
