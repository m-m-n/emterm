---
title: "relocate-wrap-ec1-scroll-test"
created_date: 2026-08-15
status: draft
---

# relocate-wrap-ec1-scroll-test - 要件定義書

## 1. 概要

### 1.1 背景

本機能は、feature `relocate-wrap-overflow-cleanup` のレビュー指摘
`532f5e5cbe0763e7`（severity: medium、confidence: 65、記録先
`feature-docs/relocate-wrap-overflow-cleanup/reviews/round1.yaml`）に由来する。

指摘の内容は、EC1 のスクロール経路テスト
`test_relocate_widened_base_via_wrap_scrolls_without_panic_or_stale_entries`
（`crates/term_core/src/print_handler/tests.rs:1559`）が、テスト名・先頭コメント・
アサーションで overflow エントリのクリーンアップ特性を主張しているにもかかわらず、
そのテスト実行中は `core.overflow` が終始空であり、当該アサーションが空虚
（vacuous）であるという点である。

### 1.2 目的

- EC1 スクロール経路テストの名前・先頭コメント・アサーションを、そのテストが
  実際に証明していることだけを主張する内容に是正する。
- EC1 が覆っているように見えていた「退避時の overflow クリア」特性を、
  実際にその処理を除去すると落ちるテストで固定する。
- リロケーションの削除分岐（`print_handler.rs:493` / `print_handler.rs:518`）が
  スクロール経路では構造上到達しないことを、その機構とともに SPEC に事実として
  記録する。
- test-docs の記録を実態と一致させる。

### 1.3 スコープ

**対象**:

- `crates/term_core/src/print_handler/tests.rs` の EC1 テスト（名称・コメント・
  アサーション）
- `crates/term_core/src/ring_buffer/tests.rs` への新規テスト追加
- `feature-docs/relocate-wrap-ec1-scroll-test/` 配下の SPEC / 要件ドキュメント
- `test-docs/relocate-wrap-ec1-scroll-test/taskNNNN.tests.yaml`（新規、NNNN は
  plan フェーズで採番）
- `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` の AC-6 エントリ
  （59-74 行）の是正

**対象外**:

- プロダクションコードの変更（FR8）
- DECSTBM スクロールリージョン経路（`shift_rows_up`）のクリア箇所（前提 a2、FR6）
- `cols <= 2` のカーソル範囲外欠陥（レビュー指摘 `3e769a761d85d839`）

## 2. ビジネス要件

### 2.1 ビジネス目標

- EC1 スクロール経路テストの名前・先頭コメント・アサーションが、そのテストが
  実際に証明していることだけを主張するようにし、overflow クリーンアップ特性が
  そこで固定されていると読者が誤認しないようにする。
- `ring_push_blank` の退避時 overflow クリアを、その処理を除去すると実際に失敗
  するテストで固定し、EC1 が覆っているように見えた特性を実際に覆う。
- リロケーションの削除分岐（`print_handler.rs:493` / `518`）がスクロール経路では
  構造上到達しないことを、その機構とともに SPEC に事実として記録し、将来の読者に
  同じ指摘を再提起させない。
- test-docs の記録を実態と一致させる。本機能自身の記録を持ち、
  `relocate-wrap-overflow-cleanup` の記録に残った古い AC-6 の主張を是正する。
- プロダクション挙動を一切変えず、既存テスト、特に TS1
  (`test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`)
  を弱めない。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| term_core の開発者・レビュアー | テスト名と主張から、どの特性がどこで固定されているかを判断する |

### 2.3 期待される効果

- テストの主張とアサーションが一致し、誤認による誤った安心が解消される。
- `ring_push_blank` の退避時 overflow クリアが、red 確認済みのテストで固定される。
- 削除分岐のスクロール経路到達性に関する指摘が、SPEC の記録により再提起されない。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | EC1 テストの主張是正 | term_core の開発者 | 高 |
| UC02 | 退避時 overflow クリアの固定 | term_core の開発者 | 高 |
| UC03 | 到達不能性の SPEC 記録と test-docs 整合 | レビュアー | 中 |

