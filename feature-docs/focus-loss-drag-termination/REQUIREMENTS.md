---
title: "focus-loss-drag-termination"
created_date: 2026-09-20
status: draft
---

# focus-loss-drag-termination - 要件定義書

## 1. 概要

### 1.1 背景

現在の `WindowEvent::Focused(false)` アーム（src-tauri/src/window_host/event_loop.rs:227-282）は、
ローカルの左選択ドラッグが進行中であれば無条件に終了させる
（`if local_drag_in_flight(host, &self.app) { publish_local_drag(...) }`、
event_loop.rs:253-255）。このため、利用者が左ボタンを押したまま物理的にドラッグを
続けている最中でも、通知・コンポジタの focus-follows-mouse・他ウィンドウの前面化などで
フォーカスが奪われるとドラッグが破棄される。

一方で、直前のフィーチャー mouse-report-reset-active-gesture が確立した
「`host.dragging` を true にするすべての経路に終端処理が保証される」という
境界づけ（bounded-state）の保証は維持する必要がある。この保証が崩れると、
ポインタ状態でゲートされた機能（`update_resize_hint`、`refresh_link_hover`、
PTY 出力によるリンク再検出）が恒久的に無効化される。

### 1.2 目的

- ウィンドウのフォーカス喪失が、利用者がまだ物理的に行っている左選択ドラッグを
  破壊しないようにする。左ボタンが押されている限り、ドラッグはフォーカスの
  奪取（通知、コンポジタの focus-follows-mouse、他ウィンドウの前面化）を生き延びる。
- 直前のフィーチャー（mouse-report-reset-active-gesture）が確立した境界づけられた状態の
  保証を維持する。`host.dragging` を true にするすべての経路に終端処理が保証され、
  ポインタ状態でゲートされた機能（`update_resize_hint`、`refresh_link_hover`、
  PTY 出力によるリンク再検出）が恒久的に無効化されることはない。
- 終端の判定を winit ウィンドウなしで表現・証明できる状態に保ち、判定層が持つ
  「素の `#[test]` からテストできる」不変条件を維持する。

### 1.3 スコープ

**対象**:

- `src-tauri/src/window_host/event_loop.rs` の `Focused(false)` アームにおける
  終端条件の絞り込み（FR1、FR2、FR3、FR7）。
- `src-tauri/src/window_host/pointer_routing.rs` への純粋な終端判定述語の追加（FR4）。
- `src-tauri/src/window_host/tests.rs` の構造テスト（ソース文字列一致）の更新（FR5）。
- 直前のフィーチャーの SPEC のうち、フォーカス喪失に関する 3 項目の supersede 記録（FR6）。

**対象外**:

- 追加の終端処理（`Focused(true)` チェック、リフォーカス後の初回モーションチェック、
  選択側の独立したプレス記録）の導入（FR8）。
- `mouse_report::clear_all` 自体とそのアーム内での位置（FR2）。
- `drag_in_flight` / `local_drag_in_flight` の既存シグネチャと意味（FR4）。
- `Focused(false)` アームのその他の挙動、および DEC マウスレポートのバイト列（FR7）。
- ホイールレポートのノッチ上限の緩和（NFR5）。

## 2. ビジネス要件

### 2.1 ビジネス目標

- ウィンドウのフォーカス喪失が、利用者がまだ物理的に行っている左選択ドラッグを破壊しない
  ようにする。左ボタンが押し下げられている限り、ドラッグはフォーカスの奪取（通知、
  コンポジタの focus-follows-mouse、他ウィンドウの前面化）を生き延びなければならない。
- 直前のフィーチャー（mouse-report-reset-active-gesture）が確立した境界づけられた状態の
  保証を維持する。`host.dragging` を true にするすべての経路が終端処理を保証されることで、
  ポインタ状態でゲートされた機能（`update_resize_hint`、`refresh_link_hover`、
  PTY 出力によるリンク再検出）が恒久的に無効化されることはない。
- 終端の判定を winit ウィンドウなしで表現でき、かつ証明できる状態に保つ。これにより
  判定層の「素の `#[test]` でテスト可能」という不変条件が維持される。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| （本要件分析に記載なし） | 対象ユーザーの定義は要件分析に含まれない |

### 2.3 期待される効果

- 左ボタンを押したままフォーカスを失っても、選択ドラッグが保持され、フォーカスが
  戻ったあとも継続できる。
- ボタンを離した状態でのフォーカス喪失は現在の挙動のまま変わらない。
- ポインタ状態でゲートされた機能が恒久的に無効化されない。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 左ボタン押下中のフォーカス喪失をドラッグが生き延びる | ターミナル利用者 | 高 |
| UC02 | 左ボタン非押下でのフォーカス喪失がドラッグを終了させる | ターミナル利用者 | 高 |
| UC03 | ドラッグ非進行時のフォーカス喪失 | ターミナル利用者 | 中 |

### 3.2 ユースケース詳細

