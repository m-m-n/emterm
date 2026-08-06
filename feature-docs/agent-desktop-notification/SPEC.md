# Feature: agent-desktop-notification

## Overview

エージェントの状態変化（ターン終了＝done 遷移、質問待ち＝blocked 遷移）のデスクトップ通知を、イベント種別ごとに個別 ON/OFF できるようにする。あわせて設定画面に「エージェント」カテゴリを新設し、既存のエージェント通知マスター（`agent_status_notifications`）をそこへ移設して、その配下にイベント種別トグルを置く。通知源は既存の hook ベース機構（OSC 777 agent-status / `emterm agent-status` CLI）をそのまま用い、並行機構は新設しない。

要件の原典は同ディレクトリの `REQUIREMENTS.md`（日本語・要件定義書）であり、本書はその実装向けの記述である。

## Objectives

- エージェントの状態変化（ターン終了・質問待ち）をイベント種別ごとに個別設定可能なデスクトップ通知として受け取れるようにする
- 設定画面に「エージェント」メニューを新設し、エージェント関連の通知設定を一箇所に集約する

## User Stories

### US1: エージェント通知をイベント種別ごとに設定する

エージェント利用者として、設定画面の「エージェント」カテゴリでターン終了・質問待ちの通知をそれぞれ ON/OFF したい。必要な種別の通知だけを受け取るため。

**Acceptance Criteria:**
- [ ] AC-1: 設定画面に「エージェント」メニュー（カテゴリ）が存在し、ナビゲーションから開ける
- [ ] AC-2: ターン終了（done 遷移）通知と質問待ち（blocked 遷移）通知をそれぞれ設定画面から個別に ON/OFF でき、OFF にした種別のみ発火しなくなる
- [ ] AC-6: 新キーが存在しない既存 `settings.json` を読み込んでもデフォルト値（ON）に解決され、デシリアライズが失敗しない

### US2: エージェント状態変化の通知を受け取る

エージェント利用者として、hook が報告した状態遷移に対して、階層ゲートを通過したときだけ通知を受け取りたい。上位の通知設定と一貫した挙動にするため。

**Acceptance Criteria:**
- [ ] AC-3: 全体の通知設定（`notification_enabled`）が OFF のとき、イベント種別設定に関わらずエージェント通知は発火しない（階層ゲート）
- [ ] AC-4: エージェント通知マスターが OFF のとき、イベント種別設定に関わらずエージェント通知は発火しない
- [ ] AC-5: 現行プロトコル（idle/working/blocked/done）の範囲で通知対象となる hook は done・blocked の 2 種で全てであることを確認済み（working/idle は既存設計で通知対象外）。よって追加項目なし
- [ ] AC-7: plain タブと mux ペインのどちらの遷移でも同一のイベント種別設定が適用される

## Technical Requirements

### Functional Requirements

