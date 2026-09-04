---
title: "ring-push-blank-note-unconditional"
created_date: 2026-09-04
status: draft
---

# ring-push-blank-note-unconditional - 要件定義書

## 1. 概要

### 1.1 背景

`crates/term_core/src/ring_buffer/tests.rs` の survivor assertion 群の上に置かれた
NOTE が、`ring_push_blank` 内の 2 つのクリア処理が冗長である理由を、フィクスチャの
寸法（2 行・スクロールバック 0）に帰属させている。一方で兄弟レコードである
`feature-docs/ring-push-blank-row-scope-test/SPEC.md` FR6 と
`feature-docs/ring-push-blank-row-scope-test/VERIFICATION.md` MT3 は、同じ事実を
無条件（フィクスチャ非依存）の事実として記述しており、記述が食い違っている。

### 1.2 目的

- NOTE を、真の無条件の理由 — `evicted_abs`（回転前に取得）と `new_bottom_abs`
  （回転後に算出）の評価順序 — で書き換える。
- NOTE、SPEC.md FR6、VERIFICATION.md MT3、および本フィーチャーで修正する
  `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` AC-5 の 4 レコードの
  記述の食い違いを解消する。
- 変更をテキストのみに留め、実行時挙動およびテスト挙動を一切変えない。

### 1.3 スコープ

**対象**

- `crates/term_core/src/ring_buffer/tests.rs` の inline `#[cfg(test)]` モジュール内の
  コメント行。
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` の AC-5 `red_reason`
  のテキスト。

**対象外**

- `crates/term_core/src/ring_buffer.rs` の変更（無変更のまま維持する）。
- `ring_buffer.rs:221-224` の冗長な新ボトム行クリアの削除・リファクタリング。
- 兄弟フィーチャー `ring-push-blank-row-scope-test` の SPEC.md FR6 および
  VERIFICATION.md MT3 の編集（既に無条件で記述済みのため、参照側として据え置く）。
- アサーション、フィクスチャ寸法、テスト名、プロダクションコードの変更。

## 2. ビジネス要件

### 2.1 ビジネス目標

| ID | 目標 |
|----|------|
| OBJ-1 | `crates/term_core/src/ring_buffer/tests.rs` の survivor assertion 群の上にある説明 NOTE を修正し、`ring_push_blank` 内の 2 つのクリア箇所が冗長である真の無条件の理由 — `evicted_abs`（回転前に取得）と `new_bottom_abs`（回転後に算出）の評価順序 — を示すようにする。フィクスチャの 2 行・スクロールバック 0 という寸法に冗長性を帰属させる記述はやめる。 |
| OBJ-2 | その NOTE と、既に事実を無条件に記述している 3 つの兄弟レコード — SPEC.md FR6、VERIFICATION.md MT3、および（本フィーチャーの修正後の）`test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` AC-5 — の間のドキュメント・ドリフトを解消する。 |
| OBJ-3 | 変更を純粋にテキストのみ — テストモジュール内のコメント行と tests.yaml のレコード文言 — に留め、`crates/term_core/src/ring_buffer.rs` と全アサーションをバイト単位で同一に保ち、実行時およびテストの挙動を変えない。 |

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | `crates/term_core/src/ring_buffer/tests.rs` の NOTE と `test-docs/` のテスト記録を読み、`ring_push_blank` のクリア処理の冗長性を理解する者 |

### 2.3 期待される効果

- NOTE が示す冗長性の理由が、実際のコードの評価順序と一致する。
- NOTE・SPEC.md FR6・VERIFICATION.md MT3・task0001.tests.yaml AC-5 の 4 レコードが、
  条件付き修飾なしで一致する。
- プロダクションコードとアサーションが無変更のため、テスト結果は変更前後で同一になる。

## 3. ユースケース

### 3.1 ユースケース一覧

本フィーチャーはユーザーに見える面を持たない。Rust の `#[cfg(test)]` モジュール内の
英文コメントブロックと、YAML レコードの 1 フィールドを書き換えるのみであり、UI・API・
データモデル・アーキテクチャ上の選択がない。したがってユースケースは定義しない。

### 3.2 ユースケース詳細

該当なし。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | NOTE が無条件の評価順序の理由を述べる | NOTE が `new_bottom_abs == evicted_abs` の成立条件とその理由を無条件に記述する | resolved |
| FR2 | NOTE からフィクスチャ限定の修飾を除去 | フィクスチャ寸法に依存する修飾句を除去する | resolved |
| FR3 | NOTE の既存の事実を保持 | 片方のクリア箇所だけを削除してもテストが緑である事実を保つ | resolved |
| FR4 | NOTE の配置・言語・形式を維持 | 位置・英語・コメント形式を変えない | resolved |
| FR5 | VERIFICATION MT3 / MT5 を満たし続ける | MT3 を満たし、MT5 の対象ブロックを乱さない | resolved |
| FR6 | tests.yaml AC-5 の red_reason を無条件表現に修正 | 末尾の "in this fixture" 修飾を落とす | resolved |
| FR7 | NOTE が no-op という帰結を記録する | 新ボトム行クリアが常に no-op である旨を述べる | resolved |

