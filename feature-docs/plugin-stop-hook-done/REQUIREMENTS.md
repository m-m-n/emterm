---
title: "plugin-stop-hook-done"
created_date: 2026-08-07
status: draft
---

# plugin-stop-hook-done - 要件定義書

## 1. 概要

### 1.1 背景

現状 `plugins/emterm/hooks/hooks.json` の Stop hook は `state=idle` を送るが、eMterm 側の発火対象は Blocked / Done のみ（`src-tauri/src/notifications.rs:226-228` `is_qualifying_agent_state`）のため、応答完了通知が原理的に一度も出ない。

### 1.2 目的

- Claude Code の応答完了時に eMterm の OS 通知が発火するようにする。
- プラグインの Stop 時報告状態を eMterm 本体の設計（done 完全実装済み・done+既読は `IDLE_BADGE_EMOJI` にエイリアスされ張り付かない: `src-tauri/src/ui/tab_bar.rs:1855, 1872-1879`）に整合させる。

### 1.3 スコープ

対象は、リポジトリ内 `plugins/emterm/` のソースのみ。

- 対象: `plugins/emterm/hooks/hooks.json` の Stop エントリ、`plugins/emterm/hooks/scripts/notify-status.test.ts` の期待値。
- 対象外: eMterm 本体（`src-tauri/`）。done 側は実装済みであり変更しない。
- 対象外: `notify-status.sh`。state 引数のホワイトリスト（`idle|working|blocked|done`, `notify-status.sh:18-24`）に done は既に含まれる。
- 対象外: `~/.claude/plugins/cache/` 配下のコピー。marketplace が directory source としてこのリポジトリを指すため、直接編集しない。
- 対象外: 通知経路（`terminalSequence` の OSC 777 → SSH 越しでもローカル eMterm に届く経路、D-Bus notify-rust）。

## 2. ビジネス要件

### 2.1 ビジネス目標

- Claude Code の応答完了時に eMterm の OS 通知が発火するようにする。
- プラグインの Stop 時報告状態を eMterm 本体の設計に整合させる。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm 上で Claude Code を使う利用者 | 非アクティブタブ（または非フォーカスウィンドウ）で応答完了を OS 通知で知りたい利用者 |

### 2.3 期待される効果

- 応答完了通知が発火するようになる（現状は原理的に一度も出ない）。
- プラグインの Stop 時報告状態が eMterm 本体の done 実装と整合する。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 応答完了の OS 通知を受け取る | eMterm 上で Claude Code を使う利用者 | 高 |

### 3.2 ユースケース詳細

#### UC01: 応答完了の OS 通知を受け取る

**アクター**: eMterm 上で Claude Code を使う利用者

**事前条件**:
- pane が非表示である（エージェント状態通知は pane 非表示時のみ発火）。
- 実行環境の per-event トグル `agent_notify_on_done` が有効である。
- 30 秒レート制限（`AGENT_NOTIFICATION_RATE_LIMIT`, `notifications.rs:221`）の範囲外である。

**基本フロー**:
1. Claude Code が応答を完了し、Stop イベントが発火する。
2. Stop hook が `notify-status.sh` を `done` 引数で呼ぶ。
3. eMterm が Done 状態を受け取り、OS 通知を 1 回発火する。

**代替フロー**:
- 30 秒レート制限内の連続完了では 2 回目の通知が出ない（仕様）。

**事後条件**:
- OS 通知が発火している。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | Stop hook が done を送る | Stop エントリの args を `["idle"]` から `["done"]` に変更する | 高 |
| FR2 | テストの期待値更新 | `notify-status.test.ts:420` の table 行を `["Stop", "done"]` に更新する | 高 |

### 4.2 機能詳細

#### FR1: Stop hook が done を送る

**説明**: `plugins/emterm/hooks/hooks.json` の Stop エントリの args を `["idle"]` から `["done"]` に変更する（現状 hooks.json 46-47 行目）。他のイベント（UserPromptSubmit / PostToolUse / PostToolUseFailure=working, PermissionRequest / Notification=blocked）は変更しない。

**入力**:
- `hooks.json` の Stop エントリ: args 配列 - 現状 `["idle"]`

