# 実装自動検証レポート (最終版 / user_event 修正後)

**検証日時**: 2026-05-23
**対象機能**: Status Bar Native Port (Wakeup-based + `ApplicationHandler::user_event` handler)
**VERIFICATION.md**: `doc/tasks/status-bar-native-port/VERIFICATION.md`
**SPEC.md**: `doc/tasks/status-bar-native-port/SPEC.md`
**プロジェクト**: eMterm (native-poc)
**対象コミット**: `b1056917bd9ec0191d9c833fa8316901b53a99aa` (+ uncommitted `user_event` patch on `native-poc/src/window_host.rs`)

---

## 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド (`cargo check`) | OK | exit 0、status-bar 範囲新規エラー/警告なし (sdd.5-check 結果を採用) |
| テスト (`cargo test`) | OK | 564 passed / 0 failed / 1 ignored (TS-32 unit test 2件含む) |
| コードフォーマット (`cargo fmt --check`) | OK | 差分なし |
| 静的解析 | OK | `user_event` / `request_redraw_on_user_event` の参照は完全配線。status-bar 範囲に新規警告なし |
| ファイル構造 | OK | files_create 16件 + files_modify 8件すべて存在 |
| SPEC.md 適合性 | OK | FR1–FR12 / NFR1–NFR5 すべて実装、TS-30 のみ実機 manual smoke 待ち |
| セキュリティ (NFR2) | OK | OSC 入力 tag-strip、コマンド名 validation、`<script>`/`<style>` 完全除去 |
| Wake → Redraw 配線 | OK | Provider → `WakeFn` → `EventLoopProxy::send_event(())` → `PocApp::user_event` → `host.window().request_redraw()` 全段配線 |
| `user_event` 配線 (TS-32) | OK | unit test 2件 pass、`host=Some` で 1 回 redraw、`host=None` で no-op |
| 新規 crate 追加なし (`tokio` / `regex` 不使用) | OK | grep でヒットなし (comment 1 件のみ — `template_engine.rs` のドキュメンテーション) |

**総合評価**: 自動検証はすべて pass。残るは 実機 release binary での TS-30 manual smoke (10秒 idle 時計動作) のみ。

---

## ファイル構造検証

### Files Created (16 件すべて存在)

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

### Files Modified (8 件すべて存在)

- [x] `native-poc/src/main.rs` — `mod html;` `mod status_bar;` 追加
- [x] `native-poc/src/settings.rs` — `StatusBarSettings` 拡張、`CustomCommand` 追加
- [x] `native-poc/src/callbacks.rs` — `statusbar_dispatcher` + `cwd_provider` 経路追加、`handle_cwd` から Provider に転送
- [x] `native-poc/src/app.rs` — `StatusBarRuntime` 所有、`status_bar_view_model()` 導入、`shared_wake_fn()` を runtime に注入
- [x] `native-poc/src/ui/status_bar.rs` — 3 行レイアウト書き換え
- [x] `native-poc/src/render/mod.rs` — `app.status_bar_view_model()` 呼び出しに差し替え、`ctx.request_repaint_after(1s)` 削除
- [x] `native-poc/src/tabs.rs` — `Tab::spawn_shell` が dispatcher + CwdProvider を受け取り callbacks に注入
- [x] `native-poc/src/wakeup.rs` — `pub type WakeFn` + `shared_wake_fn()` を追加

### Additional Modification (本フェーズで追加)

- [x] `native-poc/src/window_host.rs` — `impl ApplicationHandler for PocApp` に `user_event(&mut self, _: &ActiveEventLoop, _: ())` を追加。
  - `host.as_ref()` が `Some` のとき `host.window().request_redraw()` を呼ぶ
  - 純粋ロジックを `request_redraw_on_user_event(host, redraw)` free fn に切り出してユニットテスト可能に
  - 新規テスト 2 件: `user_event_dispatches_redraw_when_host_present` / `user_event_is_noop_when_host_absent` (TS-32)
  - 差分行数: +69 行 / -0 行（局所修正）
  - 本ファイルは 1684 行で UI Design Guidelines の "1000 行目安" を超えているが、本タスクで追加したのは末尾の `user_event` 関連 (+69 行) のみで責務拡張は伴わない。リファクタリングは別タスクで扱う

---

## ビルド検証

