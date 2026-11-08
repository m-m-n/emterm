---
title: "pty-spawn-parallel-budget-flake"
created_date: 2026-11-08
status: draft
---

# pty-spawn-parallel-budget-flake - 要件定義書

## 1. 概要

### 1.1 背景
タスクで名指しされた `mux::ipc::pty_spawn` の 4 テストについて、並列の `cargo test --lib` 実行での合否がマシン負荷に左右されないようにする。対象は次の 4 つ。

- `escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget`
- `post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget`
- `open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal`
- `visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`

reattach テストの失敗は順序の競合で、予算の問題ではない（14.1 A5）。

### 1.2 目的
- 名指しの 4 テストが並列の `cargo test --lib` 実行で合格し、その合否がマシン負荷に依存しない。
- reattach テストの on-demand 版（`on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once`）からも同じ arm 順序の競合がなくなる。
- ソースを走査するガードテストが、壁時計による予算判定、または release が arm より先に来る順序への逆戻りを検出する。

### 1.3 スコープ
- 対象: 名指しの 4 テストと、同じ release→arm 順序をもつ `on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once`（`tests.rs:5406/5408`）。
- 対象: ソース走査ガードテストの追加。
- 対象外: 上記以外の壁時計チェック。変換しない。
- 変更範囲は `src-tauri/src/mux/ipc/pty_spawn/tests` 配下のテストコードのみ。

## 2. ビジネス要件

### 2.1 ビジネス目標
- 名指しの 4 つの `mux::ipc::pty_spawn` テストが並列の `cargo test --lib` 実行で合格し、合否がマシン負荷に依存しない。
- reattach テストの on-demand 版に、同じ arm 順序の競合が残らない。
- ソース走査ガードテストが、壁時計予算や release-before-arm 順序への逆戻りを検出する。

### 2.2 対象ユーザー
| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | `cargo test --lib` を実行する開発者 |

### 2.3 期待される効果
- 対象の 5 テストとガードが、通常の `--lib` 実行 1 回でも、直列の `pty_spawn::tests` 実行でも失敗しない。

## 3. ユースケース

### 3.1 ユースケース一覧
| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | `--lib` スイートの通常実行 | 開発者 | — |
| UC02 | `pty_spawn::tests` の直列実行 | 開発者 | — |

### 3.2 ユースケース詳細

#### UC01: `--lib` スイートの通常実行

**アクター**: 開発者

**基本フロー**:
1. `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` を 1 回実行する。

**事後条件**:
- 対象の 5 テストとガードがどれも失敗しない（AC-4）。

#### UC02: `pty_spawn::tests` の直列実行

**アクター**: 開発者

**基本フロー**:
1. `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::ipc::pty_spawn::tests -- --test-threads=1` を実行する。

**事後条件**:
- 対象の 5 テストとガードが合格する（AC-5）。

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | `escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget` の CPU 時間予算 | Instant 予算を CpuMeter に置き換える | — |
| FR2 | `post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget` の CPU 時間予算 | Instant 予算を CpuMeter に置き換える | — |
| FR3 | `open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal` の CPU 時間予算 | Instant 予算を CpuMeter に置き換える | — |
| FR4 | `visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once` の arm 順序 | 2 回目の P2 停止の arm を最初の release より前に移す | — |
| FR5 | `on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once` の arm 順序 | FR4 と同じ変更 | — |
| FR6 | ソース走査ガードテスト | 対象テストのソースを読み、逆戻りを検出する | — |

### 4.2 機能詳細

#### FR1: `escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget` の CPU 時間予算

**説明**: `escape_state_carry.rs` の Instant 予算（774 行の `Instant::now`、872 行の elapsed 判定）を `CpuMeter` に置き換える。メーターは本番の書き込みフィルタ呼び出し（`feed_pieces` 経由で到達する `ScrollbackWriteFilter::feed_with_cuts`）を計測し、その合計を共有の `BUDGET` に対して `CpuMeter::judge` で判定する。

**ビジネスルール**:
- テストは、判定結果が予算内であることを assert する。
- テストは、計測合計が 0 より大きいことを assert する。

#### FR2: `post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget` の CPU 時間予算

**説明**: `post_strip_cut_csi.rs` の Instant 予算（776 行 / 846 行）を `CpuMeter` に置き換える。メーターは本番の書き込みフィルタ呼び出し（`run_pieces` と `emitted_through` 経由で到達する `feed_with_cuts`）を計測し、その合計を `BUDGET` に対して判定する。

**ビジネスルール**:
- テストは、判定結果が予算内であることを assert する。
- テストは、計測合計が 0 より大きいことを assert する。

#### FR3: `open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal` の CPU 時間予算