### 3.2 ユースケース詳細

#### UC01: EC1 テストの主張是正

**アクター**: term_core の開発者

**事前条件**:

- `test_relocate_widened_base_via_wrap_scrolls_without_panic_or_stale_entries`
  が `tests.rs:1559` に存在する。

**基本フロー**:

1. テスト名を `test_relocate_widened_base_via_wrap_scrolls_without_panic` に変更する。
2. 先頭コメント（`tests.rs:1551-1557`）を、証明していること／証明していないことを
   述べる内容に書き換える。
3. 空虚なアサーション 2 本（`tests.rs:1579-1580`）と未使用になる `abs1` 束縛
   （`tests.rs:1578`）を削除する。

**事後条件**:

- テスト名にもコメントにもアサーションにも overflow エントリに関する主張が残らない。

#### UC02: 退避時 overflow クリアの固定

**アクター**: term_core の開発者

**事前条件**:

- `ring_push_blank` が退避時に overflow / overflow_ridx をクリアしている。

**基本フロー**:

1. `crates/term_core/src/ring_buffer/tests.rs` に
   `test_ring_push_blank_clears_recycled_row_overflow_entries` を追加する。
2. `TerminalCore::new(5, 2, 0)` を構築し、viewport 行 0 の桁 0 / 桁 1 に
   ベース文字 + 結合マーク 8 個（0x0301..0x0308、17 UTF-8 バイト）を書き込む。
3. `overflow` と `overflow_ridx` にエントリがあることを事前アサートする。
4. 最終行にカーソルを置き、素の line feed を発行して全画面スクロールを起こす。
5. 該当スロットのキーが消えていることを事後アサートする。

**事後条件**:

- 未改変コードでテストが green になる。

**代替フロー**:

- red 確認では `ring_buffer.rs:196-199` と `ring_buffer.rs:221-224` の両クリア箇所を
  同時に除去し、失敗を観測する（FR5）。

#### UC03: 到達不能性の SPEC 記録と test-docs 整合

**アクター**: レビュアー

**基本フロー**:

1. SPEC が `print_handler.rs:493` / `518` の到達不能性を、3 部構成の機構と
   file:line 根拠つきで述べていることを確認する。
2. 本機能の tests.yaml 記録が存在し、全 AC をテストに対応づけていることを確認する。
3. `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` の AC-6 が
   改名後のテスト名と是正後の `red_reason` を持ち、他エントリが無改変であることを
   確認する。

**事後条件**:

- 文書レビューで確認が完了する。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | EC1 スクロール経路テストの改名 | 証明している主張に合わせた名前へ変更 | 高 |
| FR2 | EC1 先頭コメントの書き換え | 証明していること／していないことを明記 | 高 |
| FR3 | EC1 の空虚なアサーション削除 | 空虚な 2 本と未使用束縛を削除 | 高 |
| FR4 | 退避時 overflow クリアを固定するテスト追加 | `ring_push_blank` を直接の対象とする | 高 |
| FR5 | 新テストの red 基準（退避経路へ再設定） | 2 箇所同時除去で red を確認 | 高 |
| FR6 | 削除分岐の到達不能性を SPEC に記録 | 機構と file:line 根拠つきで事実として記載 | 中 |
| FR7 | test-docs の整合 | 自機能の記録作成と旧記録 AC-6 の是正 | 中 |
| FR8 | プロダクションコード無変更 | テスト・文書・記録のみを変更 | 高 |

### 4.2 機能詳細

#### FR1: EC1 スクロール経路テストの改名

**説明**: `test_relocate_widened_base_via_wrap_scrolls_without_panic_or_stale_entries`
（`crates/term_core/src/print_handler/tests.rs:1559`）を
`test_relocate_widened_base_via_wrap_scrolls_without_panic` に改名する。

**ビジネスルール**:

- `_or_stale_entries` 接尾辞を落とす理由は、当該テストが overflow エントリに関する
  特性を一切観測できないためである。テスト全体を通じて `core.overflow` は空であり、
  書き込むのは 'A'..'D' と 4 バイトの VS16 マージだけで、いずれも 16 バイトの
  インライン上限を超えない。

