# Implementation Plan: visibility-aware-pty-streaming

## Overview

frontend が hidden の間は backend (Tauri ローカル PTY) と mux daemon が PTY 出力を内部 shadow parser とリングバッファに保持し、Tauri Channel への送信を停止する。visible 復帰時に最新画面を再現する 1 メッセージのスナップショットだけを送る構造に切り替える。既存の rAF fallback / heartbeat / forward anyway / early reinit といった対症療法は本機能の動作確認後に撤去する。

## Objectives

- 非 mux と mux 両方で hidden 中の `pty_get_send_stats.sent_bytes` 増加を 0 にする
- 復帰時の main thread block を 200ms 以内に収める
- 既存対症療法 (FR15) を撤去し、診断ログを整理する
- TDD: テストファースト可能な phase は明示する

## Prerequisites

### Development Environment

- Rust toolchain (stable, 1.75 以降)
- Bun runtime
- Docker (テスト実行用 — host で test を直接走らせない)
- `rsvg-convert` または `magick` (icons 生成、postinstall 経由)

### Dependencies

- 既存 `vt100 = "0.15"` crate (shadow parser)
- 既存 `portable-pty` (PTY 抽象)
- 既存 `DetachRingBuffer` (`src-tauri/src/mux/ring_buffer.rs`) — mux 側で再利用
- 既存 Tauri API `getCurrentWebviewWindow().onFocusChanged`

### Read Before Starting

- `doc/tasks/visibility-aware-pty-streaming/SPEC.md` (Single Source of Truth)
- `src-tauri/src/reader.rs` (PTY reader thread の現状)
- `src-tauri/src/pty/backpressure.rs` (`MAX_BACKPRESSURE_WAIT` 経路と `add_sent`)
- `src-tauri/src/mux/ipc/reattach.rs` (`build_shadow_parser_snapshot`、`collect_reattach_data`、`detach_session_panes`)
- `src-tauri/src/mux/session/pane.rs` (`PaneOutputTarget`、`MuxPane`)
- `src-tauri/src/mux/ipc/protocol.rs` (`MessageType` 列挙、`from_u8`)
- `src/terminal-app/pty-handler.ts` (FR15 撤去対象の対症療法)
- `src/pty/client.ts` (`PtyClient` の現状)
- `src/terminal/mux/mux-client.ts` (`MuxMessageType` と APC エンコード)

## Architecture Overview

### Technology Stack

- **Backend**: Rust (Tauri バックエンド + 別プロセス mux daemon)
- **Frontend**: Vanilla TypeScript
- **WASM**: `wasm/src/` — 本機能で **変更しない** (例外なし)
- **IPC**: Tauri Channel (バイナリ raw) / Tauri invoke (制御) / mux APC over PTY (mux 用)

### Design Approach

ボトムアップ。まず非 mux backend のスナップショット保持と Tauri command を完成し、次に frontend を繋ぎ、最後に mux daemon に同等機構を移植する。既存対症療法の撤去は通知経路がすべて動いた後に行う。

### Component Interaction

```
Frontend (TS)
  VisibilityController
    ├─ visibilitychange + onFocusChanged listener
    ├─ debounce (1000ms hide, 0ms show)
    ├─ ヘルスチェックタイマ (10 秒)
    └─ → PtyClient.setVisibility()
         → MuxClient.sendSetVisibility()
              ↓ APC (mux のみ)
              ↓ Tauri invoke (非 mux)

Backend (Rust)
  PtyManager
    ├─ visibility_registry (新規)
    │     SessionVisibilityState per session
    │       visible: AtomicBool
    │       shadow: vt100::Parser
    │       ring: HiddenRingBuffer
    │       raw_passthrough: RawPassthroughBuffer
    └─ Reader thread (reader.rs)
         visible → channel.send + add_sent
         hidden  → shadow.process + ring.write + scanner.extract → raw_passthrough.append

Mux Daemon
  ConnectionState
    visible: AtomicBool (新規)
    network_detach: bool
  PerPane (MuxPane)
    output_target: Connected(tx) | Detached(buf)
    shadow_parser: vt100::Parser
    raw_passthrough: RawPassthroughBuffer (新規)
  evaluate_output_target() runs on:
    - SetVisibility 受信時
    - reattach 完了時 (collect_reattach_data 内)
```

## Implementation Phases

### Phase 1: Backend foundation (非 mux)

**Goal**: 非 mux PTY セッションで visibility-aware streaming を完成させる (Tauri 内蔵 PTY 経路のみ)。

**Files to Create**:
- `src-tauri/src/pty/visibility.rs` — `SessionVisibilityState`、`RawPassthroughBuffer`、定数 (`HIDDEN_PASSTHROUGH_CAPACITY` = 4 MiB (非 mux) / 1 MiB (mux), `PARTIAL_SEQUENCE_MAX = 16 MiB`, `HIDDEN_DEBOUNCE_MS = 1000`)。`HiddenRingBuffer` は本機能では作成しない (Decision Log 参照)
- `src-tauri/src/pty/passthrough_scanner.rs` — `PassthroughScanner` struct (stateful, chunk 跨ぎ対応)。`process(&mut self, data: &[u8]) -> Vec<u8>` で Kitty APC G、SIXEL DCS q、OSC 9999 を state machine で抽出

