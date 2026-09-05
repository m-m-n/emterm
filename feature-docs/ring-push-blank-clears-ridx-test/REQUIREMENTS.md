---
title: "ring-push-blank-clears-ridx-test"
created_date: 2026-09-04
status: draft
---

# ring-push-blank-clears-ridx-test - 要件定義書

## 1. 概要

### 1.1 背景

`test_ring_push_blank_clears_ridx` は、`ring_push_blank` の scrollback 有効
compress 分岐について「退避した行以外の overflow エントリを消していない」ことを
観測できていない。この行スコープ喪失の退行を検出できない箇所が term_core に
残っている。

兄弟テスト `test_ring_push_blank_clears_recycled_row_overflow_entries`
（feature `ring-push-blank-row-scope-test` / PR #48）では survivor 行方式を
導入済みだが、scrollback 有効側の分岐には適用されていない。

### 1.2 目的

- `test_ring_push_blank_clears_ridx` を、compress 分岐の行スコープ喪失を
  検出できるテストへ強化する。
- 兄弟テストで導入済みの survivor 行方式を scrollback 有効側の分岐にも適用し、
  被覆を揃える。
- テストが証明できる範囲と証明できない範囲（構造上の天井）を doc コメントとして
  固定する。

### 1.3 スコープ

**対象**:
- `crates/term_core/src/ring_buffer/tests.rs` の
  `test_ring_push_blank_clears_ridx`（`tests.rs:417` 付近）

**対象外**:
- `crates/term_core/src/ring_buffer.rs`（production コード）の恒久的な変更
- UI・レンダリング・ユーザー可視の振る舞い

## 2. ビジネス要件

### 2.1 ビジネス目標

- `test_ring_push_blank_clears_ridx` を、`ring_push_blank` の scrollback 有効
  compress 分岐が「退避した行以外の overflow エントリを消していない」ことを
  観測できるテストへ強化し、行スコープ喪失の退行を検出できない箇所を
  term_core から無くす。
- 兄弟テスト `test_ring_push_blank_clears_recycled_row_overflow_entries`
  （feature `ring-push-blank-row-scope-test` / PR #48）で導入済みの
  survivor 行方式を、scrollback 有効側の分岐にも適用して被覆を揃える。
- テストが証明できる範囲と証明できない範囲（構造上の天井）を doc コメントとして
  固定し、将来「compress 分岐の clear site 発火を直接固定せよ」という
  充足不能な要求が再提起されないようにする。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| term_core の開発者 | `ring_push_blank` を改変した際に、行スコープ喪失の退行をテストで検出したい |

### 2.3 期待される効果

- compress 分岐の row-scoped clear を全消しへ置き換える退行が、テストで
  検出されるようになる。
- 充足不能な受け入れ条件（clear site 発火の直接固定）が再提起されなくなる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 行スコープ喪失の退行検出 | term_core の開発者 | 高 |

### 3.2 ユースケース詳細

#### UC01: 行スコープ喪失の退行検出

**アクター**: term_core の開発者

**事前条件**:
- `crates/term_core/src/ring_buffer/tests.rs` の
  `test_ring_push_blank_clears_ridx` が強化済みであること

**基本フロー**:
1. 開発者が `ring_push_blank` の compress 分岐を変更する
2. `cargo test --manifest-path crates/term_core/Cargo.toml --lib` を実行する
3. 行スコープが失われている場合、`test_ring_push_blank_clears_ridx` が fail する

**代替フロー**:
- 行スコープが保たれている場合、テストは green のまま

**事後条件**:
- 退避行以外の overflow エントリを巻き込んで消す変更が検出される

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | survivor 行を含む fixture への差し替え | 回収行と survivor 行の 2 行に overflow 行きの content を置く | 高 |
| FR2 | 絶対行番号を push 前に控える | `viewport_abs(0)` / `viewport_abs(1)` で絶対行番号を保持 | 高 |
| FR3 | 反空虚性（anti-vacuity）の事前 assert | push 前に両行が overflow 行きであることを確認 | 高 |
| FR4 | 1 回 push 直後の行スコープ assert | 回収行のみが消え survivor が残ることを確認 | 高 |
| FR5 | 行スコープ assert のタイミング制約 | push 回数が rows に達する前に assert する | 高 |
| FR6 | 既存の空判定 assert の保持 | 合計 5 回 push 後の空判定 assert を残す | 高 |
| FR7 | 被覆範囲を doc コメントに明記 | 証明できる範囲/できない範囲と構造的理由を記載 | 高 |
| FR8 | 変異による red 確認 | 全消しへの変異でテストが red になることを確認し巻き戻す | 高 |

### 4.2 機能詳細

#### FR1: survivor 行を含む fixture への差し替え

