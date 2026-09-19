---
title: "mouse-report-reset-active-gesture"
created_date: 2026-09-20
status: draft
---

# mouse-report-reset-active-gesture - 要件定義書

## 1. 概要

### 1.1 背景

マウスレポート判定層の task0005 / task0006 リワークにより、ゴーストドラッグの
リグレッションが混入した。リワーク前は「左ボタンのリリースが必ずローカルの選択
ドラッグを終了させる」保証があったが、現在はその保証が失われている。

`RecordUpdates.reset`（mouse_report.rs:1027-1030）はマウストラッキングが非アクティブ
（`!tracking_active`）のときにジェスチャ所有スロットを全消去する。このため、左プレスで
記録された `GestureOwner::Local`（mouse_report.rs:792）が、ホイールノッチ
（`decide_wheel_event`、mouse_report.rs:983）や中／右ボタンのプレス
（`decide_press`、mouse_report.rs:753）によって消える。所有者を失った左リリースは
`peek(Left) == None`（mouse_report.rs:807）となり `Disposition::Nothing` を返すため、
`host.dragging` が立ったまま残る。

`host.dragging` が立ちっぱなしになると、ポインタ状態でゲートされている
`update_resize_hint`、`refresh_link_hover`（pointer_routing.rs:227）、および
PTY 出力によるリンク再検出（event_loop.rs:649）が恒久的に無効化される。

### 1.2 目的

- ゴーストドラッグのリグレッションを取り除き、左ボタンのリリースが必ずローカルの
  選択ドラッグを終了させるというリワーク前の保証を回復する。
- `host.dragging` を厳密に境界づけられた状態に保つ。true にするすべての経路に
  終端処理を保証し、ポインタ状態でゲートされた機能が恒久的に無効化されないようにする。
- PR #69 が提供したマウスレポート挙動をバイト単位で保持する。

### 1.3 スコープ

**対象**:

- ローカル所有経路（local-ownership path）の修復のみ。
- `src-tauri/src/window_host/mouse_report.rs` のリセット観測とリリース判定。
- `src-tauri/src/window_host/pointer_routing.rs` の選択完了アーム。
- `src-tauri/src/window_host/event_loop.rs` のフォーカス喪失アーム。

**対象外**:

- DEC マウスレポートプロトコル面の変更。
- Report 所有ディスポジションの挙動変更（FR4）。
- ホイールレポートのノッチ上限の緩和（NFR5）。

## 2. ビジネス要件

### 2.1 ビジネス目標

- task0005 / task0006 のリワークで混入したゴーストドラッグのリグレッションを除去し、
  左ボタンのリリースが必ずローカル選択ドラッグを終了させるというリワーク前の保証を
  回復する。
- `host.dragging` を厳密に境界づけられた状態に保つ。これを true にするすべての経路が
  終端処理を保証されることで、ポインタ状態でゲートされた機能（`update_resize_hint`、
  `refresh_link_hover`、PTY 出力によるリンク再検出）が恒久的に無効化されることが
  なくなる。
- PR #69 が提供したマウスレポート挙動をバイト単位で保持する。本件はローカル所有経路
  のみの修復であり、DEC マウスレポートプロトコル面の変更ではない。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| （本要件分析に記載なし） | 対象ユーザーの定義は要件分析に含まれない |

### 2.3 期待される効果

- 左ドラッグ中のホイールノッチ／第二ボタンのプレス／フォーカス喪失のいずれが起きても、
  選択ドラッグが終了する。
- `update_resize_hint`、`refresh_link_hover`、PTY 出力によるリンク再検出が、
  ドラッグ終了後の次のポインタ移動で再び動作する。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | ホイールノッチを挟んだ左ドラッグの完了 | ターミナル利用者 | 高 |
| UC02 | 第二ボタンのプレスを挟んだ左ドラッグの完了 | ターミナル利用者 | 高 |
| UC03 | ドラッグ中のフォーカス喪失によるドラッグ終了 | ターミナル利用者 | 高 |

### 3.2 ユースケース詳細

#### UC01: ホイールノッチを挟んだ左ドラッグの完了

**アクター**: ターミナル利用者

**事前条件**:

- マウストラッキングが非アクティブ（`!tracking_active`）である。
- ポインタがグリッド上にある。

**基本フロー**:

1. グリッド上で左ボタンを押す（`GestureOwner::Local` が Left に記録される）。
2. ボタンを押したままドラッグして選択を伸ばす。
3. 左ボタンを離さずにホイールを 1 ノッチ回す。
4. 左ボタンを離す。

