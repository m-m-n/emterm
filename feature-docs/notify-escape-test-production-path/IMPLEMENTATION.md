# Implementation Plan: notify-escape-test-production-path

## Overview

The unix notification worker `notify_worker` takes the capability fetch and the
notification send as injection points and delegates every escape decision to the
existing pure function `escape_for_send`. Tests then exercise the production
decision and the production hand-off to the send, instead of copies of the
branch. No user-visible behavior changes (NFR1).

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate (default `gui` feature; the
  `callbacks` module is GUI-only)
- **Key libraries**: notify-rust — production capability query and notification
  send (existing dependency, usage unchanged)
- **New dependencies**: none (NFR3). No license entry to record.
- **Test runner**: cargo unit tests in the `--lib` target

## Layer Structure

| Layer | Element | Responsibility | Change in this feature |
|-------|---------|----------------|------------------------|
| Producer | `NotifyRustSink::send` | Enqueue the notification only | None |
| Worker lifecycle | `NotifyRustSink::new` | Start the worker thread; on unix, hand the production injection points to `notify_worker` | Unix hand-off only |
| Worker orchestration (unix) | `notify_worker` | Per notification: redact, fetch capabilities once (injected), decide via `escape_for_send`, send (injected), log | Takes the two injection points |
| Pure decision (unix) | `escape_for_send`, `body_markup_absence_confirmed`, `escape_body_markup` | Decide and apply the markup escape | None (doc comments only) |
| I/O edge (unix) | Capability-fetch and send injection points | Production: notify-rust. Tests: fakes | New |
| Worker (Windows) | `notify_worker` (Windows) | Send the received title / body directly; no capability fetch, no escape | None |

Allowed dependency directions:

- Worker orchestration → pure decision.
- Worker orchestration → I/O edge, only through the injection points.
- Pure decision → nothing that performs I/O.
- Tests → pure decision and worker orchestration; never → notify-rust or D-Bus.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `escape_for_send` (existing, unix-only, unchanged) | The single point that decides and applies the markup escape for summary and body | Signature: escape_for_send(title text, body text, capability-fetch outcome) → (summary text, body text). The outcome is either a success carrying the list of capability names or a failure of any error type. Pre: none — pure, no I/O. Post: outcome is a success AND the list has no `body-markup` → returns title and body unchanged; otherwise → returns both passed through `escape_body_markup`. One capability evaluation decides both values. Name, signature and behavior are unchanged (pinned by the existing `summary_markup_escape` tests). | task0001, task0002 |
| `escape_body_markup` (existing, unix-only, unchanged) | Escape markup-significant characters of one text | Pure. Examples of the unchanged behavior: `a<b` → `a&lt;b`; `Tom & <b>` → `Tom &amp; &lt;b&gt;`. Its direct unit tests stay as they are. | task0001, task0002 |
| `body_markup_absence_confirmed` (existing, unix-only, unchanged) | True only when the fetch succeeded and the list has no `body-markup` | Pure. Called only from inside `escape_for_send` in production; its direct unit tests stay as they are. No other code branches on its result to decide whether to escape. | task0001, task0002 |
| `src-tauri/src/callbacks/tests.rs` (shared file) | Test home for both tasks | Edited only inside the region each task owns — see decision D2 | task0001, task0002 |

## Conventions

- **Platform gating**: `escape_for_send`, `escape_body_markup`,
  `body_markup_absence_confirmed`, the capability-fetch injection point and the
  new worker-level tests are unix-only (`cfg(unix)`). No task edits
  Windows-gated code.
- **No escape branch outside `escape_for_send`**: neither production code nor
  tests branch on `body_markup_absence_confirmed` to decide whether to call
  `escape_body_markup`. Tests get escaped / unescaped values from
  `escape_for_send`, or assert literal expected strings.
- **No D-Bus in tests** (NFR2): no test reaches notify-rust's capability query or
  send.
- **Logging**: texts and levels are unchanged — success is debug
  `notify-rust dispatched: {redacted}`, failure is warn `notify-rust failed: {e}`.
- **Comments name the actual caller**: the production caller of
  `escape_for_send` / `escape_body_markup` is `notify_worker`;
  `NotifyRustSink::send` only enqueues. Only the comments listed in FR6 change.
- **Commands**: run cargo from the project root with
  `CARGO_TARGET_DIR=src-tauri/target` and `--manifest-path src-tauri/Cargo.toml`;
  unit tests are in `--lib`.

## Cross-task Design Decisions

### D1: `escape_for_send` is the only escape decision point

- **Decision**: `notify_worker` passes the fetch outcome and the received title /
  body to `escape_for_send` and forwards only its return values to the send
  injection point. `tests.rs` calls `escape_for_send` at every place that
  previously copied the branch. `escape_for_send` itself is not modified.
- **Affected tasks**: task0001, task0002

### D2: Region ownership in `src-tauri/src/callbacks/tests.rs`

| Region | Owner |
|--------|-------|
| `mod body_markup_escape`, including its leading comment and the comments before the replaced tests | task0002 |
| A new unix-only submodule for worker-level tests, added as a new block after the existing submodules | task0001 |
| Everything else (`summary_markup_escape`, `worker_thread`, other modules) | Not edited by any task |

- **Affected tasks**: task0001, task0002

### D3: Behavior preservation (NFR1)

- **Decision**: no task changes the production summary / body bytes, the escape
  rules and fail-closed decision, the redaction position (received values,
  before the escape), the one-fetch-per-notification rule (no caching), log
  texts and levels, the queue capacity, the worker thread start / stop
  procedure, or the Windows send flow. A change to any of these is a defect.
- **Affected tasks**: task0001, task0002

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Merge conflict in `tests.rs` between the two tasks | Low | Low | Region ownership (D2); the worker-level tests are a new block |
| Windows build breaks because the unix `notify_worker` changes its parameters | Medium | Medium | Windows-gated code untouched; the thread-start call is split by platform if it is shared; Windows target check (VERIFICATION TS-10) |
| A worker-level test hangs because the queue is never closed | Low | Medium | Close the sending side before running the worker to completion; join any spawned thread before asserting |
| A test reaches D-Bus through the production injection points | Low | Medium | Worker-level tests pass fakes only; review check (NFR2) |
| Log text, redaction position or fetch count drifts during the refactor | Low | High | D3; diff review (VERIFICATION TS-12); fetch count pinned by a test (TS-7) |
| Unrelated suites (tabs replay, tmux socket discovery) flake in a parallel `--lib` run | Medium | Low | Re-run the failing unrelated test with a single test thread before treating it as a regression |

## Open Questions

- None.
