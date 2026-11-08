# Feature: mux-strip-concat-budget-overrun

## Overview

`strip_concat_alternations_and_chains_finish_within_the_budget`（以下、対象テスト）を debug ビルドで 10s の `BUDGET` 内に安定して収める。まず計測対象の入口ごと・形ごとのスレッド CPU 時間を計測し、コストが線形か非線形かを判定する。判定に応じて対象テストの処理量を減らすか本番コードの非線形経路を直し、N と 4N の CPU 時間比による回帰ガードテストを追加する。要件の詳細は `feature-docs/mux-strip-concat-budget-overrun/REQUIREMENTS.md` を参照する。

## Scope

### In Scope

- 対象テストの計測（FR1）と予算内化（FR2）。
- スケーリング比による回帰ガードテストの追加（FR3）。

### Out of Scope

次の並列実行時のみの失敗は対象外とし、別タスクで扱う（A2）。

- `escape_state_carry::escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget`（壁時計 `Instant` 予算）
- `post_strip_cut_csi::post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget`（壁時計 `Instant` 予算）
- `strip_open_string_body_closure::open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal`（壁時計 `Instant` 予算）
- `visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`（5s `recv_timeout`）

## Objectives

- 対象テストが debug ビルドで、単独実行（`--test-threads=1`）でも `mux::ipc::pty_spawn::tests` の直列実行でも安定して成功する。
- 対象テストはスレッド CPU 時間で予算を判定し、判定に CPU 負荷を必要としない。
- 処理量やコード経路が予算を超えて増えたとき、または非線形になったときに、テストが再発を検出する。

## User Stories

### US1: 対象テストが予算内で成功する
開発者として、対象テストを debug ビルドで単独でも直列でも実行したい。それにより、テストが安定して成功する。

**Acceptance Criteria:**
- [ ] AC-1: 再現コマンド `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib strip_concat_alternations_and_chains_finish_within_the_budget -- --test-threads=1` が debug ビルドで成功する。（FR2）
- [ ] AC-2: 対象テストの計測合計が、開発機の debug シングルスレッド実行で `BUDGET` の半分（5s）以下である。`BUDGET` は 10s のまま。（FR2）
- [ ] AC-3: 対象テストが、8 個の `TARGETS` × 2 つの形について、2 つの strip と 3 つの feed モードで書き出し結果をアサートし、ESC チェーンについてもアサートする。すべてのターゲット × 形の 1 回呼び出し入力が `SCROLLBACK_FILTER_PENDING_CAP` より大きい。`TARGETS`、`Target`、`BUDGET` は変更されていない。（FR2）
- [ ] AC-6: `mux::ipc::pty_spawn::tests` を直列実行（`--test-threads=1`）すると、対象テストと新しいテストが成功する。（FR2、FR3）

### US2: 支配的なコストが記録されている
開発者として、どの入口・形がコストを占めるかと、線形か非線形かの判定を確認したい。それにより、FR2 の分岐が判定に基づいて決まる。

**Acceptance Criteria:**
- [ ] AC-4: FR1 の内訳（入口ごと・形ごと）と線形 / 非線形の判定がフィーチャードキュメントに記録されている。（FR1）

### US3: 非線形化をテストが検出する
開発者として、strip と write filter が非線形になったときにテストで検出したい。それにより、再発が分かる。

**Acceptance Criteria:**
- [ ] AC-5: スケーリング比テストが実際の strip と write-filter の経路で成功する。比の判定は二次の代用処理を不合格にし、線形の代用処理を合格にする。（FR3）

## Technical Requirements

