# Implementation Plan: mux move-window

## Overview

mux モードのアクティブウィンドウを 1-origin 指定位置へ並び替える `move-window` アクションを、`prefix + m` で起動する番号入力モーダルと共に実装すること。タブラベル先頭に `[N]` バッジを描画し、バックエンドの `MuxSession` に順序モデルを追加すること。

## Objectives

- バックエンド `MuxSession` に明示的なウィンドウ順序を保持する手段を追加し、任意順 insert/move を可能にすること
- `prefix + m` からモーダル→IPC→セッション並び替え→UI 反映の一連経路を提供すること
- mux タブラベルに 1-origin 番号バッジを常時表示し、ウィンドウ 1 個時も番号を提示すること
- 既存 rename-window パターン（IPC 経路・ダイアログ骨格・`moduleLocal open-guard`）を踏襲し、実装の一貫性を保つこと

## Prerequisites

### Development Environment

- Rust toolchain（既存プロジェクトと同一）
- Bun（TypeScript / テスト）
- Docker + docker compose（テスト／E2E 実行用）

### Dependencies

- 既存の Tauri / tokio / bincode / serde / rust-i18n スタック
- 既存フロントエンドの i18n 枠組（`src/i18n/index.ts` の `t()`）
- 既存 `sftp-dialog-*` スタイル群
- 既存 GUI→Daemon IPC 経路（`MuxClient.sendControl` / `sendMuxControl`）。Daemon→GUI broadcast は本機能では使用しない

## Design Decisions

### 論点A: `MuxSession.windows` 並び順モデル

**採用**: Option A — `MuxSession` に `window_order: Vec<WindowId>` を追加する。

- `windows: BTreeMap<WindowId, MuxWindow>` は ID → データのルックアップ用途に維持すること
- 表示順を要する走査（`session_list` 含む）は `window_order` を通すこと
- 新規依存なしで済み、`remove_window` / `add_window` 等への修正範囲が局所化されること

### 論点B: 単一ウィンドウ時の `[N]` 表示

**採用**: Option A — `windows.length <= 1` のときも `mux-tab-group` 構造を維持し、`[1] title` を描画する。

- `drag-handler.ts` が `.mux-tab-group` 単位で処理している既存仕様と整合すること
- `restoreMuxOriginalTab` 呼び出し経路を mux mode 中は用いず、mode 終了時のみ維持すること
- 子要素 `.mux-window-tab` が 1 個でも番号バッジ描画経路が統一されること

### 論点C: IPC プロトコル round-trip テスト上限

- 既存 `test_message_type_round_trip` の走査上限を `0x19` から `0x1A` に拡張すること
- `test_apc_round_trip_all_message_types` の走査上限を `0x18` から `0x1A` に拡張すること
- `0x11` は引き続きスキップ、`0x1B` 以降は `None` を返すこと
- 既存の `assert!(MessageType::from_u8(0x1a).is_none());` 行は `0x1A` が有効化されるため削除し、代わりに `assert!(MessageType::from_u8(0x1b).is_none());` を追加する

### 論点D: Daemon→GUI の順序同期方式

**採用**: フロント楽観更新 + Daemon 片方向更新（broadcast なし）。

- 現行 IPC プロトコルは attach 中に「順序付きセッション状態」を GUI へ push する経路を持たないこと（`Welcome` は handshake 時のみ、`RenameWindow` lagging resync は名称のみ）
- 既存 `RenameWindow` も GUI→Daemon 経路では GUI 側で楽観更新しており、同パターンに揃える
- Daemon 側の `MuxSession.window_order` は次回 attach 時の Welcome ペイロードに乗り自然に整合する
- 新規の「順序通知」メッセージ型 / broadcast 経路は導入しないこと（本機能スコープ外）

## Architecture Overview

### Technology Stack

- **Language**: Rust（`src-tauri/`）、TypeScript（`src/`）
- **Framework**: Tauri、Tokio、Vanilla TS
- **Key Libraries**:
  - `bincode` — 4B `u32` payload シリアライズ
  - `serde` — `MoveWindowMsg` の `Serialize`/`Deserialize`
  - `rust-i18n` / TS `t()` — モーダル文言

### Design Approach

- セッション状態変更を「既存の RenameWindow 経路」にならって片方向 IPC で発行すること
- バックエンドの `MuxSession` を順序の権威ソースとして扱いつつ、フロントは **既存 `RenameWindow` と同じ楽観更新パターン** に揃える。すなわち確定時点で `muxWindows` / `muxPaneIds` をローカルで reorder し `emitMuxStateChange` を呼ぶ。IPC は daemon 側の状態を同期させるために送信する
  - 現行プロトコルには「順序付き状態をクライアントへ push する」経路が `Welcome` 以外に存在しないこと、および `RenameWindow` の `notify_rx Lagged resync` が name のみを再配信する設計であるため、broadcast 方式は採らないこと
  - Reattach 時のみ Welcome ペイロードで順序が整合するため、attach 中の再整合は本機能スコープ外とし、後続イシューに任せる
- モーダルは既存 `rename-window-dialog.ts` と同等の DOM / イベントハンドラ / cleanup 契約を踏襲すること
- タブバー描画は既存 `renderMuxSubTabs` の差分再利用パスを維持しつつ、`mux-window-number` span の挿入・更新のみ追加すること
- 単一ウィンドウ時も `[1]` を表示するため、`main.ts` の `onMuxStateChange` コールバックで `windowCount === 1` のときも `renderMuxSubTabs` を呼ぶよう変更する（現行は `clearMuxSubTabs` + `updateTabTitle` で完全にバイパスしている）

### Component Interaction

```
User (prefix + m)
  -> PrefixKeyHandler.dispatch(MuxAction{move-window})
     -> handleMuxAction (dialog open guard)
        -> showMoveWindowDialog -> user input
           -> validate (1 <= N <= count, N != currentIndex+1)
              -> 楽観更新: muxWindows / muxPaneIds をローカルで reorder
                 + activeMuxWindowIndex を追従調整
                 -> emitMuxStateChange -> main.ts onMuxStateChange
                    -> renderMuxSubTabs が [N] を再描画
              -> sendMuxControl(MoveWindow, paneId, payload(target_index))
                 -> Daemon: route_message
                    -> handle_move_window
                       -> SessionManager::move_window / MuxSession::move_window
                       -> （broadcast 無し。daemon 状態は次回 attach 時の Welcome で整合）
```

