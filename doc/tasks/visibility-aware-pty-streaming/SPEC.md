# Feature: visibility-aware-pty-streaming

## Overview

frontend が hidden の間は backend (Tauri ローカル PTY セッション) と mux daemon が PTY 出力を内部の shadow parser とリングバッファに保持し、frontend へのデータ送信を停止する。frontend が visible に復帰した時点で最新の画面状態 (cursor / SGR / セル) を再現するスナップショットを 1 回だけ送り、以降は通常の chunk streaming に戻す。復帰スナップショットにはリッチコンテンツ (画像 / Markdown、Kitty Graphics / SIXEL 含む) を連結せず、復帰時にリッチコンテンツは再現しない。

これにより、hidden 期間中の Tauri Channel 流量をゼロにし、`in_flight` の構造的な発散を防ぎ、復帰時のフリーズを排除する。

## Objectives

- hidden 期間中の backend `in_flight` の単調増加を停止する
- 復帰時の main thread block を 200ms 以内に抑える
- mux と非 mux の両方で同一の動作を提供する
- 既存の対症療法 (rAF fallback / heartbeat / forward anyway / early reinit) を撤去する

## User Stories

### US1: 別ウィンドウから eMterm に戻る

As a ターミナル常用ユーザー, I want eMterm に戻った時すぐに最新画面を見て操作再開できる, so that バックグラウンド放置による「復帰時のフリーズ」を体感しない.

**Acceptance Criteria:**
- [ ] hidden → visible 復帰直後にキー入力が遅延なく応答する
- [ ] 表示されるのは最新の画面状態であり、hidden 中の途中経過は経由しない
- [ ] 復帰時 main thread block が 200ms 以内

### US2: mux で複数ペイン稼働中にウィンドウを長時間放置する

As a AI ツール並列利用者, I want mux ペインを並列で動かしたままウィンドウを最小化し時間が経った後復帰する, so that backend メモリが線形に増えずフリーズも起きない.

**Acceptance Criteria:**
- [ ] 1 時間 hidden 後の backend / daemon RSS 増加が 10MB 未満
- [ ] 復帰時に全ペインの最新の画面状態が表示される
- [ ] 復帰後にリッチコンテンツ (画像 / Markdown) は再表示されない

### US3: 短時間ウィンドウ切替を繰り返す

As a ターミナル常用ユーザー, I want 数百 ms 程度のウィンドウクリック離脱で pause/resume が連発しない, so that 通常使用時の挙動が変わらない.

**Acceptance Criteria:**
- [ ] visible → hidden の状態確定にデバウンスがかかる
- [ ] 通常 streaming 中の追加 IPC オーバーヘッドが既存比で有意に増えない

## Technical Requirements

### Functional Requirements

