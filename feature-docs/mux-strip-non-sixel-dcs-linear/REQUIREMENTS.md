---
title: "mux-strip-non-sixel-dcs-linear"
created_date: 2026-10-04
status: draft
---

# mux-strip-non-sixel-dcs-linear - 要件定義書

## 1. 概要

### 1.1 背景
タスクで指摘された二次時間の走査経路（`scrollback_filter.rs:669-683` の `find_st_terminator`）は、HEAD 時点では存在しない（前提 A1）。共有 strip は `scan_body_end`（`scrollback_filter.rs:875`）を使い、最初の ESC で停止する。非 SIXEL DCS のフォールバック（`scrollback_filter.rs:774-792`）は ESC を 1 バイト書き出し、次の走査は次の ESC で停止する。write filter の `find_st`（`write_filter.rs:1069`）と client-parity の `find_st_terminator`（`client_parity_scan.rs:405`）も最初の ESC で停止する。

### 1.2 目的
- 非 SIXEL DCS 導入子の繰り返し `(ESC P x)*N` の後に `ESC \` が続くストリームに対し、共有 strip・scrollback write filter・snapshot builder・client-parity scan が入力長に対して線形時間で完了する。
- この入力形に対する二次時間走査の再発を、write 経路（write filter と本番 reader。抑止配信時の scan を含む）と snapshot 経路で回帰テストが検出する。

### 1.3 スコープ
- 対象: 上記入力形に対する予算（budget）テストの追加（FR1〜FR6）、非線形が判明した場合に限る本番コード修正（FR7）、test-docs 記録（FR8）。
- write 経路のテスト対象は `ScrollbackWriteFilter` と本番 reader。`client_parity_scan::scan` は直接呼び出し（長い入力）と `run_reader_with_suppressed_reads` 経由（`suppressed_output.rs:246` / `:250` の両分岐）の両方で検証する。`run_reader_without_owner` は Detached であり `client_parity_scan` に到達しない（前提 A2）。

## 2. ビジネス要件

### 2.1 ビジネス目標
- 共有 strip・scrollback write filter・snapshot builder・client-parity scan が、`(ESC P x)*N` + `ESC \` のストリームに対して入力長に線形な時間で完了する。
- この入力形に対する二次時間走査の再発を、write 経路（write filter と本番 reader。抑止配信時の scan を含む）と snapshot 経路で回帰テストが検出する。

### 2.2 対象ユーザー
該当なし。

### 2.3 期待される効果
- 2.1 のとおり。

## 3. ユースケース

該当なし（UI を伴わない、mux daemon のバイトストリーム処理に対する回帰テストの追加）。

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | 共有 strip の予算テスト | 共有 strip の各エントリポイントを 2 MiB の入力で検証する | resolved |
| FR2 | write filter の予算テスト | `ScrollbackWriteFilter` を一括・分割・上限超過の 3 ケースで検証する | resolved |
| FR3 | snapshot 経路の予算テスト | `build_snapshot_bytes` / `build_resume_snapshot_bytes` を 2 MiB の scrollback で検証する | resolved |
| FR4 | 本番 reader（Detached）の予算テスト | `run_reader_without_owner` に入力を流して検証する | resolved |
| FR5 | client-parity scan の予算テスト | `client_parity_scan::scan` を直接呼び出して検証する | resolved |
| FR6 | 抑止読み込みありの本番 reader の予算テスト | `run_reader_with_suppressed_reads` で `prepare_suppressed_replacement` の両分岐を通して検証する | resolved |
| FR7 | 非線形判明時に限る本番コード修正 | FR1〜FR6 のテストが非線形を示した場合のみ本番コードを変更する | resolved |
| FR8 | test-docs 記録 | 新規テストを受け入れ基準ごとに記録する | resolved |

以下、「入力」は `(ESC P x)*N` の後に `ESC \` が続くバイト列を指す。

### 4.2 機能詳細

#### FR1: 共有 strip の予算テスト

**説明**: 共有 strip の各エントリポイントに、2 MiB リングの大きさの入力を与えるテストを追加する。対象は次の 4 つ。
- `strip_replayable_rich_content`
- `strip_pty_output_for_scrollback_write`
- watch オフセット付きの `strip_rich_content_and_remap`
- ground 状態から開始した `strip_pty_output_for_scrollback_write_with_written_state`

**期待結果**: 各呼び出しが予算内に完了し、入力をバイト単位でそのまま返す（SIXEL DCS を含まないため、何も除去されない）。

#### FR2: write filter の予算テスト

**説明**: `ScrollbackWriteFilter` に入力を次の 3 ケースで与えるテストを追加する。
- (a) 保持中のランが `SCROLLBACK_FILTER_PENDING_CAP`（512 KiB）をわずかに下回る大きさで、1 回の呼び出しで与える。
- (b) 保持中のチェーンが上限をわずかに下回る大きさで、reader と同じ最大 65,536 バイトの断片に分けて与える。
- (c) 上限を超えて与え、オーバーフローフラッシュを実行させる。

**期待結果**: 各ケースが予算内に完了し、出力バイトを連結したものが入力と一致し、末尾の ST の後に pending が空になる。

#### FR3: snapshot 経路の予算テスト

**説明**: 入力からなる 2 MiB の scrollback と dimension セグメントから、`build_snapshot_bytes` と `build_resume_snapshot_bytes` で snapshot を組み立てるテストを追加する。

**期待結果**: 各組み立てが予算内に完了し、payload の scrollback 部分が入力と一致し、マップ後のセグメントオフセットが単調非減少かつ payload の範囲内に収まる。

#### FR4: 本番 reader（Detached）の予算テスト

**説明**: 接続中のオーナーを持たないペインの本番 reader（`run_reader_without_owner`）に、最大 65,536 バイトずつの読み込みで pending 上限の範囲を含む入力を流すテストを追加する。

**期待結果**: 予算内に完了し、リングが入力と一致する。

#### FR5: client-parity scan の予算テスト

**説明**: `client_parity_scan::scan` を、入力を長いチャンク（2 MiB）として直接呼び出すテストを追加する。除外ピースなしと除外ピースありの両方で呼び出す。

**期待結果**: 各呼び出しが予算内に完了し、item を報告しない。除外ピースなしで末尾に完全な ST がある場合、tail を報告しない。

#### FR6: 抑止読み込みありの本番 reader の予算テスト

**説明**: 抑止読み込みありの本番 reader（`run_reader_with_suppressed_reads`）に入力を流し、`prepare_suppressed_replacement` の両分岐を実行させるテストを追加する。
- write filter がチェーンを保持している間の抑止読み込み（pending が空でない。除外ピースありの scan。`suppressed_output.rs:246`）
- 末尾の ST の後の抑止読み込み（pending が空。除外ピースなしの scan。`suppressed_output.rs:250`）

**期待結果**: 予算内に完了し、リングが入力と一致し、EOF マーカー以外に空の PtyOutput チャンクが無い。

#### FR7: 非線形判明時に限る本番コード修正

**説明**: 本番コードは、FR1〜FR6 のいずれかのテストが非線形な挙動を示した場合に限り変更する。

**ビジネスルール**:
- 変更する場合、該当経路を線形にする。
- strip 対象の判定はすべて変えない（非 SIXEL DCS はバイト単位でそのまま残り、SIXEL DCS は従来どおり除去される）。

#### FR8: test-docs 記録

**説明**: `test-docs/mux-strip-non-sixel-dcs-linear/taskNNNN.tests.yaml` に、新規テストを受け入れ基準ごとに列挙する。

**ビジネスルール**:
- `red_confirmed: false` とする。
- `red_reason` には、二次時間走査は本フィーチャー以前に解消済みであり（`scan_body_end` と `find_st` は最初の ESC で停止する）、テストは当初から green の回帰ガードであることを記す。

## 5. 非機能要件

### 5.1 パフォーマンス要件
- **NFR1（予算と入力サイズ）**: 各予算アサーションは既存の 10 秒予算の慣例に従う。入力長は、二次時間の走査であれば予算を桁違いに超える大きさにする。write 経路では少なくとも 512 KiB の pending 上限の範囲、strip・snapshot・直接 scan の呼び出しでは 2 MiB とする。reader レベルのケースは 65,536 バイトの読み込みバッファで、抑止読み込みの場合はさらにハーネスの容量 16 の出力チャネルで上限が決まる。

### 5.2 セキュリティ要件
該当なし。

### 5.3 可用性要件
該当なし。

### 5.4 保守性要件
- **NFR2（テストの書き方）**: 既存の cargo `#[test]` ハーネスを使い、`#[ignore]` を付けず、新規依存を追加せず、既存の予算テストと同じ書き方にする。
- **NFR3（ビルドとテストの検証）**: `--lib` のテスト全体の実行と、`--no-default-features` の `cargo check` が成功する。

