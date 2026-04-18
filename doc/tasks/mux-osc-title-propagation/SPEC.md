# SPEC: Mux OSC Title Propagation

## Overview

mux daemon が各ペインのPTY出力から OSC タイトル（OSC 0 / OSC 2）を検出し、`SessionManager` 内の `window.name` を更新する。接続中のGUIクライアントがあれば `RenameWindow` メッセージで通知し、未接続の場合でも内部状態を最新に保つこと。

## Current State（問題）

### 症状
- `emterm mux new-window` （CLI経由）で作成したウィンドウは、zsh が発行する OSC タイトルが反映されず `"Terminal"` のままとなる
- GUI 接続前に OSC タイトルが発行された場合、以降のアタッチ時にもタイトルが反映されない

### 原因
1. `title_tx` / `title_rx` が GUI 接続ごと・CLI 接続ごとに作成されており、接続終了時に受信側 (`title_rx`) が破棄される
2. CLI 経由で作られたペインの `pane.title_sender` には短命の `title_tx` が格納されており、OSC タイトル発行時に `try_send` がサイレント失敗する
3. `window.name` の更新は GUI 接続ハンドラの `title_rx.recv()` 内でのみ行われるため、GUI 未接続時の OSC タイトルは失われる
4. reattach 時に `title_sender` は差し替えられるが、`last_title` キャッシュの差分検出により検出済みタイトルは再送されない

## Requirements

### FR-1: daemon 主導のタイトル更新

- daemon 起動時に daemon-level の `title_tx` / `title_rx` を作成し、daemon 寿命と同等に維持すること
- 全てのペイン（GUI 経由・CLI 経由を問わず）の `pane.title_sender` は daemon-level の `title_tx` を参照すること
- daemon 内で `title_rx` を受信する専用タスクを起動し、`SessionManager::rename_window` を呼び出して `window.name` を更新すること
- 本処理はクライアント接続の有無に依存せず常時動作すること

### FR-2: 接続 GUI への通知

- `window.name` 更新時、`SessionManager.notify_tx`（broadcast チャネル）で `RenameWindow` メッセージを発行すること
- 各 GUI 接続ハンドラは既存の `notify_rx` 転送ロジックで `RenameWindow` をクライアントへ送信すること
- 複数 GUI 接続時は全接続へ通知されること

### FR-3: アタッチ時の整合性

- `Welcome` メッセージに含まれる `session_list` は、アタッチ時点の最新 `window.name` を反映すること
- アタッチ後、以前検出済みのタイトルを再送する処理は不要（Welcome 経由で反映済みのため）

### FR-4: 差分検出の維持

- 同一タイトルの連続通知は抑止すること（現行の `last_title` 差分検出を維持）
- daemon 側でも `window.name` と受信タイトルを比較し、同一なら broadcast を抑止すること

### FR-5: CLI `new-window` 経路

- CLI 経由で作成したペインにも daemon-level `title_tx` がセットされ、shell の OSC タイトル発行時に `window.name` が更新されること
- CLI 接続終了後も pane 内の `title_sender` は有効であり続けること
- GUI 接続終了 (detach) 後も pane 内の `title_sender` は有効であり続けること（detach 中に shell が OSC を発行しても `window.name` が更新されること）

### FR-6: アタッチ時 Welcome ↔ notify 購読の順序保証

- `notify_tx.subscribe()` は `Welcome` を送信するより前に実行し、購読後のスナップショットで `session_list` を構築すること
- これにより、`Welcome` 構築直後から購読開始までの窓で発生した `RenameWindow` の取りこぼしを防ぐこと

### FR-7: 複数ペインとタイトル所有

- 1 ウィンドウに複数ペインが存在する場合、どのペインの OSC タイトルでも `window.name` を更新する（現行実装と同じ挙動を維持）
- ペインごとのタイトル独立管理はスコープ外

## Architecture

### データフロー

