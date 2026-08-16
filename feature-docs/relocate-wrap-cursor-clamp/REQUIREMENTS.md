---
title: "relocate-wrap-cursor-clamp"
created_date: 2026-08-16
status: draft
---

# relocate-wrap-cursor-clamp - 要件定義書

## 1. 概要

### 1.1 背景

`crates/term_core/src/print_handler.rs` の `relocate_widened_base_via_wrap` 末尾には無条件の
`self.cursor.col = 2;`（現 524 行）がある。これは `cols <= 2` のときグリッド範囲外を指し、次に
印字される 1 文字が黙って捨てられる。

同じ VS16 幅広化処理でも、非最終列経路の `widen_after_merge`（print_handler.rs:423-431）は
`cols` 境界でカーソルをクランプする形を取っており、最終列経路の
`relocate_widened_base_via_wrap`（print_handler.rs:524-525）だけがそのカーソル契約から外れている。

既存の cols=1 テスト（`crates/term_core/src/print_handler/tests.rs:1589-1601`）は panic しないこと
しか確認しておらず、カーソル位置を検証しないまま「カバー済み」として記録されている。

先行 feature relocate-wrap-overflow-cleanup の review round1 では、この件が
フォローアップ finding `3e769a761d85d839`（reviews/round1.yaml:129-152, severity medium,
confidence 80）として unresolved のまま起票されている。

### 1.2 目的

- `cols <= 2` で relocate 直後のカーソルがグリッド範囲外を指し、次の 1 文字が黙って捨てられる
  既存欠陥を解消する。
- 非最終列経路（`widen_after_merge`）と最終列経路（`relocate_widened_base_via_wrap`）で
  カーソル契約を一致させる。
- 既存 cols=1 テストにカーソル契約の検証を加え、「panic しないことしか見ていない」状態を是正する。
- フォローアップ finding `3e769a761d85d839` を閉じる。

### 1.3 スコープ

**対象**

- 本番コード: `crates/term_core/src/print_handler.rs` の `relocate_widened_base_via_wrap` 末尾の
  カーソル更新のみ。
- テスト: `crates/term_core/src/print_handler/tests.rs` の変更・追加のみ。

**対象外**

- 他モジュール・他クレート・公開 API。
- cols=1 で spacer 列を確保できないまま基底セルに width=2 が書かれる点（既存の縮退時の
  振る舞いとして扱う。5.2 NFR2 を参照）。
- E2E（本プロジェクトに E2E 基盤は存在しない。5.5 NFR5 を参照）。

## 2. ビジネス要件

### 2.1 ビジネス目標

- `relocate_widened_base_via_wrap` 末尾の無条件 `self.cursor.col = 2;` が cols <= 2 で
  グリッド範囲外を指し、次に印字される 1 文字が黙って捨てられる既存欠陥を解消する。
- 同じ VS16 幅広化処理の非最終列経路（`widen_after_merge`, print_handler.rs:423-431）と
  最終列経路（`relocate_widened_base_via_wrap`, print_handler.rs:524-525）でカーソル契約を
  一致させ、片方だけが契約から外れている状態を解消する。
- 既存の cols=1 テスト（print_handler/tests.rs:1589-1601）が panic しないことしか確認しておらず、
  カーソル位置を検証しないまま「カバー済み」として記録されている状態を是正する。
- relocate-wrap-overflow-cleanup の review round1 で unresolved のまま起票された
  フォローアップ finding `3e769a761d85d839`（reviews/round1.yaml:129-152, severity medium,
  confidence 80）を閉じる。

### 2.2 対象ユーザー

要件分析にユーザー区分の定義は含まれない（本 feature は `term_core` 内部のカーソル更新の
修正であるため）。

### 2.3 期待される効果

- cols <= 2 のグリッドで relocate 直後に印字された文字が失われなくなる。
- 幅広化処理の 2 経路がカーソル契約について同形になる。
- cols=1 / cols=2 のカーソル契約がテストで固定される。

## 3. ユースケース

