# Implementation Plan: Status Bar Native Port (egui)

## Overview

WebView 版 `src/status-bar/` の機能セットを native-poc (egui) ビルドへ完全移植する。同時に、Markdown ビューア移植でも再利用できる共通 HTML パーサー基盤を `native-poc/src/html/` として導入する。Phase 4-D で入った最小ステータスバー（clock のみ）を 3 段レイヤー（OSC / App Line 1 / App Line 2）へ拡張する。

## Objectives

- WebView 版と同等の機能セット（テンプレート、4 Provider、App Line 2、OSC 777;statusbar、HTML 解釈）を native-poc に実装する
- `native-poc/src/html/` を再利用可能な共通基盤として独立配置し、将来の Markdown ビューア移植で取り回せる API を提供する
- 既存の OSC 777 ルート（markdown/image/viewer）を壊さず `statusbar` サブコマンドを分岐する
- mux daemon の `StatusUpdateMsg` と native 側テンプレートを 3 段構造で共存させる
- 新規外部クレートをゼロ件に保ち、tokio を導入しない（既存方針）

## Prerequisites

### Development Environment

- Rust toolchain（`native-poc/Cargo.toml` の `edition` / `rust-version` に従う）
- Docker（Linux クロスビルド検証は任意。ホスト Linux 上の cargo check / cargo test で十分）
- `cargo fmt`
- `.claude/rules/native-poc-build-location.md` に従い `CARGO_TARGET_DIR=./target` (check/test) と `./target-host` (release) を使い分けること

### Dependencies

- 既存 native-poc コンポーネント（`crates/term_core`, `egui`, `wakeup`, `tabs`, `settings`, `callbacks`, `ui::status_bar`）
- Phase 4-D で導入済みの `StatusBarSettings { enabled, position }`、`status_bar_state()` メソッド
- mux `StatusUpdateMsg` 受信ロジック（`Tab::apply_mux_message`）

## Architecture Overview

### Technology Stack

- **Language**: Rust（native-poc クレート単独）
- **Framework**: egui 0.29（既存）
- **Key Libraries**:
  - `std::process::Command` - git / custom commands の同期実行
  - `std::thread` + `std::sync::{Arc, Mutex, Condvar, atomic::AtomicBool}` - worker threading
  - `libc` (Unix) / Windows API - ローカル時刻取得（`#[cfg(unix)]` / `#[cfg(windows)]` で分岐）
  - 既存 `wakeup` - worker → UI スレッド再描画通知

### Design Approach

- **責務の縦割り**: `html/`（テキスト処理）/ `status_bar/`（テンプレート＋プロバイダ＋OSC 振り分け）/ `ui/status_bar.rs`（egui 描画）の 3 層で凝集
- **共通基盤化**: HTML パーサーは status_bar 専用にせず、Markdown ビューア将来移植で `Node` enum 拡張のみで対応できるよう設計
- **worker thread 化**: 外部プロセス起動を伴う Provider（GitBranch / Command）は専用 worker で動かし、egui の render loop からは Arc<Mutex<Cache>> を読むだけにする
- **Provider オーナーシップ式 refresh-redraw**: 周期更新が必要な各 Provider は自前のタイマー / worker スレッドを持ち、コンストラクタで `Arc<Wakeup>` を受け取る。値更新時に `wakeup.wake()` を呼んで winit redraw をトリガする。`egui::Context::request_repaint_after` は winit に届かないため使わない（同じ罠は SPEC.md の Notes セクション参照）
- **winit `ApplicationHandler::user_event` の実装必須**: `wakeup.wake()` は `EventLoopProxy::send_event(())` を呼ぶ。winit 0.30 では `ApplicationHandler::user_event` がデフォルトで no-op のため、これを `PocApp` でオーバーライドして `host.window().request_redraw()` を呼ばないと UserEvent が握り潰され、Provider オーナーシップ式 wake チェーン全体が機能停止する（同じ罠は SPEC.md の Notes セクション参照）
- **依存追加なし**: regex / tokio / html5ever は引かない。テンプレートと HTML はそれぞれ単一パス・手書きスキャナで処理

### Component Interaction

```
PTY bytes
  -> term_core process_pty_data
  -> NativeCallbacks::on_osc(100, payload)
       -> try_dispatch_statusbar(&dispatcher, payload)  ── true → osc_layer 更新
                                                       └─ false → 既存 osc_queue へ
  
mux APC frame
  -> Tab::apply_mux_message(StatusUpdate)
  -> tab.mux_status_state

per-frame:
  App::status_bar_view_model()
    -> osc_layer (mux StatusUpdateMsg 優先、無ければ OSC 777 由来)
    -> app_line1 / app_line2: TemplateEngine.resolve → html::parse → to_rich_text_runs
  ui::status_bar::draw(ctx, view_model, settings)

worker threads (git_branch / custom commands):
  loop { sleep interval → spawn process → 5s timeout → write Arc<Mutex<Cache>> → version_counter += 1 → wakeup.wake() }

timer thread (time provider):
  loop { Condvar::wait_timeout(refresh_rates["time"] default 1000ms) → wakeup.wake() }

cwd event path (no thread):
  NativeCallbacks::on_osc(OSC_CWD, ...) → update cb_state.cwd → wakeup.wake()
```

各 Provider は構築時に `Arc<Wakeup>` を受け取り、これを共有して winit redraw をトリガする。`render/mod.rs` 側に存在した `ctx.request_repaint_after(Duration::from_secs(1))` は egui 内部に閉じてしまい winit にイベントが届かないため、本タスクで削除する（SPEC.md の Notes セクション参照）。

## Implementation Phases

### Phase A: HTML Parser Foundation

**Goal**: status_bar 実装が依存する前に、再利用可能な HTML パーサー基盤を独立モジュールとして完成させる。

**Files to Create**:

- `native-poc/src/html/mod.rs` - 公開 API 再エクスポート（`parse`, `Node`, `CssColor`, `RichTextRun`, `to_rich_text_runs`, `strip_html_tags`）
- `native-poc/src/html/tokenizer.rs` - タグ / エンティティ / テキストの単一パストークナイザ
- `native-poc/src/html/parser.rs` - トークン列 → `Vec<Node>` 構築（インライン subset、寛容モード）
- `native-poc/src/html/sanitizer.rs` - `strip_html_tags()` 実装
- `native-poc/src/html/rich_text.rs` - `Vec<Node>` → `Vec<RichTextRun>` 変換

