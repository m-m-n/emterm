# Feature: test-seam-serialization

## Overview

`#[cfg(test)]` のテストシーム経由でプロセスグローバル状態に触れるテストを、既定の並列 `cargo test --lib` の下で決定的に保つ。`RESTART_REQUIRED` の直列化（`self_exec::RestartFlagTestGuard`）は現在のベースで実装済みで、本件はそれを保全する。あわせて SFTP のテストシームを送信ヘルパ `send_progress` / `send_result` 経由にし、構造チェックの走査範囲をテストシームまで広げる。

要件定義は `REQUIREMENTS.md` を参照。

## Objectives

- 既定の並列 `cargo test --lib` の下で、`#[cfg(test)]` シーム経由でプロセスグローバル状態に触れるテストを決定的に保つ。
- SFTP のテストシームに、progress / result チャネルへの送信はすべて `send_progress` / `send_result`（wake ヘルパ）を通すというモジュールの不変条件を守らせる。

## User Stories

ユーザーストーリーは定義しない（テスト・テストシーム・doc コメントのみの変更で、UI を伴わない）。受け入れ基準は次のとおり。

**Acceptance Criteria:**
- [ ] AC-1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` を `--test-threads=1` 無しで全体実行する。合格条件は、その並列実行で `RESTART_REQUIRED` 関連テストと SFTP シーム関連テストが決定的に通ること。失敗を除外できるのは、既知の無関係なフレーク（例: `test/README.md:40` に記載の `tabs.rs` replay テスト）と確認できた場合だけで、その失敗は再実行して記録し、実行結果を全体合格として報告しない。新規の失敗と関連テストの失敗は除外しない。
- [ ] AC-2: `RESTART_REQUIRED` のテスト（`self_exec.rs` の 4 本、`timing.rs` のガード付き 9 本）が並列実行で決定的に通る。
- [ ] AC-3: ガード区間内で assert が panic しても、フラグが後続テストに raised のまま残らない。`restart_flag_test_guard_stays_usable_after_a_panicking_span` が固定する。
- [ ] AC-4: `self_exec.rs` に「single-threaded」の主張が無く、その doc が Mutex + RAII ガードによる直列化を説明している。
- [ ] AC-5: `test_push_progress_event` / `test_push_result_event` が `send_progress` / `send_result` 経由で送信し、どちらかのシームを bare send に戻すと、走査範囲を広げた構造チェックが失敗する。
- [ ] AC-6: `cargo check` が次の 3 構成で警告ゼロで通る: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`、`CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --lib --tests --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`。

## Technical Requirements

### Functional Requirements
- **FR1:** RESTART_REQUIRED のテスト直列化を保全する（実装済み）。`RESTART_REQUIRED` を立てる・下ろす・消費する・観測するテスト（`restart_pending` / `restart_required` を直接呼ぶもの、および `frame_work_pending` / `next_toast_deadline` / `pump_toasts` / `pump_restart_toast` 経由で触れるもの）はすべて、最初に触れてから最後に観測するまでの間、`self_exec::RestartFlagTestGuard` を保持する。現在のベースで `self_exec.rs` の 4 本と `app/tests/timing.rs` の 9 本がすでに満たしており、この状態を後退させない。
- **FR2:** フラグ復元の panic 安全性を保全する（実装済み）。`RestartFlagTestGuard` は Drop で `RESTART_REQUIRED` を `false` に戻す（unwind 中も同じ）。ロックの poison からは `PoisonError::into_inner` で回復する。既存テスト `restart_flag_test_guard_clears_the_flag_when_the_span_ends` / `restart_flag_test_guard_stays_usable_after_a_panicking_span` が固定しており、これを維持する。
- **FR3:** self_exec.rs の直列化 doc を保全する（実装済み）。`self_exec.rs` の `RESTART_FLAG_TEST_LOCK` の doc コメントは、並列実行が既定であることと、ガードによる排他を説明している。「The suite runs single-threaded」の記述は残っていない。この状態を維持する。
- **FR4:** SFTP テストシームをヘルパ経由にする。`SftpService::test_push_progress_event` は `send_progress(&self.progress_tx, ...)` で、`test_push_result_event` は `send_result(&self.result_tx, ...)` で送信する。送る合成イベントの内容（`session_id` `"test-pending"`、`file_name` `"test.txt"`、`bytes_transferred` `0`、`total_bytes` `0`、`status` `Uploading`、`error_message` `None`、`request_id` `0`、`outcome` `Ok(vec![])`）とメソッド名・可視性は変えない。
- **FR5:** テストシームの doc を実装に合わせる。`service.rs` の `#[cfg(test)] impl SftpService` の doc コメントにある「チャネルへ directly 置く」旨を、送信ヘルパ経由で置く記述に改める。`send_progress` の doc にある「このモジュールの progress 送信はすべてこのヘルパを通す」という不変条件は、変更後そのまま真になるので改めない。
- **FR6:** 構造チェックの走査範囲をテストシームまで広げる。`every_progress_and_result_send_site_routes_through_the_wake_helpers` の走査対象に `#[cfg(test)] impl SftpService` ブロックを含める。除外するのは、禁止形を文字列リテラルで組み立てている `mod tests` 本体だけにする。テストシームに bare な `progress_tx` / `result_tx` の送信が戻ると、このテストが失敗すること。
- **FR7:** スキャン範囲外のフラグ接触テストを確認する。`src-tauri/src` 配下で、`timing.rs` と `self_exec.rs` 以外に `RESTART_REQUIRED` に（直接または FR1 の App メソッド経由で）触れるテストがあれば、同じく `RestartFlagTestGuard` を保持させる。無ければ変更しない。

