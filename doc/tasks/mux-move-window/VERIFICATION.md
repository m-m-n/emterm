# Verification Document: mux move-window

## Overview

**Feature**: mux-move-window
**SPEC.md**: `doc/tasks/mux-move-window/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-move-window/IMPLEMENTATION.md`

Docker を優先して検証すること。ホスト環境のテスト実行はユーザーが明示的に許可した場合のみ行うこと。

## Build Verification

### Rust backend

- Command (Docker):
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "cargo build --manifest-path src-tauri/Cargo.toml"
  ```
- Expected: exit code 0、警告が既存レベルを超えないこと

### TypeScript frontend

- Command (Docker):
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "bun run typecheck"
  ```
- Expected: 型エラー 0 件。`MuxAction` 網羅 switch が `move-window` を要求すること

### フル build (リリースビルドは Phase 8 以降任意)

- Command: `bun tauri build`（ユーザーが明示指示した場合のみホストで実行）
- Expected: Linux `.deb`/`.rpm`、Windows `nsis` がビルド成功すること

## Test Verification

### Rust unit tests

- Command (Docker):
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
  ```
- Coverage target: `MuxSession::move_window` と `MoveWindowMsg` 関連を 100% ライン、境界条件網羅

### TypeScript unit tests

- Command (Docker):
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "bun test"
  ```
- Coverage target: `prefix-key.test.ts` の新規アサート、`move-window-dialog.test.ts` の全分岐

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `[A,B,C,D]` で D を 1 に move | 並び順が `[D,A,B,C]` | Rust unit |
| TS-2 | `[A,B,C,D]` で A を 4 に move | 並び順が `[B,C,D,A]` | Rust unit |
| TS-3 | `[A,B,C,D]` で B を 3 に move | remove-then-insert で `[A,C,B,D]` | Rust unit |
| TS-4 | 現在位置と同じ番号で move | 並び順不変、`false` 返却 | Rust unit |
| TS-5 | `target_index >= len` で move | 末尾に clamp | Rust unit |
| TS-6 | 未知 window_id で move | `false` 返却、状態不変 | Rust unit |
| TS-7 | move 後も `active_window_id` 不変 | 同一 id が引き続き active | Rust unit |
| TS-8 | 単一ウィンドウで move | `false` 返却（current == target） | Rust unit |
| TS-9 | `MoveWindowMsg { target_index: 3 }` 往復 | bincode / APC 双方で復元できること | Rust unit |
| TS-10 | `from_u8(0x1A)` | `Some(MessageType::MoveWindow)` | Rust unit |
| TS-11 | `MuxMessageType.MoveWindow` (TS) | 値が `0x1a` | TS unit |
| TS-12 | prefix + m 押下 | `MuxAction{type:"move-window"}` dispatch | TS unit |
| TS-13 | ダイアログに有効整数 Enter（範囲内） | `{confirmed:true, value:N}` | TS unit (JSDOM) |
| TS-14 | ダイアログに非整数 Enter | `{confirmed:false}` | TS unit |
| TS-15 | ダイアログに範囲外数値 Enter（0, windowCount+1, 負値） | `{confirmed:false}` | TS unit |
| TS-16 | ダイアログで Esc | `{confirmed:false}` | TS unit |
| TS-17 | ダイアログで Cancel ボタン | `{confirmed:false}` | TS unit |
| TS-18 | IME composition 中 Enter | ダイアログは resolve しない | TS unit |
| TS-19 | ダイアログ close 後 | 直前フォーカスが復帰 | TS unit |
| TS-20 | 3 window で E2E prefix+m → 1 → Enter | 順序が期待通り変化 / `[N]` が `[1][2][3]` で再描画（先頭が元の active） | E2E (Docker) |
| TS-21 | 1 window 状態のタブ | `[1] title` が描画されていること | E2E (Docker) |
| TS-22 | `windows.length` を 3 に → Esc | 順序不変 | E2E (Docker) |
| TS-23 | 非数値入力 Enter | 順序不変、IPC 未送信 | E2E (Docker) |
| TS-24 | 範囲外 (999) 入力 | 順序不変 | E2E (Docker) |
| TS-25 | 現在位置と同じ番号 | 順序不変（action-handler で弾く） | E2E (Docker) |
| TS-26 | `[A,B,C]` で B(2) を 3 へ move 後 `[A,C,B]` が即座に `[1][2][3]` 番号で再描画 | 楽観更新により UI が即時反映 | TS unit + E2E |
| TS-27 | `reorderMuxWindows([A,B,C], 1, 2)` | `muxWindows`/`muxPaneIds` が `[A,C,B]`、active 追従 | TS unit |
| TS-28 | `reorderMuxWindows` with active == 移動対象 | 移動後の `activeMuxWindowIndex` が targetIndex を指すこと | TS unit |
| TS-29 | `reorderMuxWindows` with active ∉ 移動対象（from < active < to） | active 位置が `activeIndex - 1` に補正されること | TS unit |
| TS-30 | `reorderMuxWindows` with active ∉ 移動対象（to < active < from） | active 位置が `activeIndex + 1` に補正されること | TS unit |
| TS-31 | `active_window_id` 再選出: `add_window` 順 `[A(id=2), B(id=1)]` で A を active 後 A remove | new active が `window_order.first()` = B (id=1) | Rust unit |

