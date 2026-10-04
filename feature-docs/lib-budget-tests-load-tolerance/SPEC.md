# Feature: lib-budget-tests-load-tolerance

## 概要

`--lib` の全体実行で、`round4_chain::the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` と `strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget` が環境負荷（load average 10-20）を原因として失敗しないようにする。前者からは wall-clock の経過時間判定を外し、後者の予算判定はテスト対象コードの呼び出しだけのスレッド CPU 時間で行う。要件の詳細は `feature-docs/lib-budget-tests-load-tolerance/REQUIREMENTS.md` を参照。

## 目的

- `--lib` の全体実行で、`round4_chain::the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` と `strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget` が環境負荷（load average 10-20）を原因として失敗しない。
- `strip_concat_query` AC-7（NFR1、NFR2、TS-9）の線形時間ガードを、テスト対象コードの CPU 時間だけを計測する形で維持する。

## ユーザーストーリー

該当なし（UI・表示出力のない、テストのみの変更）。受け入れ基準は「成功基準」に記載する。

## 技術要件

### 機能要件
- **FR1:** token-corpus テストの wall-clock 判定の削除。`round4_chain::the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` は経過時間の判定を持たなくなる。`let start = Instant::now()` と `assert!(start.elapsed() < BUDGET, ...)` を削除する。その他のアサーションはすべて変更しない。
- **FR2:** テスト対象コードに限定したスレッド CPU 時間の予算。`strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget` は、`strip_pty_output_for_scrollback_write`、`strip_replayable_rich_content`、`ScrollbackWriteFilter::feed` の呼び出しだけのスレッド CPU 時間を合計し、その合計が 10s の `BUDGET` 未満であることをアサートする。合計から除外するもの: 入力と期待値の生成（`repeated`、`concat`）、等値比較、フィルタ出力のテスト側での連結。テストの正しさに関するアサーションは変更しない。
- **FR3:** スレッド CPU 時間ヘルパー。テスト側のヘルパーが、呼び出し元スレッドだけが消費した CPU 時間を計測する。Linux では `libc` 経由で `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` を使う。Windows では `windows-sys` 経由で `GetThreadTimes(GetCurrentThread())`（カーネル時間とユーザー時間の合計）を使う。
- **FR4:** クロック失敗を隠さない。クロック API が失敗を報告したとき、ヘルパーは失敗内容を示すメッセージで panic する。ゼロを返すことも、wall-clock 時間にフォールバックすることもしない。
- **FR5:** Windows 依存の feature。`src-tauri/Cargo.toml`（`[target.'cfg(windows)'.dependencies]`）の既存の `windows-sys` 依存に `Win32_System_Threading` feature を追加する。新しい crate は追加しない。
- **FR6:** 再発検知: ヘルパーは CPU 外の時間を数えない。計測スレッドがスリープしている時間と、計測スレッドが待っている間に別スレッドが消費した CPU 時間を、ヘルパーが数えないことをテストで示す。
- **FR7:** 再発検知: ヘルパーは自スレッドを数える。計測スレッド上で行った CPU 処理が数えられることをテストで示す（計測値が正であり、処理量に応じて増える）。
- **FR8:** 再発検知: 2 乗の対照。短い予算を持つ通常の `#[test]` で、FR2 と同じ予算判定が 2 乗の代替負荷を不合格にすることを示す。
- **FR9:** スコープ。変更するのは名指しの 2 テストのみ。その他の wall-clock による予算判定はすべて現状のまま残す: `round4_chain.rs` と `strip_concat_query.rs` の他の `BUDGET` 使用箇所、`strip_join_escape_closure`、`strip_open_string_body_closure`、`round4_as05`、`tests.rs`。

### 非機能要件
- **NFR1 - 依存:** テストフレームワークの crate を追加しない。std と既存の `libc`（unix）、`windows-sys`（Windows）依存を使う。
- **NFR2 - 互換性:** テストは Linux と Windows でコンパイル・実行できる。`--no-default-features` のチェックビルドも引き続きコンパイルできる。
- **NFR3 - 計測の単位:** 計測はスレッド単位で行う。libtest で並列に走る他のテストは計測に含まれない。
- **NFR4 - 検出能力:** 計測対象の呼び出しに 100k 回繰り返しで 2 乗の回帰が入った場合、10s の CPU 予算を超える。参考値として、負荷のない状態での debug テスト全体はオーケストレーターの計測で 3.71s wall / 3.68s user。

## 実装方針

### アーキテクチャ

**コンポーネント:**
```
スレッド CPU 時間ヘルパー（テスト側、FR3/FR4）
  ├─ Linux:   libc::clock_gettime(CLOCK_THREAD_CPUTIME_ID)
  └─ Windows: windows-sys GetThreadTimes(GetCurrentThread())  カーネル時間 + ユーザー時間

利用側
  ├─ strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget（FR2）
  ├─ ヘルパーのテスト（FR6/FR7）
  └─ 2 乗の対照テスト（FR8）
```

### データフロー

FR2 の計測は、計測対象の呼び出しの前後でスレッド CPU 時間を読み、その差分を合計する。