#### UC01: 左ボタン押下中のフォーカス喪失をドラッグが生き延びる

**アクター**: ターミナル利用者

**事前条件**:

- ローカルの左選択ドラッグが進行中である。
- 左ボタンが押し下げられている（`host.mouse_report_held.left == true`）。

**基本フロー**:

1. グリッド上で左ボタンを押し、押したままドラッグして選択を伸ばす。
2. ボタンを押したまま、ウィンドウのフォーカスが失われる
   （通知、他ウィンドウの前面化など）。
3. フォーカスが戻り、ポインタ移動を続ける。
4. 左ボタンを離す。

**事後条件**:

- ステップ 2 の時点で PRIMARY / CLIPBOARD への書き込みも fold トグルも起きない。
  `host.dragging` は true のまま、`app.pending_selection_anchor` はそのまま残る（AC-1）。
- ステップ 3 で選択が引き続き伸びる。
- ステップ 4 の左リリースがドラッグを終了させる。`clear_all` によって
  ジェスチャ所有記録が空になっているため、このリリースは所有者なしリリースとなり、
  `decide_release` の `drag_in_flight` フォールバック（mouse_report.rs:935）が
  `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` を返す（AC-5）。

#### UC02: 左ボタン非押下でのフォーカス喪失がドラッグを終了させる

**アクター**: ターミナル利用者

**事前条件**:

- ローカルの左選択ドラッグが進行中である。
- 左ボタンは押し下げられていない。

**基本フロー**:

1. グリッド上で左ドラッグを行い、完了させる。
2. ウィンドウのフォーカスが失われる。

**事後条件**:

- `host.dragging == false`、`app.pending_selection_anchor == None`、実体化した選択が
  PRIMARY に公開される（`copy_on_select` が有効なときは CLIPBOARD にも）。
  これは現在の挙動と同一である（AC-2）。

#### UC03: ドラッグ非進行時のフォーカス喪失

**アクター**: ターミナル利用者

**事前条件**:

- ローカルの左ドラッグは進行していない。

**基本フロー**:

1. ウィンドウのフォーカスが失われる。

**事後条件**:

- 左ボタンの押下状態にかかわらず、選択状態および PRIMARY に対して純粋な no-op
  のままである（AC-3）。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | フォーカス喪失時の終端を左ボタン非押下でゲートする | `Focused(false)` アームの終端条件に「左ボタンが押されていないこと」を追加する | 高 |
| FR2 | 押下状態は `clear_all` がゼロクリアする前に読む | 判定に使う左押下値を `clear_all` 実行前に読み取る | 高 |
| FR3 | 押下中のフォーカス喪失はドラッグをそのまま保持する | 何も公開せず `dragging` と pending anchor を保持し、終端をリリース経路に委譲する | 高 |
| FR4 | 終端判定は純粋で winit 非依存な述語とする | 素の bool を取り結果を返す `drag_in_flight` の兄弟関数として抽出する | 高 |
| FR5 | 構造的なソース文字列表明を新しい合成形に更新する | needle 集合を新しい合成形に置き換え、`handle_fold_click` の否定表明を維持する | 高 |
| FR6 | 直前フィーチャーの FR3 / AC-3 / 状態表の行を supersede する | SPEC に supersede を明示し、陳腐化した行参照を更新する | 高 |
| FR7 | `Focused(false)` アームのその他は不変 | アームの残りの挙動をそのまま維持する | 高 |
| FR8 | 失われたリリース経路に追加の終端処理を導入しない | リリース経路の所有者なしフォールバックを唯一の終端処理とする | 高 |

### 4.2 機能詳細

#### FR1: フォーカス喪失時の終端を左ボタン非押下でゲートする

**説明**:
`WindowEvent::Focused(false)` のアーム（src-tauri/src/window_host/event_loop.rs:227-282）は、
その時点で左ボタンが押されていない場合に限り、進行中のローカル左ドラッグを終了させる。
ここでの「終了」は現在と完全に同じ意味である。すなわち
`publish_local_drag(host, &mut self.app)`（event_loop.rs:254）が実行され、
`host.dragging` がクリアされ、`app.pending_selection_anchor` が消費され、実体化した選択が
PRIMARY に公開される（`copy_on_select` が有効なときは CLIPBOARD にも）。
現在の無条件な合成
`if local_drag_in_flight(host, &self.app) { publish_local_drag(...) }`
（event_loop.rs:253-255）は、左ボタンが押されていないことを追加で要求する合成に
置き換えられる。

#### FR2: 押下状態は `clear_all` がゼロクリアする前に読む

**説明**:
FR1 の判定に入力する左押下値は、
`mouse_report::clear_all(&mut host.mouse_report_gesture_owner, &mut host.mouse_report_held)`
（event_loop.rs:262-265）が実行される**前**に `host.mouse_report_held.left`
（素の `HeldButtons` 値、mouse_report.rs:471-483）から読み取る。`clear_all` のあとに
読むと常に `false` を観測することになり、FR1 が現在の無条件終端に退化する。
`clear_all` 自体と、アーム内におけるその位置は変更しない。