**Files to Modify**:

- `native-poc/src/main.rs` または `native-poc/src/lib.rs`（pub mod html; を追加）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `parse(input)` | HTML 文字列をインライン subset の AST に変換 | input は UTF-8 文字列 | 戻り値は `Vec<Node>`。`<script>`/`<style>` のタグおよび内容は AST から消える |
| `Node` enum | インライン要素を表現。将来 Block 系を追加しても既存バリアント壊さない | - | `Text` / `LineBreak` / `Span{color, children}` / `Bold` / `Italic` / `Underline` を提供 |
| `strip_html_tags(input)` | すべてのタグを剥がしテキストのみ残す。`<script>`/`<style>` は本文も削除 | input は UTF-8 | 非 HTML の角括弧（例: `1 < 2`）は保持 |
| `to_rich_text_runs(nodes, theme)` | AST をネスト解消してフラットな run 列に展開 | nodes はインライン subset | 各 run に bold/italic/underline/color を畳み込み済み |
| `CssColor` | パースした色情報の中間表現 | - | `Hex` / `Rgb` / `Named` のいずれか。`egui::Color32` に変換するヘルパを提供 |
| `RichTextRun` | egui の widget 層へ渡す中間構造 | - | テキスト + per-run スタイル属性を持つ |

**Processing Flow** (parser, diagram-convertible):

1. tokenize 開始
   - 文字種別ごとに「タグ開始」「タグ終了」「自己閉じタグ `<br/>`」「エンティティ `&...;`」「テキスト」に分類
   - 不正な `<` で始まる断片はテキストとして扱う
2. parse ループで開いているタグのスタックを保持
   - 開始タグ → 対応する Node を push、子要素受け取りモードに遷移
   - 終了タグ → スタックを巻き戻し
     - 一致 → 通常クローズ
     - 不一致 → debug ログ + 該当タグまでクローズ（寛容処理）
   - `<script>` / `<style>` 開始 → "swallow until close" モードに遷移し、子要素を捨てる
   - 未知タグ → ラッパーを落として子要素のみ採用
3. エンティティ展開
   - `&amp; &lt; &gt; &quot; &apos;` および `&#NN;` の数値参照をデコード
   - 未知エンティティは `&entity;` リテラルとして残す（debug ログ）

**Implementation Steps** (5-7 max):

1. **トークナイザ着工** - 単一パス・状態機械でタグ/エンティティ/テキストを切り出す。`<` の解釈は browser 寛容モードに揃える
2. **`Node` enum と CssColor を定義** - 将来 Markdown ビューア用 variant（Block/Link/Image）追加で既存呼び出し側が壊れない順序・形にする
3. **parser 構築** - スタックベースで `<script>`/`<style>` swallow と未知タグ寛容化を組み込む
4. **`strip_html_tags` 実装** - tokenizer を共用しつつ、タグ列を捨ててテキストだけ連結する（`<script>`/`<style>` 本文も削除）
5. **`to_rich_text_runs` 実装** - AST をネスト解消して run 列にフラット化。bold/italic/underline は AND 結合、color は子ノード優先
6. **ユニットテスト整備** - 入れ子（`<b><i>x</i></b>`）、エンティティ（`&amp; &lt; &#65;`）、未知タグ、`<script>` 除去、malformed close など

**Dependencies**: なし（独立モジュール）。Blocks: Phase C / D / E

**Testing Approach**:

- Unit: parser / tokenizer / strip_html_tags / to_rich_text_runs / CssColor 解釈の網羅テスト
- Integration: Phase D で OSC dispatcher と合わせて strip_html_tags の振る舞いを統合確認
- E2E: なし（純粋ライブラリ）
- Manual: なし

**Acceptance Criteria**:

- [ ] `native-poc/src/html/` 配下のテストが `cargo test --bin emterm-native-poc html::` で全 pass
- [ ] `Node` enum は将来 Block 系追加で既存パターンマッチが壊れない構造（`#[non_exhaustive]` または変種追加可能な形）
- [ ] `strip_html_tags("1 < 2 <b>bold</b> <script>evil()</script>tail") == "1 < 2 bold tail"`（または同等の意味の振る舞い）
- [ ] WebView 版 `src/status-bar/osc-controller.ts` の `stripHtmlTags` と同じケースで同じ出力

**Estimated Effort**: medium

---

### Phase B: Settings Extension

**Goal**: `StatusBarSettings` を WebView 版互換のフィールド集合に拡張し、`CustomCommand` 型を追加する。JSON loader 連携はしない（Phase 7 別タスク）。`Settings::default()` のみ動作させる。

**Files to Modify**:

- `native-poc/src/settings.rs` - `StatusBarSettings` フィールド追加、`CustomCommand` 型追加、`Default` 実装

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `StatusBarSettings` | 拡張後のフィールド集合を保持 | 既存 `enabled` / `position` は維持 | 新規 `app_line1_left`, `app_line1_right`, `app_line2_left`, `app_line2_right`, `time_format`, `font_size`, `custom_commands`, `refresh_rates` を公開 |
| `CustomCommand` | カスタムコマンドの仕様 | name は `[a-zA-Z0-9_-]+` を満たす（呼び出し側で検証） | `executable: String`, `interval_ms: u64`（デフォルト 1000）を保持 |
| `Default for StatusBarSettings` | デフォルト値 | - | `app_line1_left = "{time}"`, `app_line1_right = "{cwd}"`, `time_format = "HH:mm:ss"`, 他は空 |

**Implementation Steps**:

1. **`CustomCommand` 型を追加** - executable と interval_ms のみのフィールド。`Default` で `interval_ms = 1000`。`#[derive(Debug, Clone, PartialEq, Eq)]`（HashMap value として要 Clone、Copy は文字列のため不可）
2. **`StatusBarSettings` の `derive` 調整** - 現状は `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`。`String` / `HashMap` フィールド追加で `Copy` は外す必要あり → `#[derive(Debug, Clone, PartialEq, Eq)]` に変更。呼び出し側（テスト含む）が `Copy` セマンティクスに依存していないか確認（`settings.statusbar` のフィールドアクセスは値コピー前提でなければ問題ない）
3. **`StatusBarSettings` フィールド拡張** - 既存 2 フィールドを温存しつつ追加。HashMap 系はデフォルト空
4. **`Default` 実装更新** - SPEC FR10 の規定値に揃える
5. **既存テストの maintenance** - Phase 4-D 由来テストが `StatusBarSettings::default()` を呼んでいたら新フィールドの挙動も加味してアサート追加
6. **ユニットテスト追加** - デフォルト値が SPEC 通りであることを検証

