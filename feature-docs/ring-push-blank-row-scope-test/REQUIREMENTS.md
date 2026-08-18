---
title: "ring-push-blank-row-scope-test"
created_date: 2026-08-19
status: draft
---

# ring-push-blank-row-scope-test - 要件定義書

## 1. 概要

### 1.1 背景

`crates/term_core/src/ring_buffer/tests.rs:444` の既存テスト
`test_ring_push_blank_clears_recycled_row_overflow_entries` は、`ring_push_blank`
の overflow サイドテーブルのクリアが「何かがクリアされた」ことしか観測しておらず、
クリアの行スコープを観測していない。

この観測ギャップは finding `821776efcf3c8be9` として記録されている。行スコープの
`overflow_clear_row` / `overflow_ridx_clear_row` のペアがテーブル全消しに置き換わる
退行が起きても、現在のアサーションはすべてグリーンのままになる。

### 1.2 目的

既存の survivor テストが `ring_push_blank` の行スコープを実際に観測するようにし、
overflow サイドテーブルのクリアが「退避された絶対行」に固定されていることを
テストで固定する。

### 1.3 スコープ

対象:

- `crates/term_core/src/ring_buffer/tests.rs` の
  `test_ring_push_blank_clears_recycled_row_overflow_entries` の fixture と
  アサーションの拡張。

対象外:

- DECSTBM のスクロールリージョン経路（`shift_rows_up`）のクリア箇所（チケット記載の対象外）。
- `cols <= 2` のカーソル範囲外の不具合（finding `3e769a761d85d839`、チケット記載の対象外）。
- 兄弟テスト `test_ring_push_blank_clears_ridx`（FR-7 参照、本フィーチャーでは触れない）。
- `crates/term_core/src/ring_buffer.rs` のプロダクションコードの挙動変更。

## 2. ビジネス要件

### 2.1 ビジネス目標

| ID | 目標 |
|----|------|
| OBJ-1 | 既存の survivor テスト `test_ring_push_blank_clears_recycled_row_overflow_entries`（`crates/term_core/src/ring_buffer/tests.rs:444`）が `ring_push_blank` の行スコープを実際に観測するようにし、overflow サイドテーブルのクリアが「何かがクリアされた」ではなく退避された絶対行に固定されるようにする。 |
| OBJ-2 | finding `821776efcf3c8be9` として記録された観測ギャップを塞ぐ。行スコープの `overflow_clear_row` / `overflow_ridx_clear_row` のペアをテーブル全消しに置き換える退行が起きても、現在のアサーションはすべてグリーンのままになる。 |
| OBJ-3 | 変更をテストのみに留める。`crates/term_core/src/ring_buffer.rs` のプロダクションコードの挙動は変えない。 |

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| `term_core` の開発者 | `ring_push_blank` および overflow サイドテーブルを変更する開発者。行スコープを壊す退行がテストで検出される。 |

### 2.3 期待される効果

- 行スコープのクリアがテーブル全消しに置き換わる退行が、テスト実行で検出される。
- overflow サイドテーブルのクリア範囲が、テストによって仕様として固定される。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 行スコープを壊す退行の検出 | `term_core` の開発者 | 高 |

### 3.2 ユースケース詳細

#### UC01: 行スコープを壊す退行の検出

**アクター**: `term_core` の開発者

**事前条件**:

- `crates/term_core` の lib テストが実行できる。

**基本フロー**:

1. 開発者が `ring_push_blank` またはその周辺を変更する。
2. `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` を実行する。
3. `test_ring_push_blank_clears_recycled_row_overflow_entries` が、退避行のエントリが消え、
   survivor 行のエントリが残っていることを検証する。

**代替フロー**:

- 行スコープのクリアがテーブル全消しに置き換わっている場合、survivor 行のアサーションが失敗する。

**事後条件**:

- 行スコープのクリアが保たれていることが確認される。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR-1 | fixture への survivor 行の追加 | リサイクルされない 2 行目に overflow 行きのセルを置く | resolved |
| FR-2 | survivor 行の事後アサーション | スクロール後も survivor 行のエントリが残ることを検証する | resolved |
| FR-3 | 非空虚化（anti-vacuity）事前アサーションの拡張 | survivor 行のエントリが事前に存在することを検証する | resolved |
| FR-4 | 既存の退避行アサーションの維持 | 既存の事後アサーションを変更しない | resolved |
| FR-5 | 退行検出の実証 | テーブル全消しに置き換えるとテストが失敗することを実証する | resolved |
| FR-6 | テスト本文への冗長性コメント | 2 つのクリア箇所の冗長性をコメントで記録する | resolved |
| FR-7 | 兄弟テスト `test_ring_push_blank_clears_ridx` のスコープ | 本フィーチャーでは触れない | excluded |

