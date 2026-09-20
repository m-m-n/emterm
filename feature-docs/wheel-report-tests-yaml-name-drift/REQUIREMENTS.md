---
title: "wheel-report-tests-yaml-name-drift"
created_date: 2026-09-20
status: draft
---

# wheel-report-tests-yaml-name-drift - 要件定義書

> **用語の注意**: このドキュメントで `AC-1`〜`AC-6` と番号だけで呼ぶものは、この
> フィーチャー自身の受け入れ基準（11.1）である。修復対象のファイル
> `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` も独自の
> `AC-1`〜`AC-10` を持つため、後者を指すときは必ずファイル名を添えて
> 「`task0001.tests.yaml` の AC-7」のように書く。

## 1. 概要

### 1.1 背景

`test-docs/wheel-report-fraction-accum/task0001.tests.yaml` は、フィーチャー
`wheel-report-fraction-accum` の task0001 のテスト記録である。この記録の一部が、
記述対象のコードから乖離している。

- `task0001.tests.yaml` の AC-7 の `tests` リスト（62 行目）は
  `window_host::tests::decide_wheel_event_detects_reactivation_observed_only_by_an_owner_recorded_release`
  を引用しているが、この名前はもうどの定義にも解決しない。この引用を名前で再実行しても、
  失敗として顕在化せず `0 tests run` と報告される。
- `task0001.tests.yaml` の AC-6 と AC-7 の `red_reason` 本文（57 行目、67 行目、
  73-75 行目）は、フィールド `MouseReportRecords.last_tracking_active` を参照して
  いるが、この識別子は HEAD のどのシンボルも指していない。

### 1.2 目的

task0001 の実行可能な受け入れ証跡を回復し、記録が引用するすべてのテスト名が実在する
定義に解決するようにする。あわせて `red_reason` 本文から、HEAD のどのシンボルも
指していない識別子を取り除く。記録された red の判定は一切再判定しない。

### 1.3 スコープ

対象: 単一ファイル `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` の中の、
列挙された 4 箇所に限定したテキスト修正（`task0001.tests.yaml` の AC-7 のテスト名
62 行目、および `red_reason` 本文の 57 行目・67 行目・73-75 行目）と、この task に
対して記録する名前解決の手動証跡。

対象外:

- テストの追加・削除。
- `task0001.tests.yaml` の AC-9 の `#[allow(dead_code)]` に関する記述（95-97 行目）の
  編集。これは乖離ではなく、task0001 自身がその時点で行ったことの正確な履歴記録で
  ある。
- テスト名の乖離を自動検出する仕組み、その他の新規ツールの追加。
- `test-docs/wheel-report-fraction-accum/task0002.tests.yaml` への編集。こちらは既に
  現行の名前を参照しており、修復を必要としない。
- `src-tauri/src/window_host/tests.rs` および production の wheel-report コードへの
  変更。

## 2. ビジネス要件

### 2.1 ビジネス目標

- `wheel-report-fraction-accum` のテスト記録における task0001 の実行可能な受け入れ証跡を
  回復する。記録が引用するすべてのテスト名が実在する定義に解決し、証跡を名前で再実行した
  ときに黙って `0 tests run` と報告されることがないようにする。
- task0001 の `red_reason` 本文から、HEAD のどのシンボルも指していない識別子を取り除く。
  実装前の失敗状態を再構成する読み手が、存在しなかったフィールド、あるいはもう存在しない
  フィールドを追いかけさせられないようにする。
- 記録の歴史的な意味をそのまま保つ。`red_reason` は実装が存在しなかった時点で観測された
  失敗を記録したものであり、この修復は壊れた参照のテキスト修正であって、red 判定の再判定
  ではない。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 記録された証跡を再実行する開発者 | `task0001.tests.yaml` が引用するテスト名を名前で実行し、受け入れ証跡を再確認する |
| 実装前の失敗状態を再構成する読み手 | `task0001.tests.yaml` の AC-6 / AC-7 の `red_reason` 本文を読み、実装が存在しなかった時点で何が失敗したのかを理解する |

### 2.3 期待される効果

- 引用されたテスト名を再実行すると、黙って `0 tests run` と報告されるかわりに、0 でない
  テスト件数が報告される。