- **FR1 — Visibility 検知と統合判定:** frontend は `document.visibilityState` と rAF heartbeat (visibility-raf-heartbeat FR1〜FR3 参照) を `effective_visible` の判定に使用する。Tauri `getCurrentWebviewWindow().onFocusChanged` は購読するが、その focused 状態は `effective_visible` の判定から除外し、`[DIAG-IDLE]` ログの `reason` 構成 (visibility-raf-heartbeat FR6) のみで参照する観測専用シグナルとする。状態変化はデバウンス (FR5) を通る。
- **FR2 — Backend への通知 (非 mux):** Tauri invoke `pty_set_visibility(session_id: String, visible: bool)` を新設する。frontend は確定した状態を当該セッションに対し通知する。
- **FR3 — Daemon への通知 (mux):** mux IPC に `SetVisibility(visible: bool)` メッセージを追加する。frontend は接続中の daemon に対し通知する。クライアント単位で 1 状態を持つ (ペイン個別ではない)。
- **FR4 — Backend shadow parser (非 mux):** Tauri 側に `SessionVisibilityState` を新設し、各セッションについて `vt100::Parser` shadow と画像/Markdown 用 raw passthrough バッファを持つ。`pty_resize` 時に shadow のスクリーンサイズも同期する。スナップショット復帰は shadow parser snapshot のみで画面状態を再構築する。raw passthrough バッファは hidden 中の drain 用途で保持するが、復帰スナップショットには連結しない。汎用 raw リングバッファ (HiddenRingBuffer) は本機能では作成しない (将来的に diagnostic 用途で追加する場合は別 PR とする)。
- **FR5 — デバウンス制御:** visible → hidden への遷移は `HIDDEN_DEBOUNCE_MS = 1000` 経過後に確定する。hidden → visible への遷移は即座に確定する。閾値未満で visible に戻った場合は確定をキャンセルする。
- **FR6 — Hidden 中の reader 挙動 (非 mux):** `src-tauri/src/reader.rs` の reader thread は hidden 中、PTY からの read を継続するが、`channel.send` を呼ばず shadow parser へ書き込む。画像/Markdown raw passthrough バッファへの書き込みは drain 用途で継続するが、復帰スナップショットには連結しない。`backpressure.add_sent` も呼ばず、`backpressure.wait_for_drain` も skip する。
- **FR7 — Hidden 中の daemon 挙動 (mux):** mux daemon は接続中クライアントが hidden の間、各ペインの `output_target` を `Detached(DetachRingBuffer)` 相当に切り替える。daemon の PTY reader は visible/hidden に関わらず PTY からの read を継続する。
- **FR8 — Visible 復帰時のスナップショット送信 (非 mux):** backend は復帰時に `build_session_snapshot(session_id)` (`b"\x1b[H\x1b[2J"` プレフィックス + shadow parser の `contents_formatted()`) を生成し、Channel へ 1 メッセージで送信する。raw passthrough は復帰スナップショットに連結しない。送信後 raw passthrough を drain (クリア) して次サイクルへ持ち越さない。**ordering 不変式:** snapshot の `Channel.send` は `SessionVisibilityState::inner` mutex 内で完了し、その後に `visible: AtomicBool` を `Release` で `true` にする。reader thread は inner mutex 外で `is_visible()` を lock-free check するため、`visible == true` を観測する時点で snapshot は必ず Channel に enqueue 済み (FIFO 保証) であり、後続の live batch が snapshot より先に届くレースは発生しない。
- **FR9 — Visible 復帰時のスナップショット送信 (mux):** daemon は復帰時に各ペインについて、(1) `pane_output_tx.reserve()` で permit を pane lock 外で取得し、(2) `output_target` の lock を取った状態で `build_shadow_parser_snapshot(shadow_parser)` + ring buffer を連結した `PtyOutputChunk` を取得済み permit 経由で **同期的に** enqueue し、(3) ring buffer / raw passthrough を `clear()` し、(4) `output_target` を `Connected(pane_output_tx)` に戻す、の順で実施する。raw passthrough は復帰スナップショットに連結せず、(3) で drain (clear) して次サイクルへ持ち越さない。`(2)→(4)` を同一 pane lock 内で行うことで、reader thread が同じ pane lock を取って live chunk を `try_send` する経路と排他され、snapshot 送信前に live chunk が enqueue されるレースを防ぐ。permit による事前予約があるため `try_send Full` のフォールバックは発生しない (capacity が live chunk で埋まっていれば `reserve().await` が drain を待つ)。reader thread 側は (4) 後の次回 batch から通常 streaming へ戻る。実装は `mux/session/pane.rs::resume_pane_with_permit` および `mux/ipc/handlers.rs::handle_set_visibility` (visible edge) で行う。
- **FR10 — メモリ上限ポリシー:** 非 mux 側は shadow parser (cols × rows × 32 bytes 程度) + `RawPassthroughBuffer` (`HIDDEN_PASSTHROUGH_CAPACITY = 4 MiB`、drain 用途で保持し復帰スナップショットには連結しない) のみを保持する。汎用 raw リングバッファは持たない。mux 側はペイン毎に既存 `DetachRingBuffer` (`DEFAULT_RING_CAPACITY = 64 MiB`) と新規 `RawPassthroughBuffer` (`HIDDEN_PASSTHROUGH_CAPACITY = 1 MiB`、ペインごと) を持つ。`HIDDEN_PASSTHROUGH_CAPACITY` を非 mux と mux で別値にすることで、mux で複数ペイン同時 hidden の場合でも passthrough によるメモリ増加を NFR3 の per-session 目標 (10 MB 以下) 内に収める (例: 8 ペインで最大 8 MiB)。
- **FR11 — Visibility 状態の `pty_ack` 解釈:** hidden 中は backend で `in_flight` を加算しない (FR6) ため、frontend からの `pty_ack` は単に既存カウンタを減算するのみ。hidden 中の ack は no-op に近い動作となる。
- **FR12 — Backend からの再構築不要保証:** スナップショットは self-contained な ANSI バイト列であり、frontend WASM grid は受信したスナップショットを通常の `process_pty_data` 経路で消費する。frontend 側に専用デコーダを追加しない。snapshot 1 つは reader path / mux pane channel いずれも単一 message として送信し、live chunk と分割混在しない。これにより frontend は snapshot prefix `\x1b[H\x1b[2J` 直後にスクリーン全体を再構築でき、live chunk が snapshot より先に届いて消えるケース (FR8/FR9 の ordering 不変式違反) を frontend 側で recover する必要がない。
- **FR13 — mux 既存 detach との共存 (確定):** daemon に「クライアント (GUI) 単位の visible 状態」を保持する。具体的な保持方式は実装中決定とし、現実の `connection.rs` 構造に合わせて以下のいずれかを採用する:
  - 案A: `connection.rs` の `handle_connection` 内 loop ローカル変数として `let visible = Arc::new(AtomicBool::new(true));` を追加し、`SetVisibility` 受信時と `collect_reattach_data` 呼出時にこの参照を渡す
  - 案B: 新規 `ConnectionState { visible: AtomicBool, ... }` struct を導入し、loop ローカルの `active_session_id` 等を移植する (将来的にクライアント単位の状態を増やす想定がある場合のみ)
  - 推奨は案A (差分最小)。
  - ペインの「buffer 要求」は (a) クライアント切断 (network_detach) または (b) クライアント接続中の hidden のいずれかで成立する。
  - 状態評価関数 `evaluate_output_target(pane: &MuxPane, network_detach: bool, visible: bool, owned_tx: &Sender<PtyOutputChunk>)` を `mux/session/pane.rs` に追加し、(a) いずれかが立てば `Detached(buf)` に切り替え、(b) 両方解消なら `Connected(owned_tx)` に戻す、動作とする。`output_target` の物理表現は既存の `PaneOutputTarget::{Connected, Detached}` 二値を維持する (新規 enum を追加しない)。
  - identity スコープ: Connected → Detached への切替は、現在 Connected の tx が `owned_tx.same_channel(...)` で一致する場合のみ実施する (既存 `detach_session_panes` と同じ基準)。Detached → Connected への切替は、`Detached` が保持する `owner` が `owned_tx.same_channel(...)` で一致するか、`owner = None` (system origin) の場合に限り実施する。owner が異なる場合は Detached のまま維持する (別 connection の hidden pane を SetVisibility(true) で reclaim しない)。`Detached` は `reason: NetworkDetach | HiddenByVisibility | Both` を保持し、`HiddenByVisibility` は `evaluate_output_target(visible=true)` で解消、`NetworkDetach` は reattach 経路でのみ解消する。両 bit が立っている場合は片方の解消では Connected に戻らない。
  - 状態再評価は次の 2 タイミングで行う:
    1. `SetVisibility` 受信時 (`handle_set_visibility`): 接続中の各ペインに対し評価関数を呼ぶ
    2. reattach 完了時 (`collect_reattach_data`): 旧 `network_detach == true` から復帰直後に、最新 `visible` で評価関数を呼ぶ
  - detach 中に hidden が変化しても `output_target` は `Detached` のまま (どっちみち buffer する)。クライアント単位の visible 状態のみ更新する。
  - reattach 時に hidden だった場合: `output_target` を `Detached` のまま維持し、snapshot 送信はせず ring buffer に蓄積継続 (raw passthrough も drain 用途で蓄積継続)。次回 visible 復帰時 (`SetVisibility(true)`) で `resume_pane_with_permit` 経由で snapshot を送る。実装は `collect_reattach_data(visible: bool)` の `visible == false` 分岐で行い、各ペインを `Detached { reason = HiddenByVisibility, owner = Some(pane_output_tx) }` に設定 (既存の `NetworkDetach` bit はクリア、`HiddenByVisibility` bit を立てる)。返却タプルは pane_id と空 `Vec<u8>` のペアで、`send_reattach_data` は `PaneCreated` のみ送信し `PtyOutput` は emit しない。これにより hidden 中の attach で snapshot が frontend に送られて二重描画になるレース (F4) を防ぐ。
