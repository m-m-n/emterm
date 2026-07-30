---
title: "mux タブ切替 snapshot replay の bypass 復旧"
created_date: 2026-07-31
status: draft
---

# mux タブ切替 snapshot replay の bypass 復旧 - 要件定義書

## 1. 概要

### 1.1 背景

mux でタブ切替を行うと、scrollback が溜まった pane への切替表示に数秒かかる。
コードレベルで機序が確定しており、eMterm クライアント再起動 + mux 再アタッチ後の
GUI 起動シーケンスでステータスバーの表示行数が 0→1 に確定する際、全 pane に
resize が連打され、`ScrollbackRingBuffer::attribute_write` が resize marker を
scrollback 末尾に溜め込む。この状態で `terminal_core.rs` の bypass split 判定
（`k <= BYPASS_PREFIX_MAX_SEGMENTS` / `split_at <= BYPASS_PREFIX_MAX_BYTES` /
`suffix_len >= split_at`）が破れ、payload 全体（実測 2.1 MB）を非 bypass で
replay するため、実測 782.8〜977.6 ms の遅延が生じる（bypass 有効時は同一 pane で
9.3〜9.5 ms）。

実測により、当初の想定（再アタッチ時に marker が 1 個付く）より深刻な形状
（141 バイト間隔で 24 回の rows 振動により marker が 27 個蓄積）が確認され、
さらに「切替処理中の resize competition による target 不一致」「同一 pane への
連続切替での snapshot 二重取得」という独立した副次問題も判明した。

### 1.2 目的

resize marker が蓄積した pane への切替、および切替処理中の resize competition が
起きた場合でも、bypass 経路と同等のオーダー（数十 ms 台）で replay を完了させる。
あわせて、resize marker が蓄積する根本原因（ステータスバー行数確定前の全 pane
reshape）を解消する。

### 1.3 スコープ

- `crates/term_core/src/terminal_core.rs` の bypass split 判定ロジック
- `src-tauri/src/mux/scrollback_buffer.rs` の resize marker 記録契機
- `src-tauri/src/tabs.rs` の GUI grid resize → 全 pane `MessageType::Resize` 送出経路
- 同一 pane への連続切替時の snapshot 取得経路

対象外の項目は本書 9.1 節（スコープ外）を参照。

## 2. ビジネス要件

### 2.1 ビジネス目標

mux でタブ切替した際の体感遅延（数秒）を解消し、通常切替（1.57 ms 水準）と
同等の応答性に戻す。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm mux ユーザー | eMterm クライアントの再起動・再アタッチ後に mux タブを切り替える利用者全般 |

### 2.3 期待される効果

- 重いタブへの切替が瞬時になる（現状 782.8〜977.6 ms → 数十 ms 台）
- 通常切替（軽いタブ）の応答性が劣化しない

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | scrollback が溜まった pane へのタブ切替 | mux ユーザー | 高 |
| UC02 | 切替処理中に GUI grid resize が発生する | mux ユーザー | 中 |
| UC03 | 同一 pane への連続切替 | mux ユーザー | 中 |

### 3.2 ユースケース詳細

#### UC01: scrollback が溜まった pane へのタブ切替

**アクター**: mux ユーザー

**事前条件**:
- eMterm クライアントを再起動し、mux daemon へ再アタッチ済み
- 切替先 pane の scrollback 末尾に、target と異なる dims の resize marker が
  複数（実測 27 個）蓄積している

**基本フロー**:
1. ユーザーが重いタブへ切り替える
2. daemon から snapshot を取得し replay する
3. 切替後の内容が表示される

**代替フロー**:
- 修正前: bypass 判定の 3 条件（`k <= 24` / `split_at <= 64 KiB` /
  `suffix_len >= split_at`）が同時に破れ、非 bypass で 2.1 MB 全体を replay
  （実測 782.8〜977.6 ms）

**事後条件**:
- 修正後: 同じ pane への切替が bypass 有効時と同等のオーダー（数十 ms 台）で完了する

