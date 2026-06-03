# 🔍 実装自動検証レポート (sdd.6-verify)

**対象機能**: background-osc-notification
**VERIFICATION.md**: `doc/tasks/background-osc-notification/VERIFICATION.md`
**プロジェクト**: eMterm

---

## 📊 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド | ✅ | sdd.5 で検証済（Docker, `cargo build --features gui` exit 0） |
| テスト実行 | ✅ | sdd.5 で検証済（Rust 1017 + 統合 33 PASS、TS feature 全 PASS） |
| コードフォーマット | ✅ | sdd.5 で検証済（feature 9 ファイル fmt クリーン、新規 clippy 警告なし） |
| 静的解析 / デッドコード | ✅ | sdd.5 で 1 件検出 → 修正済（未使用 export 型を削除）。再検出なし |
| ファイル構造 | ✅ | 17/17 期待ファイルが存在 |
| SPEC.md 適合性 | ✅ | FR1–7 / NFR1–5 すべて実装・配線を確認 |

**総合評価**: ✅ すべての自動検証項目をクリア

> ビルド/テスト/フォーマット/静的解析は sdd.5-check で実施済みのため再実行していない（staleness なし: check と HEAD は同一コミット）。

---

## ✅ ファイル構造検証 (17/17)

バックエンド (Rust):
- ✅ `src-tauri/src/pty/passthrough_scanner.rs`
- ✅ `src-tauri/src/pty/visibility.rs`
- ✅ `src-tauri/src/reader.rs`
- ✅ `src-tauri/src/payloads.rs`
- ✅ `src-tauri/src/mux/ipc/pty_spawn.rs`
- ✅ `src-tauri/src/mux/ipc/protocol.rs`
- ✅ `src-tauri/src/mux/ipc/connection.rs`
- ✅ `src-tauri/src/mux/ipc/handlers.rs`
- ✅ `src-tauri/src/mux/session/pane.rs`
- ✅ `src-tauri/src/mux/daemon.rs`

フロントエンド (TS):
- ✅ `src/terminal/mux/mux-client.ts`
- ✅ `src/terminal-app/mux/mux-session.ts`
- ✅ `src/terminal-app/index.ts`
- ✅ `src/terminal/background-notification-listener.ts`（新規）
- ✅ `src/terminal/background-notification-listener.test.ts`（新規）
- ✅ `src/terminal-app/osc-handler-notification.test.ts`（新規）

注: `src/types/pty.ts` は計画上「変更ファイル」だったが、sdd.5 で未使用 export 型 `OscNotificationPayload` を削除した結果ベースラインに復帰し、**net 差分なし**。実際に使用される型は listener 自己完結の `OscNotificationEvent`。

---

## ✅ SPEC.md 適合性検証 (FR/NFR 実装確認)

| 要件 | 実装マーカー | 結果 |
|------|-------------|------|
| FR1 非表示ウィンドウ検知 | `visibility.rs::process_hidden` が `scanner.take_notifications()` を surface → `reader.rs::emit_osc_notifications` → `app.emit("osc_notification")` → `background-notification-listener.ts` → `sendNotification` | ✅ |
| FR2 mux Detached 検知 | `pty_spawn.rs::capture_passthrough`（**Detached アーム専用**）→ `MessageType::Notify(0x1C)`/`NotifyMsg` → GUI 転送 → `mux-client.ts` dispatch → `mux-session.ts` `sendNotification` | ✅ |
| FR3 非アクティブ通常タブ | `osc-handler.ts` case 9 が前面パスで発火（アクティブタブゲート無し）。回帰テスト TS-13 で確認 | ✅ |
| FR4 進捗(`9;4`)除外 | scanner が `9;4` を通知扱いしない（TS-3） | ✅ |
| FR5 二重発火防止 | OSC 9 を replay/passthrough buffer に混ぜず `take_notifications` で別系統（TS-7, TS-8） | ✅ |
| FR6 内容＋パーミッション | `sendNotification("eMterm", message)`、sink 内で `isPermissionGranted` ゲート（TS-12） | ✅ |
| FR7 BEL/ST＋分割 | scanner が BEL/ST 両終端・チャンク分割を回収（TS-1, TS-2, TS-4） | ✅ |
| NFR1 性能/前面無影響 | スキャンは背面経路のみ。前面ホットパス不変 | ✅ |
| NFR2 バッファ上限 | `PARTIAL_SEQUENCE_MAX` ガード踏襲（TS-5） | ✅ |
| NFR3 GUI のみ発火/デーモン転送 | OS 通知は GUI で発火。デーモンは `Notify` メッセージを転送のみ（TS-9, TS-11） | ✅ |
| NFR4 Linux/Windows | デーモン通知タスクは Unix/Windows 両対応で実装 | ✅ |
| NFR5 前面挙動不変 | `osc-handler.ts` 不変。active/Connected ペインは転送せず二重発火なし（TS-14, 設計上） | ✅ |

---

## 🐳 E2E テスト結果

- Docker E2E 環境: **存在する**（`e2e-tests/`, `docker-compose.e2e.yml`, `scripts/run-e2e-docker.sh`）
- 本機能の E2E: **自動検証不可**。ヘッドレス Docker（Xvfb）には通知デーモンが無く、OS デスクトップ通知の発火を自動アサートできない。
- 回帰: ロジックは sdd.5 のユニット/統合テスト（Rust 1017 + 統合、TS feature 全 PASS）と typecheck で担保済み。フル E2E スイートは本機能のコードパスを検証しないため自動実行していない。

> 回帰の念押しが必要なら手動で `./scripts/run-e2e-docker.sh test` を実行のこと。

---

## 📋 手動確認が必要な項目（E2E 不可）

OS デスクトップ通知の実発火はヘッドレスで検証できないため、以下を実機で確認すること:

- [ ] **ウィンドウ最小化中**: ウィンドウを最小化し `printf '\033]9;done\007'` を実行 → OS 通知が 1 回出る。ウィンドウを戻しても二重に出ない。
- [ ] **mux 非アクティブペイン/ウィンドウ**: mux で非アクティブなペイン/ウィンドウから同シーケンスを発行 → GUI 経由で通知が 1 回出る。再アタッチで二重に出ない。
- [ ] **通常タブ（mux 無し）**: タブを 2 つ開き、非アクティブタブから発行 → 通知が 1 回出る。
- [ ] **前面の回帰**: アクティブ/表示中のタブで `OSC 9 ; msg` と `OSC 9 ; 4 ; …`（進捗）が従来どおり動作する。
- [ ] **mux デーモン版ずれ**: 機能追加前から常駐していたデーモンは再起動するまで OSC 9 を検出しない（既知の挙動）。

---

## 🎯 総合評価

✅ 自動検証はすべてクリア。SPEC の FR1–7 / NFR1–5 はコード上で実装・配線を確認済み。残るは上記の手動通知確認のみ。

**注意点（実装フェーズで延期・記録した事項）**:
- mux デーモンのバージョンスキュー（再起動まで未検出）— IMPLEMENTATION.md リスク表に記載。
- 既存の `src/clipboard/manager.test.ts` のフルスイート失敗は本機能と無関係（単独 35/35 PASS）。
