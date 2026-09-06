---
title: "ring-push-blank-note-scope"
created_date: 2026-09-06
status: draft
---

# ring-push-blank-note-scope - 要件定義書

## 1. 概要

### 1.1 背景

`crates/term_core/src/ring_buffer/tests.rs` のテスト
`test_ring_push_blank_clears_recycled_row_overflow_entries` に付随する NOTE
コメントが、`ring_push_blank` の Step 3 全体を「常に no-op」と読める結論文で
締めくくられている。実際に冗長なのは Step 3 のうち `overflow` /
`overflow_ridx` のクリア対のみで、セル塗りつぶしと `ring_wrapped` のリセットは
退避側に対応物を持たない必須処理である。

### 1.2 目的

- NOTE の冗長性の記述を、テストが実際に固定している範囲に限定し、将来の保守者が
  「Step 3 は常に no-op」を `ring_push_blank` の Step 3 ブロックごと削除してよい
  根拠として読めないようにする。
- NOTE の正しい中核主張（1 行以上を push するたびに `new_bottom_abs ==
  evicted_abs` が成立すること）を保持したうえで、そこから導かれていた過度に広い
  結論のみを取り除く。

### 1.3 スコープ

`crates/term_core/src/ring_buffer/tests.rs` 内のコメント文のみを対象とする。
プロダクションコード（`ring_buffer.rs`）、アサーション、フィクスチャ、テスト名は
変更せず、テストの追加・削除も行わない。

## 2. ビジネス要件

### 2.1 ビジネス目標

- `test_ring_push_blank_clears_recycled_row_overflow_entries` の NOTE における
  冗長性の記述を、テストが実際に固定している範囲に限定する。将来の保守者が
  「Step 3 は常に no-op」を `ring_push_blank` の Step 3 ブロック全体を削除して
  よい許可として読めないようにする。
- NOTE の正しい中核主張（1 行以上の push ごとに `new_bottom_abs ==
  evicted_abs`）を保持しつつ、そこから導かれた過度に広い結論を取り除く。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| `term_core` の保守者 | `ring_push_blank` とそのテストを将来読み、変更する開発者 |

### 2.3 期待される効果

- Step 3 の各処理が「冗長」か「必須」かがコメント上で正しく切り分けられる。
- Step 3 ブロック全体の削除を正当化する誤読を防げる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | NOTE を読んで Step 3 の各処理の要否を判断する | `term_core` の保守者 | 高 |

### 3.2 ユースケース詳細

#### UC01: NOTE を読んで Step 3 の各処理の要否を判断する

**アクター**: `term_core` の保守者

**事前条件**:
- `crates/term_core/src/ring_buffer/tests.rs` の
  `test_ring_push_blank_clears_recycled_row_overflow_entries` に NOTE が存在する。

**基本フロー**:
1. 保守者が NOTE の冗長性に関する結論文を読む。
2. 冗長なのは Step 3 の `overflow` / `overflow_ridx` クリア対のみであることを
   読み取る。
3. セル塗りつぶしと `ring_wrapped[new_bottom_abs] = false` は退避側に対応物が
   なく必須であることを読み取る。

**事後条件**:
- 保守者は Step 3 ブロック全体を削除可能とは判断しない。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 「常に no-op」の結論を overflow クリア対に限定する | 結論文の適用範囲を狭める | 高 |
| FR2 | セル塗りつぶしと `ring_wrapped` リセットが冗長でないことを記録する | 1〜2 行を追記する | 高 |
| FR3 | NOTE の依然として正しい論証を保持する | 既存本文をそのまま残す | 高 |
| FR4 | コメントのみの変更をテストファイル内に限定する | 変更範囲を限定する | 高 |

### 4.2 機能詳細

#### FR1: 「常に no-op」の結論を overflow クリア対に限定する

**説明**: NOTE の結論文（現状 `crates/term_core/src/ring_buffer/tests.rs:595-597`
の "The new-bottom-row clear is therefore always a no-op within a single push"）
は、Step 3 ブロック全体ではなく、Step 3 の `overflow` / `overflow_ridx` クリア対
のみが退避時のクリアと冗長である旨を述べること。

**ステータス**: resolved

#### FR2: セル塗りつぶしと `ring_wrapped` リセットが冗長でないことを記録する

**説明**: Step 3 のセル塗りつぶし（`slice.fill(Cell::EMPTY)` / BCE、
`ring_buffer.rs:211-217`）と `self.ring_wrapped[new_bottom_abs] = false`
(`ring_buffer.rs:219`) には退避側に対応物がなく、したがって冗長ではなく必須で
あることを述べる 1〜2 行を追記する。

**ステータス**: resolved

#### FR3: NOTE の依然として正しい論証を保持する

**説明**: 2 つのクリア地点、評価順序（`evicted_abs` は Step 1 の前に取得、
`ring_head` は Step 2 で回転、`new_bottom_abs` は Step 3 で導出）、および結果と
しての同一リングスロット同一性に関する既存の記述はそのまま残す。narrow するのは
結論のみで、新しい文は追記する。

**ステータス**: resolved

#### FR4: コメントのみの変更をテストファイル内に限定する

**説明**: 変更は `crates/term_core/src/ring_buffer/tests.rs` 内のコメント文のみに
触れる。プロダクションコード（`ring_buffer.rs`）、アサーション、フィクスチャ、
テスト名はいずれも変更せず、テストの追加・削除も行わない。