**Dependencies**: なし。Blocks: Phase C（テンプレート文字列の供給源）/ Phase E（ViewModel が読む）

**Testing Approach**:

- Unit: `StatusBarSettings::default()` のフィールド値検証、`CustomCommand::default()` 検証
- Integration: Phase E で View Model 経由の動作確認
- E2E: なし
- Manual: なし

**Acceptance Criteria**:

- [ ] `Settings::default().statusbar.app_line1_left == "{time}"`
- [ ] `Settings::default().statusbar.app_line1_right == "{cwd}"`
- [ ] `Settings::default().statusbar.time_format == "HH:mm:ss"`
- [ ] `Settings::default().statusbar.custom_commands` は空
- [ ] 既存の `StatusBarSettings::default()` を参照するコードがコンパイル可能

**Estimated Effort**: small

---

### Phase C: Template Engine + Providers

**Goal**: テンプレート文字列 → 解決済み文字列の変換を、UI から独立した形で実装する。4 つの Provider（Time / Cwd / GitBranch / Command）を含む。Worker thread インフラを GitBranch / Command に導入する。

**Files to Create**:

- `native-poc/src/status_bar/mod.rs` - モジュールルート。`StatusBarRuntime`（Phase E で完成）と `ViewModel` 型の置き場
- `native-poc/src/status_bar/template_engine.rs` - `TemplateEngine`, `VariableProvider` trait
- `native-poc/src/status_bar/providers/mod.rs` - Provider 群の再エクスポート + 共通 trait 実装ガイド
- `native-poc/src/status_bar/providers/time.rs` - `TimeProvider`
- `native-poc/src/status_bar/providers/cwd.rs` - `CwdProvider`（active tab の `cb_state.cwd` を読む）
- `native-poc/src/status_bar/providers/git_branch.rs` - worker thread 付き `GitBranchProvider`
- `native-poc/src/status_bar/providers/command.rs` - worker thread 付き `CommandProvider`

**Files to Modify**:

- `native-poc/src/main.rs` または `native-poc/src/lib.rs` - `pub mod status_bar;`
- `native-poc/src/wakeup.rs` - worker 側からの `poke()` 呼び出しを許容（既存 API のままで良いか確認）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `VariableProvider` trait | `get_value()` と任意の `get_color() -> Option<CssColor>` | Send + Sync 制約 | `get_value` は同期で即時 String を返す（IO は内部 worker に隔離済み） |
| `TemplateEngine` | プロバイダの登録／取消／検索／テンプレート解決 | プロバイダは名前で一意 | `resolve(template)` は 1 ms 未満（NFR1）で完結 |
| `extract_variables(template)` | テンプレート文字列に出現する `{name}` を抽出 | name 正規表現は `[a-zA-Z_][a-zA-Z0-9_]*(?::[a-zA-Z0-9_-]+)?` | 重複は重複含む。手書きスキャナで実装、`regex` クレート不使用 |
| `TimeProvider` | ローカル時刻を `time_format` トークンで整形＋自前タイマースレッドで `wakeup.wake()` を呼ぶ | format は `YYYY MM DD HH hh mm ss A` の組合せ、コンストラクタで `Arc<Wakeup>` と `RefreshConfig { interval: Duration }` を受け取る | 最長トークン優先で置換、AM/PM 境界（正午/深夜 0 時）を正しく扱う。タイマースレッドは `refresh_rates["time"]`（default 1000ms）で `wake()`、Drop で join。`get_value()` は呼ばれた時に `Instant::now()` を計算する pull 方式（タイマーは repaint トリガ専用） |
| `CwdProvider` | active tab の cwd 末尾セグメント。polling なし、OSC 7 受信イベントで wake | OSC 7 受信済み or 未受信時は空。コンストラクタで `Arc<Wakeup>` を受け取る | `/`, `C:\`, `file://host/path`（host 除去 + percent-decode）を扱う。`set_cwd()` 呼び出し時（OSC 7 受信パス）に `wakeup.wake()` を呼ぶ。タイマースレッドは持たない |
| `GitBranchProvider` | worker thread で `git rev-parse` / `git status --porcelain` を回す | git 実行可能であること（無ければ空）。コンストラクタで `Arc<Wakeup>` を受け取る | branch + 状態色を `Arc<Mutex<GitCache>>` に格納。`get_color()` で色返却。refresh 完了時に version counter bump + `wakeup.wake()` |
| `CommandProvider` | worker thread で単一 executable を回す | name 検証は呼び出し側が事前実施。コンストラクタで `Arc<Wakeup>` を受け取る | stdout 1 行目を保持。失敗・タイムアウトで前値維持（FR6）。refresh 完了時に version counter bump + `wakeup.wake()` |
| Worker base | sleep / spawn / 5s timeout / wakeup を共通化（モジュール内 free fn or helper struct） | `Arc<AtomicBool>` で停止可能、`Arc<Wakeup>` を引き回す | child を `kill()` で確実に終了させる。値更新時に渡された `Arc<Wakeup>` から `wake()` を呼ぶ |

**Processing Flow** (TemplateEngine::resolve, diagram-convertible):

1. テンプレート文字列を head から走査
   - `{` を発見 → 後続を変数名スキャナへ
     - 正規パターンに合致 → provider テーブル lookup
       - 見つかった → `get_value()` 取得
         - `get_color()` が `Some` → `<span style="color:...">value</span>` でラップ
         - `None` → 値のみ
       - 見つからない → 空文字に置換（不明変数は空）
     - 非合致 → リテラル `{` として吐く
   - 通常文字 → そのまま出力
2. 結果は HTML 断片（`<span>` などを含み得る）として返却。後段の `html::parse` でパースされる

**Processing Flow** (GitBranchProvider worker, diagram-convertible):

1. `Condvar::wait_timeout(refresh_rates["git_branch"] default 5000ms)` で待機
   - `stop` フラグ → ループ抜ける
   - タイムアウト → 次へ
2. `cwd_source()` で現在の active tab の cwd 取得
   - 空 / 変化なし & cache 非空 → スキップして 1 へ
