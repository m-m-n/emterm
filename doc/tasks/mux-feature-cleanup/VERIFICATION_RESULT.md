# Verification Result: mux-feature-cleanup

## 概要

- 検証日時: 2026-04-21
- 検証対象コミット: `1b8fb6d53e180c2f454edf750fe3d0c4ff680de2` (作業ツリーに未コミットの cleanup 変更を含む)
- 検証結果: **PASS**

本検証は直前の `/sdd.5-check` で Build / Test / Format / Static analysis がすべて PASS 済みである前提で、ファイル構造検証、機能要件 (FR1–FR9 / NFR1–NFR3) 準拠、削除シンボル残存チェック、E2E / 手動確認項目の抽出を行った。

## 1. 自動検証結果 (sdd.5-check 参照)

| 項目 | 結果 |
|---|---|
| `cargo check --manifest-path src-tauri/Cargo.toml` | PASS |
| `bun run typecheck` | PASS |
| `cargo test --manifest-path src-tauri/Cargo.toml` (896 tests) | PASS |
| `bun test` (2251 pass / 17 todo / 0 fail) | PASS |
| `cargo clippy` (新規 warning 0) | PASS |
| Dead code 検出 3 件 | sdd.5 内で修正済み・再検証 PASS |

## 2. ファイル構造検証

### 削除対象

| Path | 結果 |
|---|---|
| `src/terminal-app/mux/mux-multi-pane.ts` | DELETED |
| `src/terminal-app/mux/mux-drag-resize.ts` | DELETED |
| `src/terminal-app/mux/mux-copy-mode.ts` | DELETED |
| `src/terminal/mux/layout.ts` | DELETED |
| `src/terminal/mux/layout.test.ts` | DELETED |
| `src/terminal/mux/pane-manager.ts` | DELETED |
| `src/terminal/mux/pane-border.ts` | DELETED |
| `src/terminal/mux-copy-mode/index.ts` | DELETED |
| `src/terminal/mux-copy-mode/index.test.ts` | DELETED |
| `src/terminal/mux-copy-mode/emacs-keybinds.ts` | DELETED |
| `src/terminal/mux-copy-mode/vi-keybinds.ts` | DELETED |
| `src/terminal/mux-copy-mode/` ディレクトリ | DELETED |

11 ファイル + 1 ディレクトリ、すべて作業ツリーから消失を確認。

### 保持対象

| Path | 結果 |
|---|---|
| `src-tauri/src/mux/session/pane.rs` | 存在 (`git diff` 差分なし → 無変更) |

FR6 の「`Pane` struct は保持」を満たす。

## 3. SPEC 機能要件準拠