要件分析にユースケースの定義は含まれない。本 feature の対象はユーザー操作を伴わない
`term_core` 内部処理であり、外部から観測されるのはカーソル位置・`wrap_pending`・セル内容の
結果のみである（受け入れ基準は 11.1 を参照）。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 状態 |
|----|--------|------|
| FR1 | relocate 末尾のカーソル更新を widen_after_merge と字面まで同形のクランプに置き換える | ok |
| FR2 | cols <= 2 ではカーソルを最終列にクランプし wrap_pending を立てる | ok |
| FR3 | cols >= 3 の既存挙動（col=2 / wrap_pending=false）を維持する | ok |
| FR4 | relocate のカーソル以外の後処理を変更しない | ok |
| FR5 | 既存 cols=1 テストにカーソル契約のアサートを追加する | ok |
| FR6 | cols=2 の回帰テストを新規に追加する | ok |
| FR7 | クランプ後に次の 1 文字が落ちないことをテストで固定する | ok |

### 4.2 機能詳細

#### FR1: relocate 末尾のカーソル更新を widen_after_merge と字面まで同形のクランプに置き換える

`crates/term_core/src/print_handler.rs` の `relocate_widened_base_via_wrap` 末尾にある無条件の
`self.cursor.col = 2;`（現 524 行）を、`widen_after_merge`（同ファイル 423-431 行）と字面まで
同形の分岐に置き換える。すなわち再配置後の基底セル列 0 を起点とする `new_col`（= 0 + 2）を
`self.cols` と比較し、
`if new_col >= self.cols as u32 { if self.get_mode(MODE_AUTO_WRAP) { self.cursor.col = self.cols - 1; self.wrap_pending = true; } } else { ... }`
の形にする。内側の `if self.get_mode(MODE_AUTO_WRAP)` ガードを省略せずに含めること
（回答 mirror-verbatim）。

#### FR2: cols <= 2 ではカーソルを最終列にクランプし wrap_pending を立てる

`new_col >= self.cols`（すなわち cols <= 2）のとき、`self.cursor.col = self.cols - 1;` と
`self.wrap_pending = true;` を設定する。これにより relocate 直後のカーソルは常にグリッド内を
指し、次の印字は wrap 経由で次行へ送られる。

#### FR3: cols >= 3 の既存挙動（col=2 / wrap_pending=false）を維持する

`new_col < self.cols`（cols >= 3）のとき `self.cursor.col = 2;` に加えて現行の
`self.wrap_pending = false;`（現 525 行）を保持する。内部ヘルパ `carriage_return`
（terminal_core.rs:869-871）と `line_feed`（同 874-887）はいずれも `wrap_pending` を触らないため、
relocate に入った時点で立っている `wrap_pending` はこの 1 行でしか降りない。
`widen_after_merge` の else 分岐にはこの行が無いが、そこは元から `wrap_pending` が false の
経路であるため、同形化はカーソル列の分岐構造に対して適用し、この 1 行は残す。

#### FR4: relocate のカーソル以外の後処理を変更しない

`self.last_write = Some((0, new_row));`（現 526 行）をはじめ、セル内容の移送・overflow テーブル
整合・`ring_wrapped` の設定・dirty マークなど `relocate_widened_base_via_wrap` のカーソル更新
以外の処理は一切変更しない。

#### FR5: 既存 cols=1 テストにカーソル契約のアサートを追加する

`test_relocate_widened_base_via_wrap_no_panic_when_column_one_does_not_exist`
（crates/term_core/src/print_handler/tests.rs:1589-1601）に、現行のセル文字とカーソル行の
アサートに加えて、カーソル列が 0（= cols - 1）であることと `wrap_pending` が true であることの
アサートを追加する。テスト名とコメントも「panic しないこと」だけでなくカーソル契約を固定する
意図に合わせて更新する。

#### FR6: cols=2 の回帰テストを新規に追加する

spacer 列（col 1）は存在するが `new_col == cols` になる cols=2 のケースを新規テストとして
追加する。'A' と '5' を印字して基底セルを最終列（col 1）に置き、VS16 で relocate を起こし、
再配置先のセル・spacer・カーソル列・`wrap_pending` を検証する。

