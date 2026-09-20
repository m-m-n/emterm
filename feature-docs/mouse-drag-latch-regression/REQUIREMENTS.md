---
title: "mouse-drag-latch-regression"
created_date: 2026-09-21
status: draft
---

# mouse-drag-latch-regression - 要件定義書

## 1. 概要

### 1.1 背景

ゴーストドラッグ（`host.dragging` が恒久的にラッチされたままになる不具合）は、
ベースリビジョン（main, 8c3b4cce）の時点ですでに修正済みである。報告書が挙げた
3 つのメカニズムは次のとおり修正されている（A2）。

- `apply_outcome_with_held` は tracking-inactive リセット時に押されていない
  gesture slot だけをクリアする（`mouse_report.rs:1164-1175`）
- ボタン経路（`pointer_routing.rs:850`）とホイール経路（`mouse_report.rs:1235`）の
  両方が held 対応のエントリポイントを呼ぶ
- owner なしの左リリースは `drag_in_flight` でゲートされている
  （`mouse_report.rs:935`、収集は `pointer_routing.rs:821`）

一方で、この修正済み挙動を固定する自動リグレッションテストは存在しない。

### 1.2 目的

- mouse-report のリセットとジェスチャ所有権の境界に対する将来の変更が、
  恒久的にラッチされた `host.dragging` を再び持ち込むことがないよう、修正済みの
  ゴーストドラッグ挙動を自動リグレッションテストで固定する。
- 意図的なフォーカス喪失セマンティクス（`should_terminate_drag` の `!left_held`
  ゲート）を明示的にテストで固定し、後の修正で欠陥と誤認されて削除されることを防ぐ。
- プロダクション挙動の変更をゼロにする。本フィーチャーの成果物はテストカバレッジが
  すべてである。

### 1.3 スコープ

- 対象: `src-tauri/src/window_host/tests.rs` へのリグレッションテスト追加（A4）。
- 対象外: プロダクション挙動の変更（FR4、A3）。完了条件「再現手順で現象が起きない」は
  ベースリビジョンの時点ですでに満たされている（A3）。

## 2. ビジネス要件

### 2.1 ビジネス目標

1. 修正済みのゴーストドラッグ・ラッチ挙動を自動リグレッションテストで固定し、
   mouse-report のリセット／ジェスチャ所有権の境界への将来の変更が、恒久的に
   ラッチされた `host.dragging` を黙って再導入できないようにする。
2. 意図的なフォーカス喪失セマンティクス（`should_terminate_drag` の `!left_held`
   ゲート）を明示かつテストで固定し、後の修正時に欠陥と誤認して削除されないようにする。
3. プロダクション挙動の変更をゼロにする。本フィーチャーの成果物はテストカバレッジが
   すべてである。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 該当なし | 成果物はテストコードのみで、ユーザー向けの表面を持たない |

### 2.3 期待される効果

- リセット／ジェスチャ所有権の境界の退行がテスト失敗として即座に検出される。
- フォーカス喪失時の意図的な挙動が、テストとドキュメントコメントによって
  「欠陥ではない」と識別可能になる。
- テスト失敗時に、どのメカニズムが退行したかを再調査なしで特定できる（FR6）。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| 該当なし | — | — | — |

本フィーチャーは Rust のユニットテストのみを追加し、UI 表面・視覚的変更・新しい
ユーザー操作・デザイントークンの利用を一切導入しないため、ユースケースを持たない。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | ライブな左ドラッグ中のホイールノッチでドラッグが取り残されない | ホイールノッチ挿入シーケンスのリグレッションテスト | 高 |
| FR2 | ライブな左ドラッグ中の 2 つ目のボタン押下でドラッグが取り残されない | 第 2 ボタン押下シーケンスのリグレッションテスト | 高 |
| FR3 | ライブな左ドラッグ中のフォーカス喪失を現行の意図どおりのセマンティクスに固定する | `should_terminate_drag` の両アームの固定 | 高 |
| FR4 | プロダクション挙動を変更しない | テストモジュール以外の挙動変更なし | 高 |
| FR5 | 既存のフォーカス喪失ゲートテストをグリーンかつ無改変に保つ | 既存テストの非改変 | 高 |
| FR6 | 各新規テストが守るメカニズムを名指しする | テストのドキュメントコメント | 中 |

