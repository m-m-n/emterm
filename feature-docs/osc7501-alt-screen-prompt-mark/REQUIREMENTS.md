---
title: "osc7501-alt-screen-prompt-mark"
created_date: 2026-10-10
status: draft
---

# osc7501-alt-screen-prompt-mark - 要件定義書

## 1. 概要

### 1.1 背景

起票元: Notion バグ「OSC 7501: 別画面の OSC 133 A が通常画面のプロンプト開始を消費し、working / blocked の記録が残る」（osc7501-program-status review round 1 medium, stable_id c07288af79f66c3b）。

現在の挙動:

- `apply_program_status_feed`（`src-tauri/src/tabs/output_pipeline.rs:188-208`）は、プロンプト開始候補（OSC 133 A）を種類だけで `live_marks` と前から突き合わせる。
- 同じ処理単位に「別画面の A → 別画面を出る → OSC 7501 報告 → 通常画面の A」が並ぶと、別画面の A が唯一の通常画面の A を消費し、報告より後ろの通常画面の A が無視される。
- その結果、working / blocked の記録が次のプロンプト開始まで残る。
- 候補は `callbacks.rs` の OSC 133 分岐で、別画面かどうかに関係なく積まれる。
- `term_core` は `MODE_ALT_SCREEN` 中の OSC 133 を live の印から除外する。

### 1.2 目的

同じ処理単位に、別画面の OSC 133 A、別画面からの復帰、OSC 7501 報告、通常画面の OSC 133 A がこの順で並んでも、報告より後ろの通常画面の A で working / blocked / idle の記録が破棄されるようにする。

### 1.3 スコープ

対象:

- plain タブの OSC 7501 経路（`apply_program_status_feed` と、そこへ候補を供給する部分）

対象外:

- エージェント状態ラッチ（`src-tauri/src/agent_status_model.rs` の `reconcile_latch_feed`、`LatchFeedEvent::PromptMark`）にある、種類だけを比べる同じ型の突き合わせの不具合
- mux デーモン側の OSC 7501 記録表

## 2. ビジネス要件

### 2.1 ビジネス目標

- 同じ処理単位に、別画面の OSC 133 A、別画面からの復帰、OSC 7501 報告、通常画面の OSC 133 A がこの順で並んでも、報告より後ろの通常画面の A で working / blocked / idle の記録が破棄されるようにする。

### 2.2 対象ユーザー

該当なし

### 2.3 期待される効果

- 1.2 の目的と同じ

## 3. ユースケース

該当なし

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | 別画面の候補を突き合わせから除外する | 別画面で受け取った OSC 133 の候補を OSC 7501 記録表への突き合わせに使わない | confirmed |
| FR2 | 通常画面の A を並び順どおりに適用する | 通常画面の OSC 133 A を報告との並び順どおりに記録表へ適用する | confirmed |
| FR3 | 既存の挙動を維持する | RIS・別画面の A・A 以外の印・mux 接続中の feed の扱いを変えない | confirmed |

### 4.2 機能詳細

#### FR1: 別画面の候補を突き合わせから除外する

別画面（`?47` / `?1047` / `?1049` で有効になる代替画面）で受け取った OSC 133 の候補は、種類や並び順に関係なく、OSC 7501 記録表への突き合わせに使わない。通常画面の印を消費しない。

#### FR2: 通常画面の A を並び順どおりに適用する

通常画面で受け取った OSC 133 A は、同じ処理単位に別画面の A が先にあっても、報告との並び順どおりに記録表へ適用する。working / blocked / idle の記録を破棄し、done / error の記録は残す。

#### FR3: 既存の挙動を維持する

次の既存の挙動は変えない。

- 同じ処理単位の最後の RIS より前の候補は適用しない。
- 別画面の A は記録を破棄しない。
- A 以外の印は何も破棄しない。
- 処理開始時点で mux に接続しているタブでは、内側の内容から作られた OSC 7501 の feed を捨てる。

## 5. 非機能要件

### 5.1 パフォーマンス要件

- NFR1（処理コスト）: 1 処理単位の feed の処理は feed の長さに比例する時間で終わり、ブロックせず、I/O も行わない。

### 5.2 セキュリティ要件

該当なし

### 5.3 可用性要件

該当なし

### 5.4 保守性要件

該当なし

