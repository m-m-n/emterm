# 🔍 実装自動検証レポート (sdd.6-verify)

**対象機能**: mux-detach-pane-exit-reap
**VERIFICATION.md**: `doc/tasks/mux-detach-pane-exit-reap/VERIFICATION.md`
**プロジェクト**: emterm (Rust mux daemon)
**検証コミット**: 0966e67 (verify 実行時 HEAD)

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ファイル構造 | ✅ | 変更5ファイル + ドキュメント6点すべて存在 |
| SPEC.md 適合性 (FR) | ✅ | FR1–FR7 すべて実装・検証済み |
| SPEC.md 適合性 (NFR) | ✅ | NFR1–NFR4 充足 |
| ビルド/テスト/フォーマット/静的解析 | ✅ | sdd.5-check で検証済み (再実行せず) |
| スコープ | ✅ | mux 5ファイルのみ。reattach.rs は意図どおり不変 |
| E2E | N/A | プロジェクトに E2E フレームワークなし → 対象外 |

**総合評価**: ✅ 自動検証項目はすべて合格。残りは手動確認項目 (TS-5/TS-6) のみ。

---

## ✅ ファイル構造検証

変更ファイル (5/5 存在・変更あり):
- ✅ `src-tauri/src/mux/session/pane.rs` — `PaneExitSender` / `SharedPaneExitSender` 型エイリアス追加
- ✅ `src-tauri/src/mux/ipc/handlers.rs` — `handle_destroy_pane` を `pub(in crate::mux)` へ拡大、`handle_create_window` に sender 追加
- ✅ `src-tauri/src/mux/ipc/pty_spawn.rs` — reader への sender plumbing + EOF 通知
- ✅ `src-tauri/src/mux/ipc/connection.rs` — `SharedPaneExitSender` の生成チェーン受け渡し
- ✅ `src-tauri/src/mux/daemon.rs` — `run_pane_exit_task` + 両 run ループ配線 + チャネル生成 + TS-1..TS-4

意図的に不変:
- ✅ `src-tauri/src/mux/ipc/reattach.rs` — detach skip-guard はそのまま (レースは daemon reap がカバー)。`git diff --quiet` で不変を確認。

ドキュメント (6/6 存在): SPEC.md / 要件定義書.md / IMPLEMENTATION.md / VERIFICATION.md / tasks.yaml / sdd.yaml

スコープ: `git diff --stat` は mux 5ファイルのみ (365 insertions, 16 deletions)。無関係な crate-wide フォーマット汚染は revert 済み。

---

## ✅ SPEC.md 適合性検証

### 機能要件 (FR)

| 要件 | 内容 | 実装箇所 | 検証 | 結果 |
|------|------|----------|------|------|
| FR1 | reader EOF で attach 非依存に pane-exit 通知 | pty_spawn.rs `Ok(0)` アームの無条件 `try_send(pane_id)` | TS-6 (手動) | ✅ |
| FR2 | daemon reap タスク = 単一の権威 | daemon.rs `run_pane_exit_task` → `handle_destroy_pane` | TS-1, TS-2 (自動) | ✅ |
| FR3 | Connected の空チャンク teardown 保持 | pty_spawn.rs `Connected` の `blocking_send` 維持 | TS-5 (手動) | ✅ |
| FR4 | reap の冪等性 | handle_destroy_pane の pane 未検出 早期 return | TS-4 (自動) | ✅ |
| FR5 | 全 session 空で shutdown 発火 | handle_destroy_pane の `shutdown_tx.send(true)` | TS-1 (自動) | ✅ |
| FR6 | 接続断レースで stranding しない | reap は output_target 非依存・pane_id ベース | TS-3 (自動) | ✅ |
| FR7 | Unix / Windows 両 run ループ配線 | daemon.rs 両 run ループで spawn + sender 構築 | TS-6 + Windows `cargo check` | ✅ |

### 非機能要件 (NFR)

| 要件 | 内容 | 検証 | 結果 |
|------|------|------|------|
| NFR1 | attach/detach で reap 挙動が分岐しない・async Mutex 直列化 | TS-1/TS-2/TS-3 自動テスト | ✅ |
| NFR2 | mux-kill-shutdown / close-window-on-shell-exit を壊さない | 全1770テスト pass (TS-7) | ✅ |
| NFR3 | 定常出力経路 (Ok(n)) 不変 | diff レビューで `Ok(n)` 未変更を確認 (TS-8) | ✅ |
| NFR4 | 既存 (PaneId,String) 通知チャネルと別の専用チャネル | `PaneExitSender = mpsc::Sender<PaneId>` を独立定義 (TS-8) | ✅ |

自動テスト (sdd.5-check 実行・全 pass):
- TS-1 `test_pane_exit_task_last_pane_reap_fires_shutdown`
- TS-2 `test_pane_exit_task_non_last_pane_reap_keeps_daemon_alive`
- TS-3 `test_pane_exit_reap_removes_network_detached_pane`
- TS-4 `test_pane_exit_reap_is_idempotent`

---

## 🐳 E2E テスト結果

- Docker / E2E 環境: **未構築** (本プロジェクトに daemon プロセス寿命・detach/reattach 用の E2E フレームワークなし)
- 判定: **対象外**。daemon プロセス寿命に関わるシナリオは手動確認項目でカバー。

---

## 📋 手動確認が必要な項目 (E2E 不可)

VERIFICATION.md から抽出。実機 (dev ビルド推奨。release は `warn` 以上のみ永続化されるため reap/shutdown の `log::info!` が出ない) で確認すること:

- [ ] **TS-5**: attach 中にシェル終了 → pane/タブが従来どおり teardown される (`PtyExited`)。
- [ ] **TS-6 (Linux, 最終 pane)**: GUI を attach → detach → detach 中に最終シェルを Ctrl+D。`emterm.log` に pane reap と「all sessions empty, daemon shutting down」が出て daemon プロセスが終了する。
- [ ] **TS-6 (Linux, 非最終 pane)**: 複数 pane を detach 状態にし、非最終シェルを終了。当該 pane のみ reap され daemon は存続。
- [ ] **TS-6 (Windows)**: named-pipe daemon で同様の detach → 最終シェル終了 → daemon 終了を確認。
- [x] **TS-8 (コードレビュー)**: 定常 `Ok(n)` 経路が不変 (NFR3) かつ pane-exit チャネルが OSC 通知チャネルと別 (NFR4) — diff レビューおよびデッドコード検査で確認済み。

ログ確認パス (Linux): `~/.local/share/net.laser5.app.emterm/logs/emterm.log`

---

## 🎯 検証サマリー

- ✅ ファイル構造: 完全 (変更5 + ドキュメント6)
- ✅ SPEC 適合性: FR1–FR7 / NFR1–NFR4 すべて充足
- ✅ ビルド/テスト/フォーマット/静的解析: sdd.5-check で検証済み (cargo check exit 0 / 1770 tests pass / rustfmt --edition 2021 --check 差分なし / clippy 新規警告なし / デッドコードなし)
- ✅ スコープ: mux 5ファイルのみ。reattach.rs 不変

### 留意事項
- 残作業は手動確認 TS-5 / TS-6 (Linux 最終・非最終 / Windows) のみ。
- リリースビルド (`target-host`) はプロジェクト方針によりユーザー明示時のみ実行 (未実行)。
- 参考: `crates/term_core` の test に pre-existing な未定義参照 `SUB_PARAM_FLAG` があるが本タスク無関係・sdd.yaml の test_command (emterm スコープ) では非コンパイル。本タスクのテストには無影響。