- **FR1:** 設定画面に「エージェント」カテゴリを新設する — 設定パネル（`src-tauri/web-shared/settings/settings-panel.ts` の categories ゲッター）に新カテゴリ「エージェント」を追加する。既存カテゴリと同様に `CATEGORY_ICONS` への 24px SVG アイコン追加、i18n ラベル（`web-shared/i18n/locales/{en,ja}.json` の `settings.categories.*`）、セクションレンダラー（`settings/sections/` 配下）を備える。
- **FR2:** ターン終了通知の個別ON/OFF — エージェント状態が done へ遷移したとき（既存 OSC 777 agent-status プロトコルの done 状態＝ターン終了に対応する hook）のデスクトップ通知を、「エージェント」カテゴリ内のトグルで個別に ON/OFF できる。
- **FR3:** 質問待ち通知の個別ON/OFF — エージェント状態が blocked へ遷移したとき（質問待ち・許可待ちに対応する hook）のデスクトップ通知を、「エージェント」カテゴリ内のトグルで個別に ON/OFF できる。
- **FR4:** 全体通知設定を頂点とする階層ゲート — 通知発火は「全体の通知設定（`notification_enabled`）→ エージェント通知マスター（既存 `agent_status_notifications`）→ イベント種別ごとの個別トグル」の階層で全段 ON のときのみ行う。既存 `notifications.rs::should_fire_agent_notification` のゲート（pane 非可視条件・30 秒/ペインのレート制限）は維持し、イベント種別ゲートを追加する。
- **FR5:** エージェント通知マスターの「エージェント」カテゴリへの移設 — 既存の `agent_status_notifications` トグル（現在 `notification-section.ts` 87-98 行目で通知カテゴリに表示）を「エージェント」カテゴリへ移設し、その配下にイベント種別ごとの個別トグルを配置する。通知カテゴリ側の重複表示は行わない。
- **FR6:** 設定スキーマの追加（Settings Pattern 準拠） — イベント種別ごとの新設定キーを `crates/app_settings/src/settings.rs` に `serde(default)` 付きで追加し、`src-tauri/web-shared/settings/types.ts` の `AppSettings` ミラー・セクションレンダラー・i18n キー（en/ja）を揃える。既存 `settings.json`（新キー欠落・null）はデフォルト値に解決される。
- **FR7:** plain タブ・mux ペイン両対応 — hook ベース（OSC 777 / `emterm agent-status` CLI）の既存機構をそのまま通知源とするため、plain タブ（`PaneKey::Tab`）と mux ペイン（`PaneKey::MuxPane`）の双方で同一のイベント種別設定が適用される。既存 `AgentStatusModel` の遷移キューを流用し、並行機構は新設しない。

### Non-Functional Requirements

- **NFR1 - 既存通知ゲートの互換維持:** pane 可視時の抑止、30 秒/ペインのレート制限（`AGENT_NOTIFICATION_RATE_LIMIT`）、抑止された通知はキューせず破棄する既存挙動を変更しない。
- **NFR2 - 後方互換:** 既存ユーザーの `settings.json` は無変更で従来と同等の通知挙動を得る（新イベント種別トグルのデフォルトは ON、既存 `agent_status_notifications` の値は保持）。
- **NFR3 - i18n:** 新設 UI ラベル・説明文は en/ja 両ロケールを持つ（子 WebView は `web-shared/i18n/locales/{en,ja}.json`、通知本文は `notifications.rs` の `Locale` 分岐）。

## Implementation Approach

### Architecture

**System Architecture:**

```
┌──────────────────────────────────────────────────────┐
│ hook (emterm agent-status CLI / OSC 777 agent-status) │
├──────────────────────────────────────────────────────┤
│ AgentStatusModel（既存の遷移キュー）                  │
│   PaneKey::Tab / PaneKey::MuxPane                     │
├──────────────────────────────────────────────────────┤
│ notifications.rs                                      │
│   should_fire_agent_notification                      │
│   ├ 全体の通知設定 notification_enabled               │
│   ├ エージェント通知マスター agent_status_notifications│
│   ├ イベント種別トグル（done / blocked）← 追加        │
│   ├ pane 非可視条件（既存）                           │
│   └ 30 秒/ペインのレート制限（既存）                  │
├──────────────────────────────────────────────────────┤
│ デスクトップ通知                                      │
└──────────────────────────────────────────────────────┘

設定側:
  crates/app_settings/src/settings.rs（serde(default) の新キー）
    ↕ ミラー
  src-tauri/web-shared/settings/types.ts（AppSettings）
    ↕
  settings-panel.ts（categories / CATEGORY_ICONS）
  settings/sections/（「エージェント」セクションレンダラー）
  web-shared/i18n/locales/{en,ja}.json（ラベル・説明文）
```

**Component Diagram:**

- 通知源: 既存 hook 機構（新設なし。FR7）
- 状態遷移: 既存 `AgentStatusModel` の遷移キュー（FR7）
- 発火判定: `notifications.rs` のゲート関数にイベント種別ゲートを追加（FR4）
- 設定: `app_settings` の新キーと、その TS ミラー・UI・i18n（FR1 / FR5 / FR6 / NFR3）

