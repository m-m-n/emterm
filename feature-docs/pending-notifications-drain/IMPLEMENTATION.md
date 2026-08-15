# Implementation Plan: pending-notifications-drain

## Overview

Remove the never-drained OSC 9 notification buffer from the shared native
callback state, delete the doc comment that describes a drain contract which
does not exist, and re-point the two tests that observed the buffer at the
notification sink's receive log. Delivery behaviour is unchanged: OSC 9
notifications keep being handed to the sink inline and synchronously.

## Technology Stack

- **Language**: Rust — the only language this feature touches. The TypeScript
  child-WebView bundles are outside the change surface (NFR3).
- **Framework / runtime**: none added.
- **Key libraries**: none added, none removed, none upgraded.

**License**: this feature introduces no new dependency, so no license
question arises against `project.license: MIT`. There is nothing new to
record in a dependency/license list.

## Layer Structure

| Layer | Responsibility | Touched by this feature |
|-------|----------------|-------------------------|
| Terminal-callback layer (`src-tauri/src/callbacks.rs`) | Receives dispatched OSC events, owns the notification rate-limiting decision, owns the shared callback-state struct | Yes — one state field, its doc comment, and one statement in the OSC 9 handler are removed |
| Notification sink boundary | Abstraction the callback layer calls to deliver a notification to the desktop | No — call site, target, count and timing unchanged |
| Tab / UI drain layer (`src-tauri/src/tabs/…`) | Drains the sibling buffered fields of the callback state each pump | No — no drain site is added, removed or reordered |

Allowed dependency direction is downward only: the callback layer calls the
sink boundary; the tab/UI layer reads the shared callback state. This feature
removes one field that the tab/UI layer never read.

## Shared Components

This feature is a single task (see D1), so no component contract is shared
*between* tasks. The table below instead pins the two boundaries the task must
preserve verbatim — they are contracts the task consumes without changing.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Notification sink boundary | Deliver one notification to the desktop | Pre: a title and a body, both already parsed. Post: exactly one delivery attempt per call, performed synchronously before the call returns. Unchanged by this feature (FR4) | task0001 |
| Notification rate limiter | Decide whether an OSC 9 event is emitted or suppressed | Pre: title and body. Post: emits at most one identical (title, body) pair per one-second window; on suppression the caller logs a redacted warn line under the existing marker constant. Window, key and log are unchanged by this feature (FR7) | task0001 |

## Conventions

- **Test placement / naming**: tests stay in the co-located test module for
  the callback layer; naming follows the existing `<subject>_<scenario>_<expected>`
  convention (NFR5).
- **Assertion style**: notification behaviour is asserted through the test
  sink's receive log (the observable contract), never through the shared
  callback state's internals (NFR5).
- **Error handling**: no new error path. The only non-delivery path is
  rate-limit suppression, which keeps its existing behaviour and its existing
  redacted warn line.
- **Logging**: unchanged. No log line is added, removed or reworded, and no
  raw notification title/body reaches any log.
- **Build/test invocation**: cargo is always invoked from the project root
  with an explicit manifest path and an explicit target directory; never by
  changing directory into the crate. Unit tests live in the library target.

## Cross-task Design Decisions

### D1: One task — the change surface cannot be split

The entire change lives in two files that must move together: removing the
state field makes the two test assertions that read it fail to compile, so the
production removal and the test rewrite have to land as one atomic change.
Tasks in this workflow run fully in parallel with no ordering mechanism, so
splitting the removal from the test rewrite would give two tasks the same two
files and no way to keep any intermediate state compiling. This feature is
therefore planned as exactly ONE task by deliberate decision, not by omission.

Affected: task0001.

### D2: Remove, do not replace

The adopted remediation is removal of the buffer. A bounded vector, a ring
buffer, a log cache, or a real buffer-then-drain implementation are all
rejected (assumption `assume.remove-buffer-chosen`). The consequence carried
into the task's Acceptance Criteria is that no structure retaining the raw
title/body may remain or be introduced (NFR2).

Affected: task0001.

### D3: Delivery stays inline and synchronous on the callback thread

The doc comment being deleted claimed that buffering kept the callback fast by
avoiding a desktop-notification round-trip inside PTY data processing. That
claim is false today — delivery is already inline. This feature records
reality rather than changing it (FR4, NFR1). The "blocking delivery call on
the callback thread" property is pre-existing and explicitly out of scope for
change; it is listed under Risk Assessment so review judges it as an accepted
known property rather than an oversight.

Affected: task0001.

### D4: Sibling buffered fields keep their buffer-then-drain pattern

The sibling fields of the same state struct (the viewer-request queue, the
bell counter, the graphics/SIXEL payload buffers, the clipboard read/write
queues, the agent-status feeds) keep both their drain sites and their doc
comments — several of those comments legitimately describe a tab-side drain
that does exist. The doc-comment deletion must therefore be scoped to the
notification field's comment alone (NFR3).

Affected: task0001.

### D5: "No retained buffer" is verified statically, not at runtime

The acceptance criterion "no in-process buffer retains the raw strings after
delivery" is an absence property. There is no in-test way to prove that no
reference remains, so it is verified by three static means instead:
compile-time absence of the field, an identifier-absence source check, and a
diff review confirming no replacement field was added. This is why
VERIFICATION.md carries static-source and diff-review items alongside the unit
tests, and it is stated here so the implementer does not attempt to invent a
runtime memory-inspection test.

Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| An unnoticed consumer of the removed field exists | Low | Medium | The identifier occurs in exactly four places (declaration, push, two test assertions); the compile step of the verification plan surfaces any missed consumer immediately |
| The doc-comment deletion also removes a sibling field's comment | Low | Low | D4 scopes the deletion; a narrow static source check plus diff review confirm only the notification comment is gone |
| The rewritten tests assert less than the originals did | Medium | Medium | The task's Acceptance Criteria pin the exact expected observations (one receipt with the expected pair; zero receipts for the no-op case) |
| Inline synchronous delivery blocks the callback thread | Pre-existing | Medium | Explicitly out of scope for change (D3); recorded here so review evaluates it as accepted rather than missed |
| The crate-wide format check reports pre-existing drift unrelated to this feature | Low | Low | Only the two feature files are in scope; unrelated drift is reported, not "fixed" by reformatting untouched files |

## Open Questions

- [ ] None blocking. Every requirement is `ok` in workflow.yaml, and the one
      criterion that cannot be tested at runtime (absence of retention) has an
      agreed static verification route (D5).