**説明**: `test_ring_push_blank_clears_ridx`
（`crates/term_core/src/ring_buffer/tests.rs:417` 付近）の fixture を
`TerminalCore::new(10, 3, 2)`（cols=10 / rows=3 / scrollback capacity=2）の
まま、push 前に「最初の push で回収される行」と「回収されない survivor 行」の
2 行へ overflow 行きの content を置く形へ拡張する。基本形は
`long = "👨‍👩‍👧‍👦"` を `set_cell` で row 0 col 0 と row 1 col 1 に置く
（`set_cell` の引数順は col, row）。細部の座標・文字列は実装時に調整してよい。

**ビジネスルール**:
- fixture の構成は `TerminalCore::new(10, 3, 2)` を維持する。

#### FR2: 絶対行番号を push 前に控える

**説明**: `viewport_abs(0)` / `viewport_abs(1)` により回収行・survivor 行の
絶対行番号を push 前に取得して保持する。`ring_head` が push ごとに回るため、
push 後の viewport 相対座標は同じ絶対行を指さない。

#### FR3: 反空虚性（anti-vacuity）の事前 assert

**説明**: push 前に、回収行・survivor 行の双方について content が実際に
overflow 行きになっていることを `overflow` と `overflow_ridx` の両方で
assert する。既存の `assert!(!core.overflow_ridx.is_empty())` はこの事前
assert に含めるか、より具体的な行キー単位の assert へ強化する。

**ビジネスルール**:
- inline cap を超えられない fixture が事後 assert を無言で自明化する事故を防ぐ。

#### FR4: 1 回 push 直後の行スコープ assert

**説明**: `ring_push_blank(PackedColor::DEFAULT)` を 1 回だけ呼んだ時点で、
次の 4 点を assert する。

| # | assert 内容 |
|---|-------------|
| (a) | 回収行の絶対行キーに対応する `overflow` エントリが消えていること |
| (b) | 同じ行キーが `overflow_ridx` から消えていること |
| (c) | survivor 行の `overflow` エントリが残っていること |
| (d) | survivor 行の `overflow_ridx` メンバーシップが残っていること |

**ビジネスルール**:
- `overflow` と `overflow_ridx` は独立に確認する。

#### FR5: 行スコープ assert のタイミング制約

**説明**: FR4 の assert は push 回数が rows（=3）に達する前、すなわち
survivor 行自身が退避される前に行う。1 回 push 時点での assert が
これを満たす。

#### FR6: 既存の空判定 assert の保持

**説明**: FR4 の後にさらに 4 回 push し（合計 5 回、既存と同数）、既存の
`assert!(core.overflow.is_empty())` / `assert!(core.overflow_ridx.is_empty())`
をそのまま残す。

**ビジネスルール**:
- 行スコープ assert は「単一 eviction で過剰に消していない」こと、空判定
  assert は「全行が巡回した後に最終的に空になる」ことを検証しており、性質が
  別物であるため片方への置換は行わない。

#### FR7: 被覆範囲を doc コメントに明記

**説明**: テスト本体の doc コメントに、このテストが証明できるのは
「compress 分岐が退避行以外を巻き込んで消していない」ことであり、
「compress 分岐の clear site が発火した」ことではない旨を明記する。

**ビジネスルール**:
- 理由として、`ring_push_blank` には clear site が 3 つ
  （compress 分岐 / scrollback 無効分岐 / Step 3 の新 viewport 最下行の
  無条件 clear）あり、1 回の push 内で `new_bottom_abs == evicted_abs` が
  rows に依らず常に成立するため、eviction 時 clear と Step 3 の無条件 clear が
  必ず同一の絶対行を対象とすること（= どんな fixture でも両者を区別できない）を
  併記する。

#### FR8: 変異による red 確認

**説明**: `crates/term_core/src/ring_buffer.rs` の compress 分岐
（177-180 行付近）の `overflow_clear_row` / `overflow_ridx_clear_row` を
`self.overflow.clear()` / `self.overflow_ridx.clear()` に置き換えた変異状態で、
強化後の `test_ring_push_blank_clears_ridx` が red になることを確認する。

**ビジネスルール**:
- 確認後、変異は必ず巻き戻し、production コードの差分を残さない。

## 5. 非機能要件

### 5.1 パフォーマンス要件

**NFR3 - 決定性と実行時間**: テストは並列実行下でも決定的で、
`--test-threads=1` を要さない。`#[ignore]` を要する長時間パスにはしない。

### 5.2 セキュリティ要件

該当なし。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

**NFR1 - production コード無変更**: 変更対象は
`crates/term_core/src/ring_buffer/tests.rs` のみ。FR8 の変異は検証中の
一時的なものであり、最終成果物に `crates/term_core/src/ring_buffer.rs` の
差分を含めない。

