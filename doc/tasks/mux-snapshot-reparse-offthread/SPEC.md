# Feature: mux snapshot reparse cost — measure and decide

## Overview

After `mux-scroll-isolation` (commit `e8e538f`, FR1), the on-demand
`RequestPaneSnapshot` reply carries the pane's scrollback (up to
`DEFAULT_SCROLLBACK_CAPACITY` = 2 MiB). The client reparses the whole payload
synchronously via `reset_and_replay` → `process_pty_data_fully`, on the winit
event loop (the UI/render thread). This feature **measures** that reparse cost
on a near-full scrollback and **decides** whether to pursue the off-thread
replay redesign (plan "案a"), and—independently of that gate—**guard-rails** the
already-tight scrollback lock scope in the daemon's snapshot handler so a future
refactor cannot regress it.

The off-thread replay implementation itself is **out of scope** here; it is
deferred to a follow-up feature, gated on the measurement outcome.

## Objectives

- Provide a deterministic, reproducible measurement of `process_pty_data_fully`
  over a ~2 MiB synthetic scrollback, reported in ms and MiB/s.
- Apply the plan's §4 threshold to the measured number and record an explicit
  go/no-go decision for the off-thread replay (案a) follow-up.
- Guard-rail the already-tight scrollback lock scope in
  `handle_request_pane_snapshot` (held only for the `read_all` copy) against
  future regression, with byte-identical output.

## User Stories

### US1: Measure the reparse cost
As a developer, I want a deterministic harness that times the reparse of a
near-full scrollback, so that I can decide whether the off-thread redesign is
worth its complexity instead of guessing.

**Acceptance Criteria:**
- [ ] Running the harness prints elapsed ms and throughput (MiB/s) for a ~2 MiB
      synthetic scrollback fed to `process_pty_data_fully`.
- [ ] The harness is deterministic (fixed synthetic input; no RNG / wall-clock
      dependence in the input) and does not run as part of the default
      `cargo test` (gated, e.g. `#[ignore]`).

### US2: Decide go/no-go for off-thread replay
As a developer, I want the measured number mapped to a documented threshold,
so that the decision to build (or skip) the off-thread redesign is fact-based
and recorded.

**Acceptance Criteria:**
- [ ] The measured value is mapped to the §4 thresholds and the decision +
      rationale are written to `VERIFICATION_RESULT.md`.
- [ ] If "go", the doc states that the 案a off-thread implementation (core only,
      no LRU cache) will be filed as a separate SDD feature.

### US3: Guard-rail the snapshot lock scope
As a developer, I want the scrollback lock's tight scope made explicit and
protected by a regression test, so that a future refactor cannot accidentally
hold it across snapshot assembly / logging / channel send.

**Acceptance Criteria:**
- [ ] In `handle_request_pane_snapshot` the scrollback guard's drop point is
      explicit (scoped block + comment), holding the lock only for the
      `read_all` copy — not across assembly / log / send.
- [ ] The snapshot output bytes are byte-identical to the current behavior;
      existing mux reattach/snapshot tests pass unchanged.

## Technical Requirements

### Functional Requirements

- **FR1 — Reparse-cost measurement harness:** Add a deterministic, on-demand
  measurement in the `term_core` crate that builds a ~2 MiB synthetic
  scrollback (plain text with interspersed SGR / newlines, representative of
  terminal output), feeds it to `TerminalCore::process_pty_data_fully`, times it
  with `std::time::Instant`, and reports elapsed ms + MiB/s via `eprintln!` /
  log. Gated so it is excluded from the default `cargo test` run (e.g.
  `#[ignore]`, run with `cargo test -p term_core -- --ignored --nocapture`).
  Measuring `process_pty_data_fully` is sufficient because it is the dominant
  cost of `reset_and_replay`, and scrollback (~2 MiB) dominates the snapshot
  payload.

- **FR2 — Go/no-go decision record:** Run FR1 on the target machine, then map
  the result to the §4 thresholds and record the decision + rationale (including
  how often ~2 MiB actually accumulates in practice). Thresholds:
  - `< 5 ms`: likely not worth 案a's complexity → skip 案a (do FR3 only).
  - `5–50 ms`: gray zone → decide separately with the measured value attached.
  - `50 ms+`: implement 案a → file a follow-up SDD feature.