**出力**:
- `hooks.json` の Stop エントリ: args 配列 - `["done"]`

**ビジネスルール**:
- `hooks.json` の command は `${CLAUDE_PLUGIN_ROOT}` プレフィックス・timeout 3 の既存形式を維持する（`notify-status.test.ts:424-450` が形式を検証）。

**バリデーション**:

| 項目 | ルール | エラーメッセージ |
|------|--------|------------------|
| state 引数 | `notify-status.sh:18-24` のホワイトリスト `idle\|working\|blocked\|done` に含まれること（done は既に含まれる） | 該当なし（`notify-status.sh` は変更不要） |

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| bun test 失敗 | FR1 のみ変更し FR2 を変更しない | FR1 と FR2 を同時に変更する |

#### FR2: テストの期待値更新

**説明**: `plugins/emterm/hooks/scripts/notify-status.test.ts:420` の `test.each` テーブル行 `["Stop", "idle"]` を `["Stop", "done"]` に更新する。同ファイル内で Stop→idle を結合しているのはこの 1 箇所のみ（175 行の `ALLOWED_STATES` と 486 行の `idle_prompt` は無関係で変更不要）。

**入力**:
- `notify-status.test.ts:420` の table 行 - 現状 `["Stop", "idle"]`

**出力**:
- `notify-status.test.ts:420` の table 行 - `["Stop", "done"]`

**ビジネスルール**:
- 175 行の `ALLOWED_STATES` と 486 行の `idle_prompt` は変更しない。

## 5. 非機能要件

| ID | 内容 |
|----|------|
| NFR1 | `notify-status.sh` は変更不要: state 引数のホワイトリスト（`idle\|working\|blocked\|done`, `notify-status.sh:18-24`）に done は既に含まれる。 |
| NFR2 | eMterm 本体（`src-tauri/`）は変更しない（スコープ外指定、done 側は実装済み）。 |
| NFR3 | 編集対象はリポジトリ内 `plugins/emterm/` のソースのみ。`~/.claude/plugins/cache/` 配下のコピーは直接編集しない（marketplace が directory source としてこのリポジトリを指すため）。 |
| NFR4 | `hooks.json` の command は `${CLAUDE_PLUGIN_ROOT}` プレフィックス・timeout 3 の既存形式を維持する（`notify-status.test.ts:424-450` が形式を検証）。 |
| NFR5 | 通知経路（`terminalSequence` の OSC 777 → SSH 越しでもローカル eMterm に届く経路、D-Bus notify-rust）は変更しない。 |

### 5.1 パフォーマンス要件

該当なし。

### 5.2 セキュリティ要件

該当なし。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

NFR3 のとおり、編集対象はリポジトリ内 `plugins/emterm/` のソースのみとする。

### 5.5 互換性要件

NFR1・NFR4・NFR5 のとおり、`notify-status.sh`・`hooks.json` の既存形式・通知経路は維持する。

## 6. UI/UX要件

UI サーフェス・視覚要素・レイアウトへの変更は一切ない。既存の通知 / バッジ UI は実装済みで無変更。

## 7. データ要件

該当なし。

## 8. 外部連携

### 8.1 連携システム

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| eMterm 本体 | `terminalSequence` の OSC 777（SSH 越しでもローカル eMterm に届く経路）、D-Bus notify-rust | エージェント状態（done） |

いずれも変更しない（NFR5）。

## 9. 制約条件

### 9.1 技術的制約

- eMterm 側の発火対象は Blocked / Done のみ（`notifications.rs:226-228` `is_qualifying_agent_state`）。
- `notify-status.sh` の state 引数ホワイトリストは `idle|working|blocked|done`（`notify-status.sh:18-24`）。
- `hooks.json` の command 形式（`${CLAUDE_PLUGIN_ROOT}` プレフィックス・timeout 3）は `notify-status.test.ts:424-450` が検証している。
- `~/.claude/plugins/cache/` 配下のコピーは直接編集しない。

### 9.2 ビジネス上の制約

- eMterm 本体（`src-tauri/`）はスコープ外指定により変更しない。

### 9.3 スケジュール制約

