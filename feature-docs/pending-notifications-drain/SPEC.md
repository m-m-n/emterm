# Feature: pending-notifications-drain

## Overview

`NativeCallbackState::pending_notifications` (`src-tauri/src/callbacks.rs:391`,
`Vec<(String, String)>`) accumulates the raw `(title, body)` of every OSC 9
notification pushed in `handle_notify` (callbacks.rs:563-566), but no production
consumer ever drains it. This feature removes the buffer, the push site, and the
doc comment that describes a non-existent `Tab::pump` drain contract, and rewrites
the two tests that observed the field so they assert on the `TestSink` receive
log instead. Delivery itself stays exactly as it is today: an inline synchronous
`self.sink.send(&title, &body)`.

Requirements document: `feature-docs/pending-notifications-drain/REQUIREMENTS.md`
(Japanese; the normative statement of every requirement below).

## Objectives

- **OBJ1:** Stop the monotonic growth of `NativeCallbackState::pending_notifications`
  so that a burst of OSC 9 sequences originating from terminal output
  (attacker-influenceable data) cannot grow process memory without bound.
- **OBJ2:** Ensure raw notification title / body are not retained indefinitely in
  process memory, resolving the contradiction with the osc9-notify-log-redaction
  (PR #41) threat model — that strings scrubbed from logs survive in core dumps
  and memory scans.
- **OBJ3:** Resolve the divergence between the doc comment at
  `src-tauri/src/callbacks.rs:388-390` (which states that `Tab::pump` drains the
  buffer and that no D-Bus round-trip happens inside the callback) and the actual
  implementation.

## User Stories

### US1: Bounded memory under OSC 9 bursts
As an eMterm user, I want a stream of OSC 9 notifications not to grow the
process's memory without bound, so that terminal output cannot inflate eMterm's
footprint.

**Acceptance Criteria:**
- [ ] AC2: Sending OSC 9 repeatedly with mutually distinct `(title, body)` pairs
  leaves no in-process buffer retaining those raw strings (no reference remains
  after delivery).
- [ ] AC1: The identifier `pending_notifications` appears nowhere in
  `src-tauri/src/callbacks.rs` or `src-tauri/src/callbacks/tests.rs` (the
  same-named local variable in `src-tauri/src/app/mod.rs` is out of scope).

### US2: Notification content not retained in process memory
As an eMterm user, I want raw notification title / body not to be kept in process
memory after delivery, so that redaction applied to logs is not undone by a core
dump or memory scan.

**Acceptance Criteria:**
- [ ] AC2: No in-process buffer retains the raw strings after delivery.
- [ ] AC7: The three rate-limiter tests (tests.rs:540-569) still pass unmodified,
  showing that dedupe and the redacted suppression log are unchanged.

### US3: Doc comment that matches the implementation
As an eMterm maintainer, I want `src-tauri/src/callbacks.rs` to carry no doc
comment describing a drain contract that does not exist, so that the file does
not mislead future changes.

**Acceptance Criteria:**
- [ ] AC3: No doc comment describing a non-existent drain contract remains in
  `src-tauri/src/callbacks.rs`.

## Technical Requirements

### Functional Requirements

- **FR1 — Remove the push in `handle_notify`:** Delete
  `self.state.lock().pending_notifications.push((title.clone(), body.clone()))`
  at `src-tauri/src/callbacks.rs:563-566`. The emit branch then contains only
  `self.sink.send(&title, &body)`.
- **FR2 — Remove the `pending_notifications` field:** Delete
  `pub pending_notifications: Vec<(String, String)>` at
  `src-tauri/src/callbacks.rs:391` from `NativeCallbackState`. Replacing it with a
  bounded `Vec`, or implementing buffer-then-drain (remediation options b / c), is
  not adopted.
- **FR3 — Remove the diverged doc comment:** Along with the field, delete the doc
  comment at `src-tauri/src/callbacks.rs:388-390` ("Pending OSC 9 notifications,
  drained by `Tab::pump` … no D-Bus round-trip inside `process_pty_data`") so that
  no comment describing a non-existent drain contract remains.
- **FR4 — Keep delivery as an inline `sink.send`:** OSC 9 notifications keep being
  delivered by the inline synchronous `self.sink.send(&title, &body)` inside
  `handle_notify`; delivery target, delivery count and delivery timing do not
  change. No new drain path is added to `Tab::pump` or
  `Tab::process_outer_via_core`.
- **FR5 — Rewrite the emit-observing test onto sink receipts:** Delete
  `assert_eq!(h.state.lock().pending_notifications.len(), 1)` at
  `src-tauri/src/callbacks/tests.rs:135` (inside `osc_9_emits_notification`) and
  verify the emit through the assertion on `h.sink.calls()` (the `TestSink`
  receive log, tests.rs:16-27) that the test already carries.
- **FR6 — Rewrite the non-emit-observing test onto sink receipts:** Replace
  `assert!(s.pending_notifications.is_empty())` at
  `src-tauri/src/callbacks/tests.rs:316` (inside
  `osc_133_callback_is_a_noop_for_native_state`, which checks that OSC 133 does not
  modify `NativeCallbackState` at all) with an assertion that `TestSink` received
  nothing (equivalent to `h.sink.calls().is_empty()`).
- **FR7 — Keep the rate limiter and suppression log unchanged:** The
  `NotificationRateLimiter`'s 1-second window and `(title, body)` key, and the
  `LOG_NOTIFY_RATE_LIMIT` warn log emitted on suppression (redacted output via
  `redact_notification`, callbacks.rs:568-577), are not modified. The existing
  `rate_limiter_dedupes_identical_pair_within_window` /
  `rate_limiter_allows_after_window_elapsed` /
  `rate_limiter_distinct_pairs_not_deduped` (tests.rs:540-569) keep passing
  unmodified.

### Non-Functional Requirements

- **NFR1 — Invariance of observable behaviour:** Everything observable to the user
  (content, order and count of notifications reaching D-Bus / toasts, how the
  dedupe window behaves, the log line on suppression) is identical before and
  after this change. Because no production consumer of `pending_notifications`
  exists (the identifier occurs in exactly four places: the declaration, the push,
  and two tests), removing it does not change behaviour.
- **NFR2 — No replacement buffer:** Do not introduce any other in-process buffer
  holding raw title / body (bounded `Vec`, ring buffer, log cache, etc.). To
  satisfy OBJ2, no structure that retains the raw strings after delivery may
  remain.
- **NFR3 — Bounded change surface:** Changes are limited to
  `src-tauri/src/callbacks.rs` and `src-tauri/src/callbacks/tests.rs`. The
  same-named local variable at `src-tauri/src/app/mod.rs:1008`
  (`Vec<(String, ActivityKind)>`, the tab-activity notification path) and
  `NativeCallbackState`'s sibling fields (`osc_queue` / `bell_count` /
  `pending_apc` / `pending_dcs` and others) are not touched. The docs of prior
  features (the SPEC / REQUIREMENTS of osc9-notify-log-redaction and
  notification-summary-markup-escape) are not rewritten.
- **NFR4 — Feature-gate soundness:** Do not break compilation of the CLI-only
  build (`--no-default-features`). `callbacks` is a GUI-only module, but the
  feature-gate check is carried out by the existing procedure.
- **NFR5 — Conformance to the test conventions:** Tests follow `test/README.md`:
  they live in a `#[cfg(test)] mod tests` co-located with the code under test and
  assert on the observable contract (the `TestSink` receive log) rather than on
  internal state. Naming follows the existing `<subject>_<scenario>_<expected>`.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────┐
│  Terminal output (untrusted)        │
│  OSC 9 "<title>;<body>"             │
├─────────────────────────────────────┤
│  callbacks.rs :: handle_notify      │
│    NotificationRateLimiter          │
│      (1s window, (title, body) key) │
├─────────────────────────────────────┤
│  NotificationSink::send(title, body)│
│    inline, synchronous              │
├─────────────────────────────────────┤
│  D-Bus / toast                      │
└─────────────────────────────────────┘
```

**Component Diagram:**
```
handle_notify ──> NotificationRateLimiter ──allowed──> sink.send(&title, &body)
      │                                  └─suppressed─> LOG_NOTIFY_RATE_LIMIT warn
      │                                                 (via redact_notification)
      └──(removed by FR1/FR2) NativeCallbackState::pending_notifications
```

### Data Flow

```
Terminal → OSC 9 → handle_notify → rate limiter → sink.send → D-Bus / toast
                                        └─ suppressed → redacted warn log

Removed: handle_notify → state.lock().pending_notifications.push((title, body))
```

After the change, the raw title / body live only for the duration of the
`handle_notify` call; no process-local structure retains them (NFR2, AC2).

### API Design

Not applicable — this feature exposes no HTTP or RPC endpoint. The only interface
involved is the internal `NotificationSink::send(&title, &body)`, whose delivery
target, count and timing are unchanged (FR4).

### Database Schema

Not applicable — no persistent data model. The only data change is the removal of
the in-process field:

| Owner | Field | Type | Change |
|-------|-------|------|--------|
| `NativeCallbackState` | `pending_notifications` | `Vec<(String, String)>` | removed (FR2) |
| `NativeCallbackState` | `osc_queue` / `bell_count` / `pending_apc` / `pending_dcs` and others | — | unchanged (NFR3) |

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/callbacks.rs`: `handle_notify`, `NativeCallbackState`,
  `NotificationRateLimiter`, `redact_notification`.
- `src-tauri/src/tabs/mod.rs`: constructs `NativeCallbackState::default()` at :696
  (the only construction site); holds the `osc_queue` `std::mem::take` at :1259,
  which stays untouched.
- `src-tauri/src/tabs/output_pipeline.rs`: `bell_count` `std::mem::take` at :282 —
  untouched.
- `src-tauri/src/callbacks/tests.rs`: `TestSink` (:6-27), `Harness` with injected
  clock (:31-65).

**External Dependencies:**
- None added. Testing uses Rust's built-in `cargo test`; there is no proptest /
  criterion in this project.

### File Structure

```
src-tauri/src/
├── callbacks.rs            # field :391, doc comment :388-390, push :563-566,
│                           # suppression log :568-577  (FR1, FR2, FR3, FR4, FR7)
└── callbacks/
    └── tests.rs            # TestSink :6-27, Harness :31-65, :135, :138-147,
                            # :316, rate-limiter tests :540-569  (FR5, FR6, FR7)
```

## Test Scenarios

### Unit Tests
- [ ] **TS1** (`src-tauri/src/callbacks/tests.rs`) — one OSC 9
  `"Build done;all green"` → `TestSink.calls()` holds exactly 1 entry with
  title = "Build done" and body = "all green". Covers AC5 (FR4, FR5).
- [ ] **TS2** (`src-tauri/src/callbacks/tests.rs`) — OSC 9 without a separator
  after a title was set by OSC 2 → the title received by `TestSink` falls back to
  the preceding OSC 2 title (existing `osc_9_no_separator_uses_fallback_title`,
  tests.rs:138-147). Covers AC5 and NFR1 (FR4, FR5, NFR1).
- [ ] **TS3** (`src-tauri/src/callbacks/tests.rs`) — emitting OSC 133 (`"A"` /
  `"D;42"`) leaves `NativeCallbackState`'s `title` / `osc_queue` unchanged and
  `TestSink` receives 0 entries. Covers AC6 (FR6).
- [ ] **TS4** (`src-tauri/src/callbacks/tests.rs`) — two consecutive OSC 9 with the
  same `(title, body)` yield 1 `TestSink` receipt; a re-send after advancing the
  injected clock by 2 seconds yields 2; three distinct pairs yield 3 (the three
  existing tests, unmodified). Covers AC7 (FR7, NFR1).

### Integration Tests
None. This feature's verification is covered by the unit tests above and the
build-level scenario below.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] OSC 9 without a separator: the title falls back to the preceding OSC 2 title
  (TS2).