**ステータス**: resolved

#### FR2: EC1 先頭コメントの書き換え

**説明**: `tests.rs:1551-1557` の先頭コメントを差し替える。

**ビジネスルール**:

- (a) 証明していること: リロケーションの line feed 自体がビューポートをスクロール
  し得ること、および再配置されたベース + スペーサの書き込みが、panic も範囲外読み
  も起こさずに解決済みの行に着地すること。
- (b) overflow クリーンアップについて何も証明しない理由: `ring_push_blank` が
  再利用スロットの overflow キーを、再配置書き込みの実行前に消去するため、削除分岐は
  この経路では発火し得ない。
- コメントは、ここで stale overflow エントリを検査していると主張しても示唆しても
  ならない。

**ステータス**: resolved

#### FR3: EC1 の空虚なアサーション削除

**説明**: `tests.rs:1579-1580` の空虚なアサーション 2 本
（`!core.overflow.contains_key(&(0u32, abs1))` /
`&(1u32, abs1)`）と、それにより未使用になる `let abs1 = core.viewport_abs(1) as u32;`
（`tests.rs:1578`）を削除する。

**残すアサーション**:

| 項目 | 期待値 |
|------|--------|
| スクロール後のカーソル行 | 1 に固定 |
| `get_cell_char(0,1)` | `"5\u{FE0F}"` |
| `get_cell_width(0,1)` | 2 |
| `get_cell_width(1,1)` | 0 |
| スクロール前のカーソル位置アサーション | 既存のまま |

**ビジネスルール**:

- 真偽がテスト対象コードから独立しているアサーションを、このテストに残してはならない。

**ステータス**: resolved

#### FR4: 退避時 overflow クリアを固定するテスト追加

**説明**: `crates/term_core/src/ring_buffer/tests.rs` に
`test_ring_push_blank_clears_recycled_row_overflow_entries` を追加する。
リロケーションは一切関与させない。

**処理フロー**:

```mermaid
flowchart TD
    A[TerminalCore::new(5, 2, 0)] --> B[viewport 行0 桁0/桁1 に<br/>ベース文字 + 結合マーク 0x0301..0x0308]
    B --> C[let abs0 = core.viewport_abs(0) as u32]
    C --> D[事前アサート:<br/>overflow に (0,abs0) と (1,abs0)<br/>overflow_ridx[&abs0] に 0 と 1]
    D --> E[カーソルを最終行へ]
    E --> F[素の line feed<br/>DECSTBM なし / VS16 なし / リロケーションなし]
    F --> G[全画面スクロール経路が<br/>ring_push_blank を呼ぶ]
    G --> H[事後アサート:<br/>overflow に (0,abs0)/(1,abs0) が無い<br/>overflow_ridx に abs0 が無い]
```

**ビジネスルール**:

- 桁 0 / 桁 1 はそれぞれベース文字 + 結合マーク 8 個（0x0301..0x0308）で 17 UTF-8
  バイトとなり、16 バイトのインライン上限を超えるため `overflow` と
  `overflow_ridx` に実際にエントリが載る。これは TS1 が `tests.rs:1457-1472` で
  使っているフィクスチャ形状と同じである。
- 配置先を `ring_buffer/tests.rs` の既存 `test_ring_push_blank_clears_ridx`
  （`ring_buffer/tests.rs:417`）の隣とするのは、テスト対象が print handler ではなく
  `ring_push_blank` だからである。

**ステータス**: resolved

#### FR5: 新テストの red 基準（退避経路へ再設定）

**説明**: red 基準は、`ring_push_blank` の overflow クリアを除去して FR4 のテストが
失敗することで満たされる。

**ビジネスルール**:

- `new_bottom_abs == evicted_abs` が常に成り立つ
  （`new_bottom = ((evicted+1) + rows - 1) % rows = evicted`）ため、`ring_buffer.rs`
  の 2 つのクリア箇所 — 対象とする scrollback 無効分岐の退避時クリア
  （`ring_buffer.rs:196-199`）と new-bottom クリア（`ring_buffer.rs:221-224`）— は
  同一のリングスロットを対象としており、相互に冗長である。
