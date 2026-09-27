# Feature: notify-queue-worker-gone

## Overview

`NotifyQueue::try_submit` で、通知ワーカースレッドの消失（Receiver drop による `TrySendError::Disconnected`）を、キュー飽和（`TrySendError::Full`）と区別して報告する。
キュー飽和のエピソード抑止（`armed` フラグ）は、ワーカー消失の報告を抑止しない。

## Objectives

- 通知ワーカースレッド消失（Receiver drop による `TrySendError::Disconnected`）を、キュー飽和（`TrySendError::Full`）と区別して報告する
- キュー飽和のエピソード抑止（`armed` フラグ）がワーカー消失の報告を抑止しないようにする

## Technical Requirements

### Functional Requirements

- **FR1:** try_submit のエラーアーム分割 — `src-tauri/src/callbacks.rs` の `NotifyQueue::try_submit` は、`try_send` の結果を `Ok` / `Err(TrySendError::Full)` / `Err(TrySendError::Disconnected)` の 3 アームで扱う。単一の `Err(_)` アームは残さない。
- **FR2:** Full の挙動を維持する — `Full` アームは現行のエピソード arming をそのまま維持する。`armed` を atomic swap で check-and-disarm し、エピソード先頭の 1 件だけ `LOG_NOTIFY_QUEUE_SATURATED` の warn を出して `DroppedEpisodeStart` を返す。以降は `DroppedAlreadyWarned` を返す。`Ok` の場合は `armed` を true に戻す。
- **FR3:** ワーカー消失専用のログ定数 — ワーカー消失専用の公開ログ定数（例: `LOG_NOTIFY_WORKER_DEAD`）を、`LOG_NOTIFY_QUEUE_SATURATED` と同じログ定数セクションに追加する。`Disconnected` アームはこの定数で `log::error!` を出す。
- **FR4:** ワーカー消失報告用の独立した抑止フラグ — `Disconnected` の error 記録は `armed` とは別の `AtomicBool` フラグで抑止する。このフラグは `Ok` アーム（エピソードの再 arm）でも `Full` アームでも読み書きしない。`Disconnected` アームも `armed` を読み書きしない。check-and-set は atomic swap で行う。同時に `Disconnected` へ到達したプロデューサが何件あっても、error 記録は `NotifyQueue` 1 個につき 1 件だけになる。
- **FR5:** SubmitOutcome にワーカー消失バリアントを追加する — `SubmitOutcome` に `WorkerGone` 相当のバリアントを追加する。`Disconnected` の場合、`try_submit` は `DroppedEpisodeStart` / `DroppedAlreadyWarned` ではなくこのバリアントを返す。error 記録が出たかどうかを、ログを捕捉せずに戻り値だけで判定できるようにする（既存の飽和バリアントの判定方式と同じ）。
- **FR6:** 再発を検出する単体テスト — `src-tauri/src/callbacks/tests.rs` の `worker_thread` モジュール（`cfg(unix)` ではない）に単体テストを追加する。`NotifyQueue::new` でキューを作り、receiver を drop してから `try_submit` を呼ぶ。`NotifyRustSink`・notify-rust・D-Bus には触れない。

### Non-Functional Requirements

- **NFR1 - NotificationSink トレイト境界を変えない:** `NotificationSink` トレイトのシグネチャ（`fn send(&self, title: &str, body: &str)`、`Send + Sync`）と `NotifyRustSink::send` の呼び出し側への契約は変えない。
- **NFR2 - 投入経路はブロックしない:** `try_submit` はどの分岐でもブロックせず、外部の通知デーモンに依存する処理を行わない（既存の投入経路の契約を維持する）。
- **NFR3 - ログ記録でのリダクション:** ワーカー消失の error 記録に通知本文のテキストを含めない。通知の中身を出す場合は、既存の飽和 warn と同じく `redact_notification` のメタデータ（長さと diag_id）に限る。
- **NFR4 - プラットフォーム:** `NotifyQueue` の変更と追加テストは Linux と Windows の両方で動く（`cfg(unix)` で囲まない）。

## Assumptions

- **A1:** crossbeam-channel の bounded `try_send` は、切断済みのチャネルに対して、キューが満杯かどうかに関係なく `Disconnected` を返す。receiver drop の後は `Full` アームに入らない。
- **A2:** ワーカー消失の抑止フラグは一度立ったら `NotifyQueue` が生きている間は戻らない。ワーカー panic 後の自動再起動はスコープ外。
- **A3:** 既存の飽和バリアントの組（`DroppedEpisodeStart` / `DroppedAlreadyWarned`）にならい、`WorkerGone` 相当のバリアントも「記録あり」と「記録済み」の 2 つに分ける。テストは既存の Test Notes の方式どおり、ログを捕捉せず戻り値で判定する。
- **A4:** ワーカー消失の error 記録は既存の飽和 warn と同じ形式（`"{CONST}: {redact_notification(title, body)}"`）で出す。
- **A5:** `try_submit` の戻り値を使う本番コードは無い（`NotifyRustSink::send` は戻り値を捨てている）。既存テストは `SubmitOutcome` を `assert_eq` で比較しており網羅的な match をしていないので、バリアント追加で既存コードは壊れない。
- **A6:** `Disconnected` に到達するのは `notify_worker` 内部の panic だけ。`NotifyRustSink::drop` は queue を `None` にしてから終了処理をするので、通常の終了処理では `try_submit` は `Disconnected` に到達しない。

