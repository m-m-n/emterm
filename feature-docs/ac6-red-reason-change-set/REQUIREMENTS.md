---
title: "ac6-red-reason-change-set"
created_date: 2026-09-06
status: draft
---

# ac6-red-reason-change-set - 要件定義書

## 1. 概要

### 1.1 背景

`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-6 エントリの
`red_reason` は、`git status --porcelain` が変更集合全体としてこの YAML 1 ファイル
だけを列挙する、という節を含んでいる。この記述は実際の変更集合と一致しない。
記録自身の AC-1 は、当該タスクが `test-docs/stale-test-name-refs/task0001.tests.yaml`
という記録自身とは別のファイルを編集したことを示しており、変更集合は少なくとも 2 ファイルである。

### 1.2 目的

- AC-6 の変更集合に関する記述を、実際の変更集合と一致させる。
- AC-6 の主張を、AC-6 が本来要求する範囲（Rust ファイルと TypeScript ファイルが
  変更集合に含まれないこと）に限定する。

### 1.3 スコープ

対象は `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-6 エントリの
`red_reason` テキストのみ。同記録の他の受け入れエントリ、トップレベルキー、および
末尾の `notes` ブロックは変更しない。

## 2. ビジネス要件

### 2.1 ビジネス目標

| ID | 目標 |
|----|------|
| BO-1 | `taskNNNN.tests.yaml` の記録は、後続タスクとレビューが読む機械可読な証跡である。`test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-6 の変更集合に関する主張を実際の変更集合と一致させ、この記録から「ac7-red-confirmed-unobserved フィーチャーは 1 ファイルしか触っていない」と読み取る下流の読み手が生じないようにする。 |
| BO-2 | 各 `red_reason` の主張を、それが証拠づける受け入れ基準より強くしない。AC-6 が要求するのは「変更集合に Rust ファイルと TypeScript ファイルが含まれないこと」のみであり、現在の記録はそれより厳密に強い「この YAML 1 ファイルのみ」を主張している。これは真偽にかかわらず過剰主張である。 |

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 下流タスク・レビューの読み手 | `taskNNNN.tests.yaml` を機械可読な証跡として読み、そこに記録された受け入れ根拠を判断材料にする |

### 2.3 期待される効果

- AC-6 の記録から、変更集合の規模について誤った結論が導かれなくなる。
- 記録された主張が、対応する受け入れ基準の要求範囲と一致する。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | AC-6 の変更集合根拠を読む | 下流タスク・レビューの読み手 | 高 |

### 3.2 ユースケース詳細

#### UC01: AC-6 の変更集合根拠を読む

**アクター**: 下流タスク・レビューの読み手

**事前条件**:
- `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` が存在し、YAML として読める。

**基本フロー**:
1. 読み手が当該記録をロードする。
2. `acceptance_tests` の AC-6 エントリの `red_reason` を読む。
3. 変更集合が YAML ドキュメント記録のみで構成され、Rust ファイルと TypeScript ファイルを
   含まないことを読み取る。

**代替フロー**:
- 読み手が `feature-docs/` 配下のワークフロー生成物の存在を確認した場合、それは変更集合に
  含まれることが想定された carve-out であり、「Rust なし・TypeScript なし」の主張に影響しない。

**事後条件**:
- 読み手が、変更集合のファイル数について記録から誤った結論を導かない。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 状態 | 優先度 |
|----|--------|------|--------|
| FR1 | AC-6 の変更集合記述の訂正 | resolved | 高 |
| FR2 | ワークフロー生成物の carve-out の明記 | resolved | 高 |
| FR3 | 主張を AC-6 の要求範囲に限定 | resolved | 高 |
| FR4 | AC-6 の周辺根拠の保全 | resolved | 高 |
| FR5 | 他の受け入れエントリを変更しない | resolved | 高 |

### 4.2 機能詳細

#### FR1: AC-6 の変更集合記述の訂正

**説明**: `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-6 エントリにおいて、
`git status --porcelain` が変更集合全体としてこの YAML 1 ファイルだけを列挙すると主張する節を、
「変更集合は YAML ドキュメント記録のみ — 当該タスクが編集した記録と、タスク別テスト記録それ自体 —
で構成され、Rust ファイルと TypeScript ファイルを含まない」という記述に置き換える。

