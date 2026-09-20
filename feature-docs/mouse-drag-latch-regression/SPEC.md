# Feature: mouse-drag-latch-regression

## Overview（概要）

修正済みのゴーストドラッグ・ラッチ挙動を、自動リグレッションテストで固定する
フィーチャー。報告書が名指しした 3 つのメカニズムはベースリビジョン（main, 8c3b4cce）
ですでに修正済みであり、本フィーチャーはそれらと、フォーカス喪失時の意図的な
セマンティクスとを、素の `mouse_report` / `pointer_routing` シーム上のユニットテストで
固定する。プロダクション挙動の変更はない。要件の根拠は
`feature-docs/mouse-drag-latch-regression/REQUIREMENTS.md` を参照。

## Objectives（目的）

- mouse-report のリセットとジェスチャ所有権の境界に対する将来の変更が、恒久的に
  ラッチされた `host.dragging` を黙って再導入できないようにする。
- 意図的なフォーカス喪失セマンティクス（`should_terminate_drag` の `!left_held` ゲート）を
  明示的にテストで固定し、後の修正で欠陥と誤認されて削除されないようにする。
- プロダクション挙動の変更をゼロにする。成果物はテストカバレッジがすべて。

## Technical Requirements（技術要件）

### Functional Requirements（機能要件）

- **FR1: Wheel notch during a live left drag must not strand the drag** —
  リグレッションテストが、winit ウィンドウ・GPU・PTY・ターミナルモード型を用いない素の
  `mouse_report` / `pointer_routing` シーム上で次のシーケンスを駆動する。トラッキング
  非アクティブでグリッド上を左押下し `GestureOwner::Local` を `Left` に記録する →
  `HeldButtons { left: true, .. }` を伴って `apply_wheel_report_step` 経由でホイールノッチを
  1 つ適用する → 左リリース。ホイールノッチの tracking-inactive リセットが、まだ押されている
  `Left` の gesture slot をクリアしないこと、リリースの disposition が
  `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` であること、
  結果のドラッグ終了で `dragging == false` になること、選択の publish が PRIMARY に届くことを
  アサートする。
- **FR2: A second button press during a live left drag must not strand the drag** —
  リグレッションテストが次を駆動する。左押下で `GestureOwner::Local` を `Left` に記録する →
  `HeldButtons { left: true, .. }` を伴って `apply_outcome_with_held` 経由で 2 つ目
  （middle または right）のボタン押下を適用する → 左リリース。2 つ目の押下の
  tracking-inactive リセットが押されている `Left` の gesture slot をそのまま残すこと、
  左リリースが依然として `LocalArm::CompleteSelectionAndPublishToPrimary` を名指すこと、
  その後 `dragging == false` であること、選択が PRIMARY に publish されることをアサートする。
- **FR3: Focus loss during a live left drag is pinned to the current intended semantics** —
  リグレッションテストが、`WindowEvent::Focused(false)` のサイトで合成される
  `should_terminate_drag(drag_in_flight, left_held)` の両アームを固定する。
  (a) フォーカス喪失時に左が物理的に押されている場合、ドラッグは保持され（フォーカス喪失
  時点では終了しない）、後続の左リリースが終了者となり、`dragging == false` で終わって選択が
  publish される。(b) フォーカス喪失時に左が押されていない場合、ドラッグはフォーカス喪失
  時点で即座に終了し、`dragging == false` で終わって選択が publish される。テストは
  フォーカス喪失時の無条件終了をアサートしてはならない。
- **FR4: No production behaviour change** —
  `src-tauri/src/window_host/` 配下のファイルは、テストモジュール以外挙動を変更しない。
  `mouse_report.rs`、`pointer_routing.rs`、`event_loop.rs` のプロダクションコードは
  ベースリビジョンのまま据え置く。報告書が名指しした 3 つのメカニズム
  （`apply_outcome_with_held` の `clear_unheld`、ボタン経路とホイール経路の両方における
  held 対応エントリポイント、drag-in-flight でゲートされた owner なしの左リリース）は
  そこですでに修正済みである。