### 4.2 機能詳細

#### FR-1: fixture への survivor 行の追加

**説明**:
`test_ring_push_blank_clears_recycled_row_overflow_entries` に、リサイクルされない 2 行目の
ビューポート行（「survivor 行」）を追加する。この行は overflow 行きのセルを保持し、既存の
row 0 のセルと同じスタイルで `handle_print` を通して投入し、fixture の形を揃える。その絶対行
キーはスクロール前に `viewport_abs` で取得する。

チケットの具体案: row 0 に 'e' / 'f' を書いた後、`core.set_cursor(0, 1);` として survivor の内容
（'g' と同じ 8 個の結合文字）を出力し、`let abs_survivor = core.viewport_abs(1) as u32;` とする。

**状態**: resolved

#### FR-2: survivor 行の事後アサーション

**説明**:
全画面スクロール経路を `ring_push_blank` へ通す line feed の後、survivor 行のエントリが
**まだ存在する** ことをアサートする。
`assert!(core.overflow.contains_key(&(0u32, abs_survivor)));` および、`overflow_ridx` が survivor
の絶対行キーを列集合ごと保持していること。行がビューポート行 0 に移動しても、リングスロット /
絶対 ID は安定なのでキーはスクロールをまたいで変わらない。

**状態**: resolved

#### FR-3: 非空虚化（anti-vacuity）事前アサーションの拡張

**説明**:
既存の非空虚化事前アサーションのブロックを survivor 行にも広げる
（`assert!(core.overflow.contains_key(&(0u32, abs_survivor)));`）。survivor のセルをインライン上限を
超えて押し出せていない fixture が、新しいアサーションを空虚に真にしてしまうことを防ぐ。

**状態**: resolved

#### FR-4: 既存の退避行アサーションの維持

**説明**:
現在の退避行の事後アサーション（`!overflow.contains_key(&(0, abs0))`、
`!overflow.contains_key(&(1, abs0))`、`!overflow_ridx.contains_key(&abs0)`）は変更しない。本フィーチャーは
観測を追加するだけで、観測を削らない。

**状態**: resolved

#### FR-5: 退行検出の実証

**説明**:
`ring_push_blank` の行スコープのクリアを一時的にテーブル全消し（`self.overflow.clear()` /
`overflow_ridx.clear()`）へ置き換えたとき、拡張後のテストが失敗すること。実装中に一度実証し、
変更を確定する前に元に戻す。

**状態**: resolved

#### FR-6: テスト本文への冗長性コメント

**説明**:
テスト本文に、2 つのクリア箇所のうち **片方だけ** を削除してもテストがグリーンのままである
こと、その理由は `new_bottom_abs == evicted_abs` により 2 箇所が冗長になるためであることを述べる
コメントを置く。この知見は現在
`test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` の AC-4 にしか記録されていない。

**状態**: resolved

#### FR-7: 兄弟テスト `test_ring_push_blank_clears_ridx` のスコープ

**説明**:
`test_ring_push_blank_clears_ridx`（`crates/term_core/src/ring_buffer/tests.rs:417`）は同じ盲点を
持つ。`overflow.is_empty()` / `overflow_ridx.is_empty()` しかアサートしておらず、テーブル全消しでも
自明に満たされる。本フィーチャーでは **触れない**。

**状態**: excluded

**除外理由**:
回答済みの質問 requirement.sibling-test-scope（packet create-spec-q0001、選択肢
`separate_task`、batch モードで Codex への相談により決定）によりスコープ外。観測が失われない
よう明示的なフォローアップとして記録する。当該兄弟のアサーションを強化するか、意図的に
空判定のままにするかは、別タスクで判断する。

## 5. 非機能要件

