# Implementation Plan: Status Bar App Line 1 Auto-Hide

## Overview

Change the status-bar draw layer so App Line 1 is visible only when its resolved content is non-empty (the rule App Line 2 already uses), and rely on the existing zero-visible-rows early return to collapse the panel entirely.

## Technology Stack

- **Language / Framework**: Rust / egui (existing GUI draw layer) — no new dependencies

## Layer Structure

Unchanged. The change lives entirely in the egui widget layer (`src-tauri/src/ui/status_bar.rs`), which consumes the per-frame `StatusBarViewModel` built by the runtime layer. The view model and runtime layers are not modified.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `AppRow::has_content()` | Reports whether a resolved app row has any visible runs | Existing helper, reused as-is; returns true iff any run has non-empty text or a line break | task0001 |

## Conventions

- Follow the existing test style in the status_bar widget test module (view models built by hand, no font stack).

## Cross-task Design Decisions

Single-task feature — no cross-task decisions. Rationale recorded for the reviewer:

- Visibility is judged on resolved content (per-frame runs), not raw settings strings, matching the user-confirmed requirement and App Line 2's existing rule.
- Panel collapse (FR2) requires no new code path: the existing zero-row early return and row-count-based height already produce it once App Line 1 participates in auto-hide.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Existing tests assume App Line 1 always renders | Medium | Low | Update affected assertions; keep test intent (they mostly seed line 1 with content already) |
| Frame-to-frame flicker if a provider alternates empty/non-empty | Low | Low | Out of scope; providers already produce stable output and the same behavior exists for App Line 2 |

## Open Questions

None.