## Implementation Phases

### Phase 1: バックエンド — `MuxSession` 順序モデル追加

**Goal**: `MuxSession` にウィンドウ順序を保持する `Vec<WindowId>` を導入し、既存 `add_window`/`remove_window` を順序整合させること。`MuxSession::move_window` は **まだ追加しない**（Phase 2 のテストと分離するため、Phase 1 は純粋にデータ構造変更とし、表示順の正確性を既存テストで担保すること）。

**Files to Modify**:

- `src-tauri/src/mux/session/session.rs`
  - `MuxSession` に `window_order: Vec<WindowId>` フィールドを追加すること
  - `add_window`：`windows.insert` に加え `window_order.push(id)` を行うこと
  - `remove_window`：`windows.remove` に加え `window_order` から当該 id を除去すること
  - `active_window_id` 再選出ロジックは「`window_order` の先頭」参照に統一すること。**これは現行の `windows.keys().next()`（BTreeMap の最小 WindowId）からの意図的な挙動変更であり、順序モデル導入に伴う整合性確保のため必要である**
  - この挙動変更を単体テストで明示的に固定すること（`test_active_window_id_after_remove_uses_order` にて `[A(id=2), B(id=1)]` を `add_window` の順で登録し、A を active にした後 A を remove すると active が B になることを確認する）
- `src-tauri/src/mux/session/manager.rs`
  - `session_list` の `windows` 走査を `s.window_order.iter().filter_map(...)` 経由に変更すること
  - `active_window_index` の算出も `window_order` 上の position を使用すること

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MuxSession::add_window` | 挿入と同時に順序末尾へ id を追加すること | `windows` に同一 id が未登録であること | `window_order.last() == Some(&id)` かつ `windows[id]` が存在すること |
| `MuxSession::remove_window` | `windows` と `window_order` の整合を保って除去すること | `id` が両方に存在するか、どちらにも存在しないこと | `windows`/`window_order` 双方から id が消えること、`active_window_id` は `window_order` 先頭（空なら `None`）であること |
| `SessionManager::session_list` | `window_order` 順で `WindowInfo` を構築すること | `MuxSession` インバリアント成立 | 返り値の `windows` が `window_order` と同順であること |

**Processing Flow** — `remove_window`:

1. `windows.remove(&id)` を試行する
2. `window_order` から id を線形検索で除去する
3. `active_window_id == Some(id)` なら：
   - `window_order.first()` があればそれを新 active にする
   - なければ `None` を代入する
4. 取り出した `MuxWindow` を返す

**Implementation Steps**:

1. **フィールド追加と初期化** — `MuxSession::new` で `window_order: Vec::new()` を初期化する
2. **`add_window` 更新** — 挿入後に `window_order` へ push する
3. **`remove_window` 更新** — 両コレクションから整合除去し、active 再選出を `window_order` 基準に変える
4. **`session_list` 置換** — `manager.rs` 内の走査を `window_order` ベースに変える
5. **既存テストの確認と追補** — Phase 1 で既存 `test_session_list_includes_windows` が通ること、`add/remove` を複数回行っても順序が期待通りになる新規テストを追加すること

**Dependencies**: なし。Blocks: Phase 2 以降すべて。

**Testing Approach**:

- Unit (`cargo test`):
  - `session::tests::test_window_order_after_adds`：連続 `add_window` で `window_order` が追加順になること
  - `session::tests::test_window_order_after_removes`：中間・末尾・先頭の除去で順序が保たれること
  - `session::tests::test_active_window_id_after_remove_uses_order`：active を除去すると `window_order.first()` が新 active になること
  - `manager::tests::test_session_list_matches_window_order`：`session_list()[0].windows` が `window_order` と一致すること

**Acceptance Criteria**:

- [ ] 新規・既存の `cargo test` がすべて通ること
- [ ] `session_list` の `windows` 配列順が `window_order` と一致すること
- [ ] active 再選出が `window_order.first()` を参照すること

**Estimated Effort**: small

---

### Phase 2: IPC プロトコル拡張

**Goal**: `MessageType::MoveWindow = 0x1A` と `MoveWindowMsg { target_index: u32 }` を追加し、APC/OSC round-trip を保証すること。

**Files to Modify**:

- `src-tauri/src/mux/ipc/protocol.rs`
  - `enum MessageType` に `MoveWindow = 0x1A` バリアントを追加すること
  - `MessageType::from_u8` に `0x1A => Some(Self::MoveWindow)` を追加すること
  - `MoveWindowMsg { pub target_index: u32 }` 型を `Debug + Clone + Serialize + Deserialize` 付きで追加すること
  - `test_message_type_round_trip` の走査上限を `0x19` から `0x1A` に拡張すること（`0x11` は従来通りスキップ）
  - **既存の `assert!(MessageType::from_u8(0x1a).is_none());` 行を削除すること**（`0x1A` が有効になるため）。代わりに `assert!(MessageType::from_u8(0x1b).is_none());` を追加して将来拡張の境界を担保する
  - `test_apc_round_trip_all_message_types` の走査上限を `0x18` から `0x1A` に拡張すること
  - 新規テスト：`test_move_window_message_type`、`test_move_window_msg_round_trip`、`test_move_window_msg_via_mux_message`

- `src/terminal/mux/mux-client.ts`
  - `MuxMessageType` オブジェクトに `MoveWindow: 0x1a` キーを追加すること（値は数値リテラル）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MessageType::MoveWindow` | 0x1A に対応するバリアント | APC 経由で識別可能であること | `from_u8(0x1A) == Some(MoveWindow)` |
| `MoveWindowMsg` | 0-based target_index を運ぶペイロード | 呼び出し側が有効範囲にクランプ済みの値を持つこと | 4B LE `u32` シリアライズで round-trip すること |
| `MuxMessageType.MoveWindow` (TS) | フロント側プロトコル定数 | Rust 定義と値一致 | `0x1A` を書き出すのみ |

**Payload Contract**:

