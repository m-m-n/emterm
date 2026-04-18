# Implementation Plan: Mux OSC Title Propagation

## Overview

mux daemon に daemon-level の title チャネルを導入し、接続クライアントの有無・種別に依存せずに全ペインの OSC タイトルを `SessionManager::window.name` に反映し、接続中の GUI へ `RenameWindow` broadcast で通知すること。

## Objectives

- daemon 起動時に `title_tx` / `title_rx` を一度だけ生成し、daemon 寿命と同等に維持すること
- daemon 内の title 更新タスクで `rename_window` と `notify_tx.send(RenameWindow)` を行うこと
- GUI 接続ハンドラ内の個別 `title_rx` 処理、CLI 接続ハンドラ内のダミー `title_tx` 生成を削除すること
- CLI 経由で作成されたペインでも shell の OSC タイトルが `window.name` に反映されること
- detach（GUI 切断）時に `pane.title_sender` を null 化していた処理を止め、daemon-level `title_tx` を保持し続けること
- GUI 接続ハンドラでは `notify_tx.subscribe()` を `Welcome` 送信前に済ませ、アタッチ時のレースを排除すること

## Prerequisites

### Development Environment

- Rust toolchain（既存プロジェクトと同一）
- Docker + docker compose（テスト実行用）

### Dependencies

- `tokio::sync::mpsc`（既存）
- `tokio::sync::broadcast`（`SessionManager.notify_tx` 経由で既存）
- `SessionManager::rename_window`（変更なし）
- `SessionManager::notify_tx()`（変更なし）

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: Tokio async runtime
- **Key Libraries**:
  - `tokio::sync::mpsc` - daemon-level title チャネル
  - `tokio::sync::broadcast` - GUI 接続への通知

### Design Approach

短命チャネルを daemon 寿命のチャネルに昇格すること。GUI 接続ハンドラは「GUI への `RenameWindow` 送信」責務のみを持ち、ペインの title 更新は daemon 内の専用タスクに一元化すること。

### Component Interaction

```
pty_reader_loop (既存)
  → pane.title_sender (daemon-level title_tx 参照)
    → daemon title-update task (新設)
        → SessionManager::rename_window
        → SessionManager.notify_tx.send(RenameWindow)
            → 各 GUI connection の notify_rx.recv() (既存)
                → framed.send(RenameWindow) → クライアント
```

## Implementation Phases

### Phase 1: daemon-level title チャネルと更新タスクの導入

**Goal**: daemon 起動時に title チャネルと更新タスクを用意し、title 更新の単一起点を確立すること。

**Files to Create**: なし

**Files to Modify**:

- `src-tauri/src/mux/daemon.rs`
  - `run_daemon`（Unix / Windows の両経路）で `session_manager` 生成直後に daemon-level の `title_tx` / `title_rx` を作成すること
  - title 更新タスクを `tokio::spawn` で起動し、daemon 終了までループさせること
  - `handle_connection` 呼び出し時に `title_tx` のクローンを渡すこと

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| daemon title channel | daemon 寿命と同等の `(pane_id, title)` チャネルを保持すること | daemon 起動直後 | `title_tx` は `handle_connection` とペインへ配布される参照であること |
| title update task | `title_rx` からメッセージを受け取り、差分があれば `window.name` を更新し broadcast すること | `title_rx` が有効であること | 同一 title なら `window.name` と broadcast は不変であること |

**Processing Flow** (title update task):

1. `title_rx.recv()` で `(pane_id, new_title)` を受信すること
2. `SessionManager` をロックして `find_pane(pane_id)` で `(sid, wid)` を解決すること
   - 見つからない場合は warn ログのみ出力して次の受信に進むこと
3. 現在の `window.name` と `new_title` を比較すること
   - 同一なら何もしないこと
   - 異なるなら `rename_window(sid, wid, new_title.clone())` を呼ぶこと
4. ロックを解放した上で `notify_tx.send(RenameWindow { pane_id: wid, name: new_title })` を呼ぶこと
   - 受信者がいない場合（`SendError`）は debug ログのみ出力して継続すること
5. `title_rx.recv()` が `None`（全送信側が drop）になった場合はタスクを終了すること

**Implementation Steps**:

1. **title チャネル生成** - `session_manager` 初期化直後に daemon-level の mpsc チャネルを生成すること
2. **更新タスクの spawn** - `session_manager.clone()` と `title_rx` のムーブを伴う専用タスクを spawn すること
3. **接続ハンドラへの引き渡し** - Unix / Windows 両方の accept ループで `handle_connection` に `title_tx.clone()` を渡すこと
4. **ロギング** - タスク起動時と title 更新時に info ログを出し、broadcast 送信失敗を debug ログで記録すること
5. **graceful shutdown 整合性** - daemon 終了時、`title_tx` が drop されることで自然にタスクが終了することを確認すること（追加処理は不要）

**Dependencies**: なし（基盤フェーズ）。Blocks: Phase 2, Phase 3。

**Testing Approach**:

- Unit: title 更新タスク相当の関数（例: `apply_title_change`）が `window.name` 更新と broadcast 送信を行うことを検証すること
- Integration: daemon 起動後、`title_tx` 送信のみで `SessionManager` の `window.name` が変化することを検証すること（Phase 3 で拡張）
- E2E (Docker): 不要

**Acceptance Criteria**:

- [ ] `run_daemon` 内で title チャネルが一度だけ生成されていること
- [ ] title 更新タスクが起動ログを残すこと
- [ ] 同一タイトル連続入力で `window.name` と broadcast が更新されないこと

**Estimated Effort**: small

---

### Phase 2: 接続ハンドラと detach 経路のリファクタリング

**Goal**: GUI / CLI 接続ハンドラから title 処理を剥がし、daemon-level `title_tx` を参照渡しする形に統一すること。detach 時に `pane.title_sender` を null 化していた処理を止めること。`Welcome` 送信と `notify_tx.subscribe()` の順序を是正すること。

**Files to Create**: なし

**Files to Modify**:

- `src-tauri/src/mux/ipc/connection.rs`
  - `handle_connection` のシグネチャに daemon-level `title_tx: TitleChangeSender` を追加すること
  - GUI 接続内で生成していた `mpsc::channel::<(u32, String)>(16)` の生成箇所を削除すること
  - `tokio::select!` 内の `title_rx.recv()` アーム一式を削除すること
  - `handle_cli_client` のシグネチャに daemon-level `title_tx: TitleChangeSender` を追加し、内部のダミー `(title_tx, _title_rx)` 生成を削除すること
  - `route_message` 等で `title_tx` を下流へ引き回す経路が daemon-level の参照になっていることを確認すること
  - **`notify_tx.subscribe()` を `Welcome` 送信より前に実行し、購読後に `session_list` をスナップショットして `Welcome` を構築すること**（現状は Welcome 送信後に subscribe しており、その間に発生した `RenameWindow` が取りこぼされる）
