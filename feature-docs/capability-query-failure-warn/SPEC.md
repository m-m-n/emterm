# Feature: capability-query-failure-warn

## Overview

Unix の通知ワーカー（`src-tauri/src/callbacks.rs` の `notify_worker`）で capability 照会（`notify_rust::get_capabilities()`）が失敗したとき、`log::warn!` で emterm.log に記録を残す。warn は状態遷移時だけ出し、同じ失敗が続くときはログを冗長にしない。エスケープの判定と出力（fail-closed 方針）は変えない。

要件の詳細は [REQUIREMENTS.md](REQUIREMENTS.md) を参照する。

## Objectives

- Unix の `notify_worker` で capability 照会が失敗したとき、`log::warn!` で emterm.log に記録を残す。
- 「通知に HTML やエンティティがそのまま出る」「通知が常にエスケープされる」という報告を、capability 照会の失敗、サーバーが body-markup に対応していない場合、エスケープ側の不具合のどれに当たるか、ログから切り分けられるようにする。
- 同じ失敗が続くときにログを冗長にしない。warn は状態遷移時だけ出す。

## User Stories

### US1: 通知表示の報告をログから切り分ける
通知の不具合を調べる人として、emterm.log で capability 照会の失敗を確認したい。そうすれば、通知表示に関する報告が照会の失敗、body-markup 非対応、エスケープ側の不具合のどれに当たるか切り分けられる。

**Acceptance Criteria:**
- [ ] AC1: 初期状態で照会が Err を返すと、warn 判定ヘルパーは warn する。(FR2, FR3)
- [ ] AC2: 直前の失敗と同じエラー文字列で再び Err になると、warn しない。(FR2)
- [ ] AC3: 直前の失敗と異なるエラー文字列で Err になると、warn する。(FR2)
- [ ] AC4: Ok の後に Err になると warn する。Ok 自体では warn しない。(FR2)
- [ ] AC5: メタ文字を含まない通知は照会せず、warn 判定の状態を変えない。前後の失敗が同じエラー文字列なら、間にこの通知が挟まっても 2 回目は warn しない。(FR1, FR2)
- [ ] AC6: warn の文面は FR4 の内容だけから作られ、通知のタイトルと本文から得た文字列を含まない。(FR4)

### US2: 既存の通知動作を維持する
通知を受け取る利用者として、照会失敗時のエスケープ動作が現行どおりであってほしい。

**Acceptance Criteria:**
- [ ] AC7: 照会が失敗したときは、現行どおりタイトルと本文の両方がエスケープされて送出される。既存のエスケープ関連テストは、期待値を変えずにすべて通る。(NFR1, NFR6)
- [ ] AC8: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。
- [ ] AC9: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。(NFR4)

## Technical Requirements

### Functional Requirements
- **FR1:** 照会結果を一度だけ束縛する — Unix の `notify_worker` では、1 通知あたり capability 照会の結果を 1 回だけ束縛する。エスケープ判定と warn 判定は、その同じ 1 つの結果を使う。照会は現行どおりオンデマンドとする。タイトルと本文のどちらにも `&`、`<`、`>` が含まれない通知では照会しない。含まれる通知では、1 通知につき高々 1 回照会する。
- **FR2:** 照会失敗時の warn（状態遷移時のみ） — 照会が Err を返したとき、次のいずれかに当たる場合だけ `log::warn!` を 1 件出す。(a) ワーカーが始まってから最初の失敗、(b) 直前の失敗とエラー文字列が異なる、(c) 直前の照会が成功していた。直前の失敗と同じエラー文字列の失敗が続く間は warn を出さない。照会が成功したときはログを出さず、状態を「直前は成功」に戻す。照会を行わなかった通知（メタ文字なし）では状態を変えない。
- **FR3:** warn 判定を純粋なヘルパーに分ける — warn を出すかどうかは、直前の状態（前回の失敗のエラー文字列。成功後と初期状態は None）と今回の照会結果だけから決める。この判定は I/O を行わないヘルパー 1 つに置き、D-Bus 接続もログの取り込みも使わずに単体テストできるようにする。前回のエラー文字列は、ワーカーローカルの `Option<String>` で持つ。static やグローバル状態、別の `NotifyRustSink` インスタンスとの共有は使わない。
- **FR4:** warn の内容 — warn 1 行には、capability 照会の失敗であることが分かる固定の文言（既存の `LOG_*` マーカー定数と同じ形式）と、エラー値の Display 文字列を入れる。通知のタイトルと本文から得た文字列は、生の値も、エスケープ後の値も、秘匿化した表示も入れない。
- **FR5:** doc コメントの更新 — `callbacks.rs` の `notify_worker` の doc コメントと、照会ゲート付近のコメントに、照会失敗時の warn と、状態遷移時だけ出す規則を書き足す。