## Code Quality Verification

### Rust format

- Command:
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml -- --check"
  ```
- Expected: 差分 0

### TypeScript typecheck

- Command:
  ```
  docker compose -f docker-compose.e2e.yml run --rm --no-deps build \
    sh -c "bun run typecheck"
  ```
- Expected: エラー 0

### Platform gates

- `libc`/`unsafe` を使う Unix 固有コードを追加していないこと（本機能では不要）
- 追加コードに `#[cfg(unix)]` / `#[cfg(windows)]` ゲートが必要なら適切に付与されていること

## File Structure Verification

### Files to Create

- `src/terminal-app/mux/move-window-dialog.ts` — モーダル本体
- `src/terminal-app/mux/move-window-dialog.test.ts` — ダイアログ単体テスト
- `e2e-tests/specs/mux-move-window.e2e.js` — E2E シナリオ

### Files to Modify

- `src-tauri/src/mux/session/session.rs` — `window_order`、`move_window`、`add_window`/`remove_window`/`active_window_id` 再選出変更
- `src-tauri/src/mux/session/manager.rs` — `session_list` 順序参照
- `src-tauri/src/mux/ipc/protocol.rs` — `MessageType::MoveWindow`、`MoveWindowMsg`、テスト上限拡張、`from_u8(0x1a).is_none()` 既存アサート削除
- `src-tauri/src/mux/ipc/handlers.rs` — `handle_move_window`
- `src-tauri/src/mux/ipc/connection.rs` — `route_message` GUI 分岐のみ
- `src/main.ts` — `onMuxStateChange` 分岐統合 (windowCount >= 1 で renderMuxSubTabs)
- `src/terminal/mux/prefix-key.ts` — `MuxAction`、`DEFAULT_ACTION_BINDINGS`
- `src/terminal/mux/prefix-key.test.ts` — テスト拡張
- `src/terminal/mux/mux-client.ts` — `MuxMessageType.MoveWindow`
- `src/terminal/mux/mux-client.test.ts` — `MoveWindow === 0x1a` アサート
- `src/terminal-app/mux/mux-action-handler.ts` — `case "move-window"` + `reorderMuxWindows` 呼び出し
- `src/terminal-app/mux/mux-window-manager.ts` — `reorderMuxWindows(ctx, from, to)` 関数追加
- `src/tab-bar/tab-bar-ui.ts` — `renderMuxSubTabs` (早期 return 削除、DOM 構造 2-span 化)
- `src/styles/tab-bar.css` — `.mux-window-number`
- `src/i18n/locales/en.json` — `mux.moveDialog.*`
- `src/i18n/locales/ja.json` — `mux.moveDialog.*`

## SPEC.md Compliance

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 `MuxAction + DEFAULT_ACTION_BINDINGS` に move-window 追加 | Phase 4 | TS-12 (bun test) |
| FR2 `move-window-dialog.ts` 実装（Enter/Esc/IME） | Phase 5 | TS-13〜TS-19 (bun test JSDOM) |
| FR3 `MoveWindow` IPC メッセージ（0x1A） | Phase 2 | TS-9〜TS-11 (cargo/bun test) |
| FR4 `MuxSession::move_window` insert/move + active 保全 | Phase 1, 3 | TS-1〜TS-8, TS-31 (cargo test) |
| FR5 `[N] title` タブ描画（単一時も表示、move 後 [N] 即時再描画） | Phase 7 + Phase 6 楽観更新 | TS-21, TS-20, TS-26 (E2E) + 手動目視 |
| FR6 i18n キー `mux.moveDialog.*`（en/ja） | Phase 5 | locales ファイル差分確認 + 手動目視 |
| FR7 無効入力で IPC 未送信・順序不変 | Phase 5, 6 | TS-14,15,23,24,25 |

