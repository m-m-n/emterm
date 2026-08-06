---
title: "mux-connection-input-freeze"
created_date: 2026-08-06
status: draft
---

# mux-connection-input-freeze - 要件定義書

## 1. 概要

### 1.1 背景

先行機能 mux-window-switch-output-hang（main HEAD 1620079）は、同一形状のタスク自己ブロック（task self-block）の実例をソケット層（daemon connection task）とブリッジ stdout 層で未修正のまま残した。その結果、単一の mux bridge 接続内に入力経路のフリーズが残存している。

### 1.2 目的

大量出力（`seq 1 10000000`）を実行中のウィンドウから他のウィンドウへ切り替えたとき、生成側（producer）の完了を待つことなく、切り替え元ウィンドウの描画がフリーズせず、同一接続上の他ウィンドウのキー入力もブロックされない状態にする。

### 1.3 スコープ

対象:

- `src-tauri/src/mux/ipc/connection.rs` の connection task における PTY バッチ drain アーム（現状 665-671 行の `framed.feed(msg).await` / `framed.flush().await`、select! アーム本体内の point-position await）
- `src-tauri/src/mux/bridge.rs` の `daemon_to_stdout` 非同期ブロック（現状 594-621 行の同期 `std::io::stdout().lock()` + `write_all`）

対象外: 9.1 技術的制約（NFR1〜NFR3）に記載。

## 2. ビジネス要件

### 2.1 ビジネス目標

- 単一 mux bridge 接続内に残る入力経路フリーズの解消: 大量出力（`seq 1 10000000`）実行中のウィンドウから切り替えたとき、生成側の完了を待たずに、当該ウィンドウの描画がフリーズせず、かつ同一接続上の他ウィンドウのキー入力もブロックされないこと。
- 先行機能 mux-window-switch-output-hang（main HEAD 1620079）が未修正で残した同一形状のタスク自己ブロックの残存箇所（ソケット層 = daemon connection task、ブリッジ stdout 層）を塞ぐこと。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| mux 利用者 | 単一 mux bridge 接続上で複数ウィンドウを切り替えて操作する eMterm 利用者 |

### 2.3 期待される効果

- 大量出力中のウィンドウから切り替えても、切り替え先で連続したキー入力が可能になる。
- 大量出力中のウィンドウが、同一接続上の他ウィンドウの入力を道連れにしない。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 大量出力中のウィンドウから他ウィンドウへ切り替えて入力する | mux 利用者 | 高 |

### 3.2 ユースケース詳細

#### UC01: 大量出力中のウィンドウから他ウィンドウへ切り替えて入力する

**アクター**: mux 利用者

**事前条件**:
- 単一の mux bridge 接続上に複数の mux ウィンドウが存在する。
- 一方の mux ウィンドウで `seq 1 10000000` が実行中である。

**基本フロー**:
1. `seq 1 10000000` を実行中のウィンドウから、別のウィンドウへ切り替える。
2. 切り替え先のウィンドウでキー入力を行う。
3. `seq` の完了を待つことなく、切り替え先で連続したキー入力が受け付けられる。

**代替フロー**:
- ソケット送信バッファが満杯の状態、および GUI 側 PTY バッファが満杯の状態でも、上記フローが成立する。

**事後条件**:
- `seq` 実行中のウィンドウが、他ウィンドウの入力を低下させていない。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 | ステータス |
|----|--------|------|--------|------------|
| FR1 | daemon connection task: drain アームが select! を飢餓させない | drain アームがタスク全体を停止させないようにする | 高 | resolved |
| FR2 | bridge: tokio タスク上での同期 stdout syscall の排除 | stdout 書き込みでブリッジの tokio タスクをブロックしない | 高 | resolved |
| FR3 | 同一ペイン FIFO 順序の保持（先行機能から継承） | snapshot チャンクと PTY 出力チャンクの順序を保つ | 高 | resolved |
| FR4 | バックプレッシャの保持・非有界チャネルの禁止（先行機能から継承） | メモリ増加を有界に保つ | 高 | resolved |
| FR5 | drain 中の入力ポーリング有界性に対する回帰テスト | drain 飽和時も `framed.next()` が有界遅延でポーリングされることを保証する | 高 | resolved |

### 4.2 機能詳細

#### FR1: daemon connection task: drain アームが select! を飢餓させない