#### FR3: 押下中のフォーカス喪失はドラッグをそのまま保持する

**説明**:
ローカル左ドラッグが進行中で、かつ左ボタンが押されている状態でフォーカスを失った場合、
アームは何も公開しない。PRIMARY への書き込みも CLIPBOARD への書き込みも fold クリックの
トグルも起きない。`host.dragging` は true のまま、`app.pending_selection_anchor` は
そのまま残り、フォーカスが戻ればドラッグが継続する。

保持されたドラッグの終端は、`decide_release` のリリース経路にある所有者なしの
`drag_in_flight` フォールバック（mouse_report.rs:935: 所有者の記録がない左リリースは
`drag_in_flight` が true のときローカル完了アームを通る）に委譲される。これは
`clear_all` がジェスチャ所有記録を空にしているため、のちの左リリースが所有者なし
リリースになることで到達可能である。

#### FR4: 終端判定は純粋で winit 非依存な述語とする

**説明**:
FR1 の判定を、素の bool を対象とする純関数として抽出する。この関数は
`drag_in_flight`（src-tauri/src/window_host/pointer_routing.rs:364-366）の兄弟であり、
ドラッグ進行中シグナル（またはそれを構成する 2 つの bool）と左押下 bool を取り、
フォーカス喪失が終端するか否かを返す。winit 型・egui ウィンドウハンドル・GPU サーフェス・
PTY・`term_core` のモード型のいずれも受け取らず返さないため、素の `#[test]` から
駆動できる。既存の `drag_in_flight` と `local_drag_in_flight`
（pointer_routing.rs:364-377）は現在のシグネチャと意味を保ち、新しい述語はそれらを
置き換えるのではなく組み合わせて使う。

#### FR5: 構造的なソース文字列表明を新しい合成形に更新する

**説明**:
`focus_loss_arm_never_calls_the_fold_click_toggle`
（src-tauri/src/window_host/tests.rs:1931-1949）は現在、`event_loop.rs` のソースが
`"local_drag_in_flight(host, &self.app)"` と `"publish_local_drag(host, &mut self.app)"`
という文字列を含み、`"handle_fold_click"` を含まないことを表明している。FR1 がこの
アームの呼び出し合成を変えるため、needle 集合は単に追記するのではなく、**新しい合成形**
（新しい述語の名前と押下状態の読み取りを含む）を固定するように更新する。
`handle_fold_click` の否定表明はそのまま維持する。フォーカス喪失アームは今後も
fold クリックのトグルに到達してはならない。

#### FR6: 直前フィーチャーの FR3 / AC-3 / フォーカス喪失の状態表の行を supersede する

**説明**:
SPEC は、`feature-docs/mouse-report-reset-active-gesture/SPEC.md` の FR3
（"Focus loss terminates a live local drag"）、AC-3
（"Losing window focus mid-drag ends with host.dragging == false …"）、および
Error Handling の状態条件行 "Focus lost with a live left drag" を supersede することを、
名前を挙げて明示的に記録する。supersede 後、この 3 項目は「フォーカス喪失は、左ボタンが
押されていないときに限り、進行中のローカル左ドラッグを終了させる」と読む。

直前 SPEC の陳腐化した行参照（event_loop.rs:229-253 と :249、
pointer_routing.rs:361-406）は、現在のもの（event_loop.rs:227-282、:253-255、:262-265、
pointer_routing.rs:391-412 と :424-443）に更新する。直前 SPEC の FR1、FR2、FR4、FR5、FR6
およびその他のすべての受け入れ基準は supersede されず、引き続き有効である。

#### FR7: `Focused(false)` アームのその他は不変

**説明**:
アームの残りはバイト単位で現在の挙動を維持する。対象は
`self.app.window_focused = focused`、`notify_ime_focus`、`on_ime_focus_lost`、
`host.current_mods = Modifiers::default()`（event_loop.rs:238）、
`host.pointer_buttons_down = 0`（event_loop.rs:243）、
`mouse_report::clear_all`（event_loop.rs:262-265）、`host.update_link_cursor()`、
`Focused(true)` 分岐の `reset_blink_phase`、および末尾の `mark_full_redraw` /
`request_redraw` である。ドラッグ非進行時のフォーカス喪失は、選択状態および PRIMARY に
対して純粋な no-op のままとする。DEC マウスレポートのバイトは変わらない。

#### FR8: 失われたリリース経路に追加の終端処理を導入しない

**説明**:
`Focused(true)` チェック、リフォーカス後の初回モーションチェック、選択側の独立した
プレス記録のいずれも追加しない。FR3 で保持されたドラッグに対する終端処理は、リリース
経路の所有者なし `drag_in_flight` フォールバックただ 1 つとする。

