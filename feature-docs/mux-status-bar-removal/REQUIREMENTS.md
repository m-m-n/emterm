---
title: "mux-status-bar-removal"
created_date: 2026-08-04
status: draft
---

# mux-status-bar-removal - 要件定義書

## 1. 概要

### 1.1 背景

mux ステータスバー UI は mux サイドバーと重複した表示面になっている（セッション名 / ウィンドウ / ペイン / エージェント状態はすべてサイドバーに表示されている）。
またこの UI は「mux セッションが attach されている間だけターミナルグリッドの行数が減る」という設計（観測値 rows 49⇄51）を生んでおり、これは `tmp/discussion-mux-tab-switch-leak.md` に記録された mux→tmux タブ切り替え時の XTWINOPS 応答リークの一因になっている。

### 1.2 目的

- 冗長な UI 面である mux ステータスバーを削除する。
- mux attach 状態に依存したターミナルグリッド行数の差分（rows 49⇄51）を解消する。

### 1.3 スコープ

**対象**: mux 由来の部分のみ。すなわち daemon 側 `StatusBarEngine`、`StatusUpdate` / `RequestStatusUpdate` プロトコル、GUI 側 `mux_status_state` と OSC 行の mux ブランチ、および全ミラー上の `mux.statusbar` 設定（想定事項 A1）。

**対象外**:

- 一般アプリステータスバー（トップレベル `statusbar_*` 設定、App Line 1/2、OSC `777;statusbar` ディスパッチャ）は対象外であり、そのまま保全する（想定事項 A1、FR6）。
- 「タブごとに独立したグリッドサイズを保持する」は別タスク（フォローアップ）である。タブごとのグリッド結合の根本原因解消はそちらに委ねる。

## 2. ビジネス要件

### 2.1 ビジネス目標

- mux ステータスバー UI を削除する。これは mux サイドバーとの重複（セッション名 / ウィンドウ / ペイン / エージェント状態はすべてサイドバーに表示済み）になっており、冗長な UI 面を排除する。
- 「mux セッションが attach されている間だけターミナルグリッドの行数が減る」設計（観測値 rows 49⇄51）を解消する。これは `tmp/discussion-mux-tab-switch-leak.md` に記録された mux→tmux タブ切り替え時の XTWINOPS 応答リークの一因である。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm の mux 利用者 | mux セッションを attach してターミナルを使うユーザー。mux ステータスバーの表示面と、attach 時のグリッド行数変化の影響を受ける |
| `mux.statusbar` 設定の利用者 | `settings.json` の `mux.statusbar`（left / right / commands）でテンプレートやコマンドを設定していたユーザー。設定項目は意図的に廃止される（想定事項 A2） |

### 2.3 期待される効果

- mux サイドバーと重複した UI 面がなくなる。
- ターミナルグリッド行数が mux attach 状態に依存しなくなる（rows 49⇄51 の差分が消える）。

## 3. ユースケース

### 3.1 ユースケース一覧

本タスクは既存 UI 面の削除・クリーンアップであり、新規 UI も新規ユースケースも発生しない（design ステップが skip された理由と同一：置き換え先の UI である mux サイドバーは既に存在し、変更されない）。したがってユースケースの新規定義は行わない。

### 3.2 ユースケース詳細

該当なし。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | Remove GUI-side mux status bar state and rendering path | GUI 側の mux 由来ステータスバー経路を削除する | resolved |
| FR2 | Remove the daemon-side mux status bar engine | daemon 側 `StatusBarEngine` と周辺機構を削除する | resolved |
| FR3 | Retire the StatusUpdate / RequestStatusUpdate protocol messages | プロトコルメッセージを廃止し opcode を予約する | resolved |
| FR4 | Remove the mux statusbar settings schema on all mirrors | 全ミラー（Rust / native / TypeScript）から設定スキーマを削除する | resolved |
| FR5 | Terminal grid height identical with and without mux | mux の有無でターミナルグリッド行数が変わらないようにする | resolved |
| FR6 | Preserve the general (non-mux) status bar | 一般（非 mux）ステータスバーを保全する | resolved |
| FR7 | Preserve per-pane cwd tracking (relocate detect_osc7_cwd) | ペインごとの cwd 追跡を維持する（`detect_osc7_cwd` は移設） | resolved |
| FR8 | Tolerate stale peers and stale settings | 旧バージョンのピアと旧設定を許容する | resolved |

