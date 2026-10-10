# Feature: mux-upgrade-exited-pane-revision

## Overview

mux hot-upgrade の `refresh_live_agent_state` で、終了済みペインの OSC 7501 記録・OSC 777 の状態と名前・更新番号を同じ agent_status ロック内で取得して引き継ぐ。復元後の再同期で、終了済みペインの最新の記録と更新番号を GUI に届ける。要件の詳細は `feature-docs/mux-upgrade-exited-pane-revision/REQUIREMENTS.md` を参照。

## Objectives

- mux hot-upgrade で、終了済みペインの OSC 7501 記録・OSC 777 の状態と名前・更新番号を同じ時点の値として後継プロセスに引き継ぐ
- 復元後の再同期で、終了済みペインの最新の記録と更新番号が GUI に届く

## User Stories

ユーザーストーリーは定義しない（UI の変更は無い）。受け入れ基準は以下。

**Acceptance Criteria:**
- [ ] AC-1（FR1）: 更新番号 0 の生存ペインをスナップショットし、`apply_program_status_report` で error 報告を適用（revision 1）した後に `mark_exited` し `refresh_live_agent_state` を実行すると、ドキュメントの agent_revision は 1、program_records は error 記録を含み、ペインの現在の記録と一致する
- [ ] AC-2（FR1）: スナップショット後に終了済みペインへ OSC 777 の Set / Clear（`apply_agent_status_event`）を適用して `refresh_live_agent_state` を実行すると、ドキュメントの agent_state・agent_name・agent_revision がペインの現在値と一致する。スナップショット時点で既に終了済み（master_fd が None）のペインでも同じ
- [ ] AC-3（FR2）: AC-1 / AC-2 の終了済みペインで、agent_state・agent_name・agent_revision・program_records 以外のフィールドはスナップショット時の値と等しい
- [ ] AC-4（FR4）: AC-1 の手順で更新したドキュメントを restore し、`sync_agent_status_after_snapshot`（またはペイン単位の `sync_agent_status_after_pane_snapshot`）を呼ぶと、そのペインの AgentStatusUpdate が送られ、revision は 1、program_status の要約は error 記録を表す
- [ ] AC-5（FR3）: 既存の生存ペイン向け refresh テストと mgr に無いペインのテストが変更なしで通り、--lib テスト全体と --no-default-features の cargo check が通る

## Technical Requirements

### Functional Requirements
- **FR1:** 終了済みペインの状態・名前・更新番号・記録を引き継ぐ — `refresh_live_agent_state` は、mgr に存在するペインのうち、ドキュメント上 master_fd が None のもの、または mgr 上で exited のものについて、`pane.agent_status` の state・name・revision・program_status を 1 回のロック取得の中で読み取り、ドキュメントの agent_state・agent_name・agent_revision・program_records に書き込む
- **FR2:** 終了済みペインのその他のフィールドは記録時のまま — FR1 の対象ペインでは、agent_state・agent_name・agent_revision・program_records 以外のフィールド（id, cols, rows, cwd, title, exited, child_pid, master_fd, scrollback, latch_armed, latch_command_ended, latch_generation, alt_screen, alt_screen_dump）をスナップショット時の値のまま残す
- **FR3:** 生存ペインと mgr に無いペインの挙動は変えない — 生存ペインの再取得（状態・名前・更新番号・記録・ラッチ・代替画面）と、mgr で見つからないペインを変更しない挙動は現状のまま保つ
- **FR4:** 復元後の再同期で最新の記録と更新番号が届く — FR1 で更新したドキュメントから restore した終了済みペインは、引き継いだ revision と記録を持ち、`sync_agent_status_after_snapshot` / `sync_agent_status_after_pane_snapshot` がその revision と記録の要約を載せた AgentStatusUpdate を送る
- **FR5:** 説明コメントの更新 — `refresh_live_agent_state` の doc コメント（upgrade.rs 439-451 行）と終了済み分岐の行内コメント（467-470 行）を、終了済みペインでも状態・名前・更新番号・記録を同じロック内で引き継ぐ挙動に合わせて書き直す

