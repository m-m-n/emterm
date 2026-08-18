---
title: "stale-test-name-refs"
created_date: 2026-08-18
status: draft
---

# stale-test-name-refs - 要件定義書

> **識別子の表記について**
> 本書では旧テスト識別子を `OLD_ID`、新テスト識別子を `NEW_ID` と記す。
> `OLD_ID` の完全な文字列を本書に直接書くと、FR5 / AC-2 のリポジトリ全体
> grep 結果（旧識別子は 3 つの carve-out ファイルの 6 箇所のみ）が壊れるため、
> `OLD_ID` は連結表記でのみ示す。
>
> - `OLD_ID` = `test_relocate_widened_base_via_wrap_` + `no_panic_when_column_one_does_not_exist`
> - `NEW_ID` = `test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`

## 1. 概要

### 1.1 背景

relocate-wrap-cursor-clamp のリネームによって、テストは `OLD_ID` から
`NEW_ID` に改名された。過去フィーチャー（relocate-wrap-overflow-cleanup、
relocate-wrap-ec1-scroll-test）の記録は `OLD_ID` を参照したままになっている。
`test-docs/*/taskNNNN.tests.yaml` の `acceptance_tests[].tests` は機械可読の
テスト一覧であり、古い識別子が残っていると `cargo test <old name>` が 0 件に
マッチして exit 0 で終わるため、壊れた不変条件が黙って green として通過する。

### 1.2 目的

- `test-docs/*/taskNNNN.tests.yaml` の機械可読テスト一覧がリグレッションを
  検出できる状態に戻す。
- 過去フィーチャーの記録が、その不変条件を実際に守っているテストを指すようにする。
- リネーム自体の監査証跡（relocate-wrap-cursor-clamp の 3 記録）は
  `OLD_ID` をそのまま保持する。

### 1.3 スコープ

対象は 8 ファイルのドキュメント記録のみ。変更は識別子文字列の置換に限る。
実装コード・テストコード・スキーマは変更しない。

## 2. ビジネス要件

### 2.1 ビジネス目標

- `test-docs/*/taskNNNN.tests.yaml` の `acceptance_tests[].tests` が持つ
  リグレッション検査の価値を回復する。古い識別子は `cargo test <old name>` が
  0 件マッチ・exit 0 となり、壊れた不変条件が黙って green として通過する。
- 過去フィーチャー（relocate-wrap-overflow-cleanup、relocate-wrap-ec1-scroll-test）
  の履歴記録が、relocate-wrap-cursor-clamp のリネーム後にその不変条件を実際に
  守っているテストを指し続けるようにする。
- リネーム自体の監査証跡を保つ。旧 → 新のリネームを記録する
  relocate-wrap-cursor-clamp の 3 記録は旧識別子を逐語のまま保持する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 本リポジトリの開発者・エージェント | `test-docs/*/taskNNNN.tests.yaml` の記載テストを実行してリグレッションを確認する |

### 2.3 期待される効果

- 機械可読テスト一覧からのテスト実行が 0 件マッチにならない。
- 過去フィーチャーの記録から現行テストへ辿れる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 記録されたテスト識別子でリグレッションを確認する | 開発者・エージェント | 高 |

### 3.2 ユースケース詳細

#### UC01: 記録されたテスト識別子でリグレッションを確認する

**アクター**: 開発者・エージェント

**事前条件**:
- `test-docs/*/taskNNNN.tests.yaml` に `acceptance_tests[].tests` が記載されている。

**基本フロー**:
1. `acceptance_tests[].tests` に記載された識別子を読む。
2. その識別子を cargo test のフィルタとして実行する。
3. 1 件以上のテストが実行され、結果が不変条件の状態を反映する。

**代替フロー**:
- 識別子が古い場合、フィルタが 0 件にマッチし exit 0 で終わる（本フィーチャーが解消する状態）。

**事後条件**:
- 実行結果が不変条件の成否を表す。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | test-docs の機械可読テスト一覧の更新 | 3 ファイル各 1 箇所を置換 | 高 |
| FR2 | VERIFICATION.md のリグレッション参照の更新 | 1 ファイル 1 箇所を置換 | 高 |
| FR3 | relocate-wrap-ec1-scroll-test の残り記録の更新 | 4 ファイル各 2 箇所を置換 | 高 |
| FR4 | 文字列のみの編集 | 識別子文字列以外は byte 一致を保つ | 高 |
| FR5 | リネーム記録を carve-out したリポジトリ全体の掃き出し | 3 記録の 6 箇所以外は全置換 | 高 |
| FR6 | 新識別子が実在テストに解決する | 0 件マッチのフィルタにしない | 高 |
| FR7 | 既存テストスイートの green 維持 | `crates/term_core` のテストが通る | 高 |