### Data Flow

```
hook → OSC 777 agent-status → AgentStatusModel（遷移: done / blocked）
     → ゲート判定（全体 → マスター → イベント種別 → pane 可視 → レート制限）
     → 全段通過時のみデスクトップ通知
     → 抑止時はキューせず破棄（NFR1）
```

### Settings Schema

| キー | 型 | 既定値 | 備考 |
|------|-----|--------|------|
| `notification_enabled`（既存） | bool | 既存値を保持 | 階層ゲート最上段 |
| `agent_status_notifications`（既存） | bool | 既存値を保持 | 階層ゲート中段。キー名は互換のため維持（FR5） |
| ターン終了（done 遷移）通知トグル（新設） | bool | ON | FR2 / FR6。`serde(default)` |
| 質問待ち（blocked 遷移）通知トグル（新設） | bool | ON | FR3 / FR6。`serde(default)` |

新キーが欠落・null の既存 `settings.json` はデフォルト値（ON）に解決され、デシリアライズは失敗しない（FR6 / NFR2 / AC-6）。

### Dependencies

**Internal Dependencies:**
- `crates/app_settings`（`settings.rs`）: 設定スキーマの追加先
- `src-tauri/src/notifications.rs`: `should_fire_agent_notification`、`AGENT_NOTIFICATION_RATE_LIMIT`、`is_qualifying_agent_state`、通知本文の `Locale` 分岐
- `AgentStatusModel`: 既存の遷移キュー（plain タブ / mux ペイン）
- `src-tauri/web-shared/settings/`: `settings-panel.ts`、`types.ts`、`sections/`
- `src-tauri/web-shared/i18n/locales/{en,ja}.json`: UI ラベル

**External Dependencies:**
- OSC 777 agent-status プロトコル / `emterm agent-status` CLI（既存。本機能で拡張しない）

### File Structure

```
crates/app_settings/src/settings.rs                     # 新設定キー（serde(default)）+ serde 解決テスト
src-tauri/src/notifications.rs                          # イベント種別ゲート追加 + #[cfg(test)]
src-tauri/web-shared/settings/settings-panel.ts         # categories ゲッター / CATEGORY_ICONS
src-tauri/web-shared/settings/sections/                 # 「エージェント」セクションレンダラー
src-tauri/web-shared/settings/sections/notification-section.ts  # マスタートグルの移設（重複表示なし）
src-tauri/web-shared/settings/types.ts                  # AppSettings ミラー
src-tauri/web-shared/i18n/locales/{en,ja}.json          # settings.categories.* ほかラベル
```

## Test Scenarios

### Unit Tests

- [ ] TS1: `should_fire_agent_notification` 相当のゲート関数がイベント種別トグル（done/blocked 個別）を尊重する（既存 AC-1..AC-4 テスト群のパターンを踏襲、`src-tauri/src/notifications.rs` の `#[cfg(test)]`） — FR2 / FR3 / FR4
- [ ] TS2: `app_settings` の新キーについて missing/null/明示 false の serde 解決テスト（`agent_status_notifications` の既存テスト 1042-1067 行のパターンを踏襲） — FR6 / NFR2
- [ ] TS3: 可視ペイン抑止・レート制限が種別ゲート追加後も既存どおり機能する回帰テスト — FR4 / NFR1
- [ ] TS4: エージェントセクションのレンダラーテスト（`settings/sections/*.test.ts` のパターン、`bun test`）と `bun run typecheck` — FR1 / FR5 / FR6 / NFR3

### Integration Tests