**Files to Modify**:
- `src-tauri/src/pty/manager.rs` — `PtyManager` に visibility registry (セッション ID → `SessionVisibilityState` の Arc) を持たせる。`create_session_atomic` で初期 `visible=true` で登録、`remove_session_atomic` で除去
- `src-tauri/src/pty/mod.rs` — 新規 module 公開
- `src-tauri/src/reader.rs` — reader thread の hidden 経路追加 (詳細は処理フロー)、`pty_heartbeat` emit と `HEARTBEAT_INTERVAL` を撤去
- `src-tauri/src/pty/backpressure.rs` — `MAX_BACKPRESSURE_WAIT` 定数を撤去し、`wait_for_drain` の loop 条件を `while in_flight > LOW_WATER_BYTES` (タイムアウト無し) に変更。新規 `set_hidden_wake()` (visibility hidden 通知時の wake) を追加。`HIGH_WATER_BYTES` / `LOW_WATER_BYTES` は visible 中の保護として維持。既存テスト `test_wait_for_drain_returns_quickly_when_under_water` / `test_wait_for_drain_wakes_on_ack` は新方式の挙動 (timeout 無し ack 駆動) に書き換え
- `src-tauri/src/tauri_commands.rs` — `pty_set_visibility(state, session_id, visible) -> ()` を新設、`PtyManager` の visibility registry を経由して `set_hidden()` または `set_visible_and_take_snapshot()` を呼ぶ。後者の戻り値 `Option<Vec<u8>>` は専用イベント経路で frontend に届ける
- `src-tauri/src/lib.rs` (または `main.rs`) — Tauri の invoke handler 登録機構に `pty_set_visibility` を追加

**Snapshot 配送経路の決定:** `pty_set_visibility` は同期 Tauri command なので、その戻り値で snapshot を返すのは IPC 互換性の都合上避ける。代わりに、`PtyManager` に reader 用 Channel の登録機構を持たせる (既存 `spawn_reader_thread` と同じ Channel を再利用) か、新規 emit イベント経路を用いる。**採用案: 既存 reader Channel への参照を `SessionVisibilityState` にも保持させ、snapshot は通常 PTY データと同じ `Channel<InvokeResponseBody::Raw>` 経路で frontend WASM が消費できるようにする** (FR12 を満たす)。

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `SessionVisibilityState` | 1 セッションの hidden/visible 状態と shadow ステート所有 | `new(cols, rows)` で作成済み | `is_visible()` lock-free 読み取り可。`set_hidden()` / `set_visible_and_take_snapshot()` で状態遷移 |
| `RawPassthroughBuffer` | 画像/Markdown OSC 列の保持 (容量パラメータ化、末尾保持) | `new(capacity)` | `append(&[u8]) -> bool` (戻り値 = drop 発生)、`read_all() -> Vec<u8>`、`clear()` |
| `PassthroughScanner` | chunk 跨ぎ対応 stateful state machine。Kitty APC G / SIXEL DCS q / OSC 9999 を抽出 | `new()` | `process(&mut self, data: &[u8]) -> Vec<u8>` で完成した sequence のみを返す。partial buffer が `PARTIAL_SEQUENCE_MAX` を超えたら対象 sequence を放棄し warn |
| `pty_set_visibility` Tauri command | frontend からの状態通知を受け、registry に反映、snapshot を Channel 経由送信 | session_id 既知 | visibility registry の状態が更新され、resume 時は snapshot bytes が Channel に流れる |

**Processing Flow (reader thread の hidden 経路)**:
1. reader thread が PTY から batch を受け取る
2. visibility registry から該当セッションの状態を取得 (lock-free `is_visible()` で判定)
3. 分岐:
   - visible == true → 既存経路 (`wait_for_drain` (timeout 無し ack 駆動) → `add_sent` → Channel へ Raw 送信 → 送信成功カウンタ更新)
   - visible == false → 状態オブジェクトの `process_hidden(&batch)` を呼び、shadow と raw_passthrough を更新。Channel 送信、`add_sent`、`wait_for_drain` はすべて skip
4. 既存の drain 待機呼び出しは visible == true 経路の中でのみ実施。hidden 中は backpressure を完全にスキップする
5. visibility 状態が visible → hidden に変わった時点で `backpressure.set_hidden_wake()` を呼び、`wait_for_drain` 中の reader を起こして上記分岐へ進ませる
6. heartbeat emit 関連コードは削除する (FR15)

**Processing Flow (`set_visible_and_take_snapshot`)**:
1. visible フラグを atomic に true へ swap し、前状態を取得
2. 前状態が既に true なら `None` を返す (no-op)
3. lock した shadow から現在画面の formatted contents を取得
4. snapshot バイト列 = `b"\x1b[H\x1b[2J"` プレフィックス + shadow contents + raw_passthrough の全バイト
5. raw_passthrough をクリアし、drop_warned フラグもリセット
6. snapshot バイト列を Some で返す
7. command 側はこれを Channel に 1 メッセージで送信する