### 4.2 機能詳細

#### FR1: test-docs の機械可読テスト一覧の更新

**説明**: 次の各ファイルで `OLD_ID` を `NEW_ID` に置換する。

| ファイル | 置換箇所数 |
|----------|------------|
| `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` | 1 |
| `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` | 1 |
| `test-docs/relocate-wrap-ec1-scroll-test/task0002.tests.yaml` | 1 |

**ビジネスルール**:
- 置換対象はテスト識別子文字列のみ。

#### FR2: VERIFICATION.md のリグレッション参照の更新

**説明**: `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md` の
1 箇所（TS-3 のリグレッション確認対象）で `OLD_ID` を `NEW_ID` に置換する。

#### FR3: relocate-wrap-ec1-scroll-test の残り記録の更新

**説明**: FR2 と同じリグレッション参照クラスに属する、報告に列挙されていない
relocate-wrap-ec1-scroll-test の 4 ドキュメントで同じ置換を行う。

| ファイル | 置換箇所数 |
|----------|------------|
| `feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md` | 2 |
| `feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md` | 2 |
| `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0001.md` | 2 |
| `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0002.md` | 2 |

#### FR4: 文字列のみの編集

**説明**: 各編集はテスト識別子文字列のみを変更する。周囲の YAML 構造、
Markdown の散文、行順、その他すべての識別子は byte 一致のまま保つ。
テスト本体・実装・スキーマは変更しない。

#### FR5: リネーム記録を carve-out したリポジトリ全体の掃き出し

**説明**: 変更後、リポジトリ全体に残る `OLD_ID` の出現は、リネーム自体を
記録する次の 3 ファイル内の 6 箇所のみとする。これらは逐語のまま保持する。

| carve-out ファイル | 出現数 |
|--------------------|--------|
| `feature-docs/relocate-wrap-cursor-clamp/SPEC.md` | 2 |
| `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md` | 2 |
| `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml` | 2 |

この 3 ファイル以外の出現はすべて置換する。base revision 時点の合計は
11 ファイル 18 箇所 — 編集対象 8 ファイル（12 箇所）と carve-out 3 ファイル（6 箇所）。

#### FR6: 新識別子が実在テストに解決する

**説明**: `NEW_ID` は、プロジェクトの cargo test 実行を通したとき 1 件以上の
テストにマッチする。0 件マッチで exit 0 になるフィルタにはならない。

#### FR7: 既存テストスイートの green 維持

**説明**: 編集後も既存の `crates/term_core` テストスイートは変更なしで通る。

## 5. 非機能要件

### 5.1 NFR1: ドキュメント記録へのスコープ限定

`crates/`、`src-tauri/src/`、`src-tauri/tests/`、およびビルド設定配下の
ファイルは一切変更しない。変更集合は FR1〜FR3 に列挙した 8 ファイルに限る。

### 5.2 NFR2: 監査証跡の整合性

リネーム自身の記録集合は before/after の対として読める状態を保つ。特に
`feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml` は、この置換を
指示している指摘そのものであり、その「before」側を書き換えると記録が
自己矛盾する。

### 5.3 NFR3: 対象外

`test-docs/*/taskNNNN.tests.yaml` のスキーマ検証の機械化、およびテストロジックの
変更は明示的に対象外とする。

### 5.4 パフォーマンス要件 / セキュリティ要件 / 可用性要件 / 互換性要件

該当なし。

## 6. UI/UX要件

該当なし。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 編集はテスト識別子文字列のみ（FR4）。
- carve-out の 3 ファイルは逐語のまま（FR5、NFR2）。
- PR #45（relocate-wrap-cursor-clamp）が base revision 時点でマージ済みのため、
  `NEW_ID` は `crates/term_core` のテストソースに既に存在し、AC-3 を満たすための
  テストソース変更は不要。

### 9.2 ビジネス上の制約

- リネームの監査証跡を保つこと（NFR2）。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

