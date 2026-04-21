# Implementation Plan: Mux Feature Cleanup

## Overview

動作していない mux 機能 (ペイン分割・ペイン移動・ペイン閉じ・ズーム・コピーモード) を削除し、残す機能 (デタッチ・ウィンドウ管理・貼り付け) だけで構成される状態に整理する。新規機能実装はなく、削除・縮小が主体。

## Objectives

- `MuxAction` から 7 アクション (`split-vertical` / `split-horizontal` / `next-pane` / `prev-pane` / `close-pane` / `zoom-toggle` / `copy-mode`) を削除
- フロントエンドのペインレイアウト・ドラッグリサイズ・コピーモード関連モジュールを全削除
- バックエンド IPC から `SplitPane (0x11)` / `SplitPaneMsg` / `handle_split_pane` を削除
- 設定 UI と i18n ロケールから削除アクションの項目を除去
- E2E スペックから該当ケースを削除
- `doc/tasks/terminal-multiplexer/` 配下の関連ドキュメントに残った旧参照を更新

## Prerequisites

### Development Environment

- Docker + Docker Compose (テスト/ビルド実行)
- bun (パッケージ管理)

### Dependencies

- 既存の mux 機能実装 (`doc/tasks/terminal-multiplexer/`) が存在している必要がある
- SPEC: `doc/tasks/mux-feature-cleanup/SPEC.md` (本実装の根拠)

## Architecture Overview

### Technology Stack

- **Language**: Rust (src-tauri, WASM) + TypeScript (src)
- **Framework**: Tauri + Bun
- **Test**: `cargo test`, `bun test`, WebdriverIO (E2E)

### Design Approach

- **削除の論理的依存順** に従ってフェーズを分割する (型定義 → 型を参照するコード → 参照元の整理 → テスト/ドキュメント追従)
- 各フェーズ終了時点で `cargo check` / `bun run typecheck` が通る状態を維持する
- バックエンド `Pane` 構造体は残し、ウィンドウあたり 1 ペインを前提とするコードパスのみ残す
- 設定マイグレーションは serde のデフォルト挙動 (未知フィールドを捨てる) に任せる

### Component Interaction

削除後の mux 機能フロー (変更なし):

1. GUI: prefix キー入力 → `PrefixKeyHandler` → `MuxAction` → `handleMuxAction`
2. `handleMuxAction` → `MuxClient.sendControl` → APC/OSC → bridge → daemon
3. daemon IPC ハンドラ (`handle_create_window` / `handle_switch_window` / `handle_rename_window` / `handle_destroy_pane` / `handle_attach` / `handle_detach` / `handle_resize` / `handle_request_pane_snapshot`) のみ

## Implementation Phases

### Phase 1: フロントエンド MuxAction 型と handler 縮小

**Goal**: `MuxAction` 型を 7 アクション削除、それを参照する TS コードを追従させて `bun run typecheck` が通る状態にする。

**Files to Modify**:

- `src/terminal/mux/prefix-key.ts` — `MuxAction` ユニオンから 7 バリアントを削除、`DEFAULT_ACTION_BINDINGS` から対応キー (`%`, `"`, `o`, `;`, `x`, `z`, `[`) を削除
- `src/terminal-app/mux/mux-action-handler.ts` — `MuxActionContext` から未使用化するフィールド (`getMuxLayoutRoot`, `setActiveMuxPane`, `toggleMuxZoom`, `enterCopyMode`, `setMuxPendingSplitCount`, `setMuxPendingSplitDirection`, `getMuxPendingSplitCount`) を削除、`switch` から 7 アクションの `case` 節を削除、`getActiveMuxPaneId` の実装は残すか呼び出し元に合わせて整理
- `src/terminal-app/mux/mux-session.ts` — `MuxSessionContext` から copy-mode 関連フィールド (`getCopyModeManager`, `setCopyModeManager`, `getCopyModeKeybinds`, `setCopyModeKeybinds`) とレイアウト関連 (`getMuxLayoutRoot`, `getMuxPaneCanvases`, `getMuxPendingSplitCount`, `setMuxPendingSplitCount`, `exitMultiPaneMode`) を削除、`enterMuxMode` / `exitMuxMode` 内で該当フィールドを参照するコード (copy-mode 終了処理、`muxLayoutRoot` チェック、`exitMultiPaneMode` 呼び出し、`setMuxPendingSplitCount(0)`) を削除、`setOnPtyOutput` 内のマルチペイン分岐を削除
- `src/terminal-app/mux/mux-window-manager.ts` — `MuxWindowManagerContext` からレイアウト/スプリット関連フィールド (`getMuxPendingSplitCount`, `setMuxPendingSplitCount`, `getMuxPendingSplitDirection`, `getMuxLayoutRoot`, `getMuxPaneCanvases`, `handleMuxSplitPaneCreated`, `removeMuxPane`) を削除、`handleMuxPaneCreated` 先頭の split 分岐を削除、`handleMuxPaneExited` の layoutRoot/paneCanvases 分岐を削除
- `src/terminal/mux/mux-client.ts` — `MuxMessageType.SplitPane = 0x11` のエントリを削除 (他の値は据え置き)
- `src/settings/sections/mux-section.ts` — `ACTION_I18N_KEYS` から 7 アクション行を削除
- `src/terminal-app/index.ts`:
  - 不要 import 削除: `mux-multi-pane`, `mux-drag-resize`, `mux-copy-mode` (./mux/), `mux-copy-mode` (terminal/), `layout` 型
  - 不要フィールド削除: `muxLayoutRoot`, `muxActivePaneId`, `muxPaneCanvases`, `muxPaneContainer`, `muxPendingSplitCount`, `muxPendingSplitDirection`, `muxDragState`, `muxPreZoomLayout`, `copyModeManager`, `copyModeKeybinds`, `copyModeIndicator`
  - 不要メソッド削除: `getMuxCopyModeContext`, `enterCopyMode`, `exitCopyMode`, `handleCopyModeKey`, `copySelectionToClipboard`, `handleCopyModeIndicatorChange`, `handleMuxSplitPaneCreated`, `initMultiPaneMode`, `createPaneCanvas`, `setActiveMuxPane`, `applyMuxLayout`, `sendPaneResizes`, `removeMuxPane`, `exitMultiPaneMode`, `initMuxDragResize`, `teardownMultiPaneAfterReinit`, `getMuxDragResizeContext`, `getMuxMultiPaneContext`, `getMuxCopyModeContext`, `renderMuxPaneOutput`
  - `KeyboardHandlerContext.onCopyModeKey` を削除 (呼び出し元 `handlers/keyboard` 側も追従)
  - `onMuxResize` 内の `if (this.muxLayoutRoot)` 分岐を削除しシングルペイン処理のみ残す
  - `onWasmRecovered` の `muxPaneCanvases` クリア処理を削除
  - `pasteFromClipboard` は copy-mode 由来の実装から `mux-action-handler` / `mux-session` が要求する最小実装へ再配線 (クリップボードから読み取って active pane へ送る責務のみ)
- `src/terminal/mux/index.ts` — `export { MuxPaneManager } from "./pane-manager";` を削除
- `src/terminal-app/handlers/keyboard.ts` (`KeyboardHandlerContext` 側) — `onCopyModeKey` を削除しコピーモード分岐を除去 (`handlers/keyboard.ts` の該当箇所は `index.ts` の型エラー解消過程で特定する)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `PrefixKeyHandler` | `detach`, `new-window`, `next-window`, `prev-window`, `rename-window`, `paste`, `prefix-passthrough` の 7 アクションを dispatch | prefix キーが押下済み | 対応するアクションが `onAction` に渡るか、未知キーとして no-op |
| `handleMuxAction` | 上記 7 アクションの処理 (IPC 送信 or 内部状態更新) | `MuxAction` が 7 種のいずれか | daemon 側に適切な `MuxMessageType` が送信される、または UI 状態が更新される |
| `MuxClient.sendControl` | APC/OSC 経由で daemon へ制御メッセージ送信 | `MuxMessageType` が有効 | daemon に frame body (type + pane_id + payload) が届く |

**Processing Flow** (削除前 → 削除後):

削除前: `PrefixKeyHandler` は 14 アクション (7 削除対象 + 7 残す対象) を dispatch し、`handleMuxAction` が 14 通りの case 節を持つ。
削除後: `PrefixKeyHandler` は 7 アクション (残す対象) のみ dispatch し、未知キーを受けた場合は `reset` して終わる。`handleMuxAction` は 7 通りの case 節のみ持つ。

**Implementation Steps**:

1. **`MuxAction` 型と `DEFAULT_ACTION_BINDINGS` の縮小**: `prefix-key.ts` を修正
2. **`mux-action-handler` の縮小**: `MuxActionContext` 整理と `switch` 縮小
3. **context 要求項目の縮小**: `mux-session.ts`, `mux-window-manager.ts` の context インタフェースを整理
4. **TerminalApp の mux 関連プライベート状態削除**: `terminal-app/index.ts` のフィールド・メソッド・import を削除
5. **KeyboardHandler の copy-mode 参照削除**: `onCopyModeKey` 依存を除去
6. **`MuxMessageType.SplitPane` 削除**: `mux-client.ts`
7. **設定 UI の `ACTION_I18N_KEYS` 縮小**: `mux-section.ts`
8. **i18n ロケールキー削除**: `src/i18n/locales/{en,ja}.json` から 7 キーを削除

**Dependencies**: なし (Phase 2 の前提)

**Testing Approach**:

- Unit (bun): `prefix-key.test.ts` は Phase 3 で縮小するため一時的に skip or 許容される失敗があり得る。`mux-client.test.ts` の `MuxMessageType` アサーションを維持
- Type check: `bun run typecheck` 通過
- Integration: なし (Phase 4 E2E 後)

**Acceptance Criteria**:

- [ ] `MuxAction` 型が 7 バリアント (`detach` / `new-window` / `next-window` / `prev-window` / `rename-window` / `paste` / `prefix-passthrough`) のみに縮小されている
- [ ] `DEFAULT_ACTION_BINDINGS` に削除キー 7 個が存在しない
- [ ] `src/i18n/locales/en.json` と `ja.json` に削除 7 キーが存在しない
- [ ] `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"` が通る

**検証コマンド**:

```
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

**Estimated Effort**: large

---

### Phase 2: フロントエンド不要ファイル全削除

**Goal**: Phase 1 で参照されなくなったモジュールを削除し、typecheck と build が通る状態を維持する。

**Files to Delete**:

- `src/terminal-app/mux/mux-multi-pane.ts`
- `src/terminal-app/mux/mux-drag-resize.ts`
- `src/terminal-app/mux/mux-copy-mode.ts`
- `src/terminal/mux/layout.ts`
- `src/terminal/mux/layout.test.ts`
- `src/terminal/mux/pane-manager.ts`
- `src/terminal/mux/pane-border.ts`
- `src/terminal/mux-copy-mode/index.ts`
- `src/terminal/mux-copy-mode/index.test.ts`
- `src/terminal/mux-copy-mode/emacs-keybinds.ts`
- `src/terminal/mux-copy-mode/vi-keybinds.ts`
- 空になる場合 `src/terminal/mux-copy-mode/` ディレクトリ自体

**Files to Modify**:

- `src/styles.css` — `.mux-pane-border`, `.mux-pane-border-active`, `.copy-mode-indicator` 等の不要セレクタを削除 (grep で確認し、他で使われていないもののみ)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ファイル削除 | 削除対象ファイルを作業ツリーから消す | Phase 1 で参照元が解消済み | `bun run typecheck` / `bun test` が通る |

**Implementation Steps**:

1. **TypeScript ファイルの削除**
2. **空ディレクトリの削除**
3. **CSS の不要セレクタ削除**
4. **import 残存確認**: Phase 1 で見落とした参照がないか grep で再確認

**Dependencies**: Phase 1 完了

**Testing Approach**:

- Type check: `bun run typecheck` 通過
- Unit (bun): `bun test` 通過 (削除したファイル由来のテストも当然無くなる)

**Acceptance Criteria**:

- [ ] 11 ファイルが作業ツリーから削除されている
- [ ] `src/` 配下で `mux-multi-pane`, `mux-drag-resize`, `pane-manager`, `pane-border`, `mux-copy-mode`, `mux/layout` への import が 0 件
- [ ] `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck && bun test"` が通る

**検証コマンド**:

```
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

**Estimated Effort**: small

---

### Phase 3: フロントエンド テスト縮小

**Goal**: 削除したアクション/モジュールを参照するテストを除去し、残アクションの検証に縮小する。

**Files to Modify**:

- `src/terminal/mux/prefix-key.test.ts`:
  - 削除: `prefix + % → split-vertical`, `prefix + " → split-horizontal`, `prefix + z → zoom-toggle` の個別テスト
  - 縮小: `all tmux-compatible bindings are present` テストの `bindings` 配列から `%`, `"`, `o`, `x`, `z`, `[` の 6 行を削除 (残すのは `d`, `c`, `n`, `p`, `,`, `]`)
  - 追加/確保: `unknown key after prefix is consumed but no action` テストは `q` のままで OK だが、削除アクションキー (`%`, `"`, `o`, `;`, `x`, `z`, `[`) でも同じ挙動になることを網羅するテストを追記してもよい
