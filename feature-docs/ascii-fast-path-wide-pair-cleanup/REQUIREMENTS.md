---
title: "ascii-fast-path-wide-pair-cleanup"
created_date: 2026-08-13
status: draft
---

# ascii-fast-path-wide-pair-cleanup - 要件定義書

## 1. 概要

### 1.1 背景

term_core の `process_pty_data` にある ASCII fast path は、セルを上書きする際に D2 不変条件（グリッド上に wide ペアの孤立した片割れを残さない）の修復を行っていない。そのため全角文字のセルを ASCII 文字で上書きすると、幅 0 のスペーサーが孤立したままターミナルに見えてしまう。

加えて `blank_wide_pair_half` の doc コメントが列挙している D2 修復の呼び出し箇所は、実際に修復を行っている経路の集合と一致していない。

報告された事象は、全角文字を出力した後、別の `process_pty_data` 呼び出しで CR に続けて ASCII 文字を送り、col 0 の wide base を上書きするというもの。

### 1.2 目的

- ASCII fast path を含む term_core の全ての print 経路で D2 不変条件を成立させる（BO1）
- `blank_wide_pair_half` の doc コメントの列挙を、実際に修復を行う経路の集合と一致させる（BO2）
- ASCII fast path が存在する理由 — ASCII という共通ケースでのバイト当たりコストの最小化 — を保ったまま、正当性のギャップを閉じる（BO3）

### 1.3 スコープ

変更は `crates/term_core` に閉じる（`terminal_dispatch.rs`、`terminal_cells.rs` の doc コメント、およびテスト）。term_core の公開 API は変更せず、依存・開発依存も追加しない。

## 2. ビジネス要件

### 2.1 ビジネス目標

| ID | 目標 |
|----|------|
| BO1 | ASCII fast path を含む term_core の全ての print 経路で D2 不変条件（グリッド上に wide ペアの孤立した片割れを残さない）を成立させ、全角セルが ASCII で上書きされたときに幅 0 のスペーサーが残って見えることがないようにする |
| BO2 | コードベース自身のドキュメントを事実と一致させる。`blank_wide_pair_half` の doc コメントが列挙する D2 修復の呼び出し箇所は、実際に修復を行う経路の集合と一致すること |
| BO3 | ASCII fast path が存在する理由（ASCII 共通ケースでのバイト当たりコスト最小化）を保ったまま、正当性のギャップを閉じること |

### 2.2 対象ユーザー

要件分析で対象ユーザー区分は定義されていない（term_core 内部の正当性修正のため）。

### 2.3 期待される効果

- 全角セルを ASCII で上書きしても、孤立した幅 0 スペーサーがターミナルに残らない
- doc コメントの列挙から、読み手が「カバーされている経路」「されていない経路」を誤って読み取ることがなくなる
- ASCII 共通ケースの処理コストは維持される

## 3. ユースケース

要件分析の design ステップは skip 判定であり（理由: 変更が term_core の ANSI/グリッドエンジンに閉じた print 経路の正当性修正で、ユーザーに見える面・新規 UI・操作フロー・デザイントークンの利用がない）、ユースケースは定義されていない。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | ステータス |
|----|--------|------|-----------|
| FR1 | fast path が上書き前に orphan-neighbor 修復を行う | 書き込み前に対象セルの既存 width を読み、1 でないとき slow path と同じ orphan-neighbor blanking を呼ぶ | resolved |
| FR2 | fast path が overflow テーブルのエントリを削除する | 上書きした各セルの overflow エントリを `handle_print_ascii` と同じ形で削除する | resolved |
| FR3 | fast/slow path の観測上の等価性 | 同一バイト列に対し、fast path 経由でも slow path 経由でも結果が同一になる | resolved |
| FR4 | 修正のために fast path を狭めない | `can_fast_ascii` の受け入れ条件にグリッド状態の前提条件を追加しない | resolved |
| FR5 | D2 修復呼び出し箇所のドキュメントを実態に合わせる | `blank_wide_pair_half` の doc コメントの列挙を post-change のコードと一致させる | resolved |
| FR6 | 報告された破綻の回帰テスト | 報告シーケンスを再現し、孤立した幅 0 スペーサーが残らないことを検証する | resolved |
| FR7 | wide でない内容に対する挙動不変 | wide ペアセルに触れない入力では観測される出力が変更前と同一 | resolved |

