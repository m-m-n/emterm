# Implementation Plan: Phase 5 - Padding

## Overview

Padding 設定を実際に機能させる。現在は `settings-applier.ts` が CSS 変数 `--terminal-padding` を設定しているが、この変数を参照する CSS ルールがない。

## Objectives

- CSS 変数 `--terminal-padding` を参照するルールを追加し、ターミナルコンテンツの周囲に余白を表示する
- パディング変更後にターミナルのカラム数・行数が再計算される

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/styles.css` | `.terminal-root` に `padding: var(--terminal-padding)` を追加 |
| `src/terminal-app/index.ts` | パディングを考慮したターミナルサイズ計算 |

## Implementation Steps

1. **テストを先に書く**
   - `src/settings/settings-applier.test.ts` で `applyPadding()` が CSS 変数を正しく設定することを確認

2. **CSS ルールの追加**
   - `.terminal-root` に `padding: var(--terminal-padding)` を適用

3. **ターミナルサイズ計算の更新**
   - `TerminalApp` のリサイズ計算で、コンテナサイズからパディング分を差し引く
   - `observeContainerResize` のコールバック内でパディングを考慮
   - パディング変更時にレンダラーのリサイズ通知が発生する（CSS padding 変更により ResizeObserver がトリガーされる）

## Component Contracts

### CSS: `.terminal-root` padding rule

| Item | Description |
|------|-------------|
| Precondition | CSS 変数 `--terminal-padding` が設定されている |
| Postcondition | `.terminal-root` の内側にパディングが適用される |

### `TerminalApp` resize calculation

| Item | Description |
|------|-------------|
| Precondition | コンテナのサイズが変更された（パディング変更を含む） |
| Postcondition | パディングを差し引いた利用可能領域でカラム数・行数が再計算される |

## Processing Flow

```
1. ユーザーがパディング値を変更
2. applyPadding() が --terminal-padding CSS 変数を設定（既存実装）
3. CSS ルールが .terminal-root にパディングを適用
4. ResizeObserver がサイズ変更を検出
5. コールバックがパディングを考慮してカラム数・行数を再計算
6. ターミナルとPTYがリサイズされる
```

## Test Strategy

### Test File: `src/settings/settings-applier.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applyPadding(8)` sets CSS variable | CSS 変数が正しく設定されること |

### Manual Tests

| Test Case | Description |
|-----------|-------------|
| Visual: padding change | パディング変更でターミナル周囲に余白が表示される |
| Cols/rows recalculated | パディング変更後にカラム数・行数が減少する |

## Acceptance Criteria

- [ ] 設定でパディングを変更すると、ターミナルの周囲に余白が表示される
- [ ] パディング変更後、ターミナルのカラム数・行数が再計算される
