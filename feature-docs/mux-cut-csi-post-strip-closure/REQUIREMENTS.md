---
title: "mux-cut-csi-post-strip-closure"
created_date: 2026-10-03
status: draft
---

# mux-cut-csi-post-strip-closure - 要件定義書

## 1. 概要

### 1.1 背景

前身フィーチャー mux-suppressed-output-round4-fixes の review round 1 に、medium 指摘 `4c0ad9058a983648` がある。対象は mux デーモンの書き込みフィルタで、カット時の閉鎖と、次の呼び出しへ引き継ぐ CSI 状態の決め方にかかわる。

### 1.2 目的

- カット時の閉鎖と次の呼び出しへ引き継ぐ CSI 状態を、除去後の出力の状態で決める。strip が開いた CSI を中断する構文を除去したときも、クライアントが開始していない問い合わせ（CPR など）がリプレイで成立しないようにする。
- review round 1 の medium 指摘 `4c0ad9058a983648` について、対応済みか・対応不要かを判断し、理由と回帰テストを記録する。

### 1.3 スコープ

対象:

- カット直前の閉鎖（DEL）の判定を、strip 適用後の出力で行う（FR1）
- カットの無い呼び出しの後に引き継ぐ CSI 状態を、strip 適用後の出力で決める（FR2）
- 512 KiB を超えたときの早期フラッシュにも同じ規則を適用する（FR3）
- 回帰テストの追加（FR4）
- 指摘 `4c0ad9058a983648` の判断の記録（FR5）
- カットの無い strip 連結を残件として記録する（FR6）

対象外:

- 共有の strip（`strip_rich_content_and_remap_with_designator`。スナップショット経路を含む）の出力の変更
- カットの無い strip 連結の修正（FR6 で残件として記録し、別タスクで扱う）
- 前身の `feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml` と `DECISIONS.md` の変更
- UI の変更

## 2. ビジネス要件

### 2.1 ビジネス目標

- カット時の閉鎖と次の呼び出しへ引き継ぐ CSI 状態を、除去後の出力の状態で決める。strip が開いた CSI を中断する構文を除去したときも、クライアントが開始していない問い合わせ（CPR など）がリプレイで成立しないようにする。
- review round 1 の medium 指摘 `4c0ad9058a983648` について、対応済みか・対応不要かを判断し、理由と回帰テストを記録する。

### 2.2 対象ユーザー

記載なし。

### 2.3 期待される効果

- カットとその状態引き継ぎの経路で、クライアントが開始していない問い合わせが、リングのリプレイで成立しない。
- 指摘 `4c0ad9058a983648` の判断・理由・回帰テストが、このフィーチャーの決定記録に残る。

## 3. ユースケース

該当なし。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 状態 |
|----|--------|------|
| FR1 | カット閉鎖を除去後の出力で判定 | resolved |
| FR2 | 引き継ぐ CSI 状態を除去後の出力で決める | resolved |
| FR3 | オーバーフロー経路も同じ規則にする | resolved |
| FR4 | 回帰テストの追加 | resolved |
| FR5 | 判断の記録 | resolved |
| FR6 | カットの無い strip 連結は残件として記録 | resolved |

### 4.2 機能詳細

#### FR1: カット閉鎖を除去後の出力で判定

**説明**: カットの直前で出力したバイト列（strip 適用後）が未完の CSI で終わるとき、書き込みフィルタはその後に CSI_CLOSING（DEL）を 1 回書く。strip が除去した構文の ESC は書かれていないので、CSI を中断したとは扱わない。