## Implementation Approach

### Data Flow

```
try_submit(title, body)
  └─ try_send
       ├─ Ok                          → armed = true（ワーカー消失フラグには触れない）
       ├─ Err(TrySendError::Full)     → armed を swap で check-and-disarm
       │                                 ├─ エピソード先頭 → warn(LOG_NOTIFY_QUEUE_SATURATED) → DroppedEpisodeStart
       │                                 └─ 以降          → DroppedAlreadyWarned
       └─ Err(TrySendError::Disconnected)
                                       → ワーカー消失フラグを swap で check-and-set（armed には触れない）
                                         ├─ 初回 → error(LOG_NOTIFY_WORKER_DEAD 相当) → WorkerGone 相当（記録あり）
                                         └─ 以降 → WorkerGone 相当（記録済み）
```

### Dependencies

**Internal Dependencies:**
- `redact_notification`: ワーカー消失の error 記録に出す通知メタデータ（NFR3、A4）
- `NotificationSink` / `NotifyRustSink`: 変更しない境界（NFR1）

**External Dependencies:**
- crossbeam-channel: bounded チャネルの `try_send`（A1）

### File Structure

```
src-tauri/src/
├── callbacks.rs         # NotifyQueue::try_submit、SubmitOutcome、ログ定数（FR1〜FR5）
└── callbacks/
    └── tests.rs         # worker_thread モジュールへの単体テスト追加（FR6）
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/notify-queue-worker-gone/**`
- `test-docs/notify-queue-worker-gone/**`

`feature-docs/notify-queue-worker-gone/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/notify-queue-worker-gone/**` covers
`test-docs/notify-queue-worker-gone/{T}.tests.yaml`, the per-task test record.
It is generated and owned by `implement-phase.md`; this section cites it and
restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/notify-queue-worker-gone/` directory at all; the declared
`test-docs/notify-queue-worker-gone/**` entry is still correct in that case —
a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS1** (AC1): `NotifyQueue::new(NOTIFY_QUEUE_CAPACITY)` の後、rx を drop して `try_submit` を 1 回呼ぶ。「ワーカー消失・記録あり」のバリアントが返ることを確認する。
- [ ] **TS2** (AC3): rx を drop した後に `try_submit` を複数回呼ぶ。最初の 1 回だけが「記録あり」で、残りはすべて「ワーカー消失・記録済み」になり、どれも飽和バリアントにならないことを確認する。
- [ ] **TS3** (AC2): rx を保持したままキューを満杯にして `DroppedEpisodeStart` を発生させる（`armed` が false になる）。その後 rx を drop して `try_submit` を呼び、「ワーカー消失・記録あり」が返ることを確認する。
- [ ] **TS4** (AC3): rx を drop した `Arc<NotifyQueue>` に対して複数スレッドから同時に `try_submit` を呼び、「記録あり」の結果がちょうど 1 件になることを確認する。

### Integration Tests

- [ ] **TS5** (AC4, AC5): 既存の `worker_thread` テストを含む `--lib` スイート全体を実行する。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

## Success Criteria

- [ ] **AC1:** receiver を drop した `NotifyQueue` に対する最初の `try_submit` が、`WorkerGone` 相当のバリアントの中で「error 記録が出た」ことを表すものを返し、`DroppedEpisodeStart` / `DroppedAlreadyWarned` を返さない。
- [ ] **AC2:** キュー飽和のエピソードで `armed` が false になった後に receiver を drop しても、次の `try_submit` は「ワーカー消失の error 記録が出た」ことを返す（飽和の抑止がワーカー消失に適用されない）。
- [ ] **AC3:** receiver drop 後に `try_submit` を繰り返しても、error 記録は 1 回だけ出る。以降の呼び出しは「ワーカー消失・記録済み」を返す。このフラグはエピソードの再 arm では戻らない。
- [ ] **AC4:** 既存の飽和テスト（`queue_drops_the_ninth_submission_without_blocking_when_receiver_is_idle`、`saturation_episode_warns_exactly_once_then_rearms_after_a_successful_submission`、`concurrent_saturated_drops_produce_exactly_one_episode_start_warning`）が変更なしで通る。
- [ ] **AC5:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし
