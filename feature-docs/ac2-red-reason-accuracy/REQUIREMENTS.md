---
title: "ac2-red-reason-accuracy"
created_date: 2026-09-05
status: draft
---

# ac2-red-reason-accuracy - 要件定義書

## 1. 概要

### 1.1 背景

`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-2 エントリの
`red_reason` は、編集前の
`test-docs/stale-test-name-refs/task0001.tests.yaml` の AC-7 `red_reason` について
「必要な 4 要素がいずれも存在しなかった」と記述している。実際に欠けていたのは 4 要素の
うち 3 要素であり、4 つ目（レコードの AC-2 リポジトリ全体カウントへの言及）は編集前の
本文にすでに存在していた。

### 1.2 目的

- AC-2 の `red_reason` を、実際に観測された状態だけを述べる記述に修正する。
- red 判定そのもの（`red_confirmed: true`）は変更しない。

### 1.3 スコープ

対象は `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-2 エントリの
`red_reason` スカラーのみ。同ファイルの他のキー、および記述対象である
`test-docs/stale-test-name-refs/task0001.tests.yaml` は変更しない。Rust / TypeScript
のソース変更およびリビルドは含まない。

## 2. ビジネス要件

### 2.1 ビジネス目標

| ID | 目標 |
|----|------|
| BO-1 | `taskNNNN.tests.yaml` のレコードを後続タスクの機械可読な証跡として使える状態に保つため、すべての `red_reason` が実際に観測された状態だけを記述するようにする。 |
| BO-2 | `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-2 の根拠を、（妥当な）red 判定を変えずに訂正し、将来の読み手がベースリビジョンの本文について誤った前提を渡されないようにする。 |

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 後続タスクの読み手 | `taskNNNN.tests.yaml` を機械可読な証跡として参照する立場 |

### 2.3 期待される効果

- `red_reason` が実際に観測された状態だけを述べる記述になる。
- red 判定は維持されたまま、根拠の記述だけが正確になる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | AC-2 の `red_reason` を訂正する | 後続タスクの読み手 | 高 |

### 3.2 ユースケース詳細

#### UC01: AC-2 の `red_reason` を訂正する

**アクター**: 後続タスクの読み手

**事前条件**:
- `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` が統合ワークツリーの HEAD に存在する。

**基本フロー**:
1. AC-2 エントリの `red_reason` を、4 要素のうち 3 要素が編集前に欠けていたと述べる本文に書き換える。
2. 4 つ目の要素（AC-2 リポジトリ全体カウントへの言及）はベース本文に既に存在し、編集で保持されたと記述する。
3. 実際に欠けていた 3 要素（"invariant guard" の語句、レコード AC-6 への言及、"no observable pre-state" の言い回し）を引き続き列挙する。
4. 編集後の状態に関する記述（4 要素すべてが存在し、"confirmed by" / "observed" という red 観測の語が無い）を実質的に保持する。

**代替フロー**:
- ベースリビジョン `9eee6161` の AC-7 `red_reason` に AC-2 カウントへの言及が確認できない場合は、書き換え本文が引用する証跡と一致しないため記述を見直す。

**事後条件**:
- AC-2 の `red_reason` のみが変更され、`red_confirmed` は `true` のまま。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 欠落要素の正しい件数を述べる | 4 要素中 3 要素が欠けていたと記述する | 高 |
| FR2 | 4 つ目の要素を既存かつ保持と記録する | AC-2 カウント言及は既存であり編集で保持されたと記述する | 高 |
| FR3 | 実際に欠けていた 3 要素を列挙する | 3 要素の名称を引き続き記載する | 高 |
| FR4 | 正確な編集後半分を保持する | 編集後の状態に関する記述を実質的に保持する | 高 |
| FR5 | red 判定を維持する | `red_confirmed` を `true` のままにする | 高 |
| FR6 | 変更を AC-2 エントリに限定する | AC-2 の `red_reason` スカラー以外は不変とする | 高 |

### 4.2 機能詳細

#### FR1: 欠落要素の正しい件数を述べる

**説明**: `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-2 エントリの
`red_reason` は、`test-docs/stale-test-name-refs/task0001.tests.yaml` の編集前 AC-7
`red_reason` において、必要な 4 要素のうち 3 要素が欠けていたと述べる。4 要素のいずれも
存在しなかったという主張はしない。

**入力**:
- `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`: YAML ファイル - 訂正対象のレコード

**出力**:
- `acceptance_tests['AC-2']['red_reason']`: 文字列 - 訂正後の根拠テキスト

**ビジネスルール**:
- `red_reason` は実際に観測された状態だけを記述する（BO-1）。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| 記述が証跡と不一致 | ベース本文が示す状態と書き換え本文が食い違う | ベース blob を再確認して記述を合わせる |

#### FR2: 4 つ目の要素を既存かつ保持と記録する