- `ESC[6` + `ESC]777;emterm;markdown;…BEL`、`ESC[6` + `ESC_G…ESC\`、`ESC[6` + `ESC[6n` の直後のカットでは、出力は `ESC[6` + DEL になる。
- 除去対象の後に、カットで落とす未完の構文が続く場合（例: `ESC[6` + 完全な起動列 + `ESC]0;t` + カット）も同じ。
- 除去されない ESC 系の構文（`ESC]0;t BEL`、`ESC X` など）が CSI を中断した場合は、従来どおり DEL を書かない。

#### FR2: 引き継ぐ CSI 状態を除去後の出力で決める

**説明**: カットの無い呼び出しの後に引き継ぐ CSI 状態は、strip 適用後に書かれたバイト列の末尾の状態にする。

- 除去対象の構文が別の呼び出しで完成し、保持していた run が strip で空になった場合も、前の呼び出しから開いていた CSI は開いたままとして引き継ぐ。
- 後に続くフォールバック閉鎖（空範囲 + fed 0 のカット）や次のカットは、この状態で DEL を書く。

#### FR3: オーバーフロー経路も同じ規則にする

**説明**: 512 KiB を超えたときの早期フラッシュでも、引き継ぐ状態（最後のセグメント）とカット前の閉鎖（カットが続くセグメント）を、strip 適用後の出力の状態で決める。

#### FR4: 回帰テストの追加

**説明**: `src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs` の `CLOSING_CASES` に 3 形式を、kept=`ESC[6`、inside=true として追加する。

- `ESC[6 ESC]777;emterm;markdown;…BEL`
- `ESC[6 ESC_G…ESC\`
- `ESC[6 ESC[6n`

term_core 生ストリーム比較にも、それぞれにカットと後続の `n` を続けたケースを、同一呼び出しのカットとフォールバック閉鎖の両方で追加する。

#### FR5: 判断の記録

**説明**: stable_id `4c0ad9058a983648` について、判断（対応済み / 対応不要）、理由、回帰テストを、`feature-docs/mux-cut-csi-post-strip-closure/` 配下の決定記録に書く。前身の `feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml` と `DECISIONS.md` は変更しない。

#### FR6: カットの無い strip 連結は残件として記録

**説明**: 共有の strip（`strip_rich_content_and_remap_with_designator`。スナップショット経路を含む）の出力は変えない。カットの無い strip 連結は修正せず、決定記録に残件として書いて別タスクで扱う。対象は次の 3 つ。

1. `ESC[6` + 起動列 + `n` を 1 回の呼び出しで渡すと `ESC[6n` として書かれる。
2. リングが除去済み構文の直後の `ESC[6` で終わったままスナップショットが取られ、次のライブの `n` が CPR を成立させうる。
3. 呼び出しをまたいで書かれた照会を、スナップショット時の strip だけが除去する。

決定記録には、発生条件と、この修正の範囲外である理由を書く。

### 4.3 エッジケース

| ID | 内容 |
|----|------|
| EC-1 | 除去対象の構文が別の呼び出しで完成する場合（`ESC[6` を呼び出し 1、`ESC]777…` を pending に保持、`BEL` を呼び出し 2）。FR2 の状態は開いた CSI のまま。 |
| EC-2 | 除去対象の後に、カットで落とす未完の構文が続く場合（`ESC[6` + 完全な起動列 + `ESC]0;t` + カット）。DEL が書かれる。 |
| EC-3 | 除去された CSI 照会に C0 が埋め込まれている場合（strip は C0 を再出力する）。CSI は開いたままと判定する。 |
| EC-4 | 除去されない ESC 系の構文（`ESC]0;t BEL`、`ESC X` など）が CSI を中断した場合は、従来どおり DEL を書かない。 |
| EC-5 | 生ストリームの基準との比較では、strip が除去する構文の効果（生ストリームでの `ESC[6n` への正当な CPR、Kitty APC の応答や画像配置）が基準側にだけ現れる。比較は後続バイトの解釈と応答に絞るか、その効果を出さないペイロードを選ぶ。 |
| EC-6 | 前身フィーチャーの FR3 の、指定子 ESC と DEL が同時に書かれない不変条件を保つ。 |

## 5. 非機能要件

### 5.1 パフォーマンス要件

- NFR2（リーダー通常経路の負荷）: リーダーの通常経路にバイト走査のパスを追加しない。除去後の CSI 状態は、既存の strip のパスか scan_boundary のパスの中で O(1) の状態として求める。

### 5.2 セキュリティ要件

- NFR3（セキュリティ（TM-1））: カットとその状態引き継ぎの経路で、クライアントが開始していない問い合わせを、リングのリプレイで成立させない。
- NFR4（セキュリティ（TM-2））: 走査と strip は有界で、panic しない。512 KiB の pending 上限と、strip を通すオーバーフローフラッシュは維持する。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

- ドキュメント: 指摘の判断（FR5）と残件（FR6）を、このフィーチャーの決定記録に書く。

### 5.5 互換性要件

- NFR1（互換性）: mux_ipc のワイヤ形式、Snapshot / SnapshotRestore のフレーム形状、スナップショットのバイト配置、共有 strip の出力は変えない。リングの内容が変わるのは、FR1-FR3 の場合に DEL が 1 バイト加わることだけとする。
- NFR5（ビルドとプラットフォーム）: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。Linux と Windows で挙動は同じ。

## 6. UI/UX要件

該当なし。UI に触れない Rust 内部の変更（mux デーモンの書き込みフィルタ）であり、デザインステップは実施しない。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。mux_ipc のワイヤ形式は変えない（NFR1）。

## 9. 制約条件

### 9.1 技術的制約

- 前身の不変条件を引き継ぐ。ロック順、mux_ipc のワイヤ形式、512 KiB の pending 上限、閉鎖バイトとしての DEL（CSI_CLOSING）、指定子 ESC と閉鎖が同時に書かれないこと、リーダーの通常経路にバイト走査のパスを追加しないこと（as-01）。
- 修正範囲は cut-and-carry とする。共有 strip の出力は変えず、カットの無い連結は残件として記録する（as-04）。

### 9.2 ビジネス上の制約

- 決定記録は `feature-docs/mux-cut-csi-post-strip-closure/` 配下に置く。前身の `reviews/round1.yaml` と `DECISIONS.md` は変更しない（as-02）。
- 既存テストの改名は想定しない。改名が生じた場合は、`.claude/rules/test-docs-records.md` に従って前身の test-docs を更新する（as-03）。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-cut-csi-post-strip-closure/**`
- `test-docs/mux-cut-csi-post-strip-closure/**`

`feature-docs/mux-cut-csi-post-strip-closure/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-cut-csi-post-strip-closure/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-cut-csi-post-strip-closure/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/mux-cut-csi-post-strip-closure/` ディレクトリを生成しないが、宣言された `test-docs/mux-cut-csi-post-strip-closure/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| カットの無い strip 連結（FR6 の 3 件） | 記載なし | 修正せず、決定記録に発生条件と範囲外の理由を書き、別タスクで扱う（FR6） |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1（FR1, FR2, NFR3）: 3 形式それぞれで、カット（同一呼び出しのカット・フォールバック閉鎖）の後に `n` を続けたリングを term_core でリプレイしても CPR 応答は出ず、`n` は文字として表示される。
- [ ] AC-2（FR1, FR2, FR4）: `CLOSING_CASES` に追加した 3 ケースで、既存の経路（末尾のカット、同じ位置への 3 回のカット、1 バイトずつ、フォールバック閉鎖、2 回目の閉鎖が何も書かないこと）がすべて期待どおりになる。
- [ ] AC-3（FR2）: カットの無い呼び出しの後、`csi_phase()` は除去後の出力の状態を返す（`ESC[6` + 完全な起動列の後は `Some(Param)`）。分割位置を問わず、また 1 バイトずつ渡しても、出力バイトと状態は 1 回で渡した結果と等しい。
- [ ] AC-4（FR3）: オーバーフローフラッシュの後のカットでも、除去後の出力が開いた CSI で終われば DEL が 1 回書かれる。
- [ ] AC-5（FR4）: 修正前のコードで、追加した回帰テストが失敗し、修正後に通る。
- [ ] AC-6（FR5, FR6）: 決定記録に `4c0ad9058a983648` の判断・理由・回帰テストと、FR6 の残件（発生条件と範囲外の理由）がある。前身の `reviews/round1.yaml` と `DECISIONS.md` は変更されていない。
- [ ] AC-7（NFR1）: 既存テストは変更なしで通る。意図して期待値を変えたテストは決定記録に列挙する。改名があれば `.claude/rules/test-docs-records.md` に従って前身の test-docs を更新する。
- [ ] AC-8（NFR5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と、`--no-default-features` の cargo check が通る。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1（unit / FR1, FR2, FR4）: `CLOSING_CASES` に 3 形式を追加し、末尾のカット・3 回のカット・1 バイトずつ・フォールバック閉鎖のすべてで、出力が `ESC[6` + DEL になり、状態がクリアされることを確認する。
- [ ] TS-2（unit / FR1, FR2, FR4, NFR3）: term_core 生ストリーム比較で、3 形式 + 画面切り替えの組 + 後続（`n` ほか）の場合を検証する。同一呼び出しのカットとフォールバック閉鎖の両方で、後続の解釈と応答が基準と等しいことを確認する。strip が設計上除去する構文そのものの効果は、比較から切り離す。
- [ ] TS-3（unit / FR2）: 除去対象を含む入力で、すべての分割位置と 1 バイトずつの供給について、出力と状態（pending、指定子待ち、csi）が 1 回で渡した結果と等しいことを確認する。
- [ ] TS-4（unit / FR3）: 上限付近まで保持した OSC がフラッシュされ、その run が `ESC[6` + 完全な除去対象で終わり、カットが続く場合を検証する。DEL が書かれ、後続の `n` で CPR が成立しないことを確認する。
- [ ] TS-5（integration / FR1, FR2, NFR3）: 本番のリーダーと可視性復元のハーネス（`run_visibility_restore_at`）で、`ESC[6` + 除去対象 + 画面切り替え、その後の読み取りで `n`、という順にデータを流す。クライアントの応答・画面・カーソルが生ストリームの基準と等しいことを確認する。
- [ ] TS-6（unit / FR5, FR6）: 決定記録に `4c0ad9058a983648` の行と FR6 の残件があることを確認する。
- [ ] TS-7（performance / NFR2, NFR4）: 除去対象と開いた CSI を交互に並べた長い入力でも、予算内に終わり panic しないことを確認する。
- [ ] TS-8（build / NFR5）: `--no-default-features` の cargo check が通ることを確認する。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| CSI_CLOSING | 閉鎖バイトとしての DEL |
| 共有の strip | `strip_rich_content_and_remap_with_designator`。スナップショット経路を含む |
| フォールバック閉鎖 | 空範囲 + fed 0 のカット |
| pending 上限 | 512 KiB |

## 14. 確認事項

### 14.1 確認済み事項

- [x] requirement.scope.no-cut-strip-merge: cut-and-carry（batch-codex-consultation）。修正範囲は cut-and-carry とし、共有 strip の出力は変えず、カットの無い連結は残件として記録する。
- [x] design-step: skip（batch-decision-table）。UI に触れない Rust 内部の変更（mux デーモンの書き込みフィルタ）。

### 14.2 前提

| ID | 前提 | 理由 | 影響度 | 可逆 |
|----|------|------|--------|------|
| as-01 | 前身の不変条件を引き継ぐ。ロック順、mux_ipc のワイヤ形式、512 KiB の pending 上限、閉鎖バイトとしての DEL（CSI_CLOSING）、指定子 ESC と閉鎖が同時に書かれないこと、リーダーの通常経路にバイト走査のパスを追加しないこと。 | 前身 SPEC の NFR1/NFR3/NFR5 と DECISIONS の FR4 修正内容が前提になる。 | medium | 可 |
| as-02 | 決定記録は `feature-docs/mux-cut-csi-post-strip-closure/` 配下に置く。前身の `reviews/round1.yaml` と `DECISIONS.md` は変更しない。 | 前身フィーチャーも、さらに前のフィーチャーのレビュー記録を変更しなかった。`test-docs-records.md` も、前身の feature-docs の本文を更新義務の対象外としている。 | low | 可 |
| as-03 | 既存テストの改名は想定しない。改名が生じた場合は、`test-docs-records.md` に従って前身の test-docs を更新する。 | DoD は既存のテーブルと比較へのケース追加を求めている。 | low | 可 |
| as-04 | 修正範囲は cut-and-carry とする。共有 strip の出力は変えず、カットの無い連結は残件として記録する。 | requirement.scope.no-cut-strip-merge への回答（batch-codex-consultation, cut-and-carry）による。 | medium | 可 |

### 14.3 未確認・保留事項

なし。

## 15. 参考資料

- 前身フィーチャーのレビュー記録: `feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml`
- 前身フィーチャーの決定記録: `feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md`
- test-docs 記録のルール: `.claude/rules/test-docs-records.md`
- 回帰テストの追加先: `src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs`