**入力**: `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の AC-6 エントリの
現行 `red_reason` テキスト。

**出力**: 書き換え後の AC-6 `red_reason` テキスト。

**ビジネスルール**:
- 置き換え後のテキストは、当該タスクが編集した記録と、タスク別テスト記録それ自体の両方を名指しする。

#### FR2: ワークフロー生成物の carve-out の明記

**説明**: 書き換え後の `red_reason` は、`feature-docs/ac7-red-confirmed-unobserved/**` 配下の
ワークフロー生成ドキュメント（および `test-docs/` の記録ツリー）を、変更集合に含まれることが
想定された carve-out として名指しし、それが「Rust なし・TypeScript なし」の主張に影響しないと述べる。

#### FR3: 主張を AC-6 の要求範囲に限定

**説明**: 書き換え後のテキストは、変更集合に Rust ソースファイルと TypeScript ソースファイルが
存在しないことのみを主張する。ファイル数は主張せず、特定の 1 ファイルが
`git status --porcelain` の唯一のエントリであるとも主張しない。

#### FR4: AC-6 の周辺根拠の保全

**説明**: AC-6 エントリの残りの部分を保全する。すなわち `red_confirmed: false`、
"Invariant guard, not a red->green criterion" の枠づけ、クリーンな事前状態の観測、
`git diff` の 2 ハンク観測、および未変更キーの列挙（ヘッダーコメント、`task_id`、
`baseline_failures`、`final_failures`、記録の AC-1 から AC-6）を維持する。

#### FR5: 他の受け入れエントリを変更しない

**説明**: AC-1 から AC-5 および AC-7、`task_id`、`baseline_failures`、`final_failures`、
ならびにトップレベルのキー順序をバイト単位で同一のまま残す。

## 5. 非機能要件

### 5.1 NFR1: パース可能性

ファイルは引き続き YAML としてパースでき、同じキー集合と形状（`task_id`、
`baseline_failures`、`final_failures`、ちょうど 7 エントリを持つ `acceptance_tests` マッピング、
`notes`）を保つ。

### 5.2 NFR2: 書式の忠実性

書き換え後の `red_reason` は `>-` の折りたたみブロックスカラー指示子を保ち、ファイル既存の
2 スペースインデントと行折り返しスタイルを保つ。編集が再フォーマットではなく同一スタイルの
改訂として読めるようにする。

### 5.3 NFR3: ドキュメントのみの変更集合

`src-tauri/`、`crates/`、`scripts/` 配下のファイル、および `.rs` / `.ts` / `.css` の
いずれのパスも変更しない。したがってビルド成果物・バンドル出力は変化しない。

### 5.4 NFR4: 事後検証可能性

書き換え後のテキストは事後に検証可能であり続ける。テキストが行う各主張は、記録を読むことと
ac7 タスク自身のコミットを調べることで再確認でき、編集前の作業ツリー状態に依存しない。

### 5.5 その他の非機能カテゴリ

パフォーマンス・セキュリティ・可用性・互換性に関する要件は本フィーチャーでは確定していない。

## 6. UI/UX要件

該当なし。デザインステップは skipped。理由: ユーザーに見える面が関与しない。変更集合は
`test-docs/` 配下の機械可読な YAML 記録 1 件の中の散文の改訂であり、UI もレンダリング出力も
デザイントークン利用側も、影響を受けるインタラクションも存在しない。

## 7. データ要件

### 7.1 データモデル概要

対象は `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の 1 レコード。

### 7.2 データ項目

| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| 記録ルート | `task_id` | string | ○ | 変更しない |
| 記録ルート | `baseline_failures` | list | ○ | 変更しない |
| 記録ルート | `final_failures` | list | ○ | 変更しない |
| 記録ルート | `acceptance_tests` | mapping | ○ | ちょうど 7 エントリ（AC-1 〜 AC-7）を保つ |
| 受け入れエントリ | `red_confirmed` | boolean | ○ | AC-6 は `false` のまま |
| 受け入れエントリ | `red_reason` | string（`>-` 折りたたみブロックスカラー） | ○ | AC-6 のみ書き換える |
| 記録ルート | `notes` | string | ○ | 変更しない |

### 7.3 データ保持期間

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 変更集合は YAML ドキュメントの編集であり、コンパイル対象・バンドル対象のコードを一切
  実行しない（A-6）。検証は YAML パースとテキストアサーションで行う。
- プロジェクトコマンド（`bun test`、`bun run typecheck`、
  `cargo test --manifest-path src-tauri/Cargo.toml --lib`）は本フィーチャーの受け入れに含まれない（A-6）。

### 9.2 ビジネス上の制約

- スコープは AC-6 の `red_reason` のみ（A-1）。記録末尾の `notes` ブロックも
  「git diff/status inspection of the single changed file」という同じ狭さを持つが、
  確定した完了定義は受け入れエントリのみを対象とするため `notes` は変更しない。
- AC-6 の `red_confirmed: false` は false のまま（A-2）。本件は理由のテキストに関する指摘であり、
  当該基準が red->green の基準であったかどうかの話ではない。AC-6 は invariant guard のままとする。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの
`files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/ac6-red-reason-change-set/**`
- `test-docs/ac6-red-reason-change-set/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、
`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、
`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式:
`test-docs/{feature}/{T}.tests.yaml`）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。
- この宣言はスーパーセットの主張であり、実際の変更集合は宣言に含まれる必要がある。

## 10. 想定される課題とリスク

### 10.1 前提に依存する事項

| 事項 | 内容 |
|------|------|
| A-3 | 実装コミット fc7af5d6 が 2 ファイル以上を含むというタスク記述の報告を所与とする。この主張は記録自身の内容によって裏づけられる。記録の AC-1 は、当該タスクが記録自身とは別のファイルである `test-docs/stale-test-name-refs/task0001.tests.yaml` を編集したことを示しており、変更集合は少なくとも 2 ファイルである。 |
| A-5 | タスク記述が引用する規範的出典 — ac7 フィーチャーの `task0001.md` の AC-6 定義、およびその SPEC の "Declared Change Set" と NFR4 — は入力として利用できなかったため、AC-6 の元の要求文言はタスク記述のみから再構成している。 |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: AC-6 の `red_reason` から、変更集合をその YAML 1 ファイルのみと述べる語句が消えており、
      変更集合のファイル数に関する主張を一切含まない。
- [ ] AC-2: AC-6 の `red_reason` が、変更集合は YAML ドキュメントのみであること
      （ac7 タスクが編集した記録と、そのタスク自身のタスク別テスト記録の両方を名指しする）と、
      Rust ファイルと TypeScript ファイルを含まないことを述べている。
- [ ] AC-3: AC-6 の `red_reason` が、`feature-docs/` のワークフロー生成成果物を
      変更集合の想定された carve-out として言及している。
- [ ] AC-4: `python3 -c "import yaml; yaml.safe_load(open(PATH))"` が成功し、ロードされたマッピングが
      依然として AC-1 から AC-7 までちょうど 7 つの `acceptance_tests` エントリを持ち、
      `AC-6.red_confirmed` が引き続き `false` である。
- [ ] AC-5: 当該ファイルの `git diff` が AC-6 エントリの `red_reason` に限定されたハンクのみを示し、
      他のすべての受け入れエントリとトップレベルキーが未変更である。
- [ ] AC-6: 本フィーチャーの変更集合に Rust ファイルと TypeScript ファイルが含まれない
      （変更集合は編集した YAML 記録に加え、本フィーチャー自身の
      `feature-docs/ac6-red-reason-change-set/**` と `test-docs/ac6-red-reason-change-set/**`
      のワークフロー成果物である）。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

| ID | シナリオ | 対象要件 |
|----|----------|----------|
| TS-1 | `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml` の PyYAML ロードが成功する。`acceptance_tests` のキー集合が {AC-1..AC-7} と等しく、`AC-6.red_confirmed` が False であることをアサートする。NFR1 / AC-4 をカバー。 | NFR1 |
| TS-2 | ロードした AC-6 `red_reason` に対する固定文字列チェック: 変更集合をその YAML 1 ファイルのみと述べる語句の出現回数が 0。編集前は 1。FR1 / FR3 / AC-1 を red->green 基準としてカバー。 | FR1, FR3 |
| TS-3 | ロードした AC-6 `red_reason` に対し、「Rust なし」「TypeScript なし」の主張と両方の記録パスの部分文字列チェック。FR1 / AC-2 をカバー。 | FR1 |
| TS-4 | ロードした AC-6 `red_reason` に対する `feature-docs/` の部分文字列チェック。編集前は 0 件。FR2 / AC-3 を red->green 基準としてカバー。 | FR2 |
| TS-5 | ファイルの編集前版と編集後版をロードし、AC-6 以外のすべての受け入れエントリと `task_id` / `baseline_failures` / `final_failures` の等価性を比較する。FR5 / AC-5 をカバー。 | FR5 |
| TS-6 | 生テキストのチェック: AC-6 の `red_reason` が引き続き `>-` 折りたたみブロックスカラー指示子を使っており、生ファイルのトップレベルキー順序が未変更であること。NFR2 をカバー。 | NFR2 |
| TS-7 | 本フィーチャーの変更集合に対する `git status --porcelain` / `git diff --stat` が、`.rs` または `.ts` で終わるパスを 1 つも列挙しない。NFR3 / 本フィーチャーの AC-6 をカバー（観測可能な事前状態を持たない invariant guard）。 | NFR3 |
| TS-8 | 書き換え後の AC-6 `red_reason` を読み、保全された根拠が無傷であること（`red_confirmed: false`、invariant guard の枠づけ、クリーンな事前状態の観測、2 ハンクの git diff 観測、未変更キーの列挙）と、テキストが行う各主張が編集前の作業ツリー状態なしに記録と ac7 タスク自身のコミットから再確認できることを確認する。FR4 / NFR4 をカバー。 | FR4, NFR4 |

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `red_reason` | `taskNNNN.tests.yaml` の各受け入れエントリに記録される、red 確認に関する根拠テキスト |
| invariant guard | 観測可能な事前状態を持たず、red->green の基準ではない受け入れ基準。`red_confirmed: false` で記録される |
| 変更集合（change set） | 当該タスクのコミットが含むファイルの集合 |
| carve-out | 変更集合に含まれることが想定されており、「Rust なし・TypeScript なし」の主張に影響しない要素 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A-1: スコープは AC-6 の `red_reason` のみ。記録末尾の `notes` ブロックも
      「git diff/status inspection of the single changed file」という同じ狭さを持つが、
      確定した完了定義は受け入れエントリのみを対象とするため `notes` は変更しない。
- [x] A-2: AC-6 の `red_confirmed: false` は false のまま。本件は理由のテキストに関する指摘であり、
      当該基準が red->green の基準であったかどうかの話ではない。AC-6 は invariant guard のまま。
- [x] A-3: 実装コミット fc7af5d6 が 2 ファイル以上を含むというタスク記述の報告を所与とする。
      記録の AC-1 が、当該タスクは記録自身とは別のファイル
      `test-docs/stale-test-name-refs/task0001.tests.yaml` を編集したことを示しており、
      変更集合は少なくとも 2 ファイルである。
- [x] A-4: carve-out の文言は `feature-docs/**` と `test-docs/**` の双方のワークフロー生成成果物を
      対象とする。いずれもプロダクトのソースではなくワークフローが生成するものであるため。
- [x] A-6: プロジェクトコマンド（`bun test`、`bun run typecheck`、
      `cargo test --manifest-path src-tauri/Cargo.toml --lib`）は本フィーチャーの受け入れに含まれない。
      変更集合はコンパイル対象・バンドル対象のコードを一切実行しない YAML ドキュメント編集である。
      検証は YAML パースとテキストアサーションで行う。

### 14.2 未確認・保留事項

- [ ] A-5: タスク記述が引用する規範的出典 — ac7 フィーチャーの `task0001.md` の AC-6 定義、および
      その SPEC の "Declared Change Set" と NFR4 — は入力として利用できなかった。AC-6 の元の要求文言は
      タスク記述のみから再構成している。

## 15. 参考資料

- 訂正対象の記録: `test-docs/ac7-red-confirmed-unobserved/task0001.tests.yaml`
- 実装仕様: `feature-docs/ac6-red-reason-change-set/SPEC.md`