- `src-tauri/src/mux/ipc/reattach.rs`
  - `detach_session_panes` 内の `*pane.title_sender.lock().unwrap() = None;` を削除すること（daemon-level `title_tx` を維持するため）
  - `collect_reattach_data` の `*pane.title_sender.lock().unwrap() = Some(title_tx.clone());` は維持。daemon-level tx と同一値を書き戻すだけになるため実質 no-op だが、コードの意味は残す

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_connection` | GUI 接続の入出力ループを回すこと | `title_tx` は daemon-level | GUI 向け `RenameWindow` は `notify_rx` 経由でのみ送信されること |
| `handle_cli_client` | CLI 制御メッセージを処理すること | `title_tx` は daemon-level | 接続終了後も `pane.title_sender` は daemon-level の tx を保持し続けること |
| `detach_session_panes` | detach 時に output_target のみ Detached に切替すること | daemon-level `title_tx` は永続 | `pane.title_sender` は変更されず、detach 中の OSC も daemon タスクに届くこと |
| Welcome/notify 購読順序 | 取りこぼし防止 | `notify_tx` は存在 | subscribe → `session_list` スナップショット → Welcome 送信の順で実行されること |

**Processing Flow** (接続ハンドラ変更点):

1. accept された接続が GUI の場合
   - 旧: GUI 接続ごとに title チャネル生成 → `title_rx.recv()` ループで直接 `rename_window`
   - 新: daemon-level `title_tx.clone()` を各 handler / reattach / pty_spawn に渡すこと
   - GUI への `RenameWindow` は `notify_rx` 経由の分岐でのみ発火すること
   - `notify_rx = notify_tx.subscribe()` を `Welcome` 送信前に実行すること
2. accept された接続が CLI の場合
   - 旧: ダミーチャネルを生成して `handle_create_window` などに渡す
   - 新: daemon-level `title_tx.clone()` を渡すこと
3. detach 経路（`detach_session_panes`）
   - 旧: output_target を Detached に切替 + `title_sender = None`
   - 新: output_target のみ Detached に切替。`title_sender` はそのまま（daemon-level tx を保持）

**Implementation Steps**:

1. **`handle_connection` シグネチャ拡張** - `title_tx: TitleChangeSender` 引数を追加すること
2. **Welcome/subscribe 順序是正** - `notify_tx.subscribe()` を `Welcome` 送信の前に移動し、session_list 構築と Welcome 送信をロック内で連続実行すること
3. **GUI 内 title 処理の除去** - GUI 接続関数内の title チャネル生成と `title_rx.recv()` アームを削除すること
4. **`handle_cli_client` シグネチャ拡張と一元化** - ダミー生成を削除し、呼び出し元から渡す `title_tx` を利用すること
5. **`route_message` 呼び出し引数の整合** - 呼び出しチェーン全体（handlers 側は変更不要）で daemon-level `title_tx` を渡していることを確認すること
6. **`detach_session_panes` の title_sender クリア削除** - null 化行を削除し、detach 中も title 検出が daemon タスクに届くようにすること

**Dependencies**: Requires Phase 1。Blocks: Phase 3。

**Testing Approach**:

- Unit: 既存の `merge_consecutive_chunks` 等は不変であることを回帰確認すること
- Integration: Phase 3 で検証（detach 中 OSC 反映テストを含む）
- E2E (Docker): 不要

**Acceptance Criteria**:

- [ ] `handle_connection` 内で `mpsc::channel::<(u32, String)>` が生成されていないこと
- [ ] `handle_cli_client` 内のダミー `title_tx` が存在しないこと
- [ ] `detach_session_panes` 内で `pane.title_sender` を `None` にする行が存在しないこと
- [ ] `notify_tx.subscribe()` が `framed.send(Welcome)` より前で呼び出されていること
- [ ] `cargo build --manifest-path src-tauri/Cargo.toml` が成功すること

**Estimated Effort**: small

---

### Phase 3: 差分検出とテスト整備

**Goal**: FR-4 の差分検出維持を確認し、daemon 起点の title 更新を自動テストで保証すること。

**Files to Create**: なし（テストは既存ファイル内に追加）

**Files to Modify**:

- `src-tauri/src/mux/daemon.rs`
  - title 更新ロジックを独立した関数（例: `apply_title_change`）に切り出すこと。純粋な async 関数として `SessionManager` と `(pane_id, title)` を受け取り、更新の有無を `bool` で返す契約とすること
  - `#[cfg(test)]` 以下に unit テストを追加すること
- `src-tauri/src/mux/ipc/reattach.rs`（テストのみ）
  - 既存テストは維持。必要であれば daemon-level `title_tx` を前提としたテストケース名の整合を取ること

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `apply_title_change` | 差分検出付きで `window.name` 更新と broadcast 送信を行うこと | `SessionManager` への参照と `(pane_id, title)` が与えられていること | 差分があれば `true`、なければ `false` を返すこと |

**Processing Flow** (`apply_title_change` 契約):

- 入力: `&Arc<Mutex<SessionManager>>`, `pane_id: u32`, `new_title: String`
- 事前条件: pane は daemon 内のいずれかの session / window に存在し得る
- 事後条件:
  - pane 未発見: 戻り値 `false`、状態変化なし
  - 既存 `window.name` と `new_title` が同一: 戻り値 `false`、broadcast 送信なし
  - 差分あり: `window.name` が `new_title` に更新され、broadcast が 1 回送信され、戻り値 `true`

**Implementation Steps**:

1. **純粋関数の切り出し** - title 更新タスクのループ本体から差分判定と更新処理を関数化し、タスクはループのみ担当とすること
2. **Unit テスト追加** - 以下のシナリオをカバーすること
   - 新しい title で `window.name` が更新され、broadcast 受信側に 1 件届くこと
   - 同一 title の連続入力では 2 回目以降 broadcast が発行されないこと
   - 未登録 pane の入力では状態変化がないこと
3. **Integration テスト追加** - `SessionManager` に session/window/pane を登録し、daemon 相当のセットアップ（`title_tx` 生成 → タスク spawn）を行った上で、`title_tx.send` のみで `window.name` が更新されることを検証すること