根拠は前提 A2 として記録する。`Focused(true)` / 初回モーションのチェックは、
`clear_all` が既に `host.mouse_report_held` をゼロクリアしているため `left == false` を
無条件に観測し、現在の無条件終端に退化して、本フィーチャーが直そうとしているバグを
再導入するため機能しない。また選択側の独立したプレス記録は、まさに「リリースが失われる」
経路で `held = true` のまま残り続けるため、状態を増やすだけでカバレッジを得られない。

### 4.3 エラーケース・境界条件

| 条件 | 期待される扱い |
|------|----------------|
| ドラッグ進行中かつ左ボタン押下中のフォーカス喪失 | `host.dragging` は true のまま、`app.pending_selection_anchor` は `Some` のまま。PRIMARY / CLIPBOARD への書き込みなし、fold トグルなし（AC-1） |
| ドラッグ進行中かつ左ボタン非押下のフォーカス喪失 | `host.dragging == false`、`app.pending_selection_anchor == None`、実体化した選択を PRIMARY（`copy_on_select` 有効時は CLIPBOARD にも）へ公開。現在の挙動のまま（AC-2） |
| ドラッグ非進行時のフォーカス喪失 | 左ボタンの押下状態にかかわらず、選択状態および PRIMARY に対して純粋な no-op（AC-3） |
| 押下値を `clear_all` のあとに読んだ場合 | 常に `false` を観測し、FR1 が現在の無条件終端に退化する。よって読み取りは `clear_all` の前に行う（FR2） |
| 保持されたドラッグのその後の左リリース | `clear_all` により所有者記録が空のため所有者なしリリースとなり、`drag_in_flight == true` で `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` を返す（AC-5） |
| 左リリースが本当に失われた場合 | 即時の終端処理は存在せず、`host.dragging` は保持されたままになる。この残存リークは前提 A2 として受け入れる。劣化は境界づけられており自己修復的で、グリッド上の次の完全な左クリックが所有者なし左リリースを生み、mouse_report.rs:935 のフォールバックが `host.dragging` をクリアする |
| プレスイベントは記録されたがリリースが届かなかった場合 | `host.mouse_report_held.left` はイベント由来の記録であり物理デバイスの問い合わせではないため、ボタンが物理的に上がっていても `held = true` を報告しうる（前提 A1） |

## 5. 非機能要件

### 5.1 パフォーマンス要件

- NFR6: ホットパスのコストを増やさない。追加される処理は `Focused(false)` イベント
  ごとに `host.mouse_report_held.left` を 1 回 bool 読み取りするだけである。フォーカス
  遷移はホットパスではなく、フレームごと／モーションごとのコストは導入しない。

### 5.2 セキュリティ要件

- NFR5: 新たな PTY バイトを出さず、既存の上限も緩めない。変更はローカルのポインタ状態
  管理に限定され、PTY への追加バイト送出はない。ホイールレポートのノッチ上限
  （`bounded_wheel_report_duplicate` / `MAX_WHEEL_REPORT_NOTCHES`、pointer_routing.rs）は
  ドキュメント化されたセキュリティ特性であり、変更しない。ネットワーク面も永続データも
  WebView コンテンツも関与しない。

### 5.3 可用性要件

- 本要件分析に稼働率・障害復旧時間の記載はない。

### 5.4 保守性要件

- NFR1: 判定層はウィンドウ非依存を保つ。`src-tauri/src/window_host/mouse_report.rs` の
  いかなるシグネチャも、また新しい述語（FR4）のシグネチャも、winit 型・egui ウィンドウ
  ハンドル・GPU サーフェス・PTY・`term_core` のモード型を受け取らず返さない。押下状態は
  素の bool、または既存の素の `HeldButtons` 値（mouse_report.rs:471-483）として渡す。
  これはモジュールが明示している不変条件であり、すべてのユニットを素の `#[test]` から
  実行可能にしている根拠である。
- NFR2: 判定関数は純粋を保つ。新しい述語は素の入力に対する純関数であり、評価しても
  何も変更せず I/O も行わない。これは `drag_in_flight`（pointer_routing.rs:364-366）と
  同じである。状態の変更はすべて既存の `publish_local_drag` / `apply_outcome` の箇所に
  留め、述語は判定のみを行う。
- NFR3: テストスタイルはプロジェクトの規約に合わせる。新規テストは対象コードの隣にある
  既存のインライン `#[cfg(test)]` テストモジュール内の素の `#[test]` 関数とし
  （test/README.md "Test File Organization"）、`<subject>_<scenario>_<expected>` の命名
  （test/README.md "Test Naming Conventions"）で、共有グローバルフィクスチャを持たず
  テストごとに明示的に構築する。新しいテストフレームワーク依存は追加しない
  （test/README.md "Test Framework": proptest なし、criterion なし）。

### 5.5 互換性要件

