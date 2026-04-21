# Verification Document: Mux Feature Cleanup

## Overview

**Feature**: mux-feature-cleanup
**SPEC.md**: `doc/tasks/mux-feature-cleanup/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-feature-cleanup/IMPLEMENTATION.md`

動作していない mux 機能 (ペイン分割・移動・閉じ・ズーム・コピーモード) を削除し、残す機能だけで構成される状態に整理する削除タスク。検証は「削除が完了していること」と「残す機能が退行していないこと」の両面で行う。

## Build Verification

| Target | Command | Expected |
|---|---|---|
| Rust 型検証 | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo check --manifest-path src-tauri/Cargo.toml"` | exit 0, warnings なし |
| TS 型検証 | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"` | exit 0, 型エラーなし |

## Test Verification

| Target | Command | Expected |
|---|---|---|
| Rust unit test | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"` | 全テスト pass |
| TS unit test | `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"` | 全テスト pass |
| E2E | `./scripts/run-e2e-docker.sh test` | 削除後の構成で全 pass |

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|---|---|---|---|
| TS-1 | `MessageType::from_u8(0x11)` | `None` を返す | Unit (Rust) |
| TS-2 | `test_message_type_round_trip` の 0x11 除外 | 残存値 (0x01-0x10, 0x12-0x19) で round-trip 成功、0x11 は `None` | Unit (Rust) |
| TS-3 | `test_apc_round_trip_all_message_types` の 0x11 除外 | 残存 message type で APC round-trip 成功 | Unit (Rust) |
| TS-4 | `prefix-key.test.ts`: 残す 7 アクションで dispatch | 対応する `MuxAction.type` が発火 | Unit (TS) |
| TS-5 | `prefix-key.test.ts`: 削除キー (`%`, `"`, `o`, `;`, `x`, `z`, `[`) で no-op | `consumed=true`, `actions.length === 0`, state=idle | Unit (TS) |
| TS-6 | `mux-client.test.ts`: `MuxMessageType` に `SplitPane` なし | `(MuxMessageType as any).SplitPane === undefined` | Unit (TS) |
| TS-7 | `mux.e2e.js`: 残存 mux 機能 (detach/new-window/next-window/prev-window) | 既存テスト pass | E2E |
| TS-8 | `mux-multi-session.e2e.js`: マルチタブでの mux 動作 | 既存テスト pass | E2E |
| TS-9 | `mux-reattach.e2e.js`: detach/reattach | 既存テスト pass | E2E |
| TS-10 | `viewer-tab-switch-keyboard.e2e.js`: ウィンドウ切替 | 既存テスト pass | E2E |

## Code Quality Verification

- Format: プロジェクト標準 (`.editorconfig` / prettier 等。CI での format 自動整形に委ねる)
- 静的解析: `bun run typecheck`, `cargo check` が警告ゼロで通る

## File Structure Verification

### Files to Delete (11 ファイル + 1 ディレクトリ)

| File | 削除確認コマンド |
|---|---|
| `src/terminal-app/mux/mux-multi-pane.ts` | `test ! -f src/terminal-app/mux/mux-multi-pane.ts` |
| `src/terminal-app/mux/mux-drag-resize.ts` | `test ! -f src/terminal-app/mux/mux-drag-resize.ts` |
| `src/terminal-app/mux/mux-copy-mode.ts` | `test ! -f src/terminal-app/mux/mux-copy-mode.ts` |
| `src/terminal/mux/layout.ts` | `test ! -f src/terminal/mux/layout.ts` |
| `src/terminal/mux/layout.test.ts` | `test ! -f src/terminal/mux/layout.test.ts` |
| `src/terminal/mux/pane-manager.ts` | `test ! -f src/terminal/mux/pane-manager.ts` |
| `src/terminal/mux/pane-border.ts` | `test ! -f src/terminal/mux/pane-border.ts` |
| `src/terminal/mux-copy-mode/index.ts` | `test ! -f src/terminal/mux-copy-mode/index.ts` |
| `src/terminal/mux-copy-mode/index.test.ts` | `test ! -f src/terminal/mux-copy-mode/index.test.ts` |
| `src/terminal/mux-copy-mode/emacs-keybinds.ts` | `test ! -f src/terminal/mux-copy-mode/emacs-keybinds.ts` |
| `src/terminal/mux-copy-mode/vi-keybinds.ts` | `test ! -f src/terminal/mux-copy-mode/vi-keybinds.ts` |
| `src/terminal/mux-copy-mode/` (空ディレクトリ) | `test ! -d src/terminal/mux-copy-mode` |