**事後条件**:

- `host.dragging == false`
- `app.pending_selection_anchor == None`
- ドラッグした選択が PRIMARY に存在する（AC-1）。
- 次のポインタ移動で `update_resize_hint` / `refresh_link_hover` / PTY 出力による
  リンク再検出が再び動作する（AC-4）。

#### UC02: 第二ボタンのプレスを挟んだ左ドラッグの完了

**アクター**: ターミナル利用者

**事前条件**:

- UC01 と同一。

**基本フロー**:

1. グリッド上で左ボタンを押す。
2. ボタンを押したままドラッグして選択を伸ばす。
3. 左ボタンを離さずに中ボタン、または右ボタンを押す。
4. 左ボタンを離す。

**事後条件**:

- UC01 と同一の状態で終わる（AC-2、AC-4）。

#### UC03: ドラッグ中のフォーカス喪失によるドラッグ終了

**アクター**: ターミナル利用者

**事前条件**:

- 左選択ドラッグが進行中で、選択が実体化している。

**基本フロー**:

1. グリッド上で左ボタンを押し、ドラッグして選択を伸ばす。
2. ウィンドウのフォーカスが失われる（`WindowEvent::Focused(false)`）。

**事後条件**:

- `host.dragging == false`
- `app.pending_selection_anchor == None`
- 選択が PRIMARY に存在する（AC-3）。
- 以後の `PointerMoved` が選択を伸ばさない（AC-3）。

**代替フロー**:

- ドラッグが進行していない状態でフォーカスを失った場合、選択状態および PRIMARY に
  対して完全な no-op のままとする。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 物理的に押下中のジェスチャをリセットが保持する | `!tracking_active` によるリセットで、押下中ボタンの所有スロットを消さない | 高 |
| FR2 | 左リリースが必ずローカルドラッグを完了させる | Report 所有でない左リリースを選択完了アームに載せる | 高 |
| FR3 | フォーカス喪失が進行中のローカルドラッグを終了させる | `Focused(false)` で `dragging` と pending anchor を解消する | 高 |
| FR4 | Report 所有の挙動は不変 | Report 所有ディスポジションをバイト単位で維持する | 高 |
| FR5 | ドラッグ非進行時のリリースに新たな副作用を作らない | クローム上のクリックが選択再公開にならないようにする | 高 |
| FR6 | 陳腐化したジェスチャ記録は引き続き消去する | 押下されていないボタンのスロット消去とタブ変更時の全消去を維持する | 高 |

### 4.2 機能詳細

#### FR1: 物理的に押下中のジェスチャをリセットが保持する

**説明**:
マウストラッキングが非アクティブ（`!tracking_active`）であることを理由に
outcome の `RecordUpdates.reset` が適用されるとき（mouse_report.rs:1027-1030）、
現在物理的に押下されているボタンのジェスチャ所有スロットは保持し、押下されていない
ボタンのスロットのみを消去する。タブ変更トリガーは従来どおり全スロットを消去する
（前提 A3、FR6）。リセットのうちセル変更キャッシュ側は変更せず、従来どおり無条件に
実行する。

具体的には、トラッキング非アクティブの状態でグリッド上の左プレスが Left に対して
`GestureOwner::Local` を記録し（mouse_report.rs:792）、その後のホイールノッチ
（`decide_wheel_event`、mouse_report.rs:983）や中／右ボタンのプレス
（`decide_press`、mouse_report.rs:753）は `tracking_active` が false であるために
`reset = true` を立てるが、Left が押下中である限りその Left スロットを消してはならない。

**ビジネスルール**:

- 「現在押下中のボタン」はホスト側の押下記録（`host.mouse_report_held`、
  mouse_report.rs:436-448 の素の `HeldButtons` 値）を指す（前提 A2）。
- 押下中ボタンの除外は `!tracking_active` によるリセットトリガーにのみ適用する
  （前提 A3）。

#### FR2: 左リリースが必ずローカルドラッグを完了させる

**説明**:
Report 所有でない左ボタンのリリース、すなわち記録された所有者が `GestureOwner::Local`
であるか、所有者がまったく記録されていない（`peek(Left) == None`、
mouse_report.rs:807）場合は、`CompleteSelectionAndPublishToPrimary` のローカルアームを
通る。このアームは `host.dragging` をクリアし、`app.pending_selection_anchor` を消費し、
完了した選択を PRIMARY に公開する（`copy_on_select` が有効なときは CLIPBOARD にも公開
する）。挙動は現在の pointer_routing.rs:361-406 と同一である。所有者なしの左リリースが
`Disposition::Nothing` を返すことはなくなる。