- `MoveWindowMsg { target_index: u32 }`
  - Precondition: `target_index` は呼び出し側で 0-based、範囲チェック済みであること
  - Wire format: bincode の標準 `u32`（4 バイト little-endian）
  - Frame body: `[0x1A][pane_id: u32 LE][target_index: u32 LE]` 計 9 バイト

**Implementation Steps**:

1. **Rust enum 拡張** — `MessageType`・`from_u8` に `MoveWindow` を追加する
2. **`MoveWindowMsg` 型追加** — protocol.rs の他 `*Msg` 型と同じ派生マクロで追加する
3. **既存 round-trip テスト拡張** — `0x1A` 含む走査に更新し、`0x1B` 以降が `None` になることを確認する
4. **新規テスト追加** — `MoveWindowMsg` 単体 round-trip、`MuxMessage::control` 経由 round-trip、APC 経由 round-trip
5. **TS 定数追加** — `MuxMessageType` に `MoveWindow` キーを追加する

**Dependencies**: Phase 1 を要しない（データ構造と独立）。ただし Phase 3 のハンドラ追加より前に完了していること。

**Testing Approach**:

- Unit (`cargo test`):
  - `protocol::tests::test_move_window_message_type`：`MoveWindow as u8 == 0x1A` と `from_u8(0x1A) == Some(MoveWindow)` の双方
  - `protocol::tests::test_move_window_msg_round_trip`：bincode のみで往復
  - `protocol::tests::test_move_window_msg_via_mux_message`：`MuxMessage::control` 経由で往復し `target_index` を復元
  - 既存 `test_message_type_round_trip` / `test_apc_round_trip_all_message_types` が拡張後も通ること
- Unit (`bun test`): 既存 `mux-client.test.ts` に `MoveWindow === 0x1a` アサートを追加すること

**Acceptance Criteria**:

- [ ] Rust / TS 両方で `0x1A` = `MoveWindow` が定義されていること
- [ ] 既存 round-trip テストが拡張後も通ること
- [ ] 新規 `MoveWindowMsg` テストが通ること

**Estimated Effort**: small

---

### Phase 3: バックエンド IPC ハンドラと `MuxSession::move_window`

**Goal**: `MuxSession::move_window` を insert/move セマンティクスで実装し、`handle_move_window` を既存 `route_message` の GUI dispatch に組み込むこと（Daemon → GUI broadcast は行わない。論点D 参照）。

**Files to Modify**:

- `src-tauri/src/mux/session/session.rs`
  - `impl MuxSession` に `move_window(window_id, target_index) -> bool` メソッドを追加すること
- `src-tauri/src/mux/ipc/handlers.rs`
  - `handle_move_window(msg, session_manager)` 非同期関数を追加すること
  - pane_id → (sid, wid) 経路を第一に、失敗時は `find_window_session(id)` で window_id フォールバックを行うこと
- `src-tauri/src/mux/ipc/connection.rs`
  - `route_message` の GUI 分岐に `MessageType::MoveWindow => handle_move_window(...)` を追加すること
  - CLI 分岐（`handle_cli_client`）には **追加しないこと** — 本機能は GUI からのみ発行される（将来 CLI から移動する必要が出た場合は別途拡張する）
  - broadcast は行わない（Design Approach 参照）。Daemon は自身の `MuxSession` 状態だけ更新し、GUI の楽観更新が視覚的に反映を担う

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MuxSession::move_window` | 指定 window を insert/move で移動すること | `window_id` が `window_order` 上に存在すること、`target_index` は `usize` | 存在すれば true を返し `window_order` が更新される。active_window_id は不変 |
| `handle_move_window` | IPC メッセージを復号し `MuxSession::move_window` を呼ぶこと。broadcast は行わない | `msg.msg_type == MoveWindow` | Daemon 側 `window_order` が更新され、成功ログを出力すること |
| `route_message` 分岐 | 新メッセージ型のルーティング | `MessageType` 判定完了 | ハンドラへ委譲する |

**Contract — `MuxSession::move_window(window_id, target_index) -> bool`**:

- Precondition: `&mut self`
- Behavior:
  - `window_id` が `window_order` に無ければ `false` を返すこと
  - 現在位置 `cur` を特定し、`target_index` を `[0, window_order.len() - 1]` にクランプすること
  - `cur == clamped_target` なら変更なしで `false` を返すこと
  - それ以外では `window_order` から `cur` を remove し、`clamped_target` に insert すること
  - `active_window_id` は **一切変更しないこと**
  - 成功時は `true` を返すこと
- Postcondition: `windows` (BTreeMap) は変化しないこと、`window_order` の長さは不変、`window_order.contains(window_id)` は true

**Processing Flow** — `handle_move_window`:

1. `msg.decode_payload::<MoveWindowMsg>()` を試みる
   - `None` → warn ログを出して return
2. `id = msg.pane_id`、`target_index = move_msg.target_index as usize`
3. `session_manager.lock()` を取得
4. `mgr.find_pane(id)` を試みる
   - `Some((sid, wid))` → `mgr.get_session_mut(sid)?.move_window(wid, target_index)` を呼び、info ログを出力して return
5. 失敗時：`mgr.find_window_session(id)` を試みる
   - `Some(sid)` → `mgr.get_session_mut(sid)?.move_window(id, target_index)` を呼び、info ログを出力
6. どちらも失敗 → warn ログを出して return

**broadcast しない理由**:

- 既存 IPC プロトコルには「順序付きセッション状態」を attach 中に GUI へ push する経路が存在しない（`Welcome` はハンドシェイク時のみ）
- `RenameWindow` の `notify_rx Lagged resync` は name のみを再配信するため、順序を伝達できない
- 新規の「順序通知」メッセージ型を導入することは技術的に可能だが、本機能は GUI 発端の局所操作であり、楽観更新（フロント側で `muxWindows` を reorder）で整合性を担保する方が既存 `RenameWindow` パターンと一貫する
- Daemon 側の `window_order` は `session_list()` 返却時・次回 attach 時の Welcome で自然に整合する

**Implementation Steps**:

1. **`MuxSession::move_window` 実装** — Precondition / Postcondition に従い `Vec::remove` + `Vec::insert` で実現する
2. **`handle_move_window` 追加** — `handle_rename_window` のパターンを踏襲する
3. **`route_message` 分岐追加** — GUI 分岐（route_message 内）にのみ組み込む。CLI 分岐への組み込みは不要
4. **ログ整備** — 成功時 info、未解決 id 時 warn、decode 失敗時 warn

**Dependencies**: Phase 1（順序モデル）、Phase 2（メッセージ型）。Blocks: Phase 6（フロントからの送信）、Phase 8（E2E）。

**Testing Approach**:

- Unit (`cargo test`, `session::tests`):
  - `test_move_window_to_first`：`[A,B,C,D]` で D を 0 に → `[D,A,B,C]`
  - `test_move_window_to_last`：`[A,B,C,D]` で A を 3 に → `[B,C,D,A]`
  - `test_move_window_to_middle_forward`：`[A,B,C,D]` で B を 2 に → `[A,C,B,D]`（remove-then-insert 定義。3 に移動すると `[A,C,D,B]`）
  - `test_move_window_same_position`：current == target で `false` を返し順序不変であること
  - `test_move_window_out_of_range_clamps`：`target_index >= len` は末尾に clamp
  - `test_move_window_unknown_id`：未知 id は `false`
  - `test_move_window_preserves_active`：`active_window_id` が不変であること
  - `test_move_window_single_window_noop`：1 個のみで呼ぶと `false`（current == target）
- Unit (`cargo test`, `manager::tests`):
  - `test_session_list_reflects_move_window_order`：move 後 `session_list()` が新順を反映すること
- Integration（可能な範囲で）:
  - `handlers` で pane_id 経路／window_id 経路の双方を呼び分け、`MuxSession` 状態が期待通りになること

**Acceptance Criteria**:

- [ ] 上記 unit テストがすべて通ること
- [ ] IPC 経由で呼び出し、GUI に新順序が届く経路が動作すること（Phase 8 の E2E で裏付け）
- [ ] 未知 id / 範囲外 / 同一位置で `false` が返り状態が変わらないこと

**Estimated Effort**: medium

---

### Phase 4: フロントエンド — prefix key アクション追加

**Goal**: `MuxAction` union と `DEFAULT_ACTION_BINDINGS` に `move-window` を追加し、`prefix + m` が該当アクションを発火することをテストで担保すること。

**Files to Modify**:

- `src/terminal/mux/prefix-key.ts`
  - `MuxAction` に `| { type: "move-window" }` を追加すること
  - `DEFAULT_ACTION_BINDINGS` に `"move-window": "m"` を追加すること
- `src/terminal/mux/prefix-key.test.ts`
  - `test("all tmux-compatible bindings are present", ...)` の `bindings` 配列に `{ key: "m", expected: "move-window" }` を追加すること
  - 単独ケース `test("prefix + m dispatches move-window", ...)` を追加すること
  - `removedKeys` に `"m"` が含まれないことを確認するコメントを更新すること（現状の配列には無いため負テストの修正は不要）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MuxAction` union | 新アクション型の表現 | TypeScript 型システムで網羅的判定可能であること | `switch (action.type)` で `"move-window"` ケースが要求されること |
