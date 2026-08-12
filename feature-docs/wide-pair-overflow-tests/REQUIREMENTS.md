---
title: "wide-pair-overflow-tests"
created_date: 2026-08-12
status: draft
---

# wide-pair-overflow-tests - 要件定義書

## 1. 概要

### 1.1 背景

feature wide-pair-overwrite-cleanup（PR #30）のレビュー round1 で、deferred finding
`06ef78e20e9b9f0b`（medium / comprehensive）としてカバレッジギャップが記録された。
wide ペア相方掃除の overflow 分岐（16 バイト超グラフェム、`char_len == 0xFF`）に
ユニットテストが無く、overflow テーブルと逆引き index（`overflow_ridx`）の同期が
壊れてもテストで検知できない状態にある。

### 1.2 目的

- wide ペア相方掃除の overflow 分岐にユニットテストを追加し、overflow テーブルと
  `overflow_ridx` の同期が壊れるリグレッションを検知可能にする。
- 上記 deferred finding `06ef78e20e9b9f0b` のカバレッジギャップを解消する。

### 1.3 スコープ

- 対象: `crates/term_core` のテストコード追加。
  - print 経路（`blank_wide_pair_partner`、`crates/term_core/src/print_handler.rs:74`）
  - DCH 経路（`handle_delete_characters`、`crates/term_core/src/csi_edit.rs`）
  - ECH 経路（`handle_erase_characters`、`crates/term_core/src/csi_screen.rs`）
  - 上記 2 経路が共有する `blank_wide_pair_split`
    （`crates/term_core/src/csi_edit.rs:161`）
- 対象外: `crates/term_core` の非テストコードの変更。新規テストフレームワーク・
  dev-dependency の導入。

## 2. ビジネス要件

### 2.1 ビジネス目標

- wide ペア相方掃除の overflow（16 バイト超グラフェム、`char_len == 0xFF`）分岐に
  ユニットテストを追加し、overflow テーブルと逆引き index（`overflow_ridx`）の
  同期が壊れるリグレッションを検知可能にする。
- feature wide-pair-overwrite-cleanup（PR #30）レビュー round1 の deferred finding
  `06ef78e20e9b9f0b`（medium / comprehensive）のカバレッジギャップを解消する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| `crates/term_core` の開発者 | wide ペア相方掃除・overflow テーブル周辺を変更する際に、リグレッションをテストで検知する |

### 2.3 期待される効果

- overflow テーブルと `overflow_ridx` の同期崩れがテストで検知される。
- deferred finding `06ef78e20e9b9f0b` のカバレッジギャップが解消される。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | overflow 分岐のリグレッション検知 | `crates/term_core` の開発者 | 高 |

### 3.2 ユースケース詳細

#### UC01: overflow 分岐のリグレッション検知

**アクター**: `crates/term_core` の開発者

**事前条件**:
- PR #30 がマージ済みで、`blank_wide_pair_partner` / `blank_wide_pair_split` が
  integration worktree に存在する。

**基本フロー**:
1. 開発者が `crates/term_core` の wide ペア相方掃除・overflow テーブル周辺を変更する。
2. `cargo test --manifest-path crates/term_core/Cargo.toml --lib` を実行する。
3. 追加されたテストが overflow 分岐（`char_len == 0xFF`）を通り、掃除結果と
   overflow / `overflow_ridx` の同期を検証する。

**代替フロー**:
- overflow テーブルと `overflow_ridx` の同期が壊れている場合、該当テストが失敗する。

**事後条件**:
- overflow 分岐の掃除挙動が回帰していないことが確認される。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | print 経路の overflow 相方掃除テスト | `blank_wide_pair_partner` 経由の spacer 掃除を検証 | 高 |
| FR2 | DCH / ECH 経路の overflow 相方掃除テスト | `blank_wide_pair_split` 経由の base 掃除を検証 | 高 |
| FR3 | overflow 分岐の実行証明 | `char_len == 0xFF` 分岐を通ったことを assert で証明 | 高 |
| FR4 | 既存テストの維持 | 既存テストが引き続き全件通る | 高 |

### 4.2 機能詳細

#### FR1: print 経路の overflow 相方掃除テスト