### 4.2 機能詳細

#### FR1: fast path が上書き前に orphan-neighbor 修復を行う

**説明**: セルを書き込む前に、`crates/term_core/src/terminal_dispatch.rs` の ASCII fast path が対象セルの既存 width を読み、その width が 1 でないときに slow path と同じ orphan-neighbor blanking（`blank_orphaned_neighbor_before_overwrite`、基盤は `blank_wide_pair_half` プリミティブ）を呼び出す。これにより、壊れた wide ペアの生き残った片割れが孤立せずに blank される。

**ステータス**: resolved

#### FR2: fast path が overflow テーブルのエントリを削除する

**説明**: ASCII fast path は、上書きした各セルについて overflow テーブルのエントリを、slow path の `handle_print_ascii` と同じ形で削除する。

**ステータス**: resolved

#### FR3: fast/slow path の観測上の等価性

**説明**: 任意のバイト列について、そのバイトが ASCII fast path で消費されたか slow path の `handle_print_ascii` で消費されたかによらず、結果として得られるグリッド状態・セル幅・overflow テーブルの内容が同一であること。バイト列が任意の位置で複数の `process_pty_data` 呼び出しに分割された場合も含む。

**ステータス**: resolved

#### FR4: 修正のために fast path を狭めない

**説明**: 修正は fast path の書き込みステップの内部で実装する。`can_fast_ascii` の受け入れ条件にグリッド状態の前提条件（例: 「wide セルが存在しないこと」）を追加せず、fast path の対象となる入力の集合は変更しない。

**ステータス**: resolved

#### FR5: D2 修復呼び出し箇所のドキュメントを実態に合わせる

**説明**: `crates/term_core/src/terminal_cells.rs` の `blank_wide_pair_half` の doc コメントを更新し、D2 修復の呼び出し箇所の列挙が、print slow path（`handle_print_ascii` / `write_grapheme_to_grid`）、ICH/DCH、範囲消去に加えて dispatch の ASCII fast path を挙げるようにする。読み手が、カバーされていない経路をカバーされていると誤解する、あるいはカバーされている経路をされていないと誤解する余地を残さない。

**ステータス**: resolved

#### FR6: 報告された破綻の回帰テスト

**説明**: 報告されたシーケンス — 全角文字を出力し、続く `process_pty_data` 呼び出しで CR に続けて ASCII 文字を送って col 0 の wide base を上書きする — を再現し、col 1 に孤立した幅 0 スペーサーが残らないことを検証するユニットテストを置く。

**ステータス**: resolved

#### FR7: wide でない内容に対する挙動不変

**説明**: wide ペアセルに一切触れない入力に対して、fast path の観測可能な出力（グリッド内容、幅、発行されるコールバック）は変更前の挙動と同一であること。

**ステータス**: resolved

## 5. 非機能要件

### 5.1 パフォーマンス要件（NFR1）

ASCII 共通ケース（width 1 のセルを width 1 の文字で上書きする）に対する追加コストは、既に常駐しているフィールドの読み取り 1 回と分岐予測が効く分岐 1 つを超えないこと。追加のアロケーション、入力バッファに対する追加のパス、インライン化できない per-byte 関数呼び出しの追加を行わない。これは「NFR4 影響評価」という受け入れ基準の具体的な読み方であり、評価は本機能のドキュメントに記録する（暗黙のままにしない）。

### 5.2 スコープ要件（NFR2）

変更は `crates/term_core`（`terminal_dispatch.rs`、`terminal_cells.rs` の doc コメント、テスト）に閉じる。term_core の公開 API は変更せず、新規の依存・開発依存を追加しない。