- [ ] Repeated identical `(title, body)` within the 1-second window: delivery is
  suppressed and the redacted `LOG_NOTIFY_RATE_LIMIT` warn line is emitted; the
  behaviour is unchanged by this feature (TS4, FR7).
- [ ] A burst of mutually distinct `(title, body)` pairs: every pair is delivered
  and none is retained in an in-process buffer afterwards (AC2).

### Performance Tests
None — this feature states no performance goal.

### Build / Feature-gate Verification
- [ ] **TS5** (project root, build level) — both the default-feature `--lib` test
  run and the `--no-default-features` `cargo check` succeed. Covers AC4 and AC8
  (FR5, FR6, FR7, NFR4, NFR5).

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
```

## Security Considerations

- **Data Protection:** Raw notification title / body — which originate from
  terminal output and are therefore attacker-influenceable — are no longer
  retained in process memory after delivery (OBJ2, NFR2, AC2). No replacement
  buffer (bounded `Vec`, ring buffer, log cache) is introduced.
- **Unbounded growth:** Removing the push site removes the unbounded accumulation
  path that a stream of OSC 9 sequences could drive (OBJ1, FR1, FR2).
- **Log redaction:** Suppression logging keeps going through `redact_notification`
  (callbacks.rs:568-577) unchanged (FR7).
- **Out of scope:** log-side redaction (delivered by PR #41
  osc9-notify-log-redaction), markup escaping of notification bodies (delivered by
  PR #39), and introducing zeroing of Rust `String` memory on drop (zeroize-style).

## Error Handling

No new error path is introduced. The only non-delivery path is rate-limit
suppression, which stays as-is:

| Condition | Behaviour |
|-----------|-----------|
| Identical `(title, body)` within the 1-second window | Delivery suppressed; `LOG_NOTIFY_RATE_LIMIT` warn logged through `redact_notification` (unchanged, FR7) |

## Performance Optimization

No performance goal is specified for this feature. Delivery timing is explicitly
unchanged (FR4), and observable behaviour is invariant (NFR1).

## Success Criteria

- [ ] **AC1:** The identifier `pending_notifications` remains in neither
  `src-tauri/src/callbacks.rs` nor `src-tauri/src/callbacks/tests.rs` (the
  same-named local variable in `src-tauri/src/app/mod.rs` is out of scope).
  [FR1, FR2, FR3, FR5, FR6, NFR3]
- [ ] **AC2:** Sending OSC 9 repeatedly with mutually distinct `(title, body)`
  pairs leaves no in-process buffer retaining those raw strings (no reference
  remains after delivery). [OBJ1, OBJ2, NFR2]
- [ ] **AC3:** No doc comment describing a non-existent drain contract remains in
  `src-tauri/src/callbacks.rs`. [OBJ3, FR3]
- [ ] **AC4:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  passes. [FR5, FR6, FR7, NFR5]
- [ ] **AC5:** `osc_9_emits_notification` verifies that a single OSC 9 makes
  `TestSink` receive `("Build done", "all green")` exactly once. [FR4, FR5]
- [ ] **AC6:** `osc_133_callback_is_a_noop_for_native_state` verifies that OSC 133
  makes `TestSink` receive nothing. [FR6]
- [ ] **AC7:** The three rate-limiter tests (tests.rs:540-569) pass unmodified.
  [FR7, NFR1]
- [ ] **AC8:** `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  passes. [NFR4]

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — FR1-FR7 and NFR1-NFR5 are all resolved.

