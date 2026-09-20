---
title: "focus-loss-drag-cleanup-test"
created_date: 2026-09-20
status: draft
---

# focus-loss-drag-cleanup-test - 要件定義書

## 1. 概要

### 1.1 背景

mouse-report-reset-active-gesture の task0001 で導入されたフォーカス喪失時のローカルドラッグ後始末は、winit ウィンドウを前提とするコードパスの中にあり、ソーススキャンによるアサーションと手動確認でしか裏付けられていない。

### 1.2 目的

フォーカス喪失時のローカルドラッグ後始末を、winit ウィンドウなしで実行できる自動テストで検証可能にする。

### 1.3 スコープ

対象は `src-tauri/src/window_host/` 内部のテスト用シーム抽出と、先行フィーチャーのドキュメント追補に限る。製品挙動の変更は対象外。

**宣言された変更集合**（analyst が確定したもの）:

- `src-tauri/src/window_host/pointer_routing.rs`
- `src-tauri/src/window_host/event_loop.rs`
- `src-tauri/src/window_host/mod.rs`
- `src-tauri/src/window_host/tests.rs`
- `feature-docs/mouse-report-reset-active-gesture/SPEC.md`
- `feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md`

## 2. ビジネス要件

### 2.1 ビジネス目標

| ID | 目標 |
|----|------|
| BO-1 | mouse-report-reset-active-gesture task0001 で導入した挙動が、ソーススキャンによるアサーションと手動確認だけに依存しないよう、フォーカス喪失時のローカルドラッグ後始末を winit ウィンドウなしで走る自動テストで検証可能にする |
| BO-2 | 製品挙動をバイト単位で不変に保つ。本フィーチャーが追加するのはテスト到達性のみで、それ以外は何も追加しない |
| BO-3 | 新しいテストが揃った時点で、先行フィーチャーの SPEC.md / VERIFICATION.md のテスト対応表を実態と一致させる。ただし実機でしか確認できない効果をユニット検証済みとは主張しない |

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | `cargo test --lib` でフォーカス喪失時の後始末を検証する、本リポジトリの開発者 |

### 2.3 期待される効果

- フォーカス喪失時の後始末の 3 つの効果（ドラッグフラグのクリア、保留アンカーの消費、選択テキストの publish）が 1 つのテストで検証される
- 宛先判定の 8 通りの入力組み合わせが truth table テストで網羅される
- 先行フィーチャーのテスト対応表が実態と一致する

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | winit ウィンドウなしでフォーカス喪失時の後始末を検証する | 開発者 | 高 |
| UC02 | 先行フィーチャーのテスト対応表を追補する | 開発者 | 中 |

### 3.2 ユースケース詳細

#### UC01: winit ウィンドウなしでフォーカス喪失時の後始末を検証する

**アクター**: 開発者

**事前条件**:
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が実行できる

**基本フロー**:
1. ドラッグ中フラグと保留中の選択アンカーを立てた状態を用意する
2. drag-in-flight ガードを通す
3. terminator の publish 側を、ウィンドウ非依存のコア（FR1）と記録用シンク（FR4）に対して実行する
4. ドラッグフラグのクリア、アンカーの消費と返却、PRIMARY への publish を検証する

**代替フロー**:
- `copy_on_select` が有効かつテキストが非空の場合のみ、CLIPBOARD への書き込みも記録される

**事後条件**:
- winit ウィンドウ・GPU サーフェス・イベントループ・ディスプレイサーバーのいずれも使わずにテストが完了する

#### UC02: 先行フィーチャーのテスト対応表を追補する

**アクター**: 開発者

**事前条件**:
- 新しいテストが追加され、通っている

**基本フロー**:
1. `feature-docs/mouse-report-reset-active-gesture/SPEC.md` の TS-4 チェックボックスを新しいテストに向ける
2. `feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md` の TS-4 行と SC-C 行を新しいテストに向ける
3. 記録されたシンク呼び出しは書き込み「要求」の証拠であって、OS レベルの PRIMARY/CLIPBOARD 更新の証拠ではないことを明記する