- したがって red 確認では両箇所を 1 回の変異で同時に除去する。片方のみの除去では
  テストが green のままであること、およびその理由を、記録に明記する。
- 既存のリロケーション削除分岐（`print_handler.rs:493` / `518`）は、no-scroll 経路で
  TS1 により引き続き固定される。本機能は
  `test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`
  のいかなるアサーションも変更・削除・弱体化してはならない。

**ステータス**: resolved

#### FR6: 削除分岐の到達不能性を SPEC に記録

**説明**: SPEC は `print_handler.rs:493` および `print_handler.rs:518` の削除分岐が
スクロール経路では発火し得ないことを、推測ではなく事実として、次の機構とともに
記載する。

**機構**:

1. `viewport_abs(row) = (ring_head + row) % rows`（`ring_buffer.rs:75-82`）は
   リングスロット添字を返すため、再配置書き込みが使う行キーは、`ring_push_blank` が
   たった今再利用したスロットそのものである。
2. `ring_push_blank` は退避時に、3 つの scrollback 分岐すべてで該当スロットの
   `overflow` / `overflow_ridx` キーをクリアし（`ring_buffer.rs:147-148`、`178-179`、
   `197-198`）、さらに新しいビューポート最下行を空白化する際にも再度クリアする
   （`ring_buffer.rs:222-223`）。
3. 再配置書き込みは `line_feed()` の返却後に走る（`print_handler.rs:464-521`）ため、
   その `!self.overflow.is_empty()` ガードが短絡するか、キーが単に存在しない。

**追加記載**:

- `shift_rows_up` 経由の DECSTBM スクロールリージョン経路は別のクリア箇所
  （`terminal_rows.rs:125-126`、`164-165`、`189-190`、`226-227`、`256-257`）であり、
  本機能のスコープ外であることも SPEC に記録する。

**ステータス**: resolved

#### FR7: test-docs の整合（決定済み）

**説明**: 本機能は自身の記録
`test-docs/relocate-wrap-ec1-scroll-test/taskNNNN.tests.yaml`（NNNN は plan フェーズで
採番）を、既存のタスク単位記録の慣行に従って保有する。

**加えて**: `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` の古い
AC-6 エントリ（59-74 行）をその場で是正する。

| 項目 | 是正内容 |
|------|----------|
| 先頭に列挙されているテスト名 | FR1 の名前に更新 |
| `red_reason` | スクロールケースの overflow アサーションが空虚であった（そのテストの実行全体で `core.overflow` が空）こと、それらを削除したこと、当該テストは現在 no-panic + 正しい配置のみを主張すること、`ring_push_blank` の退避時クリアは本機能の新テストで固定されることを述べる |
| `red_confirmed: false` | そのまま正しいので維持 |
| 2 番目に列挙されたテスト（`..._no_panic_when_column_one_does_not_exist`）と他の全 AC エントリ | 無改変 |

**ステータス**: resolved

#### FR8: プロダクションコード無変更

**説明**: 差分はテストコード、SPEC / feature-docs、test-docs のみに触れる。
`#[cfg(test)]` でないコード経路は一切変更しない。`print_handler.rs`、
`ring_buffer.rs`、`terminal_rows.rs` およびその他すべてのプロダクションモジュールは
バイト単位で同一のままとする。

**ステータス**: resolved

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし（本機能はランタイム挙動を変えない）。テストスイートへの実行時間追加は無視
できる範囲である（NFR3）。

### 5.2 セキュリティ要件

該当なし（テストコードと文書のみの変更）。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

- **NFR4 規約適合**: テスト名はクレートの `test_<subject>_<behavior>` 規約に従い、
  各テストは自身が覆う AC / 要件 ID を挙げた先頭コメントを持つ。これは
  `print_handler/tests.rs` および `ring_buffer/tests.rs` の周辺スタイルと一致する。