- **FR14 — 画像 / Markdown OSC の取り扱い (確定):** `vt100::Parser` は Kitty Graphics Protocol (APC G;...) / SIXEL DCS / OSC 9999 (emterm Markdown) を内部状態として保持しない。
  - hidden 検出時から、reader は PTY 出力バイト列を 2 系統に分岐する:
    1. shadow parser (`vt100::Parser::process`) には常に全バイト投入する (cursor / SGR / 画面状態を正しく更新するため)
    2. 加えて、画像/Markdown OSC を抽出する **stateful** scanner (`PassthroughScanner::process(data) -> Vec<u8>`) を通し、抽出されたバイト列を `RawPassthroughBuffer` (固定容量 `HIDDEN_PASSTHROUGH_CAPACITY`: 非 mux=4 MiB / mux=1 MiB ペインごと、末尾 N バイト保持方式) に append する
  - scanner は **chunk 跨ぎに対応する stateful 実装**とする。state machine で APC G;…ESC \, DCS …q…ESC \, OSC 9999;…ESC \ を追跡し、開始済み未完了 sequence の partial buffer を session/pane ごとに保持する。partial buffer 上限 (`PARTIAL_SEQUENCE_MAX = 16 MiB`) を超えた場合は対象 sequence を放棄し warn ログを出す。state は visible 復帰時に reset しない (chunk 跨ぎは visible 中も発生し得るため、visible 中は scanner を呼ばない設計でもよい — 詳細は実装中決定)
  - 上限超過時 (`RawPassthroughBuffer`): 古いバイト列を head から drop し、末尾容量分を保持する。drop 発生時は `log::warn!` を 1 セッション/ペインあたり 1 回 (visible 復帰時にフラグリセット)
  - 復帰時転送順: `b"\x1b[H\x1b[2J"` + `shadow_parser.screen().contents_formatted()` の順で 1 つの `Vec<u8>` に連結し送信する。raw passthrough は復帰スナップショットに連結しない
  - **役割分担の明示**: shadow snapshot は画面状態 (cursor / SGR / セル) を再構築する。raw passthrough は復帰スナップショットに連結せず、復帰時に drain (clear) して破棄する。リッチコンテンツ (画像 / Markdown) は復帰時に再現しない。mux の既存 `DetachRingBuffer` はネットワーク detach 中の差分配信のみに使う。
  - mux 側でも同じ `RawPassthroughBuffer` (容量 1 MiB) と `PassthroughScanner` をペインごとに持ち、`PaneOutputTarget::Detached` 切替時から `pty_reader_loop` で `extract → append` を実施する
- **FR15 — 既存対症療法の撤去:** 以下を削除する。
  - `src-tauri/src/reader.rs`: `pty_heartbeat` emit、`HEARTBEAT_INTERVAL` 定数、`PtyHeartbeatPayload` (`payloads.rs`)
  - `src-tauri/src/pty/backpressure.rs`: `MAX_BACKPRESSURE_WAIT` 定数を撤去し、`wait_for_drain` の loop 条件を `while in_flight > LOW_WATER_BYTES` に変更する (タイムアウト無し、ack 駆動でのみ wake)。`force_wake` (session removal) と新規 `set_hidden_wake` (visibility hidden 通知時) で確実に reader を起こす経路を保証する。`HIGH_WATER_BYTES` / `LOW_WATER_BYTES` および `wait_for_drain` 自体は visible 中の保護として **維持** する
  - 既存テスト `test_wait_for_drain_returns_quickly_when_under_water` / `test_wait_for_drain_wakes_on_ack` は新方式 (タイムアウト無し ack 駆動) の挙動に合わせて書き換える。`MAX_BACKPRESSURE_WAIT` を参照していた箇所はすべて削除
  - `src/terminal-app/pty-handler.ts`: `RAF_FALLBACK_MS` および setTimeout fallback、`pty_heartbeat` listener、`heartbeatWakeWarnedDuringStall` 関連、`healthCheck` 内の early WASM reinit ブロック、`onVisibilityChange` 内の drain trigger (本機能の通知に置き換え)
  - 関連診断ログ (`heartbeat-wake`, `rAF fallback fired`, `early WASM reinit`)。`DIAG-MUX-*` のうち削除対象は `DIAG-MUX-HEARTBEAT` のみ。`DIAG-IDLE` (visibility/focus transition) は本機能の `VisibilityController` から再出力するため移行する
- **FR16 — 既存対症療法の維持:** 以下は本機能と独立した目的のため残す。
  - `src/terminal-app/pty-handler.ts` 内 `onFocusChanged` の WASM 健全性プローブ (suspend 復帰後の memory corruption 検知)
  - `tryRecoverFromWasmCrash` 全般 (WASM 障害復旧)

### Non-Functional Requirements

