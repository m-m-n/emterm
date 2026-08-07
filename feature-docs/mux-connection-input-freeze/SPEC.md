# Feature: mux-connection-input-freeze

## Overview

A residual input-path freeze remains inside a single mux bridge connection: while one mux window runs mass output (`seq 1 10000000`), switching away from it freezes that window's rendering and blocks key input in other windows on the same connection. This feature closes the same-shape task self-block instances that the prior feature mux-window-switch-output-hang (main HEAD 1620079) left unfixed at the socket layer (daemon connection task) and at the bridge stdout layer. The requirements document for this feature is `feature-docs/mux-connection-input-freeze/REQUIREMENTS.md`.

## Objectives

- Eliminate the residual input-path freeze within a single mux bridge connection: switching away from a window running mass output (`seq 1 10000000`) must not freeze rendering of that window nor block key input in other windows on the same connection, without waiting for the producer to finish.
- Close the remaining same-shape task self-block instances that feature mux-window-switch-output-hang (main HEAD 1620079) left unfixed at the socket layer (daemon connection task) and the bridge stdout layer.

## User Stories

### US1: Switch away from a mass-output window and keep typing
As a mux user, I want to switch from a window running `seq 1 10000000` to another window and keep typing there, so that I do not have to wait for the producer to finish.

**Acceptance Criteria:**
- [ ] AC-1: While `seq 1 10000000` runs in one mux window, switching to another window allows continuous key input at the destination, without waiting for seq to complete.
- [ ] AC-2: After switching, the seq window does not drag other windows' input down with it.

### US2: Guarantee bounded input polling by test
As a maintainer of the mux daemon, I want a test that pins the connection task's input polling to a bounded delay while its output side is saturated, so that this class of task self-block cannot regress.

**Acceptance Criteria:**
- [ ] AC-3: A test guarantees the daemon connection task's select! keeps polling `framed.next()` within a bounded delay even while draining.

## Technical Requirements

### Functional Requirements

- **FR1 — Daemon connection task: drain arm must not starve select!:** The connection task's PTY-batch drain arm in `src-tauri/src/mux/ipc/connection.rs` (currently `framed.feed(msg).await` / `framed.flush().await` at lines 665-671, point-position awaits inside the select! arm body) must no longer be able to park the whole task when the socket send buffer is full. While drain-side output is pending, the select! loop must keep polling `framed.next()` (client input: SwitchWindow, key input) within a bounded delay.
- **FR2 — Bridge: no synchronous stdout syscall on the tokio task:** The `daemon_to_stdout` async block in `src-tauri/src/mux/bridge.rs` (sync `std::io::stdout().lock()` + `write_all` at lines 594-621) must not block the bridge's tokio runtime task when the GUI PTY buffer is full; the socket-drain direction must keep making progress independently of stdout write progress.
- **FR3 — Preserve same-pane FIFO ordering (carried from prior feature):** Snapshot chunks and PTY output chunks for the same pane keep FIFO order, and the fix stays consistent with the existing deferred-output path (`flush_deferred_output` / `arm_pending_deferred_reserve`, connection.rs:707-724) whose only capacity-freeing point is the drain arm.
- **FR4 — Preserve backpressure; no unbounded channels (carried from prior feature):** The fix must not erase backpressure by introducing unbounded channels; memory growth stays bounded end-to-end.
- **FR5 — Regression test for bounded input polling during drain:** A test guarantees the daemon connection task's select! continues to poll `framed.next()` within a bounded delay while the drain/output side is saturated.

### Non-Functional Requirements

- **NFR1 - Protocol stability:** No mux protocol change (credit-based flow control is explicitly a separate feature).
- **NFR2 - Scope boundary (GUI buffer sizing):** GUI-side `event_tx` bounded(4096) buffer sizing is out of scope.
- **NFR3 - Scope boundary (platform):** Windows bridge (`bridge_main_loop_windows`) same-shape fix is out of scope; establish the pattern on Unix first.
- **NFR4 - No regression of existing drain-arm semantics:** Existing PTY-exit reap semantics in the drain arm (reap regardless of delivery success, connection.rs:672-691) and the Upgrading-frame ack path (connection.rs:738-747) must not regress.