```
shell (OSC 0/2)
  → pty_reader_loop (pty_spawn.rs)
    → shadow_parser.process() → screen().title()
      → last_title 差分検出
        → title_sender.try_send((pane_id, title))    [daemon-level title_tx]
          → daemon title-update task (daemon.rs)
            → SessionManager::rename_window(sid, wid, title)
            → SessionManager.notify_tx.send(RenameWindow)
              → 各 GUI connection の notify_rx.recv() (connection.rs)
                → framed.send(RenameWindow) → クライアント
```

### 変更対象ファイル

- `src-tauri/src/mux/daemon.rs`
  - daemon-level `title_tx` / `title_rx` 作成
  - title 更新タスクの spawn
- `src-tauri/src/mux/ipc/connection.rs`
  - `handle_connection` / `handle_cli_client` が daemon-level `title_tx` を受け取る
  - GUI 接続内の個別 `title_rx` ハンドリング削除
  - CLI 接続内のダミー `title_tx` 作成削除
- `src-tauri/src/mux/ipc/reattach.rs`
  - `detach_session_panes` 内で `pane.title_sender = None` と上書きしていた処理を削除し、daemon-level `title_tx` を保持し続けること
  - `collect_reattach_data` の `title_sender` 差し替えは維持（渡す tx が daemon-level になるだけ）
- `src-tauri/src/mux/session/manager.rs`（必要に応じ）
  - rename_window と notify_tx 送信を atomic に行うヘルパ（任意、なくても良い）

### 変更しないファイル

- `src-tauri/src/mux/ipc/pty_spawn.rs`: pty_reader_loop の検出ロジックは不変
- `src-tauri/src/mux/session/pane.rs`: `SharedTitleSender` 設計は不変
- フロントエンド側 (`src/terminal-app/`, `src/terminal/mux/`) の `RenameWindow` 受信処理は不変

## Non-Goals

- OSC 7（cwd）や他の OSC シーケンスの扱いは対象外
- `--name` 指定なし時のデフォルト値 `"Terminal"` の変更は対象外
- フロントエンド側のタブタイトル描画ロジック変更は対象外
- 非 mux モード（直接 PTY 接続時）のタイトル処理は対象外

## Test Scenarios

### Unit Tests

- `SessionManager::rename_window` が既存／非既存ウィンドウで期待通り動作すること（既存テスト維持）
- daemon-level title タスクが `rename_window` と `notify_tx.send` を呼ぶこと

### Integration Tests

- CLI `new-window` 後に shell が OSC 0/2 を発行 → `window.name` が更新されること（GUI 未接続）
- 上記状態で GUI アタッチ → `Welcome.session_list` に最新タイトルが含まれること
- GUI 接続中に OSC タイトル発行 → `RenameWindow` が GUI に届くこと
- 複数 GUI 接続時、全 GUI に `RenameWindow` が届くこと
- GUI detach 後（`detach_session_panes` 経由）に OSC タイトルを発行 → `window.name` が更新されること（detach 中でも title_sender が有効）
- 次回アタッチ時の `Welcome.session_list` に detach 中に変更された最新タイトルが含まれること

### Edge Cases

- shell が同じタイトルを連続発行 → broadcast は 1 回のみ
- 空タイトル (`""`) → 既存通り無視
- reattach 直後の既存タイトル → 追加通知不要（Welcome で反映済み）

## Success Criteria

- `~/bin/init-mux` 起動後、全ウィンドウのタブ名が zsh の OSC タイトルで上書きされていること
- GUI アタッチ前に発行された OSC タイトルが、アタッチ時にタブ名へ反映されていること
- 手動で `new-window` した直後のウィンドウでも、zsh の OSC で名前が更新されること

## References

- 関連コミット: `1dc8b44 fix(mux): move OSC title detection to daemon to fix rename race`
- プロトコル定義: `doc/tasks/mux-osc-handshake/SPEC.md`
- フロントエンド側タイトル仕様（非 mux）: `doc/tasks/dynamic-tab-title/SPEC.md`