- **NFR1 — Performance (visible 中):** visible 中の通常 PTY streaming スループットを既存比で低下させない。`pty_set_visibility` の呼び出し頻度は 1 秒あたり最大 1 回。
- **NFR2 — Performance (復帰時):** 復帰時の `build_session_snapshot` 生成 + IPC 送信 + frontend WASM 処理の合計を 200ms 以内とする (1 ペインあたり)。
- **NFR3 — Memory:** 追加メモリ目標
  - 非 mux: 1 セッションあたり shadow parser (おおよそ `cols × rows × 32 bytes` 程度) + `HIDDEN_PASSTHROUGH_CAPACITY` (4 MiB) のみ
  - mux: 1 ペインあたり既存 `DetachRingBuffer` (64 MiB、本機能で容量変更なし) + 新規 `HIDDEN_PASSTHROUGH_CAPACITY` (1 MiB) + `PassthroughScanner` partial buffer (最大 16 MiB だが通常は数 KB)
  - 1 時間 hidden 後の RSS 増加目標 < 10 MB は「非 mux で 1 session、または mux で 1〜2 ペイン active」想定の値。多ペイン環境では各ペインの passthrough cap × ペイン数の上限を見積もる。
- **NFR4 — Compatibility:** Linux (WebKitGTK) / Windows (WebView2) の両プラットフォームで動作する。
- **NFR5 — Reliability:** visibility 通知が一時的に失われた場合でも、次回の visibility イベントで状態が回復する。具体的には、frontend は visibility 状態を必ず最新値で送信し直すヘルスチェック (10 秒間隔) を持つ。
- **NFR6 — Observability:** スナップショット送信、raw passthrough drop、`PassthroughScanner` partial buffer 上限超過は `console.warn` / `log::warn!` で記録する。通常の visibility 状態遷移 (visible↔hidden) は `console.debug` / `log::debug!` で記録する (frequent transitions が warn ノイズにならないように)。

## Implementation Approach

### Architecture

**System Architecture (非 mux PTY):**
```
┌────────────────────────────────────┐
│ Frontend (WebView)                 │
│  visibility + rAF heartbeat        │
│   → debounce → invoke              │
│      pty_set_visibility            │
│  (focus は観測専用シグナル)        │
│  WASM grid ← Channel onmessage     │
└────────────────┬───────────────────┘
                 │ Tauri IPC
┌────────────────▼───────────────────┐
│ Backend (Rust)                     │
│  PtyManager                        │
│   ├─ SessionBackpressure           │
│   │    (HIGH_WATER/LOW_WATER 維持) │
│   ├─ SessionVisibilityState (NEW)  │
│   │    visible: AtomicBool         │
│   │    shadow: vt100::Parser       │
│   │    scanner: PassthroughScanner │
│   │    raw_passthrough:            │
│   │       RawPassthroughBuffer (4MiB) │
│   └─ Reader thread                 │
│        if visible → channel.send   │
│        else      → shadow.process  │
│                  + scanner.process │
│                  + raw.append      │
└────────────────────────────────────┘
```

**System Architecture (mux):**
```
┌────────────────────────────────────┐
│ Frontend (WebView)                 │
│  visibility → APC SetVisibility    │
└────────────────┬───────────────────┘
                 │ APC over PTY
┌────────────────▼───────────────────┐
│ Bridge process                     │
│  APC ↔ MuxMessage                  │
└────────────────┬───────────────────┘
                 │ Unix socket / pipe
┌────────────────▼───────────────────┐
│ Mux Daemon                         │
│  Per-connection state (loop scope) │
│    visible: Arc<AtomicBool> (NEW)  │
│    network_detach: bool (existing) │
│  PerPane:                          │
│    output_target:                  │
│      Connected(tx) | Detached(buf) │
│    shadow_parser: vt100::Parser    │
│    scanner: PassthroughScanner(NEW)│
│    raw_passthrough:                │
│       RawPassthroughBuffer(1MiB)   │
│  evaluate_output_target() runs on: │
│   - SetVisibility 受信時           │
│   - reattach 完了時                │
└────────────────────────────────────┘
```

### Data Flow

**Hidden への遷移 (非 mux):**
```
document.hidden または rAF stall (>5s)
  → frontend debounce (1000ms)
  → pty_set_visibility(session_id, false)
  → SessionVisibilityState.visible = false
  → backpressure.wake() で wait_for_drain 中の reader を起こす
  → reader thread の次回 batch から
     channel.send / add_sent / wait_for_drain をスキップ
     shadow.process(batch)
     + scanner.process(batch) → raw_passthrough.append
```

**Visible 復帰 (非 mux):**
```
document.visible または rAF resume → frontend (即座に)
  → pty_set_visibility(session_id, true)
  → SessionVisibilityState.set_visible_and_take_snapshot()
       snapshot = b"\x1b[H\x1b[2J" + parser.screen().contents_formatted()
       raw_passthrough.clear() / drop_warned リセット (連結しない / drain)
  → command が channel.send(snapshot) を 1 回呼ぶ
  → 以降通常 streaming
```

**Hidden への遷移 (mux):**
```
document.hidden または rAF stall (>5s)
  → frontend debounce (1000ms)
  → APC: SetVisibility(false) → bridge → daemon
  → connection-scope visible = false
  → 各ペインに対し evaluate_output_target(network_detach=false, visible=false)
     → Detached(DetachRingBuffer) に swap (identity-scoped)
  → pty_reader_loop は Detached 経路を通り、
     既存 ring.write(data) に加え scanner.process(data) → pane.raw_passthrough.append
```

**Visible 復帰 (mux):**
```
document.visible または rAF resume → frontend (即座に)
  → APC: SetVisibility(true) → bridge → daemon
  → connection-scope visible = true
  → 各ペインに対し pane.output_target lock を取り:
       (1) snapshot = build_shadow_parser_snapshot(shadow_parser)
           pane_output_tx.send(PtyOutput { snapshot }) を実施
           (raw_passthrough は連結しない)
       (2) raw_passthrough.clear() (drain)
       (3) output_target = Connected(pane_output_tx)
  (3) より前に (1) を確実に enqueue することで
      新規 chunk が snapshot より先に流れるレースを防ぐ
```