### Non-Functional Requirements
- **NFR1 - fail-closed 方針の維持:** エスケープの判定と出力は変えない。照会が失敗したときは、現行どおりタイトルと本文の両方をエスケープする。`escape_for_send`、`escape_body_markup`、`body_markup_absence_confirmed`、`escape_for_send_on_demand` の出力と意味は変えない。
- **NFR2 - ログレベル:** リリースビルドの emterm.log に残るよう、`log::warn!` を使う。この機能のために `log::debug!` や `log::info!` は使わない。
- **NFR3 - キャッシュ禁止:** capability の照会結果はキャッシュしない。保持してよいのは、warn 判定用の前回のエラー文字列だけとする。この値をエスケープ判定に使ってはならない。
- **NFR4 - プラットフォームとビルドへの影響:** 変更は既存の `#[cfg(unix)]` の範囲に収める。Windows の `notify_worker` と `spawn_notify_worker` は変えない。`--no-default-features`（CLI のみ）のビルドが引き続きコンパイルできる。
- **NFR5 - 依存の追加なし:** 新しい crate 依存（ログ取り込み用の crate を含む）は追加しない。
- **NFR6 - 既存テストの期待値の不変:** `callbacks/tests.rs` の既存テストは、アサーションの期待値を変えない。`notify_worker` の型制約が変わってテストの fake が差し替えを要する場合も、変えてよいのは fake のエラー型など呼び出し側の書き方だけとする。
- **NFR7 - 秘匿化の順序の不変:** dispatch 成功ログの秘匿化表示は、引き続きエスケープ前の、キューから受け取った値から作る。

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────┐
│ NotifyQueue（通知キュー）                     │
├──────────────────────────────────────────────┤
│ notify_worker（#[cfg(unix)]、ワーカースレッド）│
│   - 照会ゲート（& < > の有無）                │
│   - capability 照会（1 通知につき高々 1 回）  │
│   - エスケープ判定（既存・変更なし）          │
│   - warn 判定ヘルパー（新規・I/O なし）       │
│   - 前回のエラー文字列 Option<String>         │
├──────────────────────────────────────────────┤
│ notify-rust / D-Bus（通知サーバー）           │
└──────────────────────────────────────────────┘
```

**Component Diagram:**
```
notify_worker ──(照会結果を 1 回束縛)──┬─→ エスケープ判定（body_markup_absence_confirmed 等、変更なし）
                                       └─→ warn 判定ヘルパー（前回の状態, 今回の結果）→（warn の要否, 次の状態）
                                                 └─ warn する場合 → log::warn!（固定文言 + エラーの Display）
```

### Data Flow

```
通知（タイトル, 本文）
  → メタ文字なし: 照会しない・状態そのまま → 送出
  → メタ文字あり: get_capabilities() を 1 回 → 結果を束縛
       ├→ エスケープ判定（失敗時はタイトルと本文の両方をエスケープ）→ 送出
       └→ warn 判定ヘルパー
            Ok                          → ログなし・状態 = None
            Err(e), 状態 == Some(e)     → ログなし
            Err(e), 状態 != Some(e)     → log::warn! 1 件・状態 = Some(e)
```

### API Design

該当なし（外部 API の追加・変更はない）。

### Database Schema

該当なし（永続データの追加はない）。ワーカーローカルに前回のエラー文字列 `Option<String>` を 1 つだけ持つ（FR3）。

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/callbacks.rs`: Unix の `notify_worker`、照会ゲート、`escape_for_send` / `escape_body_markup` / `body_markup_absence_confirmed` / `escape_for_send_on_demand`、既存の `LOG_*` マーカー定数
- `src-tauri/src/callbacks/tests.rs`: 既存の `worker_injection_points`、`capability_query_skip_gate`、`body_markup_escape` の各テスト

**External Dependencies:**
- notify-rust: `get_capabilities()`（既存の依存。新しい crate 依存は追加しない、NFR5）
- log: `log::warn!`（既存の依存）

### File Structure

```
src-tauri/src/
├── callbacks.rs           # notify_worker（#[cfg(unix)]）、warn 判定ヘルパー、doc コメント
└── callbacks/
    └── tests.rs           # warn 判定ヘルパーの単体テスト、worker テスト
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/capability-query-failure-warn/**`
- `test-docs/capability-query-failure-warn/**`