## Design Step

Skipped. The change touches only a Rust internal state field, a doc comment and
unit tests — no UI surface, no design tokens, no WebView. The adopted
remove-buffer approach does not change delivery timing, so no behavioural change
requiring design review arises.

## Assumptions

- **assume.remove-buffer-chosen** (irreversible): The remediation approach is
  "(a) remove the buffer" (batch answer `requirement.remediation-approach`;
  source: batch-codex-consultation). Options (b) implementing buffer-then-drain
  and (c) a bounded `Vec` plus a doc-comment fix are not adopted.
- **assume.no-production-consumer** (reversible): No production consumer of
  `pending_notifications` exists — the identifier occurs across the repository
  only at callbacks.rs:391 / :565 and tests.rs:135 / :316, and
  `NativeCallbackState::default()` is constructed only at tabs/mod.rs:696, so no
  implicit clearing through re-initialization exists either. Removal therefore
  does not change behaviour.
- **assume.sibling-fields-untouched** (reversible): The buffer-then-drain pattern
  of the sibling fields (`osc_queue` → `std::mem::take` at tabs/mod.rs:1259,
  `bell_count` → `std::mem::take` at tabs/output_pipeline.rs:282) is out of scope
  for this feature and stays as-is.
- **assume.prior-spec-not-rewritten** (reversible): osc9-notify-log-redaction's
  SPEC NFR1 declaring `pending_notifications` buffering as an invariant was a
  scope declaration made at that time; this feature supersedes it. The
  feature-docs of prior features are not rewritten.
