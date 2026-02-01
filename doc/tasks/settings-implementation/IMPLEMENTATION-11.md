# Implementation Plan: Phase 11 - Scroll Speed

## Overview

Scroll Speed 設定を実際に機能させる。現在は設定画面の `onInput` が空関数で、スクロール処理がスクロール速度設定を参照していない。Phase 9 (Scrollback Lines) に依存する。

## Dependencies

- **Phase 9 (Scrollback Lines)** が先に実装されていること

## Objectives

- マウスホイールによるスクロール量に、スクロール速度の乗数を適用する
- 値が大きいほどスクロール量が多い

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| Phase 9 で追加されるマウスホイールハンドラ | スクロール速度設定値を乗数として適用 |

## Implementation Steps

1. **テストを先に書く**
   - マウスホイールハンドラのテストにスクロール速度の乗数テストを追加

2. **マウスホイールハンドラでスクロール速度を適用**
   - `SettingsService.getCached()` から `scroll_speed` を取得
   - ホイールデルタにスクロール速度値を乗算
   - 結果のオフセットをスクロールバック位置に適用

## Component Contracts

### Mouse wheel handler with scroll speed

| Item | Description |
|------|-------------|
| Precondition | Phase 9 のマウスホイールハンドラが存在し、`scroll_speed` 設定が読み取り可能 |
| Postcondition | ホイールデルタが `scroll_speed` で乗算されてスクロールオフセットに適用される |

## Processing Flow

```
1. マウスホイールイベント発生
2. SettingsService.getCached() から scroll_speed を取得
3. ホイールデルタ * scroll_speed でスクロール量を計算
4. スクロールオフセットを更新
5. 可視行を再描画
```

## Test Strategy

### Test File: Phase 9 で作成されるテストファイルに追加

| Test Case | Description |
|-----------|-------------|
| Scroll speed multiplier affects scroll amount | スクロール速度が量に反映されること |
| Speed 1 scrolls minimum amount | 最小速度で最小量スクロール |
| Speed 10 scrolls maximum amount | 最大速度で最大量スクロール |

## Acceptance Criteria

- [ ] スクロール速度の設定値がスクロール量に反映される
- [ ] 値が大きいほどスクロール量が多い