`feature-docs/capability-query-failure-warn/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/capability-query-failure-warn/**` covers `test-docs/capability-query-failure-warn/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/capability-query-failure-warn/` directory at all; the declared
`test-docs/capability-query-failure-warn/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS1 (unit): warn 判定ヘルパーで、前回の状態が None のとき Err("e1") → warn する (AC1) — FR2, FR3
- [ ] TS2 (unit): 前回の状態が Some("e1") のとき Err("e1") → warn しない (AC2) — FR2, FR3
- [ ] TS3 (unit): 前回の状態が Some("e1") のとき Err("e2") → warn し、次の状態は Some("e2") (AC3) — FR2, FR3
- [ ] TS4 (unit): 前回の状態が Some("e1") のとき Ok → warn せず次の状態は None。続けて Err("e1") → warn する (AC4) — FR2, FR3
- [ ] TS5 (unit, worker): 注入した fake で `notify_worker` に、Err(e1) の照会を伴う通知、メタ文字を含まない通知、Err(e1) の照会を伴う通知をこの順に流す。照会は 2 回、送出は 3 回で、エスケープ結果は現行と同じ (AC5, AC7)。warn の回数は判定ヘルパーのテストで担保する（ログの取り込みは使わない、NFR5） — FR1, FR2, NFR1, NFR5, NFR6
- [ ] TS6 (unit): 既存の `worker_injection_points`、`capability_query_skip_gate`、`body_markup_escape` の各テストが、期待値を変えずに通る (AC7) — NFR1, NFR6

### Integration Tests
- [ ] TS8 (integration): `cargo test --lib` と `cargo check --no-default-features` が成功する (AC8, AC9) — NFR4

### Review
- [ ] TS7 (review): warn の文面を作る箇所がエラー値だけを受け取り、title、body、redacted を参照しないことを確認する (AC6) — FR4

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] 該当なし

### Edge Cases
- [ ] 同じエラー文字列の失敗の間にメタ文字を含まない通知が挟まる: 照会せず状態を変えないため、2 回目の失敗では warn しない (AC5, TS5)
- [ ] 失敗からの回復（Err の後の Ok）: ログを出さず、状態を None に戻す (AC4, TS4)

### Performance Tests
- 該当なし

## Security Considerations

- **Authentication:** 該当なし
- **Authorization:** 該当なし
- **Input Validation:** 照会ゲートのメタ文字判定（`&`、`<`、`>`）は現行どおり（FR1）。
- **Data Protection:** warn の文面に、通知のタイトルと本文から得た文字列（生の値、エスケープ後の値、秘匿化した表示）を入れない（FR4）。dispatch 成功ログの秘匿化表示は、引き続きエスケープ前の、キューから受け取った値から作る（NFR7）。
- **XSS Prevention:** エスケープの判定と出力は変えない。照会が失敗したときは、現行どおりタイトルと本文の両方をエスケープする（NFR1）。
- **SQL Injection Prevention:** 該当なし
- **CSRF Protection:** 該当なし

## Error Handling

### Error Codes

| Code | Description | HTTP Status | User Message |
|------|-------------|-------------|--------------|
| capability 照会の失敗 | `notify_rust::get_capabilities()` が Err を返す | - | なし（emterm.log に warn を状態遷移時だけ 1 件出す） |

### Error Flow

```
照会が Err → タイトルと本文の両方をエスケープして送出（現行どおり）
           → warn 判定ヘルパー → 状態遷移なら log::warn!（固定文言 + エラーの Display）
```

## Performance Optimization

### Performance Goals
- 照会はオンデマンドとし、1 通知につき高々 1 回とする（FR1）。

### Optimization Strategies
- 該当なし

### Caching Strategy
- capability の照会結果はキャッシュしない。保持するのは warn 判定用の前回のエラー文字列だけで、エスケープ判定には使わない（NFR3）。

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Security requirements are satisfied
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] AC1〜AC9 をすべて満たす

## Assumptions

- A1: fail-closed の方針は変えない。タスク記述の「fail-safe 側（エスケープしない）」と `body_markup_confirmed` は、notification-markup-fail-closed より前の状態を指しており、古い。現在のコードでは照会失敗時に両フィールドをエスケープし（`callbacks.rs:603` `body_markup_absence_confirmed`）、`body_markup_confirmed` というシンボルは存在しない。警告が必要な理由は「失敗時にエスケープされ続けていることの痕跡が残らない」に読み替える。
- A2: 「状態遷移時のみ」は、最初の失敗・エラー文字列の変化・成功後の失敗で warn する形と解釈する。失敗からの回復（Err の後の Ok）はログに出さない。
- A3: warn 判定の状態は、ワーカースレッドのローカル変数として 1 つだけ持つ。App は `NotifyRustSink` を 1 つだけ作るので、実質プロセスごとに 1 つになる。
- A4: エラー文字列の比較と warn への埋め込みには、エラー値の Display 表現を使う。`notify_worker` の `FetchErr` に Display 制約を足す。既存の `worker_injection_points` テストの fake は `Err(())` を使っており、`()` は Display を実装しないため、fake のエラー型を String などに替える。アサーションは変えない。
- A5: Windows の経路は変えない。Windows の notify-rust には `get_capabilities()` がなく、照会もエスケープも行わないため。
- A6: warn が実際に出たかどうかは、ログの取り込みではなく、判定ヘルパーの戻り値で検証する。
- A7: タスク記述の行番号（`callbacks.rs:157`, `:171`）は現在のツリーでは古い。現在の照会ゲートは `callbacks.rs:346`、送出エラーの warn は `callbacks.rs:356` にある。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし

## References

- 要件定義書: [REQUIREMENTS.md](REQUIREMENTS.md)
- `src-tauri/src/callbacks.rs`
- `src-tauri/src/callbacks/tests.rs`