### 5.5 互換性要件

- NFR2（他経路の挙動維持）: mux 接続中の経路と、OSC 7501 の照会への応答の挙動は変えない。

## 6. UI/UX要件

該当なし（UI 変更なし）

## 7. データ要件

該当なし

## 8. 外部連携

該当なし

## 9. 制約条件

### 9.1 技術的制約

- 別画面かどうかは `term_core` の `MODE_ALT_SCREEN`（`?47` / `?1047` / `?1049` で共通）と同じ基準で判定する。
- 既存の osc7501_* テストは名前を変えずに通す。

### 9.2 ビジネス上の制約

該当なし

### 9.3 スケジュール制約

該当なし

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/osc7501-alt-screen-prompt-mark/**`
- `test-docs/osc7501-alt-screen-prompt-mark/**`

`feature-docs/osc7501-alt-screen-prompt-mark/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/osc7501-alt-screen-prompt-mark/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/osc7501-alt-screen-prompt-mark/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

該当なし

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1（FR1, FR2）: 通常タブに `\x1b[?1049h\x1b]133;A\x07\x1b[?1049l\x1b]7501;id=job:state=working\x07\x1b]133;A\x07` を 1 回の `process_combined` で入力したあと、記録表は空で、保留中の要約変化は `[Some(working), None]` である。
- [ ] AC-2（FR1）: 別画面の切り替えに `?1047` または `?47` を使っても、AC-1 と同じ結果になる。
- [ ] AC-3（FR1, FR2）: 同じ処理単位で、通常画面の A、別画面の A、復帰、報告、通常画面の A がこの順に並んだとき、報告の記録は最後の通常画面の A で破棄される。
- [ ] AC-4（FR3, NFR2）: `src-tauri/src/tabs/tests/output_pipeline.rs` と `src-tauri/src/callbacks/tests.rs` にある既存の osc7501 / program_status_feed のテストが、名前を変えずに全部通る。
- [ ] AC-5（FR1, FR2, FR3）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が全部通り、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` も通る。

### 11.2 KPI

該当なし

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS-1（FR1, FR2）: AC-1 の再現バイト列を 1 回の `process_combined` に渡し、記録表が空になることと、保留中の要約変化が `[Some(working), None]` になることを確かめる。
- [ ] TS-2（FR1）: `?1047` と `?47` で別画面を切り替えた同じ並びを入力し、AC-1 と同じ結果になることを確かめる。
- [ ] TS-3（FR1, FR2）: 通常画面の A、`?1049h`、別画面の A、`?1049l`、working の報告、通常画面の A を 1 回で入力し、記録表が空になることを確かめる。
- [ ] TS-4（FR1）: 別画面の A を 2 つ、復帰、working の報告、通常画面の A を 1 回で入力し、記録表が空になることを確かめる。
- [ ] TS-5（FR3, NFR1, NFR2）: 既存の osc7501_* テストと program_status_feed テストを含む `--lib` 一式を実行し、全部通ることを確かめる。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 別画面 | `?47` / `?1047` / `?1049` で有効になる代替画面（`term_core` の `MODE_ALT_SCREEN`） |
| 通常画面 | 別画面ではない画面 |
| 処理単位 | 1 回の `process_combined` で処理される入力 |
| 記録表 | OSC 7501 の報告を記録する表 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1: 対象は plain タブの OSC 7501 経路（`apply_program_status_feed` と、そこへ候補を供給する部分）に限る。mux 接続中のタブで内側の内容から作られる feed を捨てる挙動は変えない。
- [x] A2: 別画面かどうかは `term_core` の `MODE_ALT_SCREEN`（`?47` / `?1047` / `?1049` で共通）と同じ基準で判定する。
- [x] A3: 既存の osc7501_* テストは名前を変えずに通す。再発検出のテストは新しく追加する。
- [x] A4: `reconcile_latch_feed` にある同じ型の不具合は、今回の対象外とする。
- [x] デザインステップ: スキップ（UI 変更なし）

### 14.2 未確認・保留事項

なし

## 15. 参考資料

- 起票元: Notion バグ「OSC 7501: 別画面の OSC 133 A が通常画面のプロンプト開始を消費し、working / blocked の記録が残る」（osc7501-program-status review round 1 medium, stable_id c07288af79f66c3b）
