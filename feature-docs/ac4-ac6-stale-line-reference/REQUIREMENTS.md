---
title: "ac4-ac6-stale-line-reference"
created_date: 2026-09-06
status: draft
---

# ac4-ac6-stale-line-reference - 要件定義書

## 1. 概要

### 1.1 背景

`test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` の AC-4 と AC-6 の散文が、`crates/term_core/src/ring_buffer/tests.rs` の行 523 を現在形で参照している。フィーチャー `ring-push-blank-note-unconditional` は AC-5 を修正したが、この参照のずれは残したままだった。

### 1.2 目的

- ring-push-blank-row-scope のテスト記録の正確さを回復し、AC-4 / AC-6 のミューテーションを再実行する人が、無関係なフィクスチャ行やコメント行ではなく survivor-row アサーションへ導かれるようにする。
- `ring-push-blank-note-unconditional` が始めた整合作業（AC-5 は修正済み、この参照ずれは未修正）を完了させる。
- 次に `crates/term_core/src/ring_buffer/tests.rs` がずれたときに再び陳腐化しない表現を選ぶ。

### 1.3 スコープ

対象は `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` の AC-4 と AC-6 の散文のみ。Rust ソース、テストコード、その他のファイルは変更しない。`crates/term_core/src/ring_buffer/tests.rs` は読み取り専用の入力である。

## 2. ビジネス要件

### 2.1 ビジネス目標

- ring-push-blank-row-scope のテスト記録の正確さを回復し、AC-4 / AC-6 のミューテーションを再実行する人が survivor-row アサーションへ導かれるようにする。
- `ring-push-blank-note-unconditional` が始めた整合作業を完了させる。
- `crates/term_core/src/ring_buffer/tests.rs` の次のずれで再び陳腐化しない表現を選ぶ。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| AC-4 / AC-6 のミューテーションを再実行する開発者 | テスト記録の記述を頼りに、対象となる survivor-row アサーションを特定する |

### 2.3 期待される効果

- AC-4 / AC-6 の再実行時に、無関係なフィクスチャ行やコメント行へ誘導されなくなる。
- `crates/term_core/src/ring_buffer/tests.rs` の行がずれても記述の再編集が不要になる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター |
|----|----------------|----------|
| UC01 | AC-4 / AC-6 のミューテーションを再実行する | AC-4 / AC-6 のミューテーションを再実行する開発者 |

### 3.2 ユースケース詳細

#### UC01: AC-4 / AC-6 のミューテーションを再実行する

**アクター**: AC-4 / AC-6 のミューテーションを再実行する開発者

**事前条件**:
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` が読める状態にある

**基本フロー**:
1. AC-4 / AC-6 の記述を読む
2. 記述が指し示すアサーションを `crates/term_core/src/ring_buffer/tests.rs` 上で特定する
3. そのアサーションに対してミューテーションを再実行する

**代替フロー**:
- `crates/term_core/src/ring_buffer/tests.rs` の行番号がずれている場合も、アサーションの式（post-scroll survival として限定されたもの）から特定できる

**事後条件**:
- 特定されたアサーションが survivor-row アサーションであり、recycled-row アサーションではない

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 |
|----|--------|------|
| FR1 | AC-6 の締めの散文を修正する | 行 523 を現在形で survivor-row アサーションと断定する記述をやめる |
| FR2 | AC-4 の散文を修正する | `red_reason` 内の位置参照を FR1 と同じ方法で修正する |
| FR3 | 逐語トランスクリプトに手を入れない | AC-6 の `cargo test` トランスクリプトブロックをバイト単位で不変に保つ |
| FR4 | 式で特定する場合の曖昧さを解消する | 同一の式が 2 箇所に現れるため post-scroll survival として限定する |
| FR5 | 新たに書く行番号は編集時点のファイルで検証する | 行番号はタスク記述からではなく実ファイルから再導出する |
| FR6 | YAML の妥当性と周辺構造を保つ | キー構造・他の受け入れ基準・各種フィールドを変更しない |
| FR7 | スコープはドキュメントのみ | Rust ソース・テストコード・他ファイルを変更しない |

### 4.2 機能詳細

#### FR1: AC-6 の締めの散文を修正する

**説明**: `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` において、AC-6 の締めの一文（現状は 77-78 行目: `The failing line (523) is a survivor-row assertion, matching the requirement that the failure not land on a recycled-row assertion.`）は、`crates/term_core/src/ring_buffer/tests.rs` の行 523 が survivor-row アサーションであると現在形で断定することをやめなければならない。523 をその実行時点の行番号として明示するか、行番号を使わずにアサーションを特定するかのいずれかにする。

**入力**:
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`: YAML - AC-6 の散文（77-78 行目）

