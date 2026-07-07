# Implementation Plan: mux-snapshot-device-query-strip

## Overview

Extend the daemon-side scrollback filter so that snapshot assembly removes CSI device queries that would make the GUI synthesize stale replies on replay. Single-task feature; this document records only the decisions the task must not deviate from.

## Technology Stack

- **Language**: Rust (src-tauri crate, `mux` module — GUI feature-gated side)
- **Key reference**: `crates/term_core/src/csi_dispatch.rs` — the response-behavior SSOT the strip predicate mirrors (read-only; term_core is NOT modified)

## Layer Structure

Unchanged. The filter stays in `src-tauri/src/mux/scrollback_filter.rs`, the single shared home for snapshot byte filtering, called by `mux::snapshot_bytes::build_snapshot_bytes` and `mux::ipc::pty_spawn`. No new modules, no caller changes.

## Shared Components

Single-task feature — no cross-task contracts. (The public contract that matters is `strip_replayable_rich_content` keeping its existing signature so callers stay untouched.)

## Conventions

- Strip predicate must mirror term_core's actual dispatch conditions exactly (SPEC.md FR1/FR2 tables are normative). When term_core would not respond, the sequence is preserved byte-for-byte — over-stripping is a correctness bug, not a safety margin.
- Follow the module's existing filtering conventions: unterminated sequences are preserved; the pass stays O(n); doc comments on the function list what is removed vs kept (update them to include the query set).

## Cross-task Design Decisions

### D1: Strip at snapshot assembly, not at scrollback write time

The filter runs where `strip_replayable_rich_content` already runs (per-snapshot). Rationale: same placement as the viewer-OSC strip, zero protocol/GUI changes, and it automatically covers the reattach, on-demand snapshot, and visibility-resume paths. Affected task: task0001.

### D2: C0 bytes inside a stripped CSI are re-emitted

term_core's parser executes C0 controls mid-CSI without aborting the sequence, so dropping them with the query would change replay behavior. A bare ESC inside a CSI body aborts the candidate: the scanned prefix is preserved and scanning resumes at that ESC. Affected task: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Over-stripping non-query CSI (DECSTR, title stack, DECXCPR) alters replay | Medium | Medium | Keep-set tests (SPEC TS-7) enumerate the near-miss sequences; predicate mirrors dispatch table |
| Perf regression on 2 MiB scrollback | Low | Medium | Existing `#[ignore]` bench threshold (30ms) re-run |
| Divergence if term_core later answers new queries | Low | Low | Doc comment cross-references csi_dispatch.rs as the SSOT |

## Open Questions

- none