- `src/terminal/mux/mux-client.test.ts`:
  - `MuxMessageType` describe ブロックに `SplitPane` を明示する assertion は現状存在しないが、`SplitPane` が型に存在しないことを確認する assertion を追加するか、そのまま据え置き
- `src/terminal-app/mux/mux-action-handler.test.ts` (存在すれば): 削除アクションに紐づく case を検証するテストを削除 (ファイル確認は実装時)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `prefix-key.test.ts` | 残存アクションの dispatch 検証 | `MuxAction` が 7 バリアント | 7 アクションのみを対象とするテストが通る |

**Implementation Steps**:

1. **`prefix-key.test.ts` の該当テスト削除**
2. **`all tmux-compatible bindings are present` の bindings 配列を縮小**
3. **削除キー no-op テストの追加** (任意)
4. **その他削除モジュールに依存する test ファイルの削除確認** (Phase 2 で消えているはず)

**Dependencies**: Phase 1, Phase 2 完了

**Testing Approach**:

- Unit (bun): `bun test` 通過

**Acceptance Criteria**:

- [ ] `prefix-key.test.ts` の全テストが通る
- [ ] `mux-client.test.ts` の全テストが通る
- [ ] `bun test` 全体でエラーがない

**検証コマンド**:

```
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

**Estimated Effort**: small

---

### Phase 4: バックエンド IPC プロトコル削除

**Goal**: Rust 側から `SplitPane = 0x11` バリアント、`SplitPaneMsg` 構造体、`handle_split_pane` ハンドラ、`connection.rs` の dispatch アームを削除する。

**Files to Modify**:

- `src-tauri/src/mux/ipc/protocol.rs`:
  - `MessageType::SplitPane = 0x11` 列挙子を削除
  - `MessageType::from_u8` の `0x11 => Some(Self::SplitPane)` 分岐を削除
  - `SplitPaneMsg` 構造体を削除
  - テスト `test_message_type_round_trip` のループを `0x01..=0x19` から「0x11 を除く残存値を列挙」に変更 (0x11 は `None` を返すことを明示的にアサート)
  - テスト `test_apc_round_trip_all_message_types` のループからも `0x11` を除外
- `src-tauri/src/mux/ipc/handlers.rs`:
  - `handle_split_pane` 関数を削除
  - `SplitPaneMsg` の `use` 文を削除 (protocol.rs の `use` リストから)
- `src-tauri/src/mux/ipc/connection.rs`:
  - `use handlers::...handle_split_pane` を import リストから削除
  - `MessageType::SplitPane => { handle_split_pane(...).await?; ... }` アームを削除
- `src-tauri/src/mux/ipc/mod.rs` 等 — 必要なら pub re-export を整理

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MessageType` | IPC メッセージ種別判別 | 受信フレームの先頭バイト | 0x11 では `None` を返し、呼び出し元がフレームを破棄 |
| `from_frame_body` | フレーム復号 | 5 バイト以上の body | 0x11 で始まる body は `None` → 呼び出し元が warn log を出して破棄 |
| `connection.rs` message dispatch | メッセージ分岐 | デコード済み `MuxMessage` | `SplitPane` アームは存在せず、コンパイル時に未知ケースの警告が出ない |

**Processing Flow** (破壊的変更の扱い):

1. 旧クライアントが `SplitPane (0x11)` フレームを送信
2. daemon 側 `MuxCodec` がフレームを復号 → `MuxMessage::from_frame_body` が `MessageType::from_u8(0x11)` で `None` を返す
3. 復号が `None` なので warn log を出してフレームを破棄し、接続は継続

**Implementation Steps**:

1. **`protocol.rs` から `SplitPane` バリアントと `from_u8` 分岐、`SplitPaneMsg` を削除**
2. **`test_message_type_round_trip` / `test_apc_round_trip_all_message_types` の範囲を調整**
3. **`handlers.rs` から `handle_split_pane` とその `use` を削除**
4. **`connection.rs` から dispatch アームと import を削除**
5. **`cargo check` / `cargo test` で残存参照がないことを確認**