**Implementation Steps** (TDD 推奨):
1. **Test-first: visibility.rs の単体テストを書く** (`RawPassthroughBuffer` 末尾保持と drop 警告フラグ、`SessionVisibilityState` 状態遷移、resize)
2. **Test-first: passthrough_scanner.rs の単体テスト** (chunk 跨ぎ、partial buffer 上限超過、無関係バイト列)
3. **visibility.rs と passthrough_scanner.rs を実装** (テストが pass するまで)
4. **manager.rs に visibility registry を追加**、create/remove セッション時の登録解除
5. **`pty_set_visibility` Tauri command を実装**、Tauri の invoke handler 登録機構 (`src-tauri/src/app.rs` の `tauri::generate_handler!`) に追加、snapshot 送信は reader thread と同じ Channel を経由する (registry が `Channel<InvokeResponseBody>` の Arc clone を保持)
6. **reader.rs の hidden 経路を追加**、heartbeat emit と `HEARTBEAT_INTERVAL` 削除、`PtyHeartbeatPayload` 削除
7. **backpressure.rs の wait_for_drain を ack 駆動 (timeout 無し) に書き換え**、`MAX_BACKPRESSURE_WAIT` 定数削除、`set_hidden_wake` 追加。既存テスト 2 本の書き換え
8. **統合テスト**: hidden 状態で 10 MiB のテストバイト列を `process_hidden` 経由で流し、`sent_bytes` が増えないこと、`set_visible` で snapshot が返ることを確認

**Dependencies**: なし (foundation phase)

**Testing Approach**:
- Unit (Rust): visibility.rs の各構造体テスト、passthrough_scanner のパターン抽出テスト
- Integration (Rust): `pty_set_visibility` を tokio test で呼び、Channel mock に snapshot が届くことを確認
- Manual: 単体ではフリーズ症状再現できないため、Phase 4 で実機確認