#### UC02: 切替処理中に GUI grid resize が発生する

**アクター**: mux ユーザー

**事前条件**:
- eMterm 起動直後などステータスバー表示行数が確定していないタイミングでタブ切替が発生

**基本フロー**:
1. decode 時点の target dims と replay 時点の target dims が、切替処理中の
   resize により食い違う
2. 末尾 dims と target が不一致になり `k` が全 segment 数まで増大する

**代替フロー**:
- 修正前: `k` が全 segment 数となり `split_at` が payload 全長、
  `suffix_len=0` となって bypass が脱落する（実測 21.0 ms、resize が無ければ 9.5 ms）

**事後条件**:
- 修正後: 切替処理中に resize が競合しても bypass が脱落しない

#### UC03: 同一 pane への連続切替

**アクター**: mux ユーザー

**事前条件**:
- 同一 pane へ短時間に連続して切り替える

**基本フロー**:
1. 1 回目の切替で snapshot decode が発生する
2. 2 回目の切替（またはごく短時間内の再取得）で snapshot decode が再度発生する

**代替フロー**:
- 修正前: 同一 pane に対して 1 ms 差で decode が 2 回走り、1 回目の replay 結果が
  破棄される（実測: segs=9 → segs=10 の 2 回 decode）

**事後条件**:
- 修正後: 同一 pane への連続切替で snapshot を 2 回取得しない

## 4. 機能要件

機能要件の詳細は SPEC.md の Functional Requirements（FR1〜FR8）を参照。
本書では概要のみ示す。

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | resize marker 蓄積形状での高速 replay | 2 MiB scrollback + 末尾寄り resize marker 群 + 小さい suffix の形状で bypass 相当の速度に収める | 高 |
| F02 | 回帰用 bench/テスト追加 | F01 の形状を再現し、レイテンシ上限を assert する bench/回帰テスト | 高 |
| F03 | 通常切替の非劣化 | 既存の通常切替（1.57 ms 水準）を劣化させない | 高 |
| F04 | bypass 等価性維持 | viewport / cursor が非 bypass 経路と一致し、`scrollback_populated` の意味が変わらない | 高 |
| F05 | 既存 bench ガード維持 | `snapshot_replay_bench_2mib_seq` が green のまま | 高 |
| F06 | 起動時 reshape 連打の抑制 | ステータスバー行数確定前に全 pane へ resize を送らない | 高 |
| F07 | 切替中 resize competition への耐性 | 切替処理中の grid resize で target 不一致による bypass 脱落を起こさない | 中 |
| F08 | snapshot 二重取得の防止 | 同一 pane への連続切替で snapshot を 2 回取得しない | 中 |

### 4.2 機能詳細

各機能の入出力・処理フロー・エラーケースは、対象がユーザー向け画面ではなく
内部の replay/resize パイプラインであるため、SPEC.md の Implementation Approach
に記載する（本書では機能一覧のみで足りる）。

## 5. 非機能要件

### 5.1 パフォーマンス要件

- resize marker 蓄積形状（実測: 2.1 MB, 31 segments, k=27）への切替が数十 ms 台に収まる
- 通常切替（1.57 ms）が劣化しない

### 5.2 セキュリティ要件

該当なし（内部パフォーマンス修正のみ）。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

- `BYPASS_PREFIX_MAX_BYTES` / `suffix_len >= split_at` ゲートを緩める場合、
  そのゲートが防いでいる「2nd-pass worker による非 bypass コストの二重payment」
  が再発しないことをコードコメントまたはテストで担保する

### 5.5 互換性要件

該当なし。

## 6. UI/UX要件

該当なし（UI 変更なし、内部パフォーマンス修正のみ）。

## 7. データ要件

該当なし（DB 等の永続データモデル変更なし）。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約（=スコープ外）

- `cols` も変化する resize storm の full reflow コスト削減
  （`VERIFICATION.md` NFR1 で既にスコープ外宣言済み）