**説明**: 同じ `red_reason` は、4 つ目の要素、すなわちレコードの AC-2 リポジトリ全体カウント
説明への言及が、ベース本文に既に存在し、編集によって新たに導入されたのではなく保持された
ものであると述べる。

**ビジネスルール**:
- 編集が新規に導入した要素と、元から存在した要素を区別して記述する。

#### FR3: 実際に欠けていた 3 要素を列挙する

**説明**: 書き換え後の本文は、編集前に実際に欠けていた 3 要素、すなわち "invariant guard"
の語句、レコード AC-6 への言及、"no observable pre-state" の言い回しを引き続き名指しする。

#### FR4: 正確な編集後半分を保持する

**説明**: 根拠のうち編集後に関する部分は実質的に保持する。すなわち、編集後はスクリプトによる
チェックが 4 要素すべての存在を検出し、書き換え後の AC-7 本文に "confirmed by" /
"observed" という red 観測の語が無いことを検出する、という記述を保つ。

#### FR5: red 判定を維持する

**説明**: `acceptance_tests['AC-2']['red_confirmed']` は真偽値 `true` のままとする。3 要素の
欠落は当該基準を red にするのに十分であり、判定自体は見直さない。

#### FR6: 変更を AC-2 エントリに限定する

**説明**: 変更するのは AC-2 エントリの `red_reason` スカラーのみとする。`tests`、
`red_confirmed`、他のすべての acceptance エントリ（AC-1、AC-3 から AC-7）、`task_id`、
`baseline_failures`、`final_failures`、および末尾の `notes` ブロックは変更後もバイト単位で
同一とする。

## 5. 非機能要件

| ID | 要件 |
|----|------|
| NFR1 | ファイルは既存のキー集合、7 件の `acceptance_tests` エントリ、変更のないトップレベルキー順序を保ったまま PyYAML でパースできる。 |
| NFR2 | 編集するスカラーは `>-` の折り畳みブロックスカラー指示子と、ファイル全体で使われているインデント／行折り返しスタイルを保つ。 |
| NFR3 | レコードは英語のままとし、ファイル内の他のエントリと揃える。 |
| NFR4 | 訂正後の本文自体が、行われていない観測を主張しない。編集前の状態は、ベース blob が実際に裏付ける範囲でのみ記述する。 |
| NFR5 | Rust / TypeScript のソース変更およびリビルドを伴わない。単一ファイルの YAML ドキュメント訂正である。 |

### 5.1 パフォーマンス要件

該当なし（単一ファイルの YAML ドキュメント訂正のため）。

### 5.2 セキュリティ要件

該当なし（単一ファイルの YAML ドキュメント訂正のため）。

### 5.3 可用性要件

該当なし（単一ファイルの YAML ドキュメント訂正のため）。

### 5.4 保守性要件

- ドキュメント: NFR1（パース可能性・キー構造の維持）、NFR2（スカラー表記スタイルの維持）、NFR3（英語での記述）を満たす。

### 5.5 互換性要件

- PyYAML でのロード互換性を維持する（NFR1）。

## 6. UI/UX要件

該当なし。本フィーチャーは UI も視覚的な表面も持たない単一ファイルの YAML ドキュメント訂正であり、デザインステップはスキップされている。

## 7. データ要件

### 7.1 データモデル概要