- `red_reason` 本文が、HEAD に実在するシンボルを指す識別子だけに依拠する。
- 記録済みの red の判定が、記録された当時の意味をそのまま保つ。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | `task0001.tests.yaml` が引用する受け入れ証跡をテスト名で再実行する | 記録された証跡を再実行する開発者 | 高 |
| UC02 | `task0001.tests.yaml` の AC-6 / AC-7 の `red_reason` 本文から実装前の失敗状態を再構成する | 実装前の失敗状態を再構成する読み手 | 高 |

### 3.2 ユースケース詳細

#### UC01: `task0001.tests.yaml` が引用する受け入れ証跡をテスト名で再実行する

**アクター**: 記録された証跡を再実行する開発者

**事前条件**:
- PR #73（`em-workflow/wheel-report-fraction-accum/integration`）がマージ済みであり、
  記録と、それが解決先とすべきテスト定義の両方が存在する。

**基本フロー**:
1. 開発者が `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` の
   `acceptance_tests` エントリからテスト名を読む。
2. 開発者が
   `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib <name>`
   を実行する。
3. 実行結果が 0 でないテスト件数を報告する。

**代替フロー**:
- この修復の前は、`task0001.tests.yaml` の AC-7 エントリ（62 行目）が存在しない定義を
  名指ししているため、手順 3 はエラーを出さずに `0 tests run` と報告する。

**事後条件**:
- 記録が引用するすべてのテスト名が、`src-tauri/src` に存在する定義に解決する。

#### UC02: `task0001.tests.yaml` の AC-6 / AC-7 の `red_reason` 本文から実装前の失敗状態を再構成する

**アクター**: 実装前の失敗状態を再構成する読み手

**事前条件**:
- 修復済みの `task0001.tests.yaml` がマージ済みのベースに存在する。

**基本フロー**:
1. 読み手が `task0001.tests.yaml` の AC-6 の `red_reason`（54-58 行目）と AC-7 の
   `red_reason`（65-75 行目）を読む。
2. そこで引用されている識別子はすべて HEAD に実在するシンボルを指している。
3. 読み手が、実装が存在しなかった時点で観測された失敗状態を再構成する。

**代替フロー**:
- この修復の前は、本文が読み手を `MouseReportRecords.last_tracking_active` に向かわせる
  が、この識別子は HEAD のどのシンボルも指していない。

**事後条件**:
- `last_tracking_active` がファイル内のどこにも現れず、両方の本文がそれ無しで筋の通った
  文章として読める。
- すべての `red_confirmed` フラグが記録された値を保っている。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 解決しない AC-7 のテスト名を貼り替える | `task0001.tests.yaml` の AC-7 の `tests` エントリ（62 行目）を、後継のテストに置き換える | 高 |
| FR2 | AC-6 の red_reason 本文（57 行目）を修復する | 57 行目の、存在しないフィールドへの参照を取り除く | 高 |
| FR3 | AC-7 の red_reason のエラー引用（67 行目）を修復する | 引用されたフィールド不存在エラーのテキストから `last_tracking_active` を取り除く | 高 |
| FR4 | 削除したフィールドに依存する AC-7 の red_reason の文（73-75 行目）を修復する | シナリオが表現不能であることの根拠を削除フィールドに置いている節だけを書き換える | 高 |
| FR5 | すべての red 判定と、なお正確なテキストを保持する | 編集を列挙された 4 箇所に限定する | 高 |
| FR6 | AC-1 のための名前解決の手動証跡を記録する | 参照される各名前について定義のファイル+行と、名前で絞り込んだ `cargo test` の実行を記録する | 高 |

### 4.2 機能詳細

#### FR1: 解決しない AC-7 のテスト名を貼り替える

**説明**: `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` において、
`task0001.tests.yaml` の AC-7 の `tests` エントリ（62 行目）
`window_host::tests::decide_wheel_event_detects_reactivation_observed_only_by_an_owner_recorded_release`
を、これを引き継いだテスト
`window_host::tests::decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`
に置き換える。`task0001.tests.yaml` の AC-7 の他の 2 エントリ（61 行目と 63 行目）は
既に解決するため、そのままとする。

**入力**:
- `task0001.tests.yaml` 62 行目: 解決しない旧テスト名。

**出力**:
- `task0001.tests.yaml` 62 行目: 後継のテスト名。その定義は
  `src-tauri/src/window_host/tests.rs:3372` に存在する。

