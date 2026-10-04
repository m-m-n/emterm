---
title: "lib-budget-tests-load-tolerance"
created_date: 2026-10-04
status: draft
---

# lib-budget-tests-load-tolerance - 要件定義書

## 1. 概要

### 1.1 背景
`--lib` の全体実行で、次の 2 つのテストが環境負荷（load average 10-20）によって失敗する。

- `round4_chain::the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus`
- `strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget`

### 1.2 目的
- `--lib` の全体実行で、上記 2 つのテストが環境負荷（load average 10-20）を原因として失敗しないようにする。
- `strip_concat_query` AC-7（NFR1、NFR2、TS-9）の線形時間ガードを、テスト対象コードの CPU 時間だけを計測する形で維持する。

### 1.3 スコープ
- 対象は上記 2 つのテストのみ。
- 上記 2 つ以外の wall-clock による予算判定はすべて現状のまま残す（`round4_chain.rs` と `strip_concat_query.rs` の他の `BUDGET` 使用箇所、`strip_join_escape_closure`、`strip_open_string_body_closure`、`round4_as05`、`tests.rs`）。

## 2. ビジネス要件

### 2.1 ビジネス目標
- `--lib` の全体実行で、`round4_chain::the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` と `strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget` が環境負荷（load average 10-20）を原因として失敗しない。
- `strip_concat_query` AC-7（NFR1、NFR2、TS-9）の線形時間ガードを、テスト対象コードの CPU 時間だけを計測する形で維持する。

### 2.2 対象ユーザー
該当なし。

### 2.3 期待される効果
- 環境負荷（load average 10-20）の下でも、上記 2 つのテストが失敗しない。
- 線形時間ガードがテスト対象コードの CPU 時間だけで判定される。

## 3. ユースケース

該当なし（UI・表示出力のない、テストのみの変更）。

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 |
|----|--------|------|
| FR1 | token-corpus テストの wall-clock 判定の削除 | `the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` から経過時間の判定を外す |
| FR2 | テスト対象コードに限定したスレッド CPU 時間の予算 | `strip_concat_alternations_and_chains_finish_within_the_budget` の予算判定を、計測対象呼び出しのスレッド CPU 時間の合計で行う |
| FR3 | スレッド CPU 時間ヘルパー | 呼び出し元スレッドだけが消費した CPU 時間を計測するテスト側ヘルパー |
| FR4 | クロック失敗を隠さない | クロック API が失敗を報告したら panic する |
| FR5 | Windows 依存の feature | 既存の `windows-sys` 依存に `Win32_System_Threading` feature を追加する |
| FR6 | 再発検知: ヘルパーは CPU 外の時間を数えない | スリープ時間と他スレッドの CPU 時間を数えないことをテストで示す |
| FR7 | 再発検知: ヘルパーは自スレッドを数える | 計測スレッド上の CPU 処理が数えられることをテストで示す |
| FR8 | 再発検知: 2 乗の対照 | FR2 と同じ予算判定が 2 乗の代替負荷を不合格にすることをテストで示す |
| FR9 | スコープ | 変更するのは名指しの 2 テストのみ |

### 4.2 機能詳細

#### FR1: token-corpus テストの wall-clock 判定の削除

**説明**: `round4_chain::the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` は経過時間の判定を持たなくなる。`let start = Instant::now()` と `assert!(start.elapsed() < BUDGET, ...)` を削除する。その他のアサーションはすべて変更しない。

#### FR2: テスト対象コードに限定したスレッド CPU 時間の予算

**説明**: `strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget` は、`strip_pty_output_for_scrollback_write`、`strip_replayable_rich_content`、`ScrollbackWriteFilter::feed` の呼び出しだけのスレッド CPU 時間を合計し、その合計が 10s の `BUDGET` 未満であることをアサートする。

**ビジネスルール**:
- 合計から除外するもの: 入力と期待値の生成（`repeated`、`concat`）、等値比較、フィルタ出力のテスト側での連結。
- 計測対象の呼び出しには、1 回呼び出しの feed（オーバーフローのフラッシュ）、アラインされた 64 KiB 読み込み、アラインされていない 64 KiB 読み込み、16 KiB の ESC チェーン読み込みが含まれる。出力に対するテスト側の `extend_from_slice` は除外する。
- テストの正しさに関するアサーションは変更しない。

#### FR3: スレッド CPU 時間ヘルパー

**説明**: テスト側のヘルパーが、呼び出し元スレッドだけが消費した CPU 時間を計測する。

**ビジネスルール**:
- Linux: `libc` 経由で `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` を使う。
- Windows: `windows-sys` 経由で `GetThreadTimes(GetCurrentThread())`（カーネル時間とユーザー時間の合計）を使う。

#### FR4: クロック失敗を隠さない

**説明**: クロック API が失敗を報告したとき、ヘルパーは失敗内容を示すメッセージで panic する。ゼロを返すことも、wall-clock 時間にフォールバックすることもしない。