**事後条件**:
- 先行フィーチャーの AC-3（実機確認）は手動シナリオに紐づいたままで、ユニット検証済みとは表示されない

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | ウィンドウ非依存のドラッグ終了状態コア | ローカルドラッグ終了処理の状態側をウィンドウ非依存の関数として抽出する | resolved |
| FR2 | publish_local_drag は綴りを保ったまま委譲する | 既存シグネチャを保ち、薄いアダプタになる | resolved |
| FR3 | 純粋な宛先判定と全組み合わせ網羅 | 3 値から宛先集合を返す純関数を抽出し、8 通りを網羅する | resolved |
| FR4 | 注入可能なシンクのシームと記録用テストダブル | PRIMARY/CLIPBOARD の 2 つの書き込みをシーム経由にする | resolved |
| FR5 | ウィンドウ非依存のエンドツーエンドのフォーカス喪失後始末テスト | ガードから publish までを 1 つのテストで検証する | resolved |
| FR6 | 既存の書き込みルールをバイト単位で保存 | 現行の書き込みルールを修正せずそのまま固定する | resolved |
| FR7 | fold トグルの排他性を保存 | `handle_fold_click` を publish 側とコアの外に保つ | resolved |
| FR8 | 先行フィーチャーのドキュメント追補 | SPEC.md / VERIFICATION.md の対応表を訂正する | resolved |

### 4.2 機能詳細

#### FR1: ウィンドウ非依存のドラッグ終了状態コア

**説明**: ローカルドラッグ終了処理の状態側を、`src-tauri/src/window_host/pointer_routing.rs` 内のウィンドウ非依存な関数として抽出する。この関数は素の入力と所有された状態（ドラッグフラグ、保留中の選択アンカー、解決済みの選択テキスト、`copy_on_select`）だけを扱い、`WindowHost` を一切名指ししない。

**入力**:
- ドラッグフラグ: bool - 現在ドラッグ中かどうか
- 保留中の選択アンカー: `Option<Pos>` - 消費対象のアンカー
- 解決済み選択テキスト: 文字列 - publish 対象のテキスト
- `copy_on_select`: bool - 設定値

**出力**:
- 消費されたアンカー: `Option<Pos>`
- FR3 の宛先判定結果

**ビジネスルール**:
- 現在 `publish_local_drag` が行っている 2 つの状態効果（ドラッグフラグのクリア、保留中の選択アンカーの消費（take））を行う
- winit ウィンドウ・GPU サーフェス・イベントループなしの素の `#[test]` から呼び出せる

#### FR2: publish_local_drag は綴りを保ったまま委譲する

**説明**: `publish_local_drag`（pointer_routing.rs:391）は、名前・`pub(super)` の可視性・引数の順序と型 `(host: &mut WindowHost, app: &mut App)`・戻り値 `-> Option<Pos>` をそのまま保ち、薄いアダプタになる。入力を集め、FR1 のコアを呼び、結果の書き込みを FR4 のシンク経由に流し、コアが消費したアンカーを返す。

**ビジネスルール**:
- event_loop.rs:254 の呼び出しリテラル `publish_local_drag(host, &mut self.app)` と event_loop.rs:253 の `local_drag_in_flight(host, &self.app)` を 1 文字も変えずに保存する
- 理由: `focus_loss_arm_never_calls_the_fold_click_toggle`（tests.rs:1932）が `include_str!` で両方を検証しているため

#### FR3: 純粋な宛先判定と全組み合わせ網羅

**説明**: (selection_present, copy_on_select, text_empty) の 3 値から、書き込むべき宛先の集合（PRIMARY、CLIPBOARD、両方、どちらでもない）を返す純粋な述語を抽出する。

**入力**:
- selection_present: bool
- copy_on_select: bool
- text_empty: bool

**出力**:
- 宛先集合: PRIMARY / CLIPBOARD / 両方 / なし