**NFR2 - 既存テストスタイルへの追随**: 兄弟テスト
`test_ring_push_blank_clears_recycled_row_overflow_entries` の構成
（絶対行キーの事前取得 → 反空虚性 pre-assert → 操作 → removal / survival の
post-assert）と doc コメントの書きぶりを鏡写しにする。`test/README.md` の
規約どおり inline `#[cfg(test)] mod tests {}` に置き、新しい dev-dependency
（proptest / criterion 等）は追加しない。

**NFR4 - 書式**: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
が clean。crate 全体に対する広域 fmt は行わず、変更ファイルのみを整える。

### 5.5 互換性要件

該当なし。

## 6. UI/UX要件

該当なし。デザインステップは skipped。

**skipped 理由**: 変更対象は `crates/term_core/src/ring_buffer/tests.rs` の
テスト 1 本のみで、production コード・UI・レンダリング・ユーザー可視の
振る舞いに一切変更が及ばない。デザイン成果物（画面・トークン・レイアウト）を
要する要素が無い。

## 7. データ要件

### 7.1 データモデル概要

テストが直接参照する term_core の内部テーブル。

| テーブル | キー | 値 |
|----------|------|-----|
| `overflow` | `(col: u32, abs_row: u32)` | セルの overflow content |
| `overflow_ridx` | `abs_row: u32` | col 集合 |

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| fixture | cols | u32 | ○ | 10 |
| fixture | rows | u32 | ○ | 3 |
| fixture | scrollback capacity | u32 | ○ | 2 |

### 7.3 データ保持期間

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 変更対象ファイルは `crates/term_core/src/ring_buffer/tests.rs` のみ。
- 新しい dev-dependency（proptest / criterion 等）は追加しない。
- テストは inline `#[cfg(test)] mod tests {}` に置く。
- テスト名 `test_ring_push_blank_clears_ridx` は変更しない。
- 「compress 分岐の clear site が発火した」ことを Step 3 の無条件 clear と
  区別する受け入れ条件は原理的に充足不能であり、本仕様はそれを要求しない。

### 9.2 ビジネス上の制約

- FR8 の変異は一時的なものであり、production コードの差分を最終成果物に
  残さない。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の
