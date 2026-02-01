# Implementation Plan: Phase 9 - Scrollback Lines

## Overview

Scrollback Lines 設定を実際に機能させる。スクロールバック機能自体が未実装であり、`getVisibleLines()` のコメントに "Scrollback buffer support will be added later" と記載されている。この Phase は Phase 6 (Show Scrollbar) と Phase 11 (Scroll Speed) の前提条件となる。

## Objectives

- スクロールバックバッファを `TerminalState` に追加し、画面外にスクロールした行を保持する
- マウスホイールで過去の出力にスクロールできるようにする
- スクロール中に新しい出力が来た場合、スクロール位置を維持する
- 設定変更は次のセッションから適用される

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/terminal/state.ts` | スクロールバックバッファの追加、バッファサイズ制限 |
| `src/terminal/canvas-renderer.ts` | `getVisibleLines()` をスクロールバック対応に更新、スクロールオフセット管理 |
| `src/terminal-app/index.ts` | マウスホイールイベントハンドラの追加 |

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/state.test.ts` にスクロールバックバッファのテストを追加
   - `src/terminal/canvas-renderer.test.ts` に `getVisibleLines()` のスクロールバック対応テストを追加

2. **`TerminalState` にスクロールバックバッファを追加**
   - 画面上端からスクロールアウトした行を保存するバッファ
   - バッファサイズを `scrollback_lines` 設定値に制限
   - 行がスクロールアウトする処理（スクロールアップ操作、新しい行の追加）でバッファに追加

3. **スクロールオフセットの管理**
   - `scrollOffset` プロパティ (0 = 最新表示、正の値 = 過去方向へのスクロール行数)
   - スクロールオフセットの範囲制限 (0 から scrollbackバッファの行数まで)

4. **`getVisibleLines()` の更新**
   - `scrollOffset` が 0 のとき: 現在の画面バッファを返す（現在の動作）
   - `scrollOffset` > 0 のとき: スクロールバックバッファと画面バッファを結合し、オフセットに基づく行を返す

5. **マウスホイールイベントハンドラ**
   - `TerminalApp` でマウスホイールイベントを監視
   - ホイール上: スクロールオフセットを増加（過去方向）
   - ホイール下: スクロールオフセットを減少（最新方向）
   - スクロール位置変更後にレンダラーを再描画

6. **スクロール中の新出力の処理**
   - スクロールオフセット > 0 の場合、新しい出力が来てもオフセットを維持する
   - ユーザーが最下部までスクロールしたら自動スクロールに戻る

## Component Contracts

### Scrollback Buffer in `TerminalState`

| Item | Description |
|------|-------------|
| Precondition | `scrollback_lines` が設定で定義されている |
| Postcondition | 画面上端からスクロールアウトした行が最大 `scrollback_lines` 行保持される |
| Overflow behavior | バッファが上限に達したら、最も古い行を削除 |

### `getVisibleLines(state, scrollOffset)` (updated)

| Item | Description |
|------|-------------|
| Precondition | `scrollOffset` >= 0 |
| Postcondition (offset=0) | 現在の画面バッファの行を返す |
| Postcondition (offset>0) | スクロールバックバッファと画面バッファの結合から、オフセット位置の行を返す |

### Mouse wheel scroll handler

| Item | Description |
|------|-------------|
| Precondition | マウスホイールイベントが発生 |
| Postcondition | スクロールオフセットが更新され、可視行が再描画される |

## Processing Flow

```
1. 行がスクロールアウトする
   +-- TerminalState がスクロールバックバッファに行を追加
   +-- バッファサイズが上限を超えたら古い行を削除

2. マウスホイールイベント
   +-- ホイール上 --> scrollOffset を増加（上限はバッファサイズ）
   +-- ホイール下 --> scrollOffset を減少（下限は 0）
   +-- scrollOffset が変更された場合
       +-- getVisibleLines() でオフセット位置の行を取得
       +-- レンダラーを再描画

3. 新しい出力到着時
   +-- scrollOffset == 0 --> 通常描画（自動スクロール）
   +-- scrollOffset > 0 --> スクロール位置を維持、自動スクロールしない
```

## Test Strategy

### Test File: `src/terminal/state.test.ts`

| Test Case | Description |
|-----------|-------------|
| Lines pushed off screen are saved to scrollback | スクロールアウトした行がバッファに保存される |
| Buffer respects size limit | バッファサイズが設定値以内に制限される |
| Buffer overflow drops oldest lines | 上限超過時に最も古い行が削除される |

### Test File: `src/terminal/canvas-renderer.test.ts`

| Test Case | Description |
|-----------|-------------|
| `getVisibleLines()` with offset 0 returns screen buffer | オフセット 0 で画面バッファを返す |
| `getVisibleLines()` with offset > 0 returns scrollback lines | オフセット > 0 でスクロールバック行を返す |

## Acceptance Criteria

- [ ] 設定したスクロールバック行数分の履歴が保持される
- [ ] マウスホイールで過去の出力にスクロールできる
- [ ] スクロール中に新しい出力が来てもスクロール位置が維持される
- [ ] スクロールバック行数の設定変更が次のセッションから適用される
