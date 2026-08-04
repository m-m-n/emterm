# Implementation Plan: mux-hot-upgrade-alt-screen

## Overview

Two independent fixes to the mux daemon hot-upgrade path: (1) the NFR3
path-writability check gains a fail-closed private per-user-group exemption
so a umask 002 dev build can fire hot-upgrade at all, and (2) the handoff
document carries alt-screen state (schema version 3) so a restored pane's
shadow parser reports the alternate screen again after upgrade + reattach.

## Technology Stack

- **Language**: Rust — existing `src-tauri` binary crate and
  `crates/mux_ipc` workspace crate only.
- **New dependencies**: none (NFR4). No new license obligations arise;
  `project.license` (MIT) is unaffected.
- **Platform**: the entire surface is Unix-only (`cfg(unix)` gated);
  Windows behavior is untouched. The CLI-only (`--no-default-features`)
  build must keep compiling.

## Layer Structure

| Layer | Contents | May depend on |
|---|---|---|
| `crates/mux_ipc` | Handoff document types + versioned codec (`handoff.rs`) | Nothing from the daemon binary |
| `src-tauri` mux daemon | `mux/identity.rs` (path-writability check), `mux/upgrade.rs` (snapshot / refresh / restore), `mux/session/pane.rs` (pane state, shadow parser) | `mux_ipc` |
| `src-tauri/tests` | Hot-upgrade integration harness (`mux_hot_upgrade.rs`) | The daemon binary over the wire protocol only |

Dependency direction is unchanged: `src-tauri` depends on `mux_ipc`, never
the reverse.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Post-restore observable contract | Define, externally, what "alt-screen state survived the hot-upgrade" means | Pre: a pane whose handoff record carries alt flag = true was restored. Post: the pane's shadow parser reports the alternate screen as active; the next reattach's snapshot takes the alt branch (alternate-screen contents as the visible screen, scrollback beneath it); no pre-alt-screen scrollback fragments surface where the TUI's screen should be. | task0002 (implements), task0003 (integration scenario asserts it), manual AC-5 (user observes it) |

## Conventions

- Unit tests live inline in `#[cfg(test)]` modules next to the code
  (existing project convention). The integration harness keeps its
  established conventions: isolated `XDG_RUNTIME_DIR` per scenario, bounded
  waits that name the stuck step, RAII cleanup guard.
- Logging that must survive release filtering uses warn level or higher.
- Refusal reasons in the identity check stay human-readable and name the
  FIRST failed rule (existing `identity.rs` convention).
- Integration tests run with `--test-threads=1` (existing harness
  constraint).

## Cross-task Design Decisions

### D1: FR8 alt-screen dump size policy (decided in this plan; recorded assumption)

- **Limit**: the per-pane alt-screen dump in the handoff document is capped
  at `mux_ipc`'s already-exported single-frame snapshot payload limit
  (`MAX_SNAPSHOT_FRAME_PAYLOAD` in `crates/mux_ipc/src/protocol.rs`:
  16 MiB minus the frame header).
- **Overflow behavior**: when a capture would exceed the cap, the alt flag
  is KEPT true, the dump is stored EMPTY, and a warn-level log line records
  the pane id and the oversize length. Restoring flag-true + empty-dump
  enters the alternate screen with blank contents; the TUI repaints on its
  next redraw/resize.
- **Rationale**: the dump's only client-facing resurfacing path is the
  reattach snapshot, which must fit a single frame — a dump above this
  limit could never be delivered to a client anyway. Reusing the exported
  constant keeps ONE size policy in the codebase and introduces no new
  concept (NFR4). Preserving the flag keeps the semantic fix (the
  alternate-screen mode, which is exactly what the defect loses) while
  degrading only cosmetic content.
- **Applied at**: every capture point (initial snapshot and the FR7
  refresh re-capture), inside the single capture helper (task0002).
- **Affected tasks**: task0002 (enforces it), task0003 (the integration
  scenario must not assume dump delivery above the cap), VERIFICATION.md
  (edge-case row).

### D2: task0003 tests against the pinned contract, not against landed code

The integration scenario (task0003) is written against the "post-restore
observable contract" (Shared Components) in its own worktree, where
task0002's behavior has not landed: the new scenario is EXPECTED to fail
there — at a clearly-named bounded wait or assertion, never a hang. This
mirrors how `src-tauri/tests/mux_hot_upgrade.rs` was itself first written
(its module header documents the same expected-to-fail-until-siblings-land
convention). The scenario must not be weakened to pass locally; it turns
green on the integration branch once task0002 merges. Feasibility fallback:
SPEC TS4 / REQUIREMENTS.md 14.2 — if the wire-visible surface cannot
express the assertion at all, task0003 documents the concrete blocker and
the substitute verification (task0002's unit fixation + manual AC-2/AC-5).

### D3: Manual verification ordering (process constraint, not runtime)

Manual AC-2 (a umask 002 dev build fires hot-upgrade — enabled by
task0001) gates manual AC-5 (alt-screen TUI intact — produced by
task0002): AC-5 cannot be exercised on a dev build until AC-2 holds. Both
are user-performed at verify time, after all tasks merge. Task
implementation itself stays fully parallel — the three tasks touch disjoint
files.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Buffer switch between the refresh re-capture and `exec` (residual tearing window) | Low | Medium | FR7 narrows the window to the same width as agent-state staleness; SPEC accepts the residue. |
| Alt-screen scenario infeasible in the integration harness | Medium | Low | D2's fallback: unit-level fixation (task0002) + manual AC-2/AC-5; task0003 documents the infeasibility instead of forcing a flaky scenario. |
| Oversized dump degrades to a blank alternate screen until the TUI repaints | Low | Low | Accepted per D1; the warn log makes it diagnosable. |
| Identity lookups under LDAP/SSSD are slow or fail transiently | Low | Medium | NFR1 bounds the decision to one group lookup + one user lookup; NFR2 fail-closed means a transient failure only delays the upgrade, never weakens the check. |

## Open Questions

- [ ] TS4 feasibility: whether the hot-upgrade harness can drive a real
      alt-screen scenario end-to-end (REQUIREMENTS.md 14.2). The fallback
      is defined (D2); task0003 resolves this empirically.