**Dependencies**: Phase 1-3 とは独立 (並列化可)。ただし最終的な E2E 実行前にマージする。

**Testing Approach**:

- Unit (cargo): `cargo test --manifest-path src-tauri/Cargo.toml`
  - `MessageType::from_u8(0x11)` が `None` を返すことを検証するテストを追加
  - 既存 round-trip テストが 0x11 を除外した新しいレンジで通る

**Acceptance Criteria**:

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` が通る
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` が通る
- [ ] `src-tauri/` 配下で `SplitPane`, `SplitPaneMsg`, `handle_split_pane` への参照が 0 件
- [ ] `MessageType::from_u8(0x11)` が `None` を返す unit test が含まれる

**検証コマンド**:

```
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo check --manifest-path src-tauri/Cargo.toml"
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"
```

**Estimated Effort**: small

---

### Phase 5: E2E テスト スペック整理

**Goal**: 削除されたアクションに依存する E2E テストケースを除去し、残存する mux E2E が通ることを確認する。

**Files to Check (Read/Grep 事前確認済み)**:

- `e2e-tests/specs/mux.e2e.js` — pane-split/zoom/copy-mode 関連ケースは存在しない。削除対象は **0 件**。window (sub-tab) 作成・切替・デタッチケースのみでそのまま維持
- `e2e-tests/specs/mux-multi-session.e2e.js` — pane-split/zoom/copy-mode ケースは存在しない。削除対象は **0 件**。ウィンドウ作成・切替・デタッチ・マルチタブケースのみ
- `e2e-tests/specs/mux-reattach.e2e.js` — pane-split/zoom/copy-mode ケースは存在しない。削除対象は **0 件**。デタッチ/リアタッチ・ウィンドウ切替ケースのみ
- `e2e-tests/specs/viewer-tab-switch-keyboard.e2e.js` — pane/split/zoom/copy-mode 関連コードは存在しない。削除対象は **0 件**

**実装時の確認手順 (再確認):**

1. 各 spec を `rg "split|zoom|copy.mode|Pane(?!Id)"` で再 grep し、事前調査で見落としがないか確認する
2. もし該当ケースが発見された場合のみ、該当 `it(...)` ブロックを削除
3. 事前調査通り 0 件なら、この Phase は「確認のみ」で変更コミットは発生しない

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| E2E spec 群 | 削除後の mux 機能が退行していないことを保証 | Phase 1-4 が完了 | `./scripts/run-e2e-docker.sh` が通る |

**Implementation Steps**:

1. **4 つの spec を再 grep で確認**
2. **該当テストがあれば削除** (事前調査では 0 件)
3. **E2E 一括実行で回帰確認**

**Dependencies**: Phase 1-4 完了

**Testing Approach**:

- E2E (Docker): `./scripts/run-e2e-docker.sh test`
- 個別実行で debug: `./scripts/run-e2e-docker.sh test mux.e2e.js` 等

**Acceptance Criteria**:

- [ ] `./scripts/run-e2e-docker.sh test` が通る (削除後の構成で)

**検証コマンド**:

```
./scripts/run-e2e-docker.sh test
```

**Estimated Effort**: small

---

### Phase 6: doc/tasks/terminal-multiplexer/ 関連ドキュメント追従

**Goal**: 既に `/sdd.1` で更新済みの `doc/tasks/terminal-multiplexer/SPEC.md` 以外に残った旧 FR 参照 (pane split / copy mode / `layout.ts` / `pane-manager` / `mux-copy-mode/` / `SplitPane 0x11`) を更新する。

**Files to Check**:

- `doc/tasks/terminal-multiplexer/SPEC.md` — /sdd.1 で更新済み。**本実装では触らない**
- `doc/tasks/terminal-multiplexer/IMPLEMENTATION.md` — 旧 FR 番号 (FR7 Pane Layout, FR10 Copy Mode) やファイル (layout.ts, pane-manager.ts, mux-copy-mode/) への参照が残っていないか確認
- `doc/tasks/terminal-multiplexer/FIG.md` — ペインレイアウト図や `SplitPane (0x11)` 記載が残っていないか確認
- `doc/tasks/terminal-multiplexer/要件定義書.md` — Copy Mode / Pane Split / zoom 関連の章・表が残っていないか確認
- `doc/tasks/terminal-multiplexer/VERIFICATION.md` — 削除機能の検証項目が残っていないか確認
- `doc/tasks/terminal-multiplexer/VERIFICATION_RESULT.md` — 検証結果ログ。過去時点のスナップショットとして残す場合は触らない、現状と整合を取りたい場合は注記を入れる (実装時判断)

