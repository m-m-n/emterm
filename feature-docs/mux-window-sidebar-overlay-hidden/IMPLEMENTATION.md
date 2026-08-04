# Implementation Plan: mux-window-sidebar-overlay-hidden

## Overview

Single-task bug fix: the pump logic in `src-tauri/src/app.rs` gains an
attach-transition rule that re-opens the overlay mux window sidebar when the
active tab goes from not-mux-attached to mux-attached, restoring the AC-7
"default open" guarantee at startup and on reattach. One runtime boolean
assignment plus inline unit tests; no other file changes.

## Technology Stack

- **Language / Framework**: Rust — existing `emterm` binary, GUI-gated module
  `src-tauri/src/app.rs`. No new modules, no feature-gate changes.
- **New dependencies**: none. Project license (MIT) is unaffected — there is
  nothing to record against the license-compatibility check.

## Layer Structure

Only the App pump/state layer is touched (the `pump_all` bookkeeping block
that already owns the detach-side reset of the overlay flag). The mux
protocol handler (`src-tauri/src/tabs.rs`), the mux daemon/bridge, the
settings schema, and the overlay rendering path are all unchanged (NFR1,
NFR2).

## Shared Components

None — this feature is a single task; no cross-task contracts are needed.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| — | — | — | — |

## Conventions

- Tests are inline `#[cfg(test)]` unit tests in `src-tauri/src/app.rs`,
  reusing the existing test infrastructure (the mux-window app fixture
  helper, the overlay-mode settings helper, and the Welcome/Detached message
  delivery route the existing overlay tests already use).
- Code positions are anchored by code shape (the detach-guard block inside
  `pump_all`), never by SPEC.md line numbers — see D1 below.

## Cross-task Design Decisions

### D1: Verified code anchors (SPEC.md line numbers have drifted)

SPEC.md's flagged-unverified line claims were re-verified against the
integration worktree at planning time (2026-08-03):

| Claim in SPEC.md | Verified location | Status |
|---|---|---|
| Detach guard at `app.rs:3922-3929` | `app.rs:3979-3986` (inside `pump_all`: compute active-tab attach state → same-tab attached→not-attached resets the flag → update `active_mux_attached_prev_pump`) | **Drifted ~57 lines** |
| Flag initialized open at `app.rs:921` | `app.rs:921` (`mux_sidebar_overlay_open` set open at construction; field declared near `app.rs:414`) | Confirmed |
| User toggle at `app.rs:3100` | `app.rs:3100` (inside `dispatch_mux_action`) | Confirmed |

Rationale: review and verify phases must not fail traceability against the
stale SPEC numbers. All downstream work anchors on the detach-guard block's
shape, not on absolute line numbers (which will drift again once the change
lands). Affected tasks: task0001.

### D2: Attach-rule predicate pinned to the bookkeeping field transition

FR1 pins the predicate on `active_mux_attached_prev_pump` literally: the
rule fires when that field held no value at the end of the previous pump AND
the active tab is mux-attached in the current pump. The rule is
unconditional (no gate on the `window_sidebar_overlay` setting), mirroring
the existing detach guard's ungated shape — in persistent mode the flag
drives no rendering, so the assignment is inert there.

Two behavioral consequences are **accepted and intended**, not defects:

1. Reattach after an explicit user close re-opens the sidebar (FR3,
   explicitly accepted in REQUIREMENTS.md 14.1).
2. Switching the active tab from a non-mux tab to a mux-attached tab also
   satisfies the None→attached transition and re-opens the sidebar. This is
   unavoidable with the existing single-slot bookkeeping (the field cannot
   distinguish "this tab just attached" from "an already-attached tab just
   became active"), and extending the bookkeeping would exceed NFR1's
   minimal-change confinement. It is consistent with the always-open design
   intent (an idle overlay renders dimmed, per the existing
   idle-opacity design).

Affected tasks: task0001. Reviewers should treat consequence 2 as
by-design; changing the predicate would deviate from FR1.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Line numbers drift further before implementation | Medium | Low | Anchor by the detach-guard block's shape (D1), not line numbers |
| Review flags the switch-to-mux-tab reopen (D2 consequence 2) as a bug | Medium | Low | Documented as accepted in D2, in the task plan, and in VERIFICATION.md |
| `tabs.rs` replay tests flake during the full-suite regression run | Medium | Low | Pre-existing, unrelated flakiness; rerun with a single test thread per project convention |

## Open Questions

- None. All requirements are `status: ok`; no TBD, no license conflict, no
  pre-existing planning artifacts.