- **FR5: The existing focus-loss gate test stays green and unmodified** —
  `should_terminate_drag_terminates_only_when_in_flight_and_left_not_held`
  （`src-tauri/src/window_host/tests.rs:1938`）と
  `focus_loss_arm_never_calls_the_fold_click_toggle`（同ファイル、約 1975 行目、
  `event_loop.rs` に対するソーススキャンの needle）は編集も弱化もしない。FR3 の新規テストは
  それらの上に重ねる追加カバレッジである。
- **FR6: Each new test names the mechanism it guards** —
  すべての新規テストは、その失敗がどのメカニズムの退行を示すのか
  （reset-clears-held-gesture-slot、held-unaware apply entry point、
  ungated no-owner left release、focus-loss gate inversion）を述べたドキュメントコメントを
  持ち、将来の失敗を本調査の再導出なしに診断できるようにする。

### Non-Functional Requirements（非機能要件）

- **NFR1 - Bare unit-test seams only:** テストは `WindowHost`・winit ウィンドウ・
  wgpu サーフェス・PTY・ターミナルモードのいずれの型も名指さない。
  `src-tauri/src/window_host/tests.rs` の既存のシームレベルテスト
  （`decide_button_event` / `decide_wheel_event` / `apply_outcome_with_held` /
  `apply_wheel_report_step` / `consume_drag_termination` / `should_terminate_drag` /
  `drag_in_flight`）に揃える。
- **NFR2 - Inline test module, no new dependency:** テストは既存の `#[cfg(test)]`
  モジュールファイル `src-tauri/src/window_host/tests.rs` にインラインで置く
  （`test/README.md` の「Test File Organization」）。新しい結合テストのコンパイル単位も、
  新しいテストフレームワーク依存（proptest、criterion）も追加しない。
- **NFR3 - Repository test-naming convention:** テスト名はリポジトリで支配的な
  `<subject>_<scenario>_<expected>` パターンに従う（`test/README.md` の
  「Test Naming Conventions」）。
- **NFR4 - Deterministic and parallel-safe:** 各テストは自前の `MouseReportRecords` /
  `GestureOwnership` / プレーンな状態を明示的に構築し、共有のグローバルフィクスチャを
  持たない。したがってスイートは `--test-threads=1` を必要としない。
- **NFR5 - Assert observable contracts:** アサーションは観測可能な契約
  （disposition の値、gesture slot の `peek`、`dragging`、publish 先 / sink の呼び出し）に
  対して行い、内部専用の状態には行わない（`test/README.md` の「Test Structure」）。
- **NFR6 - Negligible runtime cost:** 実行コストは無視できる程度であり、`#[ignore]` による
  ゲートは不要。
- **NFR7 - Feature gating unaffected:** 新規テストは GUI 専用の `window_host` モジュール
  配下に置かれるため、`--no-default-features` のコンパイルに影響しない。

## Implementation Approach（実装方針）

### Architecture（対象シーム）

テストは以下のウィンドウ非依存シームだけを直接駆動する（NFR1）。

```
decide_button_event / decide_wheel_event   -- 入力イベント -> Disposition の決定
apply_outcome_with_held                    -- ボタン経路の held 対応 apply
apply_wheel_report_step                    -- ホイール経路の held 対応 apply
MouseReportRecords.gesture_owner (peek / record_press)
should_terminate_drag(drag_in_flight, left_held)
consume_drag_termination                   -- dragging / pending anchor の消費
selection_publish_targets / publish_to_targets  -- 記録用の選択 sink ダブル
```

`dragging == false` と「選択が publish される」のアサーションは、ユニットテストでは
構築できない実際の `WindowHost` ではなく、これらのシームに対して行う（A5）。