**実装時の確認手順:**

1. 各ファイルを `rg "SplitPane|split-vertical|split-horizontal|zoom|copy-mode|layout\.ts|pane-manager|mux-copy-mode|Pane Layout|Copy Mode"` で grep
2. ヒットがあれば個別に判断して更新 or 削除
3. `VERIFICATION_RESULT.md` は履歴なのでそのまま残すか判断する (git がバージョン管理するため注記不要)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| terminal-multiplexer 関連ドキュメント | mux 機能の完全な仕様・設計・検証を記述 | SPEC.md は更新済み | 他ドキュメントも削除された機能への言及が残っていない |

**Implementation Steps**:

1. **grep による残存参照の検出**
2. **`IMPLEMENTATION.md` / `FIG.md` / `要件定義書.md` / `VERIFICATION.md` の更新**
3. **`VERIFICATION_RESULT.md` は履歴として据え置き** (必要に応じて注記追加)

**Dependencies**: Phase 1-5 完了後、または並行実施可

**Testing Approach**:

- 手動確認: grep で残存参照が 0 件であることを確認

**Acceptance Criteria**:

- [ ] `rg "SplitPane|split-vertical|split-horizontal|zoom-toggle|copy-mode" doc/tasks/terminal-multiplexer/` が意図的な残存 (履歴ファイル) 以外で 0 件

**検証コマンド**:

```
rg "SplitPane|split-vertical|split-horizontal|zoom-toggle|copy-mode|layout\.ts|pane-manager|mux-copy-mode" doc/tasks/terminal-multiplexer/
```

**Estimated Effort**: small

---

## Complete File Structure (post-cleanup)

```
src/
├── terminal/
│   └── mux/
│       ├── index.ts            (MuxPaneManager の export を削除)
│       ├── mux-client.ts       (SplitPane 定数削除)
│       ├── mux-client.test.ts
│       ├── mux-logger.ts
│       ├── prefix-key.ts       (MuxAction 縮小)
│       ├── prefix-key.test.ts  (縮小)
│       ├── tab-group.ts
│       └── tab-group.test.ts
├── terminal-app/
│   └── mux/
│       ├── mux-action-handler.ts   (縮小)
│       ├── mux-session.ts          (縮小)
│       └── mux-window-manager.ts   (縮小)
├── settings/
│   └── sections/
│       └── mux-section.ts          (ACTION_I18N_KEYS 縮小)
└── i18n/
    └── locales/
        ├── en.json             (7 キー削除)
        └── ja.json             (7 キー削除)

src-tauri/
└── src/
    └── mux/
        ├── bridge.rs
        ├── cli.rs
        ├── daemon.rs
        ├── ipc/
        │   ├── protocol.rs         (SplitPane/SplitPaneMsg 削除)
        │   ├── handlers.rs         (handle_split_pane 削除)
        │   ├── connection.rs       (SplitPane dispatch 削除)
        │   └── ...
        ├── ring_buffer.rs
        ├── session/
        │   ├── pane.rs             (据え置き)
        │   └── ...
        ├── snapshot.rs
        └── tmux_conf/              (据え置き、split→split-vertical 変換は DEFAULT_ACTION_BINDINGS に存在しないため無視される)

e2e-tests/
└── specs/
    ├── mux.e2e.js                  (変更なし想定、実装時再確認)
    ├── mux-multi-session.e2e.js    (変更なし想定)
    ├── mux-reattach.e2e.js         (変更なし想定)
    └── viewer-tab-switch-keyboard.e2e.js  (変更なし想定)

doc/tasks/
├── mux-feature-cleanup/            (本タスク)
│   ├── SPEC.md
│   ├── IMPLEMENTATION.md
│   ├── VERIFICATION.md
│   ├── sdd.yaml
│   └── 要件定義書.md
└── terminal-multiplexer/           (関連ドキュメント追従)
    ├── SPEC.md                     (/sdd.1 で更新済み)
    ├── IMPLEMENTATION.md           (Phase 6 で更新)
    ├── FIG.md                      (Phase 6 で更新)
    ├── VERIFICATION.md             (Phase 6 で更新)
    ├── VERIFICATION_RESULT.md      (履歴、据え置き)
    ├── sdd.yaml
    └── 要件定義書.md               (Phase 6 で更新)
```