### 4.2 機能詳細

#### FR1: NOTE が無条件の評価順序の理由を述べる

**説明**: `crates/term_core/src/ring_buffer/tests.rs` の survivor assertion 群の上にある
NOTE（現状 517-522 行）は、`new_bottom_abs == evicted_abs` が `rows >= 1` である
**すべての** `ring_push_blank` 呼び出しで成立し、フィクスチャ寸法および
`scrollback_capacity` に依存しないことを述べなければならない。さらにその理由として、
`evicted_abs` が回転**前**の `ring_head` から取得されること
（`crates/term_core/src/ring_buffer.rs:129`）、一方
`new_bottom_abs = (ring_head + rows - 1) % rows` は `ring_head` が 1 進んだ**後**に
算出されること（`ring_buffer.rs:204`, `:207`）、したがって 2 つの式が同一のリングスロットを
指すことを示さなければならない。

#### FR2: NOTE からフィクスチャ限定の修飾を除去

**説明**: 書き換え後の NOTE にはフィクスチャ限定の修飾句が含まれてはならない。
具体的には `in this 2-row, zero-scrollback fixture`（tests.rs:520）および
`for this fixture, not independently pinned by it`（tests.rs:522）の各句が消え、
無条件の表現に置き換わっていること。

#### FR3: NOTE の既存の事実を保持

**説明**: 書き換え後の NOTE は、現行 NOTE が既に持つ事実を保持しなければならない。
すなわち、`ring_push_blank` 内の 2 つのクリア箇所（退避時クリアまたは新ボトム行クリア）の
**片方だけ**を取り除いても
`test_ring_push_blank_clears_recycled_row_overflow_entries` は緑のままである、という事実。
変わるのはその事実の理由のみであり、事実そのものは決して変えない。

#### FR4: NOTE の配置・言語・形式を維持

**説明**: NOTE は現在の位置 — inline `#[cfg(test)]` モジュール内、tests.rs:523 の
survivor assertion `assert!(core.overflow.contains_key(&(0u32, abs1)))` の直上 — に
英文コメントブロックとして留まり、コメントのままであること。アサーション、doc コメント、
別ドキュメントへの昇格は行わない。

#### FR5: VERIFICATION MT3 / MT5 を満たし続ける

**説明**: 書き換え後の NOTE は VERIFICATION.md MT3（存在すること、英語であること、
事実と、2 つの箇所が単一 push 内で同じ行を対象とする理由の両方を述べていること）を
満たし続けなければならない。また、MT5 が確認する、テストの上にある別の説明ブロックを
乱してはならない。

#### FR6: tests.yaml AC-5 の red_reason を無条件表現に修正

**説明**: `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` の AC-5 の
`red_reason` から末尾の "in this fixture" 修飾（現状 54 行目:
`... coincide within a single push in this fixture.`）を落とし、無条件の記述として読め、
修正後の NOTE、SPEC.md FR6、VERIFICATION.md MT3 と一致するようにする。AC-5 の
`red_confirmed: false` および "Comment-only criterion, not test-observable" という性格は
変更しない。修正するのは `red_reason` 内の修飾句の文言のみで、エントリの残りのテキストは
記載のまま据え置く。`test-docs/ring-push-blank-row-scope-test/**` は本フィーチャーの
宣言された変更集合の一部である。

#### FR7: NOTE が no-op という帰結を記録する

**説明**: 書き換え後の NOTE は `new_bottom_abs == evicted_abs` の帰結も述べなければ
ならない。すなわち `crates/term_core/src/ring_buffer.rs:221-224` の新ボトム行クリアは
単一 push 内では常に no-op であること。理由は、実行された退避時クリアの分岐
（`ring_buffer.rs:146-149`, `:177-180`, `:196-199` のいずれか）が、既に同じ絶対行を
空にしているためである。

## 5. 非機能要件

### 5.1 変更の封じ込め

| ID | 要件 |
|----|------|
| NFR1 | 変更集合はちょうど 2 ファイルで、コメント／レコードのテキストのみを含む。(a) `crates/term_core/src/ring_buffer/tests.rs` — inline `#[cfg(test)]` モジュール内のコメント行、(b) `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` — AC-5 の `red_reason` テキスト。`crates/term_core/src/ring_buffer.rs` は無変更のまま。アサーション、フィクスチャ寸法、テスト名、プロダクションコードの変更はどこにも行わない。 |
| NFR4 | `ring_buffer.rs:221-224` の冗長な新ボトム行クリアは記述するのみで、削除もリファクタリングもしない。その削除は本フィーチャーのスコープ外であることを明示する。 |

