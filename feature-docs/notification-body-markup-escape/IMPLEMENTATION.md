# Implementation Plan: notification-body-markup-escape

## Overview

OS デスクトップ通知の本文に渡る OSC 0/2 由来タイトルのマークアップインジェクションを、通知サーバーが `"body-markup"` ケイパビリティを報告する場合に限り `&` `<` `>` のエスケープで防ぐ。実装は単一タスク（task0001）で行う。

## Technology Stack

- **Rust**: 既存の通知パイプライン（`src-tauri/src/callbacks.rs`）への変更のみ。
- **notify-rust**（既存・`gui` フィーチャーのオプショナル依存）: ケイパビリティ取得（`get_capabilities()`）と通知送出（`.body()`）。

**新規依存の追加は無い。** ライセンス確認対象の依存は発生せず、`project.license: MIT` に影響しない。

## Layer Structure

通知パイプライン（上流 → 下流。依存方向は下流のみ）:

| 層 | 場所 | 本機能での扱い |
|----|------|----------------|
| 1. サニタイズ層 — `sanitize_title` | `src-tauri/src/notifications.rs:145` | **変更しない**（NFR1）。CSI 除去 / C0・DEL・C1 除去 / 入力上限 / 100 文字トランケートは従来どおり |
| 2. 本文組み立て層 — `notification_body` / `agent_notification_body` | `src-tauri/src/notifications.rs` | 変更しない |
| 3. 送出層（シンク） — `NotifyRustSink::send` | `src-tauri/src/callbacks.rs:145` | **エスケープとケイパビリティ判定を追加する唯一の変更点** |
| 4. OS 層 — org.freedesktop.Notifications（D-Bus, Linux）/ Windows トースト | notify-rust 内部 | 変更しない |

## Shared Components

タスクは task0001 の 1 つのみ。タスク間で共有されるコンポーネント契約は無い。

## Conventions

- **ゲート規約（NFR3 / FR5）**: `callbacks.rs` は既に `gui` フィーチャー配下。新規に追加するケイパビリティ判定・エスケープ適用のコードは `#[cfg(unix)]` 配下に置き、Windows の送出フローは字面上も無変更に保つ。Unix 専用コードを検証するテストは既存規約（`#[cfg(all(test, unix))]` 相当）に従う。
- **エスケープ順序（FR1）**: `&` を最初に置換し、その後 `<` と `>` を置換する（生成済み実体参照の二重エスケープ防止）。

## Cross-task Design Decisions

### D1: 実装位置は案 (b) — `NotifyRustSink::send` の `.body()` 直前

SPEC が plan フェーズに委譲した 2 案（(a) `sanitize_title` 末尾 / (b) シンク直前）のうち **(b) を採用する**。

- **NFR2（チョークポイント）**: `NotifyRustSink::send` は D-Bus へ出る唯一の生産経路。`sanitize_title` を共有する 2 経路（タブアクティビティ: `src-tauri/src/app/mod.rs` 経由、エージェント: `src-tauri/src/app/agent_status.rs` 経由）に加え、OSC 9 通知（`callbacks.rs` の `handle_notify`）と `App::notify`（`src-tauri/src/window_host/link_hover.rs` の呼び出し元）由来の本文も同じ場所で覆う。迂回経路が構造的に存在しない。
- **NFR1 の保全**: `sanitize_title` / `notification_body` に手を入れないため、既存テスト期待値の変更が発生しない（TS6 が無変更で成立する）。
- **FR2 の自動成立**: 100 文字トランケートは上流（サニタイズ層）で完了しているため、シンクでのエスケープは常にトランケート後になる。
- **案 (a) を退けた理由**: ケイパビリティ判定は副作用を持つ Linux 固有処理であり、純関数である `sanitize_title` に判定結果を渡すには両経路の呼び出し元への状態の配管が必要になり変更範囲が広がる。既存テスト期待値の更新も必要になる。
- **影響範囲の拡大について**: 案 (b) により OSC 9 / link_hover 由来の本文も body-markup 対応サーバー上でエスケープされる。これは SPEC が案 (b) の説明で明示している効果であり、NFR2（全経路のチョークポイント）の要請に沿う。

対象タスク: task0001。

### D2: ケイパビリティは送出ごとに問い合わせ、キャッシュしない

通知サーバーは実行中に入れ替わり得るため、送出時点の `get_capabilities()` 応答を使う。頻度は既存スロットル（タブ 5 秒 / ペイン 30 秒 / 同一内容 1 秒 dedupe）で抑えられており、`.show()` 自体が既に同期 D-Bus 往復であるため追加コストは同等程度に収まる。通知経路の非同期化はスコープ外（REQUIREMENTS 9.2）。

対象タスク: task0001。

### D3: 純関数とアダプタの分離（テスト容易性）

- 「マークアップエスケープ変換」と「ケイパビリティ取得結果の解釈（成功時の一覧に `"body-markup"` が含まれるときのみ確認済み。失敗・不在は未確認）」をそれぞれ純関数として分離し、ユニットテストはこの 2 つを対象にする。
- 実際の D-Bus 呼び出しは `send` 内の薄いアダプタに留め、ユニットテストの対象外とする（D-Bus 実接続に依存させない。レビューで検査する）。

対象タスク: task0001。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 二重エスケープ（`&amp;` → 曖昧な結果） | 低 | 中 | `&` 先行の置換順序を TS2 で固定 |
| プレーンテキストサーバーで実体参照が生表示される | 低 | 中 | FR3 のケイパビリティゲート + TS4 |
| Windows / CLI（`--no-default-features`）ビルドの破壊 | 低 | 高 | cfg ゲート規約の遵守 + 2 種の check コマンド（TS8 / TS9） |
| 送出ごとの D-Bus 往復による遅延 | 低 | 低 | 既存スロットルで頻度制限。`.show()` と同等の同期往復であり増分は限定的 |

## Open Questions

- なし（FR1〜FR5、NFR1〜NFR3 はすべて resolved）。