### Non-Functional Requirements
- **NFR1 - 本番挙動は不変:** 本番挙動を変えない。コード変更はすべて `#[cfg(test)]` アイテムと doc コメントに限る。
- **NFR2 - 新規依存を追加しない:** 新しい依存を追加しない（`serial_test` は追加しない）。
- **NFR3 - `cargo check` 3 構成で警告ゼロ:** `cargo check` が 3 構成で警告ゼロで通る: Linux GUI（既定 feature）、`--no-default-features`、x86_64-pc-windows-msvc 向けの `cargo xwin check --lib --tests`。
- **NFR4 - フォーマットは本機能が触れるファイルに限る:** フォーマットは本機能が触れるファイルにだけ適用する。触れていない 7 ファイルの既存ドリフトは対象外のため、クレート全体の rustfmt は行わない。
- **NFR5 - 既知の無関係な並列フレークは対象外:** 既知の無関係な並列フレーク（例: `test/README.md:40` に記載の `tabs.rs` replay テスト）の修正は対象外とする。

## Implementation Approach

### Architecture

**System Architecture:**
該当なし（本番コードの構造は変えない。NFR1）。

**Component Diagram:**
```
self_exec
  RESTART_REQUIRED            (process-global flag)
  RESTART_FLAG_TEST_LOCK      (#[cfg(test)] Mutex)
  RestartFlagTestGuard        (#[cfg(test)] RAII guard; Drop -> RESTART_REQUIRED = false)
      ^ held by: self_exec tests (4), app/tests/timing.rs tests (9), others found by FR7

sftp::service
  send_progress / send_result (wake helpers)
      ^ called by: production send sites, test_push_progress_event / test_push_result_event (FR4)
  mod tests
    every_progress_and_result_send_site_routes_through_the_wake_helpers
      scan target: module source incl. #[cfg(test)] impl SftpService (FR6)
      excluded:    mod tests body only
```

### Data Flow

```
test_push_progress_event -> send_progress(&self.progress_tx, ...) -> progress_tx (+ wake)
test_push_result_event   -> send_result(&self.result_tx, ...)     -> result_tx   (+ wake)
```

`crate::wakeup::wake()` はテストバイナリでは no-op である（A-2）。

### API Design

該当なし。`test_push_progress_event` / `test_push_result_event` のメソッド名・可視性は変えない（FR4）。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- `self_exec::RestartFlagTestGuard` / `RESTART_FLAG_TEST_LOCK`: `RESTART_REQUIRED` に触れるテストの排他（FR1〜FR3、FR7）
- `sftp::service::send_progress` / `send_result`: テストシームの送信経路（FR4）
- `crate::wakeup::wake()`: 送信ヘルパが呼ぶ wake。テストバイナリでは no-op（A-2）

**External Dependencies:**
- 追加しない（NFR2）

### File Structure

```
src-tauri/src/
├── self_exec.rs              # RESTART_REQUIRED / RESTART_FLAG_TEST_LOCK / RestartFlagTestGuard (FR1-FR3, preserve)
├── app/tests/timing.rs       # guarded RESTART_REQUIRED tests, SFTP seam callers (FR1, TS-1)
└── sftp/service.rs           # send_progress / send_result, test seams, structural check (FR4-FR6)
```

