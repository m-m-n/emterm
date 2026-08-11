---
title: "wide-pair-overwrite-cleanup"
created_date: 2026-08-11
status: draft
---

# wide-pair-overwrite-cleanup - 要件定義書

## 1. 概要

### 1.1 背景

term_core のセル書き込みで、wide ペア（幅2 base + spacer）の半分だけを上書きしたときに相方セルが掃除されない。その結果、⏭️（U+23ED + VS16）を含む行のストリーム描画で罫線ズレ・文字重なりが発生し、Ctrl+L まで残留する。

### 1.2 目的

term_core のセル書き込みで wide ペア（幅2 base + spacer）の半分だけを上書きしたとき相方セルを空白化してグリッド不変条件を守り、⏭️（U+23ED + VS16）を含む行のストリーム描画乱れ（罫線ズレ・文字重なり・Ctrl+L まで残留）を解消する。

### 1.3 スコープ

**対象**

- 幅2 base への幅1 上書き時の旧 spacer 空白化（FR1）
- spacer への上書き時の base 空白化（FR2）
- 幅2 書き込みの placeholder 作成時の連鎖掃除（FR3）
- `widen_after_merge` の spacer 作成箇所への同規則適用（FR4）
- 修正対象は term_core 内の共通コード（`write_grapheme_to_grid` / `handle_print_ascii` / `widen_after_merge`）

**実装時判断（保留）**

- `csi_edit` 系（ECH/DCH/ICH）への同種掃除の適用（FR5）

**対象外**

- Claude Code（アプリ側）の幅モデル特定・修正
- mux 経路固有の追加要因調査

## 2. ビジネス要件

### 2.1 ビジネス目標

term_core のセル書き込みで wide ペア（幅2 base + spacer）の半分だけを上書きしたとき相方セルを空白化してグリッド不変条件を守り、⏭️（U+23ED + VS16）を含む行のストリーム描画乱れ（罫線ズレ・文字重なり・Ctrl+L まで残留）を解消する。

### 2.2 対象ユーザー

本要件では該当なし（確定要件に記載なし）。

### 2.3 期待される効果

- ⏭️ を含む行のストリーム描画で罫線ズレ・文字重なりが発生しない
- Ctrl+L まで残留する描画残骸が発生しない

## 3. ユースケース

本要件では該当なし（確定要件に記載なし）。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | ステータス |
|----|--------|------|------------|
| FR1 | 幅2 base への幅1 上書きで旧 spacer を空白化 | 幅2 base セルの上に幅1 文字を書いたとき、col+1 の旧 spacer を空白（幅1 blank）にする | resolved |
| FR2 | spacer への上書きで base を空白化 | spacer（幅0）セルの上に文字を書いたとき、col-1 の幅2 base を空白（幅1 blank）にする | resolved |
| FR3 | 幅2 書き込みの placeholder 作成時の連鎖掃除 | 幅2 書き込みで col+1 に placeholder を作るとき、col+1 が別ペアの base だった場合はその spacer（col+2）も空白化する | resolved |
| FR4 | widen_after_merge の spacer 作成箇所への適用 | 遡及 widen（widen_after_merge）が col+1 に spacer を作る箇所にも FR3 と同じ相方掃除規則を適用する | resolved |
| FR5 | csi_edit 系（ECH/DCH/ICH）への同種掃除の適用 | csi_edit の ECH/DCH/ICH 系にも wide ペア相方掃除が無い。これをスコープに含めるかどうか | tbd |

### 4.2 機能詳細

#### FR1: 幅2 base への幅1 上書きで旧 spacer を空白化

**説明**: 幅2 base セルの上に幅1 文字を書いたとき、col+1 の旧 spacer を空白（幅1 blank）にする（レポート P3: 孤児 spacer 残留の解消）。

#### FR2: spacer への上書きで base を空白化