| Component | Command | Exit | Notes |
|-----------|---------|------|-------|
| native-poc check | `cd native-poc && CARGO_TARGET_DIR=./target cargo check --bin emterm-native-poc` | 0 | 39 warnings、status-bar 範囲新規警告なし |
| native-poc release | `cd native-poc && CARGO_TARGET_DIR=./target-host cargo build --release` | 0 | `native-poc/target-host/release/emterm-native-poc` (43,580,952 bytes, mtime 2026-05-23 15:46) 生成済み |

---

## テスト実行結果

| Suite | Command | Pass | Fail | Ignored |
|-------|---------|------|------|---------|
| native-poc unit | `cd native-poc && CARGO_TARGET_DIR=./target cargo test --bin emterm-native-poc` | **564** | **0** | 1 |

### TS-32 (本フェーズ追加分) 新規 2 件 pass 確認

- `window_host::tests::user_event_dispatches_redraw_when_host_present` — `host=Some` の場合 `request_redraw` callback が **1 回**呼ばれる
- `window_host::tests::user_event_is_noop_when_host_absent` — `host=None` の場合 callback は **0 回**で panic なし

### 既存の主要テスト (Provider オーナーシップ式 wake 経路 / v2)

- `wakeup::tests::shared_wake_fn_is_invokable_even_before_install`
- `wakeup::tests::wake_fn_arc_clones_invoke_same_target`
- `status_bar::providers::time::tests::time_provider_timer_thread_calls_wake_on_interval` (TS-29)
- `status_bar::providers::time::tests::time_provider_drop_joins_timer_thread` (TS-perf-3)
- `status_bar::providers::cwd::tests::set_cwd_with_wake_invokes_wake_on_change`
- `status_bar::runtime::tests::runtime_time_provider_timer_fires_wake`
- 他 view-model / OSC dispatcher / HTML parser / template engine 系 全 pass

---

## SPEC.md 適合性検証 (最終)

### Functional Requirements (FR1–FR12)

| FR | Title | Phase | Status | Tests |
|----|-------|-------|--------|-------|
| FR1 | Layer Structure (3 rows, shared bg, no anim) | E | OK | TS-25, TS-26 (+ manual smoke) |
| FR2 | Template Engine | C | OK | TS-1, TS-2, TS-3 |
| FR3 | TimeProvider (own timer thread, wake on tick) | C | OK | TS-4, TS-5, TS-perf-3, TS-29, **TS-32** (TS-30 は manual) |
| FR4 | CwdProvider (event-driven wake, no thread) | C | OK | TS-6, **TS-32** |
| FR5 | GitBranchProvider (worker thread + wake) | C | OK | TS-7, TS-8, TS-28, **TS-32** |
| FR6 | CommandProvider (worker thread + wake) | C | OK | TS-9, TS-10, TS-28, **TS-32** |
| FR7 | OSC 777;statusbar Dispatcher | D | OK | TS-18, TS-19, TS-21, TS-22 |
| FR8 | HTML Parser (inline subset, extensible) | A | OK | TS-12..TS-17 |
| FR9 | HTML Sanitizer (OSC route) | A | OK | TS-11, TS-27 |
| FR10 | Settings extension | B | OK | TS-20 |
| FR11 | Mux integration (3-layer coexistence) | E | OK | TS-23, TS-24 |
| FR12 | Auto layer visibility | E | OK | TS-25, TS-26 |

### Non-Functional Requirements (NFR1–NFR5)

| NFR | Title | Status | 備考 |
|-----|-------|--------|------|
| NFR1 | Performance + provider-owned wake + `ApplicationHandler::user_event` | **OK** | provider オーナーシップ式 wake チェーン全段配線完了。**SPEC.md "Notes" で必須化された `user_event` override も実装済み (TS-32 にて単体検証)**。`request_repaint_after` は `render/mod.rs` から削除済み |
| NFR2 | Security (OSC tag-strip, command name validation, no shell) | OK | TS-11, TS-15, TS-27 |
| NFR3 | Platform Linux + Windows | OK | Linux release build OK、Windows パスは `#[cfg(unix/windows)]` ゲート済み |
| NFR4 | Visual consistency (shared bg, no animation) | OK | `ui/status_bar.rs` で 3 行同一 panel background、開閉アニメなし |
| NFR5 | Extensibility (HTML Node enum) | OK | `Node` enum + future `Block`/`Link`/`Image` 変種コメント記述あり |

### SPEC.md NFR1 "Notes" 必須項目 (二次バグ根本原因) - 達成確認