**ビジネスルール**:

- リワーク前の基底コードにあった無条件の `(Left, Released) => host.dragging = false`
  アームを目標セマンティクスとする（前提 A6）。
- `decide_release` は位置非依存である（mouse_report.rs:798-805）。グリッド上で押して
  クローム上で離したケースでもこの性質を保つ。

#### FR3: フォーカス喪失が進行中のローカルドラッグを終了させる

**説明**:
`WindowEvent::Focused(false)` のアーム（event_loop.rs:229-253）は、既存の
`mouse_report::clear_all` 呼び出し（event_loop.rs:249）と並んで、`host.dragging` を
クリアし `app.pending_selection_anchor` を消費する。フォーカス喪失の時点で選択が
実体化した左選択ドラッグが進行中だった場合、リリースと同一のルール
（`copy_on_select` による CLIPBOARD へのミラーを含む）で選択を PRIMARY に公開する
（前提 A1）。

#### FR4: Report 所有の挙動は不変

**説明**:
すべての Report 所有ディスポジションは現在の挙動をバイト単位で維持する。対象は
プレス／リリースのエンコード（`compose_button_code` / `encode_report`）、
`active_tab` ではなく `records.built_for_tab` を対象とするリリース
（mouse_report.rs:812）、モーションゲートとセル変更フィルタ、ホイール消費マトリクスで
ある。Report 所有の左リリースはローカルの選択完了アームを通ってはならない。

#### FR5: ドラッグ非進行時のリリースに新たな副作用を作らない

**説明**:
ローカルドラッグが進行していない状態（`host.dragging == false` かつ
`app.pending_selection_anchor == None`）で到着する左リリース、たとえばタブバー／
ステータスバー／スクロールバーオーバーレイ／mux サイドバー上に着地したプレスの
リリース（現状では所有者を記録しない。mouse_report.rs:738-750）は、現行リビジョンで
既に生じているもの以外の PRIMARY／CLIPBOARD 書き込みも fold クリックのトグルも
生じさせない。FR2 の拡張がクローム上のクリックを選択の再公開に変えてはならない。

#### FR6: 陳腐化したジェスチャ記録は引き続き消去する

**説明**:
リセット観測は、現在押下されていないボタンのジェスチャ所有スロットを引き続き消去し、
タブ変更トリガーについては押下中のものも含めて全スロットを引き続き消去する。本修復は
リセットが消去する範囲を狭めるものであり、リセット自体を取り除くものではない。
2 つのリセットトリガーにまたがる絞り込みの範囲は前提 A3 で確定している。

### 4.3 エラーケース・境界条件

| 条件 | 期待される扱い |
|------|----------------|
| ドラッグ中にアプリケーションがトラッキングモードを有効化した | Left スロットは `Local` のまま `tracking_active` が true になる。リリースはレポートを出さず、ローカルドラッグを完了させる（FR2） |
| Report 所有ボタンの押下中にトラッキングモードが無効化された | `decide_release` の Report 分岐は、トラッキングモードが無い場合に既に `Disposition::Nothing` を返す（mouse_report.rs:810-828）。FR1 がこれを不正なレポート発行に変えてはならない |
| ボタン押下中にアクティブタブが変わった | 前提 A3 による。タブ変更トリガーは全スロットを消去し続ける |
| 複数ボタンの同時押下 | 中／右ボタンのリリースは左の完了アームを通らない。`decide_release` の `GestureOwner::Local` 分岐は既にアームを `MouseButtonId::Left` に限定している（mouse_report.rs:831-837） |
| DEC レポート識別子を持たないサイドボタン | `winit_button_to_report_identity` が `None` を返し、`handle_pointer_button` がリリース時に早期リターンする（pointer_routing.rs:510-521）。このリリースが左ドラッグを終了させることはなく、左リリースが依然として必要である |
| `MouseButtonId::None` | ジェスチャスロットを占有しない（`GestureOwnership::slot`、mouse_report.rs:348）。押下中ボタンの除外がこれに誤ってスロットを与えてはならない |
| ドラッグ非進行時のフォーカス喪失 | 選択状態および PRIMARY に対して純粋な no-op のままとする |
| fold クリックのトグル | `complete_selection_and_publish_to_primary` 内のトグル（pointer_routing.rs:380-388）は、pending anchor が存在し、選択が存在せず、Ctrl が押されていないときだけ発火する。FR2 の拡張でクロームクリックがこの述語を満たしてはならない |
| CSD のエッジリサイズプレスによる短絡 | pointer_routing.rs:438-445 はマウスレポート判定の前に return する。リサイズへの引き渡しは所有者を記録せずドラッグも開始しない。挙動は不変 |
| グリッド上のプレスに対するクローム上でのリリース | `decide_release` は意図的に位置非依存（mouse_report.rs:798-805）。FR2 はこの性質を保つ |
| 現状の `dragging` 固着 | event_loop.rs:649 の PTY 出力によるリンク再検出も抑止する。バグ報告には挙がっていない症状だが AC-4 が対象とする |

