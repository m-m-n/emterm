---
title: "mux タブ切替 bypass 判定の再修正"
created_date: 2026-08-05
status: draft
---

# mux タブ切替 bypass 判定の再修正 - 要件定義書

## 1. 概要

### 1.1 背景

先行 feature `mux-tab-switch-replay-latency`（PR #12、マージコミット
`1b6d2bd`）は main にマージ済みだが、実装された bypass split 判定は
実際に計測されたバグ形状を受け付けない。実測形状は 2 MiB payload / 31
segments / `k=27` で、隣接する scrollback marker の dims が常に異なるため
head fold 後も `middle_segment_count = 26` にしかならず、現行ゲート
`middle_segment_count <= BYPASS_PREFIX_MAX_SEGMENTS`（24、
`crates/term_core/src/terminal_core.rs:1244`/`2166`）に弾かれる。結果として
先行 feature の FR1/FR2 が狙ったレイテンシ目標が実ワークロードで成立せず、
~800〜1000 ms の非 bypass 全 drain が残っている（指摘 `b6a60c440da70e79`）。

あわせて、レビュー round 2 の high 指摘 4 件が「deferred（batch mode:
rework cap reached）」として closed になっており、critical 指摘
`5c6ae6b507b6f638` に対する round 2 の同一ラウンド auto-fix は、新規レビュー
パスによる再確認を受けていない。

### 1.2 目的

- 実測された scrollback 形状に対して、先行 feature の FR1/FR2 レイテンシ目標を
  実際に成立させる。すなわち、末尾寄りに resize marker 群を持つ 2 MiB の重い
  pane への切替を数十 ms のオーダーで完了させる。
- 「deferred（batch mode: rework cap reached）」として closed になった
  round 2 の high 指摘 4 件（`b6a60c440da70e79`、`81507f39e384b34e`、
  `a82206113b8160fd`、`aba5ebbdf9a9addb`）をすべて解消する。
- 新規レビューパスを受けていない round 2 の critical 指摘
  `5c6ae6b507b6f638`（D8 empty-MIDDLE）に対する auto-fix の正しさを確認する。

### 1.3 スコープ

- `crates/term_core/src/terminal_core.rs` の D7/D8 split 判定
  （`:1221`、`:1244`、`:2166`）
- `crates/term_core/src/bench.rs` の bench 群（`:169`、`:286`、および新規の
  26 segment 形状 fixture）
- `src-tauri/src/window_host.rs` の inset 適用（`:1265-1269`）と settler
  self-wake（`:1270-1271`）

対象外の項目は本書 9.1 節を参照。

## 2. ビジネス要件

### 2.1 ビジネス目標

1.2 節に示した 3 点（実測形状でのレイテンシ目標の成立、deferred high 指摘
4 件の解消、未再レビューの critical auto-fix の正しさ確認）。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm mux ユーザー | mux daemon にアタッチしてタブ/pane を切り替える利用者 |
| eMterm メンテナ | 先行 feature のレビュー指摘の解消状況を追う立場 |

### 2.3 期待される効果

- 実測形状（2 MiB payload / 31 segments / `k=27` /
  `middle_segment_count=26`）の重い pane への切替が、現状の ~800〜1000 ms から
  数十 ms のオーダーになる
- 起動時および mux の attach/reattach/tab 切替のたびに最大 1 秒間
  （`RESIZE_SETTLE_MAX_DURATION`）render loop が全速回転する状態が解消される
- ステータスバー高さ変更時の inset が恒久的に stale になる不具合が解消される

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 実測形状の重い pane へのタブ切替 | mux ユーザー | 高 |
| UC02 | 起動・attach 直後の resize settle 待ち | mux ユーザー | 高 |
| UC03 | grid サイズが変わらないステータスバー高さ変更 | mux ユーザー | 高 |
| UC04 | 未再レビューの critical auto-fix の確認 | メンテナ | 高 |