**エラーケース**:
| エラー | 条件 | 対応 |
|--------|------|------|
| クロック API の失敗（Linux） | `clock_gettime` が非ゼロを返す | 失敗内容を示すメッセージで panic する。ゼロとして報告しない |
| クロック API の失敗（Windows） | `GetThreadTimes` が `FALSE` を返す | 失敗内容を示すメッセージで panic する。ゼロとして報告しない |

#### FR5: Windows 依存の feature

**説明**: `src-tauri/Cargo.toml`（`[target.'cfg(windows)'.dependencies]`）の既存の `windows-sys` 依存に `Win32_System_Threading` feature を追加する。新しい crate は追加しない。

#### FR6: 再発検知: ヘルパーは CPU 外の時間を数えない

**説明**: 計測スレッドがスリープしている時間と、計測スレッドが待っている間に別スレッドが消費した CPU 時間を、ヘルパーが数えないことをテストで示す。

#### FR7: 再発検知: ヘルパーは自スレッドを数える

**説明**: 計測スレッド上で行った CPU 処理が数えられることをテストで示す（計測値が正であり、処理量に応じて増える）。

#### FR8: 再発検知: 2 乗の対照

**説明**: 短い予算を持つ通常の `#[test]` で、FR2 と同じ予算判定が 2 乗の代替負荷を不合格にすることを示す。

#### FR9: スコープ

**説明**: 変更するのは名指しの 2 テストのみ。その他の wall-clock による予算判定はすべて現状のまま残す: `round4_chain.rs` と `strip_concat_query.rs` の他の `BUDGET` 使用箇所、`strip_join_escape_closure`、`strip_open_string_body_closure`、`round4_as05`、`tests.rs`。

## 5. 非機能要件

### 5.1 パフォーマンス要件
- NFR3: 計測はスレッド単位で行う。libtest で並列に走る他のテストは計測に含まれない。
- NFR4: 計測対象の呼び出しに 100k 回繰り返しで 2 乗の回帰が入った場合、10s の CPU 予算を超える。参考値として、負荷のない状態での debug テスト全体はオーケストレーターの計測で 3.71s wall / 3.68s user。

### 5.2 セキュリティ要件
該当なし。

### 5.3 可用性要件
該当なし。

### 5.4 保守性要件
- NFR1: テストフレームワークの crate を追加しない。std と既存の `libc`（unix）、`windows-sys`（Windows）依存を使う。

### 5.5 互換性要件
- NFR2: テストは Linux と Windows でコンパイル・実行できる。`--no-default-features` のチェックビルドも引き続きコンパイルできる。

## 6. UI/UX要件

該当なし（UI・表示出力のない、テストのみの変更）。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約
- テストフレームワークの crate を追加しない（NFR1）。
- Linux と Windows でコンパイル・実行できる（NFR2）。
- `windows-sys` には feature を追加するのみで、新しい crate は追加しない（FR5）。

### 9.2 ビジネス上の制約
- 変更するのは名指しの 2 テストのみ（FR9）。

