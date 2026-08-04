# Implementation Plan: per-tab-grid-size

## Overview

Relocate grid-size ownership from a single app-wide record force-distributed
to every tab into per-tab ownership, so resize triggers reach only the active
tab and inactive tabs' PTYs never receive TIOCSWINSZ — eliminating the
XTWINOPS response-fragment leak into tmux-hosted shells (SPEC:
`feature-docs/per-tab-grid-size/SPEC.md`).

## Technology Stack

- **Rust (`gui` feature)** — existing native terminal stack. Changes are
  confined to `src-tauri/src/app.rs` and `src-tauri/src/tabs.rs`, both
  GUI-only modules.
- **New dependencies**: none. License review: nothing to record;
  `project.license: MIT` is unaffected.

## Layer Structure

```
window_host  (display area → grid dims; calls App once per settled resize —
              calling convention UNCHANGED by this feature)
   ↓
App          (src-tauri/src/app.rs — resize routing, active-tab selection,
              activation-time reconcile, app-level tracker invalidation)
   ↓
Tab          (src-tauri/src/tabs.rs — owns its grid size, core + PTY resize,
              mux pane Resize frames, its own reflow invalidation)
   ↓
term_core grid  /  PTY (TIOCSWINSZ)  /  mux daemon (pane Resize frames)
```

Dependencies point downward only. Neither first-pass task (task0001 /
task0002) modifies `window_host.rs`; rework task0003 adds exactly one call
point there — the reconcile-executor invocation after the insets and pending
resize settle (Shared Components, activation-reconcile request/execute
split). The calling convention into `set_grid_size` stays unchanged.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `Tab::resize(cols, rows)` | Apply a new grid size to this tab only | **Pre**: caller passes desired dims (raw; may be out of wire domain). **Post**: (a) dims are clamped by the same pure wire-domain clamp the daemon applies, before any effect; (b) the tab's PTY and core are at the clamped dims; (c) if the resize changed this tab's column count, the tab's own reflow-invalidated trackers (prompt marks, fold regions) are cleared **by the tab itself**; (d) for mux tabs, pane `Resize` control frames carrying the clamped dims are emitted for every pane in the tab's group — `Tab::resize` is the only RESIZE-PATH emission site, but not the only emission site in the codebase: mux attach/Welcome pane seeding and PaneCreated handling (both in tabs.rs) also emit pane `Resize` frames, each reading the tab's OWN core dims. No NEW emission site may be added, and frame-emission assertions in tests observe the frame observation hook (below), never a dims-only proxy; (e) the pending off-thread-switch deferred-resize behavior is unchanged. | task0001 (caller), task0002 (implementer of (c); preserver of (a)(b)(d)(e)), task0003 (caller via the App resize application helper; hook instrumenter) |
| `Tab::clear_reflow_invalidated_state()` | Drop this tab's prompt/fold trackers | Existing method, behavior unchanged. After this feature its resize-time invocation is owned by Tab internally (contract (c) above); App no longer calls it from any resize path. | task0001 (stops calling), task0002 (self-invokes) |
| Tab grid-dims source of truth | Report a tab's current grid size | A tab's owned grid size IS its core's current (post-clamp) dims, read via the core's dims accessors. No duplicate per-tab size field is introduced — a second record would reintroduce the drift class this feature removes. | task0001, task0002 |
| `App::cell_size` | Record the current display-area grid dims | Always holds wire-domain-clamped dims (clamped before recording, as today). **Invariant**: equals the ACTIVE tab's core dims after any App mutation completes (a resize application or an executed activation reconcile — between an activation REQUEST and its execution on the next render pass, the incoming tab may briefly lag; see the reconcile split below). Renderer / hit-testing / `window_host::grid_size` keep reading it as "the active tab's grid". | task0001, task0003 |
| Activation-reconcile request/execute split | Reconcile the incoming tab against ITS OWN settled display area | **Pre**: an activation path (explicit switch, close-tab fix-up, exited-tab reap fix-up) has changed which tab is active. **Post**: the path itself resizes nothing — it only records a pending reconcile request on App. A reconcile executor (an App method) consumes the request; the render pass in `window_host` invokes it at exactly one point, AFTER status-bar insets, the mux-sidebar inset, and any pending display-area resize have settled `cell_size` for the NEW active tab. The executor compares the active tab's core dims against the current `cell_size`; on mismatch it resizes through the App resize application helper; on match it issues nothing and clears nothing (FR3). Consecutive requests before a render pass collapse to one; a request with no matching tab is dropped. | task0003 (both sides: app.rs executor + window_host.rs call point) |
| App resize application helper | Single App-side path for issuing a `Tab::resize` | **Pre**: App decided a target tab must be resized (display-area change in `set_grid_size`, or a reconcile execution). **Post**: pre-resize column count read → `Tab::resize` → if the post-resize column count differs, the App-owned trackers (selection, pending selection anchor) are cleared. BOTH `set_grid_size` and the reconcile executor issue their resize through this path — App never invokes `Tab::resize` outside it, so the App-side half of the D3 split holds on every origin. | task0003 |
| Pane `Resize` frame observation hook | Test-only recording of emitted pane `Resize` frames | Test-build-gated (compiled away in release builds; zero production behavior change). Records tab/pane identity plus post-clamp dims at EVERY emission site enumerated in contract (d). app.rs tests read it to assert frame emission (FR4) directly instead of inferring from core dims. | task0003 (instrumented in tabs.rs, read from app.rs tests) |