該当なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| Stop は応答完了以外（ユーザー中断後の停止等）でも発火し得る | 低 | done+既読が `IDLE_BADGE_EMOJI` にエイリアスされる設計（`tab_bar.rs:1855`）により done 張り付きは起きず、タスク記述が Stop→done を明示指定しているため許容と判断 |
| FR1 と FR2 の片方だけを変更するとテストが落ちる | 中 | `hooks.json` 実ファイルを `readHooksJson()` で読むテストのため、FR1 と FR2 を同時に変更する |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] `plugins/emterm/hooks/hooks.json` の Stop の args が `["done"]` になっている。
- [ ] `plugins/emterm/hooks/scripts/notify-status.test.ts:420` のアサーションが `["Stop", "done"]` に更新されている。
- [ ] `bun test` が通る。
- [ ] 実機確認: 非アクティブタブ（または非フォーカスウィンドウ）で Claude Code の応答が完了したとき OS 通知が出る（ユーザーによる手動確認）。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: `bun test` — `notify-status.test.ts` の table-driven テスト（416-436 行）が更新後の `hooks.json` と整合すること。`hooks.json` 実ファイルを `readHooksJson()` で読むテストなので、FR1 と FR2 は同時に変更しないと落ちる。
- [ ] 正常系（手動）: pane 非表示状態で応答完了 → OS 通知 1 回。30 秒レート制限（`AGENT_NOTIFICATION_RATE_LIMIT`, `notifications.rs:221`）内の連続完了では 2 回目が出ないのは仕様。
- [ ] 境界値: 30 秒レート制限内の連続完了で 2 回目の通知が出ないこと（仕様どおり）。
- [ ] エッジケース: `SubagentStop` / `StopFailure` はフックに存在せず（テスト 404-405 行が undefined を検証）、本変更の影響を受けない。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| Stop hook | `plugins/emterm/hooks/hooks.json` の Stop イベントエントリ |
| done | eMterm のエージェント状態のひとつ。`is_qualifying_agent_state`（`notifications.rs:226-228`）の発火対象 |
| idle | eMterm のエージェント状態のひとつ。通知の発火対象ではない |
| `IDLE_BADGE_EMOJI` | done+既読時にエイリアスされるバッジ表示（`tab_bar.rs:1855, 1872-1879`） |

## 14. 確認事項

### 14.1 確認済み事項

- [x] Stop hook の送信状態: `idle` から `done` に変更する（タスク記述が Stop→done を明示指定）。
- [x] 変更対象範囲: リポジトリ内 `plugins/emterm/` のソースのみ。eMterm 本体（`src-tauri/`）はスコープ外。
- [x] `notify-status.sh` の変更要否: 不要（`done` はホワイトリストに既存）。
- [x] Stop が応答完了以外でも発火する件: done+既読の `IDLE_BADGE_EMOJI` エイリアスにより張り付かないため許容。
- [x] `notification_enabled` / `agent_status_notifications` の設定状態: 確認済み。

### 14.2 未確認・保留事項

- [ ] 実行環境の per-event トグル `agent_notify_on_done` が有効であること。ゲーティング（`notifications.rs:239-249` `event_type_notifications_enabled`）は `notification_enabled` / `agent_status_notifications` に加えてこのトグルも要求するが、タスク記述が確認済みと明言したのは前者 2 つのみ（前提として扱う）。
- [ ] 実機確認は pane 非表示・レート制限外の条件で行う（エージェント状態通知は pane 非表示時のみ発火）（前提として扱う）。

## 15. 参考資料

- `plugins/emterm/hooks/hooks.json`: Stop エントリ（46-47 行目）
- `plugins/emterm/hooks/scripts/notify-status.test.ts`: 期待値テーブル（416-436 行、対象行 420）、形式検証（424-450 行）、未定義イベント検証（404-405 行）
- `plugins/emterm/hooks/scripts/notify-status.sh`: state 引数ホワイトリスト（18-24 行）
- `src-tauri/src/notifications.rs`: `is_qualifying_agent_state`（226-228 行）、`AGENT_NOTIFICATION_RATE_LIMIT`（221 行）、`event_type_notifications_enabled`（239-249 行）
- `src-tauri/src/ui/tab_bar.rs`: `IDLE_BADGE_EMOJI` エイリアス（1855 行、1872-1879 行）