#### FR7: クランプ後に次の 1 文字が落ちないことをテストで固定する

cols=1 と cols=2 の双方で、relocate 直後に更に 1 文字を印字したときその文字がグリッド上に
現れる（修正前のように `cell_index` が None を返して黙って捨てられない）ことをアサートする。

## 5. 非機能要件

### 5.1 NFR1: 変更範囲を term_core のカーソル更新とテストに限定する

本番コードの変更は `crates/term_core/src/print_handler.rs` の `relocate_widened_base_via_wrap`
末尾のカーソル更新のみ。テストの変更・追加は `crates/term_core/src/print_handler/tests.rs` のみ。
他モジュール・他クレート・公開 API には触れない。

### 5.2 NFR2: cols=1 で spacer 無しの width=2 基底セルが残る点はスコープ外

cols=1 では spacer 列を確保できないまま基底セルに width=2 が書かれる。これは
`viewport_cell_offset` の `col < self.cols` 境界チェック（ring_buffer.rs:93-106）により col 1 への
書き込みが丸ごとスキップされることに由来する既存の縮退時の振る舞いであり、auto-wrap off の
幅広化分岐（print_handler.rs:385-387）や既存の wide-char 書き込み経路も同じ形を取る。
本 feature はこれを変更せず、スコープ外として明記する（回答 cursor-only）。

### 5.3 NFR3: cols >= 3 に対する外部から観測可能な挙動変更を持ち込まない

cols >= 3 のケースでは、カーソル位置・`wrap_pending`・セル内容・overflow テーブルのいずれに
ついても現行と同一の結果を返す。既存テスト
`test_retroactive_widen_at_last_column_wraps_with_autowrap`（tests.rs:662-685）は無改変で green を
維持する。

### 5.4 NFR4: 既存のテスト規約とフォーマット規約に従う

テストは term_core の既存規約どおり `crates/term_core/src/print_handler/tests.rs` のインライン
テストモジュールに置く（別 tests/ ディレクトリを新設しない）。整形は rustfmt
（style_edition 2024）に従い、変更・追加した行のみを対象にし、無関係な既存行を再整形しない。

### 5.5 NFR5: E2E 対象外

本プロジェクトに E2E 基盤は存在せず（`resolved_input_paths.e2e` は空、参照した workflow.yaml でも
全コンポーネントの `e2e_test_command` が空文字）、本 feature は E2E を新設しない。検証は
term_core の単体テストで完結する。

## 6. UI/UX要件

該当なし。変更は `crates/term_core` 内のカーソル更新 1 箇所とその単体テストに閉じており、
UI 表面・画面レイアウト・視覚的成果物・デザイントークンのいずれにも一切触れない
（design ステップは skipped）。

## 7. データ要件

該当なし。永続データモデル・データ保持期間に関する要件は要件分析に含まれない。

## 8. 外部連携

該当なし。外部システム連携・API 連携に関する要件は要件分析に含まれない。

## 9. 制約条件

### 9.1 技術的制約

- 本番コードの変更対象は `relocate_widened_base_via_wrap` 末尾のカーソル更新のみ（NFR1）。
- テストは `crates/term_core/src/print_handler/tests.rs` のインラインテストモジュールに置く（NFR4）。
- 整形は rustfmt（style_edition 2024）に従い、変更・追加した行のみを対象にする（NFR4）。
- cols >= 3 に対して外部から観測可能な挙動変更を持ち込まない（NFR3）。
- E2E 基盤が存在しないため、検証は term_core の単体テストで完結する（NFR5）。

### 9.2 ビジネス上の制約

- コンポーネント定義（term_core / main / cli の各コマンド）は先行 feature
  relocate-wrap-overflow-cleanup の workflow.yaml:15-33 をそのまま引き継ぐ。

### 9.3 スケジュール制約

