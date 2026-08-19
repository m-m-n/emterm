# Implementation Plan: flaky-frame-work-pending-test

## Overview

Remove the order dependence that makes
`app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
fail at cargo's default `--lib` parallelism, by eliminating the contention over the
process-global restart-pending state at its source, and record the diagnosis and the
chosen fix side in a durable feature document.

## Technology Stack

- **Language**: Rust (the `emterm` crate under `src-tauri/`, default features / `gui` on).
- **Test harness**: the built-in `cargo test` harness with inline `#[cfg(test)]` modules.
  No test framework crate is used (NFR5 forbids adding one).
- **New dependencies**: none. No new dependency is introduced by this feature, so the
  project's `MIT` license is unaffected and no license review item arises.

## Layer Structure

Only two layers are in play, both inside the `emterm` crate:

1. **Restart-signal layer** (`src-tauri/src/self_exec.rs`) — owns the restart-required
   state: raising it after a failed self-spawn, the one-shot consuming read, the
   non-consuming peek, and the test-only setter seam.
2. **App layer** (`src-tauri/src/app/`) — reads the restart-signal layer: the frame
   pending-work predicate performs a non-consuming peek; the per-frame timing path
   performs the consuming read that arms the restart toast exactly once.

Allowed dependency direction: App layer depends on the restart-signal layer; never the
reverse. Any change made under D1 preserves this direction — the restart-signal layer
must not gain knowledge of the App.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Restart-pending state seam | The single point through which the restart-required state is raised, cleared, observed and (if the fix serializes) exclusively held for the span of a test | **Pre**: any party that mutates the state holds exclusivity over it for the whole span between its own mutation and its last observation of it; no party observes the state without having established that span. **Post**: on span end the state is returned to *cleared*; no other party can observe a value it did not itself establish. **Failure isolation**: a party that panics inside its span must not convert unrelated tests into failures — a panic inside the span leaves the seam usable for every later span. **Consumption**: the seam preserves the existing distinction between the consuming read (arms the toast exactly once) and the non-consuming peek (feeds the pending-work predicate) — neither semantic changes. | task0001, and any rework task that touches the restart-required state |

Both the app-side test module and the restart-signal layer's own inline test module use
this seam; a fix that isolates only one of the two leaves the contention in place.

## Conventions

- **Test conventions** (NFR5, `test/README.md`): inline `#[cfg(test)] mod tests {}` next
  to the code under test; `<subject>_<scenario>_<expected>` naming; each test constructs
  its own unit under test rather than sharing a global fixture; no new test framework
  crate.
- **Determinism**: no test added or modified by this feature may depend on wall-clock
  sleeps, thread interleaving, or retry-until-green. Trading one flake for another is a
  failure of the feature, not a fix.
- **Feature gating**: any production-side change keeps the CLI-only build compiling.
  Code that GUI-only crates reach must stay behind the existing `gui` gate (NFR4).
- **Approved commands**: verification and implementation use only the command strings
  listed in VERIFICATION.md verbatim. Filtered or otherwise varied cargo invocations are
  not available in this workflow, so no plan step may depend on running a subset of the
  suite.

## Cross-task Design Decisions

### D1: The fix direction is chosen after empirical confirmation, under fixed constraints

FR2 leaves the fix side open. The choice is made by the implementing task *after* the
diagnosis is confirmed, and is bound by these constraints:

- **Test-side isolation** (reset / serialization / per-test ownership) is acceptable only
  if it covers **every** party that mutates or observes the restart-required state — the
  app-level predicate tests and the restart-signal layer's own tests alike. A partial
  seam leaves the contention live and only shifts its probability.
- **Production-side revision** (making the state non-process-global) is acceptable only
  if the off-thread raise path keeps working: the state can be raised from a thread other
  than the App thread, so a revision must keep that path intact and must not introduce a
  new lock on the per-frame predicate path, whose stated contract is a constant-time,
  non-consuming, lock-free-relative-to-the-App read.
