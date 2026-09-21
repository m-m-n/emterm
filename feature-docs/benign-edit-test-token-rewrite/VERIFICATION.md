# Verification Document: benign-edit-test-token-rewrite

## Overview

**Feature**: benign-edit-test-token-rewrite /
**SPEC.md**: `feature-docs/benign-edit-test-token-rewrite/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/benign-edit-test-token-rewrite/IMPLEMENTATION.md`

本書は統合後の検証を定義する。タスク単位の受け入れ基準は task0001 の計画
（`feature-docs/benign-edit-test-token-rewrite/tasks/task0001.md`）にある。

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0、エラーなし
- 追加ゲート（NFR4 / AC-9）:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  - Expected: exit code 0。テストは GUI 専用の `window_host` モジュール配下にあるため、
    CLI 専用ビルドは本 feature の影響を受けない。
- `web` コンポーネント（TypeScript）は本 feature で変更しないため、ビルド検証の対象外。

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0。書き換え後の良性編集テストを含めて全件パス。
- 並列実行で実行すること（`-- --test-threads=1` を付けない）。付けないと NFR3 の並列安全性が
  検証されない。
- Coverage target: 該当なし。本プロジェクトはカバレッジ計測ツールを構成しておらず、
  `workflow.yaml` の `project.components` にもカバレッジコマンドが存在しない。本 feature の
  カバレッジは、下表の TS-1〜TS-7 の充足で代替する。

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `button_path_token_level_trailing_comma_edit_is_still_accepted` — 抽出した本体の held 対応呼び出しの実引数リストに、既存トークンヘルパーで末尾カンマトークンを追加する | トークン列が未編集と異なる。実引数の個数は依然 4 個。判定が `Ok` | Unit |
| TS-2 | `button_path_token_level_statement_reorder_is_still_accepted` — 抽出した本体内で隣接する独立したトップレベル文 2 つを構造的に特定し、トークンレベルで入れ替える | トークン列が未編集と異なる。判定が `Ok` | Unit |
| TS-3 | `button_path_anchored_rewrap_edit_is_still_accepted` — 二段アンカーで呼び出しのバイト範囲を求め、トークン形状を検証したうえでスライス再構築により実引数周囲の空白を再折り返しする | 範囲のトークン形状が「呼び出し先パス + 実引数 4 個」に一致。ソーステキストが変化。実引数テキストは元とバイト同一。再抽出後の判定が `Ok` | Unit |
| TS-4 | `button_path_anchored_comment_insertion_edit_is_still_accepted` — TS-3 と同じアンカー・スライス経路で、呼び出し先と開き括弧の間にブロックコメントを挿入する | ソーステキストが変化。実引数テキストは元とバイト同一。再抽出後の判定が `Ok` | Unit |
| TS-5 | `anchor_rejects_a_span_whose_token_shape_does_not_match_the_expected_call` — 実引数の個数が異なる／呼び出し先が異なる、呼び出し形状のメモリ内入力にアンカールーチンを適用する | 静かに範囲を生成せず、明示的な失敗を報告する | Unit |
| TS-6 | `anchor_reports_an_explicit_failure_on_a_comment_broken_function_name_anchor` — `fn` キーワードと関数名の間にブロックコメントが挟まるメモリ内入力 | 誤った関数にアンカーせず、パニックもせず、明示的な `Err` を返す（EC-1） | Unit |
| TS-7 | `anchor_reports_an_explicit_failure_on_an_unbalanced_paren_inside_a_comment` — 実引数リスト内のブロックコメントに不均衡な閉じ括弧を含むメモリ内入力 | 明示的な失敗を返す、あるいはコメント対応の走査で正しい範囲を返す。静かに誤った範囲を返さない（EC-2） | Unit |
| TS-M1 | `manual: none required` — プロダクション挙動が無変更であることの確認（Manual Testing 節を参照） | `git diff --name-only` が `src-tauri/` 配下で `src-tauri/src/window_host/tests.rs` のみを示す | Manual |

### 統合後の追加確認（回帰）

- 前 feature の既存テスト（構造テスト群、`pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam`
  を含む）が緑かつ無改変であること。
- `call_site_scan` の既存ヘルパー（`find_calls` / `contains_token_sequence` / `split_arg_spans` /
  `splice_tokens`）のシグネチャ・意味論が無改変であること。
- 旧良性編集テストが残存しておらず、新規テストが重複定義になっていないこと。

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - 本プロジェクトは crate 全体 fmt を機能ファイル以外に適用しない運用のため、差分が出た場合は
    `src-tauri/src/window_host/tests.rs` 以外を巻き込んでいないことを確認する。