### 4.2 機能詳細

#### FR1: ライブな左ドラッグ中のホイールノッチでドラッグが取り残されない

**説明**: リグレッションテストが、winit ウィンドウ・GPU・PTY・ターミナルモード型を
一切用いない素の `mouse_report` / `pointer_routing` のシーム上で、次のシーケンスを
駆動する。トラッキング非アクティブの状態でグリッド上を左押下し `GestureOwner::Local`
を `Left` に記録する → `HeldButtons { left: true, .. }` を伴って
`apply_wheel_report_step` 経由でホイールノッチを 1 つ適用する → 左リリース。

**アサーション**:
- ホイールノッチの tracking-inactive リセットが、まだ押されている `Left` の
  gesture slot をクリアしないこと
- リリースの disposition が
  `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` であること
- 結果として生じるドラッグ終了で `dragging == false` になること
- 選択の publish が PRIMARY に届くこと

#### FR2: ライブな左ドラッグ中の 2 つ目のボタン押下でドラッグが取り残されない

**説明**: リグレッションテストが次を駆動する。左押下で `GestureOwner::Local` を
`Left` に記録する → `HeldButtons { left: true, .. }` を伴って
`apply_outcome_with_held` 経由で 2 つ目（middle または right）のボタン押下を適用する
→ 左リリース。

**アサーション**:
- 2 つ目の押下の tracking-inactive リセットが、押されている `Left` の gesture slot を
  そのまま残すこと
- 左リリースが依然として `LocalArm::CompleteSelectionAndPublishToPrimary` を名指すこと
- その後 `dragging == false` であること
- 選択が PRIMARY に publish されること

#### FR3: ライブな左ドラッグ中のフォーカス喪失を現行の意図どおりのセマンティクスに固定する

**説明**: リグレッションテストが、`WindowEvent::Focused(false)` のサイトで合成される
`should_terminate_drag(drag_in_flight, left_held)` の両アームを固定する。

- (a) フォーカス喪失時に左が物理的に押されている場合: ドラッグは保持される
  （フォーカス喪失時点では終了しない）。後続の左リリースが終了者となり、
  `dragging == false` で終わり、選択が publish される。
- (b) フォーカス喪失時に左が押されていない場合: ドラッグはフォーカス喪失時点で
  即座に終了し、`dragging == false` で終わり、選択が publish される。

**ビジネスルール**:
- テストはフォーカス喪失時の無条件終了をアサートしてはならない。

#### FR4: プロダクション挙動を変更しない

**説明**: `src-tauri/src/window_host/` 配下のファイルは、テストモジュール以外
挙動を変更しない。`mouse_report.rs`、`pointer_routing.rs`、`event_loop.rs` の
プロダクションコードはベースリビジョンのまま据え置く。報告書が名指しした 3 つの
メカニズム（`apply_outcome_with_held` の `clear_unheld`、ボタン経路とホイール経路の
両方における held 対応エントリポイント、drag-in-flight でゲートされた owner なしの
左リリース）は、そこですでに修正済みである。

#### FR5: 既存のフォーカス喪失ゲートテストをグリーンかつ無改変に保つ

**説明**: `should_terminate_drag_terminates_only_when_in_flight_and_left_not_held`
（`src-tauri/src/window_host/tests.rs:1938`）と
`focus_loss_arm_never_calls_the_fold_click_toggle`（同ファイル、約 1975 行目、
`event_loop.rs` に対するソーススキャンの needle）は、編集も弱化もしない。FR3 の
新規テストは、それらの上に重ねる追加カバレッジである。

#### FR6: 各新規テストが守るメカニズムを名指しする

**説明**: すべての新規テストは、その失敗がどのメカニズムの退行を示すのかを述べた
ドキュメントコメントを持つ。対象のメカニズムは、reset-clears-held-gesture-slot、
held-unaware apply entry point、ungated no-owner left release、
focus-loss gate inversion の 4 つ。これにより、将来の失敗を本調査の再導出なしに
診断できる。

## 5. 非機能要件