### 9.3 スケジュール制約
該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/lib-budget-tests-load-tolerance/**`
- `test-docs/lib-budget-tests-load-tolerance/**`

`feature-docs/lib-budget-tests-load-tolerance/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/lib-budget-tests-load-tolerance/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/lib-budget-tests-load-tolerance/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/lib-budget-tests-load-tolerance/` ディレクトリを生成しないが、宣言された `test-docs/lib-budget-tests-load-tolerance/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題
| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| スレッド CPU 時間も、競合下では周波数スケーリング・キャッシュ・SMT によってある程度増えるため、負荷から完全には独立しない（EC1、A3） | 中 | 負荷のない状態での計測値（テスト全体で約 3.7s、計測対象の呼び出しだけならそれ未満）は 10s の予算に対して余裕がある（A3） |

### 10.2 ビジネスリスク
該当なし。

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC1: 合成 CPU 負荷の下で、`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が名指しの 2 テストを通す。負荷は `run_in_background` で起動し `TaskStop` で停止する CPU バウンドのプロセスで与え、load average 10-20 相当の水準とする。（FR1、FR2）
- [ ] AC2: `the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` が経過時間のアサーションを持たず、リプレイ一致のアサーションがすべて引き続き通る。（FR1）
- [ ] AC3: `strip_concat_alternations_and_chains_finish_within_the_budget` が、strip・snapshot-strip・write-filter の呼び出しだけで合計したスレッド CPU 時間が 10s 未満であることをアサートし、正しさに関するアサーションがすべて引き続き通る。（FR2、FR3）
- [ ] AC4: ヘルパーのテストが通る: スリープは数えない、別スレッドの CPU は数えない、計測スレッド自身の CPU 処理は数える。wall-clock 時間を計測するヘルパーはスリープのケースで失敗する。（FR3、FR6、FR7）
- [ ] AC5: 対照テストが通る: 短い予算での FR2 の予算判定が 2 乗の代替負荷を不合格にする。（FR8）
- [ ] AC6: クロック API が失敗を報告したときヘルパーは panic し、ゼロや wall-clock へのフォールバック経路を持たない。コードレビューで確認する。（FR4）
- [ ] AC7: `src-tauri/Cargo.toml` で `windows-sys` の `Win32_System_Threading` が有効になっており、`--lib` ビルドがコンパイルできる。Windows クロスビルドは利用可能な環境でコンパイルできる。（FR5、NFR2）
- [ ] AC8: 名指しの 2 テスト以外の wall-clock による予算判定が変更されていない。（FR9）

### 11.2 KPI
該当なし。

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] 正常系: TS-1 ヘルパーはスリープを無視する / TS-2 ヘルパーは別スレッドの CPU を無視する / TS-3 ヘルパーは自スレッドを数える / TS-4 予算判定が 2 乗の代替負荷を不合格にする / TS-5 時間判定のない token-corpus テスト / TS-6 CPU 予算での alternations テスト
- [ ] 異常系: クロック API の失敗（`clock_gettime` が非ゼロ、または `GetThreadTimes` が `FALSE`）で panic し、ゼロとして報告しない（EC3、AC6 のコードレビュー）
- [ ] 境界値: クロック分解能。`GetThreadTimes` は 100 ns 単位でスケジューラティック粒度を持つ。ヘルパーのテストは、粒度が結果を左右しない長さの時間を使う（EC4）
- [ ] セキュリティ: 該当なし
- [ ] パフォーマンス: TS-7 合成負荷下での `--lib` 実行 / TS-8 スコープ外の予算判定が変更されていないこと

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| スレッド CPU 時間 | 呼び出し元スレッドだけが消費した CPU 時間。Linux では `clock_gettime(CLOCK_THREAD_CPUTIME_ID)`、Windows では `GetThreadTimes(GetCurrentThread())` のカーネル時間とユーザー時間の合計 |
| `BUDGET` | 10s の予算 |
| 計測対象の呼び出し | `strip_pty_output_for_scrollback_write`、`strip_replayable_rich_content`、`ScrollbackWriteFilter::feed` の呼び出し |

## 14. 確認事項

### 14.1 確認済み事項

- [x] デザインステップ: スキップ。UI・表示出力のない、テストのみの変更。バッチ回答 `design-step.decision` は `decide_autonomously` で、スキップの推奨を採用した。

### 14.2 未確認・保留事項
なし。

### 14.3 前提

| ID | 前提 | 根拠 | 影響度 | 可逆 |
|----|------|------|--------|------|
| A1 | 失敗の原因は CPU 競合下での wall-clock 時間の膨張であり、テスト対象コードの性能退行ではない | task_description: ベースの main `0c3a2c06` も同じように失敗する。負荷のない状態での debug テスト全体はオーケストレーターの計測で 3.71s wall、round4_chain の corpus テストは 2.60s | 中 | はい |
| A2 | 両テストの正しさに関するアサーションはすべて変更しない | タスクは負荷耐性のみを対象とする | 高 | はい |
| A3 | スレッド CPU 時間も競合下で（周波数スケーリング・キャッシュ・SMT により）ある程度増えるため、負荷から完全には独立しない。負荷のない状態での計測値（テスト全体で約 3.7s、計測対象の呼び出しだけならそれ未満）は 10s の予算に対して余裕がある | バッチ解決に従い残存リスクとして記録 | 中 | はい |
| A4 | `round4_chain::BUDGET` と `strip_concat_query::BUDGET` は残す。それぞれのモジュール内の他のテストが引き続き使っている（`round4_chain.rs:735, 852, 1180`、`strip_concat_query.rs:906, 1123, 1162`） | スコープは名指しの 2 テストに限定 | 低 | はい |

### 14.4 エッジケース

- EC1: スレッド CPU 時間は、競合下で周波数スケーリング・キャッシュ・SMT によりある程度増える。
- EC2: libtest はテストを並列スレッドで実行する。プロセス CPU 時間には他のテストが含まれるため、スレッド単位の時間だけを使う。
- EC3: クロック API の失敗（`clock_gettime` が非ゼロを返す、または `GetThreadTimes` が `FALSE` を返す）は panic し、ゼロとして報告しない。
- EC4: クロック分解能: `GetThreadTimes` は 100 ns 単位でスケジューラティック粒度を持つ。ヘルパーのテストは、粒度が結果を左右しない長さの時間を使う。
- EC5: 計測対象の呼び出しには、1 回呼び出しの feed（オーバーフローのフラッシュ）、アラインされた 64 KiB 読み込み、アラインされていない 64 KiB 読み込み、16 KiB の ESC チェーン読み込みが含まれる。出力に対するテスト側の `extend_from_slice` は除外する。

## 15. 参考資料

- 該当なし。
