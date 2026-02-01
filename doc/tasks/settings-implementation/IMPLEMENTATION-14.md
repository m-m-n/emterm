# Implementation Plan: Phase 14 - Copy on Select

## Overview

Copy on Select 設定を実際に機能させる。現在は設定画面で値を保存するのみで、`SelectionController` が `copy_on_select` 設定を参照していない。

## Objectives

- `copy_on_select` が ON の場合、テキスト選択完了時（mouseup）に自動でクリップボードにコピーする
- OFF の場合は選択だけではコピーされない

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/selection-v2/SelectionController.ts` | `onMouseUp()` で `copy_on_select` 設定を確認し、ON の場合にクリップボードコピーを実行 |

## Implementation Steps

1. **テストを先に書く**
   - `src/selection-v2/SelectionController.test.ts` または既存テストファイルに、copy on select のテストを追加

2. **`SelectionController.onMouseUp()` を更新**
   - 選択が完了した時点で `SettingsService.getCached()` から `copy_on_select` を取得
   - `copy_on_select` が true の場合、選択テキストをクリップボードにコピー
   - 既存の `copy()` メソッドを再利用

## Component Contracts

### `SelectionController.onMouseUp()` (updated)

| Item | Description |
|------|-------------|
| Precondition | マウスアップイベントが発生し、アクティブな選択が存在する |
| Postcondition (copy_on_select=true) | 選択テキストが自動的にクリップボードにコピーされる |
| Postcondition (copy_on_select=false) | 選択のみで、クリップボードへのコピーは行われない |

## Processing Flow

```
1. マウスアップイベントが発生
2. アクティブな選択が存在するか確認
   +-- 存在しない --> 何もしない
   +-- 存在する --> 続行
3. model.endSelection() を呼ぶ（既存処理）
4. SettingsService.getCached() から copy_on_select を取得
5. copy_on_select の値を判定
   +-- true --> copy() メソッドで選択テキストをクリップボードにコピー
   +-- false --> 何もしない
```

## Test Strategy

### Test File: `src/selection-v2/` 配下に追加

| Test Case | Description |
|-----------|-------------|
| Selection completion triggers copy when ON | ON 時に選択完了でコピーされること |
| Selection completion does not copy when OFF | OFF 時に選択完了でコピーされないこと |

## Acceptance Criteria

- [ ] 設定 ON 時、テキスト選択完了でクリップボードにコピーされる
- [ ] 設定 OFF 時、選択だけではコピーされない
