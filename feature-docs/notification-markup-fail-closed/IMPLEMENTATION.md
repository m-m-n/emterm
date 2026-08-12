# Implementation Plan: notification-markup-fail-closed

## Overview

Invert the capability decision of the Linux notification escape gate in
`NotifyRustSink::send` from fail-open to fail-closed: notification text passes
through unescaped only when the capability query succeeds AND the returned list
explicitly omits `body-markup`; every other outcome — including a failed query —
escapes both title and body. Single-file security fix closing PR #35 review
finding `eade9e7f97a29a29`.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate; no new component)
- **Key libraries**: `notify-rust` — existing optional dependency of the `gui`
  feature, supplying the capability query and the notification send call.
  **No new dependency is introduced by this feature.** License review: no new
  license enters the tree; the project license (MIT) is unaffected.

## Layer Structure

No layer changes. The change is confined to the notification-sink area of
`src-tauri/src/callbacks.rs` (implementation and doc comments) and
`src-tauri/src/callbacks/tests.rs` (unit tests), entirely inside the existing
`#[cfg(feature = "gui")]` module gating and the `#[cfg(unix)]` capability gate.

## Shared Components

None — this feature is a single task; no component contract is shared between
tasks.

## Conventions

- Feature/platform gate hygiene (NFR1, FR4): the capability decision and the
  escaping stay under `#[cfg(unix)]`; the Windows notification path receives no
  change; the CLI-only (`--no-default-features`) build must keep compiling.
- Documentation consistency (NFR2): after the change, no doc comment or test
  comment in the touched files may still describe fail-open semantics; the
  capability predicate's name must state its new fail-closed meaning.
- Untouchable surroundings (NFR3): `sanitize_title` behavior, the escape
  ordering (`&` replaced first; escaping applied after truncation), and the
  notification rate limiter are out of bounds. The only changed logic is the
  capability decision branch.

## Cross-task Design Decisions

Single-task feature — the normative decision table and all per-task design
detail live in `tasks/task0001.md`. The specification-level record of
fail-closed (FR5) is SPEC.md itself ("Decision Table" section), which
supersedes FR3 of `feature-docs/notification-body-markup-escape/SPEC.md`.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Over-escaping on capability-query failure degrades display on plain-text servers (literal `&lt;`) | Low | Low | Accepted by SPEC (failure-cost asymmetry); FR2 keeps the explicit-absence path unescaped |
| Existing fail-open test expectations survive unchanged, silently weakening the fix | Low | Medium | task0001 enumerates the exact tests whose expectations invert; TS1 pins the new `Err` path |

## Open Questions

None.