SPEC.md L932-L965 の `Notes` セクションで明文化された:

> **winit 0.30: `EventLoopProxy::send_event` requires `ApplicationHandler::user_event`**
> native-poc therefore MUST implement `ApplicationHandler::user_event` on `PocApp` and, when a window host exists, call `host.window().request_redraw()` so the next frame is scheduled.

これに対する実装が `native-poc/src/window_host.rs:1463-1467` に存在:

```rust
fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
    request_redraw_on_user_event(self.host.as_ref(), |host| {
        host.window().request_redraw();
    });
}
```

TS-32 unit test 2 件で host=Some/None の両ケース動作確認済み。SPEC.md NFR1 の "MUST NOT remove the `user_event` override" 制約も、純粋ロジック分離によりリグレッションテスト可能な状態。

---

## Wake → Redraw 完全経路検証

```
[Provider thread]
  TimeProvider timer / GitBranch worker / Command worker / CwdProvider (OSC 7 受信)
    ↓ version counter ++
    ↓ wake() (Arc<WakeFn>)
    ↓
[wakeup.rs]
  WakeFn = Arc<dyn Fn() + Send + Sync>
    ↓
[shared_wake_fn() → install() で登録された]
  EventLoopProxy::send_event(())
    ↓
[winit event loop]
  UserEvent(()) dispatch
    ↓
[window_host.rs:1463]
  PocApp::user_event(&mut self, _, ())  ← 本フェーズで追加
    ↓ self.host.as_ref() == Some
    ↓
  host.window().request_redraw()
    ↓
[next frame]
  winit redraw_requested
    ↓
  egui pass / draw_terminal
    ↓
  app.status_bar_view_model()
    ↓
  TimeProvider::get_value() (Instant::now())
    ↓ resolved template → HTML parse → RichTextRun
[status bar 1Hz redraw 完成]
```

各段は以下のテストで覆われている:

| 段 | テスト |
|----|--------|
| Provider tick → wake | `time_provider_timer_thread_calls_wake_on_interval` (TS-29) |
| wake → user_event | (winit 内部、外部テスト不要) |
| user_event → request_redraw | `user_event_dispatches_redraw_when_host_present` (**TS-32**) |
| user_event host=None | `user_event_is_noop_when_host_absent` (**TS-32**) |
| TimeProvider Drop join | `time_provider_drop_joins_timer_thread` (TS-perf-3) |
| 実機 1Hz redraw | TS-30 (manual smoke, 後述) |

---

## セキュリティ検証 (NFR2)

- [x] OSC 777;statusbar 経由のコンテンツが完全に tag-strip (TS-27)
- [x] `CustomCommand` の name 検証で `;`, `..`, スペース, 制御文字を含む name が reject (`provider-command` テスト)
- [x] CustomCommand 実行で shell を経由しない (コードレビュー: `std::process::Command::new(executable)` で args 無し)
- [x] `<script>` / `<style>` ブロックが AST に残らない (TS-15)
- [x] 新規 crate 追加なし (`grep -E 'tokio|regex' native-poc/Cargo.toml native-poc/src/`: dependency への混入なし、`template_engine.rs` のコメント 1 件のみ)

---

## パフォーマンス検証

- [x] テンプレート解決 100k iter / 4 セクション (TS-perf-1) — `cargo test --release` で 1 秒未満 (sdd.5-check 結果)
- [x] HTML parse 10k iter / 256 バイト payload (TS-perf-2) — `cargo test --release` で 1 秒未満
- [x] Provider 由来スレッド leak なし (TS-perf-3) — TimeProvider × 1 + GitBranchProvider × 1 + CommandProvider × N すべて `Drop` で join 確認
- [x] LRU cache 動作確認 — `run_cache_hits_when_template_and_versions_match` / `run_cache_misses_when_version_changes` / `run_cache_evicts_oldest_when_full`
- [ ] 実機 frame drop なし — manual smoke で目視確認 (TS-30 と同時実施)

---

## 手動確認が必要な項目（実機確認待ち）

`native-poc/target-host/release/emterm-native-poc` (43,580,952 bytes, mtime 2026-05-23 15:46) を起動して確認:

### 重要: TS-30 (二次バグ回帰確認)

- [ ] **release binary 起動後、shell idle 状態（コマンド入力なし、cursor blink 無効）で 10 秒間放置し、左の時計が秒単位で進み続けることを目視確認**