投入資料に記載なし。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Run Commands

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
bun test
bun run typecheck
```

### Edge Cases

- [ ] 新キーを持たない既存 `settings.json`: デフォルト値（ON）に解決し、デシリアライズを失敗させない（AC-6）
- [ ] 上位段が OFF: 全体の通知設定 OFF、またはエージェント通知マスター OFF のとき、イベント種別設定に関わらず発火しない（AC-3 / AC-4）
- [ ] pane 可視時・レート制限内の遷移: 抑止し、キューせず破棄する（NFR1）
- [ ] plain タブ（`PaneKey::Tab`）と mux ペイン（`PaneKey::MuxPane`）: 同一のイベント種別設定が適用される（AC-7）

## Security Considerations

投入資料に記載なし（本機能はローカル設定値の追加と発火判定の分岐追加であり、認証・認可・外部入力の新規経路を持たない）。

## Error Handling

設定デシリアライズ時に新キーが欠落・null の場合は `serde(default)` によりデフォルト値（ON）へ解決し、エラーとしない（FR6 / AC-6）。他のエラー経路は投入資料に記載なし。

## Success Criteria

- [ ] AC-1: 設定画面に「エージェント」メニュー（カテゴリ）が存在し、ナビゲーションから開ける
- [ ] AC-2: ターン終了（done 遷移）通知と質問待ち（blocked 遷移）通知をそれぞれ設定画面から個別に ON/OFF でき、OFF にした種別のみ発火しなくなる
- [ ] AC-3: 全体の通知設定（`notification_enabled`）が OFF のとき、イベント種別設定に関わらずエージェント通知は発火しない（階層ゲート）
- [ ] AC-4: エージェント通知マスターが OFF のとき、イベント種別設定に関わらずエージェント通知は発火しない
- [ ] AC-5: 現行プロトコル（idle/working/blocked/done）の範囲で通知対象となる hook は done・blocked の 2 種で全てであることを確認済み（working/idle は既存設計で通知対象外）。よって追加項目なし
- [ ] AC-6: 新キーが存在しない既存 `settings.json` を読み込んでもデフォルト値（ON）に解決され、デシリアライズが失敗しない
- [ ] AC-7: plain タブと mux ペインのどちらの遷移でも同一のイベント種別設定が適用される
- [ ] FR1..FR7 / NFR1..NFR3 が実装され、テストで確認されている
- [ ] TS1..TS4 のテストシナリオが通る

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

`status: tbd` の要件はない（FR1..FR7 / NFR1..NFR3 はすべて `resolved`）。

なお、以下は batch モードのため、ユーザー確認なしに投入資料からの推定で確定した前提である。

- 「ターン終了」= done 状態への遷移、「質問待ち」= blocked 状態への遷移（既存 OSC 777 agent-status プロトコルの 4 状態 idle/working/blocked/done に対する対応付け。ユーザーの hook が `emterm agent-status` CLI でこれらを報告する前提）
- 「ターン終了・質問待ち以外の通知すべき hook」は現行プロトコルの語彙内には存在しない（working/idle は `is_qualifying_agent_state` で通知対象外とする既存設計判断があり、頻度的にもノイズになる）。プロトコル拡張（新状態の追加）は本機能のスコープ外
- 既存 `agent_status_notifications` トグルは通知カテゴリから「エージェント」カテゴリへ移設する（重複表示しない）。設定キー名は互換のため維持
- 新設イベント種別トグルのデフォルトは ON（既存 `agent_status_notifications` のデフォルト true と整合し、既存ユーザーの通知挙動を変えない）

## Design Step

デザインステップは skipped。理由: 設定パネルへのカテゴリ・トグル追加のみで、既存コンポーネントと MD3 トークン体系を踏襲する定型作業のため、独立した設計ステップは不要。

## References

- 要件定義書: `feature-docs/agent-desktop-notification/REQUIREMENTS.md`
- `src-tauri/src/notifications.rs`
- `src-tauri/web-shared/settings/settings-panel.ts`
- `src-tauri/web-shared/settings/sections/notification-section.ts`
- `src-tauri/web-shared/settings/types.ts`
- `crates/app_settings/src/settings.rs`
- `src-tauri/web-shared/i18n/locales/{en,ja}.json`
