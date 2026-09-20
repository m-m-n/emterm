# Feature: mouse-report-held-callsite-test

要件定義書: `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`

## Overview

`src-tauri/src/window_host/pointer_routing.rs` のボタン経路 (`run_button_decision`) と
ホイール経路 (`handle_mouse_wheel`) の**呼び出し側**が、ライブな held ボタン値を held 対応の
apply エントリポイントに渡していることを固定するテストを追加する。既存の構造テストは
全ファイルに対する `src.contains("mouse_report::apply_outcome_with_held(")` というニードルで
あり、モーション経路の呼び出し 1 箇所だけで充足されるため、この 2 経路の破壊を検出できない。
本 feature は `src-tauri/src/window_host/tests.rs` への追加のみで、プロダクション挙動を一切
変更しない。

## Objectives

- mouse-drag-latch-regression の AC-3 のうち未達だった半分を閉じる: ボタン経路
  (`run_button_decision`) とホイール経路 (`handle_mouse_wheel`) の呼び出し側が、ライブな held
  ボタン値を held 対応の apply エントリポイントに渡さなくなったときに、必ず赤になるテストを置く。
- 既存の構造テストの全ファイル走査ニードル `src.contains("mouse_report::apply_outcome_with_held(")`
  が、モーション経路の呼び出し側 1 箇所だけで充足されてしまう検出漏れを解消する。
- プロダクションのマウスレポート挙動を一切変更せずに達成する（前 feature の FR4 を継承）。

## User Stories

### US1: 呼び出し側の held 受け渡しが壊れたら赤になる

eMterm の開発者として、`pointer_routing.rs` のボタン経路・ホイール経路の held 実引数の
受け渡しが既定値化・削除・旧ラッパへの差し戻しで壊れたときにテストが赤になってほしい。
そうすればマウスレポートの held ラッチ退行を再発させずに済む。

**Acceptance Criteria:**
- [ ] AC-2: `run_button_decision` 本体の `mouse_report::apply_outcome_with_held(` 第 4 実引数が
      `HeldButtons::default()` に置き換わったら失敗するテストが存在する（FR1）。
- [ ] AC-3: 同第 4 実引数が削除されて実引数が 3 個になったとき、または呼び出しが
      `mouse_report::apply_outcome(` に差し戻ったときに失敗するテストが存在する（FR1 / FR3）。
- [ ] AC-4: `handle_mouse_wheel` 本体の `mouse_report::apply_wheel_report_step(` 第 5 実引数が
      `HeldButtons::default()` に置き換わる／削除される／呼び出しが held 非対応エントリポイントに
      差し戻るいずれでも失敗するテストが存在する（FR2 / FR3）。

### US2: 良性編集では赤にならず、既存テストも壊れない

eMterm の開発者として、空白・コメント・並べ替えといった良性編集で誤って赤にならず、
既存の構造テストも無改変のまま緑であってほしい。そうすれば新しいテストが開発の妨げにならない。

**Acceptance Criteria:**
- [ ] AC-5: 対象 2 本体に対する良性編集（空白・改行・コメント追加・末尾カンマ・無関係コードの
      並べ替え）ではテストが緑のまま保たれる（FR7）。
- [ ] AC-6: 字句走査器が、コメント内および文字列リテラル内に書かれた「正しい呼び出し」を
      検出対象から除外することを、メモリ内入力に対する専用テストで示す（FR6）。
- [ ] AC-8: `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam` を含む
      既存テストが無改変のままパスする（FR8）。

## Technical Requirements

### Functional Requirements

- **FR1 — ボタン経路の呼び出し側固定:** `src-tauri/src/window_host/pointer_routing.rs` の
  トップレベル関数 `run_button_decision` の本体が `mouse_report::apply_outcome_with_held(` を
  実引数 4 個で呼び、その第 4 実引数が `[<host 引数の識別子>, `.`, `mouse_report_held`]` の
  3 トークンと完全一致することを固定するテストが存在する。この受け渡しが
  `HeldButtons::default()` などの既定値に置き換わる、または削除されて実引数が 3 個になった
  場合に赤になる。
- **FR2 — ホイール経路の呼び出し側固定:** 同ファイルのトップレベル関数 `handle_mouse_wheel` の
  本体が `mouse_report::apply_wheel_report_step(` を実引数 5 個で呼び、その第 5 実引数が FR1 と
  同じ 3 トークン列と完全一致することを固定するテストが存在する。既定値化・引数削除のいずれでも
  赤になる。