| ID | 要件名 | 内容 |
|----|--------|------|
| NFR1 | 素のユニットテストシームのみ | テストは `WindowHost`・winit ウィンドウ・wgpu サーフェス・PTY・ターミナルモードのいずれの型も名指さない。`src-tauri/src/window_host/tests.rs` の既存のシームレベルテスト（`decide_button_event` / `decide_wheel_event` / `apply_outcome_with_held` / `apply_wheel_report_step` / `consume_drag_termination` / `should_terminate_drag` / `drag_in_flight`）に揃える |
| NFR2 | インラインテストモジュール、新規依存なし | テストは既存の `#[cfg(test)]` モジュールファイル `src-tauri/src/window_host/tests.rs` にインラインで置く（`test/README.md` の「Test File Organization」に従う）。新しい結合テストのコンパイル単位も、新しいテストフレームワーク依存（proptest、criterion）も追加しない |
| NFR3 | リポジトリのテスト命名規約 | テスト名はリポジトリで支配的な `<subject>_<scenario>_<expected>` パターンに従う（`test/README.md` の「Test Naming Conventions」） |
| NFR4 | 決定的かつ並列安全 | 各テストは自前の `MouseReportRecords` / `GestureOwnership` / プレーンな状態を明示的に構築し、共有のグローバルフィクスチャを持たない。したがってスイートは `--test-threads=1` を必要としない |
| NFR5 | 観測可能な契約をアサートする | アサーションは観測可能な契約（disposition の値、gesture slot の `peek`、`dragging`、publish 先／sink の呼び出し）に対して行い、内部専用の状態には行わない（`test/README.md` の「Test Structure」） |
| NFR6 | 実行コストが無視できる | 実行コストは無視できる程度であり、`#[ignore]` によるゲートは不要 |
| NFR7 | フィーチャーゲートに影響しない | 新規テストは GUI 専用の `window_host` モジュール配下に置かれるため、`--no-default-features` のコンパイルに影響しない |

## 6. UI/UX要件

該当なし。本フィーチャーは Rust のユニットテストのみを追加し、UI 表面・視覚的変更・
新しいユーザー操作・デザイントークンの利用を一切導入しないため、デザインステップは
スキップされている。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。新しいテストフレームワーク依存を追加しない（NFR2）。

## 9. 制約条件

### 9.1 技術的制約

- テストは素のユニットテストシームのみを用いる（NFR1）。
- テストは既存のインライン `#[cfg(test)]` モジュールに置き、新規依存を追加しない（NFR2）。
- テスト名はリポジトリの命名規約に従う（NFR3）。
- 共有フィクスチャを持たず、並列実行で安全であること（NFR4）。
- アサーションは観測可能な契約に対して行う（NFR5）。
- `--no-default-features` のコンパイルに影響しない（NFR7）。
- `dragging == false` と「選択が publish される」のアサーションは、ユニットテストでは
  構築できない実際の `WindowHost` ではなく、ウィンドウ非依存のシーム
  （`consume_drag_termination`、`selection_publish_targets`、記録用の選択 sink ダブルを
  伴う `publish_to_targets`）に対して行う（A5）。

### 9.2 ビジネス上の制約

