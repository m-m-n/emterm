# Verification Document: notify-escape-test-production-path

## Overview

**Feature**: notify-escape-test-production-path / **SPEC.md**: `feature-docs/notify-escape-test-production-path/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/notify-escape-test-production-path/IMPLEMENTATION.md`

All commands run from the project root (no `cd`).

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings in `src-tauri/src/callbacks.rs` or `src-tauri/src/callbacks/tests.rs`
- Windows target check (TS-10, manual where cargo-xwin is set up via `make setup`): `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not measured (no coverage tooling is configured). Every task Acceptance Criterion maps to at least one scenario below.
- Unrelated suites (tabs replay, tmux socket discovery) can flake in the parallel run. A failure there is re-run with `-- --test-threads=1` before it counts as a regression.

### Test Scenarios from SPEC.md

TS-1 to TS-9 come from SPEC.md. TS-10 to TS-12 are review checks added by the plan so that FR5, FR6, NFR1 and NFR3 each have a verification.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Absence confirmed leaves the body unchanged (after replacement): `escape_for_send` with a fetch success listing `actions` and the body `Tom & Jerry &amp; <b>bold</b>` | The returned body equals the input | Unit |
| TS-2 | Tab-activity body is escaped by the single decision (after replacement): body built with `sanitize_title` and `notification_body` containing `<`, passed to `escape_for_send` with a fetch success listing `body-markup` | The returned body contains no `<` and no `>` | Unit |
| TS-3 | Agent-status body is escaped by the single decision (after replacement): body built with `agent_notification_body` containing `<script>`, passed to `escape_for_send` with a fetch success listing `body-markup` | The returned body contains no `<` and no `>` | Unit |
| TS-4 | Worker, body-markup present: fake fetch returns success listing `body-markup`; one notification with title `a<b` and body `Tom & <b>` goes through `notify_worker` | The send injection point receives exactly summary `a&lt;b` and body `Tom &amp; &lt;b&gt;` | Unit |
| TS-5 | Worker, body-markup absent: fake fetch returns success listing `actions`; same title / body as TS-4 | The send injection point receives exactly the input title and body | Unit |
| TS-6 | Worker, fetch failure: fake fetch returns a failure; same title / body as TS-4 | The send injection point receives exactly the escaped values of TS-4 | Unit |
| TS-7 | Worker, one fetch per notification: several notifications enqueued, sending side closed, worker runs until it returns | Fetch call count and send call count both equal the number of notifications; send order equals enqueue order | Unit |
| TS-8 | Mutation detection: apply each of AC-4 (a) to (d) of SPEC.md alone and run the `--lib` tests, then revert | Each mutation makes at least one test fail; no mutation is committed | Manual |
| TS-9 | Full regression: the `--lib` test command | Exit code 0; the existing `summary_markup_escape`, `worker_thread` (including `NotifyRustSink` creation and shutdown), `body_markup_absence_confirmed` and `escape_body_markup` tests pass unmodified | Integration |
| TS-10 | Windows path and unix gating kept: diff inspection plus the Windows target check | No diff in the Windows `notify_worker` or its call; `escape_for_send`, `escape_body_markup`, `body_markup_absence_confirmed` and the capability-fetch injection point are unix-only; the Windows target check exits 0 where cargo-xwin is available | Review + Manual |
| TS-11 | Comments name the actual caller: diff inspection of the FR6 comments | The doc comments of `escape_for_send` and `escape_body_markup` name `notify_worker` and not `NotifyRustSink::send`; the `notify_worker` doc comment describes both injection points and the production values; the comments at the head of `mod body_markup_escape` and before the replaced tests describe the path through `escape_for_send`; no other comment changes | Review |
| TS-12 | Unchanged behavior outside the test seam: diff inspection | `Cargo.toml` and `Cargo.lock` have no diff; log texts and levels (`notify-rust dispatched: {redacted}` debug, `notify-rust failed: {e}` warn) are unchanged; redaction runs on the received values before the escape; queue capacity, worker thread start / stop and `NotifyRustSink::send` are unchanged | Review |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — expected: no diff reported for `src-tauri/src/callbacks.rs` or `src-tauri/src/callbacks/tests.rs`
- Static analysis: none configured beyond the build check (no new warnings in the two changed files)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The capability fetch / summary-body decision boundary is `escape_for_send`, unix-only | TS-10 (unix gating), TS-9 (existing `summary_markup_escape` tests pin it unchanged) |
| AC-2 | `notify_worker` takes the two injection points, production passes notify-rust, only the `escape_for_send` result reaches the send, no escape branch outside it | TS-4, TS-5, TS-6, TS-8 (mutations c and d), code review |
| AC-3 | No inline escape branch left in `tests.rs`; the three sites call `escape_for_send` | TS-1, TS-2, TS-3, search of `tests.rs` |
| AC-4 | Mutations (a) to (d) each turn a `--lib` test red | TS-8 |
| AC-5 | A test drives `notify_worker` with body-markup present / absent / fetch failure and compares exact values without D-Bus | TS-4, TS-5, TS-6, TS-7 |
| AC-6 | The `--lib` test command passes | TS-9 |
| AC-7 | Windows flow, redaction position, one fetch per notification and logs are unchanged | TS-7, TS-10, TS-12 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-8, TS-9 |
| FR2 | task0001 | TS-4, TS-5, TS-6, TS-7, TS-8 |
| FR3 | task0002 | TS-1, TS-2, TS-3, TS-8 |
| FR4 | task0001 | TS-4, TS-5, TS-6, TS-7, TS-8 |
| FR5 | task0001 | TS-9, TS-10 |
| FR6 | task0001, task0002 | TS-11 |
| NFR1 | task0001 | TS-4, TS-5, TS-6, TS-7, TS-9, TS-10, TS-12 |
| NFR2 | task0001, task0002 | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-7 |
| NFR3 | task0001, task0002 | TS-12 |
| NFR4 | task0001, task0002 | TS-9 |

## E2E Testing

Not applicable: no E2E framework is configured (`e2e_test_command` is empty in workflow.yaml), and the feature has no user-visible behavior.

## Manual Testing (E2E Not Possible)

- [ ] TS-8: apply each mutation alone, run the `--lib` tests, confirm at least one failure, revert. (a) invert the absence condition in `escape_for_send`; (b) skip `escape_body_markup` on the escape side for the title only, the body only, and both (three runs); (c) remove the `escape_for_send` call from `notify_worker` and forward the received title / body; (d) keep the `escape_for_send` call in `notify_worker` but forward the pre-escape title, then the pre-escape body (two runs).
- [ ] TS-10: inspect the diff for Windows-gated code and unix gating; run the Windows target check where cargo-xwin is available.
- [ ] TS-11: inspect the FR6 comments in the diff.
- [ ] TS-12: inspect the diff for `Cargo.toml` / `Cargo.lock`, log texts and levels, redaction position, queue capacity, worker thread start / stop and `NotifyRustSink::send`.

## Performance / Security Verification

- Security — escape rules and fail-closed decision unchanged: TS-4, TS-5, TS-6 and the existing `summary_markup_escape` tests (TS-9).
- Security — redaction on the received values before the escape: TS-12.
- Performance — capability fetched exactly once per notification, no caching: TS-7.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (linux check, Windows target check) | 1 | 0 | 1 |
| Unit tests | 7 (TS-1 to TS-7) | 7 | 0 | 0 |
| Regression | 1 (TS-9) | 1 | 0 | 0 |
| Mutation detection | 1 (TS-8) | 0 | 0 | 1 |
| Review checks | 3 (TS-10, TS-11, TS-12) | 0 | 0 | 3 |
| Format | 1 | 1 | 0 | 0 |