### Data Flow（テストが駆動するシーケンス）

```
FR1: press(Left) -> record GestureOwner::Local
     -> wheel notch via apply_wheel_report_step(HeldButtons{left:true})
     -> peek(Left) == Some(GestureOwner::Local)
     -> release(Left, drag_in_flight=true) -> CompleteSelectionAndPublishToPrimary
     -> consume_drag_termination -> dragging == false, PRIMARY published

FR2: press(Left) -> record GestureOwner::Local
     -> press(Middle|Right) via apply_outcome_with_held(HeldButtons{left:true})
     -> peek(Left) survives
     -> release(Left) -> CompleteSelectionAndPublishToPrimary
     -> consume_drag_termination -> dragging == false, PRIMARY published

FR3(a): should_terminate_drag(true, true) == false
     -> clear_all -> release(Left, drag_in_flight=true) -> publish arm
     -> consume_drag_termination -> dragging == false, PRIMARY published
FR3(b): should_terminate_drag(true, false) == true
     -> consume_drag_termination -> dragging == false, anchor consumed, PRIMARY published
```

### Dependencies（依存関係）

**Internal Dependencies:**
- `src-tauri/src/window_host/mouse_report.rs`: リセット、held 対応 apply、
  owner なし左リリースの `drag_in_flight` ゲート（プロダクションは無変更、FR4）
- `src-tauri/src/window_host/pointer_routing.rs`: ボタン経路と `should_terminate_drag`
  （プロダクションは無変更、FR4）
- `src-tauri/src/window_host/event_loop.rs`: `WindowEvent::Focused(false)` アーム
  （プロダクションは無変更、FR4）
- `src-tauri/src/window_host/tests.rs`: 既存のシームレベルテストと記録用の選択 sink ダブル

**External Dependencies:**
- なし（NFR2: 新しいテストフレームワーク依存を追加しない）

### File Structure（変更対象ファイル）

```
src-tauri/src/window_host/
└── tests.rs        # 新規リグレッションテストの追加先（唯一の変更対象、A4 / NFR2）
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mouse-drag-latch-regression/**`
- `test-docs/mouse-drag-latch-regression/**`

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
it.

## Test Scenarios（テストシナリオ）

### Unit Tests

- [ ] **TS-1** `wheel_notch_during_left_drag_keeps_the_held_gesture_and_the_release_still_publishes`
      — 要件: FR1 / AC: AC-2, AC-3。
      `MouseReportRecords` を `gesture_owner.record_press(Left, GestureOwner::Local)` で
      シードする。`decide_wheel_event` でトラッキング非アクティブのホイールイベントを決定し、
      `apply_wheel_report_step(.., HeldButtons { left: true, .. })` で適用して
      `records.gesture_owner.peek(Left) == Some(GestureOwner::Local)` をアサートする
      （リセットは `clear_all` ではなく `clear_unheld` を実行していなければならない）。続いて
      `decide_button_event(Release, Left, drag_in_flight: true)` が
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` であることを
      アサートする。最後に
      `consume_drag_termination(&mut dragging=true, &mut pending_anchor, Some(text), copy_on_select)`
      を駆動し、`dragging == false` と、`tests.rs` にすでにある記録用の選択 sink ダブルに対する
      `publish_to_targets` 経由での `targets.primary == true` をアサートする。
- [ ] **TS-2** `second_button_press_during_left_drag_keeps_the_held_gesture_and_the_release_still_publishes`
      — 要件: FR2 / AC: AC-2, AC-3。
      シードは TS-1 と同じで、間に挟まるイベントは
      `apply_outcome_with_held(.., HeldButtons { left: true, .. })` 経由で適用した
      `decide_button_event(Press, Middle|Right)`。`Left` スロットが生き残ること、続く左リリースが
      publish アームを名指すこと、`dragging == false` であること、PRIMARY に書かれることを
      アサートする。
- [ ] **TS-3** `no_owner_left_release_with_drag_in_flight_still_completes_the_selection`
      — 要件: FR1, FR2 / AC: AC-4。
      3 つ目のメカニズムの直接ガード。空の records に対する
      `decide_button_event(Release, Left)` は `drag_in_flight: true` で
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` を返し、同じ呼び出しが
      `drag_in_flight: false` では `Disposition::Nothing` を返し、`drag_in_flight: true` の
      middle / right も `Disposition::Nothing` を返さねばならない。