- **assume.app-mod-local-out-of-scope** (reversible): The same-named local
  variables at `src-tauri/src/app/mod.rs:1008` / :1264 / :1367 differ in both type
  and path (a one-frame latch for tab-activity notifications) and are therefore
  out of scope for this feature.

## Out of Scope

- Log-side redaction (already delivered by PR #41 osc9-notify-log-redaction).
- Markup escaping of the notification body (already delivered by PR #39).
- Changing the drain pattern of the sibling fields (`osc_queue` / `bell_count`
  and others).
- The same-named local variable in `src-tauri/src/app/mod.rs` (tab-activity
  notification path).
- Introducing zeroing of Rust `String` memory on drop (zeroize-style).

## References

- REQUIREMENTS.md: `feature-docs/pending-notifications-drain/REQUIREMENTS.md`
- Target implementation: `src-tauri/src/callbacks.rs` (field :391, doc comment
  :388-390, push :563-566, suppression log :568-577)
- Target tests: `src-tauri/src/callbacks/tests.rs` (`TestSink` :6-27, `Harness`
  :31-65, :135, :138-147, :316, rate-limiter tests :540-569)
- `src-tauri/src/tabs/mod.rs`: `NativeCallbackState::default()` construction :696,
  `osc_queue` `std::mem::take` :1259
- `src-tauri/src/tabs/output_pipeline.rs`: `bell_count` `std::mem::take` :282
- `src-tauri/src/app/mod.rs`: same-named local variables (out of scope) :1008 /
  :1264 / :1367
- `test/README.md`: test placement and naming conventions
- osc9-notify-log-redaction (PR #41): log redaction threat model and SPEC NFR1
- notification-summary-markup-escape (PR #39): notification markup escaping
