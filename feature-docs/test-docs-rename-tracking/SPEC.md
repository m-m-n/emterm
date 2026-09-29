# Feature: test-docs-rename-tracking

## 概要

notification-summary-markup-escape と notification-body-markup-escape の過去の test-docs 記録が、現存するテストを指すように更新する。
notification-markup-fail-closed で取得失敗時の挙動が反転した受け入れ基準には、置き換え（supersede）の注記を付ける。
機能をまたぐテスト名変更の扱いを「現行名に追従する」方針として `.claude/rules/` にプロジェクトルールとして記録する。

> REQUIREMENTS.md は reduced tier では作成しない。本 SPEC.md が要件の唯一の文書となる。

## 目的

- notification-summary-markup-escape と notification-body-markup-escape の過去の test-docs 記録が現存するテストを指すようにし、それらに対して verify を再実行したときに 0 件マッチのまま成功終了しないようにする。
- notification-markup-fail-closed（fail-closed SPEC FR1/FR3）によって取得失敗時の挙動が反転した受け入れ基準に印を付け、現行コードがそれらに違反していると読まれないようにする。
- 機能をまたぐテスト名変更の方針を「凍結ではなく現行名に追従する」と定める。後続の機能が適用できるよう、`.claude/rules/` にプロジェクトルールとして記録する。

## ユーザーストーリー

該当なし（要件分析にユーザーストーリーは含まれない）。受け入れ基準は次節に記す。

## 受け入れ基準

- [ ] **AC-1**（FR1）: FR1 に挙げた 4 行（summary 記録の 14・33・34・35 行目）が、記載の対応表どおりに現行のテストパスになっている。
- [ ] **AC-2**（FR2）: body 記録の AC-4（19〜21 行目）が、記載の対応表どおりに現行の `body_markup_absence_confirmed_*` パスになっている。
- [ ] **AC-3**（FR1, FR2）: 更新した 2 つの記録のどの `acceptance_tests[].tests` エントリにも、旧テストパス 4 件のいずれも含まれていない。
- [ ] **AC-4**（FR5）: 更新した 2 つの記録に載っているすべてのテストパスが、`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list` の出力に現れる。
- [ ] **AC-5**（FR3）: summary の AC-2、summary の AC-5、body の AC-4 のそれぞれのブロックに YAML コメントがある。そのコメントは、取得失敗時の期待が notification-markup-fail-closed SPEC FR1/FR3 によって置き換えられたことを述べている。
- [ ] **AC-6**（FR4, NFR2）: 2 つの記録はどちらも YAML としてパースできる。パース後のデータは、置き換えたテストパス文字列を除いて base revision のデータと等しい。`red_confirmed`・`red_reason`・`baseline_failures`・`final_failures`・`task_id` とリストの順序は変わっていない。
- [ ] **AC-7**（FR6, NFR3）: `.claude/rules/` に新しいファイルがあり、FR6 の方針 4 要素を述べている。
- [ ] **AC-8**（NFR1, NFR2）: base revision に対する `git diff --stat` に現れるのは、2 つの記録、新しいルールファイル、本機能の feature-docs / test-docs のパスだけである。`src-tauri/src/` 配下のファイルと notification-markup-fail-closed のファイルは変更されていない。

## 技術要件

### 機能要件

- **FR1: summary-markup-escape 記録の古いテストパスを更新する**
  `test-docs/notification-summary-markup-escape/task0001.tests.yaml` で、古いテストパス 4 件を現行名に置き換える。置き換えはその場で行い、リストの順序を保つ。
  - 14 行目: `callbacks::tests::summary_markup_escape::failed_capability_fetch_leaves_the_title_unchanged` → `callbacks::tests::summary_markup_escape::failed_capability_fetch_escapes_both_title_and_body_in_the_same_call`
  - 33 行目: `callbacks::tests::body_markup_escape::body_markup_confirmed_is_true_only_for_a_successful_list_containing_it` → `callbacks::tests::body_markup_escape::body_markup_absence_confirmed_is_false_when_present_in_a_successful_list`
  - 34 行目: `callbacks::tests::body_markup_escape::body_markup_confirmed_is_false_when_absent_from_a_successful_list` → `callbacks::tests::body_markup_escape::body_markup_absence_confirmed_is_true_only_for_a_successful_list_without_it`
  - 35 行目: `callbacks::tests::body_markup_escape::body_markup_confirmed_is_false_on_fetch_failure` → `callbacks::tests::body_markup_escape::body_markup_absence_confirmed_is_false_on_fetch_failure`