### Files to Modify

| File | 変更内容 |
|---|---|
| `src/terminal/mux/prefix-key.ts` | `MuxAction` から 7 バリアント削除、`DEFAULT_ACTION_BINDINGS` から 7 キー削除 |
| `src/terminal/mux/prefix-key.test.ts` | 削除アクション関連テスト除去、bindings 配列縮小 |
| `src/terminal/mux/mux-client.ts` | `MuxMessageType.SplitPane` 削除 |
| `src/terminal/mux/mux-client.test.ts` | `SplitPane` 非存在の assertion 追加 (任意) |
| `src/terminal/mux/index.ts` | `MuxPaneManager` export 削除 |
| `src/terminal-app/mux/mux-action-handler.ts` | 削除 case と context フィールド除去 |
| `src/terminal-app/mux/mux-session.ts` | copy-mode / multi-pane 関連 context フィールドと処理削除 |
| `src/terminal-app/mux/mux-window-manager.ts` | split pane / layoutRoot 関連 context と処理削除 |
| `src/terminal-app/index.ts` | 不要 import / フィールド / メソッド削除 |
| `src/terminal-app/handlers/keyboard.ts` | `onCopyModeKey` コールバック削除 |
| `src/settings/sections/mux-section.ts` | `ACTION_I18N_KEYS` から 7 行削除 |
| `src/i18n/locales/en.json` | `settings.mux.keybind.splitVertical`/`splitHorizontal`/`nextPane`/`prevPane`/`closePane`/`zoomToggle`/`copyMode` の 7 キー削除 |
| `src/i18n/locales/ja.json` | 同上 7 キー削除 |
| `src/styles.css` | 未使用 `.mux-pane-border*`, `.copy-mode-indicator` 削除 |
| `src-tauri/src/mux/ipc/protocol.rs` | `SplitPane = 0x11` / `from_u8` 0x11 分岐 / `SplitPaneMsg` / 関連テスト範囲調整 |
| `src-tauri/src/mux/ipc/handlers.rs` | `handle_split_pane` 関数削除 |
| `src-tauri/src/mux/ipc/connection.rs` | `SplitPane` dispatch アーム削除、import 整理 |

## SPEC.md Compliance

### Functional Requirements Coverage

| FR | Title | Phase | Verification |
|---|---|---|---|
| FR1 | Actions removed from `MuxAction` / `DEFAULT_ACTION_BINDINGS` | Phase 1 | grep で `split-vertical\|split-horizontal\|next-pane\|prev-pane\|close-pane\|zoom-toggle\|copy-mode` が `src/terminal/mux/prefix-key.ts` に存在しない |
| FR2 | Actions retained (`detach`, `new-window`, `next-window`, `prev-window`, `rename-window`, `paste`, `prefix-passthrough`) | Phase 1 | `prefix-key.test.ts` が 7 アクションで通る |
| FR3 | Frontend files deleted (11 ファイル) | Phase 2 | 各ファイルが作業ツリーから消えている |
| FR4 | Frontend files reduced | Phase 1, 3 | `bun run typecheck` 通過、残アクションのみ検証するテストが通る |
| FR5 | Backend protocol (`SplitPane` / `SplitPaneMsg` / `handle_split_pane`) removed | Phase 4 | `rg "SplitPane\|SplitPaneMsg\|handle_split_pane" src-tauri/` でヒット 0 |
| FR6 | Backend `Pane` struct kept | Phase 4 | `src-tauri/src/mux/session/pane.rs` の `Pane` struct が変更されていない (git diff で確認) |
| FR7 | Settings migration policy (serde default drop) | Phase 1 | 実装上の変更なし。旧キー含む `settings.json` で起動できることを NFR3 で検証 |
| FR8 | E2E test policy | Phase 5 | 4 つの spec を grep で確認、該当ケースがあれば削除、E2E 通過 |
| FR9 | `doc/tasks/terminal-multiplexer/SPEC.md` updated | /sdd.1 で完了済み (本タスクでは触らない) + Phase 6 で関連ドキュメント追従 | `doc/tasks/terminal-multiplexer/` 配下で削除シンボルの残存がない (VERIFICATION_RESULT.md の履歴を除く) |

