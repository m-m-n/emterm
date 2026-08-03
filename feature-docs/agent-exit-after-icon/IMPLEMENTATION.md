# Implementation Plan: agent-exit-after-icon

## Overview

Add a per-pane "OSC 133 D→A inferred-clear latch" that clears a stale
agent-status icon when the shell demonstrably returns to a prompt after the
agent was last `Set`, without inventing a new clear code path or touching
content/text detection. The latch is a single build-agnostic component
consumed symmetrically by the GUI-local plain-tab path and the mux
daemon's pane path.

## Technology Stack

- **Language**: Rust (no new external crates; SPEC.md's "External
  Dependencies: None" holds).
- **Existing components reused as-is**: `src-tauri/src/agent_status.rs`
  (OSC 777 wire parsing), `src-tauri/src/prompts.rs` (OSC 133 mark types —
  `PromptMarkKind`; only the type, not `PromptTracker`'s retained-mark
  storage, is reused).

## Layer Structure

- **Core (build-agnostic, no `gui` feature)**: the new inferred-clear
  latch lives next to `agent_status.rs` — both `agent_status.rs` (OSC 777
  parsing) and `mux` (build-agnostic per `src-tauri/src/lib.rs:180`) can
  depend on it without pulling in GUI-only crates.
- **GUI process (plain tabs, `gui` feature)**: `callbacks.rs` /
  `agent_status_model.rs` own one latch instance per `PaneKey::Tab` and
  drive it from the GUI process's own live OSC 133/OSC 777 capture.
- **Mux daemon (build-agnostic, mux panes)**: `mux/session/pane.rs` /
  `mux/daemon.rs` own one latch instance per `MuxPane` and drive it from
  the daemon's own live OSC 133/OSC 777 capture of that pane's PTY
  stream. The daemon remains authoritative for mux panes (SPEC.md FR3).

Dependency direction: GUI and daemon each depend on the core latch
component; the core latch component depends on neither.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|-------------------------------|---------------|
| Inferred-clear latch (core, build-agnostic) | Per-pane state machine implementing SPEC.md FR1/FR5: tracks armed / command-ended / generation; exposes an operation to record an explicit `Set`, an operation to record an explicit `Clear`, and an operation to record a live OSC 133 mark (`PromptMarkKind`) that returns whether an inferred `Clear` should now be applied. | **Pre**: caller supplies only LIVE, main-screen-observed events, in true arrival order, for a single pane's stream (never snapshot/replay-reconstructed or alt-screen-suppressed marks — FR5). **Post**: after `Set`, the latch is armed and returns no inferred clear on its own. After a live `D` while armed, the latch records "command ended" for the current generation. After a live `A` while "command ended" (same generation), the latch returns exactly one inferred-clear signal and returns to disarmed. After an explicit `Clear`, the latch returns to disarmed and produces no inferred-clear signal, regardless of prior `D` state. A new `Set` starts a new generation; marks tagged with (or implicitly belonging to) an earlier generation never produce a signal. The latch itself never touches `agent_status`/UI/revision state — callers apply the inferred-clear signal through the SAME code path as an explicit `Clear` (FR2). | task0001 (owns), task0002, task0003 |
| Live-only OSC 133 feed (per side) | Supplies the latch with live marks without going through `PromptTracker`'s retained/pruned storage, and without alt-screen-suppressed marks (FR5). GUI and daemon each implement their own feed against this same behavioral contract, sourced from `term_core`'s live OSC 133 capture on their respective sides. | **Pre**: mark was captured from the live PTY byte stream currently being processed (not backfilled from scrollback, not derived from a mux snapshot/reattach replay). **Post**: exactly the marks meeting that condition reach the latch, in original relative order with that pane's OSC 777 events (FR4). | task0002 (GUI side), task0003 (daemon side) |

## Conventions

- Naming: name the new core module/type so its purpose (inferred-clear
  latch for agent-status) is unambiguous at the call site; do not reuse
  or overload `PromptTracker`/`AgentStatusModel` naming in a way that
  implies it replaces either.
- Error handling: the latch has no fallible operations (pure state
  transition); a live mark that doesn't match any transition is simply a
  no-op, never a panic/error.
- Logging: an inferred clear firing is logged at `warn` or higher (per
  project logging policy — `debug`/`info` are dropped in release builds),
  identifying the pane so it is diagnosable via `emterm.log` per the
  project's DevTools-unavailable debugging constraint.
- Ordering (FR4): both the GUI side and the daemon side must process a
  given pane's OSC 133 marks and OSC 777 reports through one ordered
  per-pane path — not two independently-scheduled queues/threads that
  could reorder a `Set` relative to a `D`/`A` pair emitted in the same PTY
  read.

## Cross-task Design Decisions

### D1: One core latch type, two independent wiring sites

**Decision**: task0001 builds the core latch as a single, side-agnostic
component (per the Shared Components contract above). task0002 (GUI/plain
tabs) and task0003 (mux daemon panes) each instantiate and drive their
own set of latches against that contract, independently, in parallel.
Neither wiring task depends on the other's code.

**Rationale**: SPEC.md FR3 requires symmetric behavior without letting
either side own the other's authority (daemon stays authoritative for mux
panes). A single core type keeps the state-machine rules (FR1) defined
exactly once instead of duplicated and potentially drifting between the
two sides.