**Dependencies**: Requires Phase 1, Phase 2。

**Testing Approach**:

- Unit: `apply_title_change` の差分検出・broadcast 送信の 3 ケース
- Integration: daemon 起動相当のセットアップで `title_tx` → `window.name` 反映
- Manual: `~/bin/init-mux` 起動 → 全ウィンドウのタブ名が zsh の OSC タイトルになること

**Acceptance Criteria**:

- [ ] 新規テストが Docker の `cargo test` で成功すること
- [ ] 同一タイトル連続入力で broadcast カウントが 1 に留まること
- [ ] `~/bin/init-mux` 経由の全ウィンドウで OSC タイトルが反映されること（手動）

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/
  src/
    mux/
      daemon.rs                 # Phase 1, 3: title チャネル・更新タスク・apply_title_change
      ipc/
        connection.rs            # Phase 2: handle_connection / handle_cli_client シグネチャ変更、Welcome/subscribe 順序是正
        handlers.rs              # 変更なし（既に title_tx: &TitleChangeSender を受領）
        pty_spawn.rs             # 変更なし
        reattach.rs              # Phase 2: detach_session_panes の title_sender = None 削除
      session/
        manager.rs               # 変更なし
        pane.rs                  # 変更なし（SharedTitleSender 設計は維持）
doc/
  tasks/
    mux-osc-title-propagation/
      SPEC.md
      sdd.yaml
      IMPLEMENTATION.md          # 本ドキュメント
      VERIFICATION.md            # 検証手順
      tasks.yaml                 # フェーズ別タスク状況
```

## Testing Strategy

- Unit: `apply_title_change` のロジック網羅（更新あり / 同一 title / pane 未発見）
- Integration: `SessionManager` + daemon-level `title_tx` のセットアップで `window.name` が更新され、`notify_tx` 受信側に `RenameWindow` が届くこと
- E2E (Docker): 本変更はソケット通信層内部の責務再配置のため、既存 E2E の回帰のみで十分とすること
- Manual: `~/bin/init-mux` 起動および CLI `emterm mux new-window` 後の手動確認

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| tokio | 既存 | mpsc / broadcast / spawn |

（新規依存なし）

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `title_tx` が clone されず daemon タスクが途中終了する | low | high | Phase 1 のタスク終了ログと起動ログを確認すること |
| `notify_tx.send` がバッファ溢れで失敗する | low | low | 既存 broadcast capacity(16) は十分。失敗時は debug ログのみとすること |
| daemon title チャネルが満杯で `try_send` が drop する | low | low | title 更新は低頻度。同じペインが後続タイトルを出した時点で復帰するため許容。daemon-level チャネル容量は 64 とすること（既存の 16 より大きめに取り、multi-pane 同時発火の margin を確保） |
| reattach 中の title 更新とレース | low | medium | `SessionManager` ロック取得順序を既存パターン（短時間 hold → drop → broadcast）に揃えること |
| detach 時に title_sender が null 化されていた過去挙動の消し漏れ | medium | high | Phase 2 の acceptance criteria で `detach_session_panes` 内の null 化行が無いことを明示的に確認すること |
| Welcome と subscribe の間で発生する RenameWindow 取りこぼし | medium | medium | Phase 2 で subscribe → Welcome 構築/送信 の順序を固定化。Integration test TS-11 で保証 |
| CLI `handle_cli_client` の旧ダミーチャネルを参照する箇所の消し漏れ | low | medium | `grep` で `mpsc::channel::<(u32, String)>` 残骸を確認すること |

## Open Questions

- [ ] `apply_title_change` は `daemon.rs` 内の private 関数と `pub(super)` どちらで公開するか（テスト容易性の観点で後者推奨だが、クレート構成に合わせること）。Phase 3 実装時に決定すること

## Out of Scope / Preserved Semantics

- 複数ペインを持つウィンドウでの「どのペインが `window.name` を上書きできるか」という方針は現行実装を維持する（`find_pane(pane_id)` で解決したウィンドウに対して無差別に上書きする）。ペイン別タイトル管理や active-pane-only ルールは本タスクのスコープ外。

## Success Metrics

- [ ] FR-1〜FR-7 全てのシナリオがテストまたは手動確認でパスすること
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` が Docker 環境で成功すること
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` が成功すること