- **FR3 — held 非対応エントリポイントへの差し戻し禁止:** `run_button_decision` と
  `handle_mouse_wheel` の両本体において、トークン単位で識別された呼び出し
  `mouse_report::apply_outcome(` の出現を禁止する。トークン単位判定のため
  `apply_outcome_with_held` は別識別子として扱われ、この禁止に該当しない。
- **FR4 — プロダクション挙動を変更しない:** `pointer_routing.rs` / `mouse_report.rs` /
  `event_loop.rs` のプロダクションコードはベースリビジョンのまま据え置く。本 feature の
  変更集合は `src-tauri/src/window_host/tests.rs` への追加のみに閉じる（既存テストの改変も
  行わない）。
- **FR5 — 固定メカニズム = トークン単位の source-scan と波括弧深度による本体抽出:**
  呼び出し側の固定は source-scan 方式で行う。`include_str!("pointer_routing.rs")` で自ファイルと
  同ディレクトリのソースを読み、トップレベルの `run_button_decision` と `handle_mouse_wheel` の
  本体を、引数括弧の後の `{` から対応する `}` まで波括弧深度で抽出する。行番号・コメント内容・
  次関数の位置に依存しない。生の substring 照合は用いない。
- **FR6 — 字句走査器の要件と自己検証:** 新規依存を追加せず、テストモジュール内に状態付きの
  字句走査器を置く。行コメントおよび入れ子ブロックコメントは空白相当として扱い、文字列・
  文字リテラル（エスケープ、raw 文字列の `#` 個数、byte/C 文字列、ライフタイムとの区別を含む）は
  中身を検索しない単一トークンとして扱う。走査器はメモリ内入力に対しても検証し、コメントや
  文字列リテラルの中に正しい呼び出しを書いても赤を隠せないことを確認する。
- **FR7 — 良性編集への耐性と変異感度:** 空白・改行・コメント・末尾カンマ・無関係コードの
  並べ替えといった良性編集では緑のまま保たれ、第 4/第 5 実引数の既定値化、旧ラッパ
  (`apply_outcome`) への差し戻し、当該引数の削除のいずれでも赤になる。
- **FR8 — 既存テストの無改変維持:** `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam`
  (`tests.rs:1780`) をはじめとする既存の構造テスト・シームレベルテストは無改変のまま緑を保つ。
  新規テストはそれらを置換せず並置する。

### Non-Functional Requirements

- **NFR1 — 素のユニットテストシームのみ:** テストは `WindowHost`・winit ウィンドウ・wgpu
  サーフェス・PTY・`term_core` のモード型のいずれの型も構築せず、名指さない。ソース文字列と
  字句走査器だけで完結する。
- **NFR2 — インラインテストモジュール、新規依存なし:** テストは既存の `#[cfg(test)]` モジュール
  ファイル `src-tauri/src/window_host/tests.rs` に置く（`test/README.md` の Test File
  Organization）。新しい結合テストのコンパイル単位も、新しいテストフレームワーク依存
  （proptest / criterion）も追加しない。
- **NFR3 — リポジトリのテスト命名規約:** テスト名は `<subject>_<scenario>_<expected>` パターンに
  従う（`test/README.md` の Test Naming Conventions）。
- **NFR4 — 決定的かつ並列安全:** 各テストは共有のグローバルフィクスチャを持たず、
  `include_str!` によるコンパイル時埋め込みとローカルな走査器状態のみを使う。
  `--test-threads=1` を必要としない。
- **NFR5 — 観測可能な契約をアサートする（明示的な例外つき）:** アサーションは原則として
  観測可能な契約（disposition の値、gesture slot の `peek`、`dragging`、publish 先 / sink の
  呼び出し）に対して行い、内部専用の状態には行わない。**例外**: 本 feature に限り、
  ソーステキストに対する構造アサーションを許可する。適用範囲は「ボタン経路とホイール経路の
  held 実引数の受け渡し検査」— すなわち FR1 / FR2 / FR3 が名指す 2 つの呼び出し接続のみに限る。
  この 2 接続以外へ source-scan を広げること、およびプロダクションコードを観測可能化のために
  改変することは、本例外の範囲外とする。
- **NFR6 — 実行コストが無視できる:** 走査対象はコンパイル時に埋め込まれた 1 ファイルの
  2 関数本体のみ。`#[ignore]` ゲートは不要。