各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/ring-push-blank-clears-ridx-test/**`
- `test-docs/ring-push-blank-clears-ridx-test/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、
`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、
`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、および
デザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメント
および `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式:
`test-docs/{feature}/{T}.tests.yaml`）。生成主体は `implement-phase.md` を
参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。
  除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に
  含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されて
  いても違反にはならない。implementタスクを1つも生成しないフィーチャーは
  `test-docs/{feature}/` ディレクトリを生成しないが、宣言された
  `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| inline cap を超えられない fixture が事後 assert を無言で自明化する | 高 | FR3 の反空虚性 pre-assert を置く |
| push 後の viewport 相対座標が同じ絶対行を指さない | 高 | FR2 で push 前に絶対行番号を控える |
| survivor 行自身が退避されると行スコープ assert が成立しない | 高 | FR5 のとおり 1 回 push 時点で assert する |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 充足不能な要求（clear site 発火の直接固定）の再提起 | 中 | 中 | FR7 の doc コメントで構造的理由を固定する |
| 変異が巻き戻されず production コードに差分が残る | 低 | 高 | NFR1 と AC5 で巻き戻しを受け入れ条件にする |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] **AC1**: `test_ring_push_blank_clears_ridx` の fixture に survivor 行が
      追加され、1 回 push 後に「回収行の `overflow` / `overflow_ridx` エントリが
      消え、survivor 行のエントリが残る」ことを、2 つのテーブルそれぞれについて
      assert している。（FR1, FR2, FR4, FR5）
- [ ] **AC2**: push 前に、回収行・survivor 行の双方が実際に overflow 行きで
      あることを `overflow` と `overflow_ridx` の両方で assert している。（FR3）
- [ ] **AC3**: 合計 push 回数が 5 回のまま維持され、既存の
      `core.overflow.is_empty()` / `core.overflow_ridx.is_empty()` の assert が
      削除も置換もされずに残っている。（FR6）
- [ ] **AC4**: テスト本体の doc コメントに、証明できる範囲（過剰に消していない
      こと）と証明できない範囲（compress 分岐の clear site が発火したこと）、
      および `new_bottom_abs == evicted_abs` により Step 3 の無条件 clear と
      区別できないという構造的理由が明記されている。（FR7）
- [ ] **AC5**: compress 分岐の row-scoped clear を全消しへ置換した変異状態で
      `test_ring_push_blank_clears_ridx` が red になることを確認し、その結果を
      検証記録として残したうえで、変異を巻き戻して production コードの差分が
      無い状態に戻している。（FR8, NFR1）
- [ ] **AC6**: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
      crates/term_core/Cargo.toml --lib` が green（強化後テストを含む term_core の
      --lib スイート全体）。（FR1, FR2, FR3, FR4, FR5, FR6, NFR3）
- [ ] **AC7**: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
      が clean。（NFR4）

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] **TS1** 正常系（行スコープ観測）: `TerminalCore::new(10, 3, 2)` に
      回収行・survivor 行の overflow content を置き、絶対行キーを控え、1 回
      `ring_push_blank` して回収行のみが消え survivor が残ることを確認する。
      （AC1, AC2）
- [ ] **TS2** 巡回後の全空（既存の性質）: TS1 の続きで残り 4 回 push
      （合計 5 回）し、`overflow` / `overflow_ridx` がともに空になることを
      確認する。（AC3）
- [ ] **TS3** 変異検出（mutation / red 確認）: `ring_buffer.rs` の compress
      分岐の row-scoped clear を `self.overflow.clear()` /
      `self.overflow_ridx.clear()` へ置換して
      `cargo test --manifest-path crates/term_core/Cargo.toml --lib` を実行し、
      `test_ring_push_blank_clears_ridx` が fail することを確認したのち
      復元する。（AC5）
- [ ] **TS4** 回帰（crate 全体）: `CARGO_TARGET_DIR=src-tauri/target cargo test
      --manifest-path crates/term_core/Cargo.toml --lib` を復元後に実行し、
      term_core の全 --lib テストが green であることを確認する。（AC6）
- [ ] **TS5** 書式: `cargo fmt --manifest-path crates/term_core/Cargo.toml
      --check` を実行して差分が無いことを確認する。（AC7）

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 回収行 | 最初の `ring_push_blank` で退避（evict）される行 |
| survivor 行 | 最初の push では回収されない行 |
| 反空虚性（anti-vacuity） | 事後 assert が自明に成立していないことを事前 assert で保証する性質 |
| clear site | `ring_push_blank` 内で overflow テーブルを消去するコード位置。compress 分岐 / scrollback 無効分岐 / Step 3 の新 viewport 最下行の無条件 clear の 3 つ |

## 14. 確認事項

### 14.1 確認済み事項

- [x] **A1** `set_cell` の引数順は `(col, row, ...)`（`tests.rs` の
      `test_scrollback_ordering_oldest_first` が row 0/1/2 を
      `set_cell(0, 0, "A", ...)` / `set_cell(0, 1, "B", ...)` で構築し、
      scrollback[0] が "A" になることから確認済み）。したがってタスク記述の
      「row 0 col 0 / row 1 col 1」は `set_cell(0, 0, ...)` /
      `set_cell(1, 1, ...)` に対応する。
- [x] **A2** rows=3 かつ push 前の `ring_head` は 0 のため、1 回目の push の
      `evicted_abs` は viewport row 0 の絶対行、survivor（viewport row 1）の
      絶対行は回収対象にならない。よって FR5 のタイミング条件は
      「1 回 push 直後の assert」で満たされる。
- [x] **A3** 既存テストが `assert!(!core.overflow_ridx.is_empty())` で
      確認しているとおり、`set_cell` に長い書記素クラスタを渡すと inline cap を
      超えて overflow 側テーブルに載る。survivor 行にも同じ文字列を用いることで
      同条件が成立する。
- [x] **A4** `overflow` のキーは `(col: u32, abs_row: u32)`、`overflow_ridx` の
      キーは `abs_row: u32` で値が col 集合。テストは同一クレート内の
      `#[cfg(test)]` モジュールなので、これらの `pub(crate)` フィールドへ
      直接アクセスできる。
- [x] **A5** テスト名 `test_ring_push_blank_clears_ridx` は変更しない
      （タスク記述が既存テストの「強化」を指定しており、名称変更の指示が
      無いため）。
- [x] **A6** clear site は 3 つで、1 回の push 内で
      `new_bottom_abs == evicted_abs` が rows に依らず常に成立する
      （Step 2 で `ring_head = (ring_head + 1) % rows`、Step 3 で
      `new_bottom_abs = (ring_head + rows - 1) % rows` = 旧 `ring_head` =
      `evicted_abs`）。したがって「compress 分岐の clear site が発火した」ことを
      Step 3 の無条件 clear と区別する受け入れ条件は原理的に充足不能であり、
      本仕様はそれを要求しない。

### 14.2 未確認・保留事項

なし。全ての要件が `confirmed`。

## 15. 参考資料

- `crates/term_core/src/ring_buffer/tests.rs`: 強化対象テストの所在
  （`test_ring_push_blank_clears_ridx`、417 行付近）
- `crates/term_core/src/ring_buffer.rs`: compress 分岐（177-180 行付近）
- 兄弟テスト `test_ring_push_blank_clears_recycled_row_overflow_entries`:
  feature `ring-push-blank-row-scope-test` / PR #48
- `test/README.md`: テスト配置の規約