**説明**: spacer（幅0）セルの上に文字を書いたとき、col-1 の幅2 base を空白（幅1 blank）にする（レポート P4: base 残留によるグリフ重なりの解消）。

#### FR3: 幅2 書き込みの placeholder 作成時の連鎖掃除

**説明**: 幅2 書き込みで col+1 に placeholder を作るとき、col+1 が別ペアの base だった場合はその spacer（col+2）も空白化する。

#### FR4: widen_after_merge の spacer 作成箇所への適用

**説明**: 遡及 widen（`widen_after_merge`）が col+1 に spacer を作る箇所にも FR3 と同じ相方掃除規則を適用する。

#### FR5: csi_edit 系（ECH/DCH/ICH）への同種掃除の適用

**説明**: `csi_edit` の ECH/DCH/ICH 系にも wide ペア相方掃除が無い。これをスコープに含めるかどうか。

**ステータス**: tbd

**保留理由**: task_description の制約・前提でユーザーが「スコープに含めるかは実装時に判断する」と明示的に保留している。未解決の質問ではなくユーザー決定済みの実装時判断事項。

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 分類 | 内容 | ステータス |
|----|------|------|------------|
| NFR1 | 回帰安全 | wide ペアに関与しない通常の書き込み経路の挙動を変えない | resolved |
| NFR2 | 互換性 | 相方セル空白化の挙動は xterm / Alacritty / WezTerm の実装慣行（上書き時に相方セルを空白化）と整合させる | resolved |
| NFR3 | 保守性 | 修正は term_core 内の共通コード（write_grapheme_to_grid / handle_print_ascii / widen_after_merge）に閉じ、mux 経路（daemon parse → GUI parse）にも同一コードで効く | resolved |
| NFR4 | パフォーマンス | handle_print_ascii は ASCII 高速パスであるため、旧セル状態チェックの追加で通常経路（wide ペア非関与時）の性能特性を損なわない | resolved |

### 5.2 パフォーマンス要件

- NFR4: `handle_print_ascii` は ASCII 高速パスであるため、旧セル状態チェックの追加で通常経路（wide ペア非関与時）の性能特性を損なわない。

### 5.3 互換性要件

- NFR2: 相方セル空白化の挙動は xterm / Alacritty / WezTerm の実装慣行（上書き時に相方セルを空白化）と整合させる。

### 5.4 保守性要件

- NFR3: 修正は term_core 内の共通コード（`write_grapheme_to_grid` / `handle_print_ascii` / `widen_after_merge`）に閉じ、mux 経路（daemon parse → GUI parse）にも同一コードで効く。

### 5.5 回帰安全要件

- NFR1: wide ペアに関与しない通常の書き込み経路の挙動を変えない。

### 5.6 セキュリティ要件・可用性要件

本要件では該当なし（確定要件に記載なし）。

## 6. UI/UX要件

本要件では該当なし（UI・ビジュアル要素が一切無い term_core のグリッド書き込みロジックのバグ修正）。

## 7. データ要件

本要件では該当なし（確定要件に記載なし）。

## 8. 外部連携

本要件では該当なし（確定要件に記載なし）。

## 9. 制約条件

### 9.1 技術的制約・前提

- task_description の行番号（print_handler.rs :68-146 / :149-183 / :280-333）は main df054f53 時点。integration base は 00c06f35 のため行番号はズレうるが、関数名（`write_grapheme_to_grid` / `handle_print_ascii` / `widen_after_merge`）をアンカーとする。
- Claude Code（アプリ側）の幅モデル特定・修正、および mux 経路固有の追加要因調査はスコープ外。
- 調査レポート原本 `tmp/vs16-wide-pair-overwrite-2026-08-11.md` は gitignored でワークツリーから読めないが、全文が task_description に埋め込まれておりそれを一次入力とする。

### 9.2 ビジネス上の制約・スケジュール制約

本要件では該当なし（確定要件に記載なし）。