- プロダクション挙動の変更は範囲外（FR4、A3）。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの
`files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mouse-drag-latch-regression/**`
- `test-docs/mouse-drag-latch-regression/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、
`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、
`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。
生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式:
`test-docs/mouse-drag-latch-regression/{T}.tests.yaml`）。生成主体は
`implement-phase.md` を参照。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる
  （CONTAINED IN）必要がある。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| mouse-report のリセット／ジェスチャ所有権の境界への将来の変更が、恒久的にラッチされた `host.dragging` を黙って再導入する | — | FR1・FR2 のリグレッションテストで固定する |
| 意図的なフォーカス喪失セマンティクスが欠陥と誤認され、後の修正で削除される | — | FR3 でセマンティクスを固定し、FR5 で既存テストを無改変に保つ |
| テスト失敗時にどのメカニズムが退行したか判別できない | — | FR6 の通り、各テストが守るメカニズムをドキュメントコメントで名指しする |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が新規テストを含めてパスする。
- [ ] AC-2: `apply_outcome_with_held` の tracking-inactive リセットが全 gesture slot を
      クリアする形（`clear_unheld(held)` を `clear_all()` に戻す）に変更されたら失敗する
      テストが存在する。
- [ ] AC-3: ボタン経路（`pointer_routing.rs:850`）またはホイール経路
      （`mouse_report.rs:1235`）のいずれかが、ライブな held ボタン値を held 対応の
      apply エントリポイントに渡さなくなったら失敗するテストが存在する。
- [ ] AC-4: owner なしの左リリースが `drag_in_flight`（`mouse_report.rs:935`）を
      参照しなくなり無条件の `Disposition::Nothing` に戻ったら失敗するテストが存在する。
- [ ] AC-5: フォーカス喪失アームが、左ボタンがまだ物理的に押されているドラッグを
      終了するよう変更されたら失敗するテストが存在し、かつ左ボタンが押されていない
      ドラッグを終了しなくなったら失敗するテストが存在する。
- [ ] AC-6: 本フィーチャーの `git diff` が、テストモジュール以外の
      `src-tauri/src/` 配下のプロダクション挙動に触れていない（FR4）。
- [ ] AC-7: `should_terminate_drag_terminates_only_when_in_flight_and_left_not_held` と
      `focus_loss_arm_never_calls_the_fold_click_toggle` が無変更のままパスする。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1 `wheel_notch_during_left_drag_keeps_the_held_gesture_and_the_release_still_publishes`
      （対象要件: FR1 / 対象AC: AC-2, AC-3）:
      `MouseReportRecords` を `gesture_owner.record_press(Left, GestureOwner::Local)` で
      シードする。`decide_wheel_event` でトラッキング非アクティブのホイールイベントを
      決定し、`apply_wheel_report_step(.., HeldButtons { left: true, .. })` で適用して
      `records.gesture_owner.peek(Left) == Some(GestureOwner::Local)` をアサートする
      （リセットは `clear_all` ではなく `clear_unheld` を実行していなければならない）。
      続いて `decide_button_event(Release, Left, drag_in_flight: true)` が
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` であることを
      アサートする。最後に
      `consume_drag_termination(&mut dragging=true, &mut pending_anchor, Some(text), copy_on_select)`
      を駆動し、`dragging == false` と、`tests.rs` にすでにある記録用の選択 sink ダブルに
      対する `publish_to_targets` 経由での `targets.primary == true` をアサートする。
- [ ] TS-2 `second_button_press_during_left_drag_keeps_the_held_gesture_and_the_release_still_publishes`
      （対象要件: FR2 / 対象AC: AC-2, AC-3）:
      シードは TS-1 と同じで、間に挟まるイベントを
      `apply_outcome_with_held(.., HeldButtons { left: true, .. })` 経由で適用した
      `decide_button_event(Press, Middle|Right)` にする。`Left` スロットが生き残ること、
      続く左リリースが publish アームを名指すこと、`dragging == false` であること、
      PRIMARY に書かれることをアサートする。
- [ ] TS-3 `no_owner_left_release_with_drag_in_flight_still_completes_the_selection`
      （対象要件: FR1, FR2 / 対象AC: AC-4）:
      3 つ目のメカニズムの直接ガード。空の records に対する
      `decide_button_event(Release, Left)` は `drag_in_flight: true` で
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` を返し、
      同じ呼び出しが `drag_in_flight: false` では `Disposition::Nothing` を返し、
      `drag_in_flight: true` の middle / right も `Disposition::Nothing` を返さねばならない。
- [ ] TS-4 `focus_loss_with_left_held_preserves_the_drag_and_the_later_release_terminates_it`
      （対象要件: FR3, FR5 / 対象AC: AC-5）:
      `should_terminate_drag(true, true) == false` をアサートし、続いて（フォーカス喪失が
      実行する）`mouse_report::clear_all` で空にした records に対し `drag_in_flight: true` で
      後続の左リリースを駆動し、依然として publish アームを名指すことをアサートする。
      `consume_drag_termination` を実行し、`dragging == false` と PRIMARY への書き込みを
      アサートする。ドキュメントコメントには、このアームが意図的であり欠陥ではないことを記す。
- [ ] TS-5 `focus_loss_with_left_not_held_terminates_the_drag_immediately`
      （対象要件: FR3 / 対象AC: AC-5）:
      `should_terminate_drag(true, false) == true` をアサートし、続いてフォーカス喪失アームの
      `publish_local_drag` が行うように `consume_drag_termination` を駆動し、
      `dragging == false`、pending anchor が消費されること、PRIMARY への書き込みを
      アサートする。
- [ ] TS-M1 `manual: reproduce the original report's steps`
      （対象要件: FR4 / 対象AC: AC-6）:
      手動（E2E ハーネスは存在しない — `test/README.md`: 「E2E Tests: None at the moment」）。
      リリースビルドを起動し、グリッド上で左ドラッグし、ホイールを 1 ノッチ回す
      （または 2 つ目のボタンを押す）、リリースし、ポインタ移動が選択を延長しなくなっていること、
      および選択が PRIMARY に届くことを確認する。プロダクションの修正が本フィーチャーより
      前に入っているため、ベースリビジョンの時点ですでにパスすることが期待される。