- **NFR5 フォーマット**: 変更した Rust ファイルは
  `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` を通過する。

### 5.5 互換性要件

- **NFR1 挙動保存**: ランタイム挙動の変更はゼロ。term_core の lib スイート全体が
  変更前後で通過し、通過するテスト集合は FR4 で追加される 1 本を除いて同一である。
- **NFR2 依存追加なし**: 新テストは近隣テストが既に使っているクレート内 API
  （`TerminalCore::new`、`handle_print`、`process_pty_data`、`viewport_abs`、
  `overflow`、`overflow_ridx`）のみを使う。`crates/term_core/Cargo.toml` に
  dev-dependency を追加しない。
- **NFR3 スイートの決定性**: 新テストは自前の `TerminalCore` を構築しプロセス
  グローバル状態に触れないため、既定のテストハーネス下で並列安全のままである。

## 6. UI/UX要件

該当なし。本機能は純 Rust クレート term_core 内のテストのみの変更であり、UI 表面も
視覚的成果物もユーザー可視の挙動も公開 API 変更も無い（design_step: skipped）。

## 7. データ要件

該当なし。永続データモデルの変更は無い。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- `new_bottom_abs == evicted_abs` が常に成り立つため、`ring_push_blank` の 2 つの
  クリア箇所は相互に冗長であり、外部から観測可能なテストでは両者を独立に固定できない。
  これが FR5 の red 基準が両箇所を同時に除去する理由である。
- `ring_buffer::tests::test_ring_push_blank_clears_ridx`
  （`ring_buffer/tests.rs:417`）が既に同じ特性を部分的に覆っているため、FR5 の red
  確認では 2 本のテストが red になる可能性が高い。SPEC はこの特性が従来まったく
  固定されていなかったと主張してはならない。
- プロダクションコードを変更できない（FR8）。

### 9.2 ビジネス上の制約

- 既存テストを弱めない。特に TS1
  (`test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`)
  は無改変であること。

### 9.3 スケジュール制約

該当なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 削除分岐の「実行された」の定義が曖昧 | 高 | 前提 a1 の定義（`remove(...)` が評価され、その削除が観測可能であること。ガード式に到達しただけでは不足）を採用する |
| 2 つのクリア箇所が冗長で独立に固定できない | 中 | FR5 の red 基準を両箇所同時除去とし、片方のみでは green である事実を記録に残す |
| 既存の `test_ring_push_blank_clears_ridx` も同時に red になる | 中 | 「従来まったく固定されていなかった」とは主張せず、記録に事実として残す |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 同じレビュー指摘が将来再提起される | 中 | 低 | FR6 により到達不能性とその機構を SPEC に事実として記録する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] **AC-1** (FR1, FR2): print_handler/tests.rs のスクロール経路テストが
      `test_relocate_widened_base_via_wrap_scrolls_without_panic` という名前を持ち、
      先頭コメントは no-panic / 範囲外アクセス無しと、解決済み行への再配置ベース・
      スペーサの正しい配置のみを主張する。名前もコメントも overflow エントリに関する
      特性を一切主張しない。
- [ ] **AC-2** (FR3): `!core.overflow.contains_key(...)` の 2 本と `abs1` 束縛が当該
      テストから消えており、残るいかなるアサーションも `core.overflow` が空の下で
      空虚でない。配置アサーション（カーソル行 1、`"5\u{FE0F}"`、幅 2 と 0）は残る。
- [ ] **AC-3** (FR4): `test_ring_push_blank_clears_recycled_row_overflow_entries` が
      存在し、`overflow` が `(0,abs0)` と `(1,abs0)` を保持し
      `overflow_ridx[&abs0]` が桁 0 と 1 を保持することを事前アサートし、
      リロケーション非関与の素の line feed でスクロールし、両キーが `overflow` から
      消え `abs0` が `overflow_ridx` から消えることを事後アサートする。未改変コードで
      通過する。