- NFR4: ビルド面の不変性。CLI 専用ビルド
  （`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`）
  は引き続きコンパイルできる。触れるモジュールはすべて GUI ゲート下にある。Linux と
  Windows の双方を引き続きサポートし、プラットフォーム固有 API は導入しない
  （.claude/rules/core-architecture.md, "Platform support": Linux と Windows、macOS は対象外）。

## 6. UI/UX要件

### 6.1 画面設計要件

デザインステップはスキップされた。理由: ユーザーに見える表面の変更がない。本フィーチャーは
winit の `Focused(false)` アームにおける 1 つの boolean 条件を絞り込み、GUI 内部の Rust
モジュール 3 つ（event_loop.rs、pointer_routing.rs、tests.rs）に純粋な述語を抽出する
ものである。新しい UI 要素、デザイントークン、レイアウト・文言・色・インタラクションの
アフォーダンスの変更をいずれも導入しない。観測できる唯一の差分は、進行中のドラッグが
フォーカスの奪取を生き延びるようになることである。`doc/UI-DESIGN-GUIDELINES.yaml` と
その 2 つのミラー（`src-tauri/src/ui/md3.rs`、`src-tauri/web-shared/styles.css`）は
変更しないため、`ui::dialog::tests` のドリフトテストに影響はない。ゲート
`create-spec.design-step` は `decide_autonomously` で解決され、この推奨が採用された。

### 6.2 画面遷移

該当なし。

### 6.3 レスポンシブ対応

該当なし。

## 7. データ要件

該当なし。永続データモデルの変更はない。`host.dragging`、`host.mouse_report_held`、
`app.pending_selection_anchor` は現在の宣言（`src-tauri/src/window_host/mod.rs`、
`src-tauri/src/app.rs`）を保ち、本フィーチャーはフィールドを追加せず、型も変更しない
（前提 A5）。

## 8. 外部連携

該当なし。外部システムとの連携はない。

## 9. 制約条件

### 9.1 技術的制約

- 判定層はウィンドウ非依存を保つ（NFR1）。
- 新しい述語は純関数とし、状態変更を行わない（NFR2）。
- テストは既存のインライン `#[cfg(test)]` モジュール内の素の `#[test]` とし、新しい
  テストフレームワーク依存を追加しない（NFR3）。
- CLI 専用ビルドが引き続きコンパイルでき、Linux / Windows の双方を維持する（NFR4）。
- ホイールレポートのノッチ上限を弱めない（NFR5）。
- 押下値は `clear_all` の前に読む必要がある。あとに読むと常に `false` になる（FR2）。
- 新しい述語は `drag_in_flight`（pointer_routing.rs:364-366）の隣に `pub(super)` で置く。
  `mouse_report.rs` ではなく `pointer_routing.rs` である（前提 A3）。
- 構造テストの needle リスト（tests.rs:1939-1948）は追記ではなく置き換えが必要である
  （前提 A4）。
- 本プロジェクトに E2E 自動化は存在しないため、エンドツーエンドの確認はユーザー実行の
  手動シナリオ TS-7 による（前提 A6）。

### 9.2 ビジネス上の制約

- 直前のフィーチャーが確立した境界づけられた状態の保証を崩さない。

### 9.3 スケジュール制約