- client 側の `PtyOutput` coalesce 不足（別の既知改善項目）
- `[osc-probe gui]` プローブがリリースビルドに `warn!` で残存している件
  （`src-tauri/src/tabs.rs:1478` 他 7 箇所）
- settings ウィンドウの子プロセスがゾンビ化する件（調査中に PID 3142942 で観測）

### 9.2 ビジネス上の制約

- `BYPASS_PREFIX_MAX_BYTES`（64 KiB）と `suffix_len >= split_at` は round-7 /
  round-8 のレビュー指摘で追加された条件で、「prefix が巨大な形状で split を
  engage すると 2nd-pass worker が同じ非 bypass コストを 2 回払う」ことを
  防いでいる。単純に閾値を緩めるとその二重コストが復活する。

### 9.3 スケジュール制約

なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 既存 scrollback に溜まった marker は消えない | 高 | 下流（replay 高速化）と上流（marker 連打抑制）の両方を実施する必要がある |
| 閾値を緩めるだけだと 2nd-pass の二重コストが復活する | 高 | prefix 側も安価に replay できる形にしてから緩める |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] 「2 MiB scrollback + 末尾寄りに target と異なる dims の resize marker +
  小さい suffix」形状の pane への切替が、bypass 有効時と同等のオーダー
  （数十 ms 台）に収まる
- [ ] 上記形状を再現する bench / 回帰テストが追加され、レイテンシ上限を assert している
- [ ] ordinary switch（現状 1.57 ms）が劣化していない
- [ ] bypass の等価性保証が維持されている（viewport / cursor が非 bypass 経路と
  一致、`scrollback_populated` の意味が変わらない）
- [ ] `snapshot_replay_bench_2mib_seq`（`crates/term_core/src/bench.rs:169`）の
  既存ガードが green
- [ ] ステータスバーの行数確定前に PTY を reshape しない（起動時の
  `visible_rows=0 → 1` で全 pane に `Resize` を送らない）
- [ ] 切替処理中に grid resize が入っても、target 不一致による bypass 脱落が
  起きない（1 回目 21.0 ms / 2 回目 9.5 ms の差が消える）
- [ ] 同一 pane への連続切替で snapshot を 2 回取得しない

### 11.2 KPI

該当なし（実測値による受け入れ判定のみ）。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: 通常切替（軽いタブ）の応答性が劣化しないこと
- [ ] 異常系: resize marker が 27 個蓄積した pane への切替が高速化されること
- [ ] 境界値: `BYPASS_PREFIX_MAX_SEGMENTS` / `BYPASS_PREFIX_MAX_BYTES` の閾値付近の形状
- [ ] パフォーマンス: bench による回帰ガード（既存 + 新規）

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| bypass split | scrollback replay を prefix/suffix に分割し、suffix のみ再生する高速経路 |
| resize marker | `ScrollbackRingBuffer::attribute_write` が dims 変化時に scrollback へ記録する印 |
| k | `stable_target_suffix_start` が返す、末尾から連続して target dims に一致する segment 群の開始 index |
| split_at | bypass split 時の prefix バイト長（payload オフセット） |

## 14. 確認事項

### 14.1 確認済み事項

Notion タスクページに詳細な調査記録（機序の特定・実測値・上流トリガーの確定）が
含まれており、原因はコードレベルで確定済み。batch 実行のため Codex への相談は
試みたが、Codex CLI が利用不可のため以下は Claude の判断で確定した。

- [x] feature 名: `mux-tab-switch-replay-latency` とする
- [x] design ステップ: UI 変更が無いため不要（skipped）と判断
- [x] 受け入れ条件: タスクページに記載された 8 項目（当初 5 項目 + 追加提案 3 項目）
  すべてを本書 11.1 節の受け入れ基準として採用
- [x] スコープ外 4 項目はタスクページの「スコープ外」節をそのまま採用

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- Notion タスクページ: https://www.notion.so/3ac3509ec8ee81578318cd552d238518
- 調査レポート（タスクページ記載のパス、eMterm リポジトリの `tmp/` 配下）:
  `tmp/tab-switch-latency-investigation-2026-07-30.md`
```
