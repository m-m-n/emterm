# Feature: test-notify-dbus-isolation

Requirements document: `feature-docs/test-notify-dbus-isolation/REQUIREMENTS.md`

## Overview

テストのビルド（`#[cfg(test)]`）の `App` は何もしない通知送信部を使い、`NotifyRustSink` は偽物の送信関数（unix では偽物の能力照会関数も）を渡して生成できるようにする。`worker_thread` のテストは unix・Windows の両方でこの偽物を使う。本番ビルドの通知挙動は変えない。

## Objectives

- テストのビルドでは本物のデスクトップ通知（notify-rust 経由の D-Bus 通知 / Windows のトースト）に到達しない
- 再現手順（`--lib` のテスト実行）で通知デーモンの履歴にテスト由来の通知が溜まらない
- 再発を検出するテストを置く

## Technical Requirements

### Functional Requirements

- **FR1:** テストビルドの App は何もしない通知送信部を使う — `#[cfg(test)]` のとき、`App::with_settings` は `NotifyRustSink` を組み立てず、`send` を呼ばれても何もしない通知送信部（`NotificationSink` 実装）を `App` の `notification_sink` に設定する。`App::new()` は `App::with_settings` を経由するため同じ通知送信部になる。
- **FR2:** 本番ビルドの App は従来どおり NotifyRustSink を使う — `#[cfg(test)]` でないとき、`App::with_settings` は従来どおり `NotifyRustSink::new()` を組み立てて `notification_sink` に設定する。
- **FR3:** NotifyRustSink に偽物の送信関数を渡して生成する手段 — `NotifyRustSink` を、ワーカースレッドが使う関数を差し替えて生成できるようにする。unix では送信関数と能力照会関数（既存の unix 版 `notify_worker` の注入点と同じ方式）、Windows では送信関数を渡す。`NotifyRustSink::new()` は notify-rust の送信（unix では `notify_rust::get_capabilities` による能力照会も）を渡す本番の生成経路のまま残す。
- **FR4:** Windows 版 notify_worker に送信関数の注入点を足す — `#[cfg(not(unix))]` の `notify_worker` に送信関数の注入点を足し、`notify_rust::Notification::new().summary(..).body(..).show()` の直接呼び出しをやめる。本番では `spawn_notify_worker`（Windows 版）がこの notify-rust の送信を渡す。Windows 版に能力照会とエスケープは足さない。
- **FR5:** worker_thread のテストは両 OS で偽物を使う — `callbacks/tests.rs` の `worker_thread` モジュールで `NotifyRustSink` を生成するテスト（`sink_send_never_blocks_the_caller_even_past_queue_capacity`、`dropping_the_sink_after_sending_returns_within_the_shutdown_deadline`、`dropping_a_freshly_constructed_sink_with_no_notifications_returns_within_the_shutdown_deadline`）は、unix・Windows の両方で FR3 の手段により偽物の送信関数（unix では偽物の能力照会関数も）を渡して生成する。各テストの既存の検証（`send` が 100ms 未満で戻る、drop が 2 秒未満で戻る）は保つ。
- **FR6:** 修正した 2 か所を固定する回帰テスト — (a) テストビルドの `App::new()` が保持する通知送信部が `NotifyRustSink` でないことを検証するテストを置く。(b) `worker_thread` の送信テストで、送った通知が偽物の送信関数に届いたことを検証する。全テスト横断の検出機構は入れない。
- **FR7:** テスト名を変える場合の test-docs 記録の更新 — `worker_thread` のテスト名を変える場合は、`.claude/rules/test-docs-records.md` に従い `test-docs/notification-worker-thread/task0001.tests.yaml` の `acceptance_tests[].tests` の該当名を新しい名前に更新する。

### Non-Functional Requirements