本要件分析に記載なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/focus-loss-drag-termination/**`
- `test-docs/focus-loss-drag-termination/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/{feature}/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

要件分析が立てた前提とその影響度を以下に示す。すべて可逆（reversible）である。

| 課題（前提） | 影響度 | 対応策 |
|------|--------|--------|
| A1: 「左ボタンが押されている」の定義 | 中 | ホスト側のイベント由来の押下記録 `host.mouse_report_held.left`（素の `HeldButtons` 値、mouse_report.rs:471-483）を用いる。物理デバイスへの問い合わせは行わない。プレスは記録されたがリリースが届かなかった場合、記録は物理的にボタンが上がっていても `held = true` を報告しうる |
| A2: 失われたリリース経路の残存リーク | 中 | 閉じずに受け入れる。候補となる 2 つの追加終端処理はいずれも機能しないため。劣化は境界づけられ自己修復的であり、次の完全な左クリックが mouse_report.rs:935 のフォールバックを走らせて `host.dragging` をクリアする。イベント由来でない終端処理については別途フォローアップタスクを起票する |
| A3: 新しい述語の配置 | 低 | `src-tauri/src/window_host/pointer_routing.rs` の `drag_in_flight`（:364-366）の兄弟として `pub(super)` 可視性で置く。既存のドラッグ進行中ペアがある場所であり、tests.rs が既にそこから import している場所（tests.rs:19）である |
| A4: 構造テストの needle リスト | 低 | tests.rs:1939-1948 の needle は追記ではなく置き換える。既存の needle は現在の合成の厳密なソース部分文字列であり、アームを書き換えると一致しなくなって古いエントリが失敗する。書き換え後のアームに実在するリテラルのみを残す |
| A6: E2E 自動化の不在 | 低 | エンドツーエンドの確認はユーザー実行の手動シナリオ TS-7 とする。本ディスパッチで解決済み E2E パスが 1 つもないこと（`resolved_input_paths.e2e` が空）で裏付けられる |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 押下値を `clear_all` のあとに読んでしまい、現在の無条件終端に退化する | 中 | 高 | FR2 で読み取り位置を固定し、AC-4 と TS-5 で確認する |
| 構造テストの needle を追記に留め、書き換え後のアームに存在しない文字列を残す | 中 | 中 | 前提 A4 と FR5 / AC-7 / TS-2 で置き換えを明示する |
| 左リリースが失われ `host.dragging` が保持されたままになる | 低 | 中 | 前提 A2 として受け入れる。自己修復経路（mouse_report.rs:935）が存在し、別途フォローアップタスクを起票する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: ローカル左ドラッグが進行中かつ左ボタンが押されている状態でフォーカスを
      失うと、`host.dragging` は true のまま、`app.pending_selection_anchor` は `Some` の
      まま、PRIMARY にも CLIPBOARD にも何も書き込まれず、fold トグルも発火しない。
      （FR1、FR3）
- [ ] AC-2: ローカル左ドラッグが進行中かつ左ボタンが押されて**いない**状態でフォーカスを
      失うと、`host.dragging` は false になり、`app.pending_selection_anchor` は `None` に
      なり、実体化した選択が PRIMARY に公開される（`copy_on_select` が有効なときは
      CLIPBOARD にも）。これは現在の挙動であり、変更しない。（FR1）
- [ ] AC-3: ローカルドラッグが進行していない状態でフォーカスを失った場合、左ボタンの
      押下状態にかかわらず、選択状態および PRIMARY に対して純粋な no-op である。
      （FR1、FR7）
- [ ] AC-4: AC-1 / AC-2 をゲートする左押下値は `mouse_report::clear_all` の実行前に
      記録されたものであり、アームの完了後は `host.mouse_report_held` が現在と同様に
      ゼロクリアされている。（FR2、FR7）
- [ ] AC-5: AC-1 で保持されたドラッグのその後の左リリースがドラッグを終了させる。
      所有者の記録がなく `drag_in_flight == true` の `decide_release(Left)` が
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` を返し、
      `host.dragging` がクリアされ pending anchor が消費される。（FR3、FR8）
- [ ] AC-6: 終端判定が素の bool に対する純関数として存在し、その完全な真理値表が
      winit ウィンドウを必要としない素の `#[test]` で表明されている。
      （FR4、NFR1、NFR2）
- [ ] AC-7: `src-tauri/src/window_host/tests.rs` の構造的なソース文字列表明が
      フォーカス喪失アームの新しい合成形を固定しており、なお `event_loop.rs` が
      `handle_fold_click` を含まないことを表明している。（FR5）
- [ ] AC-8: SPEC が、直前フィーチャーのどの項目を supersede するか（FR3、AC-3、
      "Focus lost with a live left drag" の状態表の行）を散文で述べ、陳腐化した行参照に
      代えて更新後の行参照を持っている。（FR6）
- [ ] AC-9: `src-tauri/src/window_host/` の既存テストは、FR5 / AC-7 で更新される構造
      テスト 1 件を唯一の例外として、すべて通り続ける。マウスレポートのバイト列、
      対象タブの選択、ホイールマトリクスの挙動はいずれも変わらない。（FR7、NFR5）
- [ ] AC-10: CLI 専用ビルドが引き続きコンパイルでき、判定層の新規・変更シグネチャに
      winit / egui / PTY / `term_core` の型が現れない。（NFR1、NFR4）

### 11.2 KPI

本要件分析に KPI の記載はない。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 境界値（TS-1、unit）: 新しい純粋な終端述語の真理値表。（ドラッグ進行中、左押下）の
      すべての組み合わせについて返る判定を表明する。終端するのはドラッグ進行中が true
      かつ左押下が false のときのみ。素の `#[test]` で winit ウィンドウを使わず、
      `drag_in_flight_is_true_whenever_either_input_is_true`（tests.rs:1915-1921）と
      同じスタイルの兄弟とする。（AC-6、FR4、NFR1、NFR2）
- [ ] 構造（TS-2、unit）: `focus_loss_arm_never_calls_the_fold_click_toggle`
      （tests.rs:1931-1949）を拡張する。`include_str!("event_loop.rs")` の needle を、
      アームに実際に綴られている新しい合成形（述語呼び出しと押下状態の読み取り）へ
      置き換え、`handle_fold_click` の否定表明を維持する。前方一致で陳腐なまま通って
      しまう needle にしないよう注意する。（AC-7、FR5）
- [ ] 正常系（TS-3、unit）: 所有者なし左リリースのフォールバックが保持されたドラッグを
      終端する。Left スロットが `None` のレコードに対する `decide_release(Left)` が
      `drag_in_flight = true` で
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` を返し、
      `drag_in_flight = false` では `Disposition::Nothing` のままであること。これは
      `no_owner_left_release_completes_selection_only_when_drag_in_flight`
      （mouse_report.rs:3144-3162）で既にカバーされており、本シナリオはそのテストが
      変更なしで通り続けることを表明し、本フィーチャーの FR3 の委譲との関係を示す
      コメントを追加する。（AC-5、FR3、FR8）
- [ ] 正常系・異常系（TS-4、unit）: 抽出した述語を AC-1 / AC-2 / AC-3 が記述する 3 つの
      状態形（進行中＋押下中、進行中＋非押下、非進行で押下値の両方）で駆動し、判定が
      一致することを表明する。エンドツーエンドのアームは winit ウィンドウなしでは
      駆動できないため、アームがこの述語に忠実であることは振る舞いではなく TS-2 の
      構造テストで固定する。（AC-1、AC-2、AC-3、FR1、FR2）
- [ ] 回帰（TS-5、unit）: `mouse_report::clear_all` が引き続きジェスチャ所有記録と
      押下記録の双方を空にし、クリア後のレコードから下した判定が何も報告せずローカル
      アームも通らないこと。既存の
      `focus_loss_clear_all_empties_gesture_and_held_records_so_the_next_decision_starts_fresh`
      （tests.rs:1835-1904）と
      `ac2_clear_all_empties_gesture_and_held_button_records`
      （mouse_report.rs:2026-2040）が変更なしで通る。（AC-4、AC-9、FR7）
- [ ] ビルド（TS-6、build）:
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      が成功し、CLI 専用のフィーチャーゲートが保たれていることを確認する。
      （AC-10、NFR4）
- [ ] 手動（TS-7、manual）: リリースバイナリに対するユーザー実行の再現。(a) グリッド上で
      左ドラッグを開始し、ボタンを押したままウィンドウのフォーカスを失わせ（他ウィンドウの
      前面化／通知の発生）、フォーカスを戻して移動を続ける。選択が伸び続けており、
      リリースまで PRIMARY が変わらないことを確認する。(b) 左ドラッグを開始して完了させ、
      ボタンを上げた状態でフォーカスを失わせる。挙動は現在どおり。本プロジェクトに E2E
      自動化は存在しない（test/README.md "E2E Tests": none at the moment）。
      （AC-1、AC-2）
- [ ] セキュリティ: PTY への追加バイト送出がなく、ホイールレポートのノッチ上限が
      維持されている（NFR5）。
- [ ] パフォーマンス: 追加コストは `Focused(false)` イベントごとの bool 読み取り 1 回のみ
      であり、フレームごと／モーションごとのコストは導入されない（NFR6）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| フォーカス喪失アーム | `src-tauri/src/window_host/event_loop.rs:227-282` の `WindowEvent::Focused(false)` を処理する分岐 |
| ドラッグ進行中（drag in flight） | `drag_in_flight`（pointer_routing.rs:364-366）が真を返す状態 |
| ローカルドラッグ進行中 | `local_drag_in_flight`（pointer_routing.rs:364-377）が真を返す状態 |
| 終端（termination） | `publish_local_drag(host, &mut self.app)`（event_loop.rs:254）が走り、`host.dragging` がクリアされ、`app.pending_selection_anchor` が消費され、実体化した選択が PRIMARY（`copy_on_select` 有効時は CLIPBOARD にも）へ公開されること |
| 押下記録（held record） | `host.mouse_report_held`（素の `HeldButtons` 値、mouse_report.rs:471-483）。イベント由来であり物理デバイスの状態問い合わせではない（前提 A1） |
| 所有者なしリリース | ジェスチャ所有記録にそのボタンの所有者が記録されていない状態でのリリース。`decide_release` の `drag_in_flight` フォールバック（mouse_report.rs:935）が担当する |
| PRIMARY | X11 の PRIMARY セレクション |

## 14. 確認事項

### 14.1 確認済み事項

create-spec フェーズで解決されたゲートと、要件分析が確定した前提を記録する。

解決済みゲート:

- [x] requirement.fr3-termination-scope: FR3 を「左ボタンが物理的に押されていないときのみ
      適用する」に絞る。押下中は何も公開せず `host.dragging` と pending anchor を保持し、
      終端をリリース経路の所有者なし `drag_in_flight` フォールバックに委譲する。この
      SPEC は直前フィーチャーの FR3 / AC-3 / 状態表の行を supersede する。
- [x] edge-case.release-never-arrives: 追加の終端処理を導入しない。リリース経路の
      所有者なし `drag_in_flight` フォールバックが保証された終端処理である。失われた
      リリースの残存リークは、イベント由来のチェックで閉じるのではなく、別のフォロー
      アップタスクとして起票する。
- [x] testing.window-free-termination-predicate: 終端判定を素の bool に対する純粋で
      winit 非依存な述語として抽出し、その真理値表を素の `#[test]` でカバーし、既存の
      ソース文字列の構造表明を新しい合成形へ更新する。
- [x] design.design-step-decision: 要件分析の推奨を採用し、デザインステップをスキップ
      する。

要件分析が確定した前提:

- [x] A1（影響度: 中、可逆）: 「左ボタンが押されている」とは、ホスト側のイベント由来の
      押下記録 `host.mouse_report_held.left`（素の `HeldButtons` 値、
      mouse_report.rs:471-483）を指し、物理デバイスへの問い合わせではない。
      requirement.fr3-termination-scope への回答に付随する Codex の留保を引き継ぐ。
      プレスイベントが記録されたのにそのリリースが届かなかった場合、記録は物理的に
      ボタンが上がっていても `held = true` を報告しうる。
- [x] A2（影響度: 中、可逆）: 失われたリリースによる残存リークは、閉じずに受け入れる。
      左リリースが本当に失われた場合、FR3 は即時の終端処理なしに `host.dragging` を
      保持する。これを受け入れるのは、候補となる 2 つの追加終端処理が機能しないため
      である。(i) `Focused(true)` またはリフォーカス後の初回モーションのチェックは、
      `clear_all`（event_loop.rs:262-265）が既にゼロクリアした `host.mouse_report_held`
      を読むため `left == false` を無条件に観測し、現在の無条件終端に退化してバグを
      再導入する。(ii) 選択側の独立したプレス記録は、まさに「リリースが失われる」経路で
      `held = true` のまま残り続け、状態を増やすだけでカバレッジを得られない。劣化は
      境界づけられ自己修復的である。グリッド上の次の完全な左クリックが
      `drag_in_flight == true` の所有者なし左リリースを生み、mouse_report.rs:935 の
      フォールバックが走って `host.dragging` をクリアする。イベント由来でない終端処理
      については別途フォローアップタスクを起票する。
- [x] A3（影響度: 低、可逆）: 新しい述語は `mouse_report.rs` ではなく
      `src-tauri/src/window_host/pointer_routing.rs` の `drag_in_flight`（:364-366）の
      隣に、その兄弟として `pub(super)` 可視性で置く。既存のドラッグ進行中ペアが既に
      置かれている場所であり、tests.rs が既にそこから import している場所（tests.rs:19）
      でもある。
- [x] A4（影響度: 低、可逆）: 構造テストの needle リスト（tests.rs:1939-1948）は単に
      拡張するのではなく**置き換える**必要がある。既存の needle は現在の合成の厳密な
      ソース部分文字列であるため、アームを書き換えると一致しなくなり、古いエントリが
      失敗する。書き換え後のアームに実在するリテラルのみを残す。
- [x] A5（影響度: 低、可逆）: `host.dragging`、`host.mouse_report_held`、
      `app.pending_selection_anchor` は現在の宣言（`src-tauri/src/window_host/mod.rs`、
      `src-tauri/src/app.rs`）を保つ。本フィーチャーはフィールドを追加せず、いかなる
      フィールドの型も変更しない。
- [x] A6（影響度: 低、可逆）: 本プロジェクトに E2E 自動化は存在しない
      （test/README.md "E2E Tests": "None at the moment. There is no docker-compose.e2e.yml
      and no e2e-tests/ directory"）。したがって本フィーチャーのエンドツーエンド確認は
      ユーザー実行の手動シナリオ TS-7 である。本ディスパッチに解決済み E2E パスが
      1 つもないこと（`resolved_input_paths.e2e` が空）で裏付けられる。

### 14.2 未確認・保留事項

`status: tbd` の要件はない。すべての機能要件・非機能要件が `status: ok` である。

## 15. 参考資料

- `src-tauri/src/window_host/event_loop.rs`: winit イベントループのアーム
  （`Focused(false)`: :227-282、:253-255、:262-265）
- `src-tauri/src/window_host/pointer_routing.rs`: ポインタルーティング、
  `drag_in_flight` / `local_drag_in_flight`（:364-377）、ローカル選択アーム
  （:391-412、:424-443）
- `src-tauri/src/window_host/mouse_report.rs`: ウィンドウ非依存の判定層、
  `HeldButtons`（:471-483）、`decide_release` の所有者なしフォールバック（:935）
- `src-tauri/src/window_host/mod.rs`: `WindowHost`（`dragging`、`mouse_report_held`、
  `mouse_report_gesture_owner`）
- `src-tauri/src/app.rs`: `App`（`pending_selection_anchor`）
- `src-tauri/src/window_host/tests.rs`: 構造テスト（:1931-1949）と既存のクリアテスト
  （:1835-1904）
- `feature-docs/mouse-report-reset-active-gesture/SPEC.md`: FR6 が supersede する対象
- test/README.md: "Test File Organization" / "Test Naming Conventions" /
  "Test Framework" / "E2E Tests"
- .claude/rules/core-architecture.md: ビルド面とプラットフォームサポート