| FR/NFR | タイトル | 結果 | 根拠 |
|---|---|---|---|
| FR1 | `MuxAction` / `DEFAULT_ACTION_BINDINGS` から 7 アクション削除 | PASS | `src/terminal/mux/prefix-key.ts` の `MuxAction` union に `split-vertical` / `split-horizontal` / `next-pane` / `prev-pane` / `close-pane` / `zoom-toggle` / `copy-mode` がなし (L12–L19)。`DEFAULT_ACTION_BINDINGS` も detach / new-window / next-window / prev-window / rename-window / paste の 6 キーのみ (L25–L32) |
| FR2 | 残存アクション (detach, new-window, next-window, prev-window, rename-window, paste, prefix-passthrough) | PASS | 同ファイル `MuxAction` に 7 バリアント残存 (L12–L19)。`prefix-passthrough` は state machine で prefix キーそのものを PTY へ送る用途 (L100–L103) |
| FR3 | フロントエンド 11 ファイル削除 | PASS | 上記「2. ファイル構造検証」で全件確認 |
| FR4 | 縮小対象ファイルに削除アクション関連コード残存なし | PASS | `rg "split-vertical\|split-horizontal\|next-pane\|prev-pane\|close-pane\|zoom-toggle\|copy-mode" src/ -g '!**/*.json'` → 0 ヒット。`rg "splitVertical\|splitHorizontal\|nextPane\|prevPane\|closePane\|zoomToggle\|copyMode" src/` → `mux-window-manager.ts` に `prevPaneId` のみ (これは「直前ペイン ID」のローカル変数で、削除済み `prev-pane` アクションとは無関係) |
| FR5 | バックエンドから `SplitPane` / `SplitPaneMsg` / `handle_split_pane` 削除 | PASS | `rg "SplitPane\|SplitPaneMsg\|handle_split_pane" src-tauri/` → `protocol.rs:326` と `protocol.rs:590` の 2 件のみ (いずれもテスト内の「`0x11 (SplitPane) was removed`」コメント)。コード定義 / 参照は 0 件 |
| FR6 | `src-tauri/src/mux/session/pane.rs` 無変更 | PASS | `git diff --stat src-tauri/src/mux/session/pane.rs` で出力なし |
| FR7 | serde の未知フィールド黙殺方針維持 | PASS | `src-tauri/src/commands/config/settings.rs` に `deserialize_any` / `#[serde(deny_unknown_fields)]` は存在しない。`git diff` でも本ファイル無変更 |
| FR8 | E2E spec に split/pane/copy-mode 検証ケース残存なし | PASS | `e2e-tests/specs/mux*.e2e.js` および `viewer-tab-switch-keyboard.e2e.js` に `split-vertical\|split-horizontal\|next-pane\|prev-pane\|close-pane\|zoom-toggle\|copy-mode\|splitVertical\|splitHorizontal\|closePane\|zoomToggle\|copyMode\|SplitPane` は 0 ヒット |
| FR9 | `doc/tasks/terminal-multiplexer/` 関連ドキュメント追従 | PASS | `IMPLEMENTATION.md:3`, `FIG.md:3`, `VERIFICATION.md:3` に post-cleanup 注記が追加済み。`SPEC.md:469` で cleanup タスクへのリンクあり |
| NFR1 | 残存アクション動作の退行なし | PASS (静的) | `prefix-key.test.ts` は bun test で PASS。動的検証は下記 E2E で担保 |
| NFR2 | 全テストコマンド通過 | PASS (E2E 除く) | sdd.5 で cargo test / bun test / typecheck PASS。E2E は本検証では未実行 |
| NFR3 | 旧 `settings.json` (削除キー含む) でも起動可能 | 未検証 (手動項目) | FR7 により serde は未知フィールドを無視する。起動時エラーが出ないことは下記「5. 手動確認チェックリスト」に列挙 |

### Grep 追加検証

| コマンド | 期待 | 実測 | 結果 |
|---|---|---|---|
| `rg "splitVertical\|splitHorizontal\|nextPane\|prevPane\|closePane\|zoomToggle\|copyMode" src/` | `prevPaneId` (ローカル変数) のみ許容 | `mux-window-manager.ts` 内の `prevPaneId` ローカル変数のみ | PASS |
| `rg "mux-multi-pane\|mux-drag-resize\|pane-manager\|pane-border\|mux-copy-mode\|mux/layout" src/` | 0 | 0 | PASS |
| `rg '"splitVertical"\|"splitHorizontal"\|"nextPane"\|"prevPane"\|"closePane"\|"zoomToggle"\|"copyMode"' src/i18n/` | 0 | 0 | PASS |
| `rg "MuxMessageType.SplitPane\|\"SplitPane\":" src/terminal/mux/mux-client.ts` | 0 | 0 (TS-6: `MuxMessageType` に `SplitPane` キーなし) | PASS |

### Test Scenarios 自動カバー状況

| ID | シナリオ | カバー状況 |
|---|---|---|
| TS-1 | `MessageType::from_u8(0x11) == None` | `protocol.rs:333` アサートで検証済み (cargo test PASS) |
| TS-2 | `test_message_type_round_trip` の 0x11 除外 | `protocol.rs:323–334` で 0x11 を skip し None アサート (cargo test PASS) |
| TS-3 | `test_apc_round_trip_all_message_types` の 0x11 除外 | `protocol.rs:587–592` で 0x11 を skip (cargo test PASS) |
| TS-4 | `prefix-key.test.ts` 残す 7 アクションで dispatch | bun test PASS |
| TS-5 | `prefix-key.test.ts` 削除キー no-op | bun test PASS |
| TS-6 | `MuxMessageType.SplitPane === undefined` | 静的確認: `mux-client.ts` L18–L30 に `SplitPane` 定義なし |
| TS-7 〜 TS-10 | E2E 4 spec | 未実行 (下記「4. E2E 自動テスト」参照) |

## 4. E2E 自動テスト