### 3.2 ユースケース詳細

#### UC01: 実測形状の重い pane へのタブ切替

**アクター**: mux ユーザー

**事前条件**:
- 切替先 pane の scrollback が実測形状（2 MiB payload、31 segments、`k=27`）
- 隣接する scrollback marker の dims が常に異なるため、head fold 後の
  `middle_segment_count` は 26

**基本フロー**:
1. ユーザーが重い pane へ切り替える
2. snapshot を replay する
3. 切替後の内容が表示される

**代替フロー**:
- 修正前: `middle_segment_count <= BYPASS_PREFIX_MAX_SEGMENTS`（24）に弾かれ、
  ~800〜1000 ms の非 bypass 全 drain になる

**事後条件**:
- 修正後: 同じ payload サイズの bypass 有効時と同等のオーダー（数十 ms 台）で
  replay が完了する

#### UC02: 起動・attach 直後の resize settle 待ち

**アクター**: mux ユーザー

**事前条件**:
- `ResizeSettler::awaiting_decision()` が true（起動時、または mux の
  attach/reattach/tab 切替の直後）

**基本フロー**:
1. `refresh_status_bar_insets` が呼ばれる
2. settler が決定に達するまで redraw が要求される
3. settler が決定に達する

**代替フロー**:
- 修正前: `src-tauri/src/window_host.rs:1270-1271` が毎フレーム無条件に
  `request_redraw()` を呼ぶため、最大 `RESIZE_SETTLE_MAX_DURATION`（1 秒）
  render loop が全速で回る

**事後条件**:
- 修正後: self-wake がレート制限され、かつアイドルなウィンドウでも
  `RESIZE_SETTLE_MAX_DURATION` 以内に settler が決定に達する

#### UC03: grid サイズが変わらないステータスバー高さ変更

**アクター**: mux ユーザー

**事前条件**:
- ステータスバー高さが変わるが、導出される `(cols, rows)` candidate は変わらない
  （例: フォントサイズを大きくして cell 高さが `ROW_HEIGHT` 22.0 を超える場合、
  または row の clamp が効く場合）

**基本フロー**:
1. ステータスバー高さが変化する
2. `status_bar_top_inset_logical` / `status_bar_bot_inset_logical` が更新される

**代替フロー**:
- 修正前: 代入が `resize_settler.observe(candidate)` が `Some` を返したときにのみ
  行われる（`src-tauri/src/window_host.rs:1265-1269`）ため、inset が恒久的に
  stale になり、同じフィールドを読む mux サイドバーのポインタ経路にも影響する

**事後条件**:
- 修正後: 導出 grid サイズが変わらない高さ変更でも inset が適用される

#### UC04: 未再レビューの critical auto-fix の確認

**アクター**: メンテナ

**事前条件**:
- `crates/term_core/src/terminal_core.rs:1221` の `&& candidate_h < k` ガードが
  `h == k`（MIDDLE が空）の形状を pre-D7 経路に落としている

**基本フロー**:
1. `h == k` 形状で core を構築する
2. 構築結果の dims と `scrollback_populated` を確認する

**事後条件**:
- 呼び出し側が要求した target dims で core が構築され、`scrollback_populated`
  が正しいこと、回帰テストで pin されていること、および本 feature 内で新規
  レビューパスを受けていること

## 4. 機能要件