**説明**: 16 バイト超・幅 2 の ZWJ グラフェムクラスタ（例: 家族絵文字 👨‍👩‍👧‍👦、25 バイト）を
col0 に書き、base セルが overflow テーブル行き（`char_len == 0xFF`）になることを
確認したうえで col0 を ASCII で上書きし、`blank_wide_pair_partner`
（`crates/term_core/src/print_handler.rs:74`）経由で col1 の spacer が
`get_cell_char == " "` かつ `width == 1` になることを assert するテストを追加する。

**入力**:
- グラフェムクラスタ: ZWJ 家族絵文字（16 バイト超・幅 2）- col0 に print
- 上書き文字: ASCII 1 文字 - col0 に print

**出力**:
- col1 の `get_cell_char`: `" "`
- col1 の `width`: `1`

**ビジネスルール**:
- テスト対象は `blank_wide_pair_partner`（`print_handler.rs:74`）。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| base セルが overflow 行きにならない | grapheme merge / 幅計算が ZWJ 家族絵文字を幅 2 の 1 クラスタとして扱わない | 幅 2 かつ 16 バイト超の別グラフェムクラスタを選定する |

#### FR2: DCH / ECH 経路の overflow 相方掃除テスト

**説明**: 同じ ZWJ ペアの spacer 位置にカーソルを置いて DCH
（`handle_delete_characters`、`csi_edit.rs`）および ECH
（`handle_erase_characters`、`csi_screen.rs`）を実行し、`blank_wide_pair_split`
（`crates/term_core/src/csi_edit.rs:161`）経由で col-1 が `get_cell_char == " "`
（空文字ではない）を返すことを assert するテストを追加する。

**入力**:
- カーソル位置: ZWJ ペアの spacer 位置
- 操作: DCH(1) / ECH(1)

**出力**:
- col-1 の `get_cell_char`: `" "`（空文字ではない）

**ビジネスルール**:
- 「csi_edit 経路」は DCH（`csi_edit.rs`）と ECH（`csi_screen.rs`）の両方を指す。

#### FR3: overflow 分岐の実行証明

**説明**: 追加テストは掃除前の対象セルが実際に overflow 状態であること
（`cell.is_overflow()` / overflow テーブルへのエントリ存在）を事前 assert し、
掃除後に overflow と `overflow_ridx` の両方からエントリが除去されていることを
assert することで、`char_len == 0xFF` 分岐を通ったことを証明する。

**出力**:
- 掃除前: `cell.is_overflow()` が真 / overflow テーブルに該当エントリが存在
- 掃除後: overflow・`overflow_ridx` の双方から該当エントリが除去されている

#### FR4: 既存テストの維持

**説明**: 既存の `term_core --lib` テストおよび workspace テストが引き続き全件通る。

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし。

### 5.2 セキュリティ要件

該当なし。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

#### NFR1: 既存テスト規約への準拠

テストは既存の inline `#[cfg(test)]` モジュール
（`crates/term_core/src/print_handler/tests.rs`、`csi_edit.rs` / `csi_screen.rs` の
`mod tests`）に置き、命名は既存の `<subject>_<scenario>_<expected>` パターンに
合わせる。新規テストフレームワーク・dev-dependency（proptest 等）は導入しない。

#### NFR2: プロダクションコード非変更

本 feature はテスト追加のみで、`crates/term_core` の非テストコードは変更しない。

### 5.5 互換性要件

該当なし。

## 6. UI/UX要件

該当なし。UI・視覚的出力・ユーザー向け挙動の変更は無い（design ステップは skipped）。

## 7. データ要件

### 7.1 データモデル概要

本 feature で扱うデータ構造は既存のものに限る。

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| セル | `char_len` | - | ○ | `0xFF` のとき overflow テーブル参照 |
| グリッド | overflow | テーブル | ○ | 16 バイト超グラフェムの実体を保持 |
| グリッド | `overflow_ridx` | 逆引き index | ○ | overflow テーブルへの逆引き |

### 7.3 データ保持期間

該当なし。

## 8. 外部連携

### 8.1 連携システム

該当なし。

