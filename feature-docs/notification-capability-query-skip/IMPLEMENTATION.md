# Implementation Plan: notification-capability-query-skip

## Overview

On Unix, the notification worker (`notify_worker` in
`src-tauri/src/callbacks.rs`) stops querying notify-rust capabilities for every
notification: the query runs only when the title or the body contains a markup
metacharacter (`&`, `<`, `>`), at most once per notification and never cached.
The text handed to notify-rust and the fail-closed escape guarantee stay
byte-for-byte identical to the current implementation.

## Technology Stack

- **Language**: Rust — existing `src-tauri` crate; the change stays inside
  `src-tauri/src/callbacks.rs` and its test file. No new module file.
- **Key libraries**: notify-rust 4 — existing optional dependency of the `gui`
  feature; its capability query and notification builder are used exactly as
  today.
- **New dependencies**: none (NFR5). No license check is triggered; the
  project license (MIT) is unaffected.

## Layer Structure

No layer changes. The pipeline introduced by notification-worker-thread stays
as is; only the capability-query step on the worker becomes conditional.

```
Producers (OSC 9 / tab activity / agent status / link hover)   — unchanged
  -> NotificationSink boundary                                  — unchanged
  -> NotifyRustSink enqueue (bounded, non-blocking)             — unchanged
  -> worker thread "emterm-notify" (notify_worker):
       1. redaction from the raw received values                — unchanged (NFR4)
       2. [unix only] on-demand escape gate                     — CHANGED
            metacharacter check -> capability query only if needed
            -> existing fail-closed decision
       3. dispatch to notify-rust, one log record               — unchanged
```

Dependency direction is unchanged: the gate depends on the existing escape
helpers; nothing new depends on the gate except `notify_worker`.

## Shared Components

This feature is a single task (task0001). The table below is not a
between-task contract; it defines the boundary that the review and verify
phases check the implementation against (same convention as
notification-worker-thread).

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Markup metacharacter set (new named constant, Unix-only) | The characters whose presence makes a capability query necessary | Exactly `&`, `<`, `>` — the same three characters `escape_body_markup` rewrites. Quotes and every other character are excluded. Defined once; the predicate reads it | task0001 |
| `contains_markup_meta` (new, private, Unix-only) | Report whether one text contains any metacharacter | Pre: none. Post: true iff at least one character of the set occurs in the text; false for the empty string. Pure, total, no I/O | task0001 |
| `escape_for_send_on_demand` (new, private, Unix-only) | Conditional capability query plus the fail-closed escape decision for one notification | Inputs: borrowed title, borrowed body, and a capability-query supplier that is consumed when called (so it can run at most once) and yields either a list of capability names or an error of any type. Output: owned (summary, body) pair. Post: (1) neither field contains a metacharacter -> supplier never called, pair equals the inputs byte-for-byte; (2) otherwise -> supplier called exactly once, pair equals what `escape_for_send` returns for the same title, body and that single result. Keeps no state between calls | task0001 |
| `escape_for_send` (existing, signature and behavior unchanged) | Fail-closed decision from an already-evaluated capability result | Unchanged: pass-through only when the result is a success whose list omits `body-markup`; every other result escapes both fields via `escape_body_markup`. Its existing tests keep their expectations (NFR6) | task0001 |
| `notify_worker` (existing, one call site changed) | Per-notification pipeline on the worker thread | Order unchanged: redaction on the raw values -> Unix-only on-demand gate, with the notify-rust capability query handed over un-invoked -> dispatch -> one success or failure log record | task0001 |

## Conventions

- **Placement**: all production changes in `src-tauri/src/callbacks.rs`; new
  tests in `src-tauri/src/callbacks/tests.rs`, inside a Unix-gated test
  module like the existing escape-gate modules.
- **Platform gating**: every new item (constant, predicate, gate) is
  `#[cfg(unix)]`, like the existing escape helpers. `notify_worker` keeps its
  single Unix-gated binding for the escape step; enqueue, receive and dispatch
  get no new cfg branches (NFR3).
- **No caching**: no process-global, thread-local or memoized capability state
  of any kind — no lazily-initialized statics, no TTL, no cross-call memo
  (NFR1).
- **Comments**: code comments and doc comments are written in English, like
  the rest of the file. Historical feature documents are never edited (SPEC
  A4).
- **Logging**: no new log records; existing record prefixes and levels stay
  unchanged.

## Cross-task Design Decisions

### D1: Metacharacter short-circuit, not caching

The capability query is skipped when neither field contains `&`, `<` or `>`
(task option (a)). Caching (option (b)) is rejected by NFR1. Moving dispatch
off the hot paths (option (c)) already exists (notification-worker-thread).

This supersedes notification-worker-thread D3 "exactly once per notification,
never cached" with "at most once per notification, only when a metacharacter
is present, never cached". The record of that change lives in this feature's
SPEC; `feature-docs/notification-worker-thread/` is not edited.

### D2: Delegate to the unchanged `escape_for_send`

The new gate does not re-implement the fail-closed rule; when a metacharacter
is present it passes the single query result to the existing
`escape_for_send`. When no metacharacter is present, returning the inputs is
byte-identical to what `escape_for_send` would have produced for any query
outcome, because `escape_body_markup` is the identity on text without the three
characters. That identity property is pinned by a dedicated test so a future
change to the escaped character set cannot silently desynchronize the
short-circuit (see Risk Assessment).

### D3: Deferred supplier injection

The gate receives the capability query as a deferred supplier rather than a
pre-computed result. Production hands over the notify-rust capability query
itself, un-invoked; tests hand over a counting stub. The supplier is consumed
on call, so "at most once" is guaranteed by the signature, and tests observe
"called or not, and how many times" without a D-Bus connection (FR4).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The metacharacter set drifts from the set `escape_body_markup` rewrites, so the short-circuit skips a needed escape (markup injection) | Low | High | Single named constant; identity-property test over printable ASCII (VERIFICATION TS-8) fails on any drift |
| The short-circuit inspects only the body, so a title-only metacharacter (for example an OSC 9 fallback tab title) skips the query | Medium | High | Title-only rows under all three query outcomes (TS-2) |
| The production call site evaluates the query eagerly (passes a result instead of the supplier), silently losing the FR1 benefit while unit tests still pass | Medium | Medium | Static review check (TS-10); manual D-Bus observation (VERIFICATION Manual Testing) |
| A memoization shortcut is introduced | Low | High | No-caching test (TS-7) and static review check (TS-10) |
| A new cfg branch leaks into the dispatch path, or a Unix-only item is referenced on Windows | Low | Low | Unix-only placement rule above; `--no-default-features` check; optional Windows cross-build (Manual Testing) |

## Open Questions

None.
