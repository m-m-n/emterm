# Implementation Plan: mux snapshot reparse cost — measure and decide

## Overview

Add a deterministic measurement of the synchronous scrollback reparse cost,
record a go/no-go decision for the off-thread replay redesign (案a), and
independently guard-rail the already-tight scrollback lock scope in the daemon's
snapshot handler. The off-thread implementation itself is deferred to a
follow-up feature.

## Objectives

- Quantify `process_pty_data_fully` cost over a ~2 MiB scrollback (FR1).
- Map the number to the §4 thresholds and record the decision (FR2).
- Guard-rail the already-tight scrollback lock scope in the snapshot copy
  (explicit drop point + comment + regression test), byte-identical (FR3).

## Prerequisites

### Development Environment

- Rust toolchain pinned by the repo (see `rust-toolchain`), `cargo`.
- Builds run from the project root with `--manifest-path` and a fixed
  `CARGO_TARGET_DIR=src-tauri/target` (build-location rule).

### Dependencies

- Internal: `crates/term_core` (`TerminalCore::process_pty_data_fully`,
  `reset_and_replay`); `src-tauri/src/mux/ipc/{handlers,reattach}.rs`
  (snapshot assembly). No new external dependency.
- Builds on `mux-scroll-isolation` (FR1: scrollback already in the on-demand
  snapshot via `build_snapshot_bytes`).

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: native terminal stack (no new framework involvement)
- **Key Libraries**: standard library only (`std::time::Instant` for timing).
  No criterion / bench framework is added.

### Design Approach

Two independent, non-data-model-changing edits plus a measurement-driven
decision:

1. A gated timing harness lives next to the measured function in `term_core`,
   using a fixed synthetic input so re-runs are stable and the default test run
   is unaffected.
2. The daemon snapshot handler already releases the scrollback lock right after
   the owned copy; the change makes that drop point explicit and adds a
   regression test, keeping the assembled bytes identical.
3. The recorded measurement feeds an explicit threshold decision, written at
   verify time, that scopes a separate follow-up feature.

### Component Interaction

The harness calls into `term_core` directly (no `App::pump_all`, no PTY), so it
is isolated from the flaky pump path. The handler change is internal to the
daemon connection task; the client and protocol are unchanged.

## Implementation Phases

### Phase 1: Reparse-cost measurement harness (FR1, NFR1)

**Goal**: A deterministic, on-demand measurement that reports the reparse time
of a ~2 MiB scrollback, excluded from the default test run.

**Files to Create**: none (added inside the existing crate test module).

**Files to Modify**:
- `crates/term_core/src/terminal_core.rs` — add a gated timing test plus a
  small deterministic synthetic-input helper.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| synthetic scrollback builder | Produce a fixed byte buffer of a requested size, representative of terminal output (printable text, periodic newlines, occasional SGR) | requested size ≥ 0 | returns a deterministic buffer of ≈ requested size; no RNG / clock input |
| reparse timing harness | Feed the buffer to `process_pty_data_fully` on a freshly built core and report elapsed time + throughput | a `TerminalCore` at a representative grid size | prints ms and MiB/s; excluded from default `cargo test` |

**Processing Flow** (diagram-convertible):
1. Build the synthetic buffer once at the target size (default ≈ capacity, 2 MiB).
2. Construct a fresh core at a representative grid size.
3. Start a monotonic timer.
4. Feed the whole buffer through the full-drain reparse entry point.
5. Stop the timer; report elapsed ms and MiB/s.
   - Optionally repeat at a few sizes (256 KiB / 1 MiB / 2 MiB) to show scaling.

**Implementation Steps** (max 5-7):
1. **Synthetic input helper** — deterministic builder for an N-byte
   representative scrollback.
2. **Timing harness** — gated test that times the full-drain reparse and prints
   the figures; default-excluded so normal `cargo test` is unaffected.
3. **Empty-input guard** — confirm a 0-byte input neither panics nor misreports.

**Dependencies**: Requires `term_core`. Blocks Phase 3 (needs the number).

**Testing Approach**:
- Unit: empty-input path returns without panic (≈0 ms).
- Manual/measurement: run the gated harness, capture the ~2 MiB figure.
- Isolation: harness does not touch `App::pump_all` or a real PTY.

**Acceptance Criteria**:
- [ ] Running the gated harness prints ms + MiB/s for a ~2 MiB synthetic input.
- [ ] The harness is deterministic and excluded from default `cargo test`.

**Estimated Effort**: small

---

### Phase 2: Lock-scope guard-rail for the snapshot copy (FR3, NFR2, NFR3)

**Goal**: Make the snapshot handler's already copy-only scrollback lock scope
explicit and regression-protected, with byte-identical snapshot output. (The
current code already drops the guard right after `read_all`; this phase locks
that invariant in rather than shortening the hold.)

**Files to Create**: none.