- [ ] **TS-4** `focus_loss_with_left_held_preserves_the_drag_and_the_later_release_terminates_it`
      — 要件: FR3, FR5 / AC: AC-5。
      `should_terminate_drag(true, true) == false` をアサートし、続いて（フォーカス喪失が実行する）
      `mouse_report::clear_all` で空にした records に対し `drag_in_flight: true` で後続の
      左リリースを駆動し、依然として publish アームを名指すことをアサートする。
      `consume_drag_termination` を実行し、`dragging == false` と PRIMARY への書き込みを
      アサートする。ドキュメントコメントには、このアームが意図的であり欠陥ではないことを記す。
- [ ] **TS-5** `focus_loss_with_left_not_held_terminates_the_drag_immediately`
      — 要件: FR3 / AC: AC-5。
      `should_terminate_drag(true, false) == true` をアサートし、続いてフォーカス喪失アームの
      `publish_local_drag` が行うように `consume_drag_termination` を駆動し、
      `dragging == false`、pending anchor が消費されること、PRIMARY への書き込みをアサートする。

### Integration Tests

なし（NFR2: 新しい結合テストのコンパイル単位を追加しない）。

### E2E Tests

**Existing E2E tests**: None at the moment（`test/README.md`）
**Run command**: Not detected

- [ ] **TS-M1** `manual: reproduce the original report's steps` — 要件: FR4 / AC: AC-6。
      手動（E2E ハーネスは存在しない）。リリースビルドを起動し、グリッド上で左ドラッグし、
      ホイールを 1 ノッチ回す（または 2 つ目のボタンを押す）、リリースし、ポインタ移動が選択を
      延長しなくなっていること、および選択が PRIMARY に届くことを確認する。プロダクションの
      修正が本フィーチャーより前に入っているため、ベースリビジョンの時点ですでにパスすることが
      期待される。

### Edge Cases

- [ ] `drag_in_flight: false` での owner なし左リリースは `Disposition::Nothing`（TS-3）
- [ ] `drag_in_flight: true` での middle / right リリースも `Disposition::Nothing`（TS-3）
- [ ] フォーカス喪失時に左が押されている場合はドラッグを保持する（TS-4、FR3(a)）

### Performance Tests

なし（NFR6: 実行コストは無視できる程度であり、`#[ignore]` ゲートは不要）。

## Assumptions（確定した前提）

- **A1**: タスク記述の「期待する挙動」の項目「フォーカス喪失時は `mouse_report::clear_all` と
  併せて `host.dragging` と pending anchor も落とす」（フォーカス喪失時の無条件終了）は、
  意図的に採用しない。`should_terminate_drag` の `!left_held` ゲート
  （`pointer_routing.rs:492-494`）は `Focused(false)` アーム（`event_loop.rs`、約 260 行目）で
  合成されており意図的なものである。フォーカス喪失時にまだ物理的に押されている左ボタンは、
  そのリリースを引き続きウィンドウに届け、通常のリリース経路でドラッグを終了する。これは
  `tests.rs:1938` で固定され、`feature-docs/focus-loss-drag-termination/tasks/task0001.md` で
  スコープ外として文書化されている。Codex のセカンドオピニオン相談を経て採用した
  （`phase-state/batch-audit.yaml` に記録）。（可逆）