**出力**:
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`: YAML - 修正後の AC-6 の散文

**ビジネスルール**:
- 523 は当時の真の失敗位置であるため、削除せず「当時の行番号」として残す（NFR2）

#### FR2: AC-4 の散文を修正する

**説明**: AC-4 の `red_reason` の散文（現状は 35-44 行目、位置参照は 42 行目: `at crates/term_core/src/ring_buffer/tests.rs:523:5.`）を FR1 と同じ方法で修正する。タスク記述はこの箇所を「保護対象のトランスクリプト」ではなく「修正対象の散文」として明示的に分類している。

**入力**:
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`: YAML - AC-4 の `red_reason` 散文（35-44 行目）

**出力**:
- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`: YAML - 修正後の AC-4 の `red_reason` 散文

#### FR3: 逐語トランスクリプトに手を入れない

**説明**: AC-6 の `cargo test` トランスクリプトブロック（69-75 行目。72 行目の `thread '...' (2553229) panicked at crates/term_core/src/ring_buffer/tests.rs:523:5:` を含む）は、変更の前後でバイト単位に同一でなければならない。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| トランスクリプト改変 | 69-75 行目に差分が生じた | 変更を差し戻し、編集範囲を AC-4 / AC-6 の散文に限定する |

#### FR4: 式で特定する場合の曖昧さを解消する

**説明**: 置き換え後の散文がアサーションを式（`core.overflow.contains_key(&(0u32, abs1))`）で特定する場合、それを post-scroll survival のアサーションとして限定しなければならない。この式はファイル内に 2 回現れる（558 行目 = pre-scroll の anti-vacuity ガード、606 行目 = post-scroll の survival）。

#### FR5: 新たに書く行番号は編集時点のファイルで検証する

**説明**: 修正が現在の行番号を引用する場合、その番号はタスク記述から取らず、実装時点の `crates/term_core/src/ring_buffer/tests.rs` から再導出しなければならない。base_revision 8c6e2e1d では post-scroll の survivor アサーションは 606 行目にある。

**バリデーション**:

| 項目 | ルール | エラーメッセージ |
|------|--------|------------------|
| 引用する現在の行番号 | 編集時点の `crates/term_core/src/ring_buffer/tests.rs` と一致すること | 引用行番号が実ファイルと一致しない |

#### FR6: YAML の妥当性と周辺構造を保つ

**説明**: `task0001.tests.yaml` は妥当な YAML のままであり、キー構造、他の受け入れ基準エントリ（AC-1、AC-2、AC-3、AC-5、AC-7）、`task_id`、`baseline_failures`、`final_failures`、およびすべての `tests:` / `red_confirmed:` フィールドが変更されないこと。編集は AC-4 と AC-6 の折り畳みスカラー散文に限定し、既存の 6 スペース継続インデントを維持する。

#### FR7: スコープはドキュメントのみ

**説明**: Rust ソース、テストコード、その他のファイルは一切変更しない。`crates/term_core/src/ring_buffer/tests.rs` はこのフィーチャーにとって読み取り専用の入力である。

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし（コンパイル対象のコードを変更しないため）。

### 5.2 セキュリティ要件

該当なし（ドキュメント内の散文修正のみ）。

### 5.3 可用性要件

該当なし（実行時の振る舞いを変更しないため）。

### 5.4 保守性要件

- **NFR1 - 将来の行ずれへの耐性**: 選んだ表現は、次に `tests.rs` がずれたときに再編集を要してはならない。アサーションの式に加えて、明示的に「当時のもの」とした行番号で固定すればこれを満たす。裸の現在行番号だけでは満たさない。
- **NFR2 - 記録の史実性**: この記録は過去の実行の証跡である。修正は起きたことを書き換えてはならない。523 は当時の真の失敗位置であるため、削除せず「当時のもの」と再ラベルして本文中に残す。
- **NFR3 - 差分の最小性**: 変更は 2 箇所の散文のみに触れる。折り畳みスカラーの無関係な行を再整形すること、およびファイルの再フォーマットはスコープ外である。

### 5.5 互換性要件

該当なし。

## 6. UI/UX要件

### 6.1 画面設計要件

該当なし。YAML テスト記録内の散文へのドキュメント限定の修正であり、UI 面も、ユーザーから見える振る舞いも、新規モジュールも、設計対象のインターフェースも存在しない。

### 6.2 画面遷移

該当なし。

### 6.3 レスポンシブ対応

該当なし。

## 7. データ要件

### 7.1 データモデル概要

該当なし（データモデルの変更はない）。

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| `task0001.tests.yaml` | AC-4 `red_reason` | 折り畳みスカラー（散文） | ○ | 修正対象（FR2） |
| `task0001.tests.yaml` | AC-6 締めの一文 | 折り畳みスカラー（散文） | ○ | 修正対象（FR1） |
| `task0001.tests.yaml` | AC-6 `cargo test` トランスクリプト（69-75 行目） | 逐語ブロック | ○ | 変更禁止（FR3） |

### 7.3 データ保持期間

該当なし。

## 8. 外部連携

### 8.1 連携システム

該当なし。

### 8.2 API仕様要件

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- `task0001.tests.yaml` は妥当な YAML のままでなければならない（FR6）。
- 編集は AC-4 と AC-6 の折り畳みスカラー散文に限定し、既存の 6 スペース継続インデントを維持する（FR6）。
- AC-6 の逐語トランスクリプト（69-75 行目）はバイト単位で不変（FR3）。

### 9.2 ビジネス上の制約

- 記録は過去の実行の証跡であり、起きたことを書き換えない（NFR2）。

### 9.3 スケジュール制約

指定なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。本フィーチャーで変更するフィーチャー固有のパスは `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` の 1 件のみである。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/ac4-ac6-stale-line-reference/**`
- `test-docs/ac4-ac6-stale-line-reference/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/{feature}/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| `core.overflow.contains_key(&(0u32, abs1))` が 558 行目と 606 行目の 2 箇所に現れ、式だけでは特定できない | 中 | post-scroll survival のアサーションとして限定する（FR4） |
| 新たに書く行番号がタスク記述由来だと再び陳腐化する | 中 | 実装時点のファイルから再導出する（FR5） |
| 折り畳みスカラーの編集がトランスクリプトや周辺構造に波及する | 中 | 編集範囲を AC-4 / AC-6 の散文に限定し、69-75 行目を不変に保つ（FR3、FR6、NFR3） |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 修正が史実を書き換え、過去の実行の証跡としての価値を損なう | 中 | 高 | 523 は削除せず「当時の行番号」と再ラベルする（NFR2） |
| 裸の現在行番号を書き、次の行ずれで再び陳腐化する | 中 | 中 | 式による特定と、明示的に歴史的な行番号を併用する（NFR1） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-6 の締めの一文が、行 523 は survivor-row アサーションであると現在形で主張しなくなっている。523 をその実行時点の行番号として明示するか、行番号を使わずにアサーションを名指ししている。
- [ ] AC-4 の `crates/term_core/src/ring_buffer/tests.rs:523:5` 参照が同じ方法で修正されている。
- [ ] ファイルの 69-75 行目（`panicked at ...:523:5` の行を含む AC-6 の `cargo test` トランスクリプト）が変更前の内容とバイト単位で同一である。
- [ ] ファイルが妥当な YAML としてパースでき、`task_id`、`baseline_failures`、`final_failures`、AC-1、AC-2、AC-3、AC-5、AC-7、およびすべての `tests:` / `red_confirmed:` フィールドが変更されていない。
- [ ] アサーションを式で特定している箇所では、post-scroll survival のアサーションが、同一の pre-scroll anti-vacuity アサーションと区別されている。
- [ ] 現在の行番号を引用している箇所では、その番号が編集時点のファイルと一致している（base_revision 8c6e2e1d では 606）。

### 11.2 KPI

指定なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系（TS-1）: `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` を YAML パーサーで読み込み、ロードできること、およびトップレベルキーと AC-1..AC-7 のキー集合が変更されていないことを確認する。
- [ ] 正常系（TS-2）: 変更後のファイルを変更前のバージョンと diff し、ハンクが AC-4 と AC-6 の `red_reason` 散文の内側だけに収まり、69-75 行目のトランスクリプト領域が手つかずであることを確認する。
- [ ] 正常系（TS-3）: 変更後のファイルを `The failing line (523) is a survivor-row assertion` で grep し、裸の現在形が消えていることを確認する。
- [ ] 境界値（TS-4）: 変更後のファイルを 523 で grep し、残存するすべての出現箇所が、保護対象のトランスクリプト内か、明示的に歴史的なものとして限定されているかのいずれかであることを確認する。
- [ ] 境界値（TS-5）: `crates/term_core/src/ring_buffer/tests.rs` を開き、post-scroll の survivor アサーションが新しい散文の言うとおりの位置にあること（base_revision 8c6e2e1d で 606 行目）、および 558 行目に同一の pre-scroll アサーションがあることを確認する。
- [ ] 異常系（TS-6）: Rust のビルドもテスト実行も不要であること。このフィーチャーはコンパイル対象のコードを変更しない。
- [ ] セキュリティ: 該当なし。
- [ ] パフォーマンス: 該当なし。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| survivor-row アサーション | スクロール後も生き残る行に対するアサーション。AC-4 / AC-6 の失敗が着地すべき対象 |
| recycled-row アサーション | 再利用される行に対するアサーション。AC-4 / AC-6 の失敗が着地してはならない対象 |
| post-scroll survival | `core.overflow.contains_key(&(0u32, abs1))` のうち、606 行目（base_revision 8c6e2e1d）に現れるスクロール後の生存確認 |
| pre-scroll anti-vacuity ガード | `core.overflow.contains_key(&(0u32, abs1))` のうち、558 行目に現れるスクロール前の空虚性回避ガード |
| 保護対象のトランスクリプト | AC-6 の `cargo test` 出力を囲ったブロック（69-75 行目）。逐語であり変更しない |

## 14. 確認事項

### 14.1 確認済み事項

- [x] タスク記述にある 2 つの修正案の関係: 2 案は択一ではなく、耐久性のある形はハイブリッドである。523 をその実行時点の行番号として残しつつ、アサーションを post-scroll survival として限定した式で特定する。
- [x] `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml` の位置づけ: 完了済みの歴史的テスト記録であり、生きた仕様ではない。これをプログラム的に消費するものは無い。
- [x] AC-4 の `:523:5` の扱い: タスク記述の明示的な切り分けに従い、保護対象のトランスクリプトではなく散文である。逐語なのは AC-6 のフェンス付き `cargo test` 出力ブロックのみ。
- [x] このフィーチャーの `workflow.yaml`: まだ存在しない（`input_revision.workflow_blob` が null）。create-spec の初回ディスパッチと整合している。

### 14.2 未確認・保留事項

なし（すべての機能要件・非機能要件が `resolved` である）。

## 15. 参考資料

- `test-docs/ring-push-blank-row-scope-test/task0001.tests.yaml`: 修正対象のテスト記録
- `crates/term_core/src/ring_buffer/tests.rs`: 読み取り専用の参照先（558 行目 = pre-scroll anti-vacuity、606 行目 = post-scroll survival、base_revision 8c6e2e1d 時点）
- フィーチャー `ring-push-blank-note-unconditional`: AC-5 を修正した先行フィーチャー