- Static analysis: 上記 `cargo check` の 2 コマンド（既定フィーチャーおよび
  `--no-default-features`）をもって静的検証とする。`workflow.yaml` に専用の静的解析コマンドは
  設定されていない。

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | 書き換え後の良性編集テストを含めて `cargo test --lib` がパスする | Test Verification のコマンドを実行し exit code 0 を確認 |
| AC-2 | `tests.rs` に `pointer_routing.rs` のソーステキストを再現する文字列リテラルが存在しない。旧 `:6832` / `:6859` / `:6860` の 3 リテラルが消えており、置き換えのリテラルも閉じた集合の外側の識別子を名指さない | `tests.rs` の良性編集テスト領域を読み、リテラルの有無と、閉じた集合（6 名）外の識別子が現れないことを目視確認。旧 3 リテラルは `git diff` で削除を確認 |
| AC-3 | 末尾カンマ編集と文の並べ替え編集が埋め込み本体に対して既存トークンヘルパーで構築され、スプライス後の判定が `Ok` | TS-1 / TS-2 の合格 |
| AC-4 | 再折り返し編集とコメント挿入編集が二段アンカーで位置決定し、元バイトのスライスで構築されている。編集後ソース中の実引数テキストが元とバイト同一 | TS-3 / TS-4 の合格（実引数バイト同一性は TS-3 / TS-4 内のアサーションで固定） |
| AC-5 | 各良性編集が受け入れアサーションの前に vacuity ガードを持つ | TS-1 / TS-2 / TS-3 / TS-4 の各テスト本文で、差分アサーションが受け入れアサーションより前に置かれていることを確認 |
| AC-6 | Class A のルーチンが特定範囲のトークン形状（呼び出し先パス + 実引数ちょうど 4 個）をアサートしており、誤った箇所への着地を赤にする | TS-5 の合格（ガードが実際に失敗を返せることの証明）と TS-3 の形状アサーション |
| AC-7 | 4 種すべての良性編集の後も、判定が無変更の閉じた集合定数とアリティ引数で `Ok` を返す | TS-1 / TS-2 / TS-3 / TS-4 の合格。加えて定数が上書き・再定義されていないことを `git diff` で確認 |
| AC-8 | `git diff --name-only` が `src-tauri/` 配下で `src-tauri/src/window_host/tests.rs` のみを示す | TS-M1（Manual Testing 節） |
| AC-9 | `--no-default-features` の `cargo check` が引き続きコンパイルできる | Build Verification の追加ゲート |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3, TS-4（加えて AC-2 のリテラル不在確認） |
| FR2 | task0001 | TS-1, TS-2 |
| FR3 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| FR4 | task0001 | TS-3, TS-4, TS-5, TS-6, TS-7 |
| FR5 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| NFR1 | task0001 | TS-1, TS-2, TS-3, TS-4（既存ヘルパー無改変・依存追加なしを `git diff` で確認） |
| NFR2 | task0001 | TS-M1 |
| NFR3 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-7（並列実行で全件パス） |
| NFR4 | task0001 | TS-M1（加えて Build Verification の `--no-default-features` ゲート） |

## E2E Testing

該当なし。`workflow.yaml` の `project.components` はいずれも `e2e_test_command` が空であり、
本プロジェクトに E2E ハーネスは構成されていない。本 feature はテスト専用の変更であり、
ランタイム挙動を追加も変更もしないため、E2E 化できるシナリオを持たない。

## Manual Testing (E2E Not Possible)

- [ ] TS-M1: 変更集合がテストのみであることの確認（NFR2 / NFR4、AC-8）
  - `git diff --name-only` を base revision に対して実行し、`src-tauri/` 配下の差分が
    `src-tauri/src/window_host/tests.rs` の 1 件のみであることを確認する。
  - `git diff --stat` で `pointer_routing.rs` / `mouse_report.rs` / `event_loop.rs` に
    差分が無いことを確認する。
  - プロダクション挙動は無変更であるため、アプリケーションを起動しての手動確認は行わない。
- デザインステップは本 feature で `skipped` のため、モックとの目視照合は行わない。

## Performance / Security Verification (if applicable)

- Performance: 該当なし（NFR4）。追加したテストの実行コストは無視できる程度であり、
  `#[ignore]` ゲートを必要としない。`cargo test --lib` の所要時間が本 feature の前後で
  体感できるほど悪化していないことだけを確認する。
- Security: 該当なし。本変更はテスト専用であり、認証・認可・入力検証・XSS・インジェクション・
  データ保護のいずれの表面も持たず、ランタイムのコード経路を追加しない。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests | 7 (TS-1〜TS-7) | 7 | 0 | 0 |
| Code quality | 2 | 2 | 0 | 0 |
| Success criteria | 9 (AC-1〜AC-9) | 7 | 0 | 2 (AC-2 の目視確認, AC-8) |
| Manual | 1 (TS-M1) | 0 | 0 | 1 |