### Non-Functional Requirements Coverage

| NFR | Title | Verification |
|---|---|---|
| NFR1 | Retained action behavior preservation | E2E 4 spec が通る (変更なし想定) |
| NFR2 | All test commands pass | `cargo test`, `bun test`, `bun run typecheck`, `./scripts/run-e2e-docker.sh test` 全通過 |
| NFR3 | Legacy settings.json loads without error | 旧 `mux.keybinds.split-vertical` 等を含む settings.json で起動し、エラー log が出ない手動確認 |

### Success Criteria

| ID | Criterion | How to Verify |
|---|---|---|
| SC-1 | 全 FR が実装されている | 上記 FR Coverage テーブルの全項目が満たされる |
| SC-2 | 削除ファイルが作業ツリーにない | `test ! -f <path>` を 11 ファイル分確認 |
| SC-3 | 縮小ファイルがコンパイル & テスト通過 | typecheck / cargo test / bun test 通過 |
| SC-4 | `cargo test` 通過 | docker コマンド exit 0 |
| SC-5 | `bun test` 通過 | docker コマンド exit 0 |
| SC-6 | `bun run typecheck` 通過 | docker コマンド exit 0 |
| SC-7 | `./scripts/run-e2e-docker.sh test` 通過 | exit 0 |
| SC-8 | 設定パネル Mux セクションに残存 7 項目のみ表示 | 手動確認 (GUI 起動して Settings > Mux > Keybinds を開く) |
| SC-9 | `doc/tasks/terminal-multiplexer/SPEC.md` が現状反映 | /sdd.1 で完了済み、grep で確認 |

## Grep Verification (削除シンボルの残存確認)

各コマンドの期待値は「記載された無視パスを除き 0 ヒット」。

### フロントエンド削除アクション名

```
rg "split-vertical|split-horizontal|next-pane|prev-pane|close-pane|zoom-toggle|copy-mode" src/ -g '!**/*.json'
```

期待ヒット:

- `src-tauri/src/mux/tmux_conf/converter.rs` (本タスクのスコープ外。converter は出力キーが `DEFAULT_ACTION_BINDINGS` に存在しない場合に無視される前提)
- `doc/` 配下 (履歴/ドキュメント。Phase 6 で整理)

### フロントエンド `MuxAction` バリアント名を含むコードシンボル

```
rg "splitVertical|splitHorizontal|nextPane|prevPane|closePane|zoomToggle|copyMode" src/
```

期待: `src/` 配下で 0 ヒット (`i18n/locales/` も含めて 0)

### バックエンド `SplitPane` 関連

```
rg "SplitPane|SplitPaneMsg|handle_split_pane" src-tauri/
```

期待: コード内 0 ヒット (テスト内で `from_u8(0x11) == None` のリテラル 0x11 は OK)

### 削除 module への import

```
rg "mux-multi-pane|mux-drag-resize|pane-manager|pane-border|mux-copy-mode|mux/layout" src/
```

期待: 0 ヒット

### i18n ロケールキー

```
rg '"splitVertical"|"splitHorizontal"|"nextPane"|"prevPane"|"closePane"|"zoomToggle"|"copyMode"' src/i18n/
```

