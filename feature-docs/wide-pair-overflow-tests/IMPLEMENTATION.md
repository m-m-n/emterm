# Implementation Plan: wide-pair-overflow-tests

## Overview

wide ペア相方掃除の overflow 分岐（16 バイト超グラフェム、`char_len == 0xFF`）に
ユニットテストを追加する。テスト追加のみで `crates/term_core` の非テストコードは
変更しない（NFR2）。

## Technology Stack

- **言語 / フレームワーク**: Rust — 標準の `cargo test` ハーネス
  （inline `#[cfg(test)]` モジュール）。
- **新規依存**: なし。新規テストフレームワーク・dev-dependency は導入しない
  （NFR1）。**ライセンス記録**: 新規依存ゼロのため `project.license: MIT` との
  衝突は発生しない（記録対象の依存なし）。

## Layer Structure

対象は `crates/term_core` 単層。テストは対象コードと同一モジュール内の
inline `#[cfg(test)]` テストモジュールに置く（NFR1）。プロダクションコードの
レイヤ構造・依存方向に変更はない。

| タスク | テスト対象プリミティブ | テストの置き場所 |
|---|---|---|
| task0001（print 経路） | `blank_wide_pair_partner`（`print_handler.rs:74`） | `crates/term_core/src/print_handler/tests.rs` |
| task0002（DCH / ECH 経路） | `blank_wide_pair_split`（`csi_edit.rs:161`） | `csi_edit.rs` / `csi_screen.rs` の `mod tests` |

## Shared Components

コード上の共有コンポーネントは作らない（D3）。両タスクが共有するのは
以下の「設計上の取り決め」で、契約として本書に固定する。

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| D1: overflow 行き幅 2 base の標準レシピ | テストの前提状態（overflow 行きの wide ペア）を作る手順の統一 | 事前条件: なし。事後条件: col0 に base（幅 2・overflow 行き）、col1 に spacer（幅 0）が存在し、overflow テーブルと `overflow_ridx` に col0 のエントリがある | task0001, task0002 |
| D2: overflow 同期の assert 方法 | `char_len == 0xFF` 分岐の実行証明（FR3）の統一 | 事前 assert: base セルの overflow フラグが真・overflow テーブルと `overflow_ridx` に該当エントリが存在。事後 assert: 両構造から該当エントリが除去済み | task0001, task0002 |

## Conventions

- **命名**: 同一モジュール内の隣接テストのパターンを踏襲する
  （`test_<subject>_<scenario>_<expected>`。例は各対象モジュールの既存
  wide-pair 掃除テスト群）。
- **配置**: inline `#[cfg(test)]` テストモジュールのみ。新規ファイル・
  統合テストディレクトリは使わない（NFR1）。
- **コメント**: 各テストに対応する TS 番号（TS1–TS4）と FR 番号を
  コメントで付記する（既存の wide-pair 掃除テスト群と同じ流儀）。
- **実行コマンド**: workflow.yaml `project.components` の承認済み文字列を
  一字一句そのまま使う（VERIFICATION.md に記載）。

## Cross-task Design Decisions

### D1: overflow 行き幅 2 base の標準レシピ

**決定**: 両タスクとも、ZWJ 家族絵文字 👨‍👩‍👧‍👦（U+1F468, U+200D, U+1F469,
U+200D, U+1F467, U+200D, U+1F466 の 7 コードポイント・UTF-8 で 25 バイト）を
row 0 の col0 に書いて前提状態を作る。term_core の grapheme merge はこれを
幅 2 の 1 クラスタとして扱い、16 バイトのインライン容量を超えるため base は
overflow テーブル行き（`char_len == 0xFF`）になる。

**注意点（両タスク共通）**:

1. print 経路ではクラスタは grapheme バッファに蓄積され、非結合の後続文字が
   届くか明示的な flush が行われるまでグリッドに書き出されない。事前 assert の
   前に必ず書き出しを完了させる（後続 ASCII の print、または公開されている
   flush 操作。既存テストの構築スタイルを踏襲）。
2. 事前 assert（D2）自体がレシピの妥当性検査を兼ねる。万一このクラスタが
   幅 2・overflow 行きにならない場合は事前 assert が落ちるので、その際は
   「term_core が 1 クラスタとして merge する・幅 2・UTF-8 長が 16 バイト超」
   の 3 条件を満たす別のグラフェムクラスタに差し替える（SPEC.md Edge Cases。
   要件の本質は「overflow 行きの幅 2 base」であり特定の絵文字ではない）。

**理由**: 両タスクで同じ前提状態を使うことで、print 経路と CSI 経路の掃除
結果を同一条件で比較可能にし、レシピ選定の判断を 1 箇所に固定する。

**影響タスク**: task0001, task0002

### D2: overflow 同期の assert 方法

**決定**: overflow テーブル・`overflow_ridx` は `TerminalCore` の crate 内
フィールドであり、inline テスト（同一 crate）から直接参照できる。各テストは

- 掃除前: base セルの overflow フラグ（`is_overflow`）が真であること、
  overflow テーブルに（base の列, 絶対行）のエントリが存在すること、
  `overflow_ridx` に対応する逆引きエントリが存在すること
- 掃除後: overflow テーブル・`overflow_ridx` の双方から該当エントリが
  除去されていること

を assert する。絶対行は viewport 相対行から crate 内の変換ヘルパで解決する。

**test/README.md との関係**: 同書は「内部状態への assert を避ける」ことを
推奨するが、FR3 は overflow / `overflow_ridx` の同期そのものの検証を明示的に
要求しているため、本 feature に限りこの 2 構造（+ セルの overflow フラグ）への
直接 assert を意図的な例外として許可する。他の内部状態への assert は行わない。

**理由**: `get_cell_char` の観測だけでは「overflow エントリが残ったまま
テーブルだけ壊れる」リグレッションを検知できない。FR3 の「分岐を通った証明」
には内部構造の直接検証が必須。

**影響タスク**: task0001, task0002

### D3: テストヘルパーの共通化はしない

**決定**: 前提状態の構築（D1）やassert（D2）を共通ヘルパ関数として切り出さ
ない。各テストが自モジュール内で完結して状態を構築・検証する。

**理由**: (1) モジュール横断のヘルパ配置は非テストコードまたは共有モジュールへ
の変更を要し NFR2 に抵触するリスクがある。(2) タスクは完全並列実装であり、
共有コードを作るとタスク間結合が生まれる。(3) 追加テストは計 4 本で重複コスト
は小さい。

**影響タスク**: task0001, task0002

### D4: get_cell_char の「空白 vs 空文字」の区別

**決定**: 掃除後の相方セルの assert は「`" "`（スペース 1 文字）を返すこと」を
明示的に検証する。overflow フラグ付きセルのテーブルエントリだけが消えた壊れ
状態では `get_cell_char` は空文字 `""` を返すため、`" "` との等値 assert が
この 2 状態を区別する（SPEC.md FR2 の「空文字ではない」要求）。

**影響タスク**: task0001, task0002

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 家族絵文字が幅 2 の 1 クラスタ / overflow 行きにならない | 低 | 中 | D1 の事前 assert が即検知。フォールバック選定基準を D1 に明記済み |
| 追加テストが既存スイートと干渉（並列実行での不安定化） | 低 | 低 | 各テストが独立に `TerminalCore` を構築（既存規約どおり）。term_core の `--lib` は並列実行で安定している既知領域 |
| 非テストコードへの誤変更（NFR2 違反） | 低 | 高 | 変更ファイルをテストモジュールに限定（各タスクの files 契約）。verify で差分確認 |

## Open Questions

なし。