**Files to Modify**:
- `src-tauri/src/mux/ipc/handlers.rs` — make the scrollback guard's drop point
  explicit and add a byte-identity regression test.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| snapshot handler lock scope | Hold the scrollback lock only for the `read_all` copy via an explicit scoped block that returns the owned buffer; comment names the invariant | pane resolved and authorized to the requester's session | lock provably not held across assembly / log / send; assembled snapshot bytes unchanged |

**Processing Flow** (diagram-convertible):
1. Resolve + authorize the pane (unchanged).
2. In an explicit scope, acquire the scrollback lock, copy to an owned buffer,
   drop the guard at scope end.
3. Assemble the snapshot from the owned copy + shadow screen (unchanged ordering).
4. Log size and send over the pane output channel (unchanged).

**Implementation Steps** (max 5-7):
1. **Make drop point explicit** — wrap the `read_all` copy in a scoped block
   that returns the owned buffer, with a comment naming the "lock held only for
   the copy" invariant; behavior unchanged.
2. **Byte-identity safeguard** — a test asserting the assembled snapshot bytes
   equal the established layout for a representative screen + scrollback (and for
   empty scrollback).

**Dependencies**: Requires the daemon snapshot path. Blocks nothing.

**Testing Approach**:
- Unit: snapshot bytes byte-identical vs. the established layout for a
  representative pane (and for empty scrollback).
- Integration: existing mux reattach/snapshot tests pass unchanged.

**Acceptance Criteria**:
- [ ] Scrollback guard's drop point is explicit (scoped block + comment), not
      spanning assembly/log/send.
- [ ] Snapshot output bytes unchanged; existing tests green.

**Estimated Effort**: small

---

### Phase 3: Measure & record go/no-go decision (FR2)

**Goal**: Run the Phase 1 harness on the target machine, map the figure to the
§4 thresholds, and record the decision + rationale (and, if "go", the follow-up
feature scope).

**Files to Create**: none in this phase (the decision is written into
`VERIFICATION_RESULT.md`, which sdd.6-verify owns/creates).

**Files to Modify**: none (documentation/decision activity).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| decision record | Capture measured ms + chosen threshold band + rationale | Phase 1 harness runnable | decision and follow-up scope recorded at verify time |

**Processing Flow** (diagram-convertible):
1. Run the gated harness; capture the ~2 MiB figure (and scaling samples).
2. Map to thresholds:
   - `< 5 ms` -> skip 案a (FR3 only).
   - `5–50 ms` -> gray zone; decide with the value attached.
   - `50 ms+` -> file a follow-up SDD feature for 案a (core only, no LRU).
3. Record the decision + rationale (including real-world 2 MiB-fill frequency).

**Implementation Steps** (max 5-7):
1. **Run & capture** — execute the harness, record figures.
2. **Decide & document** — apply thresholds and write the decision at verify.

**Dependencies**: Requires Phase 1. Blocks nothing.

**Testing Approach**:
- Manual/verify: the decision record exists and cites the measured value and
  threshold band.

**Acceptance Criteria**:
- [ ] Measured value mapped to a threshold band with rationale recorded.
- [ ] If "go", the follow-up feature scope (案a, core only, no LRU) is stated.

**Estimated Effort**: small

---

## Complete File Structure

```
crates/term_core/src/terminal_core.rs   # Phase 1: gated timing harness + helper
src-tauri/src/mux/ipc/handlers.rs        # Phase 2: explicit scrollback lock scope + byte-identity test
doc/tasks/mux-snapshot-reparse-offthread/
  要件定義書.md
  SPEC.md
  IMPLEMENTATION.md
  VERIFICATION.md
  VERIFICATION_RESULT.md                 # Phase 3 decision (created at verify)
  sdd.yaml
  tasks.yaml
```

## Testing Strategy

- Unit: empty-input guard (FR1); snapshot byte-identity (FR3).
- Integration: existing mux reattach/snapshot suite unchanged (FR3).
- Measurement: gated harness figure (FR1 → FR2 decision).
- CLI-only: `cargo check --no-default-features` stays green (NFR2/NFR3).
- The measurement harness is excluded from the default test run (gated).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | — | std `Instant` timing only; no bench framework |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Machine-dependent figures flip the decision | Medium | Medium | Threshold bands (<5 / 5–50 / 50+); gray zone decided with value in hand |
| Synthetic input not representative of real scrollback | Medium | Low | Mix printable text + newlines + occasional SGR |
| Lock-scope change alters snapshot bytes | Low | High | Byte-identity test + existing reattach/snapshot suite |

## Open Questions

- [ ] Gray-zone (5–50 ms) final call is a runtime decision with the measured
      value attached (not a spec gap).

## Success Metrics

- [ ] FR1 harness deterministic and default-excluded; ~2 MiB figure captured.
- [ ] FR2 decision recorded against thresholds with rationale.
- [ ] FR3 lock-scope guard-rail in place, snapshot bytes unchanged, suite green.
- [ ] `cargo test` (default) and CLI-only `cargo check` green.