### 5.2 挙動

| ID | 要件 |
|----|------|
| NFR2 | `term_core` のテストスイートの結果は変更前後で同一であること。同じテストが通り、同じ件数（兄弟タスクの AC-7 の記録に従い 825 passed / 0 failed / 13 ignored）であり、テストの追加も削除もない。 |

### 5.3 コードスタイル・フォーマット

| ID | 要件 |
|----|------|
| NFR3 | NOTE は英語のままとし、周囲の `// ` 行コメントスタイルを保ち、当該ファイルで既に使われているコメント幅で折り返す。これにより rustfmt が触らない状態を維持する。 |
| NFR5 | `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` が変更後に何も出力しないこと。 |

### 5.4 記録ファイルの妥当性

| ID | 要件 |
|----|------|
| NFR6 | `task0001.tests.yaml` は妥当な YAML のままで、同じ構造にパースされること。同じキー、AC-5 の `red_reason` に対する同じブロックスカラー形式とインデント、同じ `red_confirmed` と `tests` の値。変わるのは AC-5 の `red_reason` スカラー内の文字のみ。 |

### 5.5 その他の非機能要件

パフォーマンス要件、セキュリティ要件、可用性要件、互換性要件は該当なし。本フィーチャーは
コメントとレコードのテキストのみを変更し、実行されるコードを一切含まない。

## 6. UI/UX要件

