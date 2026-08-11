---
title: "agent-notification-sanitize-title"
created_date: 2026-08-12
status: draft
---

# agent-notification-sanitize-title - 要件定義書

## 1. 概要

### 1.1 背景

レビュー finding 7dd413bdd9289905 (severity: medium / category: security) として、エージェント状態通知の本文に埋め込まれるタブタイトルが未サニタイズのまま扱われている点が指摘された。タブタイトルは OSC 0 / 2 由来の非信頼入力であり、現状は `notify_rust::Notification::body` を経由して D-Bus / OS 通知サーバへそのまま渡っている。

### 1.2 目的

エージェント状態通知の本文に埋め込むタブタイトルを既存の `sanitize_title` に通し、未サニタイズの非信頼入力（OSC 0/2 由来のタブタイトル）が `notify_rust::Notification::body` 経由で D-Bus / OS 通知サーバへ渡らないようにする（finding 7dd413bdd9289905 の解消）。

### 1.3 スコープ

**対象**

- `agent_notification_body` 内でのタブタイトルのサニタイズ
- CSI / 制御文字入りタイトルが通知本文に残らないことを固定する単体テストの追加

**対象外**

- 新規サニタイズ実装の書き起こし（既存の `sanitize_title` を再利用する）
- タブアクティビティ通知側（既に `sanitize_title` を通している経路）の挙動変更
- 通知経路の非同期化

## 2. ビジネス要件

### 2.1 ビジネス目標

エージェント状態通知の本文に埋め込むタブタイトルを既存の `sanitize_title` に通し、未サニタイズの非信頼入力（OSC 0/2 由来のタブタイトル）が `notify_rust::Notification::body` 経由で D-Bus / OS 通知サーバへ渡らないようにする（レビュー finding 7dd413bdd9289905, severity medium / security の解消）。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm 利用者 | エージェント状態通知をデスクトップ通知として受け取る利用者 |

### 2.3 期待される効果

- OSC 0/2 由来の非信頼なタブタイトルが、サニタイズされないまま D-Bus / OS 通知サーバへ渡らなくなる。
- タブアクティビティ通知経路と同一のサニタイザを共有することで、両経路の挙動が一致する。

## 3. ユースケース

該当なし（内部実装のセキュリティ修正であり、新規のユーザー操作フローを伴わない）。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | ステータス |
|----|--------|------|-----------|
| FR1 | `agent_notification_body` でのタイトルサニタイズ | 通知本文へ埋め込む `tab_title` を既存の `sanitize_title` に通す | resolved |
| FR2 | サニタイズ固定の単体テスト | CSI / 制御文字入りタイトルが本文に残らないことを固定するテストを追加する | resolved |

### 4.2 機能詳細

#### FR1: `agent_notification_body` でのタイトルサニタイズ

**説明**: `agent_notification_body` の内部で、埋め込む `tab_title` を既存の `sanitize_title` に通す。両呼び出し元を一箇所で塞げる choke point として `agent_notification_body` 内で行う。新規のサニタイズ実装は書き起こさない。

**ステータス**: resolved

#### FR2: サニタイズ固定の単体テスト

**説明**: CSI / 制御文字入りのタブタイトルが通知本文に残らないことを固定する単体テストを `notifications::tests`（inline `#[cfg(test)] mod tests` 慣習）に追加する。

**ステータス**: resolved

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 名称 | ステータス |
|----|------|-----------|
| NFR1 | 既存サニタイザの再利用 | resolved |
| NFR2 | 既存経路への非影響 | resolved |

#### NFR1: 既存サニタイザの再利用

`sanitize_title` は既存関数を使用し、新規サニタイズ実装を追加しない（タブアクティビティ通知経路との挙動一致を保つ）。

#### NFR2: 既存経路への非影響

タブアクティビティ通知側（既に `sanitize_title` を通している経路）の挙動を変更しない。通知経路の非同期化も行わない（スコープ外）。

### 5.2 セキュリティ要件

- 入力検証: OSC 0/2 由来のタブタイトルは非信頼入力として扱い、通知本文へ埋め込む前に既存の `sanitize_title` を通す（FR1 / NFR1）。

## 6. UI/UX要件

該当なし（UI・ビジュアル・レイアウト・デザイントークンへの変更を伴わない）。

## 7. データ要件

該当なし。

## 8. 外部連携

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| D-Bus / OS 通知サーバ | `notify_rust::Notification::body` | サニタイズ済みのタブタイトルを含む通知本文 |

## 9. 制約条件

### 9.1 技術的制約

- サニタイズは `agent_notification_body` 内の一箇所で行い、両呼び出し元をその choke point で塞ぐ。
- 新規サニタイズ実装を追加せず、既存の `sanitize_title` を使用する。
- 単体テストは `notifications::tests`（inline `#[cfg(test)] mod tests` 慣習）に追加する。
- finding は PR #29（`em-workflow/active-window-agent-notification/integration`、main 未マージ）に対するものであり、着手時点の main の状態確認が実装側の前提作業になる。

## 10. 想定される課題とリスク

| 課題 | 対応策 |
|------|--------|
| `tabs.rs` の replay テストが並列実行でフレークすることがある | `-- --test-threads=1` で再実行する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] `agent_notification_body` の中で既存の `sanitize_title` を通している（両呼び出し元を一箇所で塞ぐ choke point）。
- [ ] CSI / 制御文字入りタイトルが本文に残らないことを固定する単体テストが `notifications::tests` に存在する。
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] セキュリティ: CSI シーケンス（例: ESC [ ... m）を含むタブタイトルを与えたとき、`agent_notification_body` の戻り値にエスケープ/CSI バイトが含まれない。
- [ ] セキュリティ: C0 制御文字を含むタブタイトルを与えたとき、本文に制御文字が残らない。
- [ ] 正常系: 通常のタブタイトルは従来どおり本文に埋め込まれる（回帰なし）。
- [ ] 回帰: 既存 `--lib` スイート全体がグリーンのまま（`tabs.rs` replay テストがフレークした場合は `-- --test-threads=1` で再実行）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `sanitize_title` | タブタイトルをサニタイズする既存関数。タブアクティビティ通知経路が現に使用している。 |
| `agent_notification_body` | エージェント状態通知の本文を組み立てる関数。タブタイトルを埋め込む。 |
| CSI | Control Sequence Introducer。エスケープシーケンスの一種。 |
| C0 制御文字 | 0x00–0x1F の制御文字。 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] デザインフェーズの要否: skipped。Rust バックエンドの通知本文組み立てに対するセキュリティ修正のみで、UI・ビジュアル・レイアウト・デザイントークンへの変更が一切ない。

### 14.2 前提事項（未検証を含む）

- [ ] `sanitize_title` は `src-tauri/src/notifications.rs` に既存で、タブアクティビティ通知経路が現に使用している（task_description 記載。resolved_input_paths 外のため未読・未検証）。
- [ ] `agent_notification_body` の呼び出し元は 2 箇所で、同関数内のサニタイズで両方を塞げる（task_description 記載）。
- [ ] finding は PR #29（`em-workflow/active-window-agent-notification/integration`, main 未マージ）に対するもの。着手時点の main の状態確認が実装側の前提作業になる（task_description の制約）。
- [ ] notifications モジュールは GUI feature 配下（notify-rust は `gui` optional dep）のため、テストは default features のユニットテストで実行される。

## 15. 参考資料

- レビュー finding: 7dd413bdd9289905（severity: medium / category: security）
- 実装仕様書: `feature-docs/agent-notification-sanitize-title/SPEC.md`