対象は `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の 1 レコード。

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| task0001.tests.yaml | task_id | - | ○ | 変更しない |
| task0001.tests.yaml | baseline_failures | - | ○ | 変更しない |
| task0001.tests.yaml | final_failures | - | ○ | 変更しない |
| task0001.tests.yaml | acceptance_tests | マッピング（7 エントリ） | ○ | AC-2 の `red_reason` のみ変更 |
| acceptance_tests['AC-2'] | red_reason | 文字列（`>-` 折り畳みスカラー） | ○ | 訂正対象 |
| acceptance_tests['AC-2'] | red_confirmed | 真偽値 | ○ | `true` のまま |
| acceptance_tests['AC-2'] | tests | - | ○ | 変更しない |
| task0001.tests.yaml | notes | ブロック | ○ | 変更しない |

### 7.3 データ保持期間

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- ファイルは PyYAML でパース可能な状態を保つ（NFR1）。
- 編集スカラーは `>-` 指示子と既存のインデント／折り返しスタイルを保つ（NFR2）。
- 記述は英語のままとする（NFR3）。
- Rust / TypeScript のソース変更およびリビルドは行わない（NFR5）。

### 9.2 ビジネス上の制約

- red 判定（`red_confirmed: true`）は変更しない（FR5）。
- 変更は AC-2 エントリの `red_reason` スカラーに限定する（FR6）。

### 9.3 スケジュール制約

なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/{feature}/**`
- `test-docs/{feature}/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/{feature}/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 折り畳みスカラーは再折り返しされるため生の行内容での検証が不安定 | 中 | パース後の値に対してアサートする（TS-1） |
| ベース本文の実際の内容と書き換え本文の食い違い | 中 | `git show 9eee6161:...` でベース blob の AC-7 `red_reason` を確認する（TS-4） |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 訂正後の本文が別の未観測の主張を含む | 低 | 高 | NFR4 に従い、ベース blob が裏付ける範囲でのみ記述する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: ファイルをパースして `acceptance_tests['AC-2']['red_reason']` を読むと、編集前に必要な 4 要素のうち 3 要素が欠けていたと述べる本文が得られる。（検証: PyYAML ロードとパース後スカラーのテキストチェック）
- [ ] AC-2: 同じパース済みスカラーが、4 つ目の要素すなわち AC-2 リポジトリ全体カウントへの言及がベース本文に既に存在し保持されたと述べている。（検証: PyYAML ロードとパース後スカラーのテキストチェック）
- [ ] AC-3: パース済みスカラーに、4 要素のいずれも存在しなかったという主張が含まれない。（検証: パース後スカラーへの否定テキストチェック）
- [ ] AC-4: `acceptance_tests['AC-2']['red_confirmed']` が真偽値 `true` である。（検証: PyYAML ロードと `True` への同一性チェック）
- [ ] AC-5: ファイル全体が元の形のまま妥当な YAML としてロードできる。（検証: PyYAML ロードで task_id、baseline_failures、final_failures、ちょうど 7 エントリの acceptance_tests マッピング、notes が現れる）
- [ ] AC-6: AC-2 以外の acceptance エントリおよび他のトップレベルキーが変更後も差分を持たない。（検証: `git diff` が AC-2 の `red_reason` スカラーに限定され、`git status --porcelain` がこの 1 つの YAML ファイルのみを挙げる）

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1: 変更前後に PyYAML でレコードをロードし、AC-2 の `red_reason` が「4 要素のいずれも無かった」という主張から「4 要素中 3 要素が欠落、4 つ目は既存」という主張へ移ることをアサートする。折り畳みスカラーは再折り返しされるため、生の行内容ではなくパース後の値でアサートする。
- [ ] TS-2: AC-2 の `red_confirmed` が変更前後とも `True` であることをアサートする。これは不変条件のガードであり、観測可能な red の事前状態は存在しない。
- [ ] TS-3: AC-2 以外のすべての acceptance エントリと `notes` ブロックについて、ベースと結果を差分比較し、等価であることをアサートする。
- [ ] TS-4: ベース blob（`git show 9eee6161:test-docs/stale-test-name-refs/task0001.tests.yaml`）を読み直し、そこでの AC-7 `red_reason` に AC-2 カウントへの言及が含まれることを確認して、書き換え後の根拠が引用する証跡と一致することを確かめる。
- [ ] TS-5: 生ファイルが AC-2 の `red_reason` に依然として `>-` を使っており、ヘッダ／トップレベルキー順序が変わっていないことを確認する。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `red_reason` | `taskNNNN.tests.yaml` の acceptance エントリが red と判定された根拠を記述するスカラー |
| `red_confirmed` | acceptance エントリの red 判定を表す真偽値 |
| ベース blob | リビジョン `9eee6161` 時点の `test-docs/stale-test-name-refs/task0001.tests.yaml` |
| 4 要素 | 編集後の AC-7 `red_reason` に求められる 4 つの要素（"invariant guard" の語句、レコード AC-6 への言及、"no observable pre-state" の言い回し、AC-2 リポジトリ全体カウントへの言及） |

## 14. 確認事項

### 14.1 確認済み事項

なし。

### 14.2 未確認・保留事項

以下は requirements-analyst が置いた前提であり、未検証の仮定を含む。

- [ ] A-1: ベースリビジョン `9eee6161` の AC-7 `red_reason` には "which is what keeps the AC-2 repository-wide count at 6 instead of climbing to 7" が含まれる。task_description に由来し、アナリストのディスパッチでは独立に検証していない。実装者が本文を書き換える前に TS-4 の `git show` で確認する。
- [ ] A-2: 現在の括弧内で挙げられている 3 要素（"invariant guard" の語句、レコード AC-6 への言及、"no observable pre-state" の言い回し）が、実際に欠けていた 3 要素とちょうど一致する。
- [ ] A-3: 末尾の `notes` ブロックは変更不要である。タスクの完了定義は acceptance エントリのみを制約する。
- [ ] A-4: レコードは英語のままとする。ファイル全体が現状英語である。
- [ ] A-5: project_commands（bun test / bun run typecheck / cargo test）は本変更集合には不要である。Rust も TypeScript のソースも触らないため。これはレコード自身の `notes` が既に記録している根拠と同じである。
- [ ] A-6: 編集対象ファイルは `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`（統合ワークツリーの HEAD に存在）であり、それが *記述している* レコードは `test-docs/stale-test-name-refs/task0001.tests.yaml` で、本フィーチャーでは変更しない。

## 15. 参考資料

- 訂正対象レコード: `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
- 記述対象レコード（変更しない）: `test-docs/stale-test-name-refs/task0001.tests.yaml`
- ベースリビジョン: `9eee6161`