要件分析にスケジュール制約は含まれない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 対応策 |
|------|--------|
| relocate 側に置く内側の `if self.get_mode(MODE_AUTO_WRAP)` ガードは常に true になり、偽側は到達不能でテストからもカバーされない（呼び出し元が `widen_after_merge` の auto-wrap 分岐の内側 print_handler.rs:379-380 のみであるため） | 到達不能分岐 1 本のコストより、受け入れ条件・レビュー指摘との字面照合可能性を優先する（回答 mirror-verbatim） |
| cols >= 3 側の `self.wrap_pending = false;` を同形化の名目で落とすと、`carriage_return` / `line_feed` が `wrap_pending` を触らないため true のまま残り、既存テスト tests.rs:662-685 が壊れる | 同形化はカーソル列の分岐構造に対して適用し、この 1 行は保持する（FR3） |
| cols=1 では spacer 列を確保できないまま基底セルに width=2 が書かれる | 既存の縮退時の振る舞いとして扱い、本 feature では変更しない（NFR2） |

### 10.2 ビジネスリスク

要件分析にビジネスリスクの記載は含まれない。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: cols=1（`TerminalCore::new(1, 3, 0)`）で '5' の後に VS16 を印字したとき、カーソル行が 1、
      カーソル列が 0、`wrap_pending` が true であり、セル (0,1) の内容が `"5\u{FE0F}"` である。
- [ ] AC2: cols=2（`TerminalCore::new(2, 3, 0)`）で 'A'、'5' の後に VS16 を印字したとき、カーソル行が 1、
      カーソル列が 1、`wrap_pending` が true であり、セル (0,1) の内容が `"5\u{FE0F}"`・width が 2、
      セル (1,1) の width が 0（spacer）である。
- [ ] AC3: AC1 / AC2 の直後に更に 1 文字を印字したとき、その文字がグリッド上のセルとして観測できる
      （修正前のようにカーソル列がグリッド外を指して黙って捨てられない）。
- [ ] AC4: cols=5 の既存シナリオでは、カーソル行 1・カーソル列 2・`wrap_pending` false・後続の 'X' が
      セル (2,1) という現行の結果が変わらない。既存テスト tests.rs:662-685 は無改変で green。
- [ ] AC5: 実装されたクランプが `widen_after_merge`（print_handler.rs:423-431）と字面まで同形であり、
      内側の `if self.get_mode(MODE_AUTO_WRAP)` ガードを含んでいる。
- [ ] AC6: term_core の build / test / format コマンド（`project.components.term_core` の各コマンド）が
      成功する。

### 11.2 KPI

要件分析に KPI の定義は含まれない。

## 12. テストシナリオ

### 12.1 テスト観点

| ID | シナリオ | 内容 | 修正前の期待 | 対応要件 |
|----|----------|------|--------------|----------|
| TS1 | cols=1 の既存テスト拡張 | `test_relocate_widened_base_via_wrap_no_panic_when_column_one_does_not_exist`（tests.rs:1589-1601）にカーソル列 0 と `wrap_pending=true` のアサートを追加する。 | red（修正前は `cursor.col` が 2 のまま） | FR2, FR5, FR7 |
| TS2 | cols=2 の新規回帰テスト | `TerminalCore::new(2, 3, 0)` で 'A'、'5'、VS16 を印字し、AC2 の各値を検証する。 | red（修正前は `cursor.col` が 2 で範囲外、`wrap_pending` が false） | FR2, FR6 |
| TS3 | cols=2 で次の 1 文字が落ちない | TS2 に続けて 'X' を印字し、'X' がグリッド上（wrap 先の行の col 0）に現れることを検証する。 | red（修正前は `cell_index` が None を返し 'X' が消える） | FR7 |
| TS4 | cols=1 で次の 1 文字が落ちない | TS1 に続けて 'X' を印字し、'X' がグリッド上（wrap 先の行の col 0）に現れることを検証する。 | red（修正前は 'X' が消える） | FR7 |
| TS5 | cols=5 の既存挙動が不変 | `test_retroactive_widen_at_last_column_wraps_with_autowrap`（tests.rs:662-685）を無改変で実行し green を維持することを確認する。 | green（修正後も green） | FR3, NFR3 |
| TS6 | コンポーネントコマンド | term_core の `build_command` / `test_command` / `format_command` を実行して成功を確認する。 | n/a | NFR1, NFR4 |

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `relocate_widened_base_via_wrap` | 最終列で VS16 による幅広化が起きたときに基底セルを次行へ再配置する print_handler.rs の処理。本 feature のカーソル更新の変更対象。 |
| `widen_after_merge` | 非最終列経路の VS16 幅広化処理（print_handler.rs:423-431）。cols 境界でカーソルをクランプする形を持ち、本 feature が字面まで同形化する参照元。 |
| `wrap_pending` | 次の印字を wrap 経由で次行へ送るかどうかを表す状態。`carriage_return`（terminal_core.rs:869-871）と `line_feed`（同 874-887）はこれを触らない。 |
| spacer | 幅広セルの後続列に置かれる width=0 のセル。 |
| VS16 | 直前の文字を幅広化させる異体字セレクタ。 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] クランプの形（`requirement.clamp-shape.autowrap-guard`）: mirror-verbatim。
      `widen_after_merge`（print_handler.rs:423-431）と字面まで同形にし、内側の auto-wrap ガードを
      含める。その帰結として relocate 側のガードは常に true になり、偽側は到達不能で、テストでも
      カバーされない。到達不能分岐 1 本のコストより、受け入れ条件・レビュー指摘との字面照合
      可能性を優先する判断である（呼び出し元は `widen_after_merge` の
      `if self.get_mode(MODE_AUTO_WRAP)` 分岐の内側 print_handler.rs:379-380 のみ）。