**説明**: `strip_open_string_body_closure.rs` の Instant 予算（235 行 / 244 行）を `CpuMeter` に置き換える。メーターは `check_a_flushed_open_body_is_closed_at_the_next_removal` 内の本番呼び出し（`overflow()` と `feed_with_cuts`）を計測する。term_core オラクルのリプレイ（`assert_replays_like_raw`）は計測しない。合計を `BUDGET` に対して判定する。

**ビジネスルール**:
- テストは、判定結果が予算内であることを assert する。
- テストは、計測合計が 0 より大きいことを assert する。
- このヘルパーを共有する OSC 版テスト（219-228 行）は、自身の Instant 予算と動作をそのまま保つ。

#### FR4: `visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once` の arm 順序

**説明**: `tests.rs` で、2 回目の P2 停止の arm（`output_capture.p2.arm()`、現在 5142 行）を、最初の release（`release_tx.send`、現在 5137 行）より前、リーダーが最初の P2 で停止している間に行う。`tests.rs:1893-1894` のパターンに従う。

**ビジネスルール**:
- 5 秒の `recv_timeout` の値は変更しない。

#### FR5: `on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once` の arm 順序

**説明**: `tests.rs` で、2 回目の P2 停止の arm（現在 5408 行）を最初の release（現在 5406 行）より前に行う。FR4 と同じ変更。

**ビジネスルール**:
- 5 秒の `recv_timeout` の値は変更しない。

#### FR6: ソース走査ガードテスト

**説明**: 対象テストのソースを読み、次のいずれかの場合に失敗するテストを追加する。

- (a) FR1-FR3 の 3 テストのうち 1 つが名前で見つからない。
- (b) それらの本体のうち 1 つが、CpuMeter の判定ではなく `Instant::now` / `.elapsed()` で判定している。または `CpuMeter::new`、`.measure(`、`.judge(BUDGET)` のいずれかを欠く。
- (c) FR4/FR5 の 2 テストのうち 1 つが見つからない。または `p2.arm()` の呼び出しが 2 回未満。
- (d) その 2 テストのうち 1 つで、最初の `release_tx.send(` が 2 回目の `p2.arm()` より前にある。

**ビジネスルール**:
- 判定ロジックは、ソース文字列を受け取る純粋関数にする。
- 負例の単体テストを持つ。旧来の壁時計形式と release 先行の順序を拒否し、新しい形式を受理し、関数名の欠落を欠落として報告する。

## 5. 非機能要件

### 5.1 パフォーマンス要件
- NFR1: FR1-FR3 の予算は、CpuMeter を通じてスレッド CPU 時間として判定する。壁時計の予算は新たに追加しない。
- NFR2: 調査・実装・検証で意図的な CPU 負荷をかけない。負荷ループ、ストレスツール、繰り返し実行、コア数ぶんの並列実行を行わない。通常の実行を 1 回ずつのみ行う。

### 5.2 セキュリティ要件
該当なし

### 5.3 可用性要件
該当なし

### 5.4 保守性要件
- NFR3: 本番コード、テスト名、ワークロード、出力の assert、共有の `BUDGET` / `TARGETS` / `Target` は変更しない。FR1-FR3 以外の壁時計チェックは変更しない。

### 5.5 互換性要件
- NFR4: 変更したテストとガードは、Linux と Windows でコンパイルでき、判定できる。

## 6. UI/UX要件

該当なし（変更は `src-tauri/src/mux/ipc/pty_spawn/tests` 配下のテストコードのみで、UI や視覚的な出力はない）

## 7. データ要件

該当なし

## 8. 外部連携

該当なし

## 9. 制約条件

### 9.1 技術的制約
- 予算は本番の呼び出し（strip / 書き込みフィルタ / overflow）だけを計測する。入力の組み立て、assert、term_core オラクルのリプレイは計測しない。`BUDGET` は 10 秒のまま。
- 共有ヘルパー（`feed_pieces`、`run_pieces`、`check_a_flushed_open_body_is_closed_at_the_next_removal`）にメーター引数を追加する場合、他の呼び出し元の動作は保つ。具体的な計測の形は create-plan で決める。
- NFR2、NFR3 に従う。

### 9.2 ビジネス上の制約
該当なし