- Either direction must leave the consuming-vs-peeking distinction intact.
- Whichever is chosen, AC-05 requires the record to state the side explicitly and why.

*Rationale*: the two directions differ by an order of magnitude in blast radius; forcing
one at plan time would either over-engineer a test-isolation problem or under-fix a real
state-management defect. Constraining both directions instead keeps the choice honest.

*Affected tasks*: task0001.

### D2: The diagnosis is recorded in a dedicated feature document

FR3 / AC-04 / AC-05 are satisfied by
`feature-docs/flaky-frame-work-pending-test/DIAGNOSIS.md`, created by the implementing
task. Required content: the named contended state and its owning file; the full
enumeration of contending parties; the mechanism by which the interleaving makes the
predicate observe `false`; the recorded baseline failure evidence; the chosen fix side
with rationale; the rejected resolutions; and which landed check acts as the regression
guard. Recording the diagnosis only in code comments does not satisfy FR3 — the
requirement names the feature's own documents.

*Rationale*: OBJ-03 wants the record to survive as a recognizable pattern, independent of
whichever source file the fix happened to land in.

*Affected tasks*: task0001.

### D3: Four resolutions are forbidden outright

No task in this feature may (a) set `-- --test-threads=1` for the `--lib` suite in
`workflow.yaml`, CI config, `test/README.md`'s unit-test section, or the project's
documented test command (FR6); (b) mark the target test `#[ignore]`, delete it, or gate
it out of the default `--lib` run (FR5); (c) remove, invert, relax, make conditional, or
retry-wrap the target assertion (FR4); (d) resolve the failure by re-ordering or renaming
tests so the interleaving becomes less likely — probability shaping is not isolation.

*Rationale*: each of these produces a green suite while leaving the defect in place; the
requirements reject them by name.

*Affected tasks*: task0001 and any rework task.

### D4: Out-of-scope known flakes do not consume the stability budget

NFR1's criterion is three consecutive green `--lib` runs. Two flakes are documented as
out of scope (ASM-02 `tabs.rs` replay non-determinism; ASM-03 `tmux_sockets` discovery).
A run whose only failures are one of these is recorded with its test name and command
output and repeated; it neither counts toward nor against the three-run budget, and it is
reported separately rather than folded into this feature. A run that fails on the target
test, or on anything attributable to this feature's change set, fails the criterion
outright.

*Rationale*: SPEC's Edge Cases section requires these to be reported separately; without
this rule NFR1's literal "0 failed" and that instruction contradict each other.

*Affected tasks*: task0001; the verify phase applies the same rule.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The fix shifts the interleaving probability instead of removing the contention, and the suite goes green by luck | Medium | High | D1 requires the seam to cover every contending party; the diagnosis record must enumerate them; three consecutive green runs plus a serial run are required (TS-01, TS-04) |
| A serialization seam converts one failing test into a cascade of failures in unrelated tests when a test panics inside its span | Medium | Medium | The Shared Components contract requires the seam to stay usable after a panic inside a span |
| A production-side revision breaks the off-thread raise path (a spawn failure that occurs off the App thread stops reaching the App) | Low | High | D1 names the off-thread path as a constraint; the CLI-only check plus the whole-suite run are required gates (TS-06, TS-01) |
| A production-side revision breaks the CLI-only build via a GUI-gated symbol | Low | Medium | AC-07 / TS-06 run the CLI-only check explicitly |
| A new isolation mechanism itself becomes a source of flakiness (sleeps, thread races) | Low | High | Conventions forbid sleep- or interleaving-dependent tests; the regression guard must be deterministic |
| The three-run stability check is polluted by the documented out-of-scope flakes | Medium | Low | D4 states the handling rule |

## Open Questions

- [ ] NFR5 (test conventions) has no verifying test scenario in SPEC.md — it is verified
      by inspection against `test/README.md` during review. It stays an inspection-only
      requirement unless the review phase decides otherwise.
- [ ] Whether the fix ends up on the test side or the production side is deliberately
      unresolved at plan time (D1); it is answered by task0001 and recorded per D2.
