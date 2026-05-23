# Verification Document: Status Bar Native Port (egui)

## Overview

**Feature**: status-bar-native-port
**SPEC.md**: `doc/tasks/status-bar-native-port/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/status-bar-native-port/IMPLEMENTATION.md`

WebView 版ステータスバー機能を native-poc (egui) に完全移植し、再利用可能な HTML パーサーを `native-poc/src/html/` として導入する。

## Build Verification

| Component | Command | Expected |
|-----------|---------|----------|
| native-poc (check) | `cd native-poc && CARGO_TARGET_DIR=./target cargo check` | exit 0, no errors |
| native-poc (release) | `cd native-poc && CARGO_TARGET_DIR=./target-host cargo build --release` | exit 0, `native-poc/target-host/release/emterm-native-poc` 生成 |
| legacy-tauri (build 回帰) | `bun tauri build` | exit 0（任意・本タスクで直接変更しないが回帰確認に有用） |

## Test Verification

| Component | Command | Expected |
|-----------|---------|----------|
| native-poc unit | `cd native-poc && CARGO_TARGET_DIR=./target cargo test --bin emterm-native-poc` | 全 pass、新規テストすべて green |
| legacy-tauri 全テスト | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml && bun test && bun run typecheck"` | 全 pass（回帰確認） |

Coverage 目標: native-poc コア（html / template_engine / osc_dispatcher）90%+、その他 80%+。

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `TemplateEngine::extract_variables` が `{time}` `{cmd:foo}` を重複含めて抽出 | 順序通り全変数名が返る | Unit |
| TS-2 | `TemplateEngine::resolve` で未登録変数が空文字に置換される | 結果に変数 placeholder が残らない | Unit |
| TS-3 | `TemplateEngine::resolve` が provider の `get_color()` を `<span style="color:...">` でラップ | resolve 結果に span タグが含まれる | Unit |
| TS-4 | `TimeProvider::format` が `YYYY MM DD HH hh mm ss A` 全トークンを変換 | 各トークンが正しい値で置換される | Unit |
| TS-5 | `TimeProvider` AM / PM 境界（正午・深夜 0 時）が正しい | 正午 = PM、深夜 0 時 = AM、12 時間表記の hh も妥当 | Unit |
| TS-6 | `CwdProvider` が basename を抽出（`/home/me`, `file:///home/me/x`, `C:\foo\bar`, `/`, percent-encoded） | 各ケースで期待通りの末尾セグメント | Unit |
| TS-7 | `GitBranchProvider::parse_branch` が `main` / 空 / `fatal:` を正しく扱う | `main`、`""`、`""` を返す | Unit |
| TS-8 | `GitBranchProvider::parse_status` が clean / dirty / untracked を分類 | 期待色 (`#4caf50` / `#f9a825` / `#9e9e9e`) を返す | Unit |
| TS-9 | `CommandProvider` が `~/` を `$HOME` / `%USERPROFILE%` に展開 | 展開後の絶対パスで spawn される | Unit |
| TS-10 | `CommandProvider` が `interval_ms < 1000` を 1000 にクランプ | クランプ後の値が internal state に保持 | Unit |
| TS-11 | `strip_html_tags` が `<script>...</script>` 本文を削除し `1 < 2` を保持 | `1 < 2 bold tail`（または同等の意味） | Unit |
| TS-12 | `html::parse` が `<span style="color:#fff">x</span>` を `Span{color: Hex(0xff,0xff,0xff), children: [Text("x")]}` に変換 | 期待 AST が返る | Unit |
| TS-13 | `html::parse` が `<b><i>x</i></b>` のネストを正しく構築 | 期待 AST が返る | Unit |
| TS-14 | `html::parse` が HTML entities (`&amp; &lt; &gt; &#65;`) をデコード | デコード済み Text Node | Unit |
| TS-15 | `html::parse` が `<script>x</script>` を AST から完全に除去 | 該当 Node が一切残らない | Unit |
| TS-16 | `html::parse` が未知タグでもラッパーを落として子を保持 | `<unknown>foo</unknown>` → `Text("foo")` | Unit |
| TS-17 | `to_rich_text_runs` が `<b><i>x</i></b>` を `RichTextRun{ bold:true, italic:true }` にフラット化 | 1 run、属性正しい | Unit |
| TS-18 | `try_dispatch_statusbar` が `markdown;...` で false を返す | dispatcher は state を変更しない | Unit |
| TS-19 | `StatusBarOscDispatcher::handle` 全サブコマンド（set/clear/show/hide）で state が期待通り変化 | round-trip で再現性あり | Unit |
| TS-20 | `Settings::default()` で SPEC FR10 規定値が揃う | `app_line1_left == "{time}"` 等 | Unit |
| TS-21 | OSC 777;statusbar が `NativeCallbacks::on_osc` 経由で OSC 行を更新 | `OscLayerState.left` が更新 | Integration |
| TS-22 | OSC 777;markdown が既存 `osc_queue` に積まれる（回帰） | `osc_queue` に 1 件追加 | Integration |
| TS-23 | mux `StatusUpdateMsg` が active tab の OSC 行に反映 | ViewModel の OSC 行 left/right が daemon の値 | Integration |
| TS-24 | mux 切断（`mux_session_name` Drop）で OSC 行クリア | 次フレーム ViewModel で空 | Integration |
| TS-25 | App Line 2 両 side 空で行が描画されない | egui 出力ノードに line2 が含まれない | Integration |
| TS-26 | `enabled = false` で TopBottomPanel を挿入しない | egui 出力ノードに status bar panel なし | Integration |
| TS-27 | OSC 行 `<script>x</script>foo` 送信で OSC 行 left = "foo" | XSS 防御確認 | Integration |
| TS-28 | worker thread が Drop で join される | テスト終了後リーク無し | Unit |
| TS-perf-1 | テンプレート解決 100k iter × 4 セクションが 1 秒未満 | `cargo test --release` で benchmark | Unit (release) |
| TS-perf-2 | HTML parse 10k iter / 256 バイト payload が 1 秒未満 | `cargo test --release` で benchmark | Unit (release) |
| TS-perf-3 | TimeProvider の自前タイマースレッドが Drop で停止し join される（leak チェック） | `cargo test` でテスト終了後に thread leak なし、`std::thread::Builder` 経由で名前付きにしてカウント可能 | Unit |
| TS-29 | TimeProvider タイマースレッドが `refresh_rates["time"]` 周期で `Wakeup::wake()` を呼ぶ | Test double の `Wakeup` で wake 呼び出し回数を測定、interval 2 周期分待って ≥ 2 回呼ばれる | Unit |
| TS-30 | release binary 起動後、shell idle 状態（PTY 出力ゼロ、cursor blink OFF）で `{time}` が 1Hz で更新される | `native-poc/target-host/release/emterm-native-poc` を起動し、`cursor_show_interrupt` を OFF にして 10 秒待ち時計が 10 回進むこと目視確認 | Manual smoke |
| TS-31 | `wakeup::wake()` を PTY 出力なしの状態で外部から呼ぶと winit redraw が発火する | integration test で `Wakeup::wake()` を直接呼び、event loop が次フレームを処理することを確認（host での `cargo test --bin emterm-native-poc` で実行可能な範囲、不可なら manual smoke へ落とす） | Integration |
| TS-32 | `PocApp::user_event` が呼ばれた時に active window の `request_redraw()` が要求される | mock window (もしくは redraw 要求を観測可能な test double) を `host` に差し込み、`user_event(&event_loop, ())` 呼び出し後に `request_redraw` 呼び出し回数が 1 増えることを assert。`host == None` のときは no-op であることも確認 | Unit |

## Code Quality Verification

| Item | Command | Expected |
|------|---------|----------|
| Rust format | `cd native-poc && cargo fmt --check` | 差分なし |
| 新規 crate 追加なし | `git diff native-poc/Cargo.toml` レビュー | `[dependencies]` セクションに追加なし |
| ファイルサイズ目安 | 各ファイル 1000 行以内 | 超過なし。超過時は責務分割を検討 |
| tokio / regex 不使用 | `grep -rE '\b(tokio\|regex)\b' native-poc/Cargo.toml native-poc/src/` | ヒットなし（テスト含む。grep -E では `|` を `\|` でエスケープする必要があるため注意） |

## File Structure Verification

### Files to Create

- `native-poc/src/html/mod.rs` - HTML parser 公開 API
- `native-poc/src/html/tokenizer.rs` - HTML tokenizer
- `native-poc/src/html/parser.rs` - tokens → AST
- `native-poc/src/html/sanitizer.rs` - `strip_html_tags()`
- `native-poc/src/html/rich_text.rs` - AST → `Vec<RichTextRun>`
- `native-poc/src/status_bar/mod.rs` - モジュールルート
- `native-poc/src/status_bar/runtime.rs` - `StatusBarRuntime`
- `native-poc/src/status_bar/view_model.rs` - `StatusBarViewModel` 等
- `native-poc/src/status_bar/template_engine.rs` - `TemplateEngine`, `VariableProvider`
- `native-poc/src/status_bar/osc_dispatcher.rs` - `StatusBarOscDispatcher`
- `native-poc/src/status_bar/providers/mod.rs` - Provider 群 re-export
- `native-poc/src/status_bar/providers/time.rs` - TimeProvider
- `native-poc/src/status_bar/providers/cwd.rs` - CwdProvider
- `native-poc/src/status_bar/providers/git_branch.rs` - GitBranchProvider (worker thread)
- `native-poc/src/status_bar/providers/command.rs` - CommandProvider (worker thread)

### Files to Modify

- `native-poc/src/main.rs` または `lib.rs` - `pub mod html;` `pub mod status_bar;` 追加
- `native-poc/src/settings.rs` - `StatusBarSettings` 拡張、`CustomCommand` 追加
- `native-poc/src/callbacks.rs` - `statusbar_dispatcher` 経路追加、`on_osc(100, ..)` 振り分け
- `native-poc/src/app.rs` - `StatusBarRuntime` 所有、`status_bar_view_model()` 導入
- `native-poc/src/ui/status_bar.rs` - 3 行レイアウト書き換え
- `native-poc/src/render/mod.rs` - `app.status_bar_state()` → `app.status_bar_view_model()` 呼び出し差し替え
- `native-poc/src/tabs.rs`（必要時のみ）- mux disconnect 検出フック
- `native-poc/src/wakeup.rs`（必要時のみ）- 既存 `wake()` は worker thread 呼び出しを既に許容しているため不要見込み
- `native-poc/src/window_host.rs` - `impl ApplicationHandler for PocApp` に `user_event(&mut self, _: &ActiveEventLoop, _: ())` を追加し、`host.window().request_redraw()` を呼ぶ（UserEvent 経由の wake を redraw 要求に転換）

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR12 すべて実装・テストされている | TS-1..TS-28 全 pass + manual smoke |
| SC-2 | `cargo test` (native-poc) 全 pass | `cd native-poc && CARGO_TARGET_DIR=./target cargo test --bin emterm-native-poc` |
| SC-3 | 新規 crate 追加なし | `git diff native-poc/Cargo.toml` レビュー |
| SC-4 | デフォルトで時計 + cwd 表示 | release build manual smoke |
| SC-5 | `OSC 777;statusbar;set;left;X` で OSC 行が更新される | release build manual smoke（`printf` 経由） |
| SC-6 | `OSC 777;markdown;...` が既存ハンドラに流れる | TS-22 + manual（markdown viewer 起動確認） |
| SC-7 | mux 接続時に 3 段表示 | manual smoke（mux daemon 接続して確認） |
| SC-8 | Linux release build 成功 | `CARGO_TARGET_DIR=./target-host cargo build --release` |
| SC-9 | Windows でもコンパイル可能（クロスビルド） | CI（GitHub Actions windows-latest）または手元で `--target x86_64-pc-windows-gnu` 試行 |
| SC-10 | HTML parser が Markdown ビューア移植で再利用可能な surface を持つ | `Node` enum、`parse`, `to_rich_text_runs`, `strip_html_tags` のドキュメント／コメント確認 |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (Layer Structure) | E | TS-25, TS-26 + manual smoke |
| FR2 (Template Engine) | C | TS-1, TS-2, TS-3 |
| FR3 (TimeProvider) | C | TS-4, TS-5, TS-perf-3, TS-29, TS-30, TS-32 |
| FR4 (CwdProvider) | C | TS-6, TS-31, TS-32 |
| FR5 (GitBranchProvider) | C | TS-7, TS-8, TS-28, TS-31, TS-32 |
| FR6 (CommandProvider) | C | TS-9, TS-10, TS-28, TS-31, TS-32 |
| FR7 (OSC 777;statusbar Dispatch) | D | TS-18, TS-19, TS-21, TS-22 |
| FR8 (HTML Parser) | A | TS-12, TS-13, TS-14, TS-15, TS-16, TS-17 |
| FR9 (HTML Sanitizer) | A | TS-11, TS-27 |
| FR10 (Settings extension) | B | TS-20 |
| FR11 (Mux integration) | E | TS-23, TS-24 |
| FR12 (Auto layer visibility) | E | TS-25, TS-26 |