```
入力・期待値の生成（repeated、concat）         … 計測に含めない
  → スレッド CPU 時間を読む
  → 計測対象の呼び出し
      strip_pty_output_for_scrollback_write
      strip_replayable_rich_content
      ScrollbackWriteFilter::feed
  → スレッド CPU 時間を読み、差分を合計に加える
  → 出力の連結（extend_from_slice）・等値比較  … 計測に含めない
最後に: 合計 < BUDGET（10s）をアサートする
```

計測対象の呼び出しには、1 回呼び出しの feed（オーバーフローのフラッシュ）、アラインされた 64 KiB 読み込み、アラインされていない 64 KiB 読み込み、16 KiB の ESC チェーン読み込みが含まれる（EC5）。

### API 設計

該当なし。

### データベーススキーマ

該当なし。

### 依存関係

**内部依存:**
- `strip_pty_output_for_scrollback_write`: FR2 の計測対象
- `strip_replayable_rich_content`: FR2 の計測対象
- `ScrollbackWriteFilter::feed`: FR2 の計測対象

**外部依存:**
- `libc`（unix、既存）: `clock_gettime(CLOCK_THREAD_CPUTIME_ID)`
- `windows-sys`（Windows、既存）: `GetThreadTimes`、`GetCurrentThread`。`Win32_System_Threading` feature を追加する（FR5）

### ファイル構成

```
src-tauri/
├── Cargo.toml              # windows-sys に Win32_System_Threading feature を追加（FR5）
└── （pty_spawn のテスト）
    ├── round4_chain.rs         # FR1
    ├── strip_concat_query.rs   # FR2
    └── ヘルパーモジュール      # FR3/FR4/FR6/FR7/FR8
```

## 宣言された変更集合

このセクションは手書きの一覧ではなく、create-plan での導出方法を示す。上記のフィーチャー固有のパスは、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、上記のフィーチャー固有のパスに加えて、ワークフローが生成する次の 2 つをデフォルトで宣言する:

- `feature-docs/lib-budget-tests-load-tolerance/**`
- `test-docs/lib-budget-tests-load-tolerance/**`

`feature-docs/lib-budget-tests-load-tolerance/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` であり、このセクションは引用のみでルールを再掲しない。

`test-docs/lib-budget-tests-load-tolerance/**` に含まれるもの: タスクごとのテスト記録 `test-docs/lib-budget-tests-load-tolerance/{T}.tests.yaml`。生成主体は `implement-phase.md` であり、このセクションは引用のみでルールを再掲しない。

この 2 つのデフォルトのメンバーは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載がないことを除外とはみなさない。除外は意図的・明示的な絞り込みである。

この宣言はスーパーセット（superset）の主張であり、検証時に観測される実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。一致する必要はない。implement タスクを 1 つも生成しないフィーチャーは `test-docs/lib-budget-tests-load-tolerance/` ディレクトリを生成しないが、宣言された `test-docs/lib-budget-tests-load-tolerance/**` は依然として正しい。宣言されたパスが生成されなくても違反にはならない。

## テストシナリオ

### ユニットテスト
- [ ] TS-1: ヘルパーはスリープを無視する。スレッド CPU 時間を読み、スリープ（約 200-500 ms）し、再び読む - CPU 時間の差分はスリープ時間よりはるかに小さい（その半分を大きく下回る）（AC4）
- [ ] TS-2: ヘルパーは別スレッドの CPU を無視する。生成したスレッドが一定時間ビジースピンし、テストスレッドが join で待つ間、テストスレッドで計測する - 計測スレッドの CPU 時間の差分は、別スレッドのスピン時間よりはるかに小さい（AC4）
- [ ] TS-3: ヘルパーは自スレッドを数える。計測スレッド上で一定量の処理を行う（例: `black_box` ループ） - CPU 時間の差分は正であり、処理の wall 時間の所定の割合以上である（AC4）
- [ ] TS-4: 予算判定が 2 乗の代替負荷を不合格にする。FR2 と同じ予算判定を短い予算で、線形処理なら予算に収まるサイズの O(n^2) の代替負荷に適用する - 判定は予算超過を報告する。同じサイズの線形の代替負荷は予算内に収まる（AC5）
- [ ] TS-5: 時間判定のない token-corpus テスト。`round4_chain::the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` を実行する - テスト本体に `Instant` / `BUDGET` の使用がなく、テストが通る（AC2）
- [ ] TS-6: CPU 予算での alternations テスト。`strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget` を実行する - テストが通る。計測対象の呼び出しの CPU 時間の合計は 10s 未満で、正しさに関するアサーションはすべて成り立つ（AC3）

### 結合テスト

該当なし。

### E2E テスト
**既存の E2E テスト**: なし
**実行コマンド**: 未検出