### 8.2 API仕様要件

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 新規テストフレームワーク・dev-dependency（proptest 等）を導入しない（NFR1）。
- `crates/term_core` の非テストコードを変更しない（NFR2）。
- テストは既存の inline `#[cfg(test)]` モジュールに置く（NFR1）。

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| ZWJ 家族絵文字が幅 2 の 1 クラスタとして扱われない可能性 | 中 | 幅 2 かつ 16 バイト超の別グラフェムクラスタを選定する（要件の本質は「overflow 行きの幅 2 base」であり特定の絵文字ではない） |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: print 経路のテストが存在し、ZWJ 家族絵文字を col0 に書いて base が
      overflow 行きになることを確認したうえで、col0 の ASCII 上書き後に col1 の
      spacer が `" "` / width 1 であることを assert している。
- [ ] AC2: `csi_edit` / `csi_screen` 経路のテストが存在し、同じ ZWJ ペアの spacer
      位置で DCH / ECH を実行後、col-1 の `get_cell_char` が `" "`（空文字ではない）を
      返すことを assert している。
- [ ] AC3: 追加テストが overflow 分岐（`char_len == 0xFF`）を実際に通ることが assert で
      確認されている（掃除前の overflow 状態の事前 assert を含む）。
- [ ] AC4: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      および workspace のテストが全件通る。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS1（print / FR1・FR3）: 家族絵文字を (0,row) に書く → overflow に (0,abs) が
      存在・base の `is_overflow()` が真であることを事前 assert → col0 に ASCII を
      print → col1 が `" "` / width 1、overflow・`overflow_ridx` から該当エントリが
      消えている。
- [ ] TS2（print / FR1・FR3）: 家族絵文字ペアの spacer（col1）を ASCII で上書き →
      col0（overflow base）が `" "` / width 1 になり overflow・`overflow_ridx` 同期が
      保たれる。
- [ ] TS3（DCH / FR2・FR3）: spacer 位置（col1）にカーソルを置き
      `handle_delete_characters(1)` → col0 の `get_cell_char == " "`
      （`unwrap_or_default` の空文字ではない）、overflow エントリ除去済み。
- [ ] TS4（ECH / FR2）: spacer 位置にカーソルを置き `handle_erase_characters(1)` →
      start-1 = col0 が `" "` を返す（`csi_screen.rs:155` の `blank_wide_pair_split`
      呼び出しを通る）。
- [ ] TS5（回帰 / FR4）: 既存の `term_core --lib` スイート全件と workspace テストが通過。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| overflow | セルの `char_len == 0xFF` のとき、グラフェム実体を別テーブルに退避する仕組み（16 バイト超グラフェム用） |
| `overflow_ridx` | overflow テーブルの逆引き index |
| 相方掃除 | wide（幅 2）ペアの片側が上書き・削除された際に、もう一方（base または spacer）を空白化する処理 |
| base / spacer | wide 文字が占める 2 セルのうち、実体を持つ側（base）と埋め側（spacer） |
| DCH | Delete Character（`handle_delete_characters`） |
| ECH | Erase Character（`handle_erase_characters`） |

## 14. 確認事項

### 14.1 確認済み事項

- [x] `term_core` の grapheme merge / 幅計算が ZWJ 家族絵文字を幅 2 の 1 クラスタとして
      扱う。万一扱わない場合は、幅 2 かつ 16 バイト超の別グラフェムクラスタを選定する
      （要件の本質は「overflow 行きの幅 2 base」であり特定の絵文字ではない）。
- [x] 相方掃除プリミティブ共通化タスク（finding `b8a62feaf016ef08` /
      `931abe859e23fa5d`）は未着手であり、テスト対象は `blank_wide_pair_partner` と
      `blank_wide_pair_split` の 2 本。
- [x] タスク記述の「csi_edit 経路」は DCH（`csi_edit.rs`）と ECH（`csi_screen.rs`）の
      両方を指す。
- [x] PR #30 はマージ済みで、対象コードは integration worktree に存在する（実在確認済み）。

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- `crates/term_core/src/print_handler.rs:74`: `blank_wide_pair_partner`
- `crates/term_core/src/csi_edit.rs:161`: `blank_wide_pair_split`
- `crates/term_core/src/csi_screen.rs:155`: ECH からの `blank_wide_pair_split` 呼び出し
- feature wide-pair-overwrite-cleanup（PR #30）レビュー round1 deferred finding
  `06ef78e20e9b9f0b`（medium / comprehensive）