## Implementation Approach

### Architecture

**Affected layers:**

```
┌─────────────────────────────────────────────┐
│ GUI (mux client)                            │  event_tx bounded(4096) — out of scope (NFR2)
├─────────────────────────────────────────────┤
│ bridge (src-tauri/src/mux/bridge.rs)        │  daemon_to_stdout, lines 594-621 — FR2
├─────────────────────────────────────────────┤
│ socket (mux bridge connection)              │  no protocol change (NFR1)
├─────────────────────────────────────────────┤
│ daemon connection task                      │  select! + PTY-batch drain arm,
│ (src-tauri/src/mux/ipc/connection.rs)       │  lines 665-671 — FR1
│                                             │  PTY-exit reap 672-691, deferred output
│                                             │  707-724, Upgrading ack 738-747 — NFR4/FR3
└─────────────────────────────────────────────┘
```

**Component notes:**
- Daemon connection task: a single select! loop that both polls `framed.next()` (inbound client messages such as SwitchWindow and key input) and drains PTY batches outbound. Today the drain arm's `framed.feed(...).await` / `framed.flush().await` are point-position awaits inside the arm body, so a full socket send buffer parks the whole task and inbound polling stops (FR1).
- Deferred-output path: `flush_deferred_output` / `arm_pending_deferred_reserve` (connection.rs:707-724); the drain arm is its only capacity-freeing point, so the reworked drain path must stay consistent with it (FR3).
- Bridge: `daemon_to_stdout` performs a synchronous `std::io::stdout().lock()` + `write_all` on a tokio task; when the GUI PTY buffer is full this blocks the runtime task and stalls the socket-drain direction (FR2).

### Data Flow

```
daemon PTY output → connection task drain arm → socket → bridge daemon_to_stdout → GUI PTY
client key input / SwitchWindow → socket → connection task framed.next() → daemon
```

The two directions above share the connection task (daemon side) and the bridge tokio task; the fix must keep the inbound direction progressing regardless of outbound progress.

### Assumptions (from requirements analysis)

- The concrete fix mechanism is a plan-phase decision, per the task description's own candidates: (B) move `framed.feed/flush` into a separate tokio task fed by a bounded channel with the connection task doing `try_send` only; (A) move bridge stdout writes to `spawn_blocking` or a dedicated thread + `tokio::sync::mpsc`.
- Both fix sites (B: connection.rs drain arm, A: bridge.rs daemon_to_stdout) are in scope of this one feature; B is the primary culprit, A the chain origin.
- "Bounded delay" in AC-3 is quantified at plan/spec time (the task does not fix a number); the test asserts a named finite timeout in line with existing mux test conventions (every wait bounded with a named timeout, per test/README.md).

### API Design

Not applicable. No mux protocol change (NFR1); no HTTP or RPC surface is added or modified.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/mux/ipc/connection.rs`: the connection task's select! loop, PTY-batch drain arm, PTY-exit reap path, deferred-output path, Upgrading-frame ack path.
- `src-tauri/src/mux/bridge.rs`: `daemon_to_stdout`; `bridge_main_loop_windows` is explicitly out of scope (NFR3).
- Prior feature mux-window-switch-output-hang (main HEAD 1620079): the same-shape fix whose remaining instances this feature closes.

**External Dependencies:**
- tokio: the runtime hosting the connection task and the bridge tasks (select!, bounded channels, `spawn_blocking`).
- Rust toolchain via cargo; builds and tests run with an explicit `CARGO_TARGET_DIR` and `--manifest-path src-tauri/Cargo.toml`.
- TypeScript / bun components are untouched by this backend fix.

### File Structure

```
src-tauri/src/mux/
├── ipc/connection.rs        # FR1, FR3, NFR4 — drain arm, deferred output, reap, ack
└── bridge.rs                # FR2 — daemon_to_stdout (Unix path only; NFR3)
src-tauri/tests/             # integration tests (alternative home for TS1/TS3)
```

## Test Scenarios

### Unit Tests
- [ ] **TS1** (FR1, FR5; AC-3): Unit/integration test (Rust, `--lib` or `src-tauri/tests/`): saturate the connection task's output side (small/full socket or stub sink) and assert an inbound client message (e.g. SwitchWindow or key input) is processed within a bounded timeout.
- [ ] **TS3** (FR2): Test (or targeted unit coverage) that bridge daemon→stdout progress stalling does not stall socket drain, Unix-gated `#[cfg(all(test, unix))]` where PTY/pipe APIs are needed.