- [ ] **AC-4** (FR5): AC-3 のテストについて、`ring_push_blank` の
      `overflow_clear_row` / `overflow_ridx_clear_row` 呼び出しを両箇所
      （`ring_buffer.rs:196-199` と `221-224`）で除去して失敗アサーションを観測し、
      red が確認される。観測された失敗メッセージが `red_confirmed: true` とともに
      記録される。片方のみの除去ではテストが green のままであること、およびその理由
      （`new_bottom_abs == evicted_abs` により 1 回の push では 2 つのクリアが冗長）
      も記録に述べられる。
- [ ] **AC-5** (FR5, FR8):
      `test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`
      （TS1、`tests.rs:1454`）が無改変かつ green、
      `test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`
      が無改変かつ green、`git diff` に `#[cfg(test)]` でないソース行の変更が無い。
- [ ] **AC-6** (FR6): SPEC が FR6 の 3 部構成の機構と明示的な file:line 根拠つきで、
      `print_handler.rs:493` / `518` がスクロール経路では構造上到達不能であることを
      述べ、`shift_rows_up` のスクロールリージョン経路を別個かつスコープ外のクリア
      箇所として注記している。
- [ ] **AC-7** (FR7): `test-docs/relocate-wrap-ec1-scroll-test/taskNNNN.tests.yaml`
      が存在し上記の全 AC をそのテストに対応づけており、
      `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` の AC-6 が
      改名後のテスト名と、空虚さおよびその除去を説明する `red_reason` を持つ。
      同ファイルの他エントリは変更されていない。
- [ ] **AC-8** (NFR1, NFR5):
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      が green であり、
      `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` がクリーンである。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] **TS-1** (FR1, FR2, FR3 / AC-1, AC-2) —
      `print_handler::tests::test_relocate_widened_base_via_wrap_scrolls_without_panic`:
      5x2 端末、scrollback 無し。最終行にカーソル、'A'..'D' の後に最終桁へ '5'、
      続いて VS16。リロケーションの line feed がビューポートをスクロールする。
      panic しないこと、カーソルが行 1 に固定されること、再配置ベース／スペーサの
      配置をアサートする。overflow アサーションは持たない。
      *red 期待*: 欠陥固定ではなく、変更前後とも green（堅牢性 / リグレッション無し）。
      主張がアサーションと一致することはレビューで確認する。
- [ ] **TS-2** (FR4, FR5 / AC-3, AC-4) —
      `ring_buffer::tests::test_ring_push_blank_clears_recycled_row_overflow_entries`:
      5x2 端末、scrollback 容量 0。行 0 の桁 0 / 桁 1 に overflow 行きの幅 1 コンテンツ
      を事前充填し、`overflow` と `overflow_ridx` の双方に存在することを事前アサート。
      最終行からの素の line feed が全画面スクロールを起こす。以後、再利用スロットの
      キーが消えていること。
      *red 期待*: `ring_push_blank` の 2 つのクリア箇所を同時に除去して red 確認
      （片方のみの除去では green のまま — AC-4 参照）。
- [ ] **TS-3** (FR5, FR8 / AC-5) —
      `print_handler::tests::test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`
      （無改変）＋
      `print_handler::tests::test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`
      （無改変）: リグレッション。`print_handler.rs:493` / `518` の削除分岐が
      no-scroll 経路で本機能前とまったく同じに固定され続ける。
      *red 期待*: green。`relocate-wrap-overflow-cleanup` task0001 の AC-1 / AC-2 / AC-3
      で既に red 確認済みであり、ここでは再導出しない。
- [ ] **TS-4** (NFR1, NFR3, NFR5 / AC-8) — term_core のスイート全体 + フォーマット
      チェック。*red 期待*: green。
- [ ] **TS-5** (FR6 / AC-6) — 文書レビュー（SPEC.md）: 到達不能性の記述、3 部構成の
      機構、file:line 根拠が存在し正確であることをレビュアーが確認する。
      *red 期待*: 自動テスト無し。手動検証のみ。