**Acceptance Criteria**:
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -p emterm visibility` が pass
- [ ] `cargo test ... passthrough_scanner` が pass (chunk 跨ぎ test 含む)
- [ ] hidden 中は `pty_get_send_stats.sent_bytes` が増えない (mock test)
- [ ] visible 復帰時に snapshot が 1 message で Channel に届く
- [ ] `pty_heartbeat` event listener コード、`HEARTBEAT_INTERVAL` 定数、`PtyHeartbeatPayload` が backend に存在しない
- [ ] `MAX_BACKPRESSURE_WAIT` 定数が backend に存在せず、`wait_for_drain` がタイムアウト無し ack 駆動で動作する (新テスト pass)
- [ ] `HIGH_WATER_BYTES` / `LOW_WATER_BYTES` および `wait_for_drain` 自体は visible 中の保護として残っている

**Estimated Effort**: medium

---

### Phase 2: Frontend integration

**Goal**: frontend で `VisibilityController` を起動し、debounce 付きで backend / mux daemon に visibility 状態を通知する。`PtyClient.setVisibility` と `MuxClient.sendSetVisibility` を追加する。

**Files to Create**:
- `src/pty/visibility-controller.ts` — `VisibilityController` クラス。constructor: `(ptyClient, muxClient | null)`。`start()` で listener 登録 + 10 秒ヘルスチェック。`stop()` で解除。

**Files to Modify**:
- `src/pty/client.ts` — `setVisibility(visible: boolean): Promise<void>` メソッドを追加 (内部で `pty_set_visibility` Tauri command を invoke)
- `src/terminal/mux/mux-client.ts` — `MuxMessageType.SetVisibility = 0x1B` を定数に追加 (mux-client 側の MessageType マップ)、`sendSetVisibility(visible: boolean): void` メソッドを追加。payload は 1 byte (visible == true なら 0x01、それ以外 0x00)、pane_id は 0 (session-wide)
- `src/terminal-app/index.ts` — `TerminalApp` 初期化フローで `VisibilityController` を生成し `start()` 呼び出し、destroy で `stop()`。タブごとに 1 instance 持つか、アプリで 1 instance 全タブ共有か実装中に決定 (推奨: 全タブ共有 1 instance、active session を都度参照)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `VisibilityController` | visibilitychange + onFocusChanged 統合判定、debounce、通知 | DOM 利用可 | start 後は確定状態の変化ごとに ptyClient/muxClient へ通知 |
| `PtyClient.setVisibility` | 該当セッションへの通知を invoke | sessionId が null でない | backend visibility registry が更新される。session 不一致時は warn ログ (backend 側) |
| `MuxClient.sendSetVisibility` | mux daemon へ APC 送信 | mux に attach 済み | daemon ConnectionState.visible が更新される |
| ヘルスチェックタイマ (10 秒) | 直近確定状態を再送 (NFR5) | controller 起動中 | 通知欠落時の最終整合性回復 |

**Processing Flow (VisibilityController 状態判定)**:
1. `visibilitychange` または `onFocusChanged` 発火
2. 統合判定: effectiveVisible = (`document.visibilityState` が "visible") かつ (webview が focused)
3. 直近確定状態と同じなら何もしない
4. effectiveVisible == true (= visible への遷移): pending hide タイマがあればキャンセル、即座に通知
5. effectiveVisible == false (= hidden 候補): 既に pending hide タイマがあれば何もしない、なければ 1000ms タイマを開始。タイマ満了時に再判定して依然 hidden なら通知

**Processing Flow (ヘルスチェックタイマ)**:
1. 10 秒ごとに発火
2. 現在の effective state を計算
3. backend / daemon に再送 (idempotent)。状態が一致していれば backend 側は no-op に近い

**Processing Flow (FR15 撤去対象、本 phase で削除)**:
1. `pty-handler.ts` 内の以下を削除:
   - `RAF_FALLBACK_MS` 定数と setTimeout fallback
   - `pty_heartbeat` event listener (Tauri `listen` 経由で登録されているもの)
   - `heartbeatWakeWarnedDuringStall` 関連
   - `healthCheck` 内の early WASM reinit ブロック
   - `onVisibilityChange` 内の drain trigger (新方式の VisibilityController に置き換え)
   - 関連診断ログ (`heartbeat-wake`, `rAF fallback fired`, `early WASM reinit`, visibility 関連の `DIAG-MUX-*`)
2. **残す** (FR16): `onFocusChanged` の WASM 健全性プローブ (`cols()` 呼び出し)、`tryRecoverFromWasmCrash`

**Implementation Steps** (TDD 推奨):
1. **Test-first: VisibilityController の単体テストを書く** (debounce 動作、即時 visible、ヘルスチェック再送)
2. **PtyClient.setVisibility メソッドを実装** (invoke wrap だけ、簡潔)
3. **MuxClient に MessageType と sendSetVisibility を追加**、APC エンコード経路を再利用
4. **VisibilityController を実装**
5. **`terminal-app/index.ts` で controller を起動**
6. **pty-handler.ts の対症療法を削除** (FR15)、bun test と typecheck で回帰なし確認

**Dependencies**: Requires Phase 1 (backend `pty_set_visibility` が存在し動作する)

**Testing Approach**:
- Unit (TS): VisibilityController の debounce / 即時 visible / ヘルスチェック (Bun test の fake timers 相当を利用)
- Unit (TS): `MuxClient.sendSetVisibility` が正しい APC バイト列を生成すること (mux-client.test.ts に追加)
- Manual: focus 変化時に backend ログ (`log::warn!` 状態遷移) が想定どおり出ることを実機で確認

**Acceptance Criteria**:
- [ ] `bun test src/pty/visibility-controller.test.ts` が pass
- [ ] `bun test src/terminal/mux/mux-client.test.ts` で SetVisibility エンコードが pass
- [ ] `bun run typecheck` がエラーなし
- [ ] pty-handler.ts から `RAF_FALLBACK_MS`, `pty_heartbeat`, `heartbeatWakeWarnedDuringStall`, `healthCheck` 内 early reinit, `onVisibilityChange` drain trigger が消えている
- [ ] pty-handler.ts に `onFocusChanged` の `cols()` 健全性プローブと `tryRecoverFromWasmCrash` が残っている

**Estimated Effort**: medium

---

### Phase 3: Mux daemon support

**Goal**: mux daemon に SetVisibility メッセージ受信、`evaluate_output_target` による soft detach、reattach 時の visibility 評価、raw_passthrough 連結を実装する。

**Files to Modify**:
- `src-tauri/src/mux/ipc/protocol.rs` — `MessageType::SetVisibility = 0x1B` を enum と `from_u8` に追加。`SetVisibilityPayload { visible: bool }` 定義 (1 byte)。**既存テスト破綻の修正必須**: `test_message_type_round_trip` のループ範囲を `0x01..=0x1Bu8` に拡張、`assert!(MessageType::from_u8(0x1b).is_none())` を `assert_eq!(MessageType::from_u8(0x1B), Some(MessageType::SetVisibility))` に置き換え、未使用 opcode の `is_none` チェックを `0x1c` 以降に変更。`test_apc_round_trip_all_message_types` のループ範囲も同様に拡張
- `src-tauri/src/mux/ipc/handlers.rs` — `handle_set_visibility(visible: bool, session_manager, active_session_id, pane_output_tx, visible_state: &Arc<AtomicBool>)` ハンドラ追加。`route_message` の match に SetVisibility arm を追加し、`visible_state.store(...)` 更新後、`active_session_id` の各ペインに対し `evaluate_output_target` を呼ぶ。visible == true の場合は snapshot 送信も実施
- `src-tauri/src/mux/ipc/connection.rs` — `handle_connection` 内 loop ローカルに `let visible_state = Arc::new(AtomicBool::new(true));` を追加し、`route_message` 呼出時と `collect_reattach_data` 呼出時にこの参照を渡す (Decision Log 案 A)。`ConnectionState` という新規 struct は導入しない
- `src-tauri/src/mux/session/pane.rs` — `MuxPane` に `raw_passthrough: Arc<StdMutex<RawPassthroughBuffer>>` と `passthrough_scanner: Arc<StdMutex<PassthroughScanner>>` フィールド追加 (容量は mux 用 1 MiB)。`evaluate_output_target(pane: &MuxPane, network_detach: bool, visible: bool, owned_tx: &Sender<PtyOutputChunk>) -> EvalResult` 関数を追加。`!network_detach && visible` なら Connected(owned_tx) に切り替え (このとき visible 復帰の snapshot 送信は handler 側で同一 lock 内で実施するために `EvalResult::ResumeWithSnapshot { combined: Vec<u8> }` のような形で snapshot bytes を返す)。それ以外は identity-scoped で Detached に切替 (現 Connected の tx が `same_channel(owned_tx)` の場合のみ)
- `src-tauri/src/mux/ipc/pty_spawn.rs` (`pty_reader_loop`) — Detached 経路を通る際、shadow_parser に process した後で同じ data を `pane.passthrough_scanner.process(data)` に通し、抽出 bytes を `pane.raw_passthrough.append` する
- `src-tauri/src/mux/ipc/reattach.rs` — `collect_reattach_data` の中で、各ペインの snapshot 構築時に raw_passthrough の全バイトを連結。送信後 `clear()`。reattach 完了直後に `visible_state` を読み、各ペインに対し `evaluate_output_target(network_detach=false, visible=current_visible, owned_tx)` を呼ぶ

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MessageType::SetVisibility` | 0x1B opcode、GUI→Daemon 片方向 | 既存 codec で frame 解析可能 | route_message で handler に到達 |
| `handle_set_visibility` | ConnectionState.visible を更新、全ペインで `evaluate_output_target` | session attach 済み | 各ペインの output_target が新しい状態に応じて切り替わる |
| `ConnectionState.visible` | クライアント単位の hidden/visible 状態 | 接続時 true で初期化 | SetVisibility で更新 |
| `evaluate_output_target` | (network_detach, visible) 二軸から output_target を決定 | pane ロック可能 | network_detach==true OR visible==false なら Detached、両方解消で Connected |
| `MuxPane.raw_passthrough` | ペインごとの画像/Markdown OSC バッファ | new で容量初期化 | hidden / detach 中の reader が append、reattach / visible 復帰時に read_all + clear |
| `pty_reader_loop` (Detached 経路) | shadow process + raw passthrough append | output_target == Detached | shadow と raw_passthrough が両方更新される |
| `collect_reattach_data` (拡張) | snapshot に raw_passthrough を連結、reattach 後 visibility 評価 | 既存ロジック動作中 | snapshot に画像/Markdown が含まれ、hidden 時は再 Detached |