- **NFR1:** テストビルドから本物の通知経路に到達しない — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` の実行中、notify-rust の送信と能力照会を呼ぶテストが無い。
- **NFR2:** 本番の通知挙動は変えない — 本番ビルドの通知の挙動（キュー容量 `NOTIFY_QUEUE_CAPACITY`、終了待ち `NOTIFY_WORKER_JOIN_TIMEOUT`、unix の能力照会ゲートとエスケープ、ログ文言 `"notify-rust dispatched: {redacted}"` / `"notify-rust failed: {e}"`）は変えない。
- **NFR3:** CLI 専用ビルドが通る — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。
- **NFR4:** Windows ターゲットでテストコードを含めてコンパイルが通る — Windows ターゲット（`x86_64-pc-windows-msvc`、`CARGO_TARGET_DIR=src-tauri/target-win`）でテストコードを含めたコンパイルが通る（例: `cargo xwin check --tests --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`）。

## Implementation Approach

### Change Points

| Location | Change | Requirement |
|----------|--------|-------------|
| `App::with_settings`（`app/mod.rs:574`） | `#[cfg(test)]` で何もしない通知送信部、非テストで `NotifyRustSink::new()` | FR1, FR2 |
| `NotifyRustSink` | ワーカースレッドが使う関数を差し替えて生成する手段。`NotifyRustSink::new()` は本番の生成経路のまま | FR3 |
| `#[cfg(not(unix))]` の `notify_worker` / `spawn_notify_worker`（Windows 版） | 送信関数の注入点。本番は notify-rust の送信を渡す | FR4 |
| `callbacks/tests.rs` の `worker_thread` モジュール（`callbacks/tests.rs:1492,1512,1532`） | 3 テストを偽物の注入に切り替え、送信テストで偽物への到達を検証 | FR5, FR6 |
| テストビルドの `App::new()` の通知送信部を検証するテスト | 新規 | FR6 |
| `test-docs/notification-worker-thread/task0001.tests.yaml` | テスト名を変える場合のみ該当名を更新 | FR7 |

### Dependencies

**Internal Dependencies:**
- `NotificationSink` / `NotifyRustSink`（`crate::callbacks`）
- 既存の unix 版 `notify_worker` の注入点（`callbacks.rs:369-374`）

**External Dependencies:**
- notify-rust: 本番の送信、unix では `notify_rust::get_capabilities` による能力照会

### Design Step