### Functional Requirements
- **FR1:** 支配的なコストの特定。計測対象の各入口（`strip_pty_output_for_scrollback_write`、`strip_replayable_rich_content`、`ScrollbackWriteFilter::feed` の 1 回呼び出し / 単位揃え読み / 64 KiB 読み、ESC チェーンの呼び出し）のスレッド CPU 時間を、ターゲットごと・形（開いた CSI、単独 ESC）ごとに計測する。計測は対象テストのシングルスレッド実行だけで行い、`CpuMeter` を使う。同じ形で 2 種類の入力サイズを計測し、コストが線形か非線形かを判定する。内訳と判定をフィーチャードキュメントに記録する。この判定が FR2 の分岐を決める。
- **FR2:** 対象テストを予算内に収める。
    - 線形の判定: 対象テストの処理量を減らし、計測合計が変更しない 10s の `BUDGET` に対して十分な余裕を持つようにする（AC-2）。次をすべて維持する: `TARGETS` の全エントリ、両方の形、3 種の feed モード（1 回呼び出し / 単位揃え読み / 64 KiB 読み）、両方の strip 入口、ESC チェーン部分、すべての出力アサーション。すべてのターゲット × 形の 1 回呼び出し入力を `SCROLLBACK_FILTER_PENDING_CAP` より大きくし、overflow flush を通す。他の経路は同じ形の小さい入力を使ってよい。共有の `TARGETS` / `Target` と共有の `BUDGET` は変更しない。
    - 非線形の判定: 本番コード（`strip_pass` / `ScrollbackWriteFilter`）の非線形経路を直し、処理量は変更せず、本番の出力をバイト単位で同一に保つ。
- **FR3:** スケーリング比による回帰ガード。実際の `strip_pty_output_for_scrollback_write` と `ScrollbackWriteFilter::feed`（1 回呼び出し、cap 超え）を、同じ交互の形で N と 4N の入力で動かすテストを追加する。それぞれを `CpuMeter` で計測し、CPU 時間比で線形か二次かを判定する。比が 8 未満なら合格とする（8 は線形の 4 と二次の 16 の幾何平均）。N は、debug ビルドで各計測が約 100 ms 以上のスレッド CPU 時間になる値にする（Windows `GetThreadTimes` の分解能）。比の判定そのものを、線形の代用処理と二次の代用処理で単体テストする（`thread_cpu_time.rs` の TS-4 と同じ方式）。

### Non-Functional Requirements
- **NFR1 - CPU 時間による予算判定:** 予算はスレッド CPU 時間で `CpuMeter` を通して判定する。壁時計の予算は追加しない。
- **NFR2 - 意図的な CPU 負荷の禁止:** 調査・実装・検証で意図的に CPU 負荷をかけない。負荷ループ、並列の負荷実行、大量の繰り返しを行わない。個々のテストのシングルスレッド実行だけを行う。
- **NFR3 - 本番出力の同一性:** strip と write filter の本番出力はバイト単位で同一に保つ。性能の変更は FR2 の非線形分岐の場合に限る。
- **NFR4 - プラットフォーム:** Linux と Windows。テストは両方のクロック（`CLOCK_THREAD_CPUTIME_ID` / `GetThreadTimes`）でコンパイルでき、正しく判定する。

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────┐
│ 対象テスト / スケーリング比テスト（テスト側） │
├──────────────────────────────────────────────┤
│ CpuMeter（thread_cpu_time.rs）               │
│   Linux: CLOCK_THREAD_CPUTIME_ID             │
│   Windows: GetThreadTimes                    │
├──────────────────────────────────────────────┤
│ 本番コード（計測対象）                        │
│   strip_pty_output_for_scrollback_write      │
│   strip_replayable_rich_content              │
│   ScrollbackWriteFilter::feed                │
│   strip_pass                                 │
└──────────────────────────────────────────────┘
```

**Component Diagram:**
```
対象テスト ──計測──> CpuMeter
    │
    ├─> strip_pty_output_for_scrollback_write
    ├─> strip_replayable_rich_content
    ├─> ScrollbackWriteFilter::feed（1 回呼び出し / 単位揃え読み / 64 KiB 読み）
    └─> ESC チェーンの呼び出し

スケーリング比テスト ──N と 4N を計測──> CpuMeter
    ├─> strip_pty_output_for_scrollback_write
    └─> ScrollbackWriteFilter::feed（1 回呼び出し、cap 超え）
```

### Data Flow

```
FR1: 入口ごと・形ごとの CPU 時間計測 → 2 つの入力サイズで線形 / 非線形を判定 → フィーチャードキュメントに記録
FR2: 判定が線形   → 対象テストの繰り返し回数と入力サイズを減らす（カバレッジ維持）
     判定が非線形 → strip_pass / ScrollbackWriteFilter の非線形経路を直す（処理量維持・出力同一）