### 4.2 機能詳細

#### FR1: Remove GUI-side mux status bar state and rendering path

**説明**: Delete the mux-sourced status bar path in the GUI: the `Tab::mux_status_state: Option<StatusUpdateMsg>` field and its `MessageType::StatusUpdate` latch/clear (src-tauri/src/tabs.rs:264, ~1862-1869, ~2306); the projection of `mux_status` into the status bar view model (src-tauri/src/app.rs:2467-2470 and `App::status_bar_state()`); the `mux_status` parameter and mux branch of `build_view_model` / `build_osc_row` (src-tauri/src/status_bar/runtime.rs:173-213, 269-271); and the mux-specific rendering and tests in src-tauri/src/ui/status_bar.rs (TS-25/TS-26 and related). Any GUI sender of `RequestStatusUpdate` is also removed.

#### FR2: Remove the daemon-side mux status bar engine

**説明**: Delete `StatusBarEngine` and its supporting machinery in src-tauri/src/mux/ipc/statusbar.rs (settings loading, template resolution, command execution/caching, periodic StatusUpdate generation, `SharedActivePaneId`, the statusbar-only `SharedPaneCwdMap` registry) and its wiring in src-tauri/src/mux/ipc/connection.rs (construction ~370-390, render ticks ~804-844, force-render arms ~1338-1359, the `MessageType::RequestStatusUpdate` handler ~1377-1379, and `register_session_pane_cwds` ~1491-1507). Update the doc comment in src-tauri/src/windows_exec.rs, which references "mux statusbar commands".

#### FR3: Retire the StatusUpdate / RequestStatusUpdate protocol messages

**説明**: Remove `StatusUpdateMsg` and the `StatusUpdate` (0x16) / `RequestStatusUpdate` (0x17) message types from crates/mux_ipc/src/protocol.rs and their round-trip tests. The opcode values 0x16 and 0x17 are reserved (never reused for a new message) so mixed-version GUI/daemon pairs cannot misinterpret frames.

#### FR4: Remove the mux statusbar settings schema on all mirrors

**説明**: Delete `MuxStatusbarSettings` / `MuxStatusbarCommand` and the `mux.statusbar` field from crates/app_settings/src/settings.rs (~645, 707-730) and their tests (~808-852); the native mirror `MuxStatusbarSettings` / `RawMuxStatusbar` and loader tests in src-tauri/src/settings.rs (~216, 231, 251, 1370-1377, 2288-2335); and the TypeScript `MuxStatusbarSettings` / `MuxStatusbarCommand` interfaces plus `statusbar` field on the mux settings interface in src-tauri/web-shared/settings/types.ts (~129-147) with the corresponding fixture data in src-tauri/web-shared/settings/sections/mux-section.test.ts (~52). Note: mux-section.ts itself renders no statusbar fields, so no settings-UI controls need removal — only types/fixtures.

#### FR5: Terminal grid height identical with and without mux

**説明**: After removal, attaching/detaching a mux session must not change the status bar's `visible_row_count` and therefore not change the bottom inset (`panel_height_logical` → `WindowHost::refresh_status_bar_insets`, src-tauri/src/window_host.rs:1238-1243) or the terminal grid rows. The observed rows 49⇄51 mux-conditional difference is gone; grid rows depend only on window size and non-mux status bar state.

#### FR6: Preserve the general (non-mux) status bar