### 5.3 堅牢性要件（NFR3）

敵対的な PTY 入力に対して修復が安全であること。列 0 にあり左隣が存在しないスペーサー、行末列にある wide base、wide ペアのスペーサーではなく結合文字の残余である幅 0 セル — これらすべてを、panic・範囲外インデックス・正当な隣接セルの blank なしに扱えること。

### 5.4 規約要件（NFR4）

新規テストは `test/README.md` に従うこと。対象コードの隣にインラインの `#[cfg(test)] mod tests {}` を置き、`<subject>_<scenario>_<expected>` 命名を用い、各テストで明示的に `TerminalCore` を構築し、入力は `process_pty_data` 経由で流し、内部状態ではなく観測可能なグリッド契約に対してアサートする。

### 5.5 その他の非機能要件

セキュリティ・可用性・互換性について、上記以外の要件は定義されていない。

## 6. UI/UX要件

該当なし。design ステップは skip 判定であり、ユーザーに見える面・新規 UI・操作フローはない。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 変更対象は `crates/term_core` に限定する（NFR2）
- term_core の公開 API を変更しない。新規の依存・開発依存を追加しない（NFR2）
- `can_fast_ascii` の受け入れ条件を狭めない。修正は fast path の書き込みステップ内部で行う（FR4）
- ASCII 共通ケースの追加コストは常駐フィールドの読み取りと分岐 1 つまで（NFR1）

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 対応策 |
|------|--------|
| 幅 0 セルは常に wide ペアのスペーサーとは限らない（結合文字も幅 0 セルを生む） | 「width == 0」だけで判定せず wide ペア関係を手掛かりに修復する。NFR3 と TS6 で担保（前提 A5 由来） |
| NFR1 の評価で ASCII 共通ケースに実測の劣化が出る可能性 | その場合のフォールバックはアプローチ (b)（`blank_wide_pair_half` の doc を print slow path に限定し、fast path を既知の例外として記録する）。本仕様は (b) を条件付き要件とせず、アプローチ (a) で確定させる（前提 A2 由来） |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: アプローチ (a) が実装されている。ASCII fast path は書き込み前に old_width を読み、old_width != 1 のとき `blank_orphaned_neighbor_before_overwrite` を呼び、`handle_print_ascii` と同じ形で overflow エントリを削除する（FR1, FR2）
- [ ] AC2: 全角出力の後、別の `process_pty_data` 呼び出しで CR + ASCII を渡しても孤立スペーサーが残らないことをユニットテストが証明する（FR6）
- [ ] AC3: ASCII fast path に対する性能影響が評価され、その評価が文書化されている。old_width の読み取りが width 1 の共通ケースに、常駐フィールドの読み取りと分岐を超えるコストを加えないことを示す（NFR1）
- [ ] AC4: `blank_wide_pair_half` の doc コメントが post-change のコードと整合する形で D2 修復の呼び出し箇所を列挙しており、カバーされていない print 経路をカバーされていると読み手が結論できない（FR5）
- [ ] AC5: 既存の term_core `--lib` スイートと src-tauri `--lib` スイートがいずれも通り、`cargo check --no-default-features` も成功する（FR7, NFR2）

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS1（unit・正常系）: 直前の `process_pty_data` 呼び出しで col 0 に全角文字が出力された状態で、続く呼び出しが CR に続けて ASCII バイトを渡すと、col 0 は width 1 の ASCII 文字、col 1 は width 1 の blank セルとなり、幅 0 スペーサーは残らない
- [ ] TS2（unit・等価性）: 全角文字に続いて CR と ASCII を含むバイト列を、(i) 単一の `process_pty_data` 呼び出しで渡した場合と (ii) ASCII の末尾が fast path 対象のチャンクに落ちるよう分割して渡した場合で、両者のグリッド内容・幅・overflow テーブルの状態が一致する
- [ ] TS3（unit・正常系）: 全角文字が col 0-1 を占める状態で、fast path の ASCII が col 1（幅 0 スペーサー）を上書きすると、col 0 の wide base が width 1 の blank に blank され、孤立した base が残らない
- [ ] TS4（unit・正常系）: overflow テーブルのエントリを持つセルを fast path の ASCII が上書きすると、overflow エントリが消え、同じ上書きに対する `handle_print_ascii` の結果と一致する
- [ ] TS5（unit・回帰）: wide セルのないグリッドに純 ASCII のストリームを fast path で処理すると、結果のグリッドは変更前の挙動と同一で、修復経路には入らない
- [ ] TS6（unit・境界値）: 列 0 のスペーサーと行末列の wide base をそれぞれ fast path の ASCII が上書きしても、panic や範囲外アクセスが起きず、隣接規則は行内にのみ適用される
- [ ] TS7（suite）: 変更適用後に term_core `--lib` スイート、src-tauri `--lib` スイート、CLI 限定の `cargo check` を実行し、いずれも通る

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| D2 不変条件 | グリッド上に wide ペアの孤立した片割れが存在しないこと |
| ASCII fast path | `crates/term_core/src/terminal_dispatch.rs` の `process_pty_data` 内にある、ASCII 共通ケースをバイト当たり最小コストで処理する経路 |
| print slow path | `handle_print_ascii` / `write_grapheme_to_grid` を通る print 経路 |
| wide base / スペーサー | 全角文字が占める 2 セルのうち、文字本体を持つ側（width 2 の base）と、続く width 0 のセル |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 本書の機能要件・非機能要件は requirements-analyst が解決済み（status: resolved）としたもののみを記載している

