# Implementation Plan: selection-anchor-pivot

## Overview

Word-mode (double-click) and line-mode (triple-click) drag selection keeps
the originally clicked word / line as an immutable pivot; both endpoints are
recomputed from that origin on every extension instead of mutating the
previously snapped anchor.

## Technology Stack

- **Language**: Rust (native terminal stack, `src-tauri` crate) — no new
  dependencies.

## Layer Structure

- Selection model (`src-tauri/src/selection.rs`) owns all range computation.
- Event wiring (`src-tauri/src/window_host.rs`) stays a thin caller
  (press → construct, motion → extend). Dependency direction unchanged:
  window_host → selection.

## Shared Components

Single-task feature — no components are shared between tasks.

## Conventions

- Every stored selection position, including the new origin, uses the
  absolute buffer-row coordinate convention (`Pos.row`: scrollback rows first,
  then live viewport rows) established by the F9 absolute-row model.

## Cross-task Design Decisions

### Origin-based stateless recomputation

Each extension derives both endpoints from the pair (immutable origin,
current pointer position) instead of incrementally mutating the stored
anchor. Rationale: makes the selection range a pure function of the origin
and the latest pointer position, so drag semantics are independent of how
many motion events occurred (the root cause of the reported bug).
Affected task: task0001 (sole task).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Adding a field to the selection type ripples to construction sites and existing tests | Medium | Low | Trace all construction sites from the two constructors; keep existing single-extend tests passing unmodified |
| Content changes under the pointer between extensions alter word boundaries | Low | Low | Boundaries are recomputed against the live core at each extension (existing rule; only the origin position itself is retained) |

## Open Questions

- None.