| `DEFAULT_ACTION_BINDINGS` | `m` → `move-window` マッピング | 既存キーと衝突しないこと（`d/c/n/p/,` は占有済） | `m` 押下で `move-window` が dispatch されること |

**Implementation Steps**:

1. **型追加** — `MuxAction` にリテラル型を追加する
2. **バインディング追加** — `DEFAULT_ACTION_BINDINGS` を拡張する
3. **既存テスト拡張** — 全 binding テストに `m` を加える
4. **単独テスト追加** — `prefix + m` 単体で `move-window` が dispatch される独立テストを書く

**Dependencies**: なし（Phase 5/6 より先に入れてよい）。Blocks: Phase 6。

**Testing Approach**:

- Unit (`bun test`):
  - `prefix + m dispatches move-window` の単独テスト
  - 既存 `all tmux-compatible bindings are present` に `m` を含める

**Acceptance Criteria**:

- [ ] `bun test src/terminal/mux/prefix-key.test.ts` が全通過すること
- [ ] TypeScript の `MuxAction` を網羅する `switch` が `move-window` を要求する（型エラーが無いこと）

**Estimated Effort**: small

---

### Phase 5: フロントエンド — `move-window-dialog.ts` 実装と i18n

**Goal**: `sftp-dialog-*` スタイルを踏襲した番号入力モーダルを提供し、英語・日本語の i18n キーを追加すること。

**Files to Create**:

- `src/terminal-app/mux/move-window-dialog.ts` — モーダル本体

**Files to Modify**:

- `src/i18n/locales/en.json`
  - `mux.moveDialog.title = "Move Window"`
  - `mux.moveDialog.label = "Target position (1-origin)"`
  - `mux.moveDialog.confirm = "OK"`
  - `mux.moveDialog.cancel = "Cancel"`
- `src/i18n/locales/ja.json`
  - `mux.moveDialog.title = "ウィンドウを移動"`
  - `mux.moveDialog.label = "移動先の位置 (1-origin)"`
  - `mux.moveDialog.confirm = "OK"`
  - `mux.moveDialog.cancel = "キャンセル"`

**Key Components** — `showMoveWindowDialog`:

| Function | Responsibility | Precondition | Postcondition |
|----------|----------------|--------------|---------------|
| `showMoveWindowDialog(options)` | モーダルを表示し、ユーザー入力結果を Promise で返すこと | `document.body` が存在すること | 確定/キャンセル時に overlay が DOM から除去され、直前フォーカスが復帰すること |

**Input Contract**:

```
MoveWindowDialogOptions {
  currentIndex: number   // 1-origin. 表示のヒント目的
  windowCount: number    // 入力上限（1..windowCount）
}
```

**Output Contract**:

```
MoveWindowDialogResult {
  confirmed: boolean
  value?: number         // 1-origin。confirmed=false では undefined
}
```

**Processing Flow**:

1. 直前フォーカス要素を保存する
2. `sftp-dialog-overlay` / `sftp-dialog` DOM を構築し、title/label/input/buttons を配置する
3. input 要素に `type="text"` `inputmode="numeric"` `pattern="[0-9]*"` `maxLength=4` を設定する
4. 初期値は空文字列、input にフォーカスして全選択する
5. keydown ハンドラ:
   - IME composition 中（`e.isComposing || keyCode === 229`）は何もしない
   - Escape → `{ confirmed: false }` で resolve し cleanup
   - Enter → input 値を trim → `Number.parseInt(v, 10)`
     - NaN、`< 1`、`> windowCount` → `{ confirmed: false }` で resolve
     - それ以外 → `{ confirmed: true, value: parsed }` で resolve