- **A2**: 報告書が名指しした最初の 3 つのメカニズムは、ベースリビジョン（main, 8c3b4cce）で
  すでに修正済み。`apply_outcome_with_held` は tracking-inactive リセット時に押されていない
  gesture slot のみをクリアする（`mouse_report.rs:1164-1175`）。ボタン経路
  （`pointer_routing.rs:850`）とホイール経路（`mouse_report.rs:1235`）の両方が held 対応の
  エントリポイントを呼ぶ。owner なしの左リリースは `drag_in_flight` でゲートされている
  （`mouse_report.rs:935`、収集は `pointer_routing.rs:821`）。（可逆）
- **A3**: 本フィーチャーのスコープはリグレッションテストのみであり、プロダクション挙動の変更は
  スコープ外。したがって完了条件「再現手順で現象が起きない」はベースリビジョンの時点で
  すでに満たされている。（可逆）
- **A4**: 新規テストは新しいファイルではなく既存の `src-tauri/src/window_host/tests.rs`
  モジュールに追加する。リポジトリのインライン `#[cfg(test)]` 規約（`test/README.md`）に
  合わせる。（可逆）
- **A5**: `dragging == false` と「選択が publish される」のアサーションは、ユニットテストでは
  構築できない実際の `WindowHost` ではなく、ウィンドウ非依存のシーム
  （`consume_drag_termination`、`selection_publish_targets`、記録用の選択 sink ダブルを伴う
  `publish_to_targets`）に対して行う。（可逆）
- **A6**: 報告書の行番号参照はベースリビジョンに対して古い。SPEC およびタスクドキュメントは
  報告書の行番号ではなくシンボル名を引用する。（可逆）

## Success Criteria（成功基準）

- [ ] AC-1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      が新規テストを含めてパスする。
- [ ] AC-2: `apply_outcome_with_held` の tracking-inactive リセットが全 gesture slot を
      クリアする形（`clear_unheld(held)` を `clear_all()` に戻す）に変更されたら失敗する
      テストが存在する。
- [ ] AC-3: ボタン経路（`pointer_routing.rs:850`）またはホイール経路（`mouse_report.rs:1235`）の
      いずれかが、ライブな held ボタン値を held 対応の apply エントリポイントに渡さなくなったら
      失敗するテストが存在する。
- [ ] AC-4: owner なしの左リリースが `drag_in_flight`（`mouse_report.rs:935`）を参照しなくなり
      無条件の `Disposition::Nothing` に戻ったら失敗するテストが存在する。
- [ ] AC-5: フォーカス喪失アームが、左ボタンがまだ物理的に押されているドラッグを終了するよう
      変更されたら失敗するテストが存在し、かつ左ボタンが押されていないドラッグを終了しなく
      なったら失敗するテストが存在する。
- [ ] AC-6: 本フィーチャーの `git diff` が、テストモジュール以外の `src-tauri/src/` 配下の
      プロダクション挙動に触れていない（FR4）。
- [ ] AC-7: `should_terminate_drag_terminates_only_when_in_flight_and_left_not_held` と
      `focus_loss_arm_never_calls_the_fold_click_toggle` が無変更のままパスする。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（FR1-FR6、NFR1-NFR7 はすべて `status: ok`）。

## Design Step

スキップ。本フィーチャーは Rust のユニットテストのみを追加し、UI 表面・視覚的変更・
新しいユーザー操作・デザイントークンの利用を一切導入しないため、デザインステップが
決定すべきものがない。

## References

- 要件定義書: `feature-docs/mouse-drag-latch-regression/REQUIREMENTS.md`
- `src-tauri/src/window_host/tests.rs`
- `src-tauri/src/window_host/mouse_report.rs`
- `src-tauri/src/window_host/pointer_routing.rs`
- `src-tauri/src/window_host/event_loop.rs`
- `test/README.md`
- `feature-docs/focus-loss-drag-termination/tasks/task0001.md`
- `phase-state/batch-audit.yaml`