### Integration Tests
- [ ] **TS2** (FR3): Test that same-pane snapshot→PTY-output FIFO order is preserved through the reworked drain path, including the deferred-output retry.
- [ ] **TS4** (FR3, FR4, NFR4): Full `--lib` suite regression run; `mux_throughput` integration test as throughput regression guard (tabs.rs replay tests may need `--test-threads=1`).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] **TS5** (FR1, FR2; AC-1, AC-2): Manual user verification of the original repro (`seq 1 10000000`, switch windows, type in destination) — no E2E infrastructure exists.

### Edge Cases
- [ ] Socket send buffer full while drain-side output is pending: inbound `framed.next()` polling continues within a bounded delay (FR1).
- [ ] GUI PTY buffer full while the bridge writes to stdout: socket-drain direction keeps making progress (FR2).
- [ ] Deferred-output retry through the reworked drain path: same-pane FIFO order holds (FR3).

### Performance Tests
- [ ] Throughput regression guard: `mux_throughput` integration test (TS4).

## Security Considerations

Not applicable. This is a backend concurrency fix inside the mux daemon connection task and bridge I/O path; no authentication, authorization, input-validation, or data-protection surface is added or changed, and the mux protocol is unchanged (NFR1).

## Error Handling

- PTY-exit reap in the drain arm keeps its existing semantics — reap regardless of delivery success (connection.rs:672-691) — after the rework (NFR4).
- The Upgrading-frame ack path (connection.rs:738-747) keeps its existing behaviour (NFR4).
- Backpressure remains in effect: no unbounded channel is introduced to absorb output that cannot be delivered, and memory growth stays bounded end-to-end (FR4).

## Performance Optimization

### Performance Goals
- Inbound client messages (SwitchWindow, key input) are polled and processed within a bounded delay while the output side is saturated; the numeric bound is quantified at plan/spec time (AC-3, FR5).
- No throughput regression relative to the current mux output path, guarded by the `mux_throughput` integration test (TS4).

### Optimization Strategies
The concrete mechanism is a plan-phase decision; see Assumptions above for the two candidates carried from the task description.

## Success Criteria

- [ ] All functional requirements (FR1-FR5) are implemented and tested
- [ ] All test scenarios (TS1-TS5) pass
- [ ] AC-1: While `seq 1 10000000` runs in one mux window, switching to another window allows continuous key input at the destination, without waiting for seq to complete
- [ ] AC-2: After switching, the seq window does not drag other windows' input down with it
- [ ] AC-3: A test guarantees the daemon connection task's select! keeps polling `framed.next()` within a bounded delay even while draining
- [ ] Non-functional requirements (NFR1-NFR4) are satisfied, including no regression of the PTY-exit reap and Upgrading-ack paths
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement (FR1-FR5, NFR1-NFR4) is `resolved`.

## Implementation Phases (if applicable)

Not applicable.

## References

- Requirements document: `feature-docs/mux-connection-input-freeze/REQUIREMENTS.md`
- Fix site (primary culprit): `src-tauri/src/mux/ipc/connection.rs`
- Fix site (chain origin): `src-tauri/src/mux/bridge.rs`
- Prior feature: mux-window-switch-output-hang (main HEAD 1620079)
- Test conventions: `test/README.md` (every wait bounded with a named timeout)