**ビジネスルール**:
- 素の値を取り、値を返し、何も変更せず、I/O を行わない
- 8 通りの入力組み合わせすべてを `src-tauri/src/window_host/tests.rs` の素の `#[test]` で網羅する
- 既存の `drag_in_flight_is_true_whenever_either_input_is_true`（tests.rs:1916）の truth table テストのスタイルに倣う

#### FR4: 注入可能なシンクのシームと記録用テストダブル

**説明**: PRIMARY と CLIPBOARD の 2 つの選択書き込みに対して、小さな注入可能なシンク抽象を導入する。publish パスは `host.set_primary` / `host.set_clipboard` を直接呼ばず、このシーム経由で書き込む。

**ビジネスルール**:
- `WindowHost` が製品実装を提供し、現行の挙動をそのまま保つ
- テストモジュール内の記録用テストダブルが、各呼び出しの宛先とペイロードを捕捉する
- winit ウィンドウなしのテストから、どの宛先に書かれたかと、各宛先に書かれた正確なテキストの両方を検証できる

**スコープ境界**:
- シームは pointer_routing.rs の publish パスにのみ適用する
- key_routing.rs:106 の `host.set_clipboard(&text);` は変更しない。`copy_chord_clears_selection_immediately_after_clipboard_write`（tests.rs:2474）が key_routing.rs をそのリテラルと `app.clear_selection();` との隣接でソーススキャンしているため

#### FR5: ウィンドウ非依存のエンドツーエンドのフォーカス喪失後始末テスト

**説明**: winit ウィンドウなしの素の `#[test]` を追加し、フォーカス喪失時の後始末の合成（drag-in-flight ガード → terminator の publish 側）を、FR1 のコアと FR4 の記録用シンクに対してエンドツーエンドで駆動する。

**ビジネスルール**: TS-4 の 3 つの効果を検証する。
- (a) ドラッグフラグがクリアされる
- (b) 保留中の選択アンカーが消費され返却される
- (c) 選択テキストが PRIMARY に publish される（CLIPBOARD へは `copy_on_select` が有効かつテキストが非空のときだけ）。記録されたペイロードは解決済み選択テキストと一致する

#### FR6: 既存の書き込みルールをバイト単位で保存

**説明**: 現在有効な書き込みルールを、修正も厳格化もせずそのまま保存する。

**ビジネスルール**:
- PRIMARY は、空テキストを含め、解決されたあらゆる選択に対して書き込まれる
- CLIPBOARD は、`app.settings.copy_on_select` が true かつ解決済みテキストが非空のときにのみ書き込まれる
- `app.selection` が `None` の場合、またはアクティブタブの参照に失敗した場合は、どちらの宛先にも書き込まれない

**エラーケース**:
| ケース | 条件 | 対応 |
|--------|------|------|
| 選択なし | `app.selection` が `None` | どちらの宛先にも書き込まない |
| タブ参照失敗 | アクティブタブの参照に失敗 | どちらの宛先にも書き込まない |

このルールに対して知覚される欠陥（例: 空テキストでの PRIMARY 書き込み）はスコープ外であり、変更せず現行挙動として新テストで固定する。

#### FR7: fold トグルの排他性を保存

**説明**: fold クリックのトグルは terminator の publish 側の外、かつ FR1 のコアの外に留まる。

**ビジネスルール**:
- `handle_fold_click` は pointer_routing.rs（424 行目以降）の `complete_selection_and_publish_to_primary` のリリースパス合成に排他的に留まる
- 文字列 `handle_fold_click` は event_loop.rs に一度も現れてはならない
- `focus_loss_arm_never_calls_the_fold_click_toggle`（tests.rs:1932）は無修正で通り続ける

#### FR8: 先行フィーチャーのドキュメント追補

**説明**: 新しいテストが入って通った後に、`feature-docs/mouse-report-reset-active-gesture/SPEC.md` の TS-4 チェックボックスと、`feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md` の TS-4 行および SC-C 行を、新しいテストを指すように訂正する。