FR3: N と 4N の CPU 時間比 → 8 未満なら合格
```

### Workload Change Record

線形の判定で処理量を減らす場合、対象テストの処理量は、対象テストの doc comment が mux-strip-concat-query-closure AC-7 について挙げる 100k 規模（`strip_concat_query.rs:1010-1013`）から外れる。本 SPEC はこの変更を記録する。対象テストの doc comment は新しいサイズに更新する。対象テストの名前は変えないため、先行フィーチャーの test-docs 記録の改名更新は不要である（`.claude/rules/test-docs-records.md`）（A8）。

### API Design

該当なし。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- `CpuMeter`（`thread_cpu_time.rs`）: CPU 時間の計測。シングルスレッドで負荷に依存しない設計。新しいヘルパーを作る場合は、テストツリー内で `CpuMeter` の隣に置く（A9）。
- 共有の `TARGETS` / `Target`: `strip_open_string_body_closure.rs:41` からも import されている。変更しない（A7）。
- 共有の `BUDGET`: `strip_concat_query.rs:907`、`1131`、`1170` でも使われている。変更しない（A7）。
- `SCROLLBACK_FILTER_PENDING_CAP`: 1 回呼び出し入力のサイズ下限の基準。

**External Dependencies:**
- 該当なし。

### File Structure

フィーチャー固有のファイルは create-plan で導出する。要件から参照されるファイルは次のとおり。

```
strip_concat_query.rs              # 対象テスト、BUDGET
strip_open_string_body_closure.rs  # TARGETS / Target を import
thread_cpu_time.rs                 # CpuMeter、TS-4 の比判定テスト
```

## Declared Change Set

このセクションは手動のリストではなく create-plan での導出を述べる。フィーチャー固有のパスは、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、フィーチャー固有のパスに加えて、次の 2 つのワークフロー生成エントリをデフォルトで宣言する。

- `feature-docs/mux-strip-concat-budget-overrun/**`
- `test-docs/mux-strip-concat-budget-overrun/**`

`feature-docs/mux-strip-concat-budget-overrun/**` は `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物を含む。これらは各フェーズドキュメントと `references/phase-state.md` が生成・管理する。このセクションはそれらを引用するだけで、ルールは再掲しない。

`test-docs/mux-strip-concat-budget-overrun/**` はタスクごとのテスト記録 `test-docs/mux-strip-concat-budget-overrun/{T}.tests.yaml` を含む。これは `implement-phase.md` が生成・管理する。このセクションはそれを引用するだけで、ルールは再掲しない。

この 2 つのデフォルトエントリは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載がないことを除外とはみなさない。除外は意図的で明示的な絞り込みである。

この宣言はスーパーセットの主張である。検証時に観測した実際の変更集合は、宣言集合と等しい必要はなく、宣言集合に含まれる（CONTAINED IN）必要がある。implement タスクを 1 つも生成しないフィーチャーは `test-docs/mux-strip-concat-budget-overrun/` ディレクトリを生成しないが、その場合も宣言した `test-docs/mux-strip-concat-budget-overrun/**` は正しい。宣言したパスが生成されなくても違反ではない。

## Test Scenarios

### Unit Tests
- [ ] TS-1: 再現コマンドをシングルスレッドで実行する - 成功し、判定メッセージの合計が 5s 以下である（AC-1、AC-2）
- [ ] TS-2: 対象テストの静的確認 - `TARGETS` の全エントリと両方の形を回している。各 1 回呼び出し入力の長さが `SCROLLBACK_FILTER_PENDING_CAP` より大きい（テスト内でアサート）。単位揃え読みと 64 KiB 読みのループ、ESC チェーンのブロックがある。`TARGETS` / `Target` / `BUDGET` が main に対して変更されていない（AC-3）
- [ ] TS-3: `strip_pty_output_for_scrollback_write` と `ScrollbackWriteFilter::feed` の N と 4N でのスケーリング比テスト - 比が 8 未満である（AC-5）
- [ ] TS-4: 線形の代用処理と二次の代用処理による比の判定 - 線形は合格、二次は不合格になる（AC-5）

### Integration Tests
- [ ] TS-5: `mux::ipc::pty_spawn::tests` を `--test-threads=1` で実行する - 対象テストと新しいテストが成功する（AC-6）

### Documentation Review
- [ ] TS-6: フィーチャードキュメントの確認 - FR1 の内訳と判定がある（AC-4）

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- 「単独 ESC + csi query」の 1 回呼び出し入力は現状 500,000 バイトで、`SCROLLBACK_FILTER_PENDING_CAP`（524,288）を下回る。FR2 で cap より大きくし、TS-2 で確認する。
- Windows のクロック分解能。FR3 の各計測を約 100 ms 以上の CPU 時間にする。

### Performance Tests
- 負荷テスト・ストレステストは行わない（NFR2）。性能の判定は TS-1 と TS-3 で行う。

## Security Considerations

該当なし。

## Error Handling

該当なし。

## Performance Optimization

### Performance Goals
- 対象テストの計測合計: 開発機の debug シングルスレッド実行で 5s 以下（`BUDGET` 10s の半分）
- N と 4N の CPU 時間比: 8 未満
- FR3 の各計測: debug ビルドで約 100 ms 以上のスレッド CPU 時間

### Optimization Strategies
- 線形の判定: 対象テスト自身の繰り返し回数と入力サイズだけを減らす（A7）。
- 非線形の判定: `strip_pass` / `ScrollbackWriteFilter` の非線形経路を直す。出力はバイト単位で同一に保つ。

### Caching Strategy
- 該当なし。

## Assumptions

- A1: q-fix-approach = investigate_then_branch（batch）。入口ごと・形ごとにシングルスレッドで計測する。線形なら処理量を減らしカバレッジを維持する。非線形なら本番コードを直し処理量を維持する。
- A2: q-parallel-scope = target_only（batch）。Out of Scope に列挙した並列実行時のみの失敗は別タスクで扱う。
- A3: q-regression-guard = both（batch）。スケーリング比テスト（FR3）と、十分な余裕を持たせた絶対値 10s の予算（AC-2）の両方を使う。
- A4: q-design-step = decide_autonomously（batch）。デザインステップは省略する。
- A5: 「十分な余裕」は、debug シングルスレッド実行での計測合計が `BUDGET` の半分（5s）以下を指す。
- A6: 比のしきい値は 8（N と 4N での線形 4 と二次 16 の幾何平均）。各計測は約 100 ms 以上の CPU 時間にし、Windows のクロック分解能が支配しないようにする。
- A7: 共有の `TARGETS` / `Target`（`strip_open_string_body_closure.rs:41` からも import）と共有の `BUDGET`（`strip_concat_query.rs:907`、`1131`、`1170` でも使用）は変更しない。処理量の削減は対象テスト自身の繰り返し回数と入力サイズだけで行う。
- A8: 対象テストの名前は変えないため、先行フィーチャーの test-docs 記録の改名更新は不要。処理量の変更は mux-strip-concat-query-closure AC-7 の 100k 規模から外れる。本 SPEC がこの変更を記録し、テストの doc comment を新しいサイズに更新する。
- A9: CPU 時間の計測は `CpuMeter` のまま行う。新しいヘルパーを作る場合は、テストツリー内で `CpuMeter` の隣に置く。

## Success Criteria

- [ ] すべての機能要件が実装・テストされている
- [ ] すべてのテストシナリオが成功する
- [ ] Performance Goals を満たす
- [ ] FR1 の内訳と判定がフィーチャードキュメントに記録されている
- [ ] コードレビューが完了している

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## Implementation Phases

### Phase 1: 支配的なコストの特定
**Goals:** FR1 の内訳と線形 / 非線形の判定を得る。
**Deliverables:**
- 入口ごと・形ごとの CPU 時間の内訳と判定（フィーチャードキュメント）

### Phase 2: 対象テストを予算内に収める
**Goals:** FR1 の判定に応じた FR2 の分岐を実施する。
**Deliverables:**
- 線形: 処理量を減らした対象テストと、更新した doc comment
- 非線形: 本番コードの非線形経路の修正

### Phase 3: スケーリング比による回帰ガード
**Goals:** FR3 のテストを追加する。
**Deliverables:**
- 実経路のスケーリング比テスト
- 比の判定の単体テスト（線形 / 二次の代用処理）

## References

- 要件定義書: `feature-docs/mux-strip-concat-budget-overrun/REQUIREMENTS.md`
- test-docs 記録のルール: `.claude/rules/test-docs-records.md`
- `CpuMeter` と TS-4 の比判定テスト: `thread_cpu_time.rs`
- 対象テストと `BUDGET`: `strip_concat_query.rs`
