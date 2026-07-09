# Implementation Plan: mux-offthread-swap-callback-restore

## Overview

Restore the tab core's wiring (callbacks + app-layer OSC 9999 registration)
across the off-thread snapshot replay core swap in `apply_offthread_swap`,
fixing the post-detach attach hang and the intermittent in-mux
viewer-launch loss.

## Technology Stack

- **Language**: Rust (src-tauri crate, `gui` feature)
- **Key modules**: `src-tauri/src/tabs.rs` (swap site + tests),
  `crates/term_core` (`TerminalCore.callbacks` public field,
  `register_osc_app_param`)

## Layer Structure

Single-task feature — no new layers. The change stays inside the existing
Tab → TerminalCore boundary: the transplant is performed by the Tab layer on
the main thread at swap time; `term_core`'s worker-side snapshot build
contract (core built with no callbacks) is unchanged.

## Shared Components

None (single task).

## Conventions

- Follow existing comment style in `tabs.rs` (explanatory step comments in
  `apply_offthread_swap` are numbered; the fix slots into step 1).
- Tests live in the existing `#[cfg(test)]` module of `tabs.rs`, following
  the existing off-thread replay test patterns (threshold-sized payloads,
  recording callback doubles as in `term_core`'s tests).

## Cross-task Design Decisions

### Transplant at swap time, on the main thread

The worker-built core stays callback-free (Send requirement). All wiring
restoration happens inside `apply_offthread_swap` while holding the core
lock, so no intermediate state where the live core lacks wiring is ever
observable by other code. Rationale: smallest change surface; keeps the
worker contract untouched. Affected task: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 2nd-pass scrollback restore also replaces the core, reintroducing the loss | Low | High | task0001 acceptance criterion verifies the merge-not-swap invariant (AC-6) |
| Other per-tab core state set at construction (beyond callbacks/OSC param) is also lost by the swap | Medium | Medium | task0001 audits `Tab::new`'s core setup for additional wiring and reports any found (implementation note, not silent scope growth) |
| Existing off-thread replay tests are order/timing sensitive | Medium | Low | run with `--test-threads=1` per project convention |

## Open Questions

- None