- [ ] **TS-6** (FR7 / AC-7) — 文書レビュー（両 tests.yaml 記録）: 本機能自身の記録が
      存在し完全であること、`relocate-wrap-overflow-cleanup` の AC-6 エントリが
      改名後テスト名と空虚さ是正の説明を持ち、他エントリが無改変であることを
      レビュアーが確認する。
      *red 期待*: 自動テスト無し。手動検証のみ。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| EC1 | `relocate-wrap-overflow-cleanup` のエッジケース 1。本機能が是正対象とするスクロール経路テスト |
| TS1 | `test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`。no-scroll 経路で削除分岐を固定する既存テスト |
| 空虚（vacuous）なアサーション | 真偽がテスト対象コードから独立しているアサーション |
| インライン上限 | セルに直接格納できる 16 バイト。これを超えるとコンテンツが `overflow` に載る |
| 退避（eviction） | `ring_push_blank` がリングスロットを再利用すること |
| 削除分岐 | `print_handler.rs:493` / `518` の、stale な overflow エントリを `remove` する分岐 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 「削除分岐が実際に実行された」の定義（前提 a1、影響度: 高、可逆）:
      `print_handler.rs:493` / `518` の分岐が `remove(...)` を評価し、その削除が
      観測可能であることを指す。ガード式に到達しただけでは不足である。
- [x] EC1 が覆うべき経路（前提 a2、影響度: 中、可逆）: DECSTBM スクロールリージョン
      なしの全画面スクロール経路。`shift_rows_up` 経由のスクロールリージョン経路は
      別のクリア箇所でありスコープ外。
- [x] FR4 テストの配置（前提 a3、影響度: 低、可逆）: 対象が `ring_push_blank` である
      ため `crates/term_core/src/ring_buffer/tests.rs` の
      `test_ring_push_blank_clears_ridx` の隣に置く。print_handler/tests.rs へ移しても
      同様に機能し、他には何も変わらない。
- [x] 新しいテスト名（前提 a4、影響度: 低、可逆）:
      `test_relocate_widened_base_via_wrap_scrolls_without_panic` と
      `test_ring_push_blank_clears_recycled_row_overflow_entries` はクレート規約に
      適合する提案であり、改名する場合は両方の tests.yaml 記録に反映する。
- [x] FR4 のフィクスチャ（前提 a5、影響度: 低、可逆）: `TerminalCore::new(5, 2, 0)`
      （scrollback 無効 → `ring_buffer.rs` の第 3 退避分岐）と、スクロール契機として
      素の line feed を使う。3 つの scrollback 分岐のいずれでも成立するが、最も単純で
      EC1 自身のフィクスチャ形状と一致するため無効分岐を選ぶ。
- [x] design フェーズは skip（理由: 純 Rust クレート term_core 内のテストのみの変更で、
      UI 表面・視覚的成果物・ユーザー可視の挙動・公開 API 変更のいずれも無い。差分は
      テストコードと文書記録のみで、design フェーズが決めるべきものが無い）。

### 14.2 未確認・保留事項

なし。全要件が resolved である。

- test-docs のタスク番号 NNNN は plan フェーズで採番される（FR7）。

## 15. 参考資料

- レビュー指摘 `532f5e5cbe0763e7`（medium、confidence 65）:
  `feature-docs/relocate-wrap-overflow-cleanup/reviews/round1.yaml`
- 既存テスト記録: `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml`
- 対象テスト: `crates/term_core/src/print_handler/tests.rs`（EC1: 1551-1580、
  TS1: 1454、フィクスチャ形状: 1457-1472）
- 対象テスト: `crates/term_core/src/ring_buffer/tests.rs`（
  `test_ring_push_blank_clears_ridx`: 417）
- 実装参照: `crates/term_core/src/ring_buffer.rs`（`viewport_abs`: 75-82、
  退避時クリア: 147-148 / 178-179 / 196-199、new-bottom クリア: 221-224）
- 実装参照: `crates/term_core/src/print_handler.rs`（再配置書き込み: 464-521、
  削除分岐: 493 / 518）
- 実装参照: `crates/term_core/src/terminal_rows.rs`（`shift_rows_up` のクリア箇所:
  125-126 / 164-165 / 189-190 / 226-227 / 256-257）