**Reattach (network detach 解除) 時の visibility 評価:**
```
新クライアント接続 → collect_reattach_data
  → 各ペインに対し evaluate_output_target(
       network_detach=false (今 reattach した),
       visible=ConnectionState.visible (新規接続時は true 既定))
  → visible なら Connected(tx) に swap + snapshot 送信
  → hidden なら Detached のまま維持 + snapshot 未送信
```

### API Design

#### Tauri Command: pty_set_visibility (新規)

**Signature:**
- `pty_set_visibility(state: PtyManager, session_id: String, visible: bool)`

**動作:**
- `visible == true`: `SessionVisibilityState.set_visible_and_flush_snapshot()` を呼び、shadow snapshot を Channel に送信する (raw passthrough は連結せず drain する)
- `visible == false`: `SessionVisibilityState.set_hidden()` を呼び、以降 reader が shadow パスへ流れる
- 該当セッションが存在しない場合は no-op

#### Mux Message: SetVisibility (新規)

```
// src-tauri/src/mux/ipc/protocol.rs
MessageType::SetVisibility = 0x1B
  (0x1A は MoveWindow に使用済み、次の空き番号 0x1B を採用)

SetVisibilityMessage {
    visible: bool
}
```

**Wire format:** `length (u32 LE) | type (u8 = 0x1B) | pane_id (u32 LE, 0=session-wide) | payload (1 byte: visible 0|1)`

**配信方向:** GUI → Daemon。応答は不要 (片方向通知)。状態反映は次の reader batch から有効。

#### Backend 内部 API

`src-tauri/src/pty/visibility.rs` (新規) で以下を公開する。

**SessionVisibilityState** — セッションごとの hidden 状態と shadow ステート所有者
- `new(cols: u16, rows: u16) -> Self` — 初期状態は visible
- `is_visible() -> bool` — lock-free 読み取り
- `set_hidden()` — visible フラグを false に降ろす
- `set_visible_and_take_snapshot() -> Option<Vec<u8>>` — visible に戻し、未送信 snapshot bytes を返す。既に visible なら None
- `process_hidden(data: &[u8])` — shadow parser に流し、画像/Markdown を抽出して raw passthrough に append
- `resize(cols: u16, rows: u16)` — shadow parser サイズ更新

**RawPassthroughBuffer** — 末尾 N バイト保持方式の固定容量 buffer
- `new(capacity: usize) -> Self`
- `append(data: &[u8]) -> bool` — 戻り値は drop が発生したか
- `read_all() -> Vec<u8>`
- `clear()`

**PassthroughScanner** — chunk 跨ぎに対応する stateful state machine
- `new() -> Self`
- `process(&mut self, data: &[u8]) -> Vec<u8>` — 新規データを state machine に通し、完成した APC G / DCS q / OSC 9999 sequence を抽出して返す
- `partial_buffer_len() -> usize` — diagnostic 用: 開始済み未完了 sequence の保持バイト数
- 内部上限 `PARTIAL_SEQUENCE_MAX = 16 MiB` を超えたら対象 sequence を放棄し warn ログを 1 回出す

**定数:**
- 非 mux `HIDDEN_PASSTHROUGH_CAPACITY: usize = 4 * 1024 * 1024` (4 MiB)
- mux `HIDDEN_PASSTHROUGH_CAPACITY: usize = 1 * 1024 * 1024` (1 MiB / ペイン)
- `PARTIAL_SEQUENCE_MAX: usize = 16 * 1024 * 1024` (16 MiB)
- `HIDDEN_DEBOUNCE_MS: u64 = 1000` (frontend 側で適用)

**画像/Markdown 抽出 scanner** — `src-tauri/src/pty/passthrough_scanner.rs` (新規)
- `PassthroughScanner` struct (chunk 跨ぎ対応 stateful)
- `process(&mut self, data: &[u8]) -> Vec<u8>` — Kitty APC G;...ST、SIXEL DCS ...q...ST、OSC 9999;...ST を state machine で追跡し、完成した sequence を連結して返す
- partial buffer 上限 `PARTIAL_SEQUENCE_MAX` を超えたら対象 sequence を放棄

#### Frontend 内部 API

`src/pty/visibility-controller.ts` (新規)

**VisibilityController** — visibility 判定 + デバウンス + 通知 (focus は観測専用)
- `constructor(ptyClient, muxClient | null)`
- `start()` — `visibilitychange` / `onFocusChanged` listener 登録 (focused は診断ログ専用) + 10 秒間隔のヘルスチェックタイマ起動
- `stop()` — listener 解除、タイマ停止
- 内部状態: 直近確定状態 (visible | hidden)、未確定の hide 候補のタイマハンドル
- 確定したら `ptyClient.setVisibility()` および `muxClient?.sendSetVisibility()` を呼ぶ

`src/pty/client.ts` 追加メソッド
- `setVisibility(visible: boolean): Promise<void>` — `pty_set_visibility` を invoke

`src/terminal/mux/mux-client.ts` 追加メソッド
- `sendSetVisibility(visible: boolean): void` — `MuxMessageType.SetVisibility` (0x1B) を APC で送信

### Database Schema