- **FR3 — Lock-scope guard-rail for the snapshot copy:** The current handler
  already holds the scrollback `Mutex` only for the `read_all()` copy — the
  temporary guard drops before snapshot assembly / logging / channel send. This
  requirement *locks that in*: make the drop point explicit (a scoped block that
  returns the owned `Vec<u8>` + a comment naming the invariant) so a future
  refactor cannot hold the lock across assembly/send, and add a regression test
  asserting the assembled snapshot bytes are byte-identical to the established
  layout. No behavioral change; the pre-existing session-scope authorization is
  unchanged. (The lock hold cannot be shortened below the unavoidable O(n) copy
  without a ring-buffer redesign, which is out of scope.)

### Non-Functional Requirements

- **NFR1 — Determinism / isolation:** FR1's measurement uses a fixed synthetic
  input and calls into `term_core` directly (pure path), NOT through
  `App::pump_all`, to avoid the known flaky behavior of pump tests that drive a
  real PTY. Re-runs on the same machine must not produce numbers that flip the
  decision.

- **NFR2 — No regression / CI hygiene:** FR3 is behavior-preserving (identical
  snapshot bytes; existing reattach/snapshot tests green). FR1 must not slow or
  destabilize the default `cargo test` (gated/ignored). CLI-only
  `cargo check --no-default-features` stays green.

- **NFR3 — Portability:** Lands in `term_core` (platform-independent) and
  `src-tauri` (gui). Does not break the Linux/Windows or CLI-only builds.

## Implementation Approach

### Architecture

This feature touches two layers and does **not** change the data model:

```
┌──────────────────────────────────────────────────────────┐
│ crate term_core (platform-independent)                   │
│   TerminalCore::process_pty_data_fully  ← measured by FR1 │
│   FR1: #[ignore] timing harness (synthetic ~2 MiB input)  │
├──────────────────────────────────────────────────────────┤
│ src-tauri/src/mux/ipc/handlers.rs (daemon)               │
│   handle_request_pane_snapshot                            │
│   FR3: guard-rail scrollback Mutex scope (copy-only)      │
└──────────────────────────────────────────────────────────┘
```

### Data Flow (current, unchanged by this feature)

```
client switch → RequestPaneSnapshot → daemon:
    scrollback.lock().read_all()  (FR3 guard-rails: lock held only here)
    build_snapshot_bytes(scrollback, screen)   (no lock held)
→ client: reset_and_replay → process_pty_data_fully  (FR1 measures this)
       ↑ runs on App::pump_all (UI thread)
```

### Measurement design (FR1)

- Build input once: a `Vec<u8>` of ~`DEFAULT_SCROLLBACK_CAPACITY` bytes,
  composed of representative terminal lines (printable text, periodic newlines,
  occasional SGR sequences). Fixed content — no RNG, no wall-clock seeding.
- Construct a `TerminalCore` at a representative grid size (e.g. 80×24 or the
  default used elsewhere in `term_core` tests).
- `let t = Instant::now(); core.process_pty_data_fully(&input); let dt = t.elapsed();`
- Report: `eprintln!("reparse {} bytes in {:?} ({:.1} MiB/s)", ...)`.
- Optionally measure a few sizes (e.g. 256 KiB, 1 MiB, 2 MiB) to show scaling,
  since cost is proportional to actual history, not capacity.

### Lock-scope guard-rail design (FR3)

- Today: `let scrollback_data = scrollback.lock().unwrap().read_all();` already
  copies into an owned `Vec<u8>` and drops the temporary guard at the end of the
  statement — before `build_shadow_parser_snapshot`, the log, and the send. So
  the lock scope is *already* copy-only; FR3 does not shorten it.
- FR3's change is a guard-rail, not an optimization: make the drop point
  explicit (a small scoped block that returns the owned `Vec` + a comment naming
  the "lock held only for the copy" invariant) so a later edit cannot slip work
  under the lock, and add a regression test asserting the assembled snapshot
  bytes are byte-identical to the established layout. Output bytes unchanged.

### Dependencies

**Internal Dependencies:**
- `crates/term_core` — `TerminalCore::process_pty_data_fully` (FR1 target).
- `src-tauri/src/mux/ipc/handlers.rs`, `reattach.rs` — snapshot assembly (FR3).
- Builds on `mux-scroll-isolation` (FR1: scrollback in on-demand snapshot).

**External Dependencies:**
- None new. (No criterion / bench framework is added; a gated `#[test]` with
  `std::time::Instant` is sufficient and avoids a new dev-dependency.)

### File Structure