**このフィーチャー固有のパス**:
- `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml`
- `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml`
- `test-docs/relocate-wrap-ec1-scroll-test/task0002.tests.yaml`
- `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0001.md`
- `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0002.md`

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/stale-test-name-refs/**`
- `test-docs/stale-test-name-refs/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、
`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、
`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成する
デザイン成果物。生成主体は各フェーズドキュメントおよび
`references/phase-state.md` を参照。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式:
`test-docs/{feature}/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。
- この宣言はスーパーセットの主張であり、実際の変更集合は宣言に含まれる必要がある。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 一括置換が carve-out の 3 ファイルまで書き換える | 高 | FR5 の carve-out を守り、AC-2 の grep で 6 箇所・3 ファイルであることを確認する |
| 識別子以外の差分が混入する | 中 | AC-5 の `git diff --stat` で 8 ファイル・識別子文字列のみであることを確認する |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 旧識別子が残り、0 件マッチのフィルタが green を偽装し続ける | 中 | 高 | AC-2 のリポジトリ全体 grep |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] **AC-1** (FR1, FR2): 元の報告で挙げられた 4 箇所
      （`test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml`、
      `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml`、
      `test-docs/relocate-wrap-ec1-scroll-test/task0002.tests.yaml`、
      `feature-docs/relocate-wrap-ec1-scroll-test/VERIFICATION.md`）が
      `NEW_ID` を持つ。
- [ ] **AC-2** (FR3, FR5): `OLD_ID` のリポジトリ全体 grep がちょうど 6 件を返し、
      そのすべてが `feature-docs/relocate-wrap-cursor-clamp/SPEC.md`（2）、
      `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md`（2）、
      `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml`（2）に属する。
      この 3 ファイル以外は 0 件。
- [ ] **AC-3** (FR6): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`
      が 1 件以上のテスト実行を報告する（`0 passed; 0 failed; N filtered out` ではない）。
- [ ] **AC-4** (FR7): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      が新規失敗なく通る。
- [ ] **AC-5** (FR4, NFR1): 変更完了時点の `git diff --stat` が FR1〜FR3 の 8 ファイル
      のみを列挙し、差分の内容が識別子文字列の変更のみである。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] **TS-1**（manual-command / AC-2）: 旧名の掃き出し。リポジトリルートから
      `OLD_ID` を grep し、結果が relocate-wrap-cursor-clamp の 3 ファイル内
      6 箇所の carve-out のみであることを確認する。
- [ ] **TS-2**（manual-command / AC-1, AC-2）: 新名の存在確認。`NEW_ID` を grep し、
      編集対象 8 ファイルそれぞれが期待出現数（1,1,1,1,2,2,2,2）で持つことを確認する。
- [ ] **TS-3**（cargo-test / AC-3）: `crates/term_core --lib` に対する `NEW_ID` の
      フィルタ実行が 1 件以上のテスト実行を報告する。
- [ ] **TS-4**（cargo-test / AC-4）: `crates/term_core --lib` の全スイートが通る。
- [ ] **TS-5**（manual-command / AC-5）: `git diff --stat` が期待どおり 8 パスを示す。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `OLD_ID` | 旧テスト識別子。`test_relocate_widened_base_via_wrap_` + `no_panic_when_column_one_does_not_exist` |
| `NEW_ID` | 新テスト識別子。`test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist` |
| carve-out | リネーム自体を記録するため `OLD_ID` を逐語で保持する 3 ファイル |

## 14. 確認事項

### 14.1 確認済み事項

- [x] **A-1**（質問 `requirement.repo-wide-grep-scope` の回答、オプション
      `sweep_except_history`）: リポジトリ全体の掃き出しは、リネーム自体を記録する
      relocate-wrap-cursor-clamp の 3 記録（SPEC.md、REQUIREMENTS.md、
      reviews/round1.yaml — 計 6 箇所）を除外し、これらは逐語のまま保持する。
      これはタスク記述の無条件の受け入れ基準
      「リポジトリ全体を grep して旧名の参照が 1 件も残っていない」を解決する。
      文字どおりには充足不能であり、round1.yaml は旧 → 新の置換を指示している
      指摘そのものであるため、記録が整合を保つには旧名がそこに残る必要がある。
      （batch 解決。Codex は `four_files_only` を提案したが、`four_files_only` では
      AC-2 が恒久的に充足不能になるため orchestrator が `sweep_except_history` を採用した。）
- [x] **A-2**（orchestrator 提供の ground truth）: 出現集合は base revision
      `688840b0a68f4d73cae34350089e23c437d86713` 時点で orchestrator が検証した
      ground truth — 11 ファイル 18 箇所（編集 8 + carve-out 3）。タスク記述の
      4 箇所の列挙はその部分集合であり、提供スコープは検証済みの集合とする。
- [x] **A-3**（タスク記述の「制約・前提」節、base revision と整合）: PR #45
      （relocate-wrap-cursor-clamp）は base revision 時点でマージ済みであり、
      `NEW_ID` は既に `crates/term_core` のテストソースに存在する。AC-3 を通すための
      テストソース変更は不要。

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- `feature-docs/relocate-wrap-cursor-clamp/reviews/round1.yaml`: 本置換を指示した指摘
- `feature-docs/relocate-wrap-cursor-clamp/SPEC.md`、`feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md`: リネームの記録