該当なし (in-memory state のみ)。

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/pty/backpressure.rs`: `add_sent` 呼び出しの hidden 中スキップ、forward anyway 経路撤去
- `src-tauri/src/reader.rs`: hidden 状態判定と shadow parser 経路追加、heartbeat 撤去
- `src-tauri/src/tauri_commands.rs`: `pty_set_visibility` コマンド追加
- `src-tauri/src/pty/visibility.rs`: 新規 `SessionVisibilityState`、`HiddenRingBuffer`、`RawPassthroughBuffer`、定数
- `src-tauri/src/pty/passthrough_scanner.rs`: 新規 画像/Markdown OSC 抽出
- `src-tauri/src/pty/manager.rs`: visibility registry の保持 (`Arc<RwLock<HashMap<SessionId, Arc<SessionVisibilityState>>>>`)
- `src-tauri/src/mux/ipc/protocol.rs`: `SetVisibility = 0x1B` MessageType 追加、`SetVisibilityMessage` payload 定義
- `src-tauri/src/mux/ipc/handlers.rs`: `handle_set_visibility` ハンドラ追加 (route_message 経由)
- `src-tauri/src/mux/ipc/connection.rs`: `ConnectionState.visible` 追加、SetVisibility メッセージのルーティング
- `src-tauri/src/mux/session/pane.rs`: `evaluate_output_target` 関数追加、`raw_passthrough` フィールド追加
- `src-tauri/src/mux/ipc/reattach.rs`: `collect_reattach_data` で `evaluate_output_target` を呼ぶ (snapshot に raw_passthrough は連結しない)
- `src/pty/client.ts`: `setVisibility(visible: boolean)` メソッド追加
- `src/pty/visibility-controller.ts`: 新規
- `src/terminal/mux/mux-client.ts`: `sendSetVisibility` メソッド追加、`MuxMessageType.SetVisibility = 0x1B` 追加
- `src/terminal-app/pty-handler.ts`: 対症療法撤去、`VisibilityController` 統合
- `src/terminal-app/index.ts`: `VisibilityController` 起動

**External Dependencies:**
- `vt100 = "0.15"`: shadow parser (既存依存)
- `portable-pty`: PTY (既存依存)
- Tauri API `getCurrentWebviewWindow().onFocusChanged`: focus 検知 (既存使用)

### File Structure

```
src-tauri/src/
├── pty/
│   ├── backpressure.rs        # 修正: forward anyway 経路撤去、hidden 中 skip
│   ├── visibility.rs          # 新規: SessionVisibilityState / RawPassthroughBuffer / 定数 (HiddenRingBuffer は廃止)
│   ├── passthrough_scanner.rs # 新規: PassthroughScanner (stateful, chunk 跨ぎ対応)
│   ├── manager.rs             # 修正: visibility registry の保持
│   └── ...
├── reader.rs                  # 修正: hidden 中 shadow 経路、heartbeat 撤去
├── tauri_commands.rs          # 修正: pty_set_visibility 追加
└── mux/
    ├── ipc/
    │   ├── protocol.rs        # 修正: SetVisibility = 0x1B 追加、payload 定義
    │   ├── handlers.rs        # 修正: handle_set_visibility ハンドラ
    │   ├── connection.rs      # 修正: ConnectionState.visible 追加、route 拡張
    │   └── reattach.rs        # 修正: evaluate_output_target (snapshot に passthrough は連結しない)
    └── session/
        └── pane.rs            # 修正: evaluate_output_target、raw_passthrough フィールド

src/
├── pty/
│   ├── client.ts              # 修正: setVisibility メソッド
│   └── visibility-controller.ts  # 新規
├── terminal-app/
│   ├── pty-handler.ts         # 修正: 対症療法撤去
│   └── index.ts               # 修正: VisibilityController 統合
└── terminal/mux/
    └── mux-client.ts          # 修正: SetVisibility 送信、MessageType 追加