**ステータス**: resolved

## 5. 非機能要件

### 5.1 パフォーマンス要件

対象外。

### 5.2 セキュリティ要件

対象外。

### 5.3 可用性要件

対象外。

### 5.4 保守性要件

- NFR2: 文言は英語で、`ring_buffer/tests.rs` の周辺コメントの語調（宣言的で、
  Step 1 / Step 2 / Step 3 を `ring_buffer.rs` で使われている名前で参照する）に
  合わせる。

### 5.5 互換性要件

- NFR1: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` はクリーン
  なまま。追記するコメント行は周辺 NOTE の折り返し幅（インデントされた `//`
  ブロック内でおよそ 72 桁）に従う。
- NFR3: 挙動変更ゼロ。クレートの観測可能な挙動およびテストのアサーション集合は
  変更前後で同一。

## 6. UI/UX要件

対象外。UI 面はない。

## 7. データ要件

対象外。データモデルの変更はない。

## 8. 外部連携

対象外。

## 9. 制約条件

### 9.1 技術的制約

- 変更はコメントのみ。プロダクションコード（`ring_buffer.rs`）は変更しない。
- テストの追加・削除、アサーション・フィクスチャ・テスト名の変更を行わない。

### 9.2 ビジネス上の制約

対象外。

### 9.3 スケジュール制約

対象外。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各
タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**:
- `feature-docs/ring-push-blank-note-scope/**`
- `test-docs/ring-push-blank-note-scope/**`

この宣言はスーパーセットの主張であり、実際の変更集合は宣言に含まれる必要がある。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| Step 3 全体を no-op と読む誤読が残る | 中 | FR1 で結論を overflow クリア対に限定し、FR2 で必須処理を明記する |

### 10.2 ビジネスリスク

対象外。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: NOTE の「常に no-op」という記述が `overflow` / `overflow_ridx` の
      クリアに限定され、Step 3 ブロック全体に関する主張として読めなくなっている。
- [ ] AC2: NOTE が、セル塗りつぶしと `ring_wrapped[new_bottom_abs] = false` の
      リセットには退避側の対応物がなく、したがって必要であることを述べている。
- [ ] AC3: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
      crates/term_core/Cargo.toml --lib` が green。
- [ ] AC4: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` が
      差分なしを報告する。

### 11.2 KPI

対象外。

## 12. テストシナリオ

### 12.1 テスト観点

| ID | 対象 | 種別 | 内容 |
|----|------|------|------|
| TS1 | AC3 | automated | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` — `test_ring_push_blank_clears_recycled_row_overflow_entries` を含む term_core lib スイート全体 |
| TS2 | AC4 | automated | `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` |
| TS3 | AC1, AC2 | review | 改訂後の NOTE を `ring_push_blank` の Step 1 の各分岐（`ring_buffer.rs:135-200`）および Step 3（`ring_buffer.rs:206-224`）と突き合わせ、Step 3 の各処理が正しく帰属されていること（冗長 = overflow クリア対、必須 = セル塗りつぶしと `ring_wrapped` リセット）を確認する |
| TS4 | FR4 | review | 差分が `crates/term_core/src/ring_buffer/tests.rs` のコメント行のみであること |

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| Step 1 / Step 2 / Step 3 | `ring_buffer.rs` の `ring_push_blank` 内で使われている段階の呼称 |
| `evicted_abs` | Step 1 の前に取得される、退避される行の絶対インデックス |
| `new_bottom_abs` | Step 3 で導出される、新しい最下行の絶対インデックス |
| overflow クリア対 | Step 3 における `overflow` / `overflow_ridx` のクリア処理 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] Step 1 の 3 分岐（`scrollback_bypass` 136-156、`scrollback_capacity > 0`
      157-192、無効時 193-200）はいずれも `evicted_abs` について `overflow` /
      `overflow_ridx` のみをクリアする。タスク記述の主張はソース上正確。
- [x] `ring_wrapped` は関数内で `ring_buffer.rs:219` の 1 箇所でのみ書き込まれる。
      181 行目（`let wrapped = self.ring_wrapped[evicted_abs];`）は
      `scrollback_wrapped` に渡すための読み取りであってリセットではないため、退避
      側に `ring_wrapped` の対応物はない。
- [x] セルのクリアは Step 3（`ring_buffer.rs:211-217`）でのみ行われ、Step 1 の
      いずれの分岐も `ring_cells` を書かない。
- [x] NOTE の中核前提は成立する。`rows >= 1` に対して
      `((ring_head + 1) % rows + rows - 1) % rows == ring_head` であり、
      `new_bottom_abs == evicted_abs`。この記述は保持する。
- [x] スコープは NOTE の文言のみ。タスクの「実害」節は Step 3 を丸ごと削除しても
      テストが green のままであることを指摘しているが、記載された期待挙動は
      コメントの修正であってセル塗りつぶし / `ring_wrapped` リセットを固定する
      新規テストの追加ではない。そのようなテストは追加しない。
- [x] タスクの「該当箇所」の行番号（`tests.rs:528-530`）は古い。現在のワーク
      ツリーでは NOTE は 584-597 行に跨り、結論文は 595-597 行にある。528-530 行は
      フィクスチャ生成コード。

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- `crates/term_core/src/ring_buffer/tests.rs`: 対象の NOTE を含むテストファイル
- `crates/term_core/src/ring_buffer.rs`: `ring_push_blank` の Step 1〜Step 3