| ID | 内容 |
|----|------|
| NFR-1 | テストのみの変更。インライン `tests` モジュールの外にある `crates/term_core/src/` 配下のプロダクションコードは編集しない。特に `ring_buffer.rs` の `ring_push_blank` は変更しない（FR-5 の変異は一時的な検証手順であり、元に戻す）。 |
| NFR-2 | クレート既存のテスト規約に従う。インライン `#[cfg(test)]` モジュール、`test_*` 関数命名、fixture は拡張対象テストの隣（`crates/term_core/src/ring_buffer/tests.rs`）に置き、テスト上部の説明コメントブロックを正確に保つ。 |
| NFR-3 | 新規依存を追加しない。`crates/term_core` の dev-dependency は `mux_ipc` のみを維持し、proptest・criterion 等のテストフレームワークを導入しない。 |
| NFR-4 | 標準の cargo test ハーネス下で決定的であること。タイミング・順序・並列性に依存せず、`--test-threads=1` なしで安定する。 |
| NFR-5 | `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` がクリーンであること。fmt を rewrite モードでクレート全体に走らせない。 |

## 6. UI/UX要件

該当なし（UI 面を持たないテストのみの変更）。

## 7. データ要件

該当なし（永続データの追加・変更なし）。観測対象のインメモリ構造は次のとおり。

| 構造 | キー | 説明 |
|------|------|------|
| `overflow` | `(col: u32, abs_row: u32)` | 行ごとの残存が直接観測できる |
| `overflow_ridx` | `abs_row` | 絶対行キーごとの列集合 |

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 全編集は `crates/term_core/src/ring_buffer/tests.rs` に限定する。新規 dev-dependency も新規
  テストファイルも導入しない（A6）。
- fixture の `TerminalCore::new(cols, 2, 0)` の形（ビューポート 2 行、スクロールバック無効）では、
  最終行からの 1 回の line feed がリングスロットをちょうど 1 つリサイクルし、もう一方の
  ビューポート行が survivor 行として残る。より明確な survivor 行が必要なら行数・列数を広げてよい（A1）。
- 既存の `marks` 列（基底文字の後の 8 個の結合文字）がセルをインライン上限の外へ押し出す手段であり、
  survivor 行も同じ手法を再利用する（A3）。

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

**このフィーチャー固有のパス**:

- `crates/term_core/src/ring_buffer/tests.rs`

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:

- `feature-docs/ring-push-blank-row-scope-test/**`
- `test-docs/ring-push-blank-row-scope-test/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、
`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、
`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズ
ドキュメントおよび `references/phase-state.md` を参照。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式:
`test-docs/ring-push-blank-row-scope-test/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照。

**意味論**:

- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な
  絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる
  （CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。

`crates/term_core/src/ring_buffer.rs` は宣言に含まれない。FR-5 の変異は一時的で、確定前に元に
戻すため、最終的な diff には残らない（A4、NFR-1）。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| survivor 行のセルがインライン上限を超えず、新アサーションが空虚に真になる | 高 | FR-3 の非空虚化事前アサーションで検出する |
| 2 つのクリア箇所が冗長で、片方の削除ではテストが落ちない | 中 | FR-6 のコメントで、この限界をテスト本文に明記する |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: fixture に、スクロールでリサイクルされる行とは別の survivor 行があり、少なくとも
      1 列に overflow 行きの内容を保持し、`abs_survivor` がスクロール前に `viewport_abs` で取得されている。
- [ ] AC-2: 事前アサーションが、リサイクル対象行と survivor 行の双方のエントリがスクロール前に
      `overflow` および `overflow_ridx` に実際に存在することを確認する。
- [ ] AC-3: 事後アサーションが、リサイクルされた行の `overflow` エントリが消え、その
      `overflow_ridx` キーが存在しないことを確認する（既存の挙動を維持）。
- [ ] AC-4: 事後アサーションが、survivor 行の `overflow` エントリと、期待される列集合を伴う
      `overflow_ridx` キーがまだ存在することを確認する。
- [ ] AC-5: テスト本文のコメントに、片側のみの削除 / `new_bottom_abs == evicted_abs` の冗長性が
      記録されている。
- [ ] AC-6: `ring_push_blank` の行スコープのクリアを一時的にテーブル全消しに置き換えると、
      拡張後のテストが survivor 行のアサーションで失敗する。変異はその後元に戻され、その差分は残らない。