これは Provider 由来の wake → `EventLoopProxy::send_event` → **`PocApp::user_event`** → `host.window().request_redraw()` の完全チェーン全段が release binary で実機動作することの最終確認。`ApplicationHandler::user_event` 未実装で 2 度目に発火した時計停止（`project_status_bar_design` 参照）の再発有無を確認する項目。

### 通常 manual smoke 項目

- [ ] 起動直後、ステータスバーが App Line 1 だけ表示され left = 時計、right = cwd basename
- [ ] 別ディレクトリへ `cd` すると right の cwd basename が更新される (CwdProvider event-driven wake)
- [ ] git repo 内で `{git_branch}` を含むテンプレに切り替え、branch 名と clean/dirty/untracked 色が出る
- [ ] `printf '\033]777;statusbar;set;left;hi\033\\'` で OSC 行 left に "hi" が反映
- [ ] `printf '\033]777;statusbar;set;left;<script>x</script>foo\033\\'` で OSC 行 left = "foo" (XSS 防御)
- [ ] `printf '\033]777;statusbar;clear\033\\'` で OSC 行が空になり、行ごと消える（自動非表示）
- [ ] `printf '\033]777;statusbar;hide\033\\'` で OSC 行が消える
- [ ] `printf '\033]777;markdown;...\033\\'` で従来の Markdown viewer が起動する（回帰）
- [ ] mux daemon に接続するとき OSC 行に daemon の left/right が出る。App Line 1/2 はローカルテンプレ
- [ ] mux 切断で OSC 行が消える
- [ ] `Settings::default().statusbar.enabled = false` 相当でステータスバー全体が描画されない（コード側で確認可）
- [ ] 視覚: 3 行の背景が同一（OSC 行に独自背景なし）、開閉アニメーションなし
- [ ] `~/.local/share/net.laser5.app.emterm/logs/emterm.log` に WARN/ERROR が新規発生していない

### legacy-tauri E2E 回帰確認

- [ ] `./scripts/run-e2e-docker.sh test` 全 spec pass (本タスクでは `src/status-bar/` 未変更だが、念のため次回 PR 直前に実施)

---

## 次のステップ

1. **TS-30 manual smoke 実施** (最優先)
   - `./native-poc/target-host/release/emterm-native-poc` を起動
   - shell idle 状態 (cursor blink OFF) で 10 秒間放置
   - 左時計が 10 回前後進むことを目視確認
   - pass すれば PoC の Go/No-Go 基準のうち「機能パリティ」項目がクリアされる
2. その他の manual smoke 項目を順に実施（OSC 777 経由テスト、git branch 状態色など）
3. すべて pass したら sdd.6-verify は完了扱い。PoC ゴールの最終判定（機能パリティ + パフォーマンス改善の両方）にすすむ

---

## 修正履歴

- **v1 (初版, 2026-05-22 以前)**: `request_repaint_after` 依存の Wakeup 設計 → release binary idle で時計停止（一次バグ）
- **v2 (2026-05-22)**: Provider オーナーシップ式 refresh-redraw を適用。各 Provider が自前の thread + `WakeFn` を所有し、`EventLoopProxy::send_event(())` で winit を起こす設計に切替 → しかし `ApplicationHandler::user_event` の no-op default impl により release binary idle で時計再停止（二次バグ）
- **v3 (本版, 2026-05-23)**: `PocApp::user_event` を実装し `host.window().request_redraw()` を呼ぶように修正。`request_redraw_on_user_event` free fn として純粋ロジックを切り出し、TS-32 unit test 2 件で host=Some/None の両ケースを単体検証可能に。これで wake → redraw 経路が release binary で完成

### v3 修正範囲（局所性確認）

- 修正ファイル: `native-poc/src/window_host.rs` **のみ**
- 差分行数: +69 / -0
- 新規 import: なし (既存 `ActiveEventLoop`, `ApplicationHandler` を流用)
- 既存テスト影響: なし (全 564 件 pass、ignored 1 件は既存ホスト環境依存テスト)
- 新規テスト: 2 件 (`window_host::tests::user_event_dispatches_redraw_when_host_present`, `window_host::tests::user_event_is_noop_when_host_absent`)
- SPEC.md `Notes` セクション L932-L965 に明文化された "MUST implement user_event" 制約を満たす
- release binary 再ビルド済み: `native-poc/target-host/release/emterm-native-poc` (43,580,952 bytes, mtime 2026-05-23 15:46)