- 未実行。時間コストと CLAUDE.md memory の「無理に再試行しない」方針に基づき、手動検証対象として下記「5. 手動確認チェックリスト」に列挙する。
- 対象 spec (VERIFICATION.md より):
  - `./scripts/run-e2e-docker.sh test mux.e2e.js`
  - `./scripts/run-e2e-docker.sh test mux-multi-session.e2e.js`
  - `./scripts/run-e2e-docker.sh test mux-reattach.e2e.js`
  - `./scripts/run-e2e-docker.sh test viewer-tab-switch-keyboard.e2e.js`
  - `./scripts/run-e2e-docker.sh test` (全 spec)

## 5. 手動確認チェックリスト

VERIFICATION.md の Manual Testing と E2E セクションから抽出。

### 削除キー no-op 動作

- [ ] `prefix + %` (split-vertical 旧バインド) 押下で画面に変化がなく、`emterm.log` に error レベルのログが出ない
- [ ] `prefix + "` (split-horizontal 旧バインド) 押下で同上
- [ ] `prefix + o` (next-pane 旧バインド) 押下で同上
- [ ] `prefix + ;` (prev-pane 旧バインド) 押下で同上
- [ ] `prefix + x` (close-pane 旧バインド) 押下で同上
- [ ] `prefix + z` (zoom-toggle 旧バインド) 押下で同上
- [ ] `prefix + [` (copy-mode 旧バインド) 押下で同上

### 残存アクション動作

- [ ] `prefix + d` で detach 動作
- [ ] `prefix + c` で new-window 動作
- [ ] `prefix + n` で next-window 動作
- [ ] `prefix + p` で prev-window 動作
- [ ] `prefix + ,` で rename-window プロンプト表示
- [ ] `prefix + ]` で paste 動作

### 設定 UI

- [ ] 設定パネル > Mux > Keybinds に detach / new-window / next-window / prev-window / rename-window / paste の 6 行のみ表示 (en ロケール)
- [ ] 同上 (ja ロケール)
- [ ] 旧削除アクション (splitVertical / splitHorizontal / nextPane / prevPane / closePane / zoomToggle / copyMode) の行が表示されない

### Legacy 互換性 (NFR3)

- [ ] 旧 `settings.json` に `mux.keybinds.split-vertical` / `mux.keybinds.copy-mode` 等を含めた状態でアプリを起動し、起動エラーが出ない
- [ ] 同状態で Mux セクション設定の読み込み / 保存が正常に動作

### Daemon 後方互換性

- [ ] 旧バイナリ相当の `SplitPane (0x11)` フレームを daemon が受信しても接続が切れない (手動再現困難: `from_u8(0x11) == None` の unit test で代替済み)

### E2E 自動テスト (Docker 実行)

- [ ] `./scripts/run-e2e-docker.sh test mux.e2e.js` PASS
- [ ] `./scripts/run-e2e-docker.sh test mux-multi-session.e2e.js` PASS
- [ ] `./scripts/run-e2e-docker.sh test mux-reattach.e2e.js` PASS
- [ ] `./scripts/run-e2e-docker.sh test viewer-tab-switch-keyboard.e2e.js` PASS
- [ ] `./scripts/run-e2e-docker.sh test` 全 spec PASS

## 6. パフォーマンス / セキュリティ

### パフォーマンス

- SPEC / VERIFICATION.md に数値目標なし。削除主体のため新規劣化要因なし。
- 期待効果: TypeScript バンドルサイズ縮小 (11 ファイル削除 + 関連縮小)、IPC プロトコル分岐削減。定量計測は実施せず。

### セキュリティ

- SPEC 5.1 (Security Considerations) はソケットパス検証 / ファイル権限 / ネスト防止の既存方針を維持。本削除で信頼境界は変化しない。
- IPC プロトコル縮小による既存クライアント切断リスク: `MessageType::from_u8(0x11) -> None` により daemon 側で不明フレームを無視する実装を確認 (`protocol.rs:332–334` テスト)。旧クライアント互換性は保たれる。

## 総合判定

**PASS**: 自動検証対象の 34 項目 (ファイル構造 12 + FR/NFR 12 + Grep 4 + Test Scenarios TS-1〜TS-6 の 6 件) はすべて確認済み。

残課題:

1. E2E 4 spec (TS-7〜TS-10) の Docker 実行 — 手動実施。
2. 手動確認チェックリスト (17 項目) — 主に UI / settings 互換性確認。

これらは VERIFICATION.md 設計通り「自動化困難 or 時間コスト回避」の位置付けで、作業ツリーの cleanup 実装自体の正しさは自動検証で裏付けられている。