3. `git rev-parse --abbrev-ref HEAD` を spawn、5s timeout
   - timeout → `child.kill()`、前値維持
   - 空 or `fatal:` → cache クリア
   - 通常 → branch 名確定 → 次へ
4. `git status --porcelain` を spawn、5s timeout
   - timeout → 前値維持
   - 出力分類:
     - 完全に空 → `clean` (`#4caf50`)
     - 非 `??` 行あり → `dirty` (`#f9a825`)
     - `??` のみ → `untracked` (`#9e9e9e`)
5. cache 更新 + `wakeup::wake()` 呼び出し → 1 へ

**Implementation Steps**:

1. **`VariableProvider` trait + `TemplateEngine` の単一パススキャナ実装** - 変数抽出と置換を 1 パスで。エスケープ規則は SPEC に従う
2. **`TimeProvider` 実装** - 長いトークン優先の置換テーブル。Unix は `libc::localtime_r`、Windows は `GetLocalTime` を `#[cfg]` で分岐。`new(wakeup: Arc<Wakeup>, refresh: RefreshConfig)` で受け取り、内部で `Condvar::wait_timeout(refresh.interval)` ループのタイマースレッドを起動。Drop で `stop = true` + `notify_all` + `join`。`get_value()` 自体は呼ばれた時刻で format
3. **`CwdProvider` 実装** - basename 抽出ヘルパに `/`, `C:\`, `file://` を網羅。値の供給源は `Arc<dyn Fn() -> Option<String> + Send + Sync>` のクロージャ。`new(wakeup: Arc<Wakeup>, cwd_source: ...)` で `Arc<Wakeup>` を保持し、`set_cwd()`（OSC 7 受信パスから呼ばれるエントリポイント）内で `wakeup.wake()` を呼ぶ。タイマースレッドは持たない
4. **worker thread ヘルパ** - sleep + spawn + 5s timeout + kill のテンプレートを `git_branch.rs` / `command.rs` で共有できる形に切り出す。`Arc<Wakeup>` も引き回し、cache 更新後に `wake()` を呼ぶ
5. **`GitBranchProvider` worker 実装** - `new(wakeup: Arc<Wakeup>, ...)` でコンストラクト。cache 構造、Drop による join、worker は値更新後に `wakeup.wake()` 呼び出し（既存設計を維持）
6. **`CommandProvider` worker 実装** - `new(wakeup: Arc<Wakeup>, ...)` でコンストラクト。name 検証関数（`is_valid_command_name`）、`~/` 展開（`$HOME` / `%USERPROFILE%`）、interval_ms クランプ、worker は値更新後に `wakeup.wake()` 呼び出し
7. **ユニットテスト整備** - 各 Provider 単体 + Engine 統合（unknown variable, span ラップ）。Wakeup 注入は `Arc<Wakeup>` を `Arc::new(Wakeup::no_op())` 相当のテスト用ダブルで差し替えできるよう設計

**Dependencies**: Requires: Phase A（戻り値 string に HTML が混じる）/ Phase B（time_format 等の供給源）。Blocks: Phase E（Runtime 組み立て）

**Testing Approach**:

- Unit:
  - `TemplateEngine::extract_variables` パターン網羅
  - `TemplateEngine::resolve` 未登録変数 = 空 / 色付きラップ
  - `TimeProvider::format_with` 全トークン、AM/PM 境界
  - `CwdProvider` basename 抽出（Unix / Windows / file URI / root）
  - `GitBranchProvider::parse_branch` / `parse_status` 分類
  - `CommandProvider` name 検証、`~/` 展開、interval clamp
- Integration: 4 Provider 登録 → `resolve("{time} {cwd} {git_branch} {cmd:foo}")` のスモーク
- E2E: なし（worker thread は spawn を mock 化せず本物の `echo` 等で fixture テストできるなら可）
- Manual: なし

**Acceptance Criteria**:

- [ ] `cargo test --bin emterm-native-poc status_bar::template_engine` 全 pass
- [ ] `cargo test --bin emterm-native-poc status_bar::providers` 全 pass
- [ ] GitBranch / Command worker は Drop で必ず join される（テストで確認）
- [ ] TimeProvider タイマースレッドも Drop で必ず join される（テストで確認、TS-perf-3）
- [ ] 4 Provider すべて `Arc<Wakeup>` をコンストラクタで受け取る（CwdProvider は polling なしだが OSC 7 イベントで wake するため受け取る）
- [ ] tokio / regex / 新規 crate なし

**Estimated Effort**: large

---

### Phase D: OSC 777;statusbar Dispatcher

**Goal**: PTY 経由の `OSC 777;statusbar;...` を専用ディスパッチャに振り分け、`OscLayerState` を更新する。既存の emterm-extension 経路（markdown/image/viewer）は壊さない。

**Files to Create**:

- `native-poc/src/status_bar/osc_dispatcher.rs` - `StatusBarOscDispatcher`, `OscLayerState`, `try_dispatch_statusbar`

**Files to Modify**:

- `native-poc/src/callbacks.rs` - `NativeCallbacks` に `statusbar_dispatcher: Arc<StatusBarOscDispatcher>` を追加し、`on_osc(OSC_EMTERM_EXTENSION, data)` 内で `try_dispatch_statusbar` を先に試行

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `OscLayerState` | OSC レイヤーの left / right / forced_visible 保持 | Arc<Mutex<...>> で共有 | `left` / `right` は `strip_html_tags` 適用後 |
| `StatusBarOscDispatcher::handle(tokens)` | トークン列に応じて state を更新 | 呼び出し側で `statusbar` prefix は外している | set / clear / show / hide / 不正コマンドの分岐網羅 |
| `try_dispatch_statusbar(dispatcher, payload)` | payload が `statusbar;` で始まれば消費 | payload は OSC 777 の content | true 返却 = 消費済み、false = 既存 osc_queue へ落とす |

**Processing Flow** (dispatcher, diagram-convertible):

1. payload を `;` で 1 回 split し先頭トークンを確認
   - `statusbar` でない → false 返却（既存 osc_queue へフォールバック）
   - `statusbar` である → 残りトークンを `handle()` に委譲