### 14.2 前提事項（requirements-analyst 由来）

- A1（確度: 高）: 選択されたアプローチは (a)「fast path を直す」であり、(b)「ギャップを文書化する」ではない。根拠: 機能スラグが "…-cleanup"（文書注記ではなく修復）であること、タスクが本件を「種別: バグ」に分類していること、選択肢 (b) が「直ちに直さない場合」というフォールバックとして書かれていること、制約セクションが (a) に対する唯一の指摘（old_width の読み取りコスト）に対し old_width が既に対象セルのキャッシュラインに載っていると先回りして答えていること。加えて FR5 が (b) の根底にある関心（ドキュメントの正確さ）を取り込んでいる
- A2（確度: 中）: NFR1 の評価で ASCII 共通ケースに実測の劣化が示された場合、フォールバックはアプローチ (b)（`blank_wide_pair_half` の doc を print slow path に限定し、fast path を既知の例外として記録する）。本仕様を (a) で確定させるため、条件付き要件ではなく前提として記録する
- A3（確度: 中）: タスクの受け入れ基準が引く "NFR4" は、先行仕様における「ASCII fast path はバイト当たりコストを最小化する」という要件を指す。元文書が読み取り範囲外だったため、本機能の NFR1 として言い換えて記載している
- A4（確度: 中）: PR #37（wide-pair-blank-primitive-unification）は本機能が分岐する base に既にマージ済みであり、`blank_wide_pair_half` と `blank_orphaned_neighbor_before_overwrite` は記載どおり存在する
- A5（確度: 中）: 幅 0 セルは常に wide ペアのスペーサーとは限らない（結合文字も幅 0 セルを生む）ため、修復は「width == 0」だけでなく wide ペア関係を手掛かりに行う必要がある。NFR3 と TS6 に反映している

## 15. 参考資料

- `crates/term_core/src/terminal_dispatch.rs`: ASCII fast path と `can_fast_ascii` の実装
- `crates/term_core/src/terminal_cells.rs`: `blank_wide_pair_half` プリミティブと D2 修復呼び出し箇所の doc コメント
- `test/README.md`: テストの配置・命名・記述規約（NFR4）
- PR #37 (wide-pair-blank-primitive-unification): `blank_wide_pair_half` / `blank_orphaned_neighbor_before_overwrite` の導入（前提 A4）
