# Implementation Plan: Phase 12 - Bell Action

## Overview

Bell Action 設定を実際に機能させる。現在は設定画面で値を保存するのみで、BEL 文字 (0x07) を受信した際のハンドリングコードが存在しない。`TerminalStateAccessor` に `onBell` コールバックを追加し、`TerminalApp` が登録する方式を採用する。

## Objectives

- `TerminalStateAccessor` に `onBell` コールバックを追加する
- `c0_handlers.ts` の `handleBel()` から `onBell` コールバックを呼び出す
- `TerminalApp` が `onBell` コールバックを登録し、設定に応じたアクションを実行する
  - "visual": 画面フラッシュ
  - "sound": ビープ音
  - "none": 何もしない

## Target Files

### Files to Modify

| File | Change Summary |
|------|----------------|
| `src/terminal/handlers/types.ts` | `TerminalStateAccessor` に `onBell?: () => void` コールバックを追加 |
| `src/terminal/state.ts` | `TerminalState` に `onBell` プロパティを実装 |
| `src/terminal/handlers/c0_handlers.ts` | `handleBel()` で `state.onBell?.()` を呼び出す |
| `src/terminal-app/index.ts` | `TerminalState` 作成後に `onBell` コールバックを登録し、設定に応じたアクションを実行 |
| `src/styles.css` | ビジュアルベルのフラッシュアニメーション CSS を追加 |

## Implementation Steps

1. **テストを先に書く**
   - `src/terminal/handlers/c0_handlers.test.ts` で `handleBel()` が `onBell` コールバックを呼び出すことを確認
   - BEL アクションのハンドリングロジックのテストを追加

2. **`TerminalStateAccessor` に `onBell` コールバックを追加**
   - `src/terminal/handlers/types.ts` の `TerminalStateAccessor` インタフェースに `onBell?: () => void` を追加
   - `src/terminal/state.ts` の `TerminalState` クラスに `onBell?: () => void` プロパティを追加

3. **`handleBel()` からコールバックを呼び出す**
   - `src/terminal/handlers/c0_handlers.ts` の `handleBel()` を更新:
     - `state.onBell?.()` を呼び出す（optional chaining でコールバック未登録時は no-op）

4. **`TerminalApp` で `onBell` コールバックを登録**
   - `TerminalState` 作成後に `state.onBell = () => this.handleBell()` を設定
   - `handleBell()` メソッドを `TerminalApp` に追加:
     - `SettingsService.getCached()` から `bell_action` を取得
     - "visual": ターミナルコンテナに CSS クラスを一時的に付与してフラッシュ効果
     - "sound": Web Audio API または `<audio>` 要素でビープ音を再生
     - "none": 何もしない

5. **CSS フラッシュアニメーション**
   - `.terminal-bell-flash` クラスとアニメーション定義
   - 短時間（100-200ms）の画面フラッシュ効果

## Component Contracts

### TerminalStateAccessor.onBell

| Item | Description |
|------|-------------|
| Precondition | `TerminalApp` が `TerminalState` 作成後にコールバックを登録 |
| Postcondition | `handleBel()` 呼び出し時にコールバックが実行される |

### handleBel() callback invocation

| Item | Description |
|------|-------------|
| Precondition | Execute(0x07) アクションが処理され、`state.onBell` が設定されている |
| Postcondition | `state.onBell()` が呼ばれ、`TerminalApp.handleBell()` が実行される |

### TerminalApp.handleBell()

| Item | Description |
|------|-------------|
| Precondition | `onBell` コールバック経由で呼ばれ、`bell_action` 設定が読み取り可能 |
| Postcondition ("visual") | ターミナルコンテナに短時間のフラッシュ効果が適用される |
| Postcondition ("sound") | ビープ音が再生される |
| Postcondition ("none") | 何もしない |

## Processing Flow

```
1. ANSI パーサーが BEL 文字 (0x07) を検出
2. TerminalState.processAction(Execute(0x07)) が呼ばれる
3. handleExecuteDispatch() -> handleBel(state) が呼ばれる
4. handleBel() が state.onBell?.() を呼び出す
5. TerminalApp が登録した onBell コールバックが実行される
6. TerminalApp.handleBell() が SettingsService.getCached() から bell_action を取得
7. bell_action の値に応じて分岐
   +-- "visual" --> ターミナルコンテナにフラッシュ CSS クラスを追加、一定時間後に除去
   +-- "sound" --> ビープ音を再生
   +-- "none" --> 何もしない
```

## Test Strategy

### Test File: `src/terminal/handlers/c0_handlers.test.ts`

| Test Case | Description |
|-----------|-------------|
| `handleBel()` calls onBell callback | `state.onBell` が呼ばれること |
| `handleBel()` without callback does nothing | コールバック未登録時に例外が発生しないこと |

### Test File: `src/terminal-app/` 配下に新規テストまたは既存テストに追加

| Test Case | Description |
|-----------|-------------|
| BEL with "visual" triggers flash | "visual" で画面フラッシュが発生すること |
| BEL with "sound" triggers beep | "sound" でビープ音再生メソッドが呼ばれること |
| BEL with "none" does nothing | "none" で何もしないこと |

## Acceptance Criteria

- [ ] `handleBel()` が `state.onBell?.()` コールバックを呼び出す
- [ ] `TerminalApp` が `onBell` コールバックを登録している
- [ ] "visual" で BEL 文字受信時に画面がフラッシュする
- [ ] "sound" で BEL 文字受信時にビープ音が鳴る
- [ ] "none" で BEL 文字受信時に何もしない