### 9.3 スケジュール制約
該当なし

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/pty-spawn-parallel-budget-flake/**`
- `test-docs/pty-spawn-parallel-budget-flake/**`

`feature-docs/pty-spawn-parallel-budget-flake/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/pty-spawn-parallel-budget-flake/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/pty-spawn-parallel-budget-flake/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/pty-spawn-parallel-budget-flake/` ディレクトリを生成しないが、宣言された `test-docs/pty-spawn-parallel-budget-flake/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題
| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| `judge` があるだけでは、対象の呼び出しが計測されたことを示せない | 中 | 変換した各テストで計測合計が 0 より大きいことを assert し、ガードで本体に `.measure(` があることを要求する |

### 10.2 ビジネスリスク
該当なし

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1: FR1-FR3 の 3 テストに `Instant::now` / `.elapsed()` がない。各テストは CpuMeter を通じて本番の呼び出しを計測し、合計を `BUDGET` に対して判定し、合計が 0 より大きいことを assert する。（FR1, FR2, FR3）
- [ ] AC-2: FR4/FR5 の 2 つの reattach テストの両方で、2 回目の `p2.arm()` が最初の `release_tx.send` より前にあり、`recv_timeout` の値は変わっていない。（FR4, FR5）
- [ ] AC-3: 変更後のソースに対してガードテストが合格する。負例の単体テストは、壁時計形式を拒否し、release 先行の順序を拒否し、関数名の欠落を報告する。（FR6）
- [ ] AC-4: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` の通常実行 1 回で、対象の 5 テストとガードのどれも失敗しない。（FR1-FR6）
- [ ] AC-5: 直列実行 `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::ipc::pty_spawn::tests -- --test-threads=1` で、対象の 5 テストとガードが合格する。（FR1-FR6）

### 11.2 KPI
該当なし（11.1 の受け入れ基準で判定する）

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] 正常系: TS-1（単体）FR1-FR3 の 3 テストを実行する。合格し、各合計が 0 より大きく `BUDGET` 未満である。（AC-1）
- [ ] 正常系: TS-2（単体）FR4/FR5 の 2 つの reattach テストを実行する。合格する。（AC-2）
- [ ] 正常系: TS-3（単体）現在のソースに対してガードテストを実行する。合格する。（AC-3）
- [ ] 異常系: TS-4（単体）ガードの負例（Instant 予算のスニペット、release 先行のスニペット、関数名の欠落）を実行する。それぞれ拒否または報告される。（AC-3）
- [ ] 正常系: TS-5（結合）`--lib` スイートを 1 回実行する。対象テストとガードのどれも失敗しない。（AC-4）
- [ ] 正常系: TS-6（結合）`pty_spawn::tests` の直列実行を行う。対象とガードが合格する。（AC-5）
- [ ] パフォーマンス: 負荷テスト・ストレステストは行わない（NFR2）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| CpuMeter | 計測した呼び出しのスレッド CPU 時間を合計し、`judge` で予算と照合するテスト用の計測器 |
| BUDGET | 予算判定で共有する予算値（10 秒） |
| P2 | reattach テストでリーダーを停止させる停止点。`output_capture.p2.arm()` で arm する |
| ガードテスト | 対象テストのソースを走査し、FR6 の (a)-(d) のいずれかで失敗するテスト |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1 q-parallel-scope（batch-codex-consultation）: target_and_same_race。範囲は名指しの 4 テストと、`on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once`（`tests.rs:5406/5408`）にある同じ release→arm 順序。他の壁時計チェックは変換しない。そのうちいくつかは他スレッドに対するデッドロック検出の期限（`tests.rs:2406`、`2735`）で、変換すると意味が変わる。
- [x] A2 q-regression-guard（batch-codex-consultation）: source_scan_guard。逆戻りは FR6 のソース走査ガードで検出する。ガードは対象関数の欠落で失敗し、負例を持つ。`judge` があるだけでは呼び出しが計測されたことを示せないため、変換した各テストは計測合計が 0 でないことも assert し、ガードは本体に `.measure(` があることを要求する。
- [x] A3 q-design-step（batch-decision-table）: skip。
- [x] A4: 予算は本番の呼び出し（strip / 書き込みフィルタ / overflow）だけを計測する。入力の組み立て、assert、term_core オラクルのリプレイは計測しない。`BUDGET` は 10 秒のまま。
- [x] A5: reattach の失敗は順序の競合で、リーダーが再 arm より前に 2 回目の P2 を通過する。予算の問題ではない。リーダーが `PauseHook::hit` でブロックしている間の arm は、次の hit にだけ影響する。
- [x] A6: テストの改名はないため、test-docs 記録の更新は不要（`.claude/rules/test-docs-records.md`）。
- [x] A7: 共有ヘルパー（`feed_pieces`、`run_pieces`、`check_a_flushed_open_body_is_closed_at_the_next_removal`）にメーター引数が加わる場合、他の呼び出し元は動作を保つ。具体的な計測の形は create-plan に委ねる。

### 14.2 未確認・保留事項
なし

## 15. 参考資料

- `.claude/rules/test-docs-records.md`: test-docs 記録の更新ルール（A6）