期待: 0 ヒット

### terminal-multiplexer ドキュメント (Phase 6 対象)

```
rg "SplitPane|split-vertical|split-horizontal|zoom-toggle|copy-mode|layout\.ts|pane-manager|mux-copy-mode|Pane Layout|Copy Mode" doc/tasks/terminal-multiplexer/
```

期待: `VERIFICATION_RESULT.md` (実行履歴) を除き意図的な残存 0 ヒット

## E2E Testing (Docker)

ref: docker-e2e-testing skill

- [ ] `./scripts/run-e2e-docker.sh test mux.e2e.js` — mux 基本フロー (entry / new-window / next-window / prev-window / detach / 最終ウィンドウ閉じで mux 終了)
- [ ] `./scripts/run-e2e-docker.sh test mux-multi-session.e2e.js` — マルチタブ mux
- [ ] `./scripts/run-e2e-docker.sh test mux-reattach.e2e.js` — detach/reattach
- [ ] `./scripts/run-e2e-docker.sh test viewer-tab-switch-keyboard.e2e.js` — ウィンドウ切替キーボード
- [ ] `./scripts/run-e2e-docker.sh test` — 全 spec 通過

## Manual Testing (E2E Not Possible)

以下は自動化困難なため手動確認。

- [ ] 設定パネルを開き Mux セクション > Keybinds に 7 行 (detach, new-window, next-window, prev-window, rename-window, paste) のみ表示されること (en / ja ロケール両方)
- [ ] `prefix + %` / `prefix + "` / `prefix + o` / `prefix + ;` / `prefix + x` / `prefix + z` / `prefix + [` が no-op であること (画面に変化がなく、emterm.log に error レベルのログが出ないこと)
- [ ] 既存ユーザーの `settings.json` に `mux.keybinds.split-vertical` 等を含めた状態でアプリを起動し、起動エラーが出ないこと (NFR3)
- [ ] 旧バイナリが生成する `SplitPane (0x11)` フレームを daemon が受信した場合、warn log (`MessageType::from_u8(0x11) returned None`) が出つつ接続が切れないこと (手動では再現困難なため、`from_u8(0x11) == None` を保証する unit test で代替)

## Performance Verification

該当なし。削除のみで性能要件は変わらない。

## Security Verification

該当なし。削除のみで信頼境界は変わらない。SPEC 5.1 (Security Considerations) の通り、ソケットパス検証・ファイル権限・ネスト防止は既存仕様を維持。

## Error Handling

| Scenario | 期待 Behavior | 検証方法 |
|---|---|---|
| Legacy client が `SplitPane (0x11)` 送信 | daemon が `MessageType::from_u8` → `None` を取得、warn log を出しフレーム破棄、接続継続 | Rust unit test で `from_u8(0x11) == None` 検証 |
| `settings.json` に削除 `mux.keybinds.*` キーが含まれる | serde が未知フィールドを破棄し設定ロード成功 | 手動で旧キー入り settings.json を用意して起動 |
| `prefix + <削除キー>` 押下 | prefix handler が consume し idle に戻る、IPC/UI 変化なし | `prefix-key.test.ts` で検証、手動でも確認 |

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|---|---|---|---|---|
| Build | 2 | 2 | 0 | 0 |
| Test | 3 | 3 | 1 | 0 |
| Test Scenarios (TS-1〜10) | 10 | 6 | 4 | 0 |
| Functional Requirements (FR1-9) | 9 | 8 | 1 | 0 |
| Non-Functional Requirements (NFR1-3) | 3 | 2 | 0 | 1 |
| Success Criteria (SC-1〜9) | 9 | 7 | 1 | 1 |
| Grep verification | 6 | 6 | 0 | 0 |
| E2E specs | 4 | 0 | 4 | 0 |
| Manual check | 4 | 0 | 0 | 4 |

**合計**: 自動化検証 34 項目、E2E 11 項目、手動確認 6 項目