### Non-Functional Requirements
- **NFR1:** 追加・変更するテストは --lib のユニットテストとして `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` で実行できる
- **NFR2:** `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る

## Implementation Approach

### Architecture

変更箇所は `src-tauri/src/mux/upgrade.rs` の `refresh_live_agent_state` の終了済みペイン分岐（`src-tauri/src/mux/upgrade.rs:466`）とその説明コメント。

### Data Flow

```
snapshot → (報告の適用) → refresh_live_agent_state → HandoffDocument → restore → sync_agent_status_after_snapshot / sync_agent_status_after_pane_snapshot → AgentStatusUpdate
```

### Assumptions

- 終了済みペインのラッチ（latch_armed / latch_command_ended / latch_generation）と代替画面（alt_screen / alt_screen_dump）は再取得の対象外とし、記録時の値を残す
- mgr で見つからないペインは従来どおり変更しない（既存テスト `refresh_live_agent_state_leaves_a_pane_no_longer_present_in_the_manager_untouched` が固定している）
- ドキュメント上は生存（master_fd が Some）だが mgr 上で exited になったペインの記述子と exited フラグの不一致は、この修正の対象外とする（`refresh_live_agent_state` の doc コメントが既存の別問題として記載）
- HandoffDocument のスキーマ（HANDOFF_SCHEMA_VERSION と HandoffPane のフィールド）は変更しない

### API Design

該当なし

### Database Schema

該当なし（HandoffDocument のスキーマは変更しない）

### Dependencies

**Internal Dependencies:**
- `refresh_live_agent_state`、`sync_agent_status_after_snapshot` / `sync_agent_status_after_pane_snapshot`、`pane.agent_status`

**External Dependencies:**
- なし

### File Structure

```
src-tauri/src/mux/
└── upgrade.rs
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-upgrade-exited-pane-revision/**`
- `test-docs/mux-upgrade-exited-pane-revision/**`

`feature-docs/mux-upgrade-exited-pane-revision/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-upgrade-exited-pane-revision/**` covers `test-docs/mux-upgrade-exited-pane-revision/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mux-upgrade-exited-pane-revision/` directory at all; the declared
`test-docs/mux-upgrade-exited-pane-revision/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1（AC-1, AC-3）: `single_live_pane_manager` → snapshot（revision 0）→ `apply_program_status_report`(error) → `mark_exited` → `refresh_live_agent_state` - agent_revision == 1、program_records が現在のテーブルの export と一致、その他フィールドは記録時と一致
- [ ] TS-2（AC-2, AC-3）: 生存ペインに Set 適用 → snapshot → `mark_exited` → Clear 適用 → refresh - agent_state None・agent_name None・agent_revision がペインの現在値と一致、その他フィールドは記録時と一致（既存テスト `refresh_live_agent_state_leaves_a_pane_that_since_exited_untouched` の期待を置き換える）
- [ ] TS-3（AC-2, AC-3）: `single_exited_pane_manager` → snapshot → `apply_program_status_report` または `apply_agent_status_event` → refresh - 状態・名前・更新番号・記録がペインの現在値と一致
- [ ] TS-4（AC-4）: TS-1 の手順で得たドキュメントを restore → `Arc<Mutex<SessionManager>>` に包んで `sync_agent_status_after_snapshot` を呼ぶ - notify_tx の購読側で受けた AgentStatusUpdate の revision が 1、program_status 要約が error を表す

### Integration Tests
- [ ] TS-5（AC-5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` の実行 - いずれも通る

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] スナップショット時点で既に終了済み（master_fd が None）のペイン - AC-2 / TS-3 のとおり、状態・名前・更新番号・記録がペインの現在値と一致する
- [ ] mgr で見つからないペイン - 変更しない（FR3）

### Performance Tests
該当なし

## Security Considerations

該当なし

## Error Handling

該当なし

## Performance Optimization

該当なし

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし

## References

- 要件定義書: `feature-docs/mux-upgrade-exited-pane-revision/REQUIREMENTS.md`
- 該当箇所: `src-tauri/src/mux/upgrade.rs:466`