## 13. 用語定義

該当なし。

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1: タスク記述の「期待する挙動」の項目「フォーカス喪失時は
      `mouse_report::clear_all` と併せて `host.dragging` と pending anchor も落とす」
      （フォーカス喪失時の無条件終了）は、意図的に採用しない。
      `should_terminate_drag` の `!left_held` ゲート（`pointer_routing.rs:492-494`）は
      `Focused(false)` アーム（`event_loop.rs`、約 260 行目）で合成されており、意図的な
      ものである。フォーカス喪失時にまだ物理的に押されている左ボタンは、そのリリースを
      引き続きウィンドウに届け、通常のリリース経路でドラッグを終了する。これは
      `tests.rs:1938` で固定され、
      `feature-docs/focus-loss-drag-termination/tasks/task0001.md` でスコープ外として
      文書化されている。Codex のセカンドオピニオン相談を経て採用した
      （`phase-state/batch-audit.yaml` に記録）。（可逆）
- [x] A2: 報告書が名指しした最初の 3 つのメカニズムは、ベースリビジョン
      （main, 8c3b4cce）ですでに修正済みである。`apply_outcome_with_held` は
      tracking-inactive リセット時に押されていない gesture slot のみをクリアする
      （`mouse_report.rs:1164-1175`）。ボタン経路（`pointer_routing.rs:850`）と
      ホイール経路（`mouse_report.rs:1235`）の両方が held 対応のエントリポイントを呼ぶ。
      owner なしの左リリースは `drag_in_flight` でゲートされている
      （`mouse_report.rs:935`、収集は `pointer_routing.rs:821`）。（可逆）
- [x] A3: 本フィーチャーのスコープはリグレッションテストのみであり、プロダクション挙動の
      変更はスコープ外。したがって完了条件「再現手順で現象が起きない」はベースリビジョンの
      時点ですでに満たされている。（可逆）
- [x] A4: 新規テストは新しいファイルではなく既存の
      `src-tauri/src/window_host/tests.rs` モジュールに追加する。リポジトリのインライン
      `#[cfg(test)]` 規約（`test/README.md`）に合わせる。（可逆）
- [x] A5: `dragging == false` と「選択が publish される」のアサーションは、ユニットテストでは
      構築できない実際の `WindowHost` ではなく、ウィンドウ非依存のシーム
      （`consume_drag_termination`、`selection_publish_targets`、記録用の選択 sink ダブルを
      伴う `publish_to_targets`）に対して行う。（可逆）
- [x] A6: 報告書の行番号参照はベースリビジョンに対して古い。SPEC およびタスクドキュメントは
      報告書の行番号ではなくシンボル名を引用する。（可逆）

### 14.2 未確認・保留事項

なし（すべての機能要件・非機能要件が `status: ok`）。

## 15. 参考資料

- `src-tauri/src/window_host/tests.rs`: 既存のシームレベルテストと新規テストの追加先
- `src-tauri/src/window_host/mouse_report.rs`: リセット／held 対応適用／owner なし左リリース
- `src-tauri/src/window_host/pointer_routing.rs`: ボタン経路と `should_terminate_drag`
- `src-tauri/src/window_host/event_loop.rs`: `WindowEvent::Focused(false)` アーム
- `test/README.md`: Test File Organization / Test Naming Conventions / Test Structure / E2E Tests
- `feature-docs/focus-loss-drag-termination/tasks/task0001.md`: フォーカス喪失時の
  無条件終了がスコープ外である旨の記録
- `phase-state/batch-audit.yaml`: A1 の Codex セカンドオピニオン相談の記録
