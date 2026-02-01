# Implementation Plan: Phase 4 - Opacity

## Overview

Opacity 設定を実際に機能させる。現在は `settings-applier.ts` が CSS 変数 `--terminal-opacity` を設定しているが、この変数を参照するコードが存在しない。`applySetting` 経由で `CanvasRenderer` に通知し、Canvas 背景描画時のアルファチャンネルに反映する。

## Objectives

- `RendererSettings` に `opacity` プロパティを追加し、既存の通知パターンで Canvas レンダラーに反映する
- Canvas 背景描画時にアルファチャンネルとして opacity を適用する
- テキストは完全に不透明を維持する

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/settings/settings-applier.ts` | `RendererSettings` に `opacity: number` を追加、`applyOpacity()` で `notifyRenderers("opacity", opacity)` を呼び出す |
| `src/terminal/canvas-renderer.ts` | `applySetting()` に `opacity` case を追加、背景描画で alpha チャンネルに反映する `setOpacity()` メソッドを追加 |

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/canvas-renderer.test.ts` で `applySetting("opacity", 0.5)` が内部 opacity プロパティを更新することを確認
   - `src/settings/settings-applier.test.ts` で `applyOpacity()` が CSS 変数を正しく設定し、レンダラーに通知することを確認

2. **`RendererSettings` に `opacity` を追加**
   - `src/settings/settings-applier.ts` の `RendererSettings` インタフェースに `opacity: number` を追加
   - `applyOpacity()` に `notifyRenderers("opacity", opacity)` を追加

3. **Canvas レンダラーの背景描画を更新**
   - `CanvasRenderer` に `private opacity: number = 1.0` プロパティを追加
   - `setOpacity(opacity: number)` メソッドを追加:
     - `this.opacity` を更新
     - `this.forceRender()` を呼び出して再描画
   - `applySetting()` switch 文に `opacity` case を追加
   - 背景描画 (`fillRect`) 時に背景色の alpha チャンネルに `this.opacity` を適用
   - CSS の `opacity` プロパティは Canvas 内のテキストも透明にするため使用しない

## Component Contracts

### RendererSettings.opacity

| Item | Description |
|------|-------------|
| Precondition | `applyOpacity()` が呼ばれ、0.3-1.0 の範囲の値が渡される |
| Postcondition | `notifyRenderers("opacity", value)` 経由で全レンダラーに通知される |

### CanvasRenderer.setOpacity()

| Item | Description |
|------|-------------|
| Precondition | `applySetting("opacity", value)` が呼ばれる |
| Postcondition | 背景描画時に alpha チャンネルが opacity 値で描画される。テキストは完全不透明を維持 |

## Processing Flow

```
1. ユーザーが透明度スライダーを操作
2. applyOpacity() が --terminal-opacity CSS 変数を設定（既存実装）
3. applyOpacity() が notifyRenderers("opacity", opacity) を呼び出す
4. tabManager.updateAllTerminalsSetting("opacity", opacity) が全ターミナルに伝搬
5. CanvasRenderer.applySetting("opacity", value) が呼ばれる
6. setOpacity(value) が this.opacity を更新し forceRender() で再描画
7. 背景描画時に fillRect の色に this.opacity を alpha として適用
```

## Test Strategy

### Test File: `src/terminal/canvas-renderer.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applySetting("opacity", 0.5)` updates opacity | opacity プロパティが 0.5 に更新されること |
| `applySetting("opacity", 1.0)` sets full opacity | 完全不透明が設定されること |
| `setOpacity()` triggers forceRender | forceRender が呼ばれること |

### Test File: `src/settings/settings-applier.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applyOpacity(0.5)` sets CSS variable | CSS 変数が正しく設定されること |
| `applyOpacity(0.5)` notifies renderers | notifyRenderers が呼ばれること |

### Manual Tests

| Test Case | Description |
|-----------|-------------|
| Visual: opacity 0.3 | 最小透明度でもコンテンツが視認可能 |
| Visual: opacity slider | スライダー操作でリアルタイムに透明度が変わる |

## Acceptance Criteria

- [ ] 設定で透明度を変更すると、ターミナル背景の透明度が反映される
- [ ] テキストは完全に不透明を維持する
- [ ] 最小値 0.3 でも内容が視認可能