2. `handle(tokens)` 分岐:
   - `["set", "left", content]` → `state.left = strip_html_tags(content)`、`forced_visible = Some(true)`
   - `["set", "right", content]` → `state.right = strip_html_tags(content)`、`forced_visible = Some(true)`
   - `["clear"]` → `left = ""`, `right = ""`
   - `["clear", "left"]` → `left = ""`
   - `["clear", "right"]` → `right = ""`
   - `["show"]` → `forced_visible = Some(true)`
   - `["hide"]` → `forced_visible = Some(false)`
   - その他 → debug log + 無視
3. 更新後 `wakeup::wake()`（or 既存の `request_repaint` 経路）でフレームを起こす

**Implementation Steps**:

1. **`OscLayerState` 型定義** - left / right / forced_visible
2. **`StatusBarOscDispatcher::handle` 実装** - サブコマンド分岐とログ
3. **`try_dispatch_statusbar` 実装** - payload 先頭判定 + tokens 切り出し（`;` content に複数 `;` を含むケースのため `splitn` 戦略を採用）
4. **`NativeCallbacks` 改修** - `statusbar_dispatcher` フィールド追加、`on_osc(100, ..)` のフォールバック分岐を実装
5. **ユニットテスト** - 全サブコマンド round-trip、不明コマンド・不明セクションのログ確認
6. **統合テスト** - `NativeCallbacks::on_osc(100, "statusbar;set;left;Hi")` で `OscLayerState.left == "Hi"` 確認、`on_osc(100, "markdown;...")` で `osc_queue` 側に積まれる回帰確認。既存テスト `osc_100_emterm_extension_pushes_to_queue` (callbacks.rs) は `markdown;hello` ペイロードを使うため、Phase D の `try_dispatch_statusbar` 差し込み後も `statusbar;` 以外は素通りすればこのテストは変更不要

**Dependencies**: Requires: Phase A（`strip_html_tags`）。Blocks: Phase E（runtime に dispatcher を組み込む）

**Testing Approach**:

- Unit:
  - 全サブコマンド分岐
  - HTML タグ stripping（`<script>x</script>foo` → `"foo"`）
  - 不明 subcommand / 不明 section ログのみ
- Integration: `NativeCallbacks::on_osc(100, ...)` の振り分け回帰
- E2E: 後段 manual（`printf '\033]777;statusbar;set;left;hi\033\\'`）
- Manual: なし（Phase E manual に含める）

**Acceptance Criteria**:

- [ ] `cargo test --bin emterm-native-poc status_bar::osc_dispatcher` 全 pass
- [ ] `on_osc(100, "statusbar;...")` が新 dispatcher に流れる
- [ ] `on_osc(100, "markdown;...")` が `osc_queue` に積まれる（回帰なし）

**Estimated Effort**: medium

---

### Phase E: View Model + UI Integration

**Goal**: Phase 4-D の `StatusBarState` を `StatusBarViewModel`（3 行構造）に置き換え、`StatusBarRuntime` を `App` に持たせ、`ui::status_bar::draw` を 3 行レイアウトに書き換える。mux 接続時の OSC レイヤー入力経路も統合する。

**Files to Create**:

- `native-poc/src/status_bar/runtime.rs` - `StatusBarRuntime`（TemplateEngine + Providers + OscDispatcher 所有）と `build_view_model()` メソッド
- `native-poc/src/status_bar/view_model.rs` - `StatusBarViewModel`, `OscRow`, `AppRow` 型（または `mod.rs` 内）

**Files to Modify**:

- `native-poc/src/app.rs` - `App` に `status_bar_runtime: StatusBarRuntime` を追加、`status_bar_state()` を `status_bar_view_model()` に置換。`StatusBarRuntime::new(wakeup: Arc<Wakeup>, settings)` で構築し、内部で各 Provider に `Arc<Wakeup>` を注入する
- `native-poc/src/ui/status_bar.rs` - 3 行レイアウト、`left_to_right` / `right_to_left` の left/right セクション、`forced_visible` と自動非表示ロジック（FR12）
- `native-poc/src/render/mod.rs` - `app.status_bar_state()` 呼び出しを `app.status_bar_view_model()` に差し替え（現状 `render/mod.rs:108` で呼ばれている）。**併せて `ctx.request_repaint_after(Duration::from_secs(1))` (現状 `render/mod.rs:151` 付近) を削除する**。winit redraw は各 Provider の `wakeup.wake()` 経由で発火するため、render 側からの periodic repaint trigger は不要かつ機能しない（SPEC.md の Notes 参照）
- `native-poc/src/callbacks.rs` - `statusbar_dispatcher` を `StatusBarRuntime` 経由で共有（Phase D のフィールドを runtime 所有に切替）。`NativeCallbacks::on_osc(OSC_CWD, ...)` の cwd 更新パスから CwdProvider の `set_cwd()` を呼ぶことで、内部の `wakeup.wake()` 経由で次フレームを起こす（既存の cwd 反映パスを CwdProvider 経由に統一）
- `native-poc/src/tabs.rs`（必要なら）- mux disconnect 検出（`mux_session_name` Drop）時のフック
- **`native-poc/src/window_host.rs`** - `impl ApplicationHandler for PocApp` に `user_event(&mut self, _: &ActiveEventLoop, _: ())` メソッドを追加。`if let Some(host) = &self.host { host.window().request_redraw(); }` を呼ぶ。これにより `EventLoopProxy::send_event(())` 経路で発火した `UserEvent(())` が確実に redraw 要求に転換され、Provider 由来の `wake()` チェーンが完成する（SPEC.md Notes 参照）

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `StatusBarRuntime` | TemplateEngine + Provider 群 + OscDispatcher を集約所有 | App 起動時に一度だけ構築 | `build_view_model(active_tab, settings)` で per-frame snapshot を返す |
| `StatusBarViewModel` | 3 行 + 共通設定（enabled / position / font_size）を保持 | - | `osc`: OscRow / `app_line1` / `app_line2`: AppRow（`Vec<RichTextRun>`） |
| `OscRow` | 生 string（mux StatusUpdateMsg 由来は verbatim、OSC 777 由来は strip 済み） | - | `left`, `right`, `forced_visible: Option<bool>` |
| `AppRow` | テンプレート解決 + html::parse + to_rich_text_runs の結果 | left/right 各々 | `Vec<RichTextRun>` |
| `ui::status_bar::draw` | TopBottomPanel に 3 行を縦並びレイアウト | view_model 非 None | row 自動非表示（FR12）、共有背景、開閉アニメ無し（NFR4） |
| Mux 統合 | `tab.mux_status_state` を OSC 行に流し込み | active tab に依存 | 切断検出時 OSC 行を空に |
| `PocApp::user_event` | winit `UserEvent(())` 受信時に active window へ `request_redraw()` を要求 | `host` が Some（init 後） | `EventLoopProxy::send_event(())` 由来の wake が確実に redraw に転換される。host 未確定（init 前）は no-op |

**Processing Flow** (`App::status_bar_view_model`, diagram-convertible):

1. `settings.statusbar.enabled` 確認
   - false → `ViewModel { enabled: false, ..Default }` 返却（描画側で即 skip）
2. active tab を解決
3. OSC 行入力決定:
   - `tab.mux_status_state` あり → daemon の left/right を verbatim 採用
   - 無し → `OscLayerState` の left/right を採用（dispatcher 経由で更新済み）
   - `forced_visible` を伝搬
4. App Line 1 / Line 2 構築:
   - 各 side について `template_engine.resolve(template)` → `html::parse(&resolved)` → `to_rich_text_runs(&nodes, theme)`
   - 結果を `(template_str, provider_version_tuple)` キーでキャッシュ参照（Phase F で実装）。Phase E では「毎フレーム計算」で十分（NFR1 達成は Phase F で）
5. `font_size` / `position` / `mux_session_name` を ViewModel に詰めて返却

**Processing Flow** (`ui::status_bar::draw`, diagram-convertible):

1. `view_model.enabled` が false → 何もしない（パネルを挿入しない）
2. `TopBottomPanel` を `position` に応じて top / bottom に作成
3. 各 row について、visibility 判定:
   - `osc`: `left` も `right` も空 かつ `forced_visible != Some(true)` → skip。`forced_visible == Some(false)` → 強制 skip
   - `app_line1`: 常に描画
   - `app_line2`: 両 side empty → skip
4. 描画する row のみ 1 行ずつ `ui.horizontal` で並べ、`left_to_right` で left、`right_to_left` で right を流す
5. 全 row は共有背景（NFR4）、`font_size` が `Some` ならフォント設定を上書き

**Implementation Steps**:

1. **`StatusBarViewModel` / `OscRow` / `AppRow` 型を定義** - 後続の draw / runtime 両方から見える形で配置
2. **`StatusBarRuntime` を作る** - TemplateEngine + 4 Provider + `OscDispatcher` 所有。`StatusBarRuntime::new(wakeup: Arc<Wakeup>, settings: &Settings)` のシグネチャで構築し、各 Provider のコンストラクタに `Arc::clone(&wakeup)` を渡す。Drop で worker / timer thread すべてを停止
3. **`App` を統合** - 既存 `status_bar_state()` を `status_bar_view_model()` に置換、Phase 4-D 由来の clock 描画コードは新 ViewModel に吸収。`App` 構築時に `Arc<Wakeup>` を `StatusBarRuntime` に渡す。`render/mod.rs` の `ctx.request_repaint_after(Duration::from_secs(1))` 呼び出しを削除する
4. **`ui::status_bar::draw` 改修** - 3 行描画 + 自動非表示 + 共有背景。`Vec<RichTextRun>` から `egui::RichText` を組み立てる
5. **mux 統合** - `Tab::apply_mux_message(StatusUpdate)` 経路は既存維持。`status_bar_view_model()` は毎フレーム以下の優先順で OSC 行を埋める:
   1. `tab.mux_session_name.is_some() && tab.mux_status_state.is_some()` → daemon の left/right を verbatim 採用
   2. 上記以外 → `OscDispatcher` の `OscLayerState` の left/right を採用
   3. どちらも空 → OSC 行は非表示（FR12）

   ただし SPEC US5 / FR11 が要求する「mux disconnect 時の OSC layer clear」を満たすため、`StatusBarRuntime` に「前フレームの mux 接続状態」を持たせる:
   - `prev_mux_attached: Mutex<bool>` を runtime に追加
   - `build_view_model` の冒頭で `mux_session_name.is_some()` を観察し、`true → false` の falling edge を検出したら `OscDispatcher::handle(&["clear"])` 相当 + `forced_visible` リセットを実行
   - これにより mux 接続中に dispatcher に残っていた古い OSC 777 状態（または mux daemon 由来の残骸）が、disconnect 直後の経路 2 フォールバックで再表示されることを防ぐ
   - 「OSC 777 経路を永続的に塞ぐ」わけではない: clear はあくまで「pre-disconnect 状態の flush」で、disconnect 後に新規 `statusbar;set;…` が来れば通常通り表示される

   `tabs.rs` への変更は引き続き不要（disconnect 検出は runtime 側に閉じる）。
6. **Phase 4-D テストの書き換え** - `disabled_status_bar_does_not_insert_panel` 系の挙動を新 ViewModel に書き換え。`enabled=false` で `ViewModel::enabled = false` であること等
7. **`PocApp::user_event` 実装** - `window_host.rs` の `impl ApplicationHandler for PocApp` に `user_event(&mut self, _: &ActiveEventLoop, _: ())` を追加し、`host` が Some の場合に `host.window().request_redraw()` を呼ぶ。これにより `Wakeup::wake()` → `EventLoopProxy::send_event(())` → `UserEvent(())` → `user_event` → `request_redraw()` のチェーンが完成し、Provider 由来の wake が PTY 出力ゼロ条件下でも確実に redraw を引き起こす。ユニットテスト TS-32（mock window で `request_redraw` が呼ばれることの検証）を追加

**Dependencies**: Requires: Phase A / B / C / D。Blocks: Phase F（cache）

**Testing Approach**:

- Unit:
  - `build_view_model` の `enabled=false` 経路
  - mux StatusUpdateMsg 経由で OSC 行が埋まること
  - OSC 777 経由（dispatcher）で OSC 行が埋まること、mux 切断時に空になること
  - App Line 2 両 side empty → AppRow が空 vec
- Integration:
  - `ui::status_bar::draw` 後の egui ペイントノードを test で確認できる範囲（既存 `disabled_status_bar_does_not_insert_panel` パターン）
  - Phase 4-D 既存テストを新 ViewModel 形に書き換え、回帰なし
- E2E: なし（sdd.6 で実機 smoke）
- Manual:
  - 起動 → デフォルトで時計 + cwd 表示
  - mux 接続 → OSC 行に daemon の left/right 表示、App Line 1/2 はローカルテンプレート
  - mux 切断 → OSC 行クリア

