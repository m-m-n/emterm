---
title: "mux off-thread swap での callbacks / OSC 登録喪失の修正"
created_date: 2026-07-09
status: draft
---

# mux off-thread swap での callbacks / OSC 登録喪失の修正 - 要件定義書

## 1. 概要

### 1.1 背景

mux の window 切替で snapshot が 64KB (`OFFTHREAD_REPLAY_THRESHOLD_BYTES`) 以上の場合、off-thread snapshot replay が発動する。worker が構築する新 `TerminalCore` は `Send` にするため callbacks 無し・app-layer OSC 登録無しで作られるが、`apply_offthread_swap` (`src-tauri/src/tabs.rs:853`) がこの新 core をタブの live core として丸ごと swap するため、旧 core の `callbacks`（`tabs.rs:468` で設置）と OSC 9999 登録（`tabs.rs:450-453`）が失われる。swap 後に復元するコードは存在しない。

これにより以下の既知バグが発生している:

1. **detach 後 attach 不能（ハング）**: 64KB 超 snapshot の window を一度表示した GUI セッションで detach すると、再 attach 時の Welcome フレーム（pre-mux 経路 = core parser の callbacks / OSC 登録依存）が処理されず、GUI が Attach を送信しないままハングする。GUI 再起動でのみ回復する。原因調査の全容は `tmp/discussion-mux-detach-attach-failure.md` を参照。
2. **mux 経由で viewer が起動しない間欠バグ**（同根の可能性が高い）: swap 後は callbacks=None のため、mux 中の inner content（viewer OSC 777 / Kitty 画像 / タイトル / bell / テーマ OSC）がすべて無視される。

### 1.2 目的

`apply_offthread_swap` の core swap 時に、旧 core から新 core へ callbacks と app-layer OSC 登録を移植し、swap 後もタブが pre-mux 経路の mux フレームと callback 依存の inner content を処理できるようにする。

### 1.3 スコープ

**対象**:
- `apply_offthread_swap` での callbacks 移植と OSC 9999（`MUX_OSC_PARAM` → `OSC_MUX_INBAND`）登録の保持

**対象外**（並行調査としてレポートのみ）:
- daemon の Detached 送信エラー握りつぶし（`connection.rs:743` の `let _`）のログ化
- 本物 Detached が bridge に届かないレース（send と close の競合疑い）の修正
- 上記 2 件は本 feature のコード変更に含めず、調査を並行して進め、関連ログと実装中に判明した事実を `./tmp` 配下にレポートとして残す

## 2. ユースケース

### UC01: 大 scrollback window 表示後の detach → attach

**アクター**: mux セッションを使う eMterm ユーザー

**事前条件**:
- mux daemon にセッションが存在し、snapshot が 64KB 以上になる window（大きな scrollback を持つ pane）がある

**基本フロー**:
1. ユーザーが該当 window に切り替える（off-thread replay が発動する）
2. ユーザーが detach する（プロンプトに戻る）
3. ユーザーが再度 attach する
4. GUI が Welcome を処理して Attach を送信し、attach が成立する

**事後条件**:
- attach が成立し、mux セッションの画面が表示される

### UC02: 大 scrollback window 表示後の mux 内リッチコンテンツ表示

**アクター**: mux セッション内で `emterm markdown` 等を使うユーザー

**基本フロー**:
1. ユーザーが 64KB 超 snapshot の window に切り替える
2. ユーザーが mux 内で `emterm markdown <file>` を実行する
3. viewer ウィンドウが起動する

**事後条件**:
- callback 依存の inner content（viewer / タイトル / bell / テーマ OSC 等）が swap 前と同様に処理される

## 3. 機能要件

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | callbacks の移植 | `apply_offthread_swap` の core swap で旧 core の `callbacks` を新 core に移植する | 高 |
| F02 | OSC 登録の保持 | swap 後の core に app-layer OSC 登録（OSC 9999 = `MUX_OSC_PARAM` → `OSC_MUX_INBAND`）が有効であること | 高 |
| F03 | swap 後の pre-mux mux フレーム処理 | swap 後のタブが pre-mux 経路の Welcome（OSC 9999 / APC 両形式）を処理し attach が成立すること | 高 |
| F04 | swap 後の callback 依存処理 | swap 後のタブで callback 経由の inner content 処理が機能すること | 高 |

## 4. 非機能要件

- 同期 replay 経路（`reset_and_replay`、64KB 未満）の挙動を変更しない（callbacks 保持済みのため変更不要）
- FR7 worker-panic フォールバック（`reset_frame_for_replay`）は同期経路のため影響を与えない
- CLI-only ビルド（`--no-default-features`）がコンパイルできること

## 5. 制約条件

- worker core（`build_from_snapshot`）が callbacks 無しで構築される設計（`Send` 制約）は変更しない。移植は swap 時（メインスレッド）に行う
- 2nd-pass scrollback restore（`spawn_scrollback_restore` → `apply_scrollback_restore`）は live core への merge 方式であり core swap を行わない前提。実装時にこの前提を確認する

## 6. 成功基準

- [ ] swap 後の core に callbacks が設置されていることを単体テストで確認できる
- [ ] swap 後の core で OSC 9999 登録が有効であることを単体テストで確認できる
- [ ] swap 後に pre-mux 経路の Welcome が `apply_mux_message` に到達することを単体テストで確認できる
- [ ] 既存テストがすべて通る
- [ ] 実機確認（64KB 超 snapshot window で switch → detach → attach、mux 内 viewer 起動）はユーザーが後日実施する

## 7. テストシナリオ

- [ ] 正常系: off-thread swap 後に callbacks / OSC 登録が保持される
- [ ] 正常系: swap 後の Welcome（OSC 9999 形式・APC 形式）が処理される
- [ ] 回帰: 同期経路（64KB 未満）の replay 挙動が変わらない
- [ ] 回帰: 既存の off-thread replay テスト（マーク backfill / 2nd-pass restore 等）が通る

## 8. 用語定義

| 用語 | 定義 |
|------|------|
| off-thread replay | snapshot ≥ 64KB のとき worker スレッドで新 core を構築し swap する機構 |
| pre-mux 経路 | mux 未確立時に core parser の callbacks / OSC 登録で mux フレームを検出する経路（`process_outer_via_core`） |
| mux_apc_extractor | mux 確立後に外側ストリームから mux フレームを抽出する独立パーサー（callbacks 非依存） |

## 9. 確認事項

### 9.1 確認済み事項

- [x] スコープ: 本体修正（callbacks 移植 + OSC 再登録）のみ。daemon send エラーログ化と Detached レースは並行調査 + `./tmp` レポートに留める
- [x] 検証方法: 単体テスト必須、実機 detach→attach / viewer 起動確認はユーザーが後日実施
- [x] feature 名: mux-offthread-swap-callback-restore

### 9.2 未確認・保留事項

- なし

## 10. 参考資料

- 原因調査レポート: `tmp/discussion-mux-detach-attach-failure.md`