**ビジネスルール**:
- 変更するのは 62 行目のエントリのみ。`task0001.tests.yaml` の AC-7 の 61 行目と
  63 行目のエントリはバイト同一のまま残す。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| 引用した名前がなお解決しない | 置き換えた名前が定義に解決しない | AC-1 の手動証跡（定義のファイル+行、および 0 でない件数を報告する `cargo test` の実行）が得られず、修復は受け入れられない |

#### FR2: AC-6 の red_reason 本文（57 行目）を修復する

**説明**: `task0001.tests.yaml` の AC-6 の `red_reason`（54-58 行目）において、
57 行目の、存在しないフィールド `MouseReportRecords.last_tracking_active` への参照を
取り除く。引用された `error[E0560]` のうち `report_accum` の側と、文の残りは保持する。
`task0001.tests.yaml` の AC-6 の `red_confirmed: true` フラグと、叙述の残りの部分には
手を触れない。

**ビジネスルール**:
- 引用された `error[E0560]` のうち `report_accum` の側は逐語的に保持する。
- `red_confirmed` には手を触れない。

#### FR3: AC-7 の red_reason のエラー引用（67 行目）を修復する

**説明**: `task0001.tests.yaml` の AC-7 の `red_reason`（65-75 行目）において、
67 行目の、引用されたフィールド不存在エラーのテキストから `last_tracking_active` を
取り除く。いずれも HEAD に実在するシンボルを指す `report_accum` と `tracking_active` は
残す。引用されている `report_accum` の出現回数と `red_confirmed: true` フラグには
手を触れない。

**ビジネスルール**:
- `report_accum` と `tracking_active` は引用に残す。
- 引用されている `report_accum` の出現回数は調整しない。

#### FR4: 削除したフィールドに依存する AC-7 の red_reason の文（73-75 行目）を修復する

**説明**: 73-75 行目の、シナリオが表現不能であることの根拠を
`MouseReportRecords.last_tracking_active` に置いている節だけを書き換え、その文が
HEAD に存在するシンボル（`RecordUpdates.tracking_active`）のみに依拠するようにする。
周囲の歴史的な叙述 — コーディネーターのセカンドオピニオンによる明確化への言及と
D10 のギャップを含む — は保持する。

**ビジネスルール**:
- 書き換えるのは 73-75 行目の節のみ。コーディネーターのセカンドオピニオンによる
  明確化と D10 のギャップを含む、周囲の叙述は保持する。

#### FR5: すべての red 判定と、なお正確なテキストを保持する

**説明**: `task0001.tests.yaml` のどの `red_confirmed` フラグも値を変えず、受け入れ基準の
エントリを追加も削除もせず、なお正確な識別子（`report_accum`、
`RecordUpdates.tracking_active`、`accumulate_wheel_report_lines`、
`MAX_WHEEL_REPORT_NOTCHES`、`wheel_report_notches_*` 系）を一切改変しない。編集は
列挙された 4 箇所（62 行目、および 57 行目・67 行目・73-75 行目の `red_reason` 本文）に
限定する。

**ビジネスルール**:
- `red_reason` は実装が存在しなかった時点で観測された失敗状態の記録であり、この修復は
  `red_confirmed` の判定を決して再判定しない。

#### FR6: AC-1 のための名前解決の手動証跡を記録する

**説明**: 修復後の `task0001.tests.yaml` が参照する各テスト名について、その `fn` 定義の
ファイルと行、および その名前で絞り込んだ `cargo test` の実行が 0 でないテスト件数を
報告することを記録し、その証跡を当該 task 自身の検証記録に添付する。新しいテストも、
自動のテスト名乖離チェックも追加しない。

**ビジネスルール**:
- 証跡は手動で記録する。自動化は導入しない。

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし: この変更はドキュメントのみであり、実行時のコードパスを一切変更しない。

### 5.2 セキュリティ要件

該当なし: この変更はドキュメントのみであり、入力処理・認証・認可の面を持たない。

### 5.3 可用性要件

該当なし: この変更はドキュメントのみである。

### 5.4 保守性要件

- **NFR1 — ドキュメントのみの変更**: 変更は
  `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` のみに触れる。
  `src-tauri/src` 配下および `crates/` 配下のファイルは変更せず、テストの追加・改名・
  削除も行わず、この task によって `--lib` スイートの成功/失敗の件数は変わらない。