**Processing Flow (SetVisibility 受信)**:
1. APC over PTY から `MessageType::SetVisibility(visible)` を受信
2. loop ローカルの `visible_state: Arc<AtomicBool>` を atomic に更新
3. session manager から該当セッションのすべての pane を列挙
4. 各 pane について `pane.output_target` の lock を取った上で:
   - visible == true: snapshot bytes (`build_shadow_parser_snapshot` + `raw_passthrough.read_all()`) を生成し、`pane_output_tx.send(PtyOutputChunk { snapshot })` を実施 → `raw_passthrough.clear()` → `output_target = Connected(pane_output_tx)`
   - visible == false: 現 Connected の tx が `owned_tx.same_channel(...)` で一致する場合のみ `output_target = Detached(DetachRingBuffer::new(DEFAULT_RING_CAPACITY))` に切替 (identity-scoped)
5. 状態遷移を `log::debug!` で記録 (NFR6 改訂)。drop / send 失敗のみ `warn`

**Processing Flow (Reattach 時の visibility 評価)**:
1. `collect_reattach_data` 呼び出しで、各ペインの snapshot 生成 (既存ロジック)
2. snapshot に raw_passthrough の全バイトを連結 (新規)
3. 旧 ring の read_all + clear (既存ロジック)
4. raw_passthrough を clear (新規)
5. すべての pane について output_target を Connected(tx) に戻す (既存ロジック)
6. caller 側 (`handle_attach`) で `visible_state.load()` を確認し、false なら直後に `evaluate_output_target(network_detach=false, visible=false, owned_tx)` で各ペインを再 Detached に切り替え (snapshot は frontend に届くが、その後の出力はまた buffer)

**Processing Flow (Detached 経路での raw_passthrough 蓄積)**:
1. `pty_reader_loop` が PTY batch を受信
2. pane の shadow_parser に batch を process (常に)
3. pane の output_target の状態を確認
4. Detached なら:
   - 既存: `DetachRingBuffer` に `write(data)` (容量 64 MiB)
   - 新規: `extracted = pane.passthrough_scanner.lock().unwrap().process(data)` を計算し、`pane.raw_passthrough.lock().unwrap().append(&extracted)`。drop が発生したら warn ログを 1 回だけ
5. Connected なら既存どおり channel 送信