6. Cancel ボタンクリック → `{ confirmed: false }`
7. Confirm ボタンクリック → Enter と同じバリデーション経路を辿る
8. cleanup: overlay を remove し、直前フォーカスを復帰させる

**Validation 条件**:

| 入力 | 判定 |
|------|------|
| 空文字 | cancel |
| 非整数 | cancel |
| `< 1` / `> windowCount` | cancel |
| 範囲内整数 | confirm |

「現在位置と同一」の判定はこのモーダルの責務外とし、呼び出し側（Phase 6 の mux-action-handler）で処理する方針とする（理由: モーダルはバリデーション対象を知らない方がダイアログ再利用性が高く、アクション側で既に `activeIndex` を持っているため）。

**Implementation Steps**:

1. **新規ファイル作成** — `rename-window-dialog.ts` の骨格をベースに `options` / `result` 型と `showMoveWindowDialog` 関数を定義する
2. **DOM 構築** — overlay/dialog/title/label/input/buttons を生成し `sftp-dialog-*` クラスを適用する
3. **バリデーション配線** — Enter/Confirm ボタンで trim → `parseInt` → 範囲判定し `confirmed`/`value` を決める
4. **IME / Esc / focus restore** — 既存 rename ダイアログと同じ契約を実装する
5. **i18n 追加** — en/ja の両方に `mux.moveDialog.*` キーを追加する
6. **単体テスト作成** — `move-window-dialog.test.ts` を新設し、DOM を JSDOM 経由で検証する

**Dependencies**: Phase 4（action 型定義）。Blocks: Phase 6。

**Testing Approach**:

- Unit (`bun test`, `move-window-dialog.test.ts` 新設):
  - 有効整数 Enter → `{ confirmed: true, value }`
  - 非整数 Enter → `{ confirmed: false }`
  - 範囲外 (`< 1` / `> windowCount`) → `{ confirmed: false }`
  - 空文字 Enter → `{ confirmed: false }`
  - Esc → `{ confirmed: false }`
  - Cancel ボタン → `{ confirmed: false }`
  - Confirm ボタン（有効入力）→ `{ confirmed: true, value }`
  - IME composition 中の Enter では resolve されないこと
  - close 後に `previouslyFocused.focus()` が呼ばれること
- 手動検証: UI 目視（描画位置、フォント、配色が rename ダイアログと一致すること）

**Acceptance Criteria**:

- [ ] 全 unit テスト通過
- [ ] en/ja 両 locale に 4 キーが揃っていること
- [ ] モーダルが既存 sftp-dialog と視覚的に一致すること（手動確認）

**Estimated Effort**: small

---

### Phase 6: フロントエンド — アクションハンドラ連携と楽観更新 + IPC 送信

**Goal**: `handleMuxAction` の `move-window` ケースで、ダイアログ表示 → 入力検証 → ローカル楽観更新（配列 reorder + `emitMuxStateChange`）→ `MoveWindowMsg` エンコード → `sendMuxControl` を繋ぐこと。

**Files to Modify**:

- `src/terminal-app/mux/mux-action-handler.ts`
  - モジュールローカル `let moveDialogOpen = false;` を宣言すること
  - `switch (action.type)` に `case "move-window"` を追加すること
  - `showMoveWindowDialog` を import すること
  - `MuxActionContext` に配列 reorder 用のフック（`moveMuxWindow(fromIndex, toIndex)` または `getMuxPaneIds()` / `getMuxWindows()` が直接 reorder 可能な mutable 参照を返す既存 API）を確認し、必要ならばラッパ関数 `reorderMuxArrays(fromIndex, toIndex)` を `mux-window-manager.ts` に追加して context 経由で呼び出せるようにする
- `src/terminal-app/mux/mux-window-manager.ts`（必要に応じて）
  - `reorderMuxWindows(ctx, fromIndex, toIndex)` 関数を追加する。以下を atomic に行うこと:
    - `muxWindows` と `muxPaneIds` の両配列から要素を `splice(from, 1)` で取り出し `splice(to, 0, item)` で挿入する
    - `activeMuxWindowIndex` を新しい位置に追従させる（移動対象が active なら `toIndex`、それ以外は from/to の関係で補正）
    - `emitMuxStateChange(ctx)` を呼ぶ
- `src/terminal-app/mux/mux-action-handler.ts` の `MuxActionContext` インターフェースに `reorderMuxWindows: (from: number, to: number) => void` を追加（または既存の `setActiveMuxWindowIndex` / `getMuxWindows` / `getMuxPaneIds` を組み合わせた書き込みが可能か確認）

**Processing Flow** — `case "move-window"`:

1. `moveDialogOpen === true` なら即 return する（重複表示防止）
2. `windows = ctx.getMuxWindows()`、`activeIndex = ctx.getActiveMuxWindowIndex()` を取得する
3. `target = windows[activeIndex]`。存在しなければ return
4. `targetWinId = target.id`、`windowCount = windows.length` を退避する（`targetWinId` は frontend-local の安定 ID として使用。バックエンドとの通信には使わないこと）
5. `moveDialogOpen = true` の上で `showMoveWindowDialog({ currentIndex: activeIndex + 1, windowCount })` を呼ぶ
6. then ハンドラ：
   - `result.confirmed === false || result.value === undefined` → return
   - 再取得 `currentWindows = ctx.getMuxWindows()`、`currentIdx = currentWindows.findIndex(w => w.id === targetWinId)`
   - `currentIdx < 0` → return（ダイアログ表示中にウィンドウが閉じられた場合）
   - `currentCount = currentWindows.length`
   - `value = result.value`
   - バリデーション：`value < 1 || value > currentCount || value === currentIdx + 1` → return
   - `targetIndex = value - 1`（0-based）
   - `paneId = ctx.getMuxPaneIds()[currentIdx]`。未定義なら return
   - **楽観更新 (先に実行)**: `ctx.reorderMuxWindows(currentIdx, targetIndex)` を呼ぶ（配列 reorder + active index 追従 + emitMuxStateChange）
   - 4B LE `u32` ペイロードを構築：`Uint8Array(4)` を作り `DataView.setUint32(0, targetIndex, true)` で書く
   - `sendMuxControl(ctx, MuxMessageType.MoveWindow, paneId, payload)` を呼ぶ