**注**: 「現在位置と同じ番号 → 並び順不変」の判定は **dialog では行わない**（dialog は `{ confirmed: true, value: N }` を返す）。判定は `mux-action-handler.ts` 側で `value === currentIdx + 1` を比較して IPC 送信を抑止する。TS-13/TS-15 と TS-25 の責務分離に注意すること。

### Non-Functional Requirements Coverage

| Requirement | Verification |
|-------------|--------------|
| NFR1 Linux/Windows 両対応 | CI で両プラットフォームビルド通過（GitHub Actions）、新規コードに OS 固有 API 無し |
| NFR2 UI 一貫性 (`sftp-dialog-*` 踏襲) | 手動目視（rename との比較）、Phase 5 CSS 差分レビュー |
| NFR3 200ms 以内の並び替え | 楽観更新により即時反映（IPC 往復不要）。手動確認（`prefix+m` → Enter → タブ更新まで）、E2E で `waitUntil` 既定タイムアウト内に成功すること |
| NFR4 失敗時非破壊 | TS-6（未知 id）、TS-23〜TS-25（E2E 無効入力）で順序不変を検証。IPC 送信失敗時は UI はロールバックしない（daemon 状態は次回 attach の Welcome で整合）ことを実装で明示 |

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | `prefix + m` でモーダルが開く | E2E TS-20 / 手動 |
| SC-2 | 有効番号 Enter で順序が insert/move で変化 | E2E TS-20、unit TS-1〜TS-3 |
| SC-3 | 無効入力・Esc・Cancel で順序不変 | unit TS-14〜TS-17、E2E TS-22〜TS-25 |
| SC-4 | タブラベル先頭に `[N]` | E2E TS-21、手動目視 |
| SC-5 | Linux / Windows 双方で動作 | CI / 手動（Linux 実機＋Windows VM） |

## E2E Testing (Docker)

ref: docker-e2e-testing skill（`.claude/skills/` or project docs）

- Command (single spec): `./scripts/run-e2e-docker.sh test mux-move-window.e2e.js`
- Command (all): `./scripts/run-e2e-docker.sh test`

### Automatable scenarios

- [ ] TS-20: 3 window 作成 → `prefix + m` → `1` Enter → 順序が期待通り / `[1][2][3]` 番号が再描画
- [ ] TS-21: 1 window で `[1]` バッジ表示
- [ ] TS-22: Esc で順序不変
- [ ] TS-23: 非数値入力で順序不変
- [ ] TS-24: 範囲外 (999) で順序不変
- [ ] TS-25: 現在位置と同じ番号で順序不変
- [ ] TS-26: move 後 `[N]` 番号が楽観更新で即時再描画
- [ ] 既存 E2E（`mux-multi-session.e2e.js` 等）の非回帰

## Manual Testing (E2E Not Possible)

以下は主観判断・視覚的評価を要するため手動で確認すること。

- [ ] モーダルの見た目が既存 rename ダイアログとほぼ同一であること（位置・フォント・色）
- [ ] `[N]` バッジが読みやすく、tab タイトルより視覚的に従属的であること（`0.85em` 目安）
- [ ] 番号桁が変わっても横幅が揺れないこと（`tabular-nums` 効果）
- [ ] ダイアログに日本語 IME で入力できること（commit-Enter で誤確定しないこと）
- [ ] ダイアログ close 後に元の要素（タブ領域）へフォーカスが戻ること
- [ ] mux mode 完全終了（detach）で通常タブ表示に戻ること
- [ ] Windows 実機または VM で同等に動作すること（CI に加え実機 1 回）

## Performance Verification

- NFR3: `prefix + m` → Enter 確定 → タブバー更新完了まで 200ms 以下（体感）
- 手動計測：OS のウィンドウキャプチャや目視で違和感のない即応性を確認すること
- Rust 側の `move_window` は `Vec::remove` + `Vec::insert` (O(n)) のためウィンドウ 32 個程度までは余裕

## Security Verification

- [ ] 入力値は TS 層で整数範囲検証し、範囲内のみ IPC に載せる
- [ ] バックエンド `MuxSession::move_window` でも `target_index` を `[0, len-1]` にクランプ
- [ ] タブバッジは `textContent` 経由で挿入（`innerHTML` 使用なし、XSS なし）
- [ ] 新規 IPC メッセージ型は既存 APC / OSC 9999 トランスポートを再利用、新規トラスト境界なし

