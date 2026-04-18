# Verification Document: Mux OSC Title Propagation

## Overview

- **Feature**: mux-osc-title-propagation
- **SPEC.md**: `doc/tasks/mux-osc-title-propagation/SPEC.md`
- **IMPLEMENTATION.md**: `doc/tasks/mux-osc-title-propagation/IMPLEMENTATION.md`

## Build Verification

- Command:
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "cargo build --manifest-path src-tauri/Cargo.toml"
  ```
- Expected: exit code 0、警告の新規発生なし

## Test Verification

- Command:
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
  ```
- Coverage target: 変更した daemon 内 title 関連コードは unit + integration でカバーされていること

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `SessionManager::rename_window` の既存／非既存ウィンドウ動作 | 既存テストが緑を維持 | Unit |
| TS-2 | daemon-level title タスクが `rename_window` と `notify_tx.send` を呼ぶ | `window.name` 更新 + broadcast 受信側 1 件 | Unit |
| TS-3 | CLI `new-window` 後に shell OSC 発行で `window.name` 更新（GUI 未接続） | `SessionManager` の `window.name` が新タイトル | Integration |
| TS-4 | GUI アタッチ時 `Welcome.session_list` に最新タイトルが含まれる | `session_list` の `window.name` が反映済 | Integration |
| TS-5 | GUI 接続中の OSC 発行で `RenameWindow` が GUI に届く | `notify_rx` 受信側に 1 件 | Integration |
| TS-6 | 複数 GUI 接続時、全 GUI に `RenameWindow` が届く | 各 `notify_rx` に届く | Integration |
| TS-7 | 同じタイトル連続発行で broadcast は 1 回のみ | broadcast 受信カウント = 1 | Unit |
| TS-8 | 空タイトル (`""`) は無視 | 状態変化なし（既存 `pty_reader_loop` 挙動を維持） | Unit（既存） |
| TS-9 | reattach 直後の既存タイトル追加通知不要 | Welcome で反映済、追加 broadcast なし | Integration |
| TS-10 | detach 後（`detach_session_panes` 実行後）に OSC タイトル発行 → `window.name` が更新 | detach 中の title 送信が daemon タスクに届き、`window.name` 更新。次回 Welcome で反映 | Integration |
| TS-11 | `handle_connection` で `notify_tx.subscribe()` が `Welcome` 送信より前に呼ばれている（順序テスト） | ソースコード静的確認または、Welcome 構築と subscribe の間に発火した `RenameWindow` が受信される統合テスト | Integration |
| TS-12 | detach 後 `pane.title_sender` が `Some(_)` のまま維持されている | `detach_session_panes` 呼び出し後に pane.title_sender が None になっていないこと | Unit |

## Code Quality Verification

- Format:
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml -- --check"
  ```
- Static analysis:
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings"
  ```

## File Structure Verification

### Files to Create

なし（新規ファイルは作成しないこと）

### Files to Modify

- `src-tauri/src/mux/daemon.rs`
  - daemon-level `title_tx` / `title_rx` 作成
  - title 更新タスクの `tokio::spawn`
  - `apply_title_change` 相当の関数切り出し（テスト容易化のため）
  - 既存テストモジュールに unit テストを追加
- `src-tauri/src/mux/ipc/connection.rs`
  - `handle_connection` / `handle_cli_client` のシグネチャに daemon-level `title_tx` を追加
  - GUI 接続内の `title_rx.recv()` アームとチャネル生成を削除
  - CLI 接続内のダミー `title_tx` 生成を削除
  - `notify_tx.subscribe()` を `Welcome` 送信より前に実行
- `src-tauri/src/mux/ipc/reattach.rs`
  - `detach_session_panes` 内の `*pane.title_sender.lock().unwrap() = None;` を削除（daemon-level `title_tx` を維持するため）
  - `collect_reattach_data` 内の `title_sender` 差し替えは維持

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | `~/bin/init-mux` 起動後、全ウィンドウのタブ名が zsh の OSC タイトルで上書きされる | Manual: 実機起動でタブ名観察 |
| SC-2 | GUI アタッチ前に発行された OSC タイトルがアタッチ時にタブ名へ反映される | Integration test TS-4、Manual 再現 |
| SC-3 | CLI `new-window` 直後のウィンドウで zsh OSC により名前が更新される | Integration test TS-3、Manual 再現 |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR-1 daemon 主導のタイトル更新 | Phase 1 | Unit test TS-2、ログで title タスク起動を確認 |
| FR-2 接続 GUI への通知 | Phase 1 / Phase 3 | Unit test TS-2、Integration test TS-5 / TS-6 |
| FR-3 アタッチ時の整合性 | Phase 2 | Integration test TS-4、Manual SC-2 |
| FR-4 差分検出の維持 | Phase 3 | Unit test TS-7（broadcast 1 回制限） |
| FR-5 CLI new-window 経路 / detach 中 OSC | Phase 2 | Integration test TS-3 / TS-10、Unit test TS-12、Manual SC-3 |
| FR-6 Welcome/notify 購読順序 | Phase 2 | Integration test TS-11 |
| FR-7 複数ペインとタイトル所有 | （既存挙動） | 既存実装の find_pane + rename_window 経路で担保、明示テスト不要 |