```
crates/term_core/src/terminal_core.rs   # FR1: #[ignore] timing test (+ helper)
src-tauri/src/mux/ipc/handlers.rs        # FR3: tighten scrollback lock scope
doc/tasks/mux-snapshot-reparse-offthread/
  要件定義書.md
  SPEC.md
  IMPLEMENTATION.md        # sdd.2
  VERIFICATION.md          # sdd.2
  VERIFICATION_RESULT.md   # sdd.6 (holds the FR2 decision record)
  sdd.yaml
```

## Test Scenarios

### Unit Tests
- [ ] FR1 harness completes and prints a number for a ~2 MiB synthetic input.
- [ ] FR1 harness handles a 0-byte input without panic (≈0 ms).
- [ ] FR3: a test asserts the assembled snapshot bytes are unchanged vs. the
      current construction for a representative pane (screen + scrollback).

### Integration Tests
- [ ] Existing mux reattach/snapshot tests pass unchanged after FR3.

### E2E Tests
**Existing E2E tests**: None applicable (Rust perf/measurement feature).
**Run command**: Not applicable.

### Edge Cases
- [ ] Empty scrollback: FR1 ≈0 ms; FR3 still produces the bare clear + screen
      snapshot.
- [ ] Near-capacity (~2 MiB) scrollback: FR1 reports the headline number used
      for the FR2 decision.

### Performance Tests
- [ ] FR1 is itself the performance measurement; its output feeds FR2.

## Security Considerations

- No change to the existing authorization: the snapshot (screen + scrollback) is
  served only for panes belonging to the requester's currently-attached session.
  FR3 must preserve this check and the output bytes.

## Error Handling

- FR3 keeps the current `lock().unwrap()` poison behavior (no behavior change to
  poisoning in this feature).

## Performance Optimization

### Performance Goals
- FR1: produce a reliable ms figure for ~2 MiB reparse.
- FR3: keep the scrollback `Mutex` scope copy-only (already the case);
  guard-rail it against regression. No output change.

### Optimization Strategies
- 案a (off-thread replay) — the eventual strategy if FR2 says "go"; **not
  implemented here** (follow-up feature, core only, no LRU cache).
- LRU cache of recent K panes — recorded in the plan but **out of scope**.
- 案c (per-pane resident core) — **rejected** (full redesign, N× memory, loses
  the daemon's "don't stream unseen panes" benefit).

### Caching Strategy
- None in this feature.

## Success Criteria

- [ ] FR1 measurement harness implemented, deterministic, gated from default tests.
- [ ] FR2 decision recorded against the §4 thresholds with rationale.
- [ ] FR3 lock-scope guard-rail in place (explicit drop point + comment +
      regression test) with byte-identical snapshot output.
- [ ] `cargo test` (default, single-thread) green; CLI-only `cargo check` green.
- [ ] Documentation complete.

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- None blocking. The 5–50 ms gray-zone resolution (if hit) is a runtime decision
  made with the measured value in hand, not a spec ambiguity.

## Implementation Phases

### Phase 1: Measure + lock-scope guard-rail (THIS feature)
**Goals:** Quantify the reparse cost; decide go/no-go; guard-rail the existing
copy-only scrollback lock scope.
**Deliverables:**
- FR1 measurement harness in `term_core`.
- FR2 decision record in `VERIFICATION_RESULT.md`.
- FR3 lock-scope guard-rail (explicit drop point + comment + regression test) in
  `handle_request_pane_snapshot`.

### Phase 2: Off-thread replay — 案a (FOLLOW-UP feature, only if FR2 = go)
**Goals:** Move the heavy reparse off the UI thread; keep FR1 (full-history
scroll) and FR3 of mux-scroll-isolation intact.
**Deliverables (deferred, not in this feature):**
- Worker-thread `TerminalCore` build + main-thread swap; "pending switch" state
  buffering live bytes; ordering correctness; marks/folds/selection + grid-size
  reconciliation on swap. Core only; no LRU cache.

## References

- Design memo: `tmp/perf-snapshot-reparse-offthread-plan.md`
- Prereq feature: `doc/tasks/mux-scroll-isolation/` (FR1 body: commit `e8e538f`)
- Related investigation: `tmp/mux-scroll-investigation.md`
- Key code: `crates/term_core/src/terminal_core.rs` (`process_pty_data_fully`,
  `reset_and_replay`), `src-tauri/src/mux/ipc/handlers.rs`
  (`handle_request_pane_snapshot`), `src-tauri/src/mux/ipc/reattach.rs`
  (`build_snapshot_bytes`)