**ビジネスルール**:
- 記録されたシンク呼び出しは書き込み「要求」の証拠であり、実際の OS レベルの PRIMARY/CLIPBOARD 更新の証拠ではないことを記録する
- したがって先行フィーチャーの AC-3（実機確認）は手動シナリオに紐づけたままとし、ユニット検証済みとしてマークしない

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし。本フィーチャーはテスト到達性の追加のみで、パフォーマンス目標を持たない。

### 5.2 セキュリティ要件

該当なし。認証・認可・データ保護・入力検証のいずれにも関わらない。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

| ID | 名称 | 内容 |
|----|------|------|
| NFR1 | ウィンドウ非依存の到達性 | 本フィーチャーが追加するすべてのテストは、`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` の下で、winit ウィンドウ・GPU サーフェス・イベントループ・ディスプレイサーバーなしに走る。新しいテストは `WindowHost` の構築に依存してはならない |
| NFR2 | decide_* の純粋性 | 判定関数（FR1 のコア判定と FR3 の宛先述語）は純粋である。値を取り、値を返し、I/O を行わず、グローバルに触れず、手渡された所有状態を超えて呼び出し側から見える状態を変更しない。副作用を伴う処理はすべてアダプタ側（FR2）かシンクの背後（FR4）に留まる |
| NFR4 | 識別子衝突の回避 | `drag_in_flight` はこのツリーで既に多重定義された識別子である（自由関数 pointer_routing.rs:364、`ButtonEventInputs` のフィールド mouse_report.rs:629、テストのローカル束縛 tests.rs:1889, 3345, 3863, 3969, 4054）。FR1/FR3/FR4 が導入する新しい名前は、この識別子に 4 つ目の意味を追加してはならず、既存のものを shadow してもならない |

### 5.5 互換性要件

| ID | 名称 | 内容 |
|----|------|------|
| NFR3 | 製品挙動の不変 | リファクタリングは出荷バイナリにおいて観測上中立である。あらゆる入力に対して、同じ宛先・同じペイロード・同じ順序・同じ状態変更・同じ戻り値となる。既存スイート全体が無修正で通る（tests.rs:1932、tests.rs:2448、tests.rs:2474 の 3 つのソーススキャンテストを含む） |

## 6. UI/UX要件

該当なし。本フィーチャーはユーザーから見える面・UI 要素・スタイリング・ユーザー操作のいずれも追加しない。

## 7. データ要件

該当なし。永続化するデータを持たない。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 新しいテストは inline `#[cfg(test)] mod tests` に置き、`--lib` で走らせる。`--bin emterm` ではテストが 0 件になる
- 本リポジトリに E2E 基盤はないため、E2E シナリオは追加せず `e2e_test_command` は空とする
- シームは pointer_routing.rs の publish パスに限定する。key_routing.rs の copy-chord クリップボード書き込みは、tests.rs:2474 が隣接条件付きでスキャンしているため `host.set_clipboard(&text);` のリテラル形を保つ
- どのプロジェクトルールファイルにもフォーマットコマンドの記載がない。`make fmt` は履歴由来であり、プロジェクトはクレート全体の `cargo fmt` を禁じているため、フォーマットは触ったファイルに限定する
- `host.set_primary` / `host.set_clipboard` は `src-tauri/src/window_host/mod.rs:705` と `:731` に定義されている。このファイルは完全にはスキャンされていないため、シームの製品実装は参照影響が部分的にしか判明していないファイルに入る

### 9.2 ビジネス上の制約

- 製品挙動の変更は禁止（NFR3）
- 既存の書き込みルールの修正・厳格化は禁止（FR6）

### 9.3 スケジュール制約