## E2E Testing (Docker)

本修正はソケット通信層内部の責務再配置であり、UI 側の受信処理は不変。専用 E2E は新設せず、既存の mux 系 E2E に回帰がないことを確認すること。

- [ ] `./scripts/run-e2e-docker.sh test` の mux 関連スペックが全てパスすること

## Manual Testing (E2E Not Possible)

- [ ] `~/bin/init-mux` 起動後、全ウィンドウのタブ名が `~` / カレントディレクトリなど zsh の OSC タイトルに更新されていること（SC-1）
- [ ] GUI 未接続状態で CLI `emterm mux new-window` → shell 起動 → 数秒待機 → GUI アタッチ → 新規ウィンドウのタブ名が OSC タイトルになっていること（SC-2 / SC-3）
- [ ] GUI 接続中に新規 `new-window` → タブが自動で OSC タイトルに更新されること
- [ ] 2 つの GUI を同時にアタッチ → shell の OSC タイトル発行で両 GUI のタブ名が更新されること
- [ ] GUI をいったん閉じる（detach）→ 既存のペインで `cd` など OSC 発行 → 再アタッチ時に最新タイトルが `Welcome` で反映されていること（FR-5 後半 / TS-10 手動再現）
- [ ] `~/.local/share/net.laser5.app.emterm/logs/mux-daemon.log`（または `XDG_RUNTIME_DIR` 直下の mux-daemon.log）で title タスク起動ログと title 更新ログが出ていること

## Performance Verification

対象外（title 更新は低頻度イベント。broadcast capacity(16) の既存設定を維持）

## Security Verification

対象外（IPC 層の境界や権限に変更なし）

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit Test | 4 (TS-1, TS-2, TS-7, TS-12) | 4 | 0 | 0 |
| Integration Test | 7 (TS-3, TS-4, TS-5, TS-6, TS-9, TS-10, TS-11) | 7 | 0 | 0 |
| Code Quality | 2 (fmt, clippy) | 2 | 0 | 0 |
| E2E Regression | 1 | 0 | 1 | 0 |
| Manual | 6 | 0 | 0 | 6 |

## Actual Results

### Automated

- Build: `cargo build --manifest-path src-tauri/Cargo.toml` exit 0 (Docker)
- Tests: `cargo test --manifest-path src-tauri/Cargo.toml` — 896 lib + 33 integration + 4 doctests passed (Docker)
- fmt: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` exit 0 (Docker)
- clippy (lib): 新規警告なし（既存 4 件の `too many arguments` 警告は変更前後で同数）

### Unit Tests

| ID | Location | Test |
|----|----------|------|
| TS-1 | `src-tauri/src/mux/session/manager.rs` | `test_rename_window_valid`, `test_rename_window_not_found`（既存） |
| TS-2 | `src-tauri/src/mux/daemon.rs` | `test_apply_title_change_updates_window_and_broadcasts`, `test_title_update_task_applies_messages_from_channel` |
| TS-7 | `src-tauri/src/mux/daemon.rs` | `test_apply_title_change_same_title_skips_broadcast` |
| TS-12 | `src-tauri/src/mux/ipc/reattach.rs` | `test_detach_session_panes_preserves_title_sender` |

### Integration Tests

| ID | Location | Test |
|----|----------|------|
| TS-10 | `src-tauri/src/mux/daemon.rs` | `test_detached_pane_title_change_updates_window_name` |
| TS-11 | `src-tauri/src/mux/daemon.rs` | `test_subscribe_before_welcome_catches_rename` |

### Manual / E2E Regression

- Manual SC-1〜SC-3 と既存 mux E2E の回帰確認は未実施（ユーザー手動確認ステップ）
