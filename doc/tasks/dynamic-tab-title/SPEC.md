# SPEC: Dynamic Tab Title

## Overview

シェルが送信するOSCシーケンス（OSC 0 / OSC 2）によるタイトル変更をタブバーのタブタイトルに反映する。

## Current State (問題)

- OSC 0 / OSC 2 シーケンスを受信すると `TerminalState._title` は更新される
- ウィンドウタイトル（Tauriウィンドウ）は `TerminalApp` 内で更新される（`index.ts:286-295`）
- しかし、タブバーのタブタイトルには反映されない
- `TabBarUI.updateTabTitle()` メソッドは存在するが、呼び出されていない
- `tab:titleChanged` イベント型は定義済みだが、発行されていない

## Requirements

### FR-1: タブタイトルの動的更新

シェルがOSCシーケンスでタイトルを設定した場合、対応するタブのタイトルテキストをリアルタイムに更新する。

- OSC 0（SetTitleAndIcon）: タブタイトルを更新する
- OSC 2（SetTitle）: タブタイトルを更新する
- OSC 1（SetIconName）: タブタイトルには影響しない

### FR-2: タイトル未設定時のデフォルト

- タイトルが空文字列または未設定の場合、"Terminal" を表示する

### FR-3: 長いタイトルの省略表示

- タブ幅に収まらない長いタイトルは末尾を `...` で省略する（CSS `text-overflow: ellipsis`、既存実装を維持）

### FR-4: ツールチップによるフルタイトル表示

- タブにマウスカーソルをホバーした際、`title` 属性によるツールチップでフルサイズのタイトルを表示する
- タイトルが省略されていない場合もツールチップを表示する（一貫性のため）

### FR-5: Tab データモデルの同期

- `Tab.title` プロパティを更新し、タブデータモデルとUI表示を一致させる
- `tab:titleChanged` イベントを発行する

## Architecture

### データフロー

```
Shell (OSC 0/2)
  → ANSIパーサー (Rust)
    → OscAction::SetTitle / SetTitleAndIcon
      → フロントエンド handleSetTitle()
        → TerminalState._title 更新
          → TerminalApp (タイトル変更検知)
            → コールバック呼び出し
              → TabManager.updateTabTitle()
                → Tab.title 更新
                → tab:titleChanged イベント発行
                  → TabBarUI.updateTabTitle() (DOM更新 + ツールチップ)
```

### 変更対象ファイル

| ファイル | 変更内容 |
|---------|---------|
| `src/terminal-app/index.ts` | タイトル変更時にコールバックを呼び出す仕組みを追加 |
| `src/tab-bar/tab-manager.ts` | `updateTabTitle()` メソッドを追加、`tab:titleChanged` を発行、`createTerminalTabInternal()` 内でコールバック接続 |
| `src/tab-bar/tab-bar-ui.ts` | `updateTabTitle()` でツールチップ（`title` 属性）も更新 |

### 変更しないファイル

- `src/terminal/handlers/osc_handlers.ts` - 既存の `handleSetTitle()` はそのまま使用
- `src-tauri/src/ansi/parser.rs` - OSCパースは正しく動作している
- `src/tab-bar/types.ts` - `tab:titleChanged` イベント型は定義済み
- `src/styles/tab-bar.css` - `text-overflow: ellipsis` は既に適用済み

## Non-Goals

- タブタイトルのフォーマットルールのカスタマイズ設定
- OSC 1（アイコン名）によるタブへの影響
- タブのアイコン表示