**説明**: The app status bar remains fully functional: App Line 1/2 templates (`{time}` / `{cwd}` / custom commands), the OSC `777;statusbar` dispatcher route (src-tauri/src/status_bar/osc_dispatcher.rs, src-tauri/src/callbacks.rs), the top-level `statusbar_*` settings, and src-tauri/web-shared/settings/sections/status-bar-section.ts are untouched. Only the mux-sourced OSC-row branch is removed; the dispatcher-sourced OSC row keeps working. The ResizeSettler / status-bar-inset machinery in window_host.rs stays (it still serves the general status bar's dynamic row count).

#### FR7: Preserve per-pane cwd tracking (relocate detect_osc7_cwd)

**説明**: `detect_osc7_cwd` (src-tauri/src/mux/ipc/statusbar.rs:474) is consumed by the pane PTY reader (src-tauri/src/mux/ipc/pty_spawn.rs:1003) to maintain `Pane.cwd`, which is persisted across daemon hot-upgrade (src-tauri/src/mux/upgrade.rs:587) and pane restoration (src-tauri/src/mux/session/pane.rs) — it is NOT statusbar-only. Relocate this function (and its tests) out of statusbar.rs rather than deleting it; per-pane cwd tracking behavior is unchanged. Only the statusbar's own `pane_cwd_map` registry and `active_pane_id` tracker are deleted with the engine.

#### FR8: Tolerate stale peers and stale settings

**説明**: (a) A GUI built after this change must gracefully ignore a `StatusUpdate` (0x16) frame pushed by an older, still-running daemon (long-lived daemons and the hot-upgrade path make mixed versions a real scenario), and a new daemon must gracefully ignore `RequestStatusUpdate` (0x17) from an older GUI — at most a warn log, never an error/disconnect. (b) Existing user settings.json files still containing a `mux.statusbar` object must continue to deserialize without error (obsolete keys ignored).

## 5. 非機能要件

### 5.1 ビルド要件

- **NFR1 - CLI-only build remains green**: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` still compiles (mux and settings code are touched; feature gates must not break).

### 5.2 テスト要件

- **NFR2 - Full test suites remain green**: Rust `--lib` suites for src-tauri and affected workspace crates (app_settings, mux_ipc), the integration tests (including mux_hot_upgrade with `--test-threads=1`), `bun test`, and `bun run typecheck` all pass after removal.

### 5.3 既存機能の非退行要件

- **NFR3 - No behavior change to tab bar / sidebar / agent status**: `mux_session_name`, `mux_group`, `AgentStatusUpdate` handling, the mux sidebar, and tab-bar mux window grouping are unrelated to the status bar and must be byte-for-byte unaffected.

### 5.4 パフォーマンス / セキュリティ / 可用性要件

本タスクでは該当する要件は確定していない。

## 6. UI/UX要件

### 6.1 画面設計要件

- mux ステータスバーの表示面を削除する（FR1、FR2）。置き換え先の UI である mux サイドバーは既に存在し、変更されない。
- 一般アプリステータスバー（App Line 1/2、OSC `777;statusbar` ディスパッチャ由来の OSC 行）は表示・動作ともそのまま維持する（FR6）。
- ユーザー設定可能な mux statusbar のテンプレート / コマンド（`mux.statusbar` の left / right / commands）は、サイドバーによる置き換えを伴わずに削除する。これは意図的な機能廃止であり、移行漏れではない（想定事項 A2）。
- mux の attach / detach でターミナルグリッドの行数が変化しない（FR5）。

### 6.2 画面遷移

該当なし（画面遷移の変更なし）。

### 6.3 レスポンシブ対応

該当なし。

## 7. データ要件

### 7.1 設定スキーマ

`mux.statusbar` 設定スキーマを以下の全ミラーから削除する（FR4）。

| ミラー | 削除対象 |
|--------|----------|
| crates/app_settings/src/settings.rs | `MuxStatusbarSettings` / `MuxStatusbarCommand` と `mux.statusbar` フィールド（~645, 707-730）およびテスト（~808-852） |
| src-tauri/src/settings.rs | native ミラーの `MuxStatusbarSettings` / `RawMuxStatusbar` とローダーテスト（~216, 231, 251, 1370-1377, 2288-2335） |
| src-tauri/web-shared/settings/types.ts | TypeScript の `MuxStatusbarSettings` / `MuxStatusbarCommand` インターフェースと mux 設定インターフェースの `statusbar` フィールド（~129-147） |
| src-tauri/web-shared/settings/sections/mux-section.test.ts | 対応するフィクスチャデータ（~52） |

mux-section.ts 自体は statusbar のフィールドを描画していないため、設定 UI コントロールの削除は不要（型とフィクスチャのみ）。

### 7.2 プロトコルメッセージ

| メッセージ | opcode | 扱い |
|------------|--------|------|
| `StatusUpdate`（`StatusUpdateMsg`） | 0x16 | crates/mux_ipc/src/protocol.rs から削除。opcode は予約（新規メッセージに再利用しない） |
| `RequestStatusUpdate` | 0x17 | 同上 |

### 7.3 データ保持期間

既存ユーザーの `settings.json` に残る `mux.statusbar` オブジェクトは、エラーなくデシリアライズできること（廃止キーは無視、FR8(b)）。

## 8. 外部連携

### 8.1 連携経路

| 経路 | 連携方法 | 本タスクでの扱い |
|------|----------|------------------|
| GUI ⇄ mux daemon | mux IPC プロトコル（crates/mux_ipc） | `StatusUpdate`(0x16) / `RequestStatusUpdate`(0x17) を廃止。opcode は予約し、旧バージョンのピアからの受信は warn ログ止まりで許容（FR3、FR8(a)、想定事項 A3） |
| アプリケーション → GUI ステータスバー | OSC `777;statusbar` ディスパッチャ | 変更なし（FR6） |
| シェル → daemon（ペイン cwd） | OSC 7 | `detect_osc7_cwd` を statusbar.rs 外へ移設し、挙動は変更なし（FR7、想定事項 A4） |

### 8.2 バージョン混在

長寿命の daemon と hot-upgrade 経路があるため、GUI と daemon のバージョン混在は現実に起きうる。新しい GUI は旧 daemon からの 0x16 を、新しい daemon は旧 GUI からの 0x17 を、それぞれ warn ログ止まりで無視し、エラーや切断にはしない（FR8(a)）。

## 9. 制約条件

### 9.1 技術的制約

- サイドバーの実装は完了済みであることが前提（precondition）。
- opcode 0x16 / 0x17 は予約であり再利用しない（FR3、想定事項 A3）。
- `detect_osc7_cwd` とペインごとの cwd 追跡は削除せず移設する。`Pane.cwd` は hot-upgrade の引き継ぎとペイン復元にも使われるため（FR7、想定事項 A4）。
- window_host.rs の ResizeSettler / `refresh_status_bar_insets` 機構は一般ステータスバーのために残す。ここでは mux 固有のコメント・テストのみ更新する（FR6、想定事項 A5）。
- mux とセッティングのコードに触れるため、feature gate を壊さないこと（NFR1）。

### 9.2 スコープ上の制約

- rows 49⇄51 のグリッド差分は本タスクで解消する。
- サイドバーの常時表示モードに由来する cols 171⇄207 のグリッド差分は本タスクでは解消しない（明示的に対象外）。
- 「タブごとに独立したグリッドサイズを保持する」は別のフォローアップタスクであり、タブごとのグリッド結合の根本原因解消はそちらに委ねる。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 対応策 |
|------|--------|
| 長寿命 daemon / hot-upgrade により GUI と daemon のバージョンが混在し、廃止済みメッセージが流れてくる | opcode 0x16 / 0x17 を予約して再利用せず、双方が受信時に warn ログ止まりで無視する（FR3、FR8(a)、想定事項 A3） |
| 既存ユーザーの settings.json に `mux.statusbar` が残っている | 廃止キーを無視してデシリアライズを継続する（FR8(b)、AC4） |
| `detect_osc7_cwd` を statusbar と一緒に消すと、ペイン cwd の hot-upgrade 引き継ぎ・ペイン復元が壊れる | 削除ではなく移設し、テストも移す（FR7、TS5） |
| mux / settings のコードに触れることで CLI-only ビルドの feature gate が壊れる | `--no-default-features` の `cargo check` をグリーンに保つ（NFR1） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: No mux status bar rendering code, state management, or settings items remain — repository-wide searches for `MuxStatusbarSettings`, `StatusUpdateMsg`, `mux_status_state`, and `StatusBarEngine` return no hits outside reserved-opcode comments and historical docs.
- [ ] AC-2: Functions not covered by the sidebar are dispositioned: pane cwd tracking is retained (FR7), user-configurable mux statusbar templates/commands are intentionally retired with no replacement (assumption A2).
- [ ] AC-3: Terminal grid rows are identical between mux-attached and non-mux states (the rows 49⇄51 delta is gone); status-bar row count is provably independent of mux attach state in unit tests.
- [ ] AC-4: A settings.json containing a populated `mux.statusbar` section loads without error.
- [ ] AC-5: All builds and test suites in NFR1/NFR2 are green.

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS1（FR1, FR5, FR6）: Unit (runtime.rs) — `build_view_model` no longer takes/uses mux status; OSC row renders only from the OSC 777 dispatcher; row count is unchanged by mux attach.
- [ ] TS2（FR1, FR8）: Unit (tabs.rs/app.rs) — receiving a raw frame with retired opcode 0x16 from a stale daemon is ignored with at most a warn log and does not disturb the tab（現行の `on_mux_message_status_update_caches_payload_on_tab` テストを置き換える）。
- [ ] TS3（FR4, FR8）: Unit (app_settings + src-tauri/src/settings.rs) — JSON with a `mux.statusbar` object deserializes; the obsolete key is ignored.
- [ ] TS4（FR5, FR6）: Unit (window_host.rs) — status-bar inset/grid-size candidates are driven only by general status-bar visibility; no mux-conditional path remains.
- [ ] TS5（FR7）: Unit (mux/ipc) — relocated `detect_osc7_cwd` tests still pass; pane cwd still updates from OSC 7 and survives hot-upgrade（mux_hot_upgrade 統合テストはグリーンのまま）。
- [ ] TS6（FR4）: TypeScript — `bun test` と `bun run typecheck` が、types.ts とフィクスチャから `MuxStatusbarSettings` を削除した状態で通る。
- [ ] TS7（FR5、手動検証・自動 verify では実行しない）: 3 タブ（mux / tmux / plain）構成で mux→tmux に切り替えたとき、非アクティブな tmux タブの PTY がリサイズされず、XTWINOPS 応答テキストが tmux 画面に漏れない（`tmp/discussion-mux-tab-switch-leak.md` のシナリオ）。タブごとのグリッド結合の根本原因解消は別のフォローアップタスクに残る。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| mux ステータスバー | mux 由来の部分のみを指す。daemon の `StatusBarEngine`、`StatusUpdate` / `RequestStatusUpdate` プロトコル、GUI の `mux_status_state` と OSC 行の mux ブランチ、全ミラーの `mux.statusbar` 設定（想定事項 A1） |
| 一般アプリステータスバー | トップレベル `statusbar_*` 設定、App Line 1/2、OSC `777;statusbar` ディスパッチャ。本タスクの対象外で保全される（想定事項 A1、FR6） |
| `StatusBarEngine` | daemon 側の mux ステータスバー生成エンジン（設定読み込み、テンプレート解決、コマンド実行・キャッシュ、定期 StatusUpdate 生成） |
| reserved-not-reused（予約・再利用しない） | opcode 0x16 / 0x17 を新規メッセージに割り当てないこと。バージョン混在時のフレーム誤解釈を防ぐ（想定事項 A3） |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1: "mux status bar" means exactly the mux-sourced pieces — daemon StatusBarEngine, the StatusUpdate/RequestStatusUpdate protocol, the GUI mux_status_state / OSC-row mux branch, and the `mux.statusbar` settings on all mirrors. The general app status bar (top-level `statusbar_*` settings, App Line 1/2, OSC 777;statusbar dispatcher) is out of scope and preserved.
- [x] A2: User-configurable mux statusbar templates/commands (mux.statusbar left/right/commands) are removed without a sidebar replacement — intentional feature retirement, not a migration gap.
- [x] A3: Protocol opcodes 0x16/0x17 are reserved-not-reused, and both sides tolerate receiving the retired messages from an older peer (FR8).
- [x] A4: `detect_osc7_cwd` and per-pane cwd tracking are kept (relocated), since Pane.cwd feeds hot-upgrade handoff and pane restoration beyond the status bar.
- [x] A5: The ResizeSettler / refresh_status_bar_insets machinery in window_host.rs is retained for the general status bar; only mux-specific comments/tests there are updated.
- [x] A6: tmp/discussion-mux-tab-switch-leak.md exists in the repository and is background context only; the grid-parity AC is the extent to which this task addresses the leak, with the per-tab grid size task explicitly out of scope.
- [x] A7: design step is skipped — 理由: Pure removal/cleanup of an existing UI surface; no new UI, no visual or layout design decisions. The replacement UI (mux sidebar) already exists and is unchanged.
- [x] 前提: サイドバーの実装は完了済み。
- [x] グリッド差分の扱い: rows 49⇄51 は本タスクで解消し、cols 171⇄207（サイドバー常時表示モード由来）は本タスクでは解消しない。

### 14.2 未確認・保留事項

なし（全 FR / NFR が resolved）。

## 15. 参考資料

- tmp/discussion-mux-tab-switch-leak.md: mux→tmux タブ切り替え時の XTWINOPS 応答リークの議論。背景コンテキストのみ（想定事項 A6）
- feature-docs/mux-status-bar-removal/SPEC.md: 本要件定義書に対応する実装仕様