## Verification Summary

| Category | Items | Automated (unit) | E2E (Docker) | Manual |
|----------|-------|------------------|--------------|--------|
| FR1 move-window Action | 1 | 1 | 1 | 0 |
| FR2 Dialog | 7 | 7 | 1 | 2 |
| FR3 IPC Protocol | 3 | 3 | 0 | 0 |
| FR4 MuxSession::move_window + active_window_id 挙動変更 | 9 | 9 | 1 | 0 |
| FR5 Tab `[N]` rendering (単一 + move 後即時反映) | 3 | 0 | 3 | 2 |
| FR6 i18n keys | 4 | 0 | 0 | 1 |
| FR7 Invalid input → no IPC | 3 | 2 | 3 | 0 |
| Phase 6 reorderMuxWindows | 4 | 4 | 0 | 0 |
| NFR1 Linux/Windows | 1 | 0 | 0 | 1 |
| NFR2 UI consistency | 1 | 0 | 0 | 1 |
| NFR3 <200ms | 1 | 0 | 0 | 1 |
| NFR4 Non-destructive | 1 | 1 | 3 | 0 |
| **Totals** | **38** | **27** | **12** | **8** |

---

## Verification Results (実施記録)

実施日: 2026-04-24  
実施環境: Docker (`docker-compose.e2e.yml`)  
Git baseline: `1f9fb9b` (branch `main`)

### Build / Test summary

| Category | Command | Result |
|----------|---------|--------|
| Rust unit test (全体) | `cargo test --manifest-path src-tauri/Cargo.toml --lib` | **921 passed, 0 failed, 1 ignored** |
| Rust 新規テスト (mux) | `cargo test --lib mux::` | 219 passed (+10 `move_window`, +4 `window_order`, +1 `session_list_reflects_move_window_order`, +1 `session_list_matches_window_order`, +4 `MoveWindowMsg`, +1 `test_move_window_message_type`) |
| `cargo fmt --check` | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | diff 0 |
| `cargo clippy` (lib) | `cargo clippy --manifest-path src-tauri/Cargo.toml --lib` | 新規警告なし（既存警告のみ） |
| TypeScript unit test (全体) | `bun test` | **2284 pass, 0 fail, 17 todo** (Ran 2301 tests / 105 files) |
| `bun run typecheck` | `tsc --noEmit` | エラー 0 |
| E2E (Docker) | `./scripts/run-e2e-docker.sh test mux-move-window.e2e.js` | **7/7 passing (40.6s)** |

### Test Scenario Results (TS-1 〜 TS-31)

