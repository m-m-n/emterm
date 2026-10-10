---
title: "mux-upgrade-exited-pane-revision"
created_date: 2026-10-10
status: draft
---

# mux-upgrade-exited-pane-revision - 要件定義書

## 1. 概要

### 1.1 背景
mux hot-upgrade の `refresh_live_agent_state` は、終了済みペイン（master_fd なし、または exited）について OSC 7501 の program_records だけを書き直し、agent_state・agent_name・agent_revision はスナップショット時点の値のまま残す。スナップショット後に報告が適用されると、新しい記録と古い更新番号が並んで引き継がれる。

### 1.2 目的
- mux hot-upgrade で、終了済みペインの OSC 7501 記録・OSC 777 の状態と名前・更新番号を同じ時点の値として後継プロセスに引き継ぐ
- 復元後の再同期で、終了済みペインの最新の記録と更新番号が GUI に届く

### 1.3 スコープ
- 対象: `src-tauri/src/mux/upgrade.rs` の `refresh_live_agent_state` における終了済みペインの扱いと、その説明コメント
- 対象外: 「9.1 技術的制約」に記載の事項

## 2. ビジネス要件

### 2.1 ビジネス目標
- mux hot-upgrade で、終了済みペインの OSC 7501 記録・OSC 777 の状態と名前・更新番号を同じ時点の値として後継プロセスに引き継ぐ
- 復元後の再同期で、終了済みペインの最新の記録と更新番号が GUI に届く

### 2.2 対象ユーザー
該当なし（UI の変更は無い。mux daemon の hot-upgrade 引き継ぎ処理の修正のみ）

### 2.3 期待される効果
- 2.1 のとおり

## 3. ユースケース

該当なし（UI の変更は無い。mux daemon の hot-upgrade 引き継ぎ処理の修正のみ）

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 終了済みペインの状態・名前・更新番号・記録を引き継ぐ | 終了済みペインの state・name・revision・program_status を 1 回のロック取得の中で読み取り、ドキュメントに書き込む | - |
| FR2 | 終了済みペインのその他のフィールドは記録時のまま | FR1 の 4 項目以外のフィールドはスナップショット時の値のまま残す | - |
| FR3 | 生存ペインと mgr に無いペインの挙動は変えない | 生存ペインの再取得と、mgr で見つからないペインを変更しない挙動を保つ | - |
| FR4 | 復元後の再同期で最新の記録と更新番号が届く | restore した終了済みペインの AgentStatusUpdate が引き継いだ revision と記録の要約を載せる | - |
| FR5 | 説明コメントの更新 | `refresh_live_agent_state` の doc コメントと終了済み分岐の行内コメントを新しい挙動に合わせる | - |

### 4.2 機能詳細

#### FR1: 終了済みペインの状態・名前・更新番号・記録を引き継ぐ

**説明**: `refresh_live_agent_state` は、mgr に存在するペインのうち、ドキュメント上 master_fd が None のもの、または mgr 上で exited のものについて、`pane.agent_status` の state・name・revision・program_status を 1 回のロック取得の中で読み取り、ドキュメントの agent_state・agent_name・agent_revision・program_records に書き込む。

#### FR2: 終了済みペインのその他のフィールドは記録時のまま

**説明**: FR1 の対象ペインでは、agent_state・agent_name・agent_revision・program_records 以外のフィールド（id, cols, rows, cwd, title, exited, child_pid, master_fd, scrollback, latch_armed, latch_command_ended, latch_generation, alt_screen, alt_screen_dump）をスナップショット時の値のまま残す。

#### FR3: 生存ペインと mgr に無いペインの挙動は変えない

**説明**: 生存ペインの再取得（状態・名前・更新番号・記録・ラッチ・代替画面）と、mgr で見つからないペインを変更しない挙動は現状のまま保つ。

#### FR4: 復元後の再同期で最新の記録と更新番号が届く

**説明**: FR1 で更新したドキュメントから restore した終了済みペインは、引き継いだ revision と記録を持ち、`sync_agent_status_after_snapshot` / `sync_agent_status_after_pane_snapshot` がその revision と記録の要約を載せた AgentStatusUpdate を送る。

#### FR5: 説明コメントの更新

**説明**: `refresh_live_agent_state` の doc コメント（upgrade.rs 439-451 行）と終了済み分岐の行内コメント（467-470 行）を、終了済みペインでも状態・名前・更新番号・記録を同じロック内で引き継ぐ挙動に合わせて書き直す。

## 5. 非機能要件

### 5.1 パフォーマンス要件
該当なし

### 5.2 セキュリティ要件
該当なし

### 5.3 可用性要件
該当なし

### 5.4 保守性要件
- NFR1: 追加・変更するテストは --lib のユニットテストとして `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` で実行できる
- NFR2: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る