7. finally で `moveDialogOpen = false` に戻す
8. 例外（DOM 構築失敗等）時も同様に reset する

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `moveDialogOpen` | 同時多重表示抑止 | モジュール単一プロセス | finally で必ず false に戻ること |
| `reorderMuxWindows(from, to)` | `muxWindows`/`muxPaneIds` を同期的に reorder し active index を追従させる | `0 <= from < len`、`0 <= to < len` | 両配列の順序が変更され、`activeMuxWindowIndex` が移動後の target を追従、`emitMuxStateChange` が発火 |
| `case "move-window"` ブロック | dialog → validate → 楽観更新 → IPC の橋渡し | アクティブ window と pane が存在すること | 有効入力時のみローカル reorder + IPC 送信が行われること |

**Implementation Steps**:

1. **import 追加** — `showMoveWindowDialog` を dialog モジュールから import する
2. **`reorderMuxWindows` 追加** — `mux-window-manager.ts` にユーティリティ関数を追加し `MuxActionContext` 経由で呼べるよう配線する
3. **guard 宣言** — ファイル冒頭に `moveDialogOpen` を追加する
4. **case 追加** — `switch (action.type)` 内に `case "move-window":` を実装する
5. **楽観更新** — validate 成功直後に `ctx.reorderMuxWindows(currentIdx, targetIndex)` を呼ぶ
6. **payload 構築** — `Uint8Array(4)` + `DataView.setUint32(..., true)` で LE `u32` を書く
7. **ログ整備** — 送信前に `muxLog.info` で target_index を記録する
8. **エラーハンドリング** — `.catch` / `.finally` でガードをリセットする

**Dependencies**: Phase 2（`MoveWindow` 定数）、Phase 4（`MuxAction`）、Phase 5（dialog）、Phase 3（バックエンドが受領可能）、Phase 7（`renderMuxSubTabs` が `emitMuxStateChange` 経由で reorder を反映）。Blocks: Phase 8。

**Testing Approach**:

- Unit (`bun test`):
  - `mux-window-manager.test.ts`（既存にあれば拡張、無ければ新設）: `reorderMuxWindows` が `muxWindows` / `muxPaneIds` を整合的に reorder し、active index を追従させることを検証する
  - `reorderMuxWindows` 単体テスト: `[A,B,C]` で B(index 1) を 2 へ → `[A,C,B]`、active が B なら newActive=2、active が A なら newActive=0、active が C なら newActive=1
- Typecheck: `bun run typecheck`
- 手動検証: Docker E2E でエンドツーエンド確認（Phase 8）

**Acceptance Criteria**:

- [ ] 既存ビルド・typecheck が通ること（`bun run typecheck`）
- [ ] dialog の返り値が有効なとき `MuxMessageType.MoveWindow` が送信されること（E2E で裏付け）
- [ ] 同一位置／範囲外／ウィンドウ喪失時は IPC が送信されないこと

**Estimated Effort**: small

---

### Phase 7: フロントエンド — タブ番号バッジ描画

**Goal**: `renderMuxSubTabs` に `[N]` バッジを挿入し、単一ウィンドウ時も `mux-tab-group` 構造を維持して `[1] title` を描画すること。

**Files to Modify**:

- `src/main.ts`
  - `onMuxStateChange` コールバック内の分岐を修正すること。現行の `windowCount === 1` 分岐は `clearMuxSubTabs + updateTabTitle` で `renderMuxSubTabs` をバイパスしているが、`[1]` バッジ表示のため `renderMuxSubTabs` 経路に合流させる。新しい分岐:
    - `windowCount === 0`: 既存の clearMuxSubTabs + updateTabTitle("Terminal") + OSC clear
    - `windowCount >= 1`（統合）: `renderMuxSubTabs(tab.id, windows)` を呼ぶ。windows は `info.windowNames.map((name, i) => ({ name, active: i === info.activeWindow }))`
- `src/tab-bar/tab-bar-ui.ts`
  - `renderMuxSubTabs` 冒頭の `if (windows.length <= 1) { this.restoreMuxOriginalTab(tabId); return; }` 分岐を削除すること（mux モード中は常に group 化）
  - 各 `.mux-window-tab` 内の DOM 構造を `<span class="mux-window-number">[N]</span><span class="tab-title">title</span>` に変更すること
  - 差分更新の際、番号 span のテキスト (`[${i+1}]`) と title span のテキストを独立に比較・更新すること
  - 差分更新ロジックは `winTab.children[0]` を number span、`winTab.children[1]` を title span として参照する。新規作成時のみ 2 span を append する構造へ変更する（既存コードは create 時に `.tab-title` 1 個のみを追加している点を修正）
- `src/styles/tab-bar.css`
  - `.mux-window-number { font-size: 0.85em; opacity: 0.75; margin-right: 0.25em; font-variant-numeric: tabular-nums; }` を追加すること
- `src/terminal-app/mux/mux-session.ts`（必要に応じて）
  - mux mode 終了時（`windowCount === 0`）のみ `clearMuxSubTabs` 経由で `restoreMuxOriginalTab` を呼ぶ既存経路が機能することを確認すること（`detach` 動作で副作用が起きないこと）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `renderMuxSubTabs` 変更 | 単一ウィンドウ時も group を維持し、番号 span を管理すること | mux mode 中の呼び出し | `windows.length >= 1` で `mux-tab-group` が存在し、各子 `.mux-window-tab` に `[N]` と title が表示されること |
| `.mux-window-number` スタイル | 番号バッジの見た目 | CSS 読み込み済 | 0.85em, opacity 0.75 で描画されること |

**Processing Flow** — 改訂後の `renderMuxSubTabs(tabId, windows)`:

1. `windows.length < 1` なら（理論上起こらないが念のため）何もせず return
2. グループ存在確認 → 不在なら group を新規作成し、既存 tab 要素を置き換えて `muxOriginalTabs` に保存する（既存ロジック踏襲）
3. 子要素数を `windows.length` にトリム or 拡張（既存ロジック踏襲）
4. 各 `.mux-window-tab` 子要素について:
   - 子が未初期化なら `mux-window-number` と `tab-title` の 2 span を生成して追加する
   - `numberEl.textContent` を `[${i + 1}]` に差分更新する
   - `titleEl.textContent` を `win.name` に差分更新する
   - `title` 属性と `mux-window-active` クラスを従来通り更新する

**Implementation Steps**:

1. **main.ts 分岐修正** — `windowCount === 1` のときも `renderMuxSubTabs` を呼ぶように統合する（`windowCount === 0` のみ `clearMuxSubTabs`）
2. **単一ウィンドウ分岐削除** — `renderMuxSubTabs` 冒頭の `windows.length <= 1` 早期 return を削除し、後段の group 化経路に合流させる
3. **DOM 構造変更** — 子 tab 生成時に `.mux-window-number` span と `.tab-title` span を両方生成する（既存は `.tab-title` のみ append している）
4. **差分更新ロジック** — `winTab.children[0]` を number span、`winTab.children[1]` を title span として参照し、番号とタイトルを独立に比較・更新する
5. **CSS 追加** — `tab-bar.css` に `.mux-window-number` スタイルを追加する
6. **mux mode 終了時経路確認** — `clearMuxSubTabs` → `restoreMuxOriginalTab` の既存経路が mux mode 完全終了時（`windowCount === 0`）にのみ呼ばれることを確認する

**Dependencies**: Phase 3 の broadcast で順序変更が伝搬すること（描画経路の整合確認）。Blocks: Phase 8。

**Testing Approach**:

- Unit: `renderMuxSubTabs` のテストは既存未整備のため必須化しない。代わりに
  - 手動確認: mux 1 window で `[1] name` 表示、複数 window で各 `[N] name`、move 後の番号更新
  - Docker E2E（Phase 8）でタブ DOM を assert する
- UI 一貫性: `doc/UI-DESIGN-GUIDELINES.yaml` の tab-bar セクション更新要否を確認する

**Acceptance Criteria**:

- [ ] `bun run typecheck` / `bun run build` が通ること
- [ ] 単一ウィンドウ時も `[1] title` が見えること（main.ts 分岐統合の効果）
- [ ] 複数ウィンドウ時に `[1] / [2] / [3] / ...` が各タブに描画されること
- [ ] 番号部分が `0.85em` 程度・ほぼ透明度 0.75 で描画されること
- [ ] mux mode 終了（detach = windowCount 0）時に通常タブに戻ること
- [ ] move-window 実行後、楽観更新により即座に `[N]` 番号が新順に再描画されること（Phase 6 の reorderMuxWindows → emitMuxStateChange 経由）

**Estimated Effort**: small

---

### Phase 8: E2E テスト（Docker）

**Goal**: `./scripts/run-e2e-docker.sh test` 上で並び替え操作と番号表示をエンドツーエンドで検証すること。

**Files to Create**:

- `e2e-tests/specs/mux-move-window.e2e.js`（新設）

**Key Scenarios**:

| ID | シナリオ | 期待結果 |
|----|----------|----------|
| E2E-1 | `emterm mux` → `prefix + c` を 2 回で 3 window 作成 → `prefix + m` → `1` Enter | active window が position 1（先頭）になること。DOM 順序が期待通り |
| E2E-2 | 3 window 状態で `prefix + m` → Esc | ダイアログが閉じ順序不変 |
| E2E-3 | 3 window 状態で `prefix + m` → `999` Enter | ダイアログが閉じ順序不変 |
| E2E-4 | 3 window 状態で `prefix + m` → `abc` Enter | ダイアログが閉じ順序不変 |
| E2E-5 | 3 window 状態で `prefix + m` → 現在と同じ番号 Enter | ダイアログが閉じ順序不変 |
| E2E-6 | `emterm mux` 直後（1 window のみ）にタブを確認 | タブに `[1]` が表示されること |

**Processing Flow** — 各シナリオ:

1. `./scripts/run-e2e-docker.sh test mux-move-window.e2e.js` で起動
2. Docker 内で Xvfb → tauri-driver → WebKitWebDriver → WebdriverIO が走る
3. spec ファイル内で `await browser.keys(["Control", "b"])` → `await browser.keys([actionKey])` で prefix+action を送出
4. DOM 確認は `browser.execute(() => { ... document.querySelector(".mux-tab-group") ... })` 経由で行う

**Implementation Steps**:

1. **既存 spec の流用** — `mux-multi-session.e2e.js` から prefix 送信ヘルパを参考にする
2. **スクリーンショット** — 各シナリオ末で `browser.saveScreenshot(...)` を呼び、`e2e-tests/screenshots/` に残す
3. **selector 設計** — `.mux-tab-group > .mux-window-tab > .mux-window-number` で番号テキストを assert する
4. **タイミング制御** — IPC round-trip を待つため、`browser.waitUntil(...)` で DOM 状態変化を待つ（Docker 環境の既定 180s タイムアウト）

**Dependencies**: Phase 1〜7 すべて。Blocks: なし。

**Testing Approach**:

- E2E (`./scripts/run-e2e-docker.sh test mux-move-window.e2e.js`): 上記シナリオ 6 件を自動実行
- 手動: Docker 上で UI 目視（番号バッジの視認性）

**Acceptance Criteria**:

- [ ] E2E-1〜E2E-6 が全通過すること
- [ ] 既存 mux E2E（`mux-multi-session.e2e.js` 等）が非回帰であること

**Estimated Effort**: medium

---

## Complete File Structure