各機能要件の完全な記述は SPEC.md の Functional Requirements（FR1〜FR6）を参照。
本書では一覧のみ示す。

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 実測 marker cluster 形状（26 segment MIDDLE）で split が engage する | `middle_segment_count = 26` の実測形状を数十 ms オーダーで replay する。NFR1 を守ること | 高 |
| FR2 | 回帰 bench が SPEC 記載の 26 segment cluster を再現する | 24 segment に絞られた既存 fixture ではなく 26 segment 形状を再現し、bypass engage 時コストに整合したレイテンシ上限を assert する | 高 |
| FR3 | ResizeSettler の self-wake をレート制限する | `awaiting_decision()` が true の間の無条件 `request_redraw()` をやめ、既存の `toast_redraw_due` に倣ったレート制限にする | 高 |
| FR4 | inset 適用を導出 grid サイズ変化に依存させない | 導出 `(cols, rows)` が変わらない高さ変更でも inset を適用する（指摘 `a82206113b8160fd` / `aba5ebbdf9a9addb` は同一機序・1 修正） | 高 |
| FR5 | D8 empty-MIDDLE auto-fix の正しさを新規レビューで確認する | `h == k` 形状で target dims と `scrollback_populated` が正しいことを回帰テストで pin し、コードに新規レビューパスを通す | 高 |
| FR6 | 先行 feature の受け入れ条件を非回帰として維持する | 通常切替 1.57 ms 水準、bypass 等価性、`visible_row_count` 0→1 の全 pane Resize 抑止、切替中 resize 競合耐性、同一 pane 連続切替の off-thread replay 重複回避 | 高 |

### 4.2 機能詳細

対象がユーザー向け画面ではなく、内部の replay 経路および window host の
resize/inset 経路であるため、各機能の機序・影響範囲は SPEC.md の
Implementation Approach に記載する。

## 5. 非機能要件

### 5.1 パフォーマンス要件

- **NFR1（非 bypass コストの二重支払いを起こさない）**: FR1 の修正は、
  round-7/round-8 レビューで追加された `BYPASS_PREFIX_MAX_BYTES`（64 KiB）と
  `suffix_len >= split_at`（現在は `suffix_len >= middle_len`）が防いでいる
  「2nd-pass による非 bypass コストの二重支払い」を復活させてはならない。
  prefix 側を安価に replay できる形にせずにゲートを緩めるのはスコープ外
  （先行 SPEC の NFR1 を引き継ぎ）。
- **NFR2（settle 期間中の render loop CPU の抑制）**: resize settle 期間中の
  self-wake redraw は、ディスプレイのフルフレームレートではなく穏当なレートに
  抑える。これにより settle 期間が off-thread snapshot replay worker と CPU を
  奪い合わない。

### 5.2 セキュリティ要件

該当なし（Rust 側の内部パフォーマンス/正しさ修正のみ）。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

- **NFR3（既存のテスト/bench ガードを green に保つ）**:
  `snapshot_replay_bench_2mib_seq`（`crates/term_core/src/bench.rs:169`）と、
  `src-tauri` および `crates/term_core` の既存 `--lib` スイートが green のまま
  であること（`tabs.rs` の replay テストは `test/README.md` の記載どおり
  `-- --test-threads=1` が必要な場合がある。`tabs.rs` の off-thread テスト 7 件は
  先行 feature の retrospect のとおり、main でも当ホストでは慢性的に flaky）。

### 5.5 互換性要件

該当なし。

## 6. UI/UX要件

該当なし。design ステップは skipped（14.1 節参照）。

## 7. データ要件

該当なし（永続データモデルの変更なし）。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約（=スコープ外）

タスク記述に基づき、以下は本 feature のスコープ外とする。

- `cols` も変化する resize storm の full reflow コスト削減
- client 側の `PtyOutput` coalesce
- `[osc-probe gui]` がリリースビルドに残存している件
- settings ウィンドウの子プロセスのゾンビ化
- 先行 feature の FR8 スコープ決定を超える decode / daemon fetch の重複排除

### 9.2 ビジネス上の制約

- スコープの基準は main にマージ済みの PR #12 のコード（マージ `1b6d2bd`）。
  D7/D8 ゲート、`ResizeSettler`、`candidate_h < k` ガードはいずれも
  integration worktree に存在することを直接確認済みであり、本 feature は
  fix-forward であってゼロからの再実装ではない。