## 5. 非機能要件

### 5.1 パフォーマンス要件

- 本要件分析にパフォーマンス目標値の記載はない。

### 5.2 セキュリティ要件

- 新たな攻撃面を作らない。変更はローカルのポインタ状態管理に限定され、PTY への追加の
  バイト送出はない。
- NFR5: ホイールレポートのノッチ上限（`bounded_wheel_report_duplicate` /
  `MAX_WHEEL_REPORT_NOTCHES`、pointer_routing.rs:726-744）は感触調整のつまみではなく
  ドキュメント化されたセキュリティ特性であり、ホイール経路の作業中も緩めない。
- FR4 のバイト単位の不変性により、本修復の副作用としてレポートシーケンスがバイトを
  獲得したり失ったりすることはない。

### 5.3 可用性要件

- 本要件分析に稼働率・障害復旧時間の記載はない。

### 5.4 保守性要件

- NFR1: 判定層はウィンドウ非依存を保つ。`src-tauri/src/window_host/mouse_report.rs` の
  いかなるシグネチャも winit 型、egui ウィンドウハンドル、GPU サーフェス、PTY、
  `term_core` のモード型を受け取らず返さない。これはモジュールが明示している
  AC-8 / AC-1 の不変条件であり（mouse_report.rs:14-20, 401-403）、すべてのユニットを
  素の `#[test]` から実行可能にしている根拠である。押下状態を `decide_press` /
  `decide_wheel_event` / `apply_outcome` に届ける必要がある場合は、素の bool か既存の
  素の `HeldButtons` 値（mouse_report.rs:436-448）として渡す。
- NFR2: 判定時にレコードを変更しない。SC-10 の性質 3（mouse_report.rs:494-497,
  507-509）を維持する。`decide_*` 関数は素の入力に対する純関数のままとし、レコードの
  変更はすべて `RecordUpdates` を経由して `apply_outcome` がちょうど一度だけ適用する。
- NFR3: テストスタイルはプロジェクトの規約に合わせる。新規テストは対象コードの隣に
  置くインライン `#[cfg(test)] mod tests {}` のユニットとし、
  `<subject>_<scenario>_<expected>` の命名で、共有グローバルフィクスチャを持たず
  テストごとに構築し、既存の `base_button_inputs` / `base_motion_inputs` /
  `base_wheel_inputs` ヘルパー（mouse_report.rs:1667-1725）から組み立てる。これらの
  ヘルパーはトラッキング ON を既定とするため、再現テストは `mode_1002 = false` を
  設定する。新しいテストフレームワーク依存は追加しない（test/README.md
  "Test Framework": proptest なし、criterion なし）。

### 5.5 互換性要件

- NFR4: ビルド面の不変性。CLI 専用ビルド（`--no-default-features`）は引き続き
  コンパイルできる（触れるモジュールはすべて GUI ゲート下にある）。Linux と Windows の
  双方を引き続きサポートし、プラットフォーム固有 API は導入しない
  （.claude/rules/core-architecture.md）。

## 6. UI/UX要件

### 6.1 画面設計要件

デザインステップはスキップされた。理由: ユーザーに見える表面の変更がない。本修復は
GUI 内部の Rust モジュール 3 つにおけるポインタ状態管理に限定され、新しい UI 要素、
新しいデザイントークン、レイアウト・文言・色・インタラクションのアフォーダンスの変更を
いずれも導入しない。観測できる唯一の差分は、基底コードが既に持っていた挙動の回復である。
`doc/UI-DESIGN-GUIDELINES.yaml` とその 2 つのミラーは変更しないため、`ui::dialog::tests`
のデザインシステムドリフトテストに影響はない。

