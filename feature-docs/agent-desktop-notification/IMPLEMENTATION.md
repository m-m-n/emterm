# Implementation Plan: agent-desktop-notification

## Overview

エージェント状態遷移（done / blocked）のデスクトップ通知をイベント種別ごとに個別 ON/OFF できるようにし、設定画面に「エージェント」カテゴリを新設して既存マスタートグルを移設する。既存の hook ベース通知機構（OSC 777 / `emterm agent-status` CLI → `AgentStatusModel` → `notifications.rs` ゲート）への追加のみで、並行機構は新設しない。

## Technology Stack

- **Rust**: 設定スキーマ（`crates/app_settings` と GUI ランタイム `src-tauri/src/settings.rs`）、通知ゲート（`src-tauri/src/notifications.rs` / `src-tauri/src/app.rs`）
- **TypeScript (vanilla)**: 子 WebView 設定パネル（`src-tauri/web-shared/settings/`）と i18n（`web-shared/i18n/locales/{en,ja}.json`）
- **新規依存**: なし（既存クレート・既存モジュールのみ。`project.license: MIT` に対するライセンス確認対象の新規依存は存在しない）

## Layer Structure

| レイヤ | 責務 | 依存方向 |
|---|---|---|
| 設定スキーマ（`app_settings` / GUI ランタイム設定） | 新キーの永続化・デフォルト解決・後方互換 | 他レイヤに依存しない（CLI ビルドにも含まれる） |
| 通知ゲート（`notifications.rs` / `app.rs`） | 階層ゲート判定（純粋関数）と発火 | 設定スキーマの値を読む |
| 設定 UI（`web-shared/settings/` + i18n） | カテゴリ・トグルの表示と保存 | TS ミラー（`types.ts`）経由で設定キーを参照 |

タスク分割はこのレイヤ境界に一致する: task0001 = Rust 側（スキーマ + ゲート）、task0002 = TS 側（UI + ミラー + i18n）。両タスクのファイル集合は互いに素。

## Shared Components

Rust / TS の両タスクが同じ名前を実装する設定キーの契約。**キー名はこの表が SSOT**。

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| 設定キー `agent_notify_on_done` | ターン終了（done 遷移）通知の個別トグル | bool。既定 ON。キー欠落・null は既定値へ解決しデシリアライズを失敗させない | task0001（スキーマ + ゲート入力）, task0002（TS ミラー + トグル UI） |
| 設定キー `agent_notify_on_blocked` | 質問待ち（blocked 遷移）通知の個別トグル | 同上 | task0001, task0002 |
| 既存キー `agent_status_notifications` | エージェント通知マスター（階層ゲート中段） | キー名・型・既定値（ON）・serde 解決挙動を一切変更しない | task0001（ゲート中段として参照継続）, task0002（UI を通知カテゴリから「エージェント」カテゴリへ移設） |
| 既存キー `notification_enabled` | 全体通知設定（階層ゲート最上段） | 変更しない。UI も通知カテゴリに残置 | task0001（参照のみ） |

## Conventions

- **Settings Pattern 準拠**（プロジェクト既存規約）: Rust 側は既存 `agent_status_notifications` フィールドと同一の宣言パターン（既定 true・null は既定値へ解決）を踏襲し、TS 側は `types.ts` の `AppSettings` に同名キーをミラーし、i18n ラベルは en/ja 両ロケールに揃える。
- **キー命名**: 既存 `notify_on_*` 系に合わせた snake_case。エージェント配下であることを `agent_` 接頭辞で表す。
- **i18n**: 新設 UI 文言は en/ja 両方（NFR3）。通知本文（`notifications.rs` の Locale 分岐）は変更しない。

## Cross-task Design Decisions

### 1. イベント種別トグルの語彙と対応付け

「ターン終了」= done 状態への遷移、「質問待ち」= blocked 状態への遷移（REQUIREMENTS.md 13 章の用語定義どおり）。working / idle は既存設計（`is_qualifying_agent_state`）のまま通知対象外であり、種別トグルも設けない。task0001 のゲート実装と task0002 のトグル説明文はこの対応付けに一致させる。

**影響タスク**: task0001, task0002

### 2. 階層ゲートは既存純粋判定関数への追加段として実装する

発火条件は「対象状態（blocked/done）∧ pane 非可視 ∧ `notification_enabled` ∧ `agent_status_notifications` ∧ 対象状態に対応する種別トグル ∧ レート制限外」の論理積。既存の可視抑止・30 秒/ペインのレート制限・「抑止時はレート制限窓を消費しない（キューせず破棄）」のセマンティクスには手を入れない（NFR1）。判定は引き続き無変異の純粋関数に閉じる。task0002 の説明文が示す挙動（「OFF にした種別のみ発火しなくなる」）はこの契約が根拠。

**影響タスク**: task0001（実装）, task0002（UI 文言の整合）

### 3. マスタートグルは移設のみ・キー名維持

`agent_status_notifications` は UI 表示位置のみ「エージェント」カテゴリへ移し（通知カテゴリ側の重複表示なし・FR5）、設定キー名と挙動は互換のため維持する（NFR2）。

**影響タスク**: task0002（UI 移設）, task0001（キー名を変えないことが前提）

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Rust ↔ TS のキー名ドリフト（別タスクで並行実装） | Low | Medium | Shared Components 表を名前の SSOT とし、両タスク plan から参照。レビューで突合 |
| 種別ゲート追加による既存ゲートの回帰 | Low | High | 回帰テスト（TS-3）を必須化。既存テスト群をグリーンのまま維持 |
| TS `AppSettings` への必須キー追加で既存テストフィクスチャが型エラーになる | High | Low | フィクスチャ更新対象（既存セクションテスト 2 ファイル）を task0002 の files に含めて明示 |

## Open Questions

なし（TBD 要件なし・新規依存なし・既存計画ファイルなし）。