```

## Test Scenarios

### Unit Tests

- [ ] `SessionVisibilityState::set_hidden` 後、`process_hidden` で渡したデータが shadow parser と raw passthrough (画像/Markdown 含むデータ) に反映される
- [ ] `SessionVisibilityState::set_visible_and_take_snapshot` がスナップショットを返し、raw_passthrough がクリアされる
- [ ] `RawPassthroughBuffer` が容量上限を超えると古いデータが破棄され、末尾 N バイトが保持される
- [ ] `RawPassthroughBuffer` の drop 発生時 1 回のみ warn ログが出る (連続 append でログ重複しない)
- [ ] `SessionVisibilityState::resize` で shadow parser のスクリーンサイズが更新される
- [ ] mux `SetVisibility` メッセージのエンコード / デコード往復一致
- [ ] mux `MessageType::from_u8` が 0x1B で `Some(SetVisibility)` を返し、未使用 opcode `0x1C` 以降で `None` を返す (既存テスト書き換え)
- [ ] frontend `VisibilityController` のデバウンス: visible → hidden は 1000ms 後確定、hidden → visible は即時
- [ ] 1000ms 内の visible → hidden → visible 連続トグルでバックエンド通知が発生しない
- [ ] `PassthroughScanner::process` が Kitty APC G、SIXEL DCS q、OSC 9999 を正しく抽出する
- [ ] `PassthroughScanner::process` が **chunk を分割** した場合 (例: APC G の途中で chunk 境界) でも完成 sequence を抽出する
- [ ] `PassthroughScanner` が通常テキスト + ANSI CSI のみのバイト列で空 Vec を返す
- [ ] `PassthroughScanner` の partial buffer が `PARTIAL_SEQUENCE_MAX` を超えると対象 sequence を放棄し warn ログを出す
- [ ] `evaluate_output_target` が (network_detach=true, visible=true) で Detached を維持する
- [ ] `evaluate_output_target` が (network_detach=false, visible=false) で Detached に切り替える (identity-scoped)
- [ ] `evaluate_output_target` が (network_detach=false, visible=true) で Connected に戻す
- [ ] mux visible 復帰時に snapshot 送信 → output_target 切替の順序が保たれる (lock test or sequence assertion)
- [ ] backpressure `wait_for_drain` がタイムアウト無し (ack 駆動) で wake する。`set_hidden_wake` 等の visibility hidden 通知で wake する

### Integration Tests

- [ ] 非 mux: hidden 状態で 10MB 分の PTY 出力を流し込んでも `pty_get_send_stats` の `sent_bytes` が増えない
- [ ] 非 mux: visible 復帰時に shadow snapshot が 1 メッセージで届き、frontend WASM grid が最新画面と一致する
- [ ] 非 mux: hidden 中に Kitty 画像 sequence を流し込み、復帰スナップショットに raw passthrough (画像) が連結されない (復帰後に再表示されない)
- [ ] mux: hidden 中に各ペインの `output_target` が `Detached` 相当となり、ring buffer に蓄積される
- [ ] mux: visible 復帰時に各ペインの `build_shadow_parser_snapshot` が送られ、raw passthrough は連結されず drain される
- [ ] mux: detach (network) 中に SetVisibility を受けても output_target は Detached のまま、ConnectionState.visible だけ更新される

### E2E Tests

**Existing E2E tests**: `e2e-tests/specs/*.e2e.js` (`./scripts/run-e2e-docker.sh test`)
**Run command**: `./scripts/run-e2e-docker.sh test`

- [ ] 既存 E2E が回帰なくパスする
- [ ] 新規 spec: 非 mux で **Tauri webview API** 経由で minimize/restore (もしくは tauri-driver の WebDriver `setWindowRect` で off-screen に移動) を行い、復帰後に PTY 出力が最新表示される。`document.visibilityState` の DOM property 上書きには依存しない (read-only のため)
- [ ] 新規 spec: mux で同手順を実施し、各ペインの shadow snapshot が反映される (raw passthrough は連結されない)
- [ ] 新規 spec: hidden 中 (1〜3 秒で十分) の `pty_get_send_stats.sent_bytes` が増えないことを assert する (10 分待ちの freeze 再現の CI 代替手段)

### Edge Cases

- [ ] hidden 中に `pty_resize` が呼ばれた場合、shadow parser のスクリーンサイズが追従する
- [ ] hidden 中に PTY が exit した場合、visible 復帰時に exit イベントが届き、最終画面状態が表示される
- [ ] hidden → visible の遷移中に backend がスナップショット送信に失敗した場合、次回 PTY 出力時に通常 streaming で再同期される
- [ ] CLI 画像表示 (Kitty Graphics Protocol) が hidden 中に送られても、復帰スナップショットに連結されず復帰後に再表示されない
- [ ] CLI Markdown 表示 (OSC 9999) が hidden 中に送られても、復帰スナップショットに連結されず復帰後に再表示されない
- [ ] raw passthrough が容量を超えた場合、末尾 4 MiB が保持され古いものから drop され、警告ログが 1 回出力される (drain 用途のバッファ動作)
- [ ] mux で「クライアント切断による detach」と「hidden による soft detach」が同時に発生した場合、`evaluate_output_target` により detach が解消されるまで Detached を維持し、reattach 時に hidden 状態を再評価する
- [ ] frontend からの visibility 通知が IPC エラーで失われた場合、10 秒ごとのヘルスチェック再送 (NFR5) で次回回復する

### Performance Tests

- [ ] visible 中の通常 streaming スループット (`yes | head -c 100M` など) で本機能適用前後の throughput が ±5% 以内
- [ ] hidden 1 時間継続後、backend RSS 増加が 10MB 未満
- [ ] visible 復帰時の main thread block (`performance.now()` 計測) が 200ms 以内 (1 ペイン)

### Manual Reproduction (Existing Freeze Symptom)

実装中、本機能が既存のフリーズ症状を実際に解消することを実機で確認する。

- [ ] 起動した eMterm でいずれかのタブにて `while true; do date; sleep 0.1; done` 等の継続出力を流す
- [ ] eMterm から focus を外し、別アプリで 10 分以上作業を続ける (デスクトップロックでも可)
- [ ] eMterm に focus を戻したとき、UI 操作が即座に応答することを確認する (本機能適用前は数秒〜数十秒のフリーズ)
- [ ] 同じ手順を mux モード (複数ペイン稼働) でも実施する

## Security Considerations

- visibility 状態は trust boundary を跨がない (frontend と backend は同一ユーザー権限)
- `pty_set_visibility` は session_id を引数に取るが、PtyManager の既存 session 検索を流用するため不正 session_id は no-op
- mux `SetVisibility` メッセージは既存の APC 経路を使うため追加の入力検証は既存 codec が担う

## Error Handling

### Error Categories

| カテゴリ | 発生条件 | 対応 |
|---------|---------|------|
| `pty_set_visibility` で session_id 不一致 | セッション削除済み | no-op (warn ログ) |
| shadow parser パニック (vt100 crate 内部) | 不正バイト列で稀に発生 | catch_unwind で包み、状態リセット |
| ring buffer write 中の OOM | 通常発生しない (固定容量) | panic — backend クラッシュ |
| Channel.send 失敗 (visible 復帰時のスナップショット送信) | frontend 切断 | warn ログ、状態は維持 |
| mux `SetVisibility` codec エラー | プロトコル不整合 | bridge 既存エラーハンドラに従う |
| raw_passthrough 容量超過 | 長時間 hidden + 大量画像 | head から drop、warn 1 回 |

## Performance Optimization

### Performance Goals

- visible 中の追加 IPC オーバーヘッド: 1 invoke / 秒未満
- 復帰時の snapshot 生成 (`contents_formatted`): < 50ms (1 ペイン)
- 復帰時の frontend WASM 処理: < 150ms (1 スナップショット)
- 合計復帰時 block: < 200ms

### Optimization Strategies

- shadow parser へのバイト供給は reader thread のメインバッチと同じ呼び出しコンテキストで行い、追加スレッド切替を避ける
- リングバッファは固定容量 `Vec<u8>` で alloc を回避する
- visibility 状態は `AtomicBool` で読み取りを lock-free にする
- 復帰時 snapshot は 1 メッセージにまとめ、frontend WASM の `process_pty_data` を 1 回で完了させる

### Caching Strategy

- shadow parser 自体が screen state のキャッシュ
- 復帰時にリングバッファ raw 内容は破棄し、shadow snapshot のみを真実とする
- 画像 / Markdown OSC は raw passthrough バッファに保持するが、復帰スナップショットには連結せず復帰時に drain して破棄する

## Success Criteria

- [ ] FR1〜FR16 が実装され、テストが pass する
- [ ] NFR1〜NFR6 を満たす
- [ ] hidden 1 時間継続後の backend RSS 増加 < 10MB
- [ ] 復帰時 main thread block < 200ms
- [ ] mux / 非 mux の両方で `pty_get_send_stats.sent_bytes` が hidden 中に増加しない
- [ ] 既存 E2E テストが回帰なく pass する
- [ ] 既存対症療法 (FR15) が削除され、関連診断ログが消える
- [ ] CLI 画像 / Markdown 表示は hidden 跨ぎで復帰後に再表示されない (復帰スナップショットに連結されない)

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理する。

(現在 Open Question なし。FR13/FR14 は確定済み)

## Internal Diagnostic API

`pty_get_send_stats(session_id) -> (i64, u64)` は backend に残す diagnostic-only Tauri command。frontend からは呼ばない (FR15 撤去済) が、E2E spec (TS-29 等) と将来の手動デバッグから `__TAURI_INTERNALS__.invoke` 経由で reader thread の `channel.send` 統計を観測するために存続する。実装は `SessionBackpressure::{sent_count, sent_bytes, record_send_success}` (Atomic counter)。

## Decision Log (verify-plan で確定したもの)

- 非 mux で汎用 raw リングバッファ (`HiddenRingBuffer`) は **持たない**。snapshot は shadow parser snapshot のみで構築する。raw passthrough は復帰スナップショットに連結しない (FR4 / FR8 / FR10 改訂)。
- `PassthroughScanner` は **stateful** (chunk 跨ぎ対応) とする。stateless 案は画像/Markdown が chunk 境界で欠落する問題を解消できないため不採用 (FR14 改訂)。
- mux 側 `RawPassthroughBuffer` の容量は per-pane 1 MiB (非 mux は 4 MiB) とする。NFR3 のメモリ目標を multi-pane でも維持するため (FR10 / FR14 改訂)。
- `wait_for_drain` は visible 中の HIGH_WATER 保護として残し、`MAX_BACKPRESSURE_WAIT` を撤去して ack 駆動の無期待ちに変更する。hidden 通知時は `set_hidden_wake` で確実に wake させる (FR15 改訂)。
- daemon 側のクライアント単位 visible 状態は `connection.rs` の `handle_connection` 内で `Arc<AtomicBool>` を loop ローカルに持つ案 A を推奨 (新規 `ConnectionState` struct 導入は不要) (FR13 改訂)。
- mux visible 復帰時の snapshot 送信と `output_target` 切替は同一 pane lock 内で「snapshot enqueue → clear → Connected 切替」の順で実行し、レースを排除する (FR9 改訂)。
- 状態遷移ログは `debug` レベル、drop / send 失敗 / scanner overflow は `warn` レベルとする (NFR6 改訂)。
- 復帰時のリッチコンテンツ (画像 / Markdown / Kitty / SIXEL) 再現を廃止する。raw passthrough は復帰スナップショットに連結しない (drain して破棄する)。mux 側 (reattach.rs / pane.rs / handlers.rs) はコード実装済み。非 mux 側 (reader.rs / SessionVisibilityState) はコード追従が後続で必要 (Overview / US2 / FR4 / FR6 / FR8 / FR9 / FR10 / FR13 / FR14 改訂)。

## Implementation Phases

### Phase 1: Backend foundation (非 mux)

**Goals:** 非 mux PTY セッションで visibility-aware streaming を実現する

**Deliverables:**
- `src-tauri/src/pty/visibility.rs` 新規作成
- `src-tauri/src/pty/passthrough_scanner.rs` 新規作成
- `pty_set_visibility` Tauri command 追加
- `reader.rs` の hidden 経路実装
- `pty/backpressure.rs` の forward anyway 撤去
- 単体テスト

### Phase 2: Frontend integration

**Goals:** frontend で VisibilityController を起動し、debounce を含む通知経路を完成する

**Deliverables:**
- `src/pty/visibility-controller.ts` 新規作成
- `src/pty/client.ts` に `setVisibility` 追加
- `pty-handler.ts` の対症療法 (FR15 対象) 撤去
- 単体テスト

### Phase 3: Mux daemon support

**Goals:** mux daemon に SetVisibility と soft detach 機構を追加する

**Deliverables:**
- mux IPC protocol 拡張 (SetVisibility = 0x1B)
- daemon 側 ConnectionState に visible 追加
- ペインの evaluate_output_target ロジック
- raw_passthrough フィールド追加 (drain 用途、snapshot には連結しない)
- 単体テスト

### Phase 4: Verification

**Goals:** 統合動作と性能を検証する

**Deliverables:**
- 新規 E2E spec
- 1 時間 hidden の RSS 計測スクリプト
- 復帰時 block 時間計測
- 既存フリーズ症状の手動再現確認

## References

- `doc/tasks/visibility-aware-pty-streaming/REQUIREMENTS.md`: 要件定義書
- `doc/tasks/visibility-render-recovery/SPEC.md`: 既存の visibility/focus 検知実装
- `doc/tasks/mux-output-throughput/SPEC.md`: mux daemon 出力経路の既存仕様
- `src-tauri/src/mux/ipc/reattach.rs`: `build_shadow_parser_snapshot` 既存実装
- `src-tauri/src/mux/session/pane.rs`: ペインの `output_target` (`Connected | Detached`) 既存実装
- `src-tauri/src/pty/backpressure.rs`: backpressure 既存実装
- `src-tauri/src/reader.rs`: PTY reader thread 既存実装
- `src/terminal-app/pty-handler.ts`: 対症療法群の現状実装