### 6.2 画面遷移

該当なし。

### 6.3 レスポンシブ対応

該当なし。

## 7. データ要件

該当なし。永続データモデルの変更はない。

## 8. 外部連携

該当なし。外部システムとの連携はない。

## 9. 制約条件

### 9.1 技術的制約

- 判定層はウィンドウ非依存を保つ（NFR1）。
- レコードは判定時に変更せず、`RecordUpdates` 経由でのみ変更する（NFR2）。
- テストはインライン `#[cfg(test)] mod tests {}` とし、新しいテストフレームワーク依存を
  追加しない（NFR3）。
- CLI 専用ビルドが引き続きコンパイルでき、Linux / Windows の双方を維持する（NFR4）。
- ホイールレポートのノッチ上限を弱めない（NFR5）。
- `HeldButtons` は `WindowHost` の独立したフィールド（mod.rs:263）で、意図的に
  `MouseReportRecords` の一部ではないため、押下状態を届けるにはシグネチャ変更が
  避けられない（前提 A4）。
- `src-tauri/src/window_host/tests.rs:1782-1791` は pointer_routing.rs のソースが
  `"mouse_report::apply_outcome("` という文字列を literally 含むことを表明している。
  コンパニオンのエントリポイントを追加して 3 箇所の本番呼び出しを移す場合、この構造
  テストの委譲リストにコンパニオン名を追加する必要がある（前提 A7）。

### 9.2 ビジネス上の制約

- PR #69 が提供したマウスレポート挙動をバイト単位で保持する。

### 9.3 スケジュール制約