**説明**: `src-tauri/src/mux/ipc/connection.rs` の connection task における PTY バッチ drain アーム（現状 665-671 行の `framed.feed(msg).await` / `framed.flush().await`、select! アーム本体内の point-position await）が、ソケット送信バッファ満杯時にタスク全体を停止させ得る状態を解消する。drain 側の出力が保留中であっても、select! ループはクライアント入力（SwitchWindow、キー入力）の `framed.next()` を有界遅延内でポーリングし続けること。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| タスク自己ブロック | ソケット送信バッファ満杯 | drain 側の保留中も `framed.next()` のポーリングを有界遅延内で継続する |

#### FR2: bridge: tokio タスク上での同期 stdout syscall の排除

**説明**: `src-tauri/src/mux/bridge.rs` の `daemon_to_stdout` 非同期ブロック（現状 594-621 行の同期 `std::io::stdout().lock()` + `write_all`）が、GUI 側 PTY バッファ満杯時にブリッジの tokio ランタイムタスクをブロックしないようにする。ソケット drain 方向は、stdout 書き込みの進捗とは独立して進行し続けること。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| ランタイムタスクのブロック | GUI 側 PTY バッファ満杯 | ソケット drain 方向を stdout 書き込みの進捗から独立させる |

#### FR3: 同一ペイン FIFO 順序の保持（先行機能から継承）

**説明**: 同一ペインの snapshot チャンクと PTY 出力チャンクは FIFO 順序を保つ。また本修正は、既存の deferred-output 経路（`flush_deferred_output` / `arm_pending_deferred_reserve`、connection.rs:707-724。容量が解放される唯一の地点が drain アームである）と整合したままであること。

#### FR4: バックプレッシャの保持・非有界チャネルの禁止（先行機能から継承）

**説明**: 本修正は非有界チャネルの導入によってバックプレッシャを失わせないこと。メモリ増加はエンドツーエンドで有界に保たれること。

#### FR5: drain 中の入力ポーリング有界性に対する回帰テスト

**説明**: daemon connection task の select! が、drain / 出力側が飽和している間も `framed.next()` を有界遅延内でポーリングし続けることをテストで保証する。

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 内容 | ステータス |
|----|------|------------|
| NFR1 | mux プロトコルの変更を行わない（クレジットベースのフロー制御は明示的に別機能とする）。 | resolved |
| NFR2 | GUI 側 `event_tx` の bounded(4096) バッファのサイズ調整はスコープ外とする。 | resolved |
| NFR3 | Windows ブリッジ（`bridge_main_loop_windows`）の同一形状の修正はスコープ外とする。まず Unix でパターンを確立する。 | resolved |
| NFR4 | drain アームにおける既存の PTY 終了時 reap 挙動（配送成否によらず reap する、connection.rs:672-691）と Upgrading フレームの ack 経路（connection.rs:738-747）を退行させない。 | resolved |

### 5.2 パフォーマンス要件

数値目標の指定は本要件には含まれない。スループット退行の確認手段は 12 章のテストシナリオに記載。

### 5.3 セキュリティ要件

該当なし。

### 5.4 可用性要件

該当なし。

### 5.5 保守性要件

該当なし。

### 5.6 互換性要件

- mux プロトコルの変更を行わない（NFR1）。

## 6. UI/UX要件

該当なし。design ステップは skipped。理由: mux daemon connection task およびブリッジ I/O 経路のバックエンド並行性バグ修正であり、UI サーフェスがなく、視覚・操作上の変更が発生しないため。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。mux プロトコルの変更を行わない（NFR1）。

## 9. 制約条件

### 9.1 技術的制約