- [ ] AC-7: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      がパスし、クレート内の他のテストに退行がない。
- [ ] AC-8: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` がクリーンである。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1 行スコープのクリアの観測: リサイクル対象行と survivor 行に overflow 行きのセルを投入し、
      双方を事前アサートし、line feed で全画面スクロール経路を発火させ、リサイクル行のエントリが
      クリアされ **かつ** survivor 行のエントリが無傷であることをアサートする。
- [ ] TS-2 非空虚化ガード: fixture の内容がインライン上限を超えなくなった場合（=セルが実際には
      overflow 行きでない場合）、事前アサーションが明確に失敗することを確認する。
- [ ] TS-3 変異チェック: `ring_push_blank` 内でテーブル全消しに一時的に差し替え、テストが失敗する
      ことを確認してから元に戻す。
- [ ] TS-4 クレート単位の退行実行: `term_core` の lib テストスイート全体を実行し、隣接する fixture
      （触れていない `test_ring_push_blank_clears_ridx` を含む）に退行がないことを確認する。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| survivor 行 | スクロールでリサイクルされない側のビューポート行。overflow 行きのセルを保持し、クリアの行スコープを観測するために使う |
| 退避行（recycled row） | line feed によるスクロールでリングスロットが再利用される行。`abs0` の絶対行キーを持つ |
| 非空虚化（anti-vacuity）アサーション | 対象エントリがスクロール前に実在することを確認し、事後アサーションが空虚に真になることを防ぐ事前アサーション |
| インライン上限 | セルの内容がインライン格納から overflow サイドテーブルへ移る境界 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 兄弟テスト `test_ring_push_blank_clears_ridx` の扱い（質問 requirement.sibling-test-scope、
      packet create-spec-q0001）: 選択肢 `separate_task`。batch モードで Codex への相談により決定。
      本フィーチャーでは触れず、別タスクで扱う（FR-7）。
- [x] デザインステップ: skipped。`crates/term_core` のインラインテストモジュール内のテストのみの
      変更であり、UI 面もユーザーから見える挙動も新規公開 API も、決着すべきアーキテクチャ上の
      選択もない。fixture の形は拡張対象の既存テストによって完全に決まっている。
- [x] プロジェクトのライセンス: MIT（確度高）。
- [x] コンポーネントのコマンド: term_core テスト =
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`、
      term_core フォーマット = `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`。
      E2E 基盤は存在しない。

### 14.2 未確認・保留事項

- [ ] 兄弟テスト `test_ring_push_blank_clears_ridx` のアサーションを強化するか、意図的に空判定の
      ままにするかは、別タスクで判断する（FR-7 の除外理由に記録）。

### 14.3 前提

| ID | 内容 |
|----|------|
| A1 | fixture の `TerminalCore::new(cols, 2, 0)` の形（ビューポート 2 行、スクロールバック無効）では、最終行からの 1 回の line feed がリングスロットをちょうど 1 つリサイクルし、もう一方のビューポート行が survivor 行として残る。より明確な survivor 行が必要なら fixture の行数・列数を広げてよい。 |
| A2 | `overflow` は `(col: u32, abs_row: u32)`、`overflow_ridx` は `abs_row` をキーとするため、行ごとの残存は新しいアクセサなしにこの 2 つのマップから直接観測できる。 |
| A3 | 既存の `marks` 列（基底文字の後の 8 個の結合文字）がセルをインライン上限の外へ押し出す手段であり、survivor 行も同じ手法を再利用する。 |
| A4 | FR-5 の変異チェックは実装中に一時的に行い、元に戻す。最終的な diff では `crates/term_core/src/ring_buffer.rs` は変更されていない。 |
| A5 | 必須の説明コメントは、周囲のテストモジュールのスタイルに合わせて英語で書く。 |
| A6 | 全編集は `crates/term_core/src/ring_buffer/tests.rs` に限定する。新規 dev-dependency も新規テストファイルも導入しない。 |

## 15. 参考資料

- 拡張対象テスト: `crates/term_core/src/ring_buffer/tests.rs:444`
  （`test_ring_push_blank_clears_recycled_row_overflow_entries`）
- 兄弟テスト（対象外）: `crates/term_core/src/ring_buffer/tests.rs:417`
  （`test_ring_push_blank_clears_ridx`）
- プロダクションコード（変更しない）: `crates/term_core/src/ring_buffer.rs`
- 冗長性の知見の既存記録: `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` の AC-4
- 観測ギャップの finding: `821776efcf3c8be9`
- 対象外の finding: `3e769a761d85d839`（`cols <= 2` のカーソル範囲外の不具合）