該当なし。本フィーチャーはユーザーに見える面を持たない。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- `crates/term_core/src/ring_buffer.rs` は無変更でなければならない（NFR1）。
- 変更される Rust の行はすべて inline `#[cfg(test)]` モジュール内のコメント行であること。
- 変更される YAML の行はすべて AC-5 の `red_reason` スカラー内であること。
- `ring_buffer.rs:221-224` の冗長なクリアは削除しない（NFR4、A1）。

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの
`files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**:

- `feature-docs/ring-push-blank-note-unconditional/**`
- `test-docs/ring-push-blank-note-unconditional/**`

**追加メンバー**（FR6 により本フィーチャーの変更集合に含まれる）:

- `crates/term_core/src/ring_buffer/tests.rs`
- `test-docs/ring-push-blank-row-scope-test/**`

**意味論**: この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に
含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても
違反にはならない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 冗長なクリアを「記述する」のではなく「削除する」方向に踏み込む | 中 | 削除は挙動変更であり、独自のフィーチャーを要する。本フィーチャーでは記述に留める（NFR4、A1） |
| tests.yaml の編集が完了済みタスクの再実行と誤解される | 低 | AC-5 の `red_confirmed: false` / "not test-observable" 分類は事実として正しいまま保持する（A5） |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: `crates/term_core/src/ring_buffer/tests.rs` の NOTE が、`new_bottom_abs == evicted_abs` は行数とスクロールバック容量によらずすべての `ring_push_blank` 呼び出しで成立することを述べ、評価順序という理由（`evicted_abs` は `ring_head` の回転前に取得、`new_bottom_abs` は回転後に算出）を挙げている。
- [ ] AC2: 書き換え後の NOTE に `in this 2-row, zero-scrollback fixture` も `for this fixture, not independently pinned by it` も、その他いかなるフィクスチャ限定の修飾句も現れない。
- [ ] AC3: NOTE が、2 つのクリア箇所の片方だけを取り除いてもテストが緑のままであることを引き続き述べている。
- [ ] AC4: NOTE がさらに、その帰結として新ボトム行クリアが単一 push 内で常に no-op であること、およびその理由（退避時クリアが既に同じ絶対行を空にしている）を述べている。
- [ ] AC5: `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` AC-5 の `red_reason` に "in this fixture" の句が含まれず、無条件の記述として読める。一方で AC-5 の `red_confirmed` は false のまま、`tests` リストは空のまま、エントリの残りの文言も無傷である。
- [ ] AC6: 4 つのレコード — tests.rs の NOTE、SPEC.md FR6（`feature-docs/ring-push-blank-row-scope-test/SPEC.md:98-102`）、VERIFICATION.md MT3（`:103-105`）、task0001.tests.yaml AC-5 — が条件付き修飾なしで一致している。
- [ ] AC7: 最終差分が触れるのは `crates/term_core/src/ring_buffer/tests.rs` と `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` のみ（ワークフローが生成するエントリを除く）。`crates/term_core/src/ring_buffer.rs` に変更ハンクが無く、変更された Rust 行はすべて inline `#[cfg(test)]` モジュール内のコメント行であり、変更された YAML 行はすべて AC-5 の `red_reason` スカラー内である。
- [ ] AC8: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` が緑で、件数が変更前の実行と一致し、`cargo fmt --manifest-path crates/term_core/Cargo.toml --check` がクリーンである。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS1: `term_core` のライブラリテストスイートを変更前後で実行し、pass / fail / ignored の件数を比較する。同一でなければならない（NFR2、AC8）。
- [ ] TS2: 書き換え後の NOTE を読み、3 要素すべてを備えているか確認する。事実（片方の箇所の削除でもテストは緑）、無条件の理由（同一スロットに対する回転前と回転後の評価）、no-op という帰結（AC1、AC3、AC4）。
- [ ] TS3: 書き換え後の NOTE と修正後の AC-5 `red_reason` をフィクスチャ限定の修飾句（`this fixture`、`2-row`、`zero-scrollback`）で grep する。いずれも残っていてはならない（AC2、AC5）。
- [ ] TS4: 統合後の差分を検査する。変更ファイルはちょうど 2 つ、`ring_buffer.rs` にハンク無し、アサーション行・フィクスチャ行の変更無し、`task0001.tests.yaml` が YAML としてパースでき AC-5 の `red_confirmed: false` が無傷であること（AC7、NFR1、NFR6）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| NOTE | `crates/term_core/src/ring_buffer/tests.rs` の survivor assertion 群の直上（現状 517-522 行）に置かれた英文コメントブロック |
| survivor assertion | tests.rs:523 の `assert!(core.overflow.contains_key(&(0u32, abs1)))` を含むアサーション群 |
| `evicted_abs` | `ring_buffer.rs:129` で回転前の `ring_head` から取得される絶対行 |
| `new_bottom_abs` | `ring_buffer.rs:207` で `(ring_head + rows - 1) % rows` として、`ring_head` が 1 進んだ後（`:204`）に算出される絶対行 |
| 退避時クリア | `ring_buffer.rs:146-149`、`:177-180`、`:196-199` のいずれか。1 回の push につきちょうど 1 つが実行される |
| 新ボトム行クリア | `ring_buffer.rs:221-224` のクリア処理 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1: 本フィーチャーは `crates/term_core/src/ring_buffer.rs:221-224` の冗長な新ボトム行クリアを意図的に削除**しない**。冗長性を記述するのみであり、削除は挙動変更となるため独自のフィーチャーを要する。
- [x] A2: 冗長性は真に無条件でありフィクスチャ固有ではない。`evicted_abs = self.ring_head` は `ring_buffer.rs:129` で `:204` の回転より前に読まれ、`:207` の `new_bottom_abs = (ring_head + rows - 1) % rows` は `rows >= 1` の任意の値に対して回転前の `ring_head` と等しくなる。したがって退避時クリア（`:146-149` / `:177-180` / `:196-199` のいずれか、1 push につきちょうど 1 つ）と新ボトムクリア（`:221-224`）は常に同じ絶対行を対象とする。
- [x] A3: 兄弟フィーチャー `ring-push-blank-row-scope-test` の SPEC.md FR6 と VERIFICATION.md MT3 は既に事実を無条件に記述しているため、本フィーチャーでは編集しない。NOTE と AC-5 を揃える先の参照文言である。
- [x] A4: プロダクションコードもアサーションも変わらないため、適切な検証は差分の検査と、結果が変わらないテスト実行である。新規テストは追加せず、mutation 実験の再実行も行わない。
- [x] A5: tests.yaml の編集は完了済みタスクの記録の訂正であり、そのタスクの再実行ではない。AC-5 の `red_confirmed: false` / "not test-observable" という分類は事実として正しいままであり、保持される。
- [x] デザインステップ: skipped。ユーザーに見える面が無く（Rust `#[cfg(test)]` モジュール内の英文コメントブロックと YAML レコード 1 フィールドの書き換え）、UI・API・データモデル・アーキテクチャ上の選択が無いため。回答済みの `create-spec.design-step` ゲートが `decide_autonomously` に解決し、スキップ推奨をユーザーに問わずに受理した。

### 14.2 未確認・保留事項

なし。すべての要件が `status: resolved` である。

## 15. 参考資料

- `crates/term_core/src/ring_buffer/tests.rs`: 書き換え対象の NOTE（517-522 行）と survivor assertion（523 行）
- `crates/term_core/src/ring_buffer.rs`: `:129`（`evicted_abs` 取得）、`:204` / `:207`（回転と `new_bottom_abs` 算出）、`:146-149` / `:177-180` / `:196-199`（退避時クリア）、`:221-224`（新ボトム行クリア）
- `feature-docs/ring-push-blank-row-scope-test/SPEC.md`: FR6（98-102 行）
- `feature-docs/ring-push-blank-row-scope-test/VERIFICATION.md`: MT3（103-105 行）、MT5
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`: AC-5 の `red_reason`（54 行）、AC-7 のテスト件数記録
