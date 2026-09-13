# Implementation Plan: mux-link Pending-Switch Single Borrow

Cross-task decisions only. Per-task detail lives in `tasks/taskNNNN.md`.
Requirement IDs (FR1-FR5 / NFR1-NFR5) and assumption IDs (A1-A6) are the ones
in `feature-docs/mux-link-pending-switch-single-borrow/SPEC.md` and
`workflow.yaml`.

This feature decomposes into a single task (`tasks/task0001.md`). The sections
below carry the decisions that bind that task to the surrounding code it must
**not** change, so the task is implementable from its own plan plus this
document alone.

## Overview

Collapse the two separate look-ups of the tab's pending-switch slot inside the
PTY-output entry point into one mutable borrow, and remove the unreachable
"no pending switch" arm — the one code path on which a queued PTY payload can
be discarded with neither a log line nor a fallback. Observable behaviour,
including every log line, stays exactly as it is today.

## Technology Stack

- **Language**: Rust, the existing `src-tauri` crate only. No TypeScript /
  WebView surface is involved, and no UI or design token is touched (the
  design step is `skipped` for this reason).
- **Module modified**: `src-tauri/src/tabs/mux_link.rs` — the pending-switch
  block of the PTY-output entry point, and nothing else.
- **Modules referenced but not modified**: `src-tauri/src/tabs/replay.rs`
  (the pending-switch type, its live-queue entry point, and the two-valued
  live-queue outcome enum), `src-tauri/src/tabs/tests/replay.rs` (the two
  existing regression tests that pin behaviour).
- **New dependencies**: none. Zero new dependencies means nothing to classify
  against the project's `MIT` license
  (`references/license-compat.md`), so no license constraint is raised by this
  feature.

## Layer Structure

| Layer | Responsibility | May depend on |
|---|---|---|
| `src-tauri/src/tabs/mux_link.rs` | Entry point for PTY output arriving over the mux link: decides whether a payload is queued, dropped, or triggers the overflow fallback | the replay layer below it |
| `src-tauri/src/tabs/replay.rs` | Owns the pending-switch state, its live queue, the queue cap, and the outcome vocabulary the caller matches on | nothing in `mux_link.rs` |

The dependency direction is one-way and unchanged: the live-queue cap and its
outcome vocabulary stay owned by the replay layer, and the caller adapts to
them (NFR1). No new module, type, trait or public surface is introduced.

## Shared Components

Nothing new is shared. The two contracts below are **consumed** by this
feature and are pinned here so the single task implements against them without
reading the replay module's own plan or changing it.

| Component | Responsibility | Contract (pre/post) | Used by tasks |
|---|---|---|---|
| **SC-1 Live-queue entry point** (`PendingSwitch::queue_live_output`, declared in `src-tauri/src/tabs/replay.rs`) | Append an arriving payload to the pending switch's live queue, or report that the queue's cap would be exceeded | **Pre**: called at most once per arriving payload, on a pending switch whose target pane equals the payload's pane; takes ownership of the payload. **Post**: returns exactly one outcome of the two-valued outcome enum — *queued* (payload retained; the caller owes no redraw) or *overflowed* (the caller must run SC-2). The return value is must-use; the signature, the semantics, the must-use attribute and the declaration site are all out of scope (NFR1, A6) | task0001 (consumer only — never modified) |
| **SC-2 Overflow fallback sequence** (already present in `mux_link.rs`) | Abandon the pending switch and rebuild the frame synchronously when the live queue overflows | **Pre**: reached only from the overflowed outcome of SC-1; the mutable borrow of the pending-switch slot has already ended. **Post**: the existing call order is preserved verbatim — take the coalesced redispatch payload, supersede the pending replay (preferring that coalesced payload), reset the frame for replay, apply the queued live output — and the entry point reports that a redraw is owed. Apart from the FR4 rename, no statement in this sequence changes (NFR2) | task0001 (carried over verbatim) |

## Conventions

- **Logging policy**: no log line is added, removed, reworded, re-levelled or
  reordered. The mismatched-pane drop path keeps its two lines (the osc-probe
  warning followed by the debug line) exactly as they are (FR5).
- **Naming policy**: when collapsing to one borrow makes two bindings in the
  same scope share a name, the binding that is renamed is named after the
  value it actually holds, and every later use keeps reading the same value it
  read before (FR4).
- **Error-handling policy**: no error type, no new panic, no assertion and no
  new log line is introduced. The impossible state that the removed arm used
  to absorb becomes unrepresentable rather than reported — that is the point
  of the change. The alternative shape the review raised (keep two look-ups
  and make the impossible state observable through an assertion plus an error
  log) is explicitly not taken (A3).