**Affected tasks**: task0001, task0002, task0003.

### D2: Live-only feed is a per-side responsibility, not the core latch's

**Decision**: the core latch (task0001) accepts whatever live marks it is
handed — it does not itself decide "live vs. replay" or "main screen vs.
alt screen." Each wiring task (task0002, task0003) is responsible for only
ever handing it live, main-screen marks (FR5), per the "Live-only OSC 133
feed" contract above.

**Rationale**: "live vs. replay" and "alt-screen suppression" are
concerns of where each side taps into `term_core`'s OSC 133 capture
(different call sites on the GUI vs. daemon side), not something the
core latch can determine from a mark's data alone.

**Affected tasks**: task0001, task0002, task0003.

### D3: Hot-upgrade latch preservation is a dedicated task

**Decision**: task0004 is a separate task scoped to `mux/upgrade.rs`,
carrying the daemon-side latch state (per pane) across a hot-upgrade
boundary (FR6). It depends on task0003's latch field existing on the
mux pane type but does not modify task0003's files — it only adds the
upgrade-time copy for the new field(s) task0003 introduces on `MuxPane`.

**Rationale**: hot-upgrade state transfer is an existing, narrowly-scoped
mechanism (`mux/upgrade.rs`) unrelated in shape to daemon runtime wiring;
keeping it a separate task avoids conflating "does the latch work at
runtime" with "does the latch survive a binary swap," which have
different verification approaches (unit test vs. hot-upgrade integration
test, per existing `mux_hot_upgrade` test conventions).

**Affected tasks**: task0003 (defines the field task0004 must carry
across), task0004.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| task0002/task0003 diverge in behavior despite sharing the core latch (e.g. one feeds replay-derived marks by mistake) | Medium | Medium | Each task's Acceptance Criteria explicitly requires a test proving replay/snapshot-derived marks do not fire the latch (SPEC.md test scenario) |
| task0004 lands before task0003's field shape is final, causing a merge-time contract mismatch | Low | Low | task0004's Acceptance Criteria requires reading task0003's actual field additions to `MuxPane` at implementation time (parallel tasks may finish in any order — the implementer resolves this the same way any cross-task file-overlap is resolved, via the parent-side-adoption protocol) |
| Core latch (task0001) contract ambiguity leads task0002/task0003 to each invent slightly different generation semantics | Medium | Medium | The Shared Components contract above is explicit about generation invalidation; task0001's Acceptance Criteria requires tests covering the generation-invalidation edge case from SPEC.md |

## Open Questions

None — SPEC.md's Open Questions section is empty; the one residual item
(whether common OSC 133 shell integrations reliably emit `D`) is recorded
as a documented Known Limitation in SPEC.md, not a blocking open question
for planning.