### 5.5 互換性要件
- HandoffDocument のスキーマ（HANDOFF_SCHEMA_VERSION と HandoffPane のフィールド）は変更しない（9.1 参照）

## 6. UI/UX要件

該当なし（UI の変更は無い）

## 7. データ要件

該当なし（HandoffDocument のスキーマは変更しない）

## 8. 外部連携

該当なし

## 9. 制約条件

### 9.1 技術的制約
以下は前提として扱う（いずれも取り消し可能）。
- 終了済みペインのラッチ（latch_armed / latch_command_ended / latch_generation）と代替画面（alt_screen / alt_screen_dump）は再取得の対象外とし、記録時の値を残す
- mgr で見つからないペインは従来どおり変更しない（既存テスト `refresh_live_agent_state_leaves_a_pane_no_longer_present_in_the_manager_untouched` が固定している）
- ドキュメント上は生存（master_fd が Some）だが mgr 上で exited になったペインの記述子と exited フラグの不一致は、この修正の対象外とする（`refresh_live_agent_state` の doc コメントが既存の別問題として記載）
- HandoffDocument のスキーマ（HANDOFF_SCHEMA_VERSION と HandoffPane のフィールド）は変更しない

### 9.2 ビジネス上の制約
該当なし

### 9.3 スケジュール制約
該当なし

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-upgrade-exited-pane-revision/**`
- `test-docs/mux-upgrade-exited-pane-revision/**`

`feature-docs/mux-upgrade-exited-pane-revision/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-upgrade-exited-pane-revision/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-upgrade-exited-pane-revision/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/mux-upgrade-exited-pane-revision/` ディレクトリを生成しないが、宣言された `test-docs/mux-upgrade-exited-pane-revision/**` は依然として正しい。

## 10. 想定される課題とリスク

該当なし

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1（FR1）: 更新番号 0 の生存ペインをスナップショットし、`apply_program_status_report` で error 報告を適用（revision 1）した後に `mark_exited` し `refresh_live_agent_state` を実行すると、ドキュメントの agent_revision は 1、program_records は error 記録を含み、ペインの現在の記録と一致する
- [ ] AC-2（FR1）: スナップショット後に終了済みペインへ OSC 777 の Set / Clear（`apply_agent_status_event`）を適用して `refresh_live_agent_state` を実行すると、ドキュメントの agent_state・agent_name・agent_revision がペインの現在値と一致する。スナップショット時点で既に終了済み（master_fd が None）のペインでも同じ
- [ ] AC-3（FR2）: AC-1 / AC-2 の終了済みペインで、agent_state・agent_name・agent_revision・program_records 以外のフィールドはスナップショット時の値と等しい
- [ ] AC-4（FR4）: AC-1 の手順で更新したドキュメントを restore し、`sync_agent_status_after_snapshot`（またはペイン単位の `sync_agent_status_after_pane_snapshot`）を呼ぶと、そのペインの AgentStatusUpdate が送られ、revision は 1、program_status の要約は error 記録を表す
- [ ] AC-5（FR3）: 既存の生存ペイン向け refresh テストと mgr に無いペインのテストが変更なしで通り、--lib テスト全体と --no-default-features の cargo check が通る

### 11.2 KPI
該当なし

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] TS-1（AC-1, AC-3）: `single_live_pane_manager` → snapshot（revision 0）→ `apply_program_status_report`(error) → `mark_exited` → `refresh_live_agent_state` → agent_revision == 1、program_records が現在のテーブルの export と一致、その他フィールドは記録時と一致
- [ ] TS-2（AC-2, AC-3）: 生存ペインに Set 適用 → snapshot → `mark_exited` → Clear 適用 → refresh → agent_state None・agent_name None・agent_revision がペインの現在値と一致、その他フィールドは記録時と一致（既存テスト `refresh_live_agent_state_leaves_a_pane_that_since_exited_untouched` の期待を置き換える）
- [ ] TS-3（AC-2, AC-3）: `single_exited_pane_manager` → snapshot → `apply_program_status_report` または `apply_agent_status_event` → refresh → 状態・名前・更新番号・記録がペインの現在値と一致
- [ ] TS-4（AC-4）: TS-1 の手順で得たドキュメントを restore → `Arc<Mutex<SessionManager>>` に包んで `sync_agent_status_after_snapshot` を呼び、notify_tx の購読側で受けた AgentStatusUpdate の revision が 1、program_status 要約が error を表すことを確認
- [ ] TS-5（AC-5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` の実行

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 終了済みペイン | mgr に存在するペインのうち、ドキュメント上 master_fd が None のもの、または mgr 上で exited のもの |

## 14. 確認事項

### 14.1 確認済み事項
なし

### 14.2 未確認・保留事項
なし

## 15. 参考資料

- 該当箇所: `src-tauri/src/mux/upgrade.rs:466`
