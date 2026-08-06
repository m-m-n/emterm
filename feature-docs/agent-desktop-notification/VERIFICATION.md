# Verification Document: agent-desktop-notification

## Overview

**Feature**: agent-desktop-notification / **SPEC.md**: `feature-docs/agent-desktop-notification/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/agent-desktop-notification/IMPLEMENTATION.md`

本書は verify フェーズが実行する統合検証を記述する。タスク単位の受け入れ基準は各 `tasks/taskNNNN.md` にある。

## Build Verification

- Rust: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Rust（CLI ビルド互換。`app_settings` は CLI ビルドにも含まれるため必須）: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- TypeScript: `bun run typecheck`
- Expected: いずれも exit code 0、エラーなし

## Test Verification

- Rust: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- TypeScript: `bun test`
- Coverage target: プロジェクトに数値カバレッジ基準は設定されていない。判定基準は下記 TS-1..TS-5 の全シナリオと各タスク Acceptance Criteria のテストがグリーンであること
- 既知の注意点: `tabs.rs` の replay テストは並列実行で非決定的に落ちることがある（プロジェクト既知事項）。本機能と無関係な失敗はベースライン（main）と突合して判定する

### Test Scenarios from SPEC.md

SPEC.md の TS1..TS4 に対応する（TS-5 は FR7 検証のため本書で追加）。

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | イベント種別ゲート: done / blocked の各遷移が対応トグル（`agent_notify_on_done` / `agent_notify_on_blocked`）を尊重し、相互に独立である（SPEC TS1） | OFF にした種別のみ発火しない。他方の種別は影響を受けない。上位段（全体・マスター）OFF なら種別値に関わらず発火しない | Unit (Rust, notifications.rs) |
| TS-2 | 新キーの serde 解決: missing / null / 明示 false（`app_settings` と GUI ランタイム設定ローダーの両方。SPEC TS2） | missing / null は既定 ON へ解決しデシリアライズ失敗なし。明示 false は false | Unit (Rust, app_settings + src-tauri/src/settings.rs) |
| TS-3 | 既存ゲート回帰: 可視ペイン抑止・30 秒/ペインのレート制限・working/idle 非発火・抑止時にレート制限窓を消費しない挙動（SPEC TS3） | 種別ゲート追加後もすべて既存どおり | Unit (Rust, notifications.rs / app.rs) |
| TS-4 | エージェントセクション UI: カテゴリ追加・マスター移設（通知カテゴリに重複なし）・種別トグル描画と保存・en/ja ラベル解決（SPEC TS4） | agent セクションが契約どおり描画・保存され、notification セクションからマスターが消え、両ロケールで訳文が解決される | Unit (TS, bun test) + `bun run typecheck` |
| TS-5 | pane 種別非依存: plain タブ形式・mux ペイン形式のキーで同一入力に対する発火判定が一致する | 両形式で判定結果が同一 | Unit (Rust, app.rs) |

## Code Quality Verification

- Format: 未設定（workflow.yaml の format_command は両コンポーネントとも空）
- Static analysis: 上記 Build Verification（cargo check / tsc）で兼ねる

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | 設定画面に「エージェント」カテゴリが存在しナビゲーションから開ける | TS-4 + 手動確認（下記） |
| AC-2 | ターン終了・質問待ち通知を個別 ON/OFF でき、OFF にした種別のみ発火しなくなる | TS-1 + TS-4 + 手動確認 |
| AC-3 | `notification_enabled` OFF なら種別設定に関わらず発火しない | TS-1 |
| AC-4 | マスター OFF なら種別設定に関わらず発火しない | TS-1 |
| AC-5 | 通知対象 hook は done・blocked の 2 種で全て（working/idle は対象外のまま） | TS-3（working/idle 非発火の回帰）+ SPEC で確認済みの設計判断 |
| AC-6 | 新キーの無い既存 `settings.json` が既定 ON に解決されデシリアライズが失敗しない | TS-2 + 手動確認 |
| AC-7 | plain タブ・mux ペインのどちらの遷移でも同一のイベント種別設定が適用される | TS-5 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0002 | TS-4 |
| FR2 | task0001, task0002 | TS-1, TS-4 |
| FR3 | task0001, task0002 | TS-1, TS-4 |
| FR4 | task0001 | TS-1, TS-3 |
| FR5 | task0002 | TS-4 |
| FR6 | task0001, task0002 | TS-2, TS-4 |
| FR7 | task0001 | TS-5 |
| NFR1 | task0001 | TS-3 |
| NFR2 | task0001 | TS-2 |
| NFR3 | task0002 | TS-4 |

## Manual Testing (E2E Not Possible)

プロジェクトに E2E 基盤は存在しないため、以下は文書化された手動確認項目とする。実機確認はリリースビルドで行う。

- [ ] 設定パネルを開き、ナビゲーションに「エージェント」カテゴリが表示され、選択するとマスター + 種別トグル 2 つが表示される（AC-1）。通知カテゴリにエージェントのマスタートグルが残っていない（FR5）
- [ ] 非表示のペイン（別タブ/別 mux ペイン）で `emterm agent-status done` を実行: ターン終了トグル ON なら通知が出て、OFF なら出ない。`emterm agent-status blocked` と質問待ちトグルでも同様（AC-2）
- [ ] 全体の通知設定 OFF、またはマスター OFF の状態では、種別トグルが ON でも通知が出ない（AC-3 / AC-4）
- [ ] 新キーを含まない既存 `settings.json` のままアプリを起動し、エラーなく起動して従来同等の通知挙動（種別トグルは ON 相当）になる（AC-6 / NFR2）
- [ ] UI 言語を en / ja で切り替え、カテゴリ名・トグルのラベルと説明文が両言語で表示される（NFR3）

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build（rust / cli-feature / ts） | 3 | 3 | 0 | 0 |
| Unit（TS-1..TS-5） | 5 | 5 | 0 | 0 |
| SPEC Success Criteria（AC-1..AC-7） | 7 | 7 | 0 | 5（自動確認の実機補完） |
| Manual | 5 | 0 | 0 | 5 |
