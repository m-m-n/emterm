---
title: "ユニットテストの本物のデスクトップ通知送信を止める"
created_date: 2026-07-30
status: draft
---

# ユニットテストの本物のデスクトップ通知送信を止める - 要件定義書

## 1. 概要

### 1.1 背景

`cargo test` を実行するたびに、デスクトップ通知「eMterm / agent: (ブロック中)」が 1 個
ポップアップする。通知元名はテストバイナリ名（例: `emterm-958649d0f395b13c`）になる。

- テスト: `pump_all_applies_daemon_agent_status_update_to_model`（`src-tauri/src/app.rs`）
- 通知送信: `NotifyRustSink`（notify-rust → D-Bus）— `src-tauri/src/callbacks.rs:121`
- 通知配線: `pump_all` の transition drain — `src-tauri/src/app.rs:3785`

このテストは `App::new()` のまま `notification_sink` を `TestNotifySink` に差し替えずに、
`state: Blocked, name: "agent"` の daemon 更新を `pump_all()` に流している。

- テスト自体は task0005 時点から存在し、当時 `pump_all` は通知を出さなかった
- コミット `db10cca`（2026-07-24、task0009: wire agent-status transitions into the
  notification pipeline）で `pump_all` に通知配線が入り、以降このテストが本物の D-Bus
  通知を飛ばすようになった
- 発火条件が全て揃う: `window_focused` デフォルト false → 非可視 pane 扱い、通知設定は
  デフォルト両方 on、レートリミッタ初回
- 通知本文の tab_title は pane 42 に対応するタブが無く空文字になり、locale は
  language=Auto → OS ロケール解決で日本語になるため「agent: (ブロック中)」となる

task0009 の専用テスト群は全て `app_with_test_sink()` でキャプチャ用シンクに差し替えており、
差し替え漏れはこの 1 本のみ。他の `App::new()` + Blocked 系テスト（バッジテスト）は
`pump_all()` を呼ばないため発火しない。

### 1.2 目的

ユニットテストがデスクトップ通知を飛ばさない状態にする。

### 1.3 スコープ

- 対象: `src-tauri/src/app.rs` の
  `pump_all_applies_daemon_agent_status_update_to_model` テスト 1 本
- 対象外: プロダクションコード（`pump_all` / `NotifyRustSink` / 通知配線）の変更、
  `App::new()` のデフォルトシンク自体の変更

## 2. ビジネス要件

### 2.1 ビジネス目標

テストスイートを外部環境（D-Bus セッション）から独立させ、開発者のデスクトップを
汚さない。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | ローカルで `cargo test` を実行する人 |
| CI | D-Bus の無い環境（コンテナ）でテストを実行する系 |

### 2.3 期待される効果

- `cargo test` 実行時にデスクトップ通知が出なくなる
- テストが D-Bus セッションの有無に依存しなくなる

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | ローカルでユニットテストを実行する | 開発者 | 高 |

### 3.2 ユースケース詳細

#### UC01: ローカルでユニットテストを実行する

**アクター**: 開発者

**事前条件**:
- D-Bus セッションが生きているデスクトップ環境
- 通知設定はデフォルト（`agent_status_notifications` / `notification_enabled` 共に on）

**基本フロー**:
1. 開発者が `cargo test` を実行する
2. `pump_all_applies_daemon_agent_status_update_to_model` が実行される
3. テストは通知をキャプチャ用シンクへ送り、OS へは何も出さない
4. テストがパスする

**事後条件**:
- デスクトップ通知が 1 個も表示されていない

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | 対象テストのシンク差し替え | 対象テストが `app_with_test_sink()` を使う | 高 |

### 4.2 機能詳細

#### F01: 対象テストのシンク差し替え

**説明**: `pump_all_applies_daemon_agent_status_update_to_model` の `App::new()` を
`app_with_test_sink()` に置き換える。既存の検証（`agent_status` に daemon 更新が
反映され、`state == Blocked` / `revision == 7` になること）はそのまま維持する。

**ビジネスルール**:
- テストの検証内容（AC-2: daemon `AgentStatusUpdate` が `pump_all` 経由でモデルに届く）
  を弱めない

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし。

### 5.2 セキュリティ要件

該当なし。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

- テストが外部プロセス（D-Bus）へ副作用を出さない

### 5.5 互換性要件

- Linux / Windows 両方でビルド・テストが通ること（テストコードのみの変更なので
  プラットフォーム分岐は不要）

## 6. UI/UX要件

該当なし（UI 変更は無い）。

## 7. データ要件

該当なし。

## 8. 外部連携

### 8.1 連携システム

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| D-Bus (notify-rust) | `NotifyRustSink` | 通知タイトル・本文 |

テスト実行時にこの連携が起きないようにするのが本タスクの目的。

## 9. 制約条件

### 9.1 技術的制約

- `app_with_test_sink()` は `src-tauri/src/app.rs` の `#[cfg(test)] mod tests` 内に
  既に存在し、対象テストと同一モジュールにあるためそのまま呼べる
- `app_with_test_sink()` は通知設定のデフォルト（両方 on）を assert する

### 9.2 ビジネス上の制約

なし。

### 9.3 スケジュール制約

なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 同種の差し替え漏れが他テストに残っている可能性 | 低 | 検証時に `App::new()` + `pump_all()` + Blocked/Done 系テストを走査して確認する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] `pump_all_applies_daemon_agent_status_update_to_model` が `app_with_test_sink()` を使う
- [ ] テストの既存アサーション（`state == Blocked` / `revision == 7`）が維持されている
- [ ] `cargo test` 実行時にデスクトップ通知が表示されない
- [ ] プロダクションコードに変更が無い

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: 対象テストがパスし、キャプチャ用シンクが通知を受け取る（OS へは出ない）
- [ ] 回帰: `cargo test --lib` 全体がパスする
- [ ] 走査: 他に `App::new()` のまま通知を発火し得るテストが無いことを確認する

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| NotificationSink | OS 通知面を抽象化する trait（`src-tauri/src/callbacks.rs`） |
| NotifyRustSink | 本物の通知を送る本番実装（notify-rust → D-Bus） |
| TestNotifySink | 通知をキャプチャするテスト用実装 |
| app_with_test_sink() | `App` を作り `notification_sink` を `TestNotifySink` に差し替えるテストヘルパー |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 修正範囲: Notion タスクの「期待する挙動」に従い、対象テスト 1 本の
      `app_with_test_sink()` 差し替えのみ（2〜3 行の変更）
- [x] プロダクションコード（`pump_all` の通知配線・`App::new()` のデフォルトシンク）は
      変更しない
- [x] `app_with_test_sink()` と対象テストは同一の `mod tests` 内にある

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- Notion タスク: [https://www.notion.so/3a83509ec8ee81d7873aec3beaaba5db](https://www.notion.so/3a83509ec8ee81d7873aec3beaaba5db)
- 通知配線を入れたコミット: `db10cca`（2026-07-24, task0009）