FR7 の確認で該当テストが見つかった場合は、そのファイルも対象になる。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/test-seam-serialization/**`
- `test-docs/test-seam-serialization/**`

`feature-docs/test-seam-serialization/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/test-seam-serialization/**` covers
`test-docs/test-seam-serialization/{T}.tests.yaml`, the per-task test record.
It is generated and owned by `implement-phase.md`; this section cites it and
restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/test-seam-serialization/` directory at all; the declared
`test-docs/test-seam-serialization/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1（FR4, AC-5）: `test_push_progress_event` / `test_push_result_event` を呼ぶ `timing.rs` のテスト（`frame_work_pending_true_when_progress_channel_nonempty_and_consumes_nothing`、`frame_work_pending_true_when_result_channel_nonempty_and_consumes_nothing`、`next_toast_deadline_some_when_pretoast_progress_event_pending`）が、シームをヘルパ経由にした後も通る。
- [ ] TS-2（FR6, AC-5）: `every_progress_and_result_send_site_routes_through_the_wake_helpers` が走査範囲を広げた状態で通る。シームを一時的に bare な `progress_tx` / `result_tx` 送信に戻すと失敗する（red チェック）。
- [ ] TS-3（FR1, FR2, AC-2, AC-3）: `self_exec.rs` の 4 本と `timing.rs` のガード付き 9 本が並列の `--lib` 実行で通る。`restart_flag_test_guard_stays_usable_after_a_panicking_span` を含む。
- [ ] TS-4（FR3, AC-4）: `self_exec.rs` に「single-threaded」の文言が無く、`RESTART_FLAG_TEST_LOCK` の doc がガードによる直列化を説明している。
- [ ] TS-5（FR7）: `src-tauri/src` から、`timing.rs` / `self_exec.rs` 以外で `RESTART_REQUIRED` に直接、または `frame_work_pending` / `next_toast_deadline` / `pump_toasts` / `pump_restart_toast` 経由で触れるテストを探す。見つかったものはすべて `RestartFlagTestGuard` を保持している。

### Integration Tests
- [ ] TS-6（AC-1, NFR5）: 並列の `cargo test --lib` を全体実行する。失敗はすべて、関連（除外しない）か、確認済みの既知の無関係なフレーク（再実行して記録し、全体合格として報告しない）に分類する。
- [ ] TS-7（AC-6, NFR3）: 3 構成の `cargo check` が警告ゼロで完了する。

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] ガード区間内で assert が panic した場合、フラグは後続テストに raised のまま残らない（AC-3、TS-3）。
- [ ] 並列実行で既知の無関係なフレークが失敗した場合、再実行して記録し、全体合格として報告しない（AC-1、TS-6）。

### Performance Tests
該当なし。

## Security Considerations

該当なし（本番挙動を変えない。NFR1）。

## Error Handling

### Error Codes

該当なし。

### Error Flow

該当なし。

## Performance Optimization

該当なし。

## Assumptions

- **A-1:** AC-1 の合格範囲は `RESTART_REQUIRED` 関連テストと SFTP シーム関連テストに限る。並列の `--lib` 全体実行は行い、確認済みの既知の無関係なフレーク（例: `tabs.rs` replay テスト）は再実行後に記録した例外としてのみ除外し、結果を全体合格として報告しない。（出典: answer `requirement.parallel-lib-pass-scope`（create-spec-q0001、batch codex consultation、record_as_assumption）、可逆）
- **A-2:** テストバイナリでは `crate::wakeup::wake()` は no-op である（`wakeup::install` がそこで実行されない）。そのため、シームを `send_progress` / `send_result` 経由にしてもテスト時の副作用は増えない。（出典: `src-tauri/src/sftp/service.rs` のコメント。`wakeup.rs` 自体は調査対象外、可逆）
- **A-3:** FR1〜FR3 は現在のベースですでに満たされている（`RestartFlagTestGuard`、panic 安全な Drop、更新済みの doc）。本件はこれらを再実装せず保全する。（出典: `src-tauri/src/self_exec.rs`、`src-tauri/src/app/tests/timing.rs`、可逆）

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC-1〜AC-6 を満たす
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（すべての要件が confirmed）。

## References

- 要件定義書: `feature-docs/test-seam-serialization/REQUIREMENTS.md`
- `src-tauri/src/self_exec.rs`
- `src-tauri/src/app/tests/timing.rs`
- `src-tauri/src/sftp/service.rs`
- `test/README.md:40`