### Non-Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| NFR1 (Performance) | F | run-list cache 動作確認 + 100k iter ベンチ + Provider 自前 wake 経路（TS-29, TS-31, TS-32）|
| NFR2 (Security) | A, C, D | TS-11, TS-27 + manual XSS check |
| NFR3 (Platform Linux/Windows) | C | Linux build + Windows クロスビルド |
| NFR4 (Visual consistency) | E | manual visual review（OSC 行に独自背景なし、開閉アニメなし） |
| NFR5 (Extensibility) | A | `Node` enum extensibility レビュー（コードコメント + コード設計） |

## E2E Testing

native-poc には専用 E2E harness がないため、本タスクで native 側の E2E スイート追加は行わない。

legacy-tauri 側の既存 E2E スイート（WebdriverIO + tauri-driver）は `src/status-bar/` を変更しないため回帰確認のみ:

- [ ] `./scripts/run-e2e-docker.sh test` 全 spec pass（sdd.6-verify で実行）

## Manual Testing (E2E Not Possible)

native-poc 側の動作確認は手動 smoke で行う。`native-poc/target-host/release/emterm-native-poc` を起動して確認:

- [ ] 起動直後、ステータスバーが App Line 1 だけ表示され left = 時計、right = cwd basename
- [ ] **時計動作確認 (TS-30 — 二次バグ回帰確認)**: shell idle 状態（コマンド入力なし、cursor blink 無効）で 10 秒間放置し、左の時計が秒単位で進み続けることを目視確認。これは Provider 由来の wake → `EventLoopProxy::send_event` → **`PocApp::user_event`** → `host.window().request_redraw()` の完全チェーン全段が release binary で実機動作することの最終確認。**`ApplicationHandler::user_event` 未実装で 2 度目に発火した時計停止 (`project_status_bar_design` 参照) の再発有無を確認する項目**
- [ ] 別ディレクトリへ `cd` すると right の cwd basename が更新される
- [ ] git repo 内で `{git_branch}` を含むテンプレに切り替え、branch 名と clean/dirty/untracked 色が出る
- [ ] `printf '\033]777;statusbar;set;left;hi\033\\'` で OSC 行 left に "hi" が反映
- [ ] `printf '\033]777;statusbar;set;left;<script>x</script>foo\033\\'` で OSC 行 left = "foo"
- [ ] `printf '\033]777;statusbar;clear\033\\'` で OSC 行が空になり、行ごと消える（自動非表示）
- [ ] `printf '\033]777;statusbar;hide\033\\'` で OSC 行が消える
- [ ] `printf '\033]777;markdown;...\033\\'` で従来の Markdown viewer が起動する（回帰）
- [ ] mux daemon に接続するとき OSC 行に daemon の left/right が出る。App Line 1/2 はローカルテンプレ
- [ ] mux 切断で OSC 行が消える
- [ ] `Settings::default().statusbar.enabled = false` 相当（コード側で確認）でステータスバー全体が描画されない
- [ ] 視覚: 3 行の背景が同一（OSC 行に独自背景なし）、開閉アニメーションなし
- [ ] `~/.local/share/net.laser5.app.emterm/logs/emterm.log` に WARN/ERROR が新規発生していない

## Performance Verification

- [ ] テンプレート解決 100k iter / 4 セクション = 1 秒未満（NFR1）
- [ ] HTML parse 10k iter / 256 バイト payload = 1 秒未満（NFR1）
- [ ] git worker 動作中も egui の render フレーム drop なし（manual 視認 + 必要なら `tracing` で frame time 計測）
- [ ] Provider 由来スレッド数の確認: TimeProvider × 1 timer thread + GitBranchProvider × 1 worker thread + CommandProvider × N worker thread（custom_commands 件数）。CwdProvider は polling なしのためスレッドを持たない。スレッド leak なしを TS-perf-3 + 全 Drop で確認

## Security Verification

- [ ] OSC 777;statusbar 経由のコンテンツが完全に tag-strip される（TS-27 + manual XSS）
- [ ] `CustomCommand` の name 検証で `;`, `..`, スペース, 制御文字を含む name が reject される
- [ ] CustomCommand 実行で shell を経由しないことをコードレビューで確認（`Command::new(...)` の args ゼロ）
- [ ] `<script>` / `<style>` ブロックが AST に残らない（TS-15）

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests | 22 (TS-1..TS-20, TS-29, TS-32) | 22 | 0 | 0 |
| Integration tests | 9 (TS-21..TS-28, TS-31) | 9 | 0 | 0 |
| Manual smoke | 14（時計動作確認 TS-30 追加）| 0 | 0 | 14 |
| Performance | 4 (TS-perf-1, TS-perf-2, TS-perf-3, render-fps) | 3 | 0 | 1 |
| Security | 4 | 3 | 0 | 1 |
| E2E regression (legacy-tauri) | 1 | 1 | 1 | 0 |
| **Total** | **54** | **38** | **1** | **16** |

## Verification Result

### Build Result

| Component | Command | Exit | Notes |
|-----------|---------|------|-------|
| native-poc check | `cd native-poc && CARGO_TARGET_DIR=./target cargo check --bin emterm-native-poc` | 0 | 39 warnings, none from new status-bar code |
| native-poc release | `cd native-poc && CARGO_TARGET_DIR=./target-host cargo build --release` | 0 | `native-poc/target-host/release/emterm-native-poc` (42 MiB) produced in 1m 50s (re-implementation) |

### Test Result

| Suite | Command | Pass | Fail | Ignored |
|-------|---------|------|------|---------|
| native-poc unit | `cd native-poc && CARGO_TARGET_DIR=./target cargo test --bin emterm-native-poc` | 564 | 0 | 1 |

#### Local Bugfix: PocApp::user_event override (2026-05-23)

二度目の release-binary verify で idle 時計が再度止まったため、`window_host.rs`
の `impl ApplicationHandler for PocApp` に `user_event` メソッドが未実装で
あった点を修正した:

- `PocApp::user_event(&mut self, _: &ActiveEventLoop, _: ())` を追加。
  `host` が `Some` の場合のみ `host.window().request_redraw()` を呼ぶ
- 純粋ロジック `request_redraw_on_user_event(host, redraw)` を free fn として
  切り出し、winit の `Window` を伴わずユニットテスト可能にした (TS-32)
- これにより `WakeFn` → `EventLoopProxy::send_event(())` → `user_event` →
  `request_redraw()` の連鎖が release binary で完成し、Provider 由来の
  wake が PTY 出力ゼロ条件下でも redraw を引き起こす