### エッジケース
- [ ] EC1: スレッド CPU 時間は、競合下で周波数スケーリング・キャッシュ・SMT によりある程度増える。
- [ ] EC2: libtest はテストを並列スレッドで実行する。プロセス CPU 時間には他のテストが含まれるため、スレッド単位の時間だけを使う。
- [ ] EC3: クロック API の失敗（`clock_gettime` が非ゼロを返す、または `GetThreadTimes` が `FALSE` を返す）は panic し、ゼロとして報告しない。
- [ ] EC4: クロック分解能: `GetThreadTimes` は 100 ns 単位でスケジューラティック粒度を持つ。ヘルパーのテストは、粒度が結果を左右しない長さの時間を使う。
- [ ] EC5: 計測対象の呼び出しには、1 回呼び出しの feed（オーバーフローのフラッシュ）、アラインされた 64 KiB 読み込み、アラインされていない 64 KiB 読み込み、16 KiB の ESC チェーン読み込みが含まれる。出力に対するテスト側の `extend_from_slice` は除外する。

### 負荷テスト
- [ ] TS-7: 合成負荷下での `--lib`。CPU バウンドのプロセスをバックグラウンドで起動し（`run_in_background`）、`--lib` コマンドを実行し、`TaskStop` で負荷を停止する - 名指しの 2 テストのどちらも失敗しない（AC1）

### 差分確認
- [ ] TS-8: スコープが変わっていない。pty_spawn のテストにある他の `BUDGET` と経過時間のアサートを差分で確認する - ヘルパーモジュールを除き、名指しの 2 テスト以外に変更がない（AC8）

## セキュリティ上の考慮事項

該当なし。

## エラー処理

### エラー一覧

| 条件 | 対応 |
|------|------|
| `clock_gettime` が非ゼロを返す（Linux） | 失敗内容を示すメッセージで panic する。ゼロを返さず、wall-clock 時間にフォールバックしない（FR4、EC3） |
| `GetThreadTimes` が `FALSE` を返す（Windows） | 失敗内容を示すメッセージで panic する。ゼロを返さず、wall-clock 時間にフォールバックしない（FR4、EC3） |

### エラーフロー

```
クロック API の呼び出し → 失敗を報告 → 失敗内容を示すメッセージで panic
```

## パフォーマンス

### 性能目標
- `strip_concat_alternations_and_chains_finish_within_the_budget` の計測対象の呼び出しのスレッド CPU 時間の合計が 10s（`BUDGET`）未満（FR2）。
- 計測対象の呼び出しに 100k 回繰り返しで 2 乗の回帰が入った場合、10s の CPU 予算を超える（NFR4）。

## 前提

- A1: 失敗の原因は CPU 競合下での wall-clock 時間の膨張であり、テスト対象コードの性能退行ではない。
- A2: 両テストの正しさに関するアサーションはすべて変更しない。
- A3: スレッド CPU 時間も競合下で（周波数スケーリング・キャッシュ・SMT により）ある程度増えるため、負荷から完全には独立しない。負荷のない状態での計測値（テスト全体で約 3.7s、計測対象の呼び出しだけならそれ未満）は 10s の予算に対して余裕がある。
- A4: `round4_chain::BUDGET` と `strip_concat_query::BUDGET` は残す。それぞれのモジュール内の他のテストが引き続き使っている（`round4_chain.rs:735, 852, 1180`、`strip_concat_query.rs:906, 1123, 1162`）。

## 成功基準

- [ ] すべての機能要件が実装され、テストされている
- [ ] すべてのテストシナリオが通る
- [ ] コードレビューが完了している
- [ ] AC1: 合成 CPU 負荷の下で、`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が名指しの 2 テストを通す。負荷は `run_in_background` で起動し `TaskStop` で停止する CPU バウンドのプロセスで与え、load average 10-20 相当の水準とする。（FR1、FR2）
- [ ] AC2: `the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` が経過時間のアサーションを持たず、リプレイ一致のアサーションがすべて引き続き通る。（FR1）
- [ ] AC3: `strip_concat_alternations_and_chains_finish_within_the_budget` が、strip・snapshot-strip・write-filter の呼び出しだけで合計したスレッド CPU 時間が 10s 未満であることをアサートし、正しさに関するアサーションがすべて引き続き通る。（FR2、FR3）
- [ ] AC4: ヘルパーのテストが通る: スリープは数えない、別スレッドの CPU は数えない、計測スレッド自身の CPU 処理は数える。wall-clock 時間を計測するヘルパーはスリープのケースで失敗する。（FR3、FR6、FR7）
- [ ] AC5: 対照テストが通る: 短い予算での FR2 の予算判定が 2 乗の代替負荷を不合格にする。（FR8）
- [ ] AC6: クロック API が失敗を報告したときヘルパーは panic し、ゼロや wall-clock へのフォールバック経路を持たない。コードレビューで確認する。（FR4）
- [ ] AC7: `src-tauri/Cargo.toml` で `windows-sys` の `Win32_System_Threading` が有効になっており、`--lib` ビルドがコンパイルできる。Windows クロスビルドは利用可能な環境でコンパイルできる。（FR5、NFR2）
- [ ] AC8: 名指しの 2 テスト以外の wall-clock による予算判定が変更されていない。（FR9）

## 未解決事項

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## 参考資料

- 要件定義書: `feature-docs/lib-budget-tests-load-tolerance/REQUIREMENTS.md`