**Implementation Steps** (TDD 推奨):
1. **Test-first: protocol.rs に SetVisibility round-trip テストを書く** (encode → decode で `visible` 値が一致)
2. **既存 protocol.rs テスト 2 本 (`test_message_type_round_trip`, `test_apc_round_trip_all_message_types`) のループ範囲を `0x1B` まで拡張、`is_none` チェックを `0x1c` 以降に変更**
3. **MessageType に 0x1B を追加**、`from_u8` 拡張、payload 構造体定義、テスト pass
4. **MuxPane に raw_passthrough と passthrough_scanner フィールドを追加**、テスト用 helper を更新
5. **`evaluate_output_target` 関数を実装**、各分岐の単体テスト (identity-scoped Connected→Detached、無条件 Detached→Connected、no-op、snapshot bytes return)
6. **`handle_set_visibility` を実装**、`route_message` に登録、loop ローカルから `visible_state` 参照
7. **`pty_reader_loop` の Detached 経路に raw_passthrough 蓄積を追加**
8. **`collect_reattach_data` を拡張** (snapshot 連結、reattach 後の visibility 評価)
9. **統合テスト**: SetVisibility(false) → Detached 切替確認、SetVisibility(true) → snapshot enqueue 順序確認 (snapshot が先、reader chunk が後)
10. **`mux-client.test.ts` で `sendSetVisibility` の APC バイト列が daemon の codec で正しく decode できる回路テスト**

**Dependencies**: Requires Phase 1 (`extract_passthrough_sequences` と `RawPassthroughBuffer` を再利用、Phase 1 の visibility.rs から import) and Phase 2 (`MuxClient.sendSetVisibility` 実装済み)

**Testing Approach**:
- Unit (Rust): SetVisibility encode/decode、evaluate_output_target の各組合せ、`collect_reattach_data` の hidden 時挙動
- Integration (Rust): mock socket pair で SetVisibility(false) → Detached 切替、SetVisibility(true) → PtyOutput 受信を tokio test で確認
- Manual: mux モード起動後、blur → focus サイクルで daemon ログに状態遷移が記録されること

**Acceptance Criteria**:
- [ ] `cargo test ... mux::ipc::protocol` で SetVisibility round-trip テストと既存 protocol テスト書き換え版が pass
- [ ] `cargo test ... mux::session::pane` で evaluate_output_target テスト pass (3 分岐 + identity-scoped)
- [ ] `cargo test ... mux::ipc::reattach` で hidden 時 reattach 挙動テスト pass
- [ ] mux 統合テスト: hidden 中は frontend に PtyOutput が送られず、visible 復帰時に snapshot が先頭で届く
- [ ] reattach 時に raw_passthrough が snapshot に連結される

**Estimated Effort**: large

---

### Phase 4: Verification, E2E spec, freeze reproduction

**Goal**: 統合動作の自動検証、新規 E2E spec、既存フリーズ症状の実機再現確認、性能目標値の計測。

**Files to Create**:
- `e2e-tests/specs/visibility-aware-streaming.e2e.js` — 非 mux で blur → focus サイクル後に画面が最新で復帰することを確認する spec
- `e2e-tests/specs/visibility-aware-streaming-mux.e2e.js` — 同等を mux モードで実施
- `scripts/measure-hidden-rss.sh` (任意) — 1 時間 hidden で `/proc/$pid/status` の VmRSS を 1 分間隔で記録するシェルスクリプト

**Files to Modify**:
- 既存 E2E config (`e2e-tests/wdio.docker.conf.js`) — 必要に応じて新規 spec のタイムアウト調整 (180 秒は維持)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| 新規 E2E spec (非 mux) | blur/focus サイクルで snapshot が反映されることを検証 | Docker E2E 環境セットアップ済み | screenshots に最終画面が映る、回帰なし |
| 新規 E2E spec (mux) | 同等を mux モードで実施 | mux daemon を起動できる E2E ハーネス | 全ペインの最終画面が反映される |
| RSS 計測スクリプト | 1 時間 hidden の VmRSS 記録 | Linux 実機 | 増加 < 10 MB を確認 |
| 手動再現確認 | 既存フリーズ症状が解消することを実機で確認 | Linux 実機、本機能適用済みビルド | 10 分 hidden 後の focus 復帰時 UI 即応答 |

**Processing Flow (新規 E2E spec の概要)**:
1. tauri-driver で eMterm 起動
2. PTY に 5 秒間継続出力するコマンドを送る (例: 1 秒間隔で 5 回 echo するループ)
3. **Tauri webview API** で hidden 状態を作る。候補:
   - 案A (推奨): WebDriver `setWindowRect` で window を画面外 (例: x=-9999) に移動して focus を奪う。Linux Xvfb 環境では別の dummy window を `xdotool` で focus させる
   - 案B: `getCurrentWebviewWindow().minimize()` を frontend スクリプト経由で呼ぶ
   - **DOM `visibilityState` の Object.defineProperty 上書きは使わない** (実 WebView では visibilitychange を発火させない、効果が limited)
4. 1500 ms 待機 (debounce 1000ms 以上) + 数百 ms hidden 維持
5. window を visible に戻す (案A: setWindowRect で元位置、案B: `unminimize` + `setFocus`)
6. screenshot を取り、最終出力が表示されていることを画像差分または DOM テキスト読み取りで確認
7. **追加 assertion (CI proxy for freeze symptom)**: hidden 区間中に `invoke("pty_get_send_stats", { sessionId })` を呼び `sent_bytes` の増分が 0 であることを確認 (10 分待機の代替として短時間で freeze 構造の検証になる)