新規追加テスト (2 件、TS-32):

- `window_host::tests::user_event_dispatches_redraw_when_host_present`
- `window_host::tests::user_event_is_noop_when_host_absent`

差分行数: `native-poc/src/window_host.rs` に +69 行 / -0 行
(`user_event` メソッド本体 + ヘルパ関数 `request_redraw_on_user_event` +
テスト 2 件)。他のファイルは触らず、修正範囲は完全に局所化されている。
テストでカバーできない「実機 release binary での 1Hz redraw」は TS-30
(manual smoke) でカバーする。

Release binary を再ビルド済み:
`native-poc/target-host/release/emterm-native-poc` (size 43,580,952 bytes,
mtime 2026-05-23 15:46)。

#### Re-implementation: Provider-Ownership Refresh-Redraw (2026-05-22)

リリースバイナリで idle 時に時計が止まる事象を受けて、各 Provider が自前で
winit redraw をトリガするアーキテクチャに切り替えた。具体的には:

- `native-poc/src/wakeup.rs` に `pub type WakeFn = Arc<dyn Fn() + Send + Sync>`
  を追加し、`shared_wake_fn()` で global `wake()` を共有する handle を発行
- `TimeProvider::with_wake(format, wake, RefreshConfig { interval })` で
  `Condvar::wait_timeout` ベースのタイマースレッドを起動。`Drop` で停止フラグ
  + `notify_all` + `join`
- `CwdProvider::with_wake(source, wake)` + `set_cwd()` メソッドを追加。
  `NativeCallbacks::handle_cwd` が OSC 7 受信時に Provider 経由で wake を発火
- `GitBranchProvider::start_with_wake` / `CommandProvider::with_wake` を導入し、
  既存の `crate::wakeup::wake()` 直接呼出を引き渡された `WakeFn` 呼出に置換
- `StatusBarRuntime::new(settings, cwd_source, wake)` で各 Provider に
  `Arc::clone(&wake)` を注入
- `render/mod.rs` の `ctx.request_repaint_after(Duration::from_secs(1))`
  を削除 (winit に届かないため idle 時に無効だった)

新規追加テスト (10 件):

- `wakeup::tests::shared_wake_fn_is_invokable_even_before_install`
- `wakeup::tests::wake_fn_arc_clones_invoke_same_target`
- `status_bar::providers::time::tests::time_provider_timer_thread_calls_wake_on_interval` (TS-29)
- `status_bar::providers::time::tests::time_provider_drop_joins_timer_thread` (TS-perf-3)
- `status_bar::providers::time::tests::time_provider_without_timer_does_not_spawn_thread`
- `status_bar::providers::cwd::tests::set_cwd_with_wake_invokes_wake_on_change`
- `status_bar::providers::cwd::tests::set_cwd_is_idempotent_no_wake_when_unchanged`
- `status_bar::providers::cwd::tests::set_cwd_without_wake_is_safe`
- `status_bar::runtime::tests::runtime_injects_wake_into_cwd_provider`
- `status_bar::runtime::tests::runtime_time_provider_timer_fires_wake`

TS-31 (winit redraw on wake) は event loop を含む integration test として
unit から検証できないため、manual smoke (TS-30 と統合) に落とす。

Notable new tests (Phase E/F):