**削除されるファイル (フロントエンド)**:

- `src/terminal-app/mux/mux-multi-pane.ts`
- `src/terminal-app/mux/mux-drag-resize.ts`
- `src/terminal-app/mux/mux-copy-mode.ts`
- `src/terminal/mux/layout.ts`
- `src/terminal/mux/layout.test.ts`
- `src/terminal/mux/pane-manager.ts`
- `src/terminal/mux/pane-border.ts`
- `src/terminal/mux-copy-mode/index.ts`
- `src/terminal/mux-copy-mode/index.test.ts`
- `src/terminal/mux-copy-mode/emacs-keybinds.ts`
- `src/terminal/mux-copy-mode/vi-keybinds.ts`
- `src/terminal/mux-copy-mode/` (空になる)

## Testing Strategy

- **Unit (Rust)**: `cargo test` — `MessageType::from_u8(0x11) == None` 検証、`test_message_type_round_trip` の除外確認、他の既存テストは変更なしで通る
- **Unit (TypeScript)**: `bun test` — `prefix-key.test.ts` が 7 アクションのみで通る、`mux-client.test.ts` が維持される
- **Type check**: `bun run typecheck` — Phase 1 完了時点で通る
- **E2E (Docker)**: `./scripts/run-e2e-docker.sh test` — Phase 5 完了時点で 4 つの mux 関連 spec が通る
- **手動確認**: 設定パネルの Mux > Keybinds に削除アクションの行が表示されないこと、`prefix + %` 等の削除キーが no-op であること (開発者確認)

## Dependencies

変更なし。`portable-pty`, `bincode`, `base64`, `tokio`, `serde` 等既存依存のみを継続利用。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Phase 1 で typecheck エラーが芋づる式に発生 | High | Medium | `bun run typecheck` を小刻みに回し、エラー件数を段階的に減らす |
| `terminal-app/index.ts` の削除するメソッドが他ファイルからも参照されている | Medium | Medium | grep で公開メソッド名 (`enterCopyMode`, `handleCopyModeKey` など) を全文検索し参照元を特定 |
| `VERIFICATION_RESULT.md` に履歴として書かれた削除機能の記述を残すか削除するかで議論になる | Low | Low | 原則据え置き (git で履歴は残る)。必要なら追記で現状を注記 |
| 削除キーがユーザーの独自キーバインドで上書きされていた場合の挙動 | Low | Low | `DEFAULT_ACTION_BINDINGS` から該当アクションが消えるため、上書き先のアクションがそもそも `MuxAction` 型に存在せず `handleMuxAction` で no-op。serde で未知フィールドは破棄される |
| tmux_conf/converter.rs の `split-vertical` / `copy-mode` 生成が残存 | Low | Low | 出力キーが `DEFAULT_ACTION_BINDINGS` に存在しないため無害 (要件定義書 6 項参照)。converter 自体の縮小は本タスクのスコープ外 |
| Phase 4 の `test_message_type_round_trip` 修正で範囲表記ミス | Low | Low | 0x11 を除外するため明示的に `[0x01..=0x10, 0x12..=0x19]` のような 2 レンジに分割するか、各値を列挙する |

## Open Questions

特になし。SPEC 段階で仕様は確定しており、E2E スペックの個別ケース削除も事前調査で 0 件を確認済み。

## Success Metrics

- [ ] SPEC.md の全 FR (FR1-FR9) と NFR (NFR1-NFR3) が実装されている
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` 通過
- [ ] `bun test` 通過
- [ ] `bun run typecheck` 通過
- [ ] `./scripts/run-e2e-docker.sh test` 通過
- [ ] `src/`, `src-tauri/` 配下で削除シンボル (`split-vertical`, `split-horizontal`, `next-pane`, `prev-pane`, `close-pane`, `zoom-toggle`, `copy-mode`, `SplitPane`, `handle_split_pane`, `SplitPaneMsg`) への参照が 0 件 (tmux_conf/converter.rs の変換ロジックを除く)
- [ ] `src/i18n/locales/` から `splitVertical`, `splitHorizontal`, `nextPane`, `prevPane`, `closePane`, `zoomToggle`, `copyMode` の 7 キーが en/ja ともに消えている
- [ ] 設定パネルの Mux > Keybinds セクションに 7 行 (detach, new-window, next-window, prev-window, rename-window, paste) のみ表示される