本要件分析に記載なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mouse-report-reset-active-gesture/**`
- `test-docs/mouse-report-reset-active-gesture/**`

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
| A1: フォーカス喪失時に選択を PRIMARY へ公開するか、状態クリアに留めるか | 中 | バグ報告の Definition of Done が 3 シナリオすべてで「`dragging == false` かつ選択が公開されている」ことを要求しているため、より厳しく後発の記述である DoD を採用し、既存の `complete_selection_and_publish_to_primary` 経路を再利用して 3 シナリオを 1 つの終端処理に収束させる |
| A3: 押下中ボタンの除外をタブ変更トリガーにも広げるか | 中 | `!tracking_active` トリガーのみに限定する。タブトリガーに広げると、押下中の Report 所有ボタンについて現在の「リリースで何も出さない」が「プレス時のタブを対象とするリリースレポート」に変わり、FR4 のバイト単位不変性に反し、`ac6_ts23_release_after_active_tab_change_targets_the_tab_the_press_recorded`（mouse_report.rs:2604-2630）を壊す |
| A4: 押下状態を届ける実装位置 | 中 | `apply_outcome` の内側で除外を行い、`HeldButtons` を取るコンパニオンのエントリポイントを追加する。既存の `apply_outcome` は委譲する薄いラッパーとして残し、約 30 箇所のインラインテスト呼び出しを変更不要にする |
| A7: 構造テストの文字列一致 | 低 | tests.rs:1782-1791 の委譲リストにコンパニオン名を追加する。文字列一致を満たすためだけに旧名の呼び出しを残すことはしない |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| FR2 の拡張がクローム上のクリックを選択の再公開に変える | 中 | 中 | FR5 と AC-8 / TS-8 で明示的に否定する |
| 修復の副作用で Report 所有経路のバイト列が変わる | 低 | 高 | FR4 と AC-7 / TS-7 で既存テストスイートの不変を確認する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: 再現手順 1-4（グリッド上で左プレス＆ドラッグ、離さずにホイール 1 ノッチ、
      その後に左リリース）が `host.dragging == false`、
      `app.pending_selection_anchor == None`、およびドラッグした選択が PRIMARY に
      存在する状態で終わる。
- [ ] AC-2: ホイールノッチを中ボタンまたは右ボタンのプレスに置き換えた同じ手順が、
      同一の状態で終わる。
- [ ] AC-3: ドラッグ中にウィンドウフォーカスを失うと `host.dragging == false`、
      `app.pending_selection_anchor == None`、選択が PRIMARY にある状態で終わり、
      以後の `PointerMoved` が選択を伸ばさない。
- [ ] AC-4: AC-1〜AC-3 のいずれかの後、`update_resize_hint` と `refresh_link_hover`
      （pointer_routing.rs:227）、および PTY 出力によるリンク再検出
      （event_loop.rs:649）が次のポインタ移動で再び動作する。
- [ ] AC-5: ボタンが 1 つも押下されていない状態で発生したリセット観測が、依然として
      すべてのジェスチャ所有スロットを消去し、セル変更キャッシュもリセットする（FR6）。
- [ ] AC-6: ボタンが押下されている状態で発生したリセット観測が、依然としてセル変更
      キャッシュをリセットし、押下されていないボタンのスロットを消去する。
- [ ] AC-7: `src-tauri/src/window_host/mouse_report.rs` の `mod tests` の既存テストが
      意味を変えずにすべて通り続ける。レポートのバイト列、対象タブの選択、ホイール
      マトリクスは変わらない（FR4）。
- [ ] AC-8: タブバー／ステータスバー／スクロールバーオーバーレイ／mux サイドバー上の
      左プレスとそのリリースが、PRIMARY / CLIPBOARD への書き込みも fold トグルも
      起こさない（FR5）。
- [ ] AC-9: 現行リビジョンに対して失敗し、修正後に通るリグレッションテストが
      AC-1 / AC-2 / AC-3 に 1 つずつ存在する。

### 11.2 KPI

本要件分析に KPI の記載はない。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系（TS-1、unit）: 左ドラッグがホイールノッチを跨いで生き残る。
      `decide_press(Left, トラッキング非アクティブ, グリッド上)` →
      `decide_wheel_event(WheelUp, グリッド上, トラッキング非アクティブ)` →
      `decide_release(Left)` を 1 つの `MouseReportRecords` に `apply_outcome` 経由で
      通し、リリースのディスポジションが
      `Disposition::Local(LocalArm::CompleteSelectionAndPublishToPrimary)` であることを
      表明する（AC-1、AC-9）。
- [ ] 正常系（TS-2、unit）: 左ドラッグが第二ボタンのプレスを跨いで生き残る。TS-1 の
      ホイールノッチを中プレス（および右プレス）に置き換え、左リリースが依然として
      `CompleteSelectionAndPublishToPrimary` を指名し、中／右スロットが独立に記録・
      消去されることを表明する（AC-2、AC-9）。
- [ ] 境界値（TS-3、unit）: 所有者なしの左リリースでもドラッグが完了する。Left スロットが
      `None` で、レコードがローカルドラッグ進行中を示す `MouseReportRecords` に対する
      `decide_release(Left)` が `Disposition::Nothing` ではなく完了アームを返す
      （AC-1、AC-2）。
- [ ] 正常系（TS-4、unit）: フォーカス喪失がドラッグを終了させる。左ドラッグが進行中の
      状態でフォーカス喪失のクリーンアップ入口を実行し、`dragging` がクリアされ、
      pending anchor が消費され、選択が公開されることを表明する。winit ウィンドウなしで
      到達できるようクリーンアップを構成する（NFR1）。event_loop.rs のアームでしか
      表現できない場合は、状態変更をウィンドウ非依存のヘルパーに切り出してそれを
      テストする（AC-3、AC-9）。
- [ ] 境界値（TS-5、unit）: 押下中ジェスチャの最中でもリセットがセルキャッシュを
      リセットする。Left を押下中かつそのスロットが記録された状態で `reset: true` を
      持つ outcome を適用し、直前にキャッシュされていたセルについて
      `records.cell_cache.would_report(c, r)` が true になり、かつ `peek(Left)` が
      依然として `Some(GestureOwner::Local)` であることを表明する（AC-6）。
- [ ] 境界値（TS-6、unit）: 押下されていないボタンの陳腐化した記録をリセットが消去する。
      Middle のスロットが記録されているが Middle が押下されていない状態で `reset: true`
      を持つ outcome を適用し、`peek(Middle) == None` を表明する（AC-5、AC-6）。
- [ ] 回帰（TS-7、unit）: Report 所有経路が不変である。mouse_report.rs の既存の
      `mod tests` スイート（バイト厳密な X10 / SGR ケースとタブ指定リリースケースを含む）
      が変更なしで通る（AC-7）。
- [ ] 異常系（TS-8、unit）: クローム上のリリースが副作用を起こさない。
      `grid.in_tab_bar_band = true` の左プレス（所有者を記録しない）とそのリリースが、
      PRIMARY への公開も fold トグルも生じさせない（AC-8）。
- [ ] 手動（TS-9、manual）: リリースバイナリを起動して再現手順の 4 ステップと
      フォーカス喪失のバリエーションを実行し、リリース後に選択が伸びなくなること、
      および中クリックペーストで PRIMARY に選択が入っていることを確認する。本プロジェクトに
      E2E 自動化は存在しないため（test/README.md "E2E Tests"）、このシナリオは
      ユーザーが実行する（AC-1、AC-2、AC-3、AC-4）。
- [ ] セキュリティ: ホイール経路の作業後も `bounded_wheel_report_duplicate` /
      `MAX_WHEEL_REPORT_NOTCHES` の上限が維持されている（NFR5）。
- [ ] パフォーマンス: 本要件分析に記載なし。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| ゴーストドラッグ | 左ボタンを離した後も `host.dragging` が true のまま残り、選択がポインタ移動に追従し続ける状態 |
| ジェスチャ所有スロット | ボタンごとに `GestureOwner`（`Local` / Report）を記録する `MouseReportRecords` 内のスロット |
| リセット観測 | outcome の `RecordUpdates.reset`（mouse_report.rs:1027-1030）が適用されること |
| Report 所有 | ジェスチャの所有者がマウスレポート側であり、ディスポジションがレポート送出になる状態 |
| ローカル所有 | ジェスチャの所有者が `GestureOwner::Local` であり、ローカルの選択処理が担当する状態 |
| トラッキングアクティブ | DEC マウストラッキングモードが有効な状態（`tracking_active`） |
| PRIMARY | X11 の PRIMARY セレクション |
| クローム | タブバー、ステータスバー、スクロールバーオーバーレイ、mux サイドバーなどグリッド以外の UI 領域 |

## 14. 確認事項

### 14.1 確認済み事項

要件分析が確定した前提（assumption）を記録する。

- [x] A1（影響度: 中、可逆）: 進行中の左ドラッグ中にフォーカスを失った場合、単に
      ドラッグ状態をクリアするだけでなく、進行中の選択を PRIMARY に公開する。
      理由: バグ報告の Expected-behaviour の箇条書きはクリアのみを挙げるが、
      Definition of Done は 3 シナリオ（ホイール、他ボタン、フォーカス喪失）すべてが
      `dragging == false` かつ選択公開済みで終わることを要求している。DoD の方が
      厳しくかつ後発の記述であり、既存の `complete_selection_and_publish_to_primary`
      経路を再利用することで 3 シナリオが 1 つの終端処理に収束する。
- [x] A2（影響度: 低、可逆）: 「現在押下中のボタン」はホストの押下記録
      （`host.mouse_report_held`、mouse_report.rs:436-448 の素の `HeldButtons` 値）を
      意味する。理由: `handle_pointer_button` が pointer_routing.rs:486-494 で、判定や
      ガードがイベントを消費する前に無条件でこの記録を維持しており、物理的な押下の
      真実として既に利用可能な唯一の情報源である。`mouse_report::clear_all` は
      フォーカス喪失時に既にこれをゼロクリアしている（event_loop.rs:249-252）。
- [x] A3（影響度: 中、可逆）: 押下中ボタンの除外は `!tracking_active` のリセット
      トリガーにのみ適用する。真のタブ変更（`built_for_tab != Some(active_tab)`）は
      従来どおり全ジェスチャ所有スロットを消去し、ボタンごとのプレス起点タブは導入
      しない。理由: Codex のセカンドオピニオン（turn 1）とそれに続く Opus のエスカレー
      ション（turn 2、Codex 利用不可）で決着した。報告された再現手順にタブ変更は
      含まれない。左プレスが `built_for_tab = Some(active_tab)` を設定するため
      （mouse_report.rs:791）、続くホイールノッチや第二プレスは `tab_changed == false`
      となり `reset = !tracking_active` のみで決まる（mouse_report.rs:751-753,
      981-983）。カーブアウトをタブトリガーに広げると、押下中の Report 所有ボタンに
      ついて現在の「リリースで何も出さない」が「プレス時のタブを対象とするリリース
      レポート」に変わり、FR4 のバイト単位不変性に反し、
      `ac6_ts23_release_after_active_tab_change_targets_the_tab_the_press_recorded`
      （mouse_report.rs:2604-2630）を壊す。また `MouseReportRecords::built_for_tab` と
      並ぶ「このレコードがどのタブのものか」という第二の重複概念を導入し、
      クロスタブドラッグ中にモーション（`inputs.active_tab` に報告）とリリース
      （プレス起点）が食い違う。残る唯一の隙間、すなわち真のジェスチャ中タブ変更を
      生き延びるローカル左ドラッグは、FR2 の所有者非依存な左リリースのクリーンアップで
      無効化されるため、ユーザーに見えるゴーストドラッグは残らない。
- [x] A4（影響度: 中、可逆）: 押下中ボタンの除外は `ButtonEventInputs` /
      `WheelEventInputs` に押下フラグを通すのではなく `apply_outcome` の内側で行う。
      `apply_outcome` に `HeldButtons` を取るコンパニオンのエントリポイントを追加し、
      既存の `apply_outcome` は委譲する薄いラッパーとして残すことで、約 30 箇所の
      インラインテスト呼び出しに変更を不要にする。理由: 同じ Opus エスカレーションで
      決着した。判定バンドルに押下フラグを通すとレコードのライフサイクルポリシーが
      `decide_*` 関数へ移動し、今日「セルキャッシュのリセット」と「全所有スロットの
      消去」の両方を意味している `RecordUpdates.reset` の分割を強いる。これは NFR2 が
      明示する純粋性の不変条件（mouse_report.rs:494-497, 507-510）に反する。また
      `host.mouse_report_held` は判定より前に更新されるため（pointer_routing.rs:486-494）、
      `decide_press` にまさに今押されているボタンの押下フラグを渡すことになる。
      `HeldButtons` はホストの独立フィールド（mod.rs:263）であり意図的に
      `MouseReportRecords` の一部ではないため、シグネチャ変更は避けられない。
      コンパニオン関数の形にすれば変更は本番の 3 呼び出し箇所に閉じる。
- [x] A5（影響度: 低、可逆）: `host.dragging` と `app.pending_selection_anchor` は
      それぞれ `WindowHost` と `App` のフィールドであり、バグ報告が挙げた 3 ファイルの
      外で宣言されている。理由: 双方とも pointer_routing.rs と event_loop.rs で読み書き
      されるが、宣言は `src-tauri/src/window_host/mod.rs` と `src-tauri/src/app.rs` に
      ある。`mouse_report_records()` / `set_mouse_report_records()` アクセサと、
      event_loop.rs:250 が使う別フィールド `host.mouse_report_gesture_owner` も同様。
- [x] A7（影響度: 低、可逆）: `src-tauri/src/window_host/tests.rs:1782-1791` は
      pointer_routing.rs のソースが `"mouse_report::apply_outcome("` という needle を
      literally 含むことを表明している。A4 のコンパニオンエントリポイントを追加して
      本番の 3 呼び出し箇所をそれに移すには、この構造テストの委譲リストに
      コンパニオン名を追加する必要がある。理由: Opus のエスカレーションが既存テストを
      読む過程で発見した。リストを編集するのが誠実な修正であり、文字列一致を満たす
      ためだけに旧名の呼び出しを痕跡として残すことはしない。

### 14.2 未確認・保留事項

`status: tbd` の要件はない。すべての機能要件が resolved である。ただし次の前提は
独立検証ができていない。

- [ ] A6（影響度: 低、可逆）: バグ報告が挙げるリワーク前の挙動、すなわち基底コードの
      無条件な `(Left, Released) => host.dragging = false` アームが FR2 の目標セマン
      ティクスである。理由: バグ報告に、このバグが以前は到達不能だった理由として
      記載されている。リワーク前のリビジョンは提供された入力の範囲外であるため、
      独立に検証できなかった。

## 15. 参考資料

- `src-tauri/src/window_host/mouse_report.rs`: マウスレポート判定層（ウィンドウ非依存）
- `src-tauri/src/window_host/pointer_routing.rs`: ポインタイベントのルーティングと
  ローカル選択アーム
- `src-tauri/src/window_host/event_loop.rs`: winit イベントループのアーム
  （`Focused(false)`、PTY 出力によるリンク再検出）
- `src-tauri/src/window_host/mod.rs`: `WindowHost`（`dragging`、`mouse_report_held`、
  `mouse_report_gesture_owner`）
- `src-tauri/src/app.rs`: `App`（`pending_selection_anchor`）
- `src-tauri/src/window_host/tests.rs`: 構造テスト（委譲リスト）
- test/README.md: "Test Framework" / "E2E Tests"
- .claude/rules/core-architecture.md: ビルド面とプラットフォームサポート
- PR #69: 現行のマウスレポート挙動の出所