- 「数十 ms」の上限は、絶対的な実時間定数ではなく、既存 bench と同様に
  同一ホストで実測した bypass engage 時コストに対する相対的な bound として
  assert する（`marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost`
  と同じ流儀）。

### 9.3 スケジュール制約

なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| ゲートを緩めるだけでは 2nd-pass の非 bypass コスト二重支払いが復活する | 高 | prefix 側を安価に replay できる形にしてから緩める（NFR1） |
| settler の self-wake を絞りすぎると、アイドルなウィンドウで決定に到達しなくなる（round-1 指摘 `02546e5e10deb500` / `5b1878c41d3e02d6-perf-P2` の再発） | 高 | レート制限しつつ `RESIZE_SETTLE_MAX_DURATION` 以内の決定到達をテストで担保する（FR3） |
| `tabs.rs` の off-thread テスト 7 件が当ホストで慢性的に flaky | 中 | `-- --test-threads=1` を用い、main のベースラインと比較して判定する（NFR3） |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] 実測形状（2 MiB payload、31 segments、`k=27`、head fold 後
  `middle_segment_count=26`）の replay が split を engage する（あるいは他の
  手段で上限内に収まる）。term_core レベルの単体テストで engage を示し、
  release ビルドの bench で bypass engage 時コストに対するレイテンシ上限を
  assert する
- [ ] 新規 bench fixture が MIDDLE 26 segment を再現し、既存の 24 segment
  bench は変更なく green のまま
- [ ] settler が決定待ちの間、redraw の self-wake がレート制限されている
  （`toast_redraw_due` に倣った、単体テスト可能な述語を `window_host.rs` に
  用意する）。かつアイドルなウィンドウでも `RESIZE_SETTLE_MAX_DURATION`
  以内に settler が決定に到達する
- [ ] 導出 grid サイズが変わらないステータスバー高さ変更で、新しい inset 値が
  適用される（inset 適用述語の単体テスト）。PTY reshape storm を再発させない
  （先行 FR6 が green のまま）
- [ ] `h == k`（MIDDLE が空）形状を回帰テストで pin する: 呼び出し側が要求した
  dims で core が構築され、`scrollback_populated` が参照実装と一致する
- [ ] 通常切替 bench のベースラインが劣化していない。bypass 等価性テストと
  `snapshot_replay_bench_2mib_seq` が green のまま
- [ ] deferred になった round 2 の high 指摘 4 件と、未再レビューの critical が
  すべて対処され、本 feature 自身のレビューを通過する

### 11.2 KPI

該当なし（実測値と既存 bench による受け入れ判定のみ）。

## 12. テストシナリオ

### 12.1 テスト観点