**Acceptance Criteria**:

- [ ] Phase 4-D の `cargo test --bin emterm-native-poc` 既存テストを新 ViewModel ベースで全 pass
- [ ] `enabled = false` で TopBottomPanel が挿入されない
- [ ] mux 接続時に OSC 行が daemon 由来 verbatim になる
- [ ] OSC 777;statusbar 経由でも OSC 行が更新される（mux 非接続時）
- [ ] App Line 2 両 side empty で行が描画されない
- [ ] 開閉アニメーションを追加していない / OSC 行に独自背景を追加していない（NFR4）
- [ ] `render/mod.rs:151` 付近の `ctx.request_repaint_after(Duration::from_secs(1))` 呼び出しが削除されている
- [ ] `StatusBarRuntime` 構築時に `Arc<Wakeup>` が各 Provider へ注入されている（コードレビューで確認、TS-31 で実機相当の動作確認）
- [ ] `PocApp` の `impl ApplicationHandler` に `user_event` メソッドが存在し、`host.window().request_redraw()` を呼ぶ（コードレビュー + TS-32 ユニットテストで確認）
- [ ] TS-30（release-build idle-clock 10 秒 smoke）で時計が秒単位で実際に更新される

**Estimated Effort**: large

---

### Phase F: Polish + Verification

**Goal**: NFR1（< 1 ms / フレーム）達成のため run-list キャッシュを導入し、smoke 確認と release build を整える。

**Files to Modify**:

- `native-poc/src/status_bar/runtime.rs` - `(template_str, provider_version_tuple) -> Vec<RichTextRun>` の LRU キャッシュ
- `native-poc/src/status_bar/providers/*.rs` - 各 Provider に `version: u64` を追加。値が変わったときだけインクリメント
- `native-poc/README.md`（存在すれば）- ステータスバー機能の概観追記

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Per-Provider version counter | 値が変わった時のみインクリメント | 並行 worker 書き換えと衝突しないよう `AtomicU64` | キャッシュキーの一部 |
| Run-list LRU | 4 セクション × 直近テンプレートを保持 | 直近 N（例: 16）保持 | hit 時 html::parse をスキップ |

**Processing Flow** (cache lookup, diagram-convertible):

1. 各 row 描画前に `(template_str, version_tuple)` を計算
2. LRU を lookup
   - hit → 既存 `Vec<RichTextRun>` を再利用
   - miss → `resolve → html::parse → to_rich_text_runs` を実行、LRU へ insert

**Implementation Steps**:

1. **各 Provider に version カウンタを追加** - worker thread が cache を書き換えた直後にインクリメント
2. **LRU キャッシュ実装** - シンプルな HashMap + 挿入順キューで十分（外部 crate 不要）
3. **ベンチ計測** - 100k 反復で 1 ms を切るか dev build で確認（リリースビルドではさらに余裕）
4. **release build** - `cd native-poc && CARGO_TARGET_DIR=./target-host cargo build --release`
5. **manual smoke** - SPEC.md Success Criteria の 3 項目（時計 + cwd / OSC 777 set / mux 接続時 3 段表示）を `target-host/release/emterm-native-poc` で確認

**Dependencies**: Requires: Phase E

**Testing Approach**:

- Unit: LRU の hit / miss、version インクリメントが結果差し替えにつながること
- Integration: 全体スモーク（cargo test）
- E2E: なし（sdd.6 verify で smoke）
- Manual: 上記 release build smoke

**Acceptance Criteria**:

- [ ] テンプレート解決 100k iter / 4 セクションが 1 秒未満で完了
- [ ] release build が `native-poc/target-host/release/emterm-native-poc` に成果物を生成
- [ ] 起動 manual smoke で時計 + cwd 表示
- [ ] `printf '\033]777;statusbar;set;left;hi\033\\'` で OSC 行 left に "hi" が反映

**Estimated Effort**: small

---

## Complete File Structure

```
native-poc/
├── Cargo.toml                                  # 既存（新規依存なし）
├── src/
│   ├── main.rs / lib.rs                        # MOD: pub mod html; pub mod status_bar;
│   ├── settings.rs                             # MOD: StatusBarSettings 拡張 + CustomCommand
│   ├── callbacks.rs                            # MOD: NativeCallbacks に statusbar 振り分け
│   ├── app.rs                                  # MOD: StatusBarRuntime 所有 + status_bar_view_model
│   ├── tabs.rs                                 # MOD（必要なら）: mux disconnect 通知
│   ├── wakeup.rs                               # 既存（worker から poke を呼ぶ）
│   ├── ui/
│   │   └── status_bar.rs                       # MOD: 3 行レイアウト
│   ├── html/                                   # NEW: 共通 HTML パーサー
│   │   ├── mod.rs
│   │   ├── tokenizer.rs
│   │   ├── parser.rs
│   │   ├── sanitizer.rs
│   │   └── rich_text.rs
│   └── status_bar/                             # NEW: ステータスバーランタイム
│       ├── mod.rs
│       ├── runtime.rs
│       ├── view_model.rs
│       ├── template_engine.rs
│       ├── osc_dispatcher.rs
│       └── providers/
│           ├── mod.rs
│           ├── time.rs
│           ├── cwd.rs
│           ├── git_branch.rs
│           └── command.rs
└── target / target-host                        # ビルド成果物（git ignore 既存）
```

## Testing Strategy

- **Unit**: 各モジュール直下にテストモジュールを置く。html / template_engine / providers / osc_dispatcher / runtime をカバー。コア 80%+ 目標、特に html / template_engine / osc_dispatcher は 90%+ を狙う
- **Integration**: `NativeCallbacks::on_osc(100, ...)` の振り分け回帰、ViewModel 構築の end-to-end、Phase 4-D 既存テストを ViewModel 形に書き換え
- **E2E**: native-poc には E2E harness なし。WebView 版（legacy-tauri）の E2E はソース無改変なので回帰確認のみ。実行は sdd.6-verify で `./scripts/run-e2e-docker.sh test` 実施
- **Manual**: SPEC.md Success Criteria の smoke 3 件（時計 + cwd / OSC 777 set / mux 3 段）を release build で確認