- **NFR2 — ファイルの YAML の形を保つ**: ファイルは妥当な YAML のままであり、既存の形を
  保つ。折り畳みブロックスカラー、引用されたコンパイルエラー内で使われている
  バックスラッシュでエスケープしたバッククォートの慣行、キー順、周囲の行折り返しスタイル。

### 5.5 互換性要件

- **NFR3 — 順序の制約**: 作業は PR #73
  （`em-workflow/wheel-report-fraction-accum/integration`）がマージされた後にのみ開始する。
  修復対象のファイルと、それが解決先とすべきテスト定義の両方が、そのブランチに由来する
  ためである。

## 6. UI/UX要件

該当なし: この変更はユーザーから見える面を持たず、UI も視覚要素も持たない。

## 7. データ要件

該当なし: この変更はデータの形を導入しない。触れる成果物は YAML の証跡記録
`task0001.tests.yaml` のみで、そのトップレベル構造（`task_id`、`baseline_failures`、
`final_failures`、AC-1..AC-10 を持つ `acceptance_tests`、`notes`）は変更せずに保持する
（NFR2）。

## 8. 外部連携

該当なし: 外部システムは関与しない。

## 9. 制約条件

### 9.1 技術的制約

- ファイルは妥当な YAML のままで、既存の形を保つ必要がある（NFR2）。
- `src-tauri/src` 配下および `crates/` 配下のファイルは変更できず、テストの追加・改名・
  削除も行えない（NFR1）。

### 9.2 ビジネス上の制約

- この修復は壊れた参照のテキスト修正であり、記録された red 判定の再判定では決してない。

### 9.3 スケジュール制約

- 作業は PR #73（`em-workflow/wheel-report-fraction-accum/integration`）がマージされた
  後にのみ開始する（NFR3）。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの
`files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（明示的に除外しない限り、常に宣言に含まれる）:

- `feature-docs/wheel-report-tests-yaml-name-drift/**`
- `test-docs/wheel-report-tests-yaml-name-drift/**`

これに加えて、このフィーチャーは修復対象の単一ファイルを宣言する:

- `test-docs/wheel-report-fraction-accum/task0001.tests.yaml`

**意味論**:

- デフォルトのメンバーは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。除外は
  意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセットの主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）
  必要がある。実際には生成されないパスが宣言されていても違反にはならない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 後継の名前が、`task0001.tests.yaml` の AC-7 が書かれた対象のシナリオをカバーしていない可能性 | 高 | `task0002.tests.yaml` の AC-4 が、task0001 の reactivation テストから貼り替えたものとしてこの名前を挙げており、その定義は `src-tauri/src/window_host/tests.rs:3372` に存在する（A4） |
| `MouseReportRecords.last_tracking_active` が production の struct に存在しないことを独立に再検証していない | 高 | `mouse_report.rs` はスキャン対象に含まれておらず、結論は `src-tauri/src/window_host/tests.rs` に `last_tracking_active` の出現が 0 件であることに依拠している（A5） |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| `red_reason` を現在の green なコードの記述に書き換える修復をしてしまうと、証跡の意味そのものが変わる | 低 | 中 | 修復は HEAD のどのシンボルも指していない識別子の削除のみを行い、`red_confirmed` の判定を決して再判定しない（A1） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] **AC-1**: `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` が参照する
      すべてのテスト名が、`src-tauri/src` に存在する定義に解決する。
      *検証*: 手動証跡 — 参照される各名前について、定義のファイルと行、および
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib <name>`
      の実行が 0 でないテスト件数を報告することを記録する。（FR1, FR6）
- [ ] **AC-2**: これまで旧名を記していた `task0001.tests.yaml` の AC-7 エントリが
      `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`
      を記すようになり、旧名がファイル内のどこにも現れない。
      *検証*: 修復後のファイルを旧名でテキスト検索して一致が無いこと、および
      `task0001.tests.yaml` の AC-7 の `tests` リストに新しい名前が存在すること。（FR1）
- [ ] **AC-3**: 識別子 `last_tracking_active` が `task0001.tests.yaml` のどこにも現れず、
      `task0001.tests.yaml` の AC-6 / AC-7 の `red_reason` 本文がそれ無しで筋の通った
      文章として読める。
      *検証*: テキスト検索で一致が無いこと。`task0001.tests.yaml` の AC-6 と AC-7 の
      `red_reason` 本文を通しで読み、削除したフィールドへの宙に浮いた参照が残っていない
      こと。（FR2, FR3, FR4）
- [ ] **AC-4**: すべての `red_confirmed` の値、すべての受け入れ基準のキー、および列挙
      された 4 箇所の外側のテキストが、変更前のファイルとバイト同一である。
      *検証*: 当該ファイルの `git diff` のハンクが、`task0001.tests.yaml` の AC-7 の
      テスト名の行と AC-6 / AC-7 の `red_reason` 本文に収まっており、変更箇所に
      `red_confirmed` の行も `task0001.tests.yaml` の AC-9 の行も含まれないこと。（FR5）
- [ ] **AC-5**: 修復後のファイルが妥当な YAML としてパースでき、トップレベル構造
      （`task_id`、`baseline_failures`、`final_failures`、AC-1..AC-10 を持つ
      `acceptance_tests`、`notes`）が同一である。
      *検証*: ファイルの YAML パースが成功し、キー集合が変わっていないこと。（NFR2）
- [ ] **AC-6**: `test-docs/wheel-report-fraction-accum/task0001.tests.yaml` 以外の
      ファイルが変更されていない。
      *検証*: ベースに対する `git diff --name-only` がそのパス 1 つ（および このフィーチャー
      自身の workflow ドキュメント）だけを列挙すること。（NFR1）

### 11.2 KPI

該当なし: この変更は一度限りのドキュメント修復であり、継続的に測定する指標を持たない。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] **TS-1**（名前解決）: 修復後の `task0001.tests.yaml` の AC-1..AC-10 全体で参照
      される 13 個の異なるテスト名それぞれについて、`src-tauri/src/window_host/tests.rs`
      で定義を探し、その定義行を記録する。
      *方法*: 手動証跡（`fn <name>(` の grep）。自動化は追加しない。
- [ ] **TS-2**（実行可能な証跡）: 修復後の `task0001.tests.yaml` の AC-7 の名前で
      絞り込んだ `cargo test --lib` が 0 でないテスト件数を報告する（変更前の名前では
      0 になる）。
      *方法*:
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`。
- [ ] **TS-3**（不在確認）: 修復後のファイルに旧テスト名の出現も `last_tracking_active`
      の出現も無い。
      *方法*: 変更した単一ファイルのテキスト検索。
- [ ] **TS-4**（スコープの封じ込め）: 差分が列挙された箇所に収まっており、すべての
      `red_confirmed` フラグと `task0001.tests.yaml` の AC-9 の本文に触れていない。
      *方法*: `git diff` のレビュー。
- [ ] **TS-5**（回帰ガード）: `--lib` スイートが同じ件数で引き続き成功し、この変更が
      ドキュメントのみであることを裏づける。
      *方法*:
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `red_reason` | `{T}.tests.yaml` の受け入れエントリのフィールド。実装が存在しなかった時点で観測された失敗状態を記録する。 |
| `red_confirmed` | `red_reason` が記述する失敗が実際に観測されたことを記録するフラグ。 |
| 名前の乖離（name drift） | テスト記録が、もうどの定義にも解決しないテスト名を引用している状態。名前で絞り込んだ実行が失敗せず `0 tests run` になる。 |
| D10 | `task0001.tests.yaml` の AC-7 の `red_reason` の叙述が参照しているギャップ。 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] `requirement.red-reason-update-style` → `rewrite_minimal`: `task0001.tests.yaml` の
      AC-6 / AC-7 の `red_reason` 本文のうち、HEAD に存在しないシンボルを名指ししている
      部分（`MouseReportRecords.last_tracking_active` への参照と、それに依存する
      73-75 行目の文）だけを修復する。歴史的な叙述の残り、なお正確な識別子
      （`report_accum`、`RecordUpdates.tracking_active`）、およびすべての `red_confirmed`
      フラグには手を触れない。
- [x] `requirement.stale-statement-scope` → `enumerated_only`: 変更を、62 行目のテスト名と
      `red_reason` 本文に、タスク説明が列挙したとおりに限定する。`task0001.tests.yaml` の
      AC-9 の `#[allow(dead_code)]` に関する記述（95-97 行目）はそのまま残す。
- [x] `requirement.ac1-verification-method` → `manual_evidence`: AC-1 は、参照される各
      テスト名について、その定義のファイルと行、および その名前で絞り込んだ `cargo test`
      の実行が 0 でないテスト件数を報告することを記録して証明する。新しいテストも、自動の
      乖離チェックも追加しない。
- [x] `design-step.recommendation` → `decide_autonomously`: デザインステップはスキップ
      する。この変更は単一の YAML 証跡記録内のテキスト修正であり、production コードも、
      ユーザーから見える面も、データの形も、アーキテクチャ上の選択も関与しない。

### 14.2 前提事項

- [ ] **A1**（影響度 中、可逆）: `red_reason` は実装が存在しなかった時点で観測された失敗
      状態の記録である。したがって修復は HEAD のどのシンボルも指していない識別子の削除の
      みを行い、本文を現在の green なコードの記述に書き換えることも、`red_confirmed` の
      判定を再判定することも決してしない。
      *根拠*: `requirement.red-reason-update-style` の回答（`rewrite_minimal`）。
      オーケストレーターは、これがこのリポジトリで姉妹フィーチャー
      `feature-docs/ac7-red-reason-scope/SPEC.md` によって既に確立された原則であると
      注記している。
- [ ] **A2**（影響度 中、可逆）: `task0001.tests.yaml` の AC-9 の `#[allow(dead_code)]` に
      関する記述（95-97 行目）は記述されたまま残す。後続の task が実装を変更しても、
      それ以前の履歴記録が偽になるわけではなく、その記述は task0001 自身がその時点で
      行ったことを述べたものである。
      *根拠*: `requirement.stale-statement-scope` の回答（`enumerated_only`）。A1 と整合する。
- [ ] **A3**（影響度 低、可逆）: AC-1 は記録された手動証跡（定義のファイル+行、および
      名前で絞り込んだ `cargo test` が 0 でない件数を報告すること）によって証明する。
      自動のテスト名乖離チェックは導入しない。
      *根拠*: `requirement.ac1-verification-method` の回答（`manual_evidence`）。タスク説明が
      テストの追加・削除をスコープ外としている。
- [ ] **A4**（影響度 高、可逆）:
      `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`
      が、削除する `task0001.tests.yaml` の AC-7 の名前の正しい後継であり、
      `task0001.tests.yaml` の AC-7 が書かれた対象である、owner が記録した release が
      tracking の非アクティブを観測するという D10 のシナリオを引き続きカバーしている。
      *根拠*: `test-docs/wheel-report-fraction-accum/task0002.tests.yaml` の AC-4 が、
      task0001 の reactivation テストから貼り替えたものとしてこの名前を挙げており、その定義は
      `src-tauri/src/window_host/tests.rs:3372` に存在する。
- [ ] **A5**（影響度 高、可逆）: `report_accum` と `RecordUpdates.tracking_active` は HEAD に
      実在するシンボルを指しているため手を触れない。一方
      `MouseReportRecords.last_tracking_active` は指していない。
      *根拠*: `src-tauri/src/window_host/tests.rs` には `report_accum` / `tracking_active` 系の
      出現が 39 件あり、`last_tracking_active` の出現は 0 件である。`mouse_report.rs` は
      スキャン対象に含まれておらず、このフィールドが production の struct に存在しないことは
      独立には再検証していない。
- [ ] **A6**（影響度 中、可逆）: 作業は PR #73 がマージされた後にのみ開始し、修復対象は
      そのマージ済みベース上のファイルである。
      *根拠*: タスク説明に制約として記述されている。

## 15. 参考資料

- `test-docs/wheel-report-fraction-accum/task0001.tests.yaml`: このフィーチャーが修復する
  ファイル。
- `test-docs/wheel-report-fraction-accum/task0002.tests.yaml`: その AC-4 が、task0001 の
  reactivation テストから貼り替えたものとして後継のテストを挙げている。
- `src-tauri/src/window_host/tests.rs:3372`:
  `decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero`
  の定義。
- `feature-docs/ac7-red-reason-scope/SPEC.md`: 「再判定ではなくテキスト修正」という原則を
  このリポジトリで確立した姉妹フィーチャー。
- PR #73（`em-workflow/wheel-report-fraction-accum/integration`）: この作業が順序として
  後に続くマージ。