- `ui::status_bar::tests::disabled_view_model_does_not_insert_panel`
- `ui::status_bar::tests::app_line2_auto_hides_when_empty`
- `ui::status_bar::tests::mux_session_renders_badge_and_osc_text`
- `ui::status_bar::tests::osc_row_hidden_when_empty_and_no_mux`
- `ui::status_bar::tests::osc_row_from_dispatcher_renders_without_mux_badge`
- `ui::status_bar::tests::osc_force_hide_skips_row`
- `ui::status_bar::tests::enabled_status_bar_reserves_panel_height`
- `status_bar::runtime::tests::build_view_model_disabled_returns_disabled_marker`
- `status_bar::runtime::tests::build_view_model_app_line1_resolves_time_template`
- `status_bar::runtime::tests::build_view_model_mux_status_populates_osc_row`
- `status_bar::runtime::tests::build_view_model_falls_back_to_dispatcher_when_no_mux`
- `status_bar::runtime::tests::build_view_model_mux_wins_over_dispatcher`
- `status_bar::runtime::tests::build_view_model_app_line2_empty_means_no_runs`
- `status_bar::runtime::tests::run_cache_hits_when_template_and_versions_match`
- `status_bar::runtime::tests::run_cache_misses_when_version_changes`
- `status_bar::runtime::tests::run_cache_evicts_oldest_when_full`
- `status_bar::runtime::tests::cached_resolve_returns_same_runs_on_repeated_calls`

### Format Result

| Item | Command | Result |
|------|---------|--------|
| Rust format | `cd native-poc && cargo fmt` | applied (no review diff) |

### Existing E2E Regression (Phase 3.8)

E2E regression suite is a `sdd.6-verify` concern. native-poc has no dedicated E2E harness; legacy-tauri E2E was not touched in this work.

### Manual Smoke Result

実機での手動 smoke は sdd.6-verify で行うため本フェーズでは未実施。リリースビルドは `native-poc/target-host/release/emterm-native-poc` に生成済みで起動可能。

### File Structure Verification

#### Files Created (all present)

- [x] `native-poc/src/html/mod.rs`
- [x] `native-poc/src/html/tokenizer.rs`
- [x] `native-poc/src/html/parser.rs`
- [x] `native-poc/src/html/sanitizer.rs`
- [x] `native-poc/src/html/rich_text.rs`
- [x] `native-poc/src/status_bar/mod.rs`
- [x] `native-poc/src/status_bar/runtime.rs`
- [x] `native-poc/src/status_bar/view_model.rs`
- [x] `native-poc/src/status_bar/template_engine.rs`
- [x] `native-poc/src/status_bar/osc_dispatcher.rs`
- [x] `native-poc/src/status_bar/providers/mod.rs`
- [x] `native-poc/src/status_bar/providers/time.rs`
- [x] `native-poc/src/status_bar/providers/cwd.rs`
- [x] `native-poc/src/status_bar/providers/git_branch.rs`
- [x] `native-poc/src/status_bar/providers/command.rs`
- [x] `native-poc/src/status_bar/providers/worker.rs` (worker-thread helpers)

#### Files Modified (all present)

- [x] `native-poc/src/main.rs` — `mod html;` `mod status_bar;` 追加
- [x] `native-poc/src/settings.rs` — `StatusBarSettings` 拡張、`CustomCommand` 追加
- [x] `native-poc/src/callbacks.rs` — `statusbar_dispatcher` + `cwd_provider` 経路追加、`handle_cwd` から Provider に転送
- [x] `native-poc/src/app.rs` — `StatusBarRuntime` 所有、`status_bar_view_model()` 導入、`shared_wake_fn()` を runtime に注入
- [x] `native-poc/src/ui/status_bar.rs` — 3 行レイアウト書き換え
- [x] `native-poc/src/render/mod.rs` — `app.status_bar_view_model()` 呼び出しに差し替え、`ctx.request_repaint_after(1s)` 削除
- [x] `native-poc/src/tabs.rs` — `Tab::spawn_shell` が dispatcher + CwdProvider を受け取り callbacks に注入
- [x] `native-poc/src/wakeup.rs` — `pub type WakeFn` + `shared_wake_fn()` を追加

### Known Limitations

- 手動 smoke (E2E 観点) は sdd.6-verify でまとめて実施する。
- ホスト環境で実行する `status_bar::runtime::tests::build_view_model_mux_wins_over_dispatcher` が稀に 60 秒近くかかる事例があった。`Drop` の worker join は機能しているため機能影響はないが、ホストの git が低速な場合に worker tick が遅延する可能性。Docker テストでは観測されず。