| ID | Result | Source |
|----|--------|--------|
| TS-1 `[A,B,C,D]` で D を 1 に | PASS | `session::tests::test_move_window_to_first` |
| TS-2 `[A,B,C,D]` で A を 4 に | PASS | `session::tests::test_move_window_to_last` |
| TS-3 `[A,B,C,D]` で B を 3 に → `[A,C,B,D]` | PASS | `session::tests::test_move_window_to_middle_forward` |
| TS-3 補: D を 2 に → `[A,D,B,C]` | PASS | `session::tests::test_move_window_to_middle_backward` |
| TS-4 同一位置 | PASS | `session::tests::test_move_window_same_position` |
| TS-5 範囲外 (999) clamp | PASS | `session::tests::test_move_window_out_of_range_clamps` |
| TS-6 未知 id | PASS | `session::tests::test_move_window_unknown_id` |
| TS-7 active 保全 | PASS | `session::tests::test_move_window_preserves_active` |
| TS-8 単一ウィンドウ | PASS | `session::tests::test_move_window_single_window_noop` |
| TS-9 `MoveWindowMsg` round-trip | PASS | `protocol::tests::test_move_window_msg_round_trip` / `..._via_mux_message` / `..._zero_index` |
| TS-10 `from_u8(0x1A)` | PASS | `protocol::tests::test_move_window_message_type`、`test_message_type_round_trip`、`test_apc_round_trip_all_message_types` |
| TS-11 `MuxMessageType.MoveWindow === 0x1a` | PASS | `mux-client.test.ts > MuxMessageType > MoveWindow has correct value` |
| TS-12 prefix + m dispatch | PASS | `prefix-key.test.ts > prefix + m dispatches move-window` / `all tmux-compatible bindings are present` |
| TS-13 有効整数 Enter | PASS | `move-window-dialog.test.ts > Enter with valid integer...` |
| TS-14 非整数 Enter | PASS | `move-window-dialog.test.ts > Enter with non-integer cancels` |
| TS-15 範囲外 (0 / 999 / 負値 / 小数) | PASS | `move-window-dialog.test.ts > Enter with value < 1 ... / > windowCount ... / Floating-point ... / Negative ...` |
| TS-16 Esc | PASS | `move-window-dialog.test.ts > Escape cancels` |
| TS-17 Cancel ボタン / Confirm ボタン (有効/無効) | PASS | `move-window-dialog.test.ts > Cancel button cancels` / `Confirm button with valid input confirms` / `Confirm button with invalid input cancels` |
| TS-18 IME composition 中 Enter | PASS | `move-window-dialog.test.ts > Enter during IME composition does not confirm` |
| TS-19 close 後フォーカス復帰 | PASS | `move-window-dialog.test.ts > previously focused element is restored after close` + `Closing removes the overlay from DOM` |
| TS-20 E2E prefix+m → 1 → Enter | PASS | `mux-move-window.e2e.js > E2E-1: prefix+m -> 1 -> Enter moves active to position 1` |
| TS-21 単一ウィンドウで `[1]` | PASS | `mux-move-window.e2e.js > E2E-6: single mux window is rendered with [1] badge` + `tab-bar-ui.test.ts > mux sub-tabs > renders [1] badge even for a single mux window` |
| TS-22 Esc で順序不変 | PASS | `mux-move-window.e2e.js > E2E-2: prefix+m -> Esc leaves order unchanged` |
| TS-23 非数値入力で順序不変 | PASS | `mux-move-window.e2e.js > E2E-4: prefix+m -> abc -> Enter cancels` |
| TS-24 範囲外 (999) で順序不変 | PASS | `mux-move-window.e2e.js > E2E-3: prefix+m -> 999 -> Enter cancels` |
| TS-25 同一位置で順序不変 | PASS | `mux-move-window.e2e.js > E2E-5: prefix+m -> same position cancels` |
| TS-26 楽観更新で `[N]` 即時再描画 | PASS | `tab-bar-ui.test.ts > mux sub-tabs > updates number badges when the window list is reordered` (E2E-1 assertion: `[1][2][3]` after move) |
| TS-27 reorderMuxWindows `[A,B,C]` 1→2 | PASS | `mux-window-manager.test.ts > reorderMuxWindows > [A,B,C] move B(1) to 2 => [A,C,B]` |
| TS-28 active == 移動対象 | PASS | `mux-window-manager.test.ts > active follows its own move` / `active is A(0), move A(0) -> 2, active follows to 2` |
| TS-29 from < active < to | PASS | `mux-window-manager.test.ts > active is C (2), move A(0) -> 2, active shifts to 1` |
| TS-30 to < active < from | PASS | `mux-window-manager.test.ts > active is B (1), move C(2) -> 0, active shifts to 2` |
| TS-31 `window_order` 順の active 再選出 | PASS | `session::tests::test_active_window_id_after_remove_uses_order` |

### Additional tests added (not in the original plan)

- Rust
  - `session::tests::test_window_order_after_adds`
  - `session::tests::test_window_order_after_removes`
  - `session::tests::test_active_window_id_none_after_all_removed`
  - `session::tests::test_move_window_windows_btreemap_unchanged`
  - `manager::tests::test_session_list_matches_window_order`
  - `manager::tests::test_session_list_reflects_move_window_order`
  - `protocol::tests::test_move_window_msg_zero_index`
- TypeScript
  - `move-window-dialog.test.ts > Whitespace-only input is treated as empty and cancels`
  - `move-window-dialog.test.ts > Boundary values are accepted (1 and windowCount)`
  - `tab-bar-ui.test.ts > mux sub-tabs > renders sequential [1] [2] [3] badges for multiple windows`

### Known limitations / follow-ups

- 既存の `mux-multi-session.e2e.js` は旧セレクタ (`.mux-sub-tabs` / `[data-testid="terminal"]`) を使っており、本機能とは無関係に未整備。今回は対象外。
- NFR1 の Windows 実機確認は CI 側（GitHub Actions `windows-latest`）での通過を根拠とする。手動実機確認は未実施。
- 本機能では Daemon → GUI の順序 broadcast を追加していない（論点 D 確定事項）。attach 中に外部から `MuxSession::move_window` が呼ばれても GUI に通知されない。次回 attach の `Welcome` で整合する前提は維持。

（E2E と Automated は一部同一シナリオを二重にカウントしている。Manual は主に UI 目視と多言語確認。）