- FR8 のドキュメント追補は、新しいテストが通った後にのみ適用する

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/{feature}/**`
- `test-docs/{feature}/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/{feature}/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。
- この宣言はスーパーセットの主張であり、実際の変更集合は宣言に含まれる必要がある。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| `host.set_primary` / `host.set_clipboard` の定義ファイル（mod.rs:705, :731）が完全にはスキャンされておらず、シームの製品実装の参照影響が部分的にしか分かっていない | 中 | シームは pointer_routing.rs の publish パスに限定し、NFR3 の観測上の中立性を既存スイート全体で確認する |
| `drag_in_flight` が既に 3 つの意味を持つ多重定義識別子である | 中 | NFR4 に従い、新しい名前で 4 つ目の意味を作らず、既存を shadow しない |
| フォーマットコマンドがルールファイルに存在せず、`make fmt` は履歴由来である | 低 | クレート全体の fmt を避け、触ったファイルにフォーマットを限定する |
| 記録されたシンク呼び出しは書き込み要求の証拠であって、OS レベルの更新の証拠ではない | 中 | 先行フィーチャーの AC-3 を手動シナリオ（TS-8）に紐づけたままにする |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| リファクタリングが製品挙動を変えてしまう | 低 | 高 | NFR3 に従い、既存スイート全体（ソーススキャン 3 件を含む）を無修正で通す |
| FR6 の書き込みルールを「修正」してしまう | 低 | 中 | 現行ルールを新テストで固定し、知覚された欠陥はスコープ外とする |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: 素の `#[test]` が winit ウィンドウなしでフォーカス喪失後始末の合成を最後まで駆動し、ドラッグフラグのクリア、保留アンカーの消費、期待テキストでの PRIMARY 書き込みの記録を検証する
- [ ] AC-2: 宛先述語の (selection_present, copy_on_select, text_empty) の 8 通りすべてが素の `#[test]` で検証され、そのすべてで検証される宛先集合が FR6 のルールと一致する
- [ ] AC-3: 記録用シンクが宛先とペイロードの両方を観測し、「PRIMARY のみ」と「PRIMARY と CLIPBOARD」を区別でき、各宛先に書かれた正確なテキストを検証できる
- [ ] AC-4: `publish_local_drag` の名前・可視性・引数順・引数型・戻り値型が不変で、リテラル `local_drag_in_flight(host, &self.app)` と `publish_local_drag(host, &mut self.app)` が event_loop.rs にそのまま現れ、tests.rs:1932 が無修正で通る
- [ ] AC-5: Rust スイート全体が追加分を除き無修正で通り（`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`）、既存テストファイルのアサーションが弱められも削除もされず、CLI 専用のフィーチャーチェックも依然としてコンパイルされる
- [ ] AC-6: テストが通った後、mouse-report-reset-active-gesture の SPEC.md の TS-4 チェックボックスと VERIFICATION.md の TS-4 / SC-C 行が新しいテストを指し、同フィーチャーの AC-3 実機確認は手動シナリオに紐づいたままでユニット検証済みとしては提示されない

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1（unit / 境界値）: 宛先述語の truth table。(selection_present, copy_on_select, text_empty) の 8 通りすべてで期待宛先集合を検証する。選択なしなら何も書かない。選択ありなら空テキストを含め常に PRIMARY を書く。CLIPBOARD は `copy_on_select` かつ非空テキストのときのみ
- [ ] TS-2（unit / 正常系）: 状態コアの効果。ドラッグフラグが立ち保留アンカーがある状態では、コアはフラグをクリアし消費したアンカーを返す。どちらも無い状態では `None` を返し状態をクリーンに保つ
- [ ] TS-3（unit / 正常系）: 記録シンクのペイロード。`copy_on_select` 有効で非空選択を publish すると、同一テキストを運ぶ呼び出しがちょうど 2 回（PRIMARY → CLIPBOARD）記録される。`copy_on_select` 無効ならちょうど 1 回の PRIMARY 呼び出しが記録される
- [ ] TS-4（unit / 正常系）: winit ウィンドウなしのフォーカス喪失後始末エンドツーエンド。ガードが true、publish 側が走り、3 つの効果（フラグのクリア、アンカーの消費、選択の publish）を 1 つのテストですべて検証する。先行フィーチャーがソーススキャンでしか確認できなかったシナリオ
- [ ] TS-5（unit / 境界値）: 空選択の境界。テキストが空の解決済み選択でも PRIMARY 呼び出しは記録され、`copy_on_select` 有効でも CLIPBOARD 呼び出しは記録されない（FR6 の保存された挙動）
- [ ] TS-6（regression）: 既存のソーススキャンテスト 3 件（tests.rs:1932 fold トグル排他、tests.rs:2448 forwarded ブランチの clear、tests.rs:2474 copy-chord クリップボード隣接）が無修正で通る
- [ ] TS-7（build）: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が依然として成功し、GUI 専用の型が CLI 共有コードに漏れていないことを確認する
- [ ] TS-8（manual）: 先行フィーチャーの AC-3 の実機 PRIMARY/CLIPBOARD 確認は手動のまま。テキストを選択し、フォーカスを外し、別の場所に中クリックで貼り付ける。記録されたシンク呼び出しはこれの代用にならない

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| terminator | ローカルドラッグ終了処理。状態側（FR1）と publish 側から成る |
| publish 側 | 解決済み選択を PRIMARY / CLIPBOARD へ書き込む処理 |
| シンク（sink） | PRIMARY / CLIPBOARD の 2 つの書き込みを注入可能にする抽象（FR4） |
| 記録用テストダブル | 各呼び出しの宛先とペイロードを捕捉するシンク実装 |
| PRIMARY | X11 の primary selection |
| ソーススキャンテスト | `include_str!` でソース文字列を検査するテスト |

## 14. 確認事項

### 14.1 確認済み事項

- [x] a1: `publish_local_drag` の名前・引数順と、event_loop.rs:254 の呼び出しリテラル `publish_local_drag(host, &mut self.app)`（および :253 の `local_drag_in_flight(host, &self.app)`）は保存する。tests.rs:1932 が `include_str!` でこれらを固定しているため。sink_seam の回答が変えるのは書き込みの行き先であって、これらの綴りではない
- [x] a2: 書き込みルールはバイト単位で保存する（解決済み選択には空テキストを含め PRIMARY、CLIPBOARD は `copy_on_select` かつ非空テキストのときのみ、選択が無ければ何も書かない）。既存挙動をそのまま固定する
- [x] a3: fold トグルの排他性は目標ではなく不変条件である。`handle_fold_click` は event_loop.rs の外、publish 側の外に留まる（先行フィーチャーの D4、tests.rs:1932 が固定）
- [x] a4: ユニットテストは inline `#[cfg(test)] mod tests` に置き `--lib` で走らせる。プロジェクト自身の注記が `--bin emterm` ではテストが 0 件になることを記録している。新しいテストは `src-tauri/src/window_host/tests.rs` でこの慣習に従う
- [x] a5: 本リポジトリに E2E 基盤はないため E2E シナリオは追加せず、`e2e_test_command` は空とする
- [x] a6: `drag_in_flight` は多重定義された識別子である（自由関数、`ButtonEventInputs` のフィールド、テストのローカル束縛）。新しい名前はこれと衝突・shadow してはならない
- [x] a7: シームのシームは pointer_routing.rs の publish パスに限定する。key_routing.rs の copy-chord クリップボード書き込みは、tests.rs:2474 が隣接条件付きでスキャンしているためリテラル `host.set_clipboard(&text);` の形を保つ
- [x] a8: FR8 の追補は `feature-docs/mouse-report-reset-active-gesture/` 配下のドキュメント（SPEC.md、VERIFICATION.md）のみを編集し、新しいテストが通った後にのみ適用する。コードへの影響のないドキュメント訂正である

### 14.2 未確認・保留事項

なし。全機能要件・非機能要件は resolved である。

## 15. 参考資料

- 先行フィーチャー: `feature-docs/mouse-report-reset-active-gesture/SPEC.md`
- 先行フィーチャー: `feature-docs/mouse-report-reset-active-gesture/VERIFICATION.md`
- 対象コード: `src-tauri/src/window_host/pointer_routing.rs`、`event_loop.rs`、`mod.rs`、`tests.rs`
- ライセンス: MIT