```
emterm/
├── doc/tasks/mux-move-window/
│   ├── SPEC.md                                     (既存)
│   ├── requirements.md                             (既存)
│   ├── sdd.yaml                                    (既存)
│   ├── IMPLEMENTATION.md                           (本ファイル)
│   ├── VERIFICATION.md                             (新規)
│   └── tasks.yaml                                  (新規)
├── src-tauri/src/mux/
│   ├── session/
│   │   ├── session.rs                              (Phase 1, 3: window_order, move_window)
│   │   └── manager.rs                              (Phase 1: session_list)
│   └── ipc/
│       ├── protocol.rs                             (Phase 2: MessageType, MoveWindowMsg, tests)
│       ├── handlers.rs                             (Phase 3: handle_move_window)
│       └── connection.rs                           (Phase 3: route_message dispatch)
├── src/
│   ├── terminal/mux/
│   │   ├── prefix-key.ts                           (Phase 4: MuxAction, bindings)
│   │   ├── prefix-key.test.ts                      (Phase 4: テスト拡張)
│   │   └── mux-client.ts                           (Phase 2: MuxMessageType.MoveWindow)
│   ├── main.ts                                     (Phase 7: onMuxStateChange 分岐統合)
│   ├── terminal-app/mux/
│   │   ├── move-window-dialog.ts                   (Phase 5: 新規)
│   │   ├── move-window-dialog.test.ts              (Phase 5: 新規)
│   │   ├── mux-action-handler.ts                   (Phase 6: case "move-window")
│   │   ├── mux-window-manager.ts                   (Phase 6: reorderMuxWindows 関数追加)
│   │   └── mux-window-manager.test.ts              (Phase 6: reorderMuxWindows テスト。既存あれば拡張、無ければ新設)
│   ├── tab-bar/
│   │   └── tab-bar-ui.ts                           (Phase 7: renderMuxSubTabs)
│   ├── styles/
│   │   └── tab-bar.css                             (Phase 7: .mux-window-number)
│   └── i18n/locales/
│       ├── en.json                                 (Phase 5: mux.moveDialog.*)
│       └── ja.json                                 (Phase 5: mux.moveDialog.*)
└── e2e-tests/specs/
    └── mux-move-window.e2e.js                      (Phase 8: 新規)
```

## Testing Strategy

- **Unit (Rust)**: `cargo test --manifest-path src-tauri/Cargo.toml` — `session`、`manager`、`protocol`、可能なら `handlers`
- **Unit (TS)**: `bun test` — `prefix-key.test.ts`、`move-window-dialog.test.ts`、`mux-client.test.ts`（`MoveWindow` 定数）
- **Typecheck**: `bun run typecheck`
- **E2E (Docker)**: `./scripts/run-e2e-docker.sh test mux-move-window.e2e.js`（個別） / `./scripts/run-e2e-docker.sh test`（全体）
- **手動**: モーダルの見た目、番号バッジのサイズ感、IME 動作、フォーカス復帰
- **カバレッジ目標**: `MuxSession::move_window` / `MoveWindowMsg` 周りは分岐網羅（境界・同一位置・未知 id・範囲外）

すべて Docker で実行すること（ホスト設定汚染を避けるため）。

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| （新規依存なし） | — | Option A 採用のため外部 crate 追加不要 |

既存で使用中の `bincode`、`serde`、`tokio`、`rust-i18n` で事足りる。

### Phase 依存関係マトリクス

```
Phase 1 (backend order model) ─┬→ Phase 3 (backend move handler)
Phase 2 (IPC protocol) ────────┤
                                └→ Phase 6 (via Phase 3)

Phase 4 (prefix key) ──────────→ Phase 5 (dialog) ──→ Phase 6 (action dispatch)
Phase 7 (tab render) ──────────→ (推奨順序のみ、Phase 6 と strict な dep なし)

Phase 6 ──→ Phase 8 (E2E)
Phase 7 ──→ Phase 8 (E2E)
```

- Phase 7 は Phase 6 の視覚確認を容易にするため先行を推奨するが、技術的依存ではない
- Phase 7 と Phase 8 以外は並列化可能（Phase 1/2 と Phase 4 は独立）

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `window_order` と `windows` (BTreeMap) の整合性逸脱（add/remove のどちらか片方漏れ） | 中 | 高 | Phase 1 で unit テストを追加し、add/remove 両方を走査するケースでインバリアントを検証する |
| IME commit-Enter でのダイアログ誤確定 | 低 | 中 | `e.isComposing || keyCode === 229` ガードを既存 rename ダイアログと同一に実装 |
| タブ幅増によるレイアウト影響 | 低 | 低 | バッジを `0.85em` に抑え、`tabular-nums` で桁揺れを避ける |
| 単一ウィンドウ時に group 化するとドラッグ挙動が変化 | 低 | 中 | `drag-handler.ts` は既に `.mux-tab-group` を受容しているため挙動不変。念のため E2E で既存 mux ドラッグテストが通ることを確認する |
| 楽観更新と daemon 状態の乖離（IPC 失敗時） | 低 | 中 | NFR4 の非破壊性は「失敗時に順序が崩れないこと」ではなく「UI とバックエンドの双方が同一順序に整合すること」を指す。IPC 送信失敗は `sendControl` の catch でログだけ残し、**ローカル順序は元に戻さない**（UI は楽観更新のまま。次回 attach で Welcome により整合）。この挙動を実装・テストで明示する |
| 楽観更新中に daemon 側でウィンドウが削除された場合の整合 | 低 | 中 | `currentIdx = findIndex(id === targetWinId)` で再解決し `< 0` なら中止。ダイアログ中にウィンドウが閉じても楽観更新は実行しない |
| 単一ウィンドウ時に `renderMuxSubTabs` を呼ぶよう `main.ts` 分岐を変えたことで既存 E2E が回帰 | 中 | 中 | Phase 8 の既存 mux E2E（`mux-multi-session.e2e.js` など）を回帰確認する。単一ウィンドウが常に tab-group として描画される変更に依存するアサーションが既存 E2E にあれば更新する |
| `active_window_id` の remove_window 時の再選出挙動変更（BTreeMap 最小 id → window_order 先頭） | 低 | 中 | 挙動変更を `test_active_window_id_after_remove_uses_order` で明示的に固定する。ユーザー可視の挙動として「最も古く作成されたウィンドウが次の active になる」に変わる点を本計画で文書化 |

## Open Questions

- なし（論点 A/B/C/D はこの計画で確定、SPEC.md は仕様確定済）

## Success Metrics

- [ ] FR1〜FR7 の全機能要件が実装・テストで検証済
- [ ] NFR1（Linux/Windows）、NFR2（UI 一貫性）、NFR3（200ms 以内）、NFR4（失敗時非破壊）が達成
- [ ] E2E-1〜E2E-6 全通過、既存 mux E2E 非回帰
- [ ] `cargo fmt` / `bun run typecheck` が警告・エラー無し