### 5.5 互換性要件
- **NFR4（挙動の維持）**: 既存のすべての経路で、strip 対象の判定と書き込まれるバイトを変えない。

## 6. UI/UX要件

該当なし。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約
- reader レベルの読み込みは最大 65,536 バイト（`pty_reader_loop` のバッファ、`mod.rs:339`）。`run_reader_with_suppressed_reads` は容量 16 のチャネルを使い、join までコンシューマを持たないため、抑止読み込みケースの読み込み回数に上限がある。入力サイズを桁違いにする要件は strip・write filter・snapshot・直接 scan のケースに適用する（前提 A4）。
- 新規依存を追加しない（NFR2）。

### 9.2 ビジネス上の制約
該当なし。

### 9.3 スケジュール制約
該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-strip-non-sixel-dcs-linear/**`
- `test-docs/mux-strip-non-sixel-dcs-linear/**`

`feature-docs/mux-strip-non-sixel-dcs-linear/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-strip-non-sixel-dcs-linear/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-strip-non-sixel-dcs-linear/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題
| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 無関係な予算テストが負荷によって超過することがある | 低 | 過去にも記録されており、本フィーチャーの回帰シグナルとは扱わない（前提 A5） |

### 10.2 ビジネスリスク
該当なし。

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1（FR1, NFR1）: 共有 strip の各エントリポイントが 2 MiB の入力に対して 10 秒以内に完了し、入力をそのまま返す。
- [ ] AC-2（FR2, NFR1）: write filter が一括・分割・オーバーフローの各ケースで 10 秒以内に完了し、出力バイトが入力と一致し、最後に pending が空になる。
- [ ] AC-3（FR3, NFR1）: 両方の snapshot builder が 2 MiB の scrollback に対して 10 秒以内に完了し、scrollback 部分が入力と一致し、セグメントオフセットが単調非減少かつ payload の範囲内に収まる。
- [ ] AC-4（FR4, NFR1）: Detached の本番 reader が 10 秒以内に完了し、リングが入力と一致する。
- [ ] AC-5（FR5, FR6, NFR1）: 直接呼び出した `client_parity_scan::scan` が除外ピースあり・なしの両方で 10 秒以内に完了し、item を報告しない。抑止読み込みありの本番 reader が置き換えの両分岐を 10 秒以内に実行し、リングが入力と一致し、EOF 以外に空の PtyOutput チャンクを送らない。
- [ ] AC-6（FR7, NFR4）: AC-1〜AC-5 が通る場合、本番コードを変更しない。変更が必要な場合、変更後に AC-1〜AC-5 が通り、既存のテストがすべて green のままである。
- [ ] AC-7（FR8）: test-docs 記録がテストを `red_confirmed: false` と上記の `red_reason` で列挙している。
- [ ] AC-8（NFR2, NFR3）: `--lib` のテスト全体が通り、`--no-default-features` の `cargo check` が成功する。

### 11.2 KPI
該当なし。

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] TS-1（AC-1）: 2 MiB の `(ESC P x)*N` + `ESC \` に対する共有 strip の各エントリポイント。10 秒未満、出力が入力と一致する。
- [ ] TS-2（AC-2）: `ScrollbackWriteFilter` に、上限をわずかに下回る 1 回呼び出し、上限をわずかに下回る最大 65,536 バイトの断片、上限超過（オーバーフローフラッシュ）。10 秒未満、出力が入力と一致し、pending が空。
- [ ] TS-3（AC-3）: この形の 2 MiB の scrollback とセグメントによる `build_snapshot_bytes` / `build_resume_snapshot_bytes`。10 秒未満、scrollback 部分が入力と一致し、オフセットが単調かつ payload の範囲内。
- [ ] TS-4（AC-4）: 上限の範囲にわたり 65,536 バイトずつ読み込む `run_reader_without_owner`。10 秒未満、リングが入力と一致する。
- [ ] TS-5（AC-5）: 2 MiB のチャンクに対する除外ピースあり・なしの `client_parity_scan::scan` 直接呼び出し。10 秒未満、item なし。
- [ ] TS-6（AC-5）: チェーン保持中に 1 回、末尾の ST の後に 1 回の抑止読み込みを行う `run_reader_with_suppressed_reads`。10 秒未満、リングが入力と一致し、空の PtyOutput チャンクは EOF のみ。
- [ ] TS-7（AC-6, AC-8）: `--lib` のテスト全体の実行と `--no-default-features` の `cargo check`。
- [ ] TS-8（AC-7）: test-docs 記録がすべての新規テストを `red_confirmed: false` と記載の `red_reason` で列挙している。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 入力 | `(ESC P x)*N` の後に `ESC \` が続くバイト列 |
| 予算（budget） | テストの所要時間の上限。10 秒 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 二次時間経路の現状: タスクで指摘された `find_st_terminator`（`scrollback_filter.rs:669-683`）は HEAD に存在しない。共有 strip の `scan_body_end`（`scrollback_filter.rs:875`）、非 SIXEL DCS のフォールバック（`scrollback_filter.rs:774-792`）、write filter の `find_st`（`write_filter.rs:1069`）、client-parity の `find_st_terminator`（`client_parity_scan.rs:405`）はいずれも最初の ESC で停止する（A1）。
- [x] write 経路のテスト範囲（`requirement.write-path-test-scope` = `filter_and_reader`）: `ScrollbackWriteFilter` と本番 reader を対象にする。`client_parity_scan::scan` は直接呼び出しと `run_reader_with_suppressed_reads` 経由（`suppressed_output.rs:246` / `:250` の両分岐）で検証する。`run_reader_without_owner` は Detached で `client_parity_scan` に到達しない（A2）。
- [x] red フェーズの扱い（`requirement.red-phase-handling` = `regression_guard_green`）: 新規テストは HEAD で通ることを想定した回帰ガードとして追加し、`test-docs/mux-snapshot-strip-can-abort/task0001.tests.yaml`（AC-4 / AC-6 の項目）に倣って `red_confirmed: false` で記録する（A3）。
- [x] reader レベルの読み込み上限: 最大 65,536 バイト（`pty_reader_loop` のバッファ、`mod.rs:339`）。`run_reader_with_suppressed_reads` は容量 16 のチャネルで join までコンシューマを持たない。入力サイズを桁違いにする要件は strip・write filter・snapshot・直接 scan のケースに適用する（A4）。
- [x] 10 秒の予算: pty_spawn テストモジュールの既存 `BUDGET` 定数と、mux-strip-concat-query-closure の TS-9 の慣例に従う。無関係な予算テストの負荷による超過は過去にも記録されており、本フィーチャーの回帰シグナルではない（A5）。
- [x] シンボル・テストの削除や改名は無いため、`.claude/rules/test-docs-records.md` の改名時の更新義務は適用されない（A6）。

### 14.2 未確認・保留事項
なし。

## 15. 参考資料

- `test-docs/mux-snapshot-strip-can-abort/task0001.tests.yaml`: 回帰ガードの記録形式（AC-4 / AC-6 の項目）
- `.claude/rules/test-docs-records.md`: test-docs 記録のルール