- [x] スコープ（`requirement.scope.cols1-width2-base`）: cursor-only。cols=1 で spacer 列を持てない
      まま基底セルに width=2 が書かれる点は既存の縮退時の振る舞いとして扱い、本 feature では
      変更しない。スコープはカーソルのクランプとテストに限定する。
- [x] 同形化の適用範囲（コード事実: terminal_core.rs:869-887, print_handler.rs:423-431,
      tests.rs:662-685）: 同形化は「cols 境界でのカーソル列の分岐構造」に対して適用し、in-range 側の
      `self.wrap_pending = false;` は現行どおり保持する。`carriage_return` も `line_feed` も
      `wrap_pending` を触らないため、この 1 行を落とすと cols >= 3 で `wrap_pending` が true のまま
      残り、既存テスト tests.rs:662-685 の `assert!(!core.get_wrap_pending())` と後続 'X' の配置が
      壊れる。`widen_after_merge` の else 分岐にこの行が無いのは、そちらが元から `wrap_pending`
      false の経路だからである。
- [x] design ステップ（`design-step.recommendation`）: skip。これは analyst 推奨の自動採用
      （batch decision table）による決定であり、視覚的成果物の要否について別途ユーザー判断を
      得たものではない。
- [x] コンポーネント定義: term_core / main / cli の各コマンドは先行 feature
      relocate-wrap-overflow-cleanup の workflow.yaml:15-33 をそのまま引き継ぐ。E2E 基盤は存在
      しないため `e2e_test_command` は空のままとする。

### 14.2 未確認・保留事項

なし。全要件（FR1-FR7 / NFR1-NFR5）が `status: ok` であり、保留（tbd）の要件は存在しない。

## 15. 参考資料

- `crates/term_core/src/print_handler.rs`: `relocate_widened_base_via_wrap`（524-526 行）、
  `widen_after_merge`（423-431 行）、呼び出し元（379-380 行）、auto-wrap off の幅広化分岐
  （385-387 行）
- `crates/term_core/src/print_handler/tests.rs`: 既存 cols=1 テスト（1589-1601 行）、
  cols=5 の既存テスト（662-685 行）
- `crates/term_core/src/terminal_core.rs`: `carriage_return`（869-871 行）、`line_feed`（874-887 行）
- `crates/term_core/src/ring_buffer.rs`: `viewport_cell_offset` の境界チェック（93-106 行）
- `feature-docs/relocate-wrap-overflow-cleanup/reviews/round1.yaml`: フォローアップ finding
  `3e769a761d85d839`（129-152 行, severity medium, confidence 80）
- `feature-docs/relocate-wrap-overflow-cleanup/workflow.yaml`: コンポーネント定義（15-33 行）
