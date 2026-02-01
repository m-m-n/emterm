# Implementation Plan: Phase 6 - Show Scrollbar

## Overview

Show Scrollbar 設定を実際に機能させる。現在は CSS 変数 `--terminal-scrollbar-mode` が設定されるが、参照するルールがない。`applyScrollbar()` で CSS 変数値を `overflow-y` に対応する値にマッピングし、CSS 側で `var()` で参照する方式を採用する。Phase 9 (Scrollback Lines) に依存する。

## Dependencies

- **Phase 9 (Scrollback Lines)** が先に実装されていること

## Objectives

- `applyScrollbar()` で設定値を CSS の `overflow-y` 値にマッピングする
- CSS 変数 `var()` 参照でスクロールバーの表示モードを制御する
- "always" / "never" / "auto" の 3 モードを実装

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/settings/settings-applier.ts` | `applyScrollbar()` で always->scroll, never->hidden, auto->auto にマッピングして CSS 変数に設定 |
| `src/styles.css` | スクロールコンテナに `overflow-y: var(--terminal-scrollbar-overflow)` を適用 |

## Implementation Steps

1. **テストを先に書く**
   - `src/settings/settings-applier.test.ts` で `applyScrollbar()` がマッピング後の CSS 変数を正しく設定することを確認

2. **`applyScrollbar()` のマッピング処理を実装**
   - 設定値から CSS `overflow-y` 値へのマッピング:
     - `"always"` -> `"scroll"`
     - `"never"` -> `"hidden"`
     - `"auto"` -> `"auto"`
   - マッピング後の値を CSS 変数 `--terminal-scrollbar-overflow` に設定
   - 既存の `--terminal-scrollbar-mode` も引き続き設定（他のコンポーネントが参照する可能性）

3. **CSS ルールの追加**
   - Phase 9 で作成されるスクロール可能コンテナに対し、`overflow-y: var(--terminal-scrollbar-overflow, auto)` を適用
   - デフォルト値として `auto` を指定

4. **カスタムスクロールバースタイリング**
   - `::-webkit-scrollbar` 擬似要素でスクロールバーの外観をカスタマイズ

## Component Contracts

### applyScrollbar() mapping

| Item | Description |
|------|-------------|
| Precondition | ScrollbarMode 値 ("always" / "never" / "auto") が渡される |
| Postcondition | `--terminal-scrollbar-overflow` に対応する CSS overflow-y 値が設定される |

### CSS scrollbar rules

| Item | Description |
|------|-------------|
| Precondition | Phase 9 のスクロール可能コンテナが存在し、`--terminal-scrollbar-overflow` が設定されている |
| Postcondition | スクロールバーがモードに応じて表示/非表示される |

## Processing Flow

```
1. ユーザーがスクロールバーモードを選択
2. applyScrollbar() がモード値を overflow-y 値にマッピング
   +-- "always" --> "scroll"
   +-- "never" --> "hidden"
   +-- "auto" --> "auto"
3. マッピング後の値を --terminal-scrollbar-overflow CSS 変数に設定
4. CSS ルールが var(--terminal-scrollbar-overflow) でスクロールコンテナの overflow-y を更新
```

## Test Strategy

### Test File: `src/settings/settings-applier.test.ts`

| Test Case | Description |
|-----------|-------------|
| `applyScrollbar("always")` sets overflow variable to "scroll" | `--terminal-scrollbar-overflow` が `scroll` に設定されること |
| `applyScrollbar("never")` sets overflow variable to "hidden" | `--terminal-scrollbar-overflow` が `hidden` に設定されること |
| `applyScrollbar("auto")` sets overflow variable to "auto" | `--terminal-scrollbar-overflow` が `auto` に設定されること |

### Manual Tests (Phase 9 完了後)

| Test Case | Description |
|-----------|-------------|
| "always" shows scrollbar | スクロールバーが常時表示される |
| "never" hides scrollbar | スクロールバーが非表示になる |
| "auto" shows scrollbar only when scrollable | スクロール可能時のみ表示 |

## Acceptance Criteria

- [ ] "always" でスクロールバーが常時表示される
- [ ] "never" でスクロールバーが非表示になる
- [ ] "auto" でスクロール可能時のみスクロールバーが表示される