- mux プロトコルの変更を行わない（クレジットベースのフロー制御は別機能）。
- GUI 側 `event_tx` の bounded(4096) バッファのサイズ調整はスコープ外。
- Windows ブリッジ（`bridge_main_loop_windows`）の同一形状の修正はスコープ外（まず Unix でパターンを確立する）。
- drain アームにおける既存の PTY 終了時 reap 挙動および Upgrading フレームの ack 経路を退行させない。
- 非有界チャネルを導入しない。

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 具体的な修正機構が未確定（plan フェーズの決定事項） | 中 | タスク記述が挙げた候補: (B) `framed.feed/flush` を有界チャネルで供給される別 tokio タスクへ移し、connection task 側は `try_send` のみを行う / (A) bridge の stdout 書き込みを `spawn_blocking` または専用スレッド + `tokio::sync::mpsc` へ移す |
| AC-3 の「有界遅延」の数値が未確定 | 中 | plan / spec 時点で数値化する。テストは既存 mux テスト慣行（test/README.md: 全ての待機に名前付きタイムアウトを与える）に沿って、名前付きの有限タイムアウトをアサートする |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: 一方の mux ウィンドウで `seq 1 10000000` が実行中でも、別ウィンドウへ切り替えると、`seq` の完了を待たずに切り替え先で連続したキー入力が可能である。
- [ ] AC-2: 切り替え後、`seq` を実行中のウィンドウが他ウィンドウの入力を道連れに低下させない。
- [ ] AC-3: daemon connection task の select! が、drain 中であっても `framed.next()` を有界遅延内でポーリングし続けることをテストが保証する。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS1（異常系 / 境界値）: ユニット / 統合テスト（Rust、`--lib` もしくは `src-tauri/tests/`）で connection task の出力側を飽和させ（小容量・満杯のソケットまたはスタブ sink）、受信したクライアントメッセージ（例: SwitchWindow やキー入力）が有界タイムアウト内で処理されることをアサートする。(AC-3 / FR1)
- [ ] TS2（正常系）: 改修後の drain 経路を通して、同一ペインの snapshot → PTY 出力の FIFO 順序が保たれることをテストする。deferred-output のリトライを含む。(FR3)
- [ ] TS3（異常系）: bridge の daemon → stdout の進捗停止がソケット drain を停止させないことをテスト（または対象を絞ったユニットカバレッジ）する。PTY / pipe API が必要な箇所は `#[cfg(all(test, unix))]` で Unix ゲートする。(FR2)
- [ ] TS4（パフォーマンス / 回帰）: `--lib` スイート全体の回帰実行、および `mux_throughput` 統合テストをスループット退行のガードとして実行する（`tabs.rs` の replay テストは `--test-threads=1` が必要な場合がある）。(FR3 / FR4 / NFR4)
- [ ] TS5（正常系・手動）: 元の再現手順（`seq 1 10000000`、ウィンドウ切り替え、切り替え先で入力）をユーザーが手動検証する。E2E インフラは存在しない。(AC-1 / AC-2 / FR1 / FR2)

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| drain アーム | connection.rs の select! 内にある PTY バッチ drain のアーム。deferred-output 経路において容量が解放される唯一の地点 |
| deferred-output 経路 | `flush_deferred_output` / `arm_pending_deferred_reserve`（connection.rs:707-724） |
| タスク自己ブロック | select! アーム本体内の await によりタスク全体が停止し、自身の入力ポーリングが進まなくなる状態 |
| bridge stdout 層 | bridge.rs の `daemon_to_stdout` 非同期ブロック |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 修正対象は connection.rs の drain アーム（B）と bridge.rs の `daemon_to_stdout`（A）の両方であり、いずれも本機能のスコープに含まれる: B が主因、A が連鎖の起点。
- [x] mux プロトコル変更は行わない: クレジットベースのフロー制御は明示的に別機能。
- [x] GUI 側 `event_tx` bounded(4096) のサイズ調整はスコープ外。
- [x] Windows ブリッジ（`bridge_main_loop_windows`）の同一形状修正はスコープ外。まず Unix でパターンを確立する。
- [x] E2E インフラは存在しないため、元の再現手順はユーザーの手動検証で確認する。

### 14.2 未確認・保留事項

- [ ] 具体的な修正機構（候補 A / B）の選択は plan フェーズの決定事項。
- [ ] AC-3 の「有界遅延」の具体値は plan / spec 時点で数値化する。

## 15. 参考資料

- SPEC.md: `feature-docs/mux-connection-input-freeze/SPEC.md`
- 修正対象: `src-tauri/src/mux/ipc/connection.rs`（drain アーム 665-671、PTY 終了 reap 672-691、deferred-output 707-724、Upgrading ack 738-747）
- 修正対象: `src-tauri/src/mux/bridge.rs`（`daemon_to_stdout` 594-621、`bridge_main_loop_windows` はスコープ外）
- 先行機能: mux-window-switch-output-hang（main HEAD 1620079）
- テスト慣行: `test/README.md`（全ての待機に名前付きタイムアウトを与える）
