# Implementation Plan: tmux-startup-query-response-leak

## Overview

Starting tmux inside eMterm leaks device-query responses (DA2 + XTWINOPS,
reported as `\x1b[>65;1;0c\x1b[8;51;207t\x1b[4;816;1656t` shown twice) into
the visible pane. This feature identifies the root cause (currently unknown)
and fixes the routing so every synthesized response reaches the querying
application's PTY input exactly once and never becomes visible content.
Single-task feature; this document records only the decisions the task must
not deviate from.

## Technology Stack

- **Language**: Rust — `crates/term_core` (response synthesis, `--lib` tests)
  and the `src-tauri` crate (`tabs.rs` write-back sites, `mux` module; GUI
  feature-gated side).
- **New dependencies**: none (assumption A2). License review: nothing to
  record; `project.license: MIT` is unaffected.

## Resolved TBD Requirements (recorded assumptions)

Both TBDs were pre-resolved by the batch gate `create-plan.tbd-resolution`
(policy: assume); `workflow.yaml` carries them as `status: assumed`.

- **FR4 → `both`**: the fix and its verification cover BOTH runtime
  contexts — a plain eMterm tab running tmux, AND a tmux running inside a
  mux pane (including the detach/reattach replay path).
- **FR5 → `generalize`**: the in-scope response set is the taxonomy defined
  by feature `mux-snapshot-device-query-strip`: DA1 / DA2 / DSR (status and
  CPR) / XTWINOPS 14, 16, 18 / DECRPM — not only the observed DA2 +
  XTWINOPS 8/4 symptom.

## Layer Structure

Unchanged. Response synthesis stays in `crates/term_core` (single-slot
response buffer drained via `take_response`); response write-back stays in
the `src-tauri` Tab layer (the three existing write-back sites in
`tabs.rs`); snapshot/replay filtering stays in `src-tauri/src/mux/`
(`scrollback_filter.rs` as the single home of the strip predicates). No new
modules, no new layers, no dependency-direction changes.

## Shared Components

Single-task feature — no cross-task contracts. The public contracts that
matter are existing ones the task must preserve:

| Component | Responsibility | Contract to preserve | Used by tasks |
|-----------|----------------|----------------------|---------------|
| `TerminalCore::take_response` | Hand the pending device response to the embedder and clear the slot | Post: returned bytes are the response(s) synthesized since the previous drain; a second call returns empty. Callers (all in `tabs.rs`) remain the only delivery route to a PTY | task0001 |
| `strip_replayable_rich_content` (+ its write-path alias) | Remove replay-unsafe content (viewer OSC, device queries, response echoes) from daemon snapshot/scrollback bytes | Signature and existing strip/keep sets unchanged or strictly documented if extended; never weakened (NFR4) | task0001 |

## Conventions

- Tests are inline `#[cfg(test)]` modules next to the code under test and
  run under `--lib` (FR3); follow `test/README.md` naming
  (`<subject>_<scenario>_<expected>`) and construction style.
- Run cargo from the project root with `--manifest-path` and
  `CARGO_TARGET_DIR=src-tauri/target`; never `cd` into `src-tauri/`. The
  `tabs.rs` replay tests may need `-- --test-threads=1` (known parallel
  flake).
- Never run a crate-wide `cargo fmt` (the tree is intentionally not
  rustfmt-clean); keep formatting local to edited lines.
- GUI-only code stays behind the `gui` feature; the CLI build
  (`--no-default-features`) must keep compiling (NFR2). `crates/term_core`
  is built in both configurations.

## Cross-task Design Decisions

Single-task feature; these bind the one task.

### D1: Routing, not suppression (FR1, FR2)

The fix delivers each synthesized response to the PTY input of the
application that issued the query, exactly once, and guarantees response
bytes never enter the visible grid, the scrollback, or any snapshot/replay
store. Dropping responses is not an acceptable fix: tmux capability
negotiation must keep working (verified manually by TS3).

### D2: Root cause first, bounded (FR1)

The root cause is NOT established. The task begins with a bounded
investigation phase whose deliverables are (a) a unit test that reproduces
the leak mechanism at byte level and fails on the pre-fix code, and (b) a
root-cause statement in the implementer report. No speculative fix is
committed before that test exists. The investigative leads (assumption A3
and the candidate paths) are enumerated in the task plan, not here.

### D3: Existing protections are floors, not levers (FR6, NFR4)

The `per-tab-grid-size` resize-routing behavior and the
`mux-snapshot-device-query-strip` snapshot filtering are prerequisites the
fix builds on. Their existing tests stay green and their strip/keep sets
are not weakened. If the fix extends a strip predicate, over-stripping
remains a correctness bug (sequences term_core would not answer are
preserved byte-for-byte — NFR3).

### D4: Hot-path neutrality (NFR1)

Non-query PTY traffic must not gain per-byte work. The existing
query-detection gate (`payload_has_device_query` /
`pty_output_batch_eligible` in `tabs.rs`) is the pattern: classification
happens once per frame/chunk, not per byte of ordinary output. Any change
to the 2 MiB-scrollback strip path re-runs that module's existing
`#[ignore]` bench against its documented threshold.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Root cause differs from lead A3 and lies outside the predicted file set | Medium | Medium | Investigation phase is explicitly bounded and reports before fixing; a file outside the task's `files` set is a reportable plan deviation, not silent scope creep |
| Fix suppresses instead of routes → tmux loses capability negotiation | Low | High | D1 invariant + TS3 manual health check + exactly-once delivery assertions in TS4 |
| Exactly-once delivery breaks the PSReadLine CPR write-back (Windows) or other legitimate consumers | Low | High | TS4 asserts delivery still happens (not just "no leak"); existing DSR/CPR tests stay green |
| Single-slot 64-byte response buffer drops one reply when a chunk carries multiple queries | Medium | Medium | Investigation item in the task plan; if implicated, the fix must preserve every response (FR2), not only deduplicate |
| Regressing per-tab-grid-size / mux-snapshot-device-query-strip | Low | High | TS7: their suites run unmodified in the `--lib` gate |

## Open Questions

- none