- **Ownership policy**: the payload is moved into SC-1 only on the
  matching-pane path, so the drop path still owns it where it reports its
  length. No clone is added and no `unsafe` is introduced (NFR3, A5).

## Cross-task Design Decisions

### D1 — One mutable borrow, with the pane id latched as a copy (FR1, FR3)

The pending-switch slot is borrowed mutably exactly once for the whole block.
The target pane id is read out immediately and latched as a plain copied
integer, so every later use — the pane comparison and both drop-path log
lines — reads the copy and never the borrow. The previous shape (an immutable
look-up to read the pane id, then a second mutable look-up to queue the
payload) is removed together with the arm that handled "the second look-up
found nothing". Rationale: the invariant that both look-ups observe the same
state stops being a comment and becomes structural, and the only silent-drop
path in this function disappears.

### D2 — Borrow-region discipline is what makes the fallback compile (FR3, A4)

The borrow of the pending-switch slot must be dead before the overflow
fallback runs, because every call in SC-2 needs the whole tab mutably. The
condition for that is mechanical: after the outcome match, the borrow binding
itself is never touched again — only the copied pane id is. Any later use of
the binding (for example reading another field off it inside the fallback, or
in a log line) re-extends the borrow region and the fallback stops compiling.
This condition is proved mechanically by the check gates in D4, not by
inspection.

### D3 — Resolve the name collision by renaming the inner binding (FR4)

The overflow fallback already binds a name for the superseded pending replay,
and under D1 the outer borrow binding would shadow it. The **inner** (fallback)
binding is the one renamed, to a name that says what it holds — the superseded
replay, e.g. a `superseded_replay`-style name — while the outer binding keeps
the name the pending switch has carried at this site historically. Rationale:
the two values are different things, the inner one is the one whose current
name is the less accurate of the two, and renaming it leaves the outer shape
identical to the pre-refactor form the requirement asks to restore. The rename
is surface-level: each later use must read exactly the value it reads today.

### D4 — Four acceptance gates, all four are blocking (A1, NFR5)

Correctness here is almost entirely a compile-time property, so the gates are
the verification. All four run from the project root with an explicit target
directory (never from inside `src-tauri/`, and never letting cargo fall back to
the workspace-default target directory):

| Gate | Command | Proves |
|---|---|---|
| Test | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | Behaviour unchanged (FR5) |
| Check (default features) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | Borrow region ends in time (D2); match is exhaustive (FR2) |
| Check (CLI-only) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | No feature-gate regression (NFR5) |
| Check (Windows cross) | `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests` | No warning under the cross-compiled configuration (NFR5, A1) |

"Succeeds" means exit code 0 **and** no warning that was not already there
before the change.

### D5 — Documentation touch rule (NFR4)

Only the doc-comment sentences that describe the removed arm are rewritten.
Neighbouring commentary about other requirements at this call site is left
untouched, including its existing requirement numbering, which belongs to a
different document than this feature's FR ids and must not be renumbered to
match.

### D6 — No new test is written (A2)

The removed arm is unreachable, so no runtime test can distinguish before from
after; the exhaustiveness property is a compile-time property. The two existing
regression tests stay exactly as they are and are the behavioural safety net.
An implementer who feels the need to add a test should read that as a signal
that the change has grown beyond its scope, and report it rather than widen the
task.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The single borrow is still live when the overflow fallback runs, so the code does not compile | Medium | Low (caught immediately) | D2's rule: never touch the borrow binding after the outcome match; latch the pane id as a copy. The failure is a compile error, not a runtime defect |
| The binding rename silently changes which value a later statement reads | Low | High (would alter the overflow path's behaviour) | D3 states the rename is surface-level; the overflow regression test (TS-2) exercises the whole fallback sequence |
| The overflow fallback's call order is disturbed while the block is restructured | Low | High (ordering defect in the off-thread replay handoff) | NFR2 / SC-2 pin the order; TS-2 asserts the end state; the diff is confined to one function |
| The drop path loses access to the payload length because ownership moved earlier | Low | Medium (log regression) | Ownership policy above: the payload is moved only on the matching-pane path (A5); TS-1 covers the drop path |
| Scope creep into the live-queue entry point or the queue cap | Medium | Medium (widens review and the declared change set) | NFR1 / A6 declare them out of scope; the task's file set is a single file |

## Open Questions

- [ ] None. All ten requirements are `ok` in `workflow.yaml`, and the six
      assumptions (A1-A6) recorded at create-spec are carried into this plan
      without change. A1 in particular is a re-decision the orchestrator made
      against the consultation's recommendation, so if the Windows cross-check
      proves to be a toolchain problem in practice, that is a gate question to
      raise — not a licence for the implementer to drop the gate silently.