- **NFR7 — フィーチャーゲートに影響しない:** 新規テストは GUI 専用の `window_host` モジュール
  配下に置かれるため、
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  のコンパイルに影響しない。
- **NFR8 — false red の抑制:** 固定する名前は対象関数名（`run_button_decision` /
  `handle_mouse_wheel`）、呼び出し先名（`apply_outcome_with_held` / `apply_wheel_report_step` /
  `apply_outcome`）、フィールド名（`mouse_report_held`）に限る。`host` 相当の識別子は本体を
  ハードコードせず、抽出した関数の実際の引数名から取得する。正当なリネームを行う場合は
  同じ変更でテスト側の検索名も更新する。

## Implementation Approach

### Architecture

**System Architecture:** 該当なし。本 feature は層構造を持つシステムを追加せず、既存の
`#[cfg(test)]` モジュールにテストを追加するだけのテスト専用変更である。

**Component Diagram:** テスト内の構成要素のみ。

```
tests.rs (#[cfg(test)])
  ├── 字句走査器          — 行コメント / 入れ子ブロックコメント / 文字列・文字リテラルを
  │                         処理してトークン列を作る（FR6）
  ├── 本体抽出器          — トップレベル関数のシグネチャを見つけ、引数括弧の後の `{` から
  │                         対応する `}` までを波括弧深度で抽出する（FR5）
  ├── 実引数分解器        — 呼び出しの実引数をトップレベルのカンマで分割する（FR1 / FR2）
  └── 判定関数            — 実引数個数と held 実引数の 3 トークン列を照合し、
                            `apply_outcome(` の出現を禁止する（FR1 / FR2 / FR3）
```

### Data Flow

```
include_str!("pointer_routing.rs")
  → 字句走査（コメント除去・文字列リテラルを単一トークン化）
  → トップレベル関数の探索（run_button_decision / handle_mouse_wheel）
  → 波括弧深度による本体抽出
  → 対象呼び出しの実引数分解
  → held 実引数の 3 トークン照合 / apply_outcome( の不在確認
  → assert!
```

### API Design

該当なし。公開 API もエンドポイントも追加しないテスト専用の変更である。

### Database Schema

該当なし。永続化するデータを持たないテスト専用の変更である。

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/window_host/pointer_routing.rs`: `include_str!` の走査対象。読み取りのみで、
  改変しない（FR4）。`include_str!` は同ディレクトリの相対パスで解決できる（A5）。
- `src-tauri/src/window_host/mouse_report.rs`: `apply_outcome_with_held` /
  `apply_wheel_report_step` / `apply_outcome` の定義元。改変しない（FR4）。
- `src-tauri/src/window_host/tests.rs`: 新規テストの置き場所（NFR2）。既存テストは無改変（FR8）。

**External Dependencies:**
- なし。新しいテストフレームワーク依存（proptest / criterion）を追加しない（NFR2）。

### File Structure

```
src-tauri/src/window_host/
├── pointer_routing.rs      # 走査対象（改変しない）
├── mouse_report.rs         # 呼び出し先の定義元（改変しない）
├── event_loop.rs           # 改変しない
└── tests.rs                # ここにのみ追記する（TS-1〜TS-6）
```

### Known Trade-off

source-scan は名前に結合する。`run_button_decision` / `handle_mouse_wheel` /
`apply_outcome_with_held` / `apply_wheel_report_step` / `mouse_report_held` の正当なリネームを
行う場合は、**同じ変更でテスト側の検索名も更新する**。これは NFR8 として明記した既知の
トレードオフであり、欠陥ではない（A8）。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mouse-report-held-callsite-test/**`
- `test-docs/mouse-report-held-callsite-test/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** `run_button_decision_passes_live_held_value_to_the_held_aware_apply_entry_point`
      — `include_str!("pointer_routing.rs")` を波括弧深度で走査して `run_button_decision` の
      本体を抽出し、トークン列から `mouse_report::apply_outcome_with_held(` の実引数を分解する。
      実引数 4 個、第 4 実引数が `[host 引数名, `.`, `mouse_report_held`]` の 3 トークンと完全
      一致することをアサートする。host 引数名は同関数のシグネチャから取得する。
      （要件: FR1, FR5 / AC: AC-2, AC-3）
- [ ] **TS-2** `handle_mouse_wheel_passes_live_held_value_to_the_wheel_report_step`
      — 同様に `handle_mouse_wheel` の本体を抽出し、`mouse_report::apply_wheel_report_step(` の
      実引数が 5 個で第 5 実引数が同じ 3 トークン列であることをアサートする。
      （要件: FR2, FR5 / AC: AC-4）
- [ ] **TS-3** `button_and_wheel_bodies_never_call_the_held_unaware_apply_entry_point`
      — 抽出した 2 本体のトークン列に `mouse_report :: apply_outcome (` 相当の並びが現れない
      ことをアサートする。`apply_outcome_with_held` は別識別子なので誤検出しないことを同テスト内で
      確認する。（要件: FR3 / AC: AC-3, AC-4）
- [ ] **TS-4** `body_extractor_tolerates_benign_edits_and_ignores_comments_and_string_literals`
      — メモリ内のソース断片に対する走査器テスト。行コメント／入れ子ブロックコメント、
      エスケープ入り文字列、raw 文字列（`#` 個数違い）、byte/C 文字列、ライフタイム `'a` と
      文字リテラル `'a'` の区別、末尾カンマ、改行位置の違いをカバーする。コメント／文字列の
      中に正しい呼び出しを書いた入力では検出が成立しない（赤を隠せない）ことをアサートする。
      （要件: FR6, FR7 / AC: AC-5, AC-6）
- [ ] **TS-5** `argument_scanner_rejects_defaulted_and_deleted_held_arguments`
      — メモリ内の変異済みソース断片（第 4/第 5 実引数を `HeldButtons::default()` に置換した
      もの、当該引数を削除したもの、旧ラッパへ差し戻したもの）に対し、走査器の判定関数が
      不合格を返すことをアサートする。実ファイルに変異を加えずに TS-1〜TS-3 の赤条件を証明する。
      （要件: FR7 / AC: AC-2, AC-3, AC-4）
- [ ] **TS-6** `held_argument_scan_is_scoped_to_the_two_named_bodies_only`
      — モーション経路（`handle_pointer_moved` 内の 3 番目の `apply_outcome_with_held` 呼び出し）が
      抽出結果に混入しないことをアサートする。これが既存の全ファイル走査ニードルとの差分であり、
      本 feature の検出漏れ解消の本体。（要件: FR5, NFR8 / AC: AC-5）

### Integration Tests

該当なし。新しい結合テストのコンパイル単位を追加しない（NFR2）。全シナリオは
`cargo test --lib` の対象となるインラインユニットテストである。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] **TS-M1** `manual: none required` — プロダクション挙動を変更しないため手動確認は不要。
      E2E ハーネスも存在しない。AC-7 は `git diff --name-only` / `git diff --stat` による
      変更集合検証で満たす。（要件: FR4 / AC: AC-7）

### Edge Cases

- [ ] 入れ子ブロックコメント: 空白相当として扱い、閉じ位置を正しく追跡する（FR6 / TS-4）。
- [ ] raw 文字列の `#` 個数違い: 対応する終端で閉じる（FR6 / TS-4）。
- [ ] byte 文字列 / C 文字列: 中身を検索しない単一トークンとして扱う（FR6 / TS-4）。
- [ ] ライフタイム `'a` と文字リテラル `'a'` の区別: 文字リテラルの誤検出を起こさない
      （FR6 / TS-4）。
- [ ] 末尾カンマ: 実引数個数の判定に影響させない（FR7 / TS-4）。
- [ ] `apply_outcome_with_held` を `apply_outcome` として誤検出しない: トークン単位判定に
      よって別識別子として扱う（FR3 / TS-3）。

### Performance Tests

該当なし。走査対象はコンパイル時に埋め込まれた 1 ファイルの 2 関数本体のみで、実行コストが
無視できる（NFR6）。

## Security Considerations

該当なし。認証・認可・入力検証・データ保護・XSS・SQL インジェクション・CSRF のいずれの
表面も持たない、テスト専用の変更である。

## Error Handling

### Error Codes

該当なし。エラーコードを返す実行時経路を持たない。失敗はすべてテストのアサーション失敗として
`cargo test` の出力に現れる。

### Error Flow

```
判定関数が不合格 → assert! が失敗 → cargo test が赤 → 開発者が呼び出し側の held 受け渡しを修正
```

## Performance Optimization

### Performance Goals

該当なし（NFR6 のとおり実行コストが無視できるため、数値目標を置かない）。

### Optimization Strategies

該当なし。

### Caching Strategy

該当なし。`include_str!` によるコンパイル時埋め込みのみを使い、実行時キャッシュを持たない（NFR4）。

## Traceability

| ID | Title | Status | AC | TS |
|----|-------|--------|----|----|
| FR1 | ボタン経路の呼び出し側固定 | resolved | AC-2, AC-3 | TS-1 |
| FR2 | ホイール経路の呼び出し側固定 | resolved | AC-4 | TS-2 |
| FR3 | held 非対応エントリポイントへの差し戻し禁止 | resolved | AC-3, AC-4 | TS-3 |
| FR4 | プロダクション挙動を変更しない | resolved | AC-7 | TS-M1 |
| FR5 | 固定メカニズム = トークン単位の source-scan と波括弧深度による本体抽出 | resolved | AC-2, AC-3, AC-4, AC-5 | TS-1, TS-2, TS-6 |
| FR6 | 字句走査器の要件と自己検証 | resolved | AC-5, AC-6 | TS-4 |
| FR7 | 良性編集への耐性と変異感度 | resolved | AC-2, AC-3, AC-4, AC-5, AC-6 | TS-4, TS-5 |
| FR8 | 既存テストの無改変維持 | resolved | AC-8 | — |
| NFR1 | 素のユニットテストシームのみ | resolved | AC-1 | — |
| NFR2 | インラインテストモジュール、新規依存なし | resolved | AC-1 | — |
| NFR3 | リポジトリのテスト命名規約 | resolved | AC-1 | — |
| NFR4 | 決定的かつ並列安全 | resolved | AC-1 | — |
| NFR5 | 観測可能な契約をアサートする（明示的な例外つき） | resolved | AC-7 | — |
| NFR6 | 実行コストが無視できる | resolved | AC-1 | — |
| NFR7 | フィーチャーゲートに影響しない | resolved | AC-1 | — |
| NFR8 | false red の抑制 | resolved | AC-5 | TS-6 |

| TS ID | Requirements | Acceptance Criteria |
|-------|--------------|---------------------|
| TS-1 | FR1, FR5 | AC-2, AC-3 |
| TS-2 | FR2, FR5 | AC-4 |
| TS-3 | FR3 | AC-3, AC-4 |
| TS-4 | FR6, FR7 | AC-5, AC-6 |
| TS-5 | FR7 | AC-2, AC-3, AC-4 |
| TS-6 | FR5, NFR8 | AC-5 |
| TS-M1 | FR4 | AC-7 |

## Assumptions

- **A1**: 両プロダクション呼び出し側は現時点で正しい。`pointer_routing.rs:850` が
  `mouse_report::apply_outcome_with_held(outcome, &mut records, &mut dest, host.mouse_report_held);`、
  `pointer_routing.rs:1129-1135` が
  `mouse_report::apply_wheel_report_step(outcome, &mut records, &mut dest, lines, host.mouse_report_held)`
  であることを実地確認した。したがって新規テストはベースリビジョンで緑から始まる。
  （reversible: true / 根拠: `src-tauri/src/window_host/pointer_routing.rs:850, 1129-1135`）
- **A2**: 既存の構造テストのニードルはモーション経路だけで充足される。`tests.rs:1806-1822` の
  `delegate` ループは全ファイルに対する `src.contains("mouse_report::apply_outcome_with_held(")`
  であり、`handle_pointer_moved`（`pointer_routing.rs:293-298`）の呼び出し 1 箇所で満たされる。
  これが AC-3 の検出漏れの直接の原因。（reversible: true / 根拠:
  `src-tauri/src/window_host/tests.rs:1780-1823`; `src-tauri/src/window_host/pointer_routing.rs:293-298`）
- **A3**: 記録済みの変異プローブ W と B はいずれも呼び出し先（callee）を変異させており、
  呼び出し側は一度も変異させていない。（reversible: true / 根拠:
  `test-docs/mouse-drag-latch-regression/task0001.tests.yaml`: mutation_probes W, B）
- **A4**: 前 feature の AC-3 がホイール経路を `mouse_report.rs:1235` と名指していたことが、
  呼び出し先を指す表現になっていた。本 feature はホイール経路の固定対象を `pointer_routing.rs`
  内の `handle_mouse_wheel` 本体の呼び出し側として再定義する。（reversible: true / 根拠:
  `feature-docs/mouse-drag-latch-regression/SPEC.md:295-297`,
  `REQUIREMENTS.md:256-258`）
- **A5**: `include_str!("pointer_routing.rs")` は既に同ファイル内の複数テストで使われている
  確立した手法であり、`tests.rs` と `pointer_routing.rs` が同ディレクトリにあるため相対パスで
  解決できる。（reversible: true / 根拠: `src-tauri/src/window_host/tests.rs:1781, 2033, 2081, 2133, 2181`）
- **A6**: `mouse_report::apply_outcome`（held 非対応ラッパ）は削除されず、`mouse_report.rs:1138`
  に残り続ける。FR3 はこのラッパの存在自体ではなく、対象 2 本体からの呼び出しを禁止する。
  （reversible: true / 根拠: `src-tauri/src/window_host/mouse_report.rs:1138-1144`）
- **A7**: 前 feature の修正試行 964be64d は自身の NFR5 とタスクスコープに違反したため 8ddefa0c で
  revert された。本 feature はその衝突を、NFR5 に範囲限定の明示的例外を置くことで解消する
  （プロダクションコードの観測可能化リファクタは行わない）。（reversible: false / 根拠:
  task_description; `answers[requirement.callsite-fixation-mechanism].resolution_note`）
- **A8**: source-scan は名前に結合するため、`run_button_decision` / `handle_mouse_wheel` /
  `apply_outcome_with_held` / `apply_wheel_report_step` / `mouse_report_held` の正当なリネームは
  テスト側の検索名の同時更新を要する。これは NFR8 として SPEC に明記する既知のトレードオフで
  あり、欠陥ではない。（reversible: true / 根拠:
  `answers[requirement.callsite-fixation-mechanism].normalized_answer`）

## Success Criteria

- [ ] AC-1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      が新規テストを含めてパスする。
- [ ] AC-2: `run_button_decision` 本体の `mouse_report::apply_outcome_with_held(` 第 4 実引数が
      `HeldButtons::default()` に置き換わったら失敗するテストが存在する（FR1）。
- [ ] AC-3: 同第 4 実引数が削除されて実引数が 3 個になったとき、または呼び出しが
      `mouse_report::apply_outcome(` に差し戻ったときに失敗するテストが存在する（FR1 / FR3）。
- [ ] AC-4: `handle_mouse_wheel` 本体の `mouse_report::apply_wheel_report_step(` 第 5 実引数が
      `HeldButtons::default()` に置き換わる／削除される／呼び出しが held 非対応エントリポイントに
      差し戻るいずれでも失敗するテストが存在する（FR2 / FR3）。
- [ ] AC-5: 対象 2 本体に対する良性編集（空白・改行・コメント追加・末尾カンマ・無関係コードの
      並べ替え）ではテストが緑のまま保たれる（FR7）。
- [ ] AC-6: 字句走査器が、コメント内および文字列リテラル内に書かれた「正しい呼び出し」を
      検出対象から除外することを、メモリ内入力に対する専用テストで示す（FR6）。
- [ ] AC-7: 本 feature の `git diff` が `src-tauri/src/window_host/tests.rs` の追加のみであり、
      `src-tauri/src/` 配下のプロダクション挙動に一切触れていない（FR4）。
- [ ] AC-8: `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam` を含む
      既存テストが無改変のままパスする（FR8）。
- [ ] すべての機能要件が実装・テストされている。
- [ ] すべてのテストシナリオがパスする。
- [ ] コードレビューが完了している。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。FR1-FR8 / NFR1-NFR8 のすべてが resolved であり、`status: tbd` の要件は存在しない。

## Implementation Phases (if applicable)

該当なし。単一の変更（`src-tauri/src/window_host/tests.rs` へのテスト追加）で完結するため、
段階分割を設けない。

## Design Step

Skipped。UI 表面・デザイントークンに一切触れないテスト専用の変更であり、変更集合は
`src-tauri/src/window_host/tests.rs` への追加のみに閉じる（FR4）。描画・レイアウト・MD3 トークンの
いずれにも影響しないため、design ステップで決めるべき視覚的判断が存在しない。

## References

- 要件定義書: `feature-docs/mouse-report-held-callsite-test/REQUIREMENTS.md`
- 前 feature の SPEC: `feature-docs/mouse-drag-latch-regression/SPEC.md:295-297`
- 前 feature の REQUIREMENTS: `feature-docs/mouse-drag-latch-regression/REQUIREMENTS.md:256-258`
- 変異プローブ記録: `test-docs/mouse-drag-latch-regression/task0001.tests.yaml`
- 走査対象: `src-tauri/src/window_host/pointer_routing.rs:293-298, 850, 1129-1135`
- 呼び出し先の定義元: `src-tauri/src/window_host/mouse_report.rs:1138-1144`
- テスト配置先と既存テスト: `src-tauri/src/window_host/tests.rs:1780-1823, 2033, 2081, 2133, 2181`
- テスト規約: `test/README.md`（Test File Organization / Test Naming Conventions）