TDD は phase 単位で unit test レベルのみ回す。E2E スイートは sdd.6-verify でまとめて。

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| egui | 既存（0.29 系） | 描画。新規追加なし |
| libc (Unix) | 既存 | localtime_r |
| (Windows) winapi or std | TBD | GetLocalTime。既に native-poc が引いていれば利用、無ければ chrono の代替で thin にラップする |
| (no new crate) | - | regex / tokio / html5ever は採用しない |

> **Windows 時刻 API**: native-poc が現状で chrono を transitively に持っているかは実装初期に確認する。持っていれば chrono を使い、なければ `windows-sys` も `winapi` も追加せずに済むかどうかも合わせて検討。新規 crate 追加判断は Phase C 着工時に決める（実装者注: ここを open question として扱う）。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| HTML パーサーがインライン subset 想定で Markdown ビューア要件を満たせない | 中 | 中 | `Node` enum を `#[non_exhaustive]` 相当に設計し、Block variant 追加で対応できる API surface に保つ。Phase A で `to_rich_text_runs` は `for node in nodes { match node { Text => .., LineBreak => .., Span => .., Bold => .., Italic => .., Underline => .., _ => skip } }` の形で書き、将来 variant 追加で fall-through する |
| GitBranchProvider の worker が大量の repo で git をスピンさせ続け、5s timeout が頻発する | 低 | 中 | `Condvar::wait_timeout` で interval を最低 5s に保ち、cwd 変化が無ければスキップ |
| OSC 777;statusbar 経由の content に `;` が含まれるとトークン分割が壊れる | 中 | 中 | `splitn(N, ';')` 戦略を採用し `set;left;<content>` は 3 要素 split で content の `;` を保護 |
| mux StatusUpdateMsg と OSC 777;statusbar の同時更新で OSC 行が高速チャタリング | 低 | 低 | view_model 構築時は「mux 優先、無ければ OSC 777」の決定論的順序にする。両方ある場合は mux が常勝 |
| worker thread が Drop で join せずに残る | 中 | 中 | `Arc<AtomicBool>` + `Condvar` で停止シグナル、Drop で `notify_all` + `join()` |
| Windows 時刻 API のために新規 crate が必要になる | 中 | 低 | Phase C 着工時に判定、必要なら最小依存（`windows-sys` の特定 feature のみ）に絞る |
| Phase 4-D 既存テスト書き換えで回帰 | 中 | 中 | 既存テストは「振る舞いベース」で再記述し、ViewModel が責任を持つ範囲だけアサート |
| TimeProvider タイマースレッドが Drop で join されず leak する | 中 | 中 | `Arc<AtomicBool>` 停止フラグ + `Condvar::notify_all()` + `join()` を必須化、TS-perf-3 でテスト |
| Provider スレッド数増加で起動コストが顕在化（TimeProvider + GitBranch + CommandProvider × N） | 低 | 低 | CwdProvider は polling なし、TimeProvider は default 1Hz、GitBranch は 0.2Hz、CommandProvider はユーザー設定。実測（VERIFICATION.md パフォーマンスセクション）で確認 |
| `wakeup.wake()` 経由でも redraw が走らない（過去の `request_repaint_after` と同じ罠を Wakeup 内部でも踏む可能性） | 低 | 高 | TS-31 を integration test として用意し、PTY 出力なしで `wake()` を呼んだら次フレームが走ることを確認 |
| winit `ApplicationHandler::user_event` の未実装で `EventLoopProxy::send_event` 経由の wake が握り潰される（実際に二度目の verify で発火した） | 中 | 高 | `PocApp::user_event` で必ず `request_redraw()` を呼ぶ。TS-32（mock window を使ったハンドラ単体テスト）+ TS-30（release-build idle-clock smoke）で恒久的に塞ぐ。今後 `impl ApplicationHandler` を改訂する際は user_event の挙動を明示的に保守する |
| winit `ApplicationHandler` の他メソッド網羅性が漏れて将来の winit 更新時に類似の盲点が再発する | 中 | 中 | SPEC.md Notes セクションに罠記録を残す。winit バージョン更新時は `ApplicationHandler` の trait 定義変更点をレビューし、新規追加メソッドの no-op default が wake/redraw 経路を壊さないか確認する |

## Open Questions

- [ ] Windows 時刻取得を `chrono` で行うか native API で行うか（Phase C 着工時に最終判断）
- [ ] GitBranchProvider / CommandProvider の `cwd_source` (`Arc<dyn Fn() -> Option<String> + Send + Sync>`) は active tab の cwd を返す必要があるが、active tab はフレーム毎に変わる。`App` 全体を Send で共有することはできないため、典型的な実装パターンは「ローカルテンプレ resolve は per-frame の main thread 内」（worker は触らない）か「`Arc<Mutex<Option<String>>>` の "current active cwd" を App が毎フレーム更新し worker は読むだけ」のいずれか。Phase C 実装時に確定する
- [x] `wakeup::wake()` は worker thread からの呼び出しを既存設計が許容する（`native-poc/src/wakeup.rs`: `EventLoopProxy::send_event` をラップした `Box<dyn Fn() + Send + Sync>` を `OnceLock` 経由で公開。PTY reader からも既に呼ばれている）
- [x] `egui::Context::request_repaint_after` は winit に届かないため、周期 redraw は各 Provider の `wakeup.wake()` 経由で発火させる方式に統一する（初回 verify で時計が止まる事象を根拠に確定）
- [x] winit 0.30 `ApplicationHandler::user_event` の no-op default で `EventLoopProxy::send_event(())` が握り潰される問題は、`PocApp::user_event` を実装し `host.window().request_redraw()` を呼ぶことで解決（2 度目の release-build verify で時計が再度止まった事象を根拠に確定）
- [ ] `StatusBarViewModel.mux_session_name` の表示位置（App Line 1 prefix か、独立 badge か）は実装時に UI 設計に従う。SPEC では両オプション保持

## Success Metrics

- [ ] FR1–FR12 / NFR1–NFR5 すべてに対応するテストが存在し pass
- [ ] `cargo test --bin emterm-native-poc` 全 pass
- [ ] `cargo fmt --check` warning なし
- [ ] release build (`CARGO_TARGET_DIR=./target-host cargo build --release`) 成功
- [ ] `native-poc/Cargo.toml` の `[dependencies]` 差分なし（新規 crate ゼロ）
- [ ] manual smoke 3 件 OK
- [ ] legacy-tauri E2E スイート（`./scripts/run-e2e-docker.sh test`）回帰なし