- **FR2: body-markup-escape 記録の同じ古いテストパスを更新する（波及分）**
  `test-docs/notification-body-markup-escape/task0001.tests.yaml` の AC-4（19〜21 行目）に、FR1 と同じ `body_markup_escape` の置き換え 3 件を適用する。置き換えはその場で行い、リストの順序を保つ。
  - `containing_it` → `is_false_when_present_in_a_successful_list`
  - `absent_from` → `is_true_only_for_a_successful_list_without_it`
  - `on_fetch_failure` → `body_markup_absence_confirmed_is_false_on_fetch_failure`

- **FR3: 反転した各 AC に YAML コメントで置き換え注記を付ける**
  取得失敗時の期待が反転した各 AC のブロック内に YAML コメントを追加する。対象は summary-markup-escape の AC-2 と AC-5、body-markup-escape の AC-4 である。各コメントは、取得失敗時の期待（`get_capabilities()` が失敗したとき title/body をエスケープせずに残す）が `feature-docs/notification-markup-fail-closed/SPEC.md` の FR1/FR3（fail-closed: 取得失敗時にエスケープする）によって置き換えられたことを述べる。リストに載っている現行テストは、その新しい挙動を固定している。`red_confirmed` と `red_reason` を含む既存のキーはそのまま残す。

- **FR4: 文字列とコメントのみの編集**
  2 つの記録への編集は、テストパス文字列の置き換え（FR1/FR2）とコメント行の追加（FR3）だけである。それ以外の行・キー・値・リスト順序はバイト単位で同一に保つ。パース後、各記録は置き換えたテストパス文字列を除いて base revision のデータと等しい。

- **FR5: 載っているすべてのテストパスが実在のテストに解決する**
  更新した 2 つの記録の `acceptance_tests[].tests` リストにあるすべてのテストパスが、src-tauri の `--lib` テスト一覧の少なくとも 1 件のテストにマッチする。0 件マッチのフィルタになるパスは無い。

- **FR6: 機能をまたぐテスト名変更のプロジェクトルール**
  `.claude/rules/` 配下に新しいファイルを 1 つ追加し、test-docs 記録について「現行名に追従する」方針を定める。
  1. ある機能が、先行機能の `test-docs/*/taskNNNN.tests.yaml` に載っているテストの名前を変えた場合、その機能がそれらの `acceptance_tests[].tests` エントリを現行名に更新する。
  2. 名前変更に、先行 AC の期待を反転させる挙動変更が伴う場合、その AC のブロックに YAML コメントで置き換え注記を追加する。注記は置き換え元の SPEC と FR を名指しし、`red_reason` は変更しない。
  3. 名前変更を行った機能自身の、名前変更を記録している文書は原文のまま残す。対象はその機能の SPEC、`reviews/roundN.yaml`、およびその機能自身の tests.yaml である。
  4. 更新した各名前は、プロジェクトの cargo test 一覧の少なくとも 1 件のテストにマッチしなければならない。

### 非機能要件

- **NFR1 - 範囲の限定:** `src-tauri/src/`・`crates/`・ビルド設定の配下は何も変更しない。test-docs のスキーマも変更しない（キーと構造は変わらない。コメントはスキーマではない）。変更対象は FR1/FR2 の 2 つの記録、FR6 のルールファイル、本機能自身の feature-docs / test-docs に限る。
- **NFR2 - 監査記録の完全性:** notification-markup-fail-closed 自身の記録は原文のまま残す。対象は `SPEC.md`、`reviews/round1.yaml`（`line: 14` を参照する finding `03533374d9cc5821` を含む）、`test-docs/notification-markup-fail-closed/task0001.tests.yaml`（`body_markup_confirmed` への過去の言及を含む）である。更新する記録の `red_reason` の値は書き換えない。
- **NFR3 - ルールの配置先はこのリポジトリ内:** 方針はこのプロジェクトの `.claude/rules/` にのみ記録する。em-workflow プラグインのリポジトリ（templates / references）のファイルは書き込まない。ルールファイル名は中身を表す名詞とし、`project-` 接頭辞も `core-` 接頭辞も付けない。