**Processing Flow (フリーズ症状再現確認)**:
1. eMterm を起動 (本機能適用済みビルド)
2. いずれかのタブで継続出力するコマンド (例: 100 ms 間隔で date を出すループ) を起動
3. eMterm から focus を外して別アプリで 10 分以上作業 (デスクトップロックでも可)
4. eMterm に戻った瞬間、UI 操作 (タブ切替、文字入力) が即座に応答することを確認
5. 比較として、本機能撤去ブランチで同じ手順を実施し対比 (任意)
6. 同じ手順を mux モード (複数ペイン稼働) でも実施

**Implementation Steps**:
1. **新規 E2E spec を実装** (非 mux)
2. **mux 版 E2E spec を実装**
3. **`./scripts/run-e2e-docker.sh test visibility-aware-streaming.e2e.js` で確認**
4. **既存 E2E スイート全実行で回帰なし確認**
5. **性能計測 (任意): RSS スクリプト、復帰時 block を `performance.now()` で計測**
6. **実機でフリーズ症状再現手順を実施し記録**

**Dependencies**: Requires Phases 1-3 完了

**Testing Approach**:
- E2E (Docker): 新規 spec 2 本 + 既存スイート全部
- Manual: 実機 freeze 再現確認 + RSS 計測
- Performance: visible 中の throughput を `yes | head -c 100M` 等で計測し ±5% 以内確認

**Acceptance Criteria**:
- [ ] `./scripts/run-e2e-docker.sh test visibility-aware-streaming.e2e.js` が pass
- [ ] `./scripts/run-e2e-docker.sh test visibility-aware-streaming-mux.e2e.js` が pass
- [ ] 既存 E2E スイートが回帰なく全 pass
- [ ] 実機で 10 分 hidden 後の focus 復帰時 UI ブロックが体感ゼロ
- [ ] mux モードでも同様
- [ ] hidden 1 時間後 backend RSS 増加 < 10 MB (計測スクリプト or 任意)

**Estimated Effort**: medium

---

### Phase 5: Diagnostic log cleanup & final pass

**Goal**: 残った診断ログの整理、最終ビルド/テスト確認。

**Files to Modify**:
- `src/terminal-app/pty-handler.ts` — `DIAG-MUX-*` のうち visibility 関連を削除 (本機能で不要になったもの)
- `src/terminal-app/index.ts` — heartbeat 関連診断ログ整理
- `src-tauri/src/reader.rs` — backpressure stalled 診断ログのうち forward anyway 撤去で意味を失った部分を整理
- `src-tauri/src/pty/backpressure.rs` — `record_send_success` の診断 counter は残す (`pty_get_send_stats` で参照される)

**Implementation Steps**:
1. **`grep -rn "DIAG-MUX\|heartbeat-wake\|rAF fallback\|early WASM reinit" src/ src-tauri/`** で残骸を洗い出す
2. **visibility 関連の診断ログのみ削除**、WASM 健全性 / 通常 backpressure の診断は残す
3. **`bun run typecheck` と `cargo check` で警告ゼロ確認**
4. **完全なテストスイート再実行** (Phase 1〜4 のテストを全走らせる)
5. **CLAUDE.md / MEMORY.md の更新は不要** (実装方針自体に変更なし、本機能完了後に SDD update-spec が走る予定)

**Dependencies**: Requires Phases 1-4 動作確認済み

**Testing Approach**:
- Automated: 全 Rust + TS test、typecheck
- Manual: ログ出力を実機で観察し、visibility 関連の DIAG ログが消えていることを確認

**Acceptance Criteria**:
- [ ] `grep -rn "heartbeat-wake\|RAF_FALLBACK\|pty_heartbeat" src/ src-tauri/` が空
- [ ] visibility 関連 `DIAG-MUX-*` が消えている
- [ ] `cargo check` 警告ゼロ、`bun run typecheck` エラーゼロ
- [ ] 全テスト pass

**Estimated Effort**: small

---

## Complete File Structure

