# Implementation Plan: mux-connection-input-freeze

## Overview

Close the two remaining task self-block instances left by
mux-window-switch-output-hang: the daemon connection task's drain arm parks
the whole `select!` loop when the socket send buffer is full
(`src-tauri/src/mux/ipc/connection.rs`), and the bridge performs synchronous
stdout syscalls on its tokio runtime thread
(`src-tauri/src/mux/bridge.rs`). Two independent fix sites, two independent
tasks, one shared architectural pattern.

## Technology Stack

- **Language / Runtime**: Rust, tokio async runtime (already in use at both
  fix sites). No new dependencies are introduced; the fixes use tokio's
  existing bounded-channel / task primitives and the standard library's
  threading primitives only.
- **License note (MANDATORY check)**: no new dependency is added, so the
  project license (MIT) is unaffected. Nothing to record beyond this line.

## Layer Structure

```
GUI (mux client)                       — untouched (NFR2)
  ↑ PTY (stdout)         ↓ stdin
bridge forward_loop (bridge.rs)        — task0002 (FR2)
  ↑ socket               ↓ socket
daemon connection task (connection.rs) — task0001 (FR1, FR3, NFR4)
  ↑ pane_output channel
PTY reader threads                     — untouched
```

Each task owns exactly one hop. The two hops share no code: task0001 touches
only `src-tauri/src/mux/ipc/` and task0002 touches only
`src-tauri/src/mux/bridge.rs`, so the tasks have zero file overlap and no
cross-task component contract is required.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none)    | The two tasks modify disjoint files and call none of each other's code. | — | — |

## Conventions

Cross-task rules both tasks must follow:

1. **No point-position capacity-await in a polling loop** (the anti-pattern
   this feature removes): inside a `select!` loop that is also responsible
   for polling input, no arm body may contain an await whose completion
   depends on a slow consumer draining (socket send capacity, stdout
   progress). Blocked-writer state is held as loop-owned state polled as its
   own `select!` arm, or handed to a dedicated writer component — never
   awaited inline in an arm body.
2. **Every queue is bounded** (FR4): any channel, queue, or holding buffer
   introduced by either task has an explicit, named capacity constant with a
   one-line rationale comment. Unbounded channels are forbidden. When a
   bounded queue is full, the producer side stops consuming its own
   upstream (backpressure propagates) instead of buffering further.
3. **FIFO per hop** (FR3): each hop forwards frames strictly in the order it
   accepted them — a single ordered admission path feeding a single writer.
   End-to-end same-pane ordering follows from each hop preserving order
   independently; neither task depends on the other's internals for this.
4. **No protocol change** (NFR1): neither task modifies `crates/mux_ipc`,
   the codec (`src-tauri/src/mux/ipc/codec.rs`), or any wire message shape.
5. **Test waits are named and bounded**: every wait in a new test uses a
   named timeout constant (project convention, `test/README.md`). The AC-3
   "bounded delay" quantification is fixed here for both the tests and
   VERIFICATION.md: the regression tests assert completion within a named
   **5-second** timeout constant (matching the existing named-timeout
   convention in `connection.rs` tests; the design-level expectation is that
   the asserted event does not wait on send capacity at all, i.e. it is
   millisecond-scale — the 5 s constant only absorbs CI scheduling noise).
6. **Command discipline**: build/test/format runs use exactly the command
   strings listed in VERIFICATION.md (project PreToolUse hook enforces
   them): always from the project root, with `CARGO_TARGET_DIR` and
   `--manifest-path`, never `cd src-tauri/`. Crate-wide write-mode
   formatting is forbidden (the crate is not fully rustfmt-normalized);
   release builds are user-initiated only.
7. **Platform neutrality**: changed code must compile for both platforms
   (the bridge's `forward_loop` is shared by the Unix and Windows entry
   points). Test helpers needing PTY/pipe APIs are Unix-gated
   (`#[cfg(all(test, unix))]`); Windows-specific validation is out of scope
   (NFR3).

## Cross-task Design Decisions

### D1: Writer-decoupling pattern (both tasks)

Both fixes apply the same shape: the component that must keep polling
(connection task's `select!` loop; the bridge's async forwarding future)
never performs the slow write itself. Instead, a **bounded FIFO admission
path feeds a dedicated writer** (a separate tokio task for the socket in
task0001; a dedicated blocking-capable writer context for stdout in
task0002) that alone performs the write and drains independently of the
polling component. Rationale: this is the candidate fix SPEC.md's own
assumptions record, it preserves FIFO trivially (single queue, single
writer), and it keeps backpressure intact (bounded admission). Affected:
task0001, task0002.

### D2: End-to-end backpressure chain stays intact (both tasks)

After both fixes the stall chain under a saturated consumer is:
GUI PTY full → bridge stdout writer stalls → bridge's bounded stdout queue
fills → daemon→stdout forwarding future suspends (yields; stdin→daemon keeps
running) → socket fills → daemon's socket writer stalls → connection's
bounded outbound path fills → drain arm stops consuming `pane_output_rx` →
pane channel fills → PTY reader threads park in their existing blocking
send. Every element is bounded; no hop parks a thread or a polling loop.
Each task is responsible for its own two links in this chain and must not
assume more than "my downstream eventually drains or my queue fills".
Affected: task0001, task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Single-writer rework in connection.rs reorders control replies vs. PTY frames (FR3 break) | Medium | High | task0001 contract: ONE ordered admission path for all client-bound frames after the GUI loop starts; ordering AC + existing connection-level saturation test must stay green |
| Reap/ack semantics silently change when sends become asynchronous (NFR4) | Medium | High | task0001 pins both contracts explicitly (reap at chunk-consumption time; Upgrading ack only after the frame is flushed to the socket) with dedicated AC |
| Drain arm re-blocks one level deeper (await on outbound admission instead of on the socket) | Medium | High | Convention 1 + task0001 AC-1 regression test (input processed within the named timeout while the socket side is saturated) |
| Bridge queue erases backpressure or grows unboundedly | Low | High | Convention 2; task0002 AC pins the bound and the suspend-on-full behavior |
| Shared `forward_loop` change breaks the Windows build | Low | Medium | Convention 7; both `cargo check` variants are verification gates |
| Throughput regression from extra queue hop | Low | Medium | `mux_throughput` integration test is a verification gate (TS4) |

## Open Questions

- [ ] None. All requirements are `ok`; no TBD, no license conflict, no
  existing planning artifacts to reconcile.