## Conventions

- Tests are inline `#[cfg(test)]` modules in the same source files (they run
  under the `--lib` test target). The verification command pins
  `--test-threads=1` because the tabs.rs replay tests are non-deterministic
  in parallel.
- Run cargo from the project root with `--manifest-path src-tauri/Cargo.toml`
  and `CARGO_TARGET_DIR=src-tauri/target`; never `cd` into `src-tauri/`.
- Never run a crate-wide `cargo fmt` (the project is intentionally not
  rustfmt-clean); keep formatting local to edited lines.
- Both files are GUI-only (`gui` feature); the CLI build
  (`--no-default-features`) must keep compiling with no source change to
  non-GUI modules (NFR3).
- Updated existing tests carry a comment stating WHY the assertion moved or
  changed (NFR2's "updated with justification").

## Cross-task Design Decisions

### D1: Active-only resize routing (FR1, FR2, FR4) — affects task0001, task0002

App resizes exactly one tab per trigger: the active tab on a display-area
change, or the newly-activated tab on an activation reconcile. The all-tabs
distribution loop in `App::set_grid_size` is removed. Pane `Resize` frame
emission on the RESIZE path lives only inside `Tab::resize` (Shared
Components, contract (d) — which also enumerates the two non-resize
emission sites at mux attach/Welcome pane seeding and PaneCreated, both
reading the tab's own core dims), so restricting WHO gets resized yields
the FR4 frame rule for every resize — no separate frame-gating logic exists
anywhere, and no new emission site may be added. Because "single emission
site" is not literally true codebase-wide, FR4 test assertions observe the
frame observation hook (Shared Components), never a dims-only proxy.
(Corrected in review round 1: cfcbfae57964beb5.)

### D2: Activation reconcile at every activation point (FR3) — affects task0001, task0003

"A tab becomes active" is not just the explicit tab switch: the active index
also moves in the close-tab fix-up and the exited-tab reap fix-up. Every path
that changes WHICH tab is active must end with the same reconcile step.
**Corrected in review round 1 (dbb7766a6212fb1a / 09f0e6096bbc36ee)**: the
reconcile is NOT executed synchronously inside the activation path — at that
moment `App::cell_size` still holds dims computed for the OUTGOING tab
(the persistent-sidebar inset depends on whether the active tab is
mux-attached), which caused a wrong-dims resize followed by a corrective one.
Instead, activation paths only REQUEST a reconcile; execution is deferred to
the render pass after the insets and any pending display-area resize have
settled `cell_size` for the incoming tab (Shared Components,
activation-reconcile request/execute split). The comparison rule is
unchanged: on mismatch, resize that tab (through the App resize application
helper); on match, issue nothing. Newly-spawned tabs are created at
`App::cell_size` dims and need no reconcile. Tab reordering keeps the same
logical tab active and needs no reconcile.

### D3: Reflow-invalidation ownership split (FR6, NFR1) — affects task0001, task0002, task0003

Tab-owned trackers (prompt marks, fold regions) are cleared BY THE TAB inside
its own resize when its column count changes — structural and
caller-independent, so correctness cannot depend on which UI element caused
the resize (NFR1). App-owned trackers (selection, pending selection anchor)
are cleared by App whenever App issues a width-changing `Tab::resize`,
through the App resize application helper (Shared Components) — the single
path both `set_grid_size` and the reconcile executor use. **Corrected in
review round 1 (a172de726b3cbc29 / d39a6a9468ff892e)**: the earlier caveat
"tab switching already clears both, so the activation-reconcile path needs no
extra app-level clearing" covered only `switch_to_tab`; the close-tab and
exited-tab-reap activation origins reach a width-changing resize without any
prior clearing, so the App-side half is owned by the helper itself and never
delegated to caller preconditions. Tabs that are not resized keep every
tracker. Neither side duplicates the other's clearing.

### D4: Wire-domain clamp stays dual-sided (FR5) — affects task0001, task0002

The identical pure clamp continues to run at both the app-side record point
(before recording `App::cell_size`) and inside `Tab::resize`. Neither task
may relocate or unify these into one site — the dual application is what
keeps client and daemon agreeing on accepted dimensions without a wire round
trip.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A missed activation path reintroduces the leak | Medium | High | D2 enumerates the fix-up sites (switch, close-tab, exited reap); TS2 covers activation; TS7 manual reproduces the original scenario |
| Interplay with the off-thread pending-switch deferred resize | Low | Medium | `Tab::resize` contract (e) freezes that behavior; task0002 leaves the deferred-resize machinery untouched |
| Existing tests assume all-tabs distribution | High | Low | NFR2 permits justified updates; each task plan names the exact tests and the justification to record |
| tabs.rs replay tests flake in parallel | Medium | Low | Verification command pins `--test-threads=1` |

## Open Questions

- None.