## 前提

- **A-1:** PR #39（notification-markup-fail-closed）は base revision でマージ済みである。`src-tauri/src/callbacks/tests.rs` には現行名 4 件がすべて定義済み（762・756・773・897 行目）であり、テストソースの変更は不要である。
- **A-2:** 方針は follow_current_names とする。更新した記録には現行名を載せ、反転した AC には置き換え注記を付ける。fail-closed の記録に対応表は追加しない。
- **A-3:** 編集対象は summary 記録（4 行）と、body 記録への波及分（AC-4 の 19〜21 行目）である。置き換え注記は summary の AC-2、summary の AC-5、body の AC-4 に付ける。
- **A-4:** 置き換え注記は YAML コメントとし、`red_reason` は実装前の観測記録として残す。test-docs の YAML を書き換えるツールは無いため、コメントは失われない。
- **A-5:** 方針の成果物は、このプロジェクトの `.claude/rules/` 配下の新規ファイル 1 つ（例: `test-docs-records.md`）とする。em-workflow プラグインのテンプレートは変更しない。正確なファイル名は NFR3 の範囲内で planner が決める。
- **A-6:** ルールと本変更の対象は test-docs 記録（`acceptance_tests[].tests` リスト）である。先行機能の feature-docs（SPEC / tasks / VERIFICATION）の本文にある旧名への言及は、本機能の対象外である。
- **A-7:** ルールの対象は後継テストがある名前変更である。後継の無いテスト削除は対象外である。

## 実装方針

### アーキテクチャ

該当なし。design step はスキップした（ドキュメントのみの変更: 2 つの test-docs 記録における文字列置き換えとコメント追加、および `.claude/rules/` のファイル 1 つ。設計すべき UI・モジュール・データモデル・インターフェースは無い）。

### データフロー

該当なし。

### API 設計

該当なし。

### データベーススキーマ

該当なし。

### 依存関係

**内部依存:**
- notification-markup-fail-closed（PR #39）: base revision でマージ済み（A-1）。置き換え注記は `feature-docs/notification-markup-fail-closed/SPEC.md` の FR1/FR3 を参照する。
- `src-tauri/src/callbacks/tests.rs`: 現行名 4 件の定義元（A-1）。変更しない。

**外部依存:**
- なし。

### ファイル構成

```
test-docs/
├── notification-summary-markup-escape/
│   └── task0001.tests.yaml      # FR1, FR3（AC-2, AC-5）
└── notification-body-markup-escape/
    └── task0001.tests.yaml      # FR2, FR3（AC-4）
.claude/rules/
└── <新規ルールファイル>.md       # FR6（ファイル名は NFR3 の範囲内で planner が決める。例: test-docs-records.md）
```

## 宣言する変更集合

本節は手書きの一覧ではなく、create-plan での導出方法を記す。機能固有のパスは、create-plan で `workflow.yaml` の各タスクの `files` エントリから導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、機能固有のパスに加えて、ワークフローが生成する次の 2 つのエントリを既定で宣言する。

- `feature-docs/test-docs-rename-tracking/**`
- `test-docs/test-docs-rename-tracking/**`

`feature-docs/test-docs-rename-tracking/**` は `REQUIREMENTS.md`・`SPEC.md`・`IMPLEMENTATION.md`・`workflow.yaml`・`phase-state/`・`tasks/`・`reviews/roundN.yaml`・`VERIFICATION.md`・`retrospect.yaml`、および design step が生成する設計成果物を含む。これらはフェーズ文書と `references/phase-state.md` が生成・所有する。本節はそれらを引用するのみで、その規則は再掲しない。

`test-docs/test-docs-rename-tracking/**` はタスクごとのテスト記録 `test-docs/test-docs-rename-tracking/{T}.tests.yaml` を含む。これは `implement-phase.md` が生成・所有する。本節はそれを引用するのみで、その規則は再掲しない。

この 2 つの既定エントリは、SPEC の作成者が明示的に取り除かない限り宣言に含まれる。記載が無いことをもって除外とはみなさない。除外は意図的かつ明示的な絞り込みとして行う。