```
doc/tasks/visibility-aware-pty-streaming/
├── REQUIREMENTS.md           # 既存
├── SPEC.md                   # 既存 (本 phase で FR13/FR14 確定済み)
├── IMPLEMENTATION.md         # 本ファイル
├── VERIFICATION.md           # 本 phase で生成
├── tasks.yaml                # 本 phase で生成
└── sdd.yaml                  # 既存

src-tauri/src/
├── pty/
│   ├── visibility.rs            # 新規 (Phase 1, HiddenRingBuffer は持たない)
│   ├── passthrough_scanner.rs   # 新規 (Phase 1, stateful)
│   ├── backpressure.rs          # 修正 (Phase 1: MAX_BACKPRESSURE_WAIT 撤去 + ack 駆動化 + set_hidden_wake 追加)
│   ├── manager.rs               # 修正 (Phase 1: visibility registry)
│   └── mod.rs                   # 修正 (新 module 公開)
├── reader.rs                    # 修正 (Phase 1: hidden 経路、heartbeat 撤去)
├── tauri_commands.rs            # 修正 (Phase 1: pty_set_visibility)
├── lib.rs (or main.rs)          # 修正 (Phase 1: invoke handler 登録)
└── mux/
    ├── ipc/
    │   ├── protocol.rs          # 修正 (Phase 3: SetVisibility = 0x1B + 既存 round-trip テスト 2 本書き換え)
    │   ├── handlers.rs          # 修正 (Phase 3: handle_set_visibility)
    │   ├── connection.rs        # 修正 (Phase 3: handle_connection 内 loop ローカル Arc<AtomicBool> visible_state 追加。新規 ConnectionState struct は導入しない)
    │   ├── reattach.rs          # 修正 (Phase 3: snapshot 連結、visibility 評価)
    │   └── pty_spawn.rs         # 修正 (Phase 3: Detached 時 passthrough 蓄積)
    └── session/
        └── pane.rs              # 修正 (Phase 3: raw_passthrough (1 MiB / pane)、passthrough_scanner、evaluate_output_target (identity-scoped))

src/
├── pty/
│   ├── client.ts                # 修正 (Phase 2: setVisibility)
│   └── visibility-controller.ts # 新規 (Phase 2)
├── terminal-app/
│   ├── pty-handler.ts           # 修正 (Phase 2: 対症療法撤去)
│   └── index.ts                 # 修正 (Phase 2: VisibilityController 起動)
└── terminal/mux/
    └── mux-client.ts            # 修正 (Phase 2/3: SetVisibility 送信、type 0x1B)

e2e-tests/specs/
├── visibility-aware-streaming.e2e.js     # 新規 (Phase 4)
└── visibility-aware-streaming-mux.e2e.js # 新規 (Phase 4)

scripts/
└── measure-hidden-rss.sh        # 新規 任意 (Phase 4)
```

## Testing Strategy

- **Unit (Rust)**: visibility.rs / passthrough_scanner.rs / pane.rs / protocol.rs 各構造体に対して 80%+ カバレッジ。状態遷移と境界 (容量超過、空入力、不正シーケンス) を網羅
- **Unit (TypeScript)**: VisibilityController の debounce、MuxClient.sendSetVisibility の APC エンコード、PtyClient.setVisibility の invoke 呼び出し
- **Integration (Rust)**: tokio test で `pty_set_visibility` 経由の状態遷移と Channel 経由 snapshot 配信、mux daemon の SetVisibility ハンドラ
- **E2E (Docker)**: ref docker-e2e-testing skill。新規 spec 2 本 + 既存スイート全部の回帰
- **Manual**: 実機 freeze 再現 (10 分以上 hidden) と RSS 計測

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | — | 既存依存 (vt100, portable-pty, tauri, tokio) のみ使用 |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Linux compositor / Windows で focus event の発火タイミング差異 | High | Medium | `visibilityState` と `onFocusChanged` の OR 条件で吸収。実機で両 OS 確認 |
| reader thread の hidden 判定がレースで誤判定 | Medium | Medium | atomic boolean で lock-free 判定、状態遷移は 1 boolean なので race window 最小 |
| snapshot の Channel 送信失敗時に状態が hidden に戻れない | Low | Medium | warn ログ + `set_visible_and_take_snapshot` 失敗時の状態リセット。次回 visibility 通知で回復 |
| mux で reattach 中に SetVisibility が来て output_target が二重切替 | Medium | Medium | `evaluate_output_target` は output_target lock を取るため atomic。reattach 完了直後の評価で最終整合性 |
| raw_passthrough scanner が複雑な escape 列を取りこぼし | Medium | Low | scanner はステートレスでチャンク跨ぎを諦める方針。再表示は best-effort、SPEC で明示 |
| Phase 1 と Phase 3 の visibility.rs を import するクロス module 依存で循環参照 | Low | Low | `RawPassthroughBuffer` 等を `pty/visibility.rs` に置き、mux 側からは pub re-export 経由で利用 |
| 実機 freeze 再現が phase 4 で不十分 (10 分待機が CI で困難) | High | Medium | 手動チェックリスト形式に倒し、E2E では debounce 越えの短時間再現に留める |

## Open Questions

(verify-plan で全 Open Questions を Decision Log として SPEC に確定済。実装中の細部は以下を参照)

- snapshot 配送経路は SPEC Decision Log の通り「reader thread の Channel を visibility registry が共有保持」案で確定。Channel 参照の登録は `spawn_reader_thread` で `manager.visibility_registry().register_channel(session_id, channel.clone())` を呼ぶ。`pty_set_visibility(visible=true)` ハンドラはこの参照を使って snapshot を送信する。

## Success Metrics

- [ ] FR1〜FR16 を満たし、関連単体/統合テストが全 pass
- [ ] hidden 1 時間継続後の backend RSS 増加 < 10 MB
- [ ] 復帰時 main thread block < 200ms (1 ペイン、`performance.now()` 計測)
- [ ] 既存 E2E スイートが回帰なく pass
- [ ] 実機 freeze 再現手順 (10 分 hidden) で復帰時即応答を確認
- [ ] FR15 撤去対象コードと visibility 関連診断ログが消えている