Skipped: UI に触れない。テストビルドの通知経路の修正のみ（answer design.step.recommendation）。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/test-notify-dbus-isolation/**`
- `test-docs/test-notify-dbus-isolation/**`

`feature-docs/test-notify-dbus-isolation/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/test-notify-dbus-isolation/**` covers
`test-docs/test-notify-dbus-isolation/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (AC-1; FR1, FR6) unit (Rust, --lib): テストビルドで `App::new()` を生成し、`notification_sink` が `NotifyRustSink` でないことを判定する。 - `NotifyRustSink` でないと判定される。
- [ ] TS-2 (AC-2; FR1) unit (Rust, --lib): テストビルドで `App::new()` を生成し、`App::notify("t", "b")` を呼ぶ。 - パニックせずに戻る。
- [ ] TS-3 (AC-3; FR3, FR5, FR6) unit (Rust, --lib, unix / Windows): 記録する偽物の送信関数（unix では偽物の能力照会関数も）を渡して `NotifyRustSink` を生成し、`NOTIFY_QUEUE_CAPACITY * 4` 件 send する。 - send が 100ms 未満で戻り、偽物の送信関数が 1 件以上受け取る。
- [ ] TS-4 (AC-3; FR3, FR5, FR6) unit (Rust, --lib, unix / Windows): 偽物を渡して生成した `NotifyRustSink` に 3 件 send し、drop する。 - drop が 2 秒未満で戻り、偽物の送信関数が送った 3 件を受け取る。
- [ ] TS-5 (AC-3; FR3, FR5, FR6) unit (Rust, --lib, unix / Windows): 偽物を渡して生成した `NotifyRustSink` を何も送らずに drop する。 - drop が 2 秒未満で戻る。

### Build Checks
- [ ] TS-6 (AC-4, AC-6; FR2, FR3, FR4, NFR2, NFR3, NFR4) build check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`、同 `--no-default-features`、Windows ターゲットでのテストコードを含むコンパイル確認を実行する。 - すべて成功する。

### Manual Tests
- [ ] TS-7 (AC-5; NFR1) manual: Linux のデスクトップセッションで `--lib` のテストを実行し、前後で通知デーモンの履歴を比べる。 - テスト由来の通知が増えない。

### Record Checks
- [ ] TS-8 (AC-7; FR7) record check (conditional): テスト名を変えた場合のみ、`--lib -- --list` の出力と `test-docs/notification-worker-thread/task0001.tests.yaml` を突き合わせる。 - 記録の各名前が `<name>: test` 行として現れる。

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] TS-3 は送信中もワーカーが並行してキューを消費するため、偽物の送信関数が受け取る件数は不定。検証は「1 件以上届いた」とする。
- [ ] TS-4 の「3 件届いた」は、drop の終了待ち（200ms）を過ぎるとワーカーが切り離されて未完になり得る。drop のタイミングに頼らず、記録側を十分な上限時間で待って確かめる。
- [ ] `#[cfg(test)]` のときだけ `NotifyRustSink` を使わなくなるため、`app/mod.rs:16` の `use crate::callbacks::{NotificationSink, NotifyRustSink};` がテストビルドで未使用 import 警告になり得る。
- [ ] unix の能力照会は title/body に `&` `<` `>` が含まれるときだけ走る。既存の `worker_thread` テストの通知（t0〜, b）には含まれないが、偽物の能力照会を渡せばこの条件に関係なく D-Bus に到達しない。
- [ ] `#[cfg(test)]` は lib 自身の単体テストのビルドにだけ効く。`src-tauri/tests/*.rs` の統合テストは非テストビルドの lib にリンクされるため、そこから `App::new()` を呼ぶと本物の `NotifyRustSink` になる（現時点で該当なし）。

## Success Criteria

- [ ] AC-1 (FR1, FR6): テストビルドで `App::new()` が保持する通知送信部が `NotifyRustSink` でないことを検証するテストが通る。
- [ ] AC-2 (FR1): テストビルドで `App::new()` の `App::notify(title, body)` を呼ぶと、パニックせず何も送らずに戻る。
- [ ] AC-3 (FR3, FR5, FR6): `worker_thread` の送信テストで、偽物の送信関数を渡して生成した `NotifyRustSink` に送った通知が偽物の送信関数に届き、かつ既存の検証（32 件の `send` が 100ms 未満で戻る、3 件送信後の drop が 2 秒未満で戻る、未送信の drop が 2 秒未満で戻る）が通る。
- [ ] AC-4 (FR2, FR3, FR4, NFR2): 本番ビルドでは `App::with_settings` が `NotifyRustSink::new()` を使い、`NotifyRustSink::new()` が unix では notify-rust の能力照会と送信を、Windows では notify-rust の送信を渡す（コードの確認と cargo check で確かめる）。
- [ ] AC-5 (NFR1): Linux のデスクトップセッションで `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` を実行しても、通知デーモンの履歴に t0 などのタイトルの通知が増えない（手動確認）。
- [ ] AC-6 (NFR3, NFR4): `--no-default-features` の cargo check と、Windows ターゲットのテストコードを含むコンパイル確認が通る。
- [ ] AC-7 (FR7): `worker_thread` のテスト名を変えた場合、`test-docs/notification-worker-thread/task0001.tests.yaml` の該当名が新しい名前になっており、各名前が `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list` の出力に `<name>: test` として現れる。名前を変えない場合は対象外。

## Assumptions

- **A1:** Windows も対象にする。Windows 版 `notify_worker` に送信関数の注入点を足し、`worker_thread` の送信テストは両 OS で偽物の送信関数を使う。（answer requirement.windows-scope）
- **A2:** 回帰テストは修正した 2 か所（テストビルドの `App::new()` の送信部、`worker_thread` の送信の偽物到達）に限る。全テスト横断の検出機構は入れない。（answer requirement.regression-detection-scope）
- **A3:** design step は skip する。（answer design.step.recommendation）
- **A4:** `src-tauri/tests/*.rs` は `App::new` / `NotifyRustSink` を参照しないため、`#[cfg(test)]` での切り替えで `--lib` のテストの漏れを塞げる。（orchestrator-verified fact）
- **A5:** unix では送信関数に加えて能力照会関数も偽物にする（既存 unix 版 `notify_worker` の 2 つの注入点に揃える）。（task_description 修正方針 2 と `callbacks.rs:369-374` の既存注入点から導出）
- **A6:** `worker_thread` で `NotifyRustSink` を生成する 3 テストすべてを偽物の注入に切り替える（送信しない `dropping_a_freshly_constructed_sink_...` も含む）。（FR5 の範囲として導出）
- **A7:** Windows でのテスト実行は Linux ホストでは行わず、Windows ターゲットのコンパイル確認で代える。（実行環境（Linux）からの導出）
- **A8:** `NotifyRustSink` を生成する箇所は `App::with_settings`（`app/mod.rs:574`）と `worker_thread` の 3 テスト（`callbacks/tests.rs:1492,1512,1532`）のみ。確認範囲は reference_scan_targets に限る。（reference_scan_targets の調査結果）
- **A9:** 本番ビルドの通知挙動（`NotifyRustSink::new()` の経路、ログ文言、キュー容量、終了待ち、unix の能力照会ゲートとエスケープ）は不変条件として保つ。（既存実装と既存テスト（`worker_injection_points` 等））

## Open Questions

None. No requirement has `status: tbd`.

## References

- Requirements: `feature-docs/test-notify-dbus-isolation/REQUIREMENTS.md`
- Test-docs record rule: `.claude/rules/test-docs-records.md`
- Predecessor test record: `test-docs/notification-worker-thread/task0001.tests.yaml`
