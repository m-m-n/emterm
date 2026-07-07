---
title: "mux snapshot デバイスクエリ除去"
created_date: 2026-07-07
status: draft
---

# mux snapshot デバイスクエリ除去 - 要件定義書

## 1. 概要

### 1.1 背景

mux セッションで zsh プロンプトのタブを detach → attach すると、プロンプトに `65;1;4;22c` が 100% 挿入されるバグがある。

原因の連鎖:

1. タブ起動時に何らかのプログラム（zsh プラグイン等）が DA1 クエリ `ESC[c` を発行し、そのバイトが daemon の scrollback ring に記録される
2. attach 時、daemon は `collect_reattach_data`（`src-tauri/src/mux/ipc/reattach.rs`）で snapshot を組み立て、`send_reattach_data` が `MessageType::PtyOutput` フレームとして送る
3. GUI 側の PtyOutput 経路 `apply_active_pane_output`（`src-tauri/src/tabs.rs`）は、PSReadLine のライブ CPR クエリに応答するため `take_response` → `write_device_response` で応答を PTY に書き戻す設計になっている
4. snapshot に焼き込まれた過去の `ESC[c` にも応答 `ESC[?65;1;4;22c` を生成して daemon → PTY → zsh に届けてしまい、zle が `ESC[?` を食った残りの `65;1;4;22c` がプロンプトに挿入される

既存の防御（`reset_frame_for_replay` / `build_from_snapshot` の response buffer 破棄）は `Snapshot`/`SnapshotRestore` メッセージ経路（window 切り替え）専用であり、PtyOutput フレームで届く reattach snapshot には効かない。scrollback フィルタ `strip_replayable_rich_content`（`src-tauri/src/mux/scrollback_filter.rs`）はビューア系シーケンスのみ除去し、CSI 系デバイスクエリは素通しする。

### 1.2 目的

daemon 側の snapshot 組み立て時に、応答を生成する CSI デバイスクエリを scrollback バイトから除去し、detach → attach で `65;1;4;22c` 等の応答残骸がプロンプトに挿入されないようにする。

### 1.3 スコープ

- 対象: `src-tauri/src/mux/scrollback_filter.rs` の `strip_replayable_rich_content`（および必要に応じて同モジュール内の補助関数）
- 同フィルタは `build_snapshot_bytes`（reattach / on-demand snapshot）と `pty_spawn.rs` の resume 経路から呼ばれており、拡張はそれら全経路に自動的に効く
- 対象外: GUI 側（`tabs.rs` / `term_core`）の変更、mux プロトコル（MessageType）の変更

## 2. ビジネス要件

### 2.1 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm mux 利用者 | detach / attach、window 切り替え、visibility resume を行うユーザー |

### 2.2 期待される効果

- detach → attach 後のプロンプトにデバイスクエリ応答の残骸（`65;1;4;22c` 等）が挿入されなくなる

## 3. ユースケース

### UC01: detach → attach

**アクター**: mux 利用者

**事前条件**:
- zsh プロンプトのタブの scrollback に DA1 クエリ `ESC[c` が記録されている

**基本フロー**:
1. ユーザーが detach する
2. ユーザーが attach する
3. daemon が snapshot を組み立てる際、scrollback 中のデバイスクエリが除去される
4. GUI は snapshot 再生時に応答を生成せず、プロンプトに残骸が挿入されない

**事後条件**:
- プロンプト表示が detach 前と同等（クエリ以外のバイトはすべて保存される）

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | デバイスクエリ除去 | snapshot 組み立て時に応答生成 CSI クエリを除去する | 高 |
| F02 | 非クエリ CSI の保存 | 応答を生成しない CSI は byte-for-byte 保存する | 高 |

### 4.2 機能詳細

#### F01: デバイスクエリ除去

**説明**: `strip_replayable_rich_content` を拡張し、term_core が応答を生成する CSI シーケンスを除去する。

**除去対象**（term_core `csi_dispatch.rs` / `csi_device.rs` の応答挙動と厳密に一致させる）:

| シーケンス | 条件 | term_core の応答 |
|-----------|------|------------------|
| `CSI 5 n` / `CSI 6 n` | プレフィックス無し、第1パラメータが 5 または 6 | DSR / CPR |
| `CSI … c` | プレフィックス無しまたは `?` | DA1 (`ESC[?65;1;4;22c`) |
| `CSI > … c` | `>` プレフィックス | DA2 (`ESC[>65;1;0c`) |
| `CSI 14 t` / `CSI 16 t` / `CSI 18 t` | プレフィックス無し、第1パラメータが 14 / 16 / 18 | XTWINOPS レポート |
| `CSI ? Ps $ p` | `?` プレフィックス + `$` intermediate | DECRPM（未知モードでも常に応答） |

**ビジネスルール**:
- CSI 本体に埋め込まれた C0 制御バイト（ESC 以外）は、term_core のパーサーがシーケンスを中断せず実行するため、除去時に出力へ再出力する
- 末尾で終端していない不完全な CSI は除去しない（既存フィルタの「未完シーケンスは保存」の慣例に従う）

#### F02: 非クエリ CSI の保存

**説明**: 応答を生成しない CSI は一切変更しない。

**保存対象の例**:
- `CSI = … c`（Tertiary DA — term_core は無応答）
- `CSI ? 6 n`（DECXCPR — term_core の dispatch はプレフィックス付き `n` に無応答）
- `CSI 5 n` 以外のパラメータの `n`（例: `CSI 0 n`）
- `CSI 22 t` / `CSI 23 t` 等の 14/16/18 以外の `t`（title stack 操作等）
- `CSI ! p`（DECSTR）/ `CSI " p`（DECSCL）等の `? Ps $ p` 形式でない `p`
- SGR、カーソル移動、`ESC[?1049h/l` 等すべての既存保存対象

## 5. 非機能要件

### 5.1 パフォーマンス要件

- フィルタは単一 O(n) パスを維持する（既存ベンチ: 2 MiB plain payload で 30ms 未満）

## 9. 制約条件

### 9.1 技術的制約

- 除去判定は term_core の実際の応答挙動（`crates/term_core/src/csi_dispatch.rs`）と一致させる。過剰除去（DECSTR / title stack 等の消失）はリプレイ結果を変えるため禁止
- `--no-default-features`（CLI ビルド）を壊さない（mux モジュールは GUI 側だが、feature gate の確認を行う）

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] DA1 / DA2 / DSR(5,6) / XTWINOPS(14,16,18) / DECRPM クエリを含む scrollback からこれらが除去される
- [ ] 非クエリ CSI・プレーンテキスト・SGR・既存の保存対象が byte-for-byte 保存される
- [ ] 不完全 CSI が保存される
- [ ] 既存の `strip_replayable_rich_content` テストが全て通る

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: 各クエリ種別（DA1 / DA2 / DSR 5 / DSR 6 / XTWINOPS 14・16・18 / DECRPM）が単体で除去される
- [ ] 正常系: クエリとテキストの混在 payload でテキストのみ残る
- [ ] 異常系: 不完全 CSI（終端無し）が保存される
- [ ] 境界値: `CSI ? 6 n`・`CSI = c`・`CSI 0 n`・`CSI 22 t`・`CSI ! p` が保存される
- [ ] 境界値: CSI 本体に C0 制御バイトを含むクエリ（例: `ESC [ BEL 6 n`）が除去され、C0 バイトは再出力される

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| DA1 / DA2 | Primary / Secondary Device Attributes（`CSI c` / `CSI > c`） |
| DSR / CPR | Device Status Report / Cursor Position Report（`CSI 5n` / `CSI 6n`） |
| XTWINOPS | ウィンドウ操作 CSI（final `t`）。14/16/18 がサイズレポート要求 |
| DECRPM | DEC Private Mode Report（`CSI ? Ps $ p` への応答） |
| snapshot | reattach / window 切り替え / resume 時に daemon が組み立てる「clear + scrollback + shadow screen」のバイト列 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 修正方式: daemon 側の snapshot 組み立て時に strip する（GUI 側・プロトコル変更はしない）
- [x] strip 範囲: term_core が実際に応答を生成するシーケンスのみ（final byte による粗い除去はしない）
- [x] 検証範囲: unit test のみ（実機の detach → attach 確認は後日ユーザーが実施）

### 14.2 未確認・保留事項

- なし

## 15. 参考資料

- 原因調査記録: メモリ `project_mux_reattach_da1_leak.md`
- 同型の先行修正: `strip_replayable_rich_content` によるビューア OSC 除去（2026-06-19）