この宣言は上位集合としての表明である。検証時に観測される実際の変更集合は、宣言した集合に「含まれて」いなければならず、一致する必要はない。implement タスクを生まない機能は `test-docs/{feature}/` ディレクトリを生成しないが、その場合も宣言した `test-docs/{feature}/**` は正しい。宣言したパスが実体化しないことは違反ではない。

## テストシナリオ

### cargo test による確認

- [ ] **TS-2**（AC-1, AC-2, AC-4）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list` を実行する。両記録のすべてのテストパスが、`: test` 接尾辞付きで出力に現れることを確認する。
- [ ] **TS-3**（AC-4）: 新しい名前 4 件のそれぞれで、フィルタ付き実行 `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib <name>` を行う。各実行で少なくとも 1 件のテストが実行されたと報告されること（`0 passed; 0 failed; N filtered out` ではないこと）。

### 手動コマンドによる確認

- [ ] **TS-1**（AC-3）: `test-docs/notification-summary-markup-escape/` と `test-docs/notification-body-markup-escape/` を旧テストパス 4 件で grep する。`acceptance_tests` のリストエントリにマッチするものが無いことを確認する。
- [ ] **TS-4**（AC-6）: 両記録を base revision と HEAD でそれぞれ YAML safe loader でパースして比較する。差分は置き換えたテストパス文字列だけであること。
- [ ] **TS-5**（AC-5）: summary の AC-2、summary の AC-5、body の AC-4 の 3 ブロックを確認する。それぞれに notification-markup-fail-closed SPEC FR1/FR3 を引用するコメントがあること。
- [ ] **TS-6**（AC-7）: 新しい `.claude/rules/` ファイルを読み、FR6 の方針 4 要素を述べていることを確認する。
- [ ] **TS-7**（AC-8）: `git diff --stat <base>..HEAD` を実行し、変更集合が宣言したパスの範囲内であることを確認する。

### E2E テスト

**既存の E2E テスト**: なし
**実行コマンド**: 検出されず

### エッジケース

- [ ] 名前変更に伴って述語の極性が反転している（`body_markup_confirmed` → `body_markup_absence_confirmed`）。対応付けは true/false の語ではなく capability の場合分け（あり / なし / 取得失敗）に従う: `containing_it` → `is_false_when_present`、`absent_from` → `is_true_only_..._without_it`。
- [ ] summary の AC-2 の取得失敗エントリを置き換える現行テストは、AC-2 の元の期待とは逆の挙動を固定している。これを読める状態に保つのが FR3 の置き換え注記である。
- [ ] コメント行の追加で summary 記録の行番号がずれる。`round1.yaml` の finding `03533374d9cc5821`（`line: 14`）とチケットの行番号参照は過去の記録として残し、更新しない（NFR2）。
- [ ] ツールがファイルをパースして書き戻すと（safe_load の往復）、YAML コメントは失われる。現状、test-docs の YAML を書き戻すツールは無い（前提 A-4）。
- [ ] summary の AC-2 で反転しているのは取得失敗エントリだけである。Ok かつ body markup なしの 2 エントリは引き続き成立するため、注記の範囲は取得失敗時の挙動に限定する。

### 性能テスト

該当なし。

## セキュリティ上の考慮事項

該当なし。

## エラー処理

該当なし。

## 性能最適化

該当なし。

## 成功基準

- [ ] すべての機能要件が実装され、確認されている
- [ ] すべてのテストシナリオが通る
- [ ] AC-1〜AC-8 がすべて満たされている

## 未解決事項

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（`status: tbd` の要件は無い）。

## 参照

- notification-markup-fail-closed SPEC: `feature-docs/notification-markup-fail-closed/SPEC.md`
- notification-markup-fail-closed レビュー記録: `feature-docs/notification-markup-fail-closed/reviews/round1.yaml`
- notification-markup-fail-closed テスト記録: `test-docs/notification-markup-fail-closed/task0001.tests.yaml`
- summary-markup-escape テスト記録: `test-docs/notification-summary-markup-escape/task0001.tests.yaml`
- body-markup-escape テスト記録: `test-docs/notification-body-markup-escape/task0001.tests.yaml`
- 現行テストの定義元: `src-tauri/src/callbacks/tests.rs`
- REQUIREMENTS.md: reduced tier では作成しない