| ID | 種別 | 内容 | 対応要件 |
|----|------|------|----------|
| TS1 | 単体（`crates/term_core`） | 26 segment MIDDLE の marker cluster（隣接 dims が全て異なり、D8 の方向に沿って settle 後 target より上に振動する形状）で、修正後に split が engage する。修正前は fail することを確認する | FR1 |
| TS2 | 単体（`crates/term_core`） | 新しい segment 数の扱いの境界（ちょうど / 1 つ超え）。既存の 24 境界テストの意図を保つ | FR1 |
| TS3 | 単体（`crates/term_core`） | `h == k`（MIDDLE が空）形状が呼び出し側の target dims を返し、`scrollback_populated` が正しい（FR5 の pin） | FR5 |
| TS4 | 単体（`src-tauri` `window_host`） | settler wake のレート制限述語: 制限内で繰り返される `awaiting_decision` フレームは redraw を要求せず、制限を超えたら要求する。決定は `RESIZE_SETTLE_MAX_DURATION` 以内に到達する | FR3, NFR2 |
| TS5 | 単体（`src-tauri` `window_host`） | 高さが変わり導出 grid サイズが変わらない場合に inset が適用される。何も変わらない場合は inset も変わらず `pending_resize` も発生しない | FR4, FR6 |
| TS6 | bench（release、`--include-ignored`） | 新規 26 segment 形状 bench が bypass engage 時コストに対する上限を assert する。`snapshot_replay_bench_2mib_seq` と `marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost` は green のまま | FR2, NFR1, NFR3 |
| TS7 | 手動（実機、先行 VERIFICATION の MT-1 を引き継ぎ） | クライアントを再起動し、mux daemon に再アタッチして重い pane に切り替える。表示が数十 ms で現れる | FR1, FR6 |

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| bypass split | snapshot replay を分割し、target dims に一致する末尾側のみを即時再生する高速経路 |
| MIDDLE / HEAD | split 対象 payload のうち、head fold 後に残る中間 segment 群（MIDDLE）と、その手前に畳まれる先頭側（HEAD） |
| `middle_segment_count` | head fold 後に MIDDLE に残る segment 数。現行ゲートは `BYPASS_PREFIX_MAX_SEGMENTS`（24）以下を要求する |
| `k` | 末尾から連続して target dims に一致する segment 群の開始 index |
| resize marker | dims 変化時に scrollback へ記録される印。隣接する marker の dims が常に異なると head fold で畳めない |
| `ResizeSettler` | grid サイズ candidate の揺れが収まるまで決定を保留する仕組み。`awaiting_decision()` が true の間が settle 期間 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] スコープの基準: main にマージ済みの PR #12 のコード（マージ `1b6d2bd`）。
  D7/D8 ゲート、`ResizeSettler`、`candidate_h < k` ガードが integration
  worktree に存在することを直接確認済み。本 feature は fix-forward であり、
  ゼロからの再実装ではない（オーケストレータがタスク記述に記録したスコープ訂正）
- [x] スコープ外（タスク記述より）: `cols` も変化する resize storm の full
  reflow コスト削減、client 側 `PtyOutput` coalesce、`[osc-probe gui]` の
  リリースビルド残存、settings ウィンドウ子プロセスのゾンビ化、先行 FR8 の
  スコープ決定を超える decode / daemon fetch の重複排除
- [x] 「数十 ms」の上限の assert 方法: 絶対的な実時間定数ではなく、同一ホストで
  実測した bypass engage 時コストに対する相対的な bound として assert する
  （`marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost` と同じ流儀）
- [x] 指摘 `a82206113b8160fd` と `aba5ebbdf9a9addb` は、同一の不具合を 2 つの
  レビュー観点から報告したものであり、1 つの修正で満たされる（`round2.yaml`
  自身に明記）
- [x] design ステップ: skipped。Rust の terminal core と window host における
  内部のパフォーマンス/正しさ修正であり、新規 UI 面も視覚・操作設計の変更も無い
  （ステータスバー inset の修正は既に仕様化済みの描画挙動を回復するもの）。
  design system トークンの消費・生成も無い

### 14.2 未確認・保留事項

なし（全要件が resolved）。

## 15. 参考資料

- SPEC.md: `feature-docs/mux-tab-switch-bypass-refix/SPEC.md`
- 先行 feature: `feature-docs/mux-tab-switch-replay-latency/SPEC.md`,
  `feature-docs/mux-tab-switch-replay-latency/REQUIREMENTS.md`
- 先行 feature のレビュー round 2（`round2.yaml`）: 指摘
  `b6a60c440da70e79`、`81507f39e384b34e`、`a82206113b8160fd`、
  `aba5ebbdf9a9addb`、`5c6ae6b507b6f638`
- 先行 feature のマージ: PR #12、マージコミット `1b6d2bd`
- `crates/term_core/src/terminal_core.rs:1221`, `:1244`, `:2166`
- `crates/term_core/src/bench.rs:169`, `:286`
- `src-tauri/src/window_host.rs:1265-1269`, `:1270-1271`, `:2477`
- `test/README.md`（`tabs.rs` replay テストの `--test-threads=1` 注記）