## 10. 想定される課題とリスク

本要件では該当なし（確定要件に記載なし）。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] 幅2 base の上に幅1 文字を書いたとき、col+1 の旧 spacer が空白化される（レポート P3）
- [ ] spacer の上に文字を書いたとき、col-1 の base が空白化される（レポート P4）
- [ ] 幅2 書き込みの placeholder 作成時（col+1）、そこが別ペアの base だった場合はその spacer（col+2）も空白化される
- [ ] widen_after_merge の spacer 作成箇所（col+1 上書き）にも同じ規則が適用される
- [ ] レポートの P3 / P4 / P5 の再現手順がユニットテストとして追加され、回帰ガードになる
- [ ] ⏭️ を含むテーブルのストリーム描画で乱れが再現しないこと（実機確認）

### 11.2 KPI

本要件では該当なし（確定要件に記載なし）。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] P3 再現: 幅2 ペア（⏭️ = U+23ED + U+FE0F）を書いた後、base 位置に幅1 文字を上書きし、`get_cell_char` / `get_cell_width` で col+1 が空白（幅1）であることを検証するユニットテスト
- [ ] P4 再現: spacer 位置に幅1 文字を上書きし、col-1 の base が空白（幅1）で幅2 グリフが残っていないことを検証するユニットテスト
- [ ] P5 再現: 行を 1 桁ズラして書き直し（フレーム間の列幅変化を模擬）、旧フレームの残骸が残らないことを検証するユニットテスト
- [ ] チャンク分割耐性の既存挙動維持: U+23ED と VS16 を別チャンクで流しても遡及 widen が正常（レポート P1/P2 の正常系）
- [ ] 実機確認（受け入れ基準 6 項目目）: E2E 基盤が存在しないため、⏭️ を含むテーブルの Claude Code ストリーム描画をユーザーが手動確認する

### 12.2 テスト実装規約

- テストは `crates/term_core` の inline `#[cfg(test)] mod tests` に、命名規約 `<subject>_<scenario>_<expected>` で追加し、`process_pty_data` で駆動して観測可能なグリッド契約（`get_cell_char` / `get_cell_width`）に対して assert する。
- 実行コマンド: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| wide ペア | 幅2 base セルと、その直後の spacer セルの組 |
| base | 幅2 文字を保持するセル |
| spacer | 幅2 文字の右半分を占める幅0 のプレースホルダセル |
| 遡及 widen | `widen_after_merge` による、VS16 結合後に幅2 へ広げる処理 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] FR5（csi_edit 系 ECH/DCH/ICH への同種掃除）の扱い: スコープに含めるかは実装時に判断する、とユーザーが明示的に保留
- [x] 調査レポート原本 `tmp/vs16-wide-pair-overwrite-2026-08-11.md` の扱い: gitignored で読めないが全文が task_description に埋め込まれており、それを一次入力とする
- [x] task_description の行番号の基準: main df054f53 時点。integration base 00c06f35 とはズレうるため関数名をアンカーとする
- [x] design ステップ: skipped。UI・ビジュアル要素が一切無い term_core（グリッド書き込みロジック）のバグ修正であり、受け入れ条件が調査レポート由来で既に具体的に確定しているため、デザイン検討の対象が存在しない

### 14.2 未確認・保留事項

- [ ] FR5: csi_edit 系（ECH/DCH/ICH）への同種掃除の適用 - task_description の制約・前提でユーザーが「スコープに含めるかは実装時に判断する」と明示的に保留している。未解決の質問ではなくユーザー決定済みの実装時判断事項

## 15. 参考資料

- 調査レポート `tmp/vs16-wide-pair-overwrite-2026-08-11.md`: gitignored のためワークツリーから読めない。全文は task_description に埋め込まれている
- term_core 該当関数: `write_grapheme_to_grid` / `handle_print_ascii` / `widen_after_merge`（print_handler.rs）
