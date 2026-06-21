# Feature: mux client coalesce (phase1)

## Overview

Improve large-output throughput in mux windows by coalescing, on the client (GUI) side, the inner payloads of consecutive `PtyOutput` frames addressed to the same active pane that arrive within a single `pump`, so that `term_core` is invoked once per consecutive run instead of once per frame.

This is phase1 of the mux performance work (client-side `PtyOutput` inner-payload coalescing). phase2/3 are out of scope.

## Objectives

- Reduce `process_pty_data_fully` invocations per `pump` from ~1400 to a few (one per consecutive active-pane `PtyOutput` run).
- Improve daemon-direct E2E throughput (before: 2.85 MiB/s at N=10M) and reduce frame count (before: 124,233 at N=10M).
- Keep the change purely performance-oriented: coalesced output must be byte-for-byte equivalent to the per-frame path.

## Background (measured bottleneck)

- mux window: `seq 1 10000000` runs at 2% CPU / 2m26s (a normal tab: 96% CPU / 3.8s).
- daemon-direct E2E (excludes bridge/GUI rendering, includes term_core parse):
  - N=10,000,000 → 84.77 MiB / 124,233 frames / 29.75s = 2.85 MiB/s
  - N=1,000,000 → 7.52 MiB / 13,958 frames / 3.15s = 2.39 MiB/s
- 85 MiB is split into ~120k frames (≈700 B/frame). The frame count is the core of the overhead.
- `pump()` coalesces raw bytes up to 1 MiB (`COALESCE_CAP`, `tabs.rs`), but on the mux path each extracted mux APC frame still triggers its own `process_pty_data_fully`.
- The bottleneck is parse invocation count: 1 MiB ÷ 700 B ≈ 1400 frames/pump, each parsed individually.

## User Stories

### US1: Display large output in a mux window

As a mux-window user, I want large bursts of output to render with throughput close to a normal tab, so that AI tooling that streams heavy output stays responsive.

**Acceptance Criteria:**
- [ ] Consecutive active-pane `PtyOutput` payloads are parsed in a single `process_pty_data_fully` call.
- [ ] Output is byte-for-byte equivalent to the per-frame path (correctness contract).
- [ ] daemon-direct E2E shows improved throughput and reduced frame count.

## Technical Requirements

### Functional Requirements

- **FR1 (Coalesce consecutive PtyOutput):** In the extracted-frame loop of `process_combined`, accumulate the inner payloads of consecutive active-pane `PtyOutput` frames into a concatenation buffer instead of parsing each immediately. Flush the buffer to `process_pty_data_fully` exactly once when (a) a non-`PtyOutput` control message is encountered (before handling it), or (b) the loop ends.
- **FR1a (Device-query frames are not coalesced):** A `PtyOutput` frame whose payload contains a complete CSI device query — a sequence `term_core` answers, i.e. final byte `n` (DSR), `c` (DA), `t` (XTWINOPS size report), or `p` (DECRPM) — is NOT batch-eligible. It is a boundary: the accumulator is flushed and the frame is parsed on its own via the per-frame path so its reply is captured before a later query overwrites `term_core`'s single-slot, overwrite-only response buffer. This preserves device-response parity with the per-frame baseline (NFR2). Detection is conservative (final-byte match), so a few non-response sequences sharing those finals are also parsed individually — correctness-neutral, negligible perf cost since such sequences are absent from bulk output.
- **FR2 (Flush at control-message boundaries):** Before handling any control message (`Welcome` / `PaneCreated` / `Detached` / `PtyExited`, etc.), flush the accumulation buffer so output/control ordering is preserved.
- **FR3 (Post-batch side effects once):** After the coalesced `process_pty_data_fully`, perform `take_response` (device-response write-back) and `drain_marks` (OSC 133) once per batch (not per frame). Inner image APC/DCS produced by the parse are drained once per pump by the existing post-loop block in `process_combined` — this matches the per-frame baseline (which also deferred the inner-image drain to the same post-loop point), so NFR2 byte-equivalence is preserved; the coalesce change does not alter image-drain granularity. Device queries (`CSI ... n` / `c` / `t` / `p`) are excluded from coalescing (see FR1a) so each device reply is captured before the next overwrites `term_core`'s single-slot response buffer.
- **FR4 (Non-active pane drop):** Non-active-pane `PtyOutput` frames are not included in the coalesce buffer; they are dropped as before.
- **FR5 (pending_switch keeps legacy path):** While `pending_switch` is `Some` (off-thread replay in progress), `PtyOutput` is excluded from coalescing; the existing per-frame path (push to `live_queue`) is preserved.

### Non-Functional Requirements

- **NFR1 - Performance:** Per-`pump` `process_pty_data_fully` count drops from ~1400 to a few. daemon-direct E2E throughput improves substantially over 2.85 MiB/s (N=10M) and frame count drops below 124,233. The phase1〜3 shared end goal is real-environment `time seq 1 10000000` (mux window) under ~10s (stretch 7.6s) vs. 146s today; phase1 alone is not required to reach that end goal (reaching it determines whether phase2/3 are needed).
- **NFR2 - Correctness preservation:** Coalesced parsing produces identical output to per-frame parsing. The split==concatenated contract test stays green.
- **NFR3 - Scope isolation:** No changes to daemon, bridge, transport layer, or the WebView build (`src/`).

## Implementation Approach

### Affected Code

- `src-tauri/src/tabs.rs`
  - `process_combined` (`tabs.rs:1533`) — mux-established path. `mux_apc_extractor.feed_with_offsets(&combined)` extracts multiple mux APC frames from the coalesced bytes; the extracted-frame loop dispatches each via `apply_mux_message`.
  - `apply_mux_message` (`tabs.rs:822`) — its `MessageType::PtyOutput` arm (`tabs.rs:879`) currently calls `c.process_pty_data_fully(&msg.payload)` once per frame, plus `take_response` / `drain_marks` per frame.
  - `process_outer_via_core` (`tabs.rs:1497`) — non-mux path (single parse of coalesced bytes); unchanged, used as the correctness reference.

### Change Strategy

Rework the extracted-frame loop in `process_combined` so that:

1. Consecutive active-pane `PtyOutput` inner payloads are appended to a concatenation buffer (no immediate parse).
2. The buffer is flushed (single `process_pty_data_fully`) when a non-`PtyOutput` control message arrives (before handling it) or at loop end.
3. Post-batch `take_response`, `drain_marks`, and inner image APC/DCS drain run once per batch.
4. The behaviors currently embedded in the `MessageType::PtyOutput` arm are lifted into the coalesce path:
   - Non-active-pane drop (`tabs.rs:891`) — excluded from the coalesce buffer.
   - `pending_switch` `live_queue` push (`tabs.rs:909`) — retained on the legacy per-frame path (not batched).

### Data Flow

```
PTY (active pane)
  → daemon: split into many PtyOutput mux APC frames (~700 B each)
  → client pump(): coalesce raw bytes up to 1 MiB → process_combined
  → feed_with_offsets: extract mux APC frames
  → loop:
       active-pane PtyOutput  → append to concat buffer
       control message        → flush concat buffer (1 parse) → handle message
       loop end               → flush concat buffer (1 parse)
  → per batch: take_response, drain_marks, image APC/DCS drain
  → render
```

## Test Scenarios

### Unit / Integration Tests (`src-tauri/src/tabs.rs`, `tabs::tests`)

- [ ] **New (required):** A test directly showing that consecutive active-pane `PtyOutput` frames collapse into a single `process_pty_data_fully` invocation.
- [ ] `c_split_messages_equal_single_concatenated_message` (`tabs.rs:4835`) — split==concatenated correctness contract. Must stay green (proves the change is purely a performance change).
- [ ] `c_pty_output_parsed_per_message_grid_grows_step_by_step` (`tabs.rs:4794`) — behavior changes from per-message stepwise reflection to batched reflection. Expected values must be updated to the new (batched) behavior.

### Edge Cases

- [ ] Non-active-pane `PtyOutput` is excluded from coalescing and dropped.
- [ ] Control message in the middle of a `PtyOutput` run flushes the buffer first, preserving ordering.
- [ ] `pending_switch` active: `PtyOutput` follows the legacy per-frame path (`live_queue` push), not the coalesce path.
- [ ] Kitty image chunk spanning `PtyOutput` frame boundaries: coalescing removes the boundary (safe side); control-message boundaries still flush to preserve order.

### Performance Tests

- [ ] daemon-direct E2E (`src-tauri/tests/mux_throughput.rs`, `#[ignore]`, release required): compare MiB/s and frame count before/after. Expect throughput up, frame count down.

## Success Criteria

- [ ] FR1–FR5 implemented.
- [ ] New required test (consecutive PtyOutput → single parse) passes.
- [ ] `c_split_messages_equal_single_concatenated_message` green.
- [ ] `c_pty_output_parsed_per_message_grid_grows_step_by_step` updated to batched behavior and green.
- [ ] daemon-direct E2E shows improved throughput and reduced frame count.
- [ ] Full lib test suite green (`--test-threads=1`; the `tabs.rs` replay tests are non-deterministic under parallelism).

## Verification Commands

Run from the project root with `CARGO_TARGET_DIR=src-tauri/target` (do not use the production `target-host`).

```sh
# E2E throughput (release required, #[ignore])
CARGO_TARGET_DIR=src-tauri/target cargo test --release --test mux_throughput \
  --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture

# client coalesce metrics (tabs.rs c_ tests)
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path src-tauri/Cargo.toml --lib 'tabs::tests::c_'

# regression (whole lib). tabs.rs replay tests are non-deterministic under parallelism → single thread
CARGO_TARGET_DIR=src-tauri/target cargo test \
  --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
```

## Implementation Phases

This SPEC covers phase1 only.

### Phase 1: Client-side PtyOutput inner-payload coalesce
**Goals:** Collapse the per-frame parse flood (~120k parses) into per-run parses; relieve backpressure so PTY reads grow larger and frame count itself drops.
**Deliverables:**
- Reworked `process_combined` extracted-frame loop with coalesce buffer.
- New required test + updated metric test.
- daemon-direct E2E before/after measurement.

### Phase 2 / Phase 3 (out of scope)
Escalation phases, only undertaken if phase1 does not reach the shared end goal in the real environment. Tracked separately.

## References

- Design doc: `tmp/mux-perf-fix-1-client-coalesce.md`
- Requirements: `doc/tasks/mux-client-coalesce/要件定義書.md`
- Existing E2E: `src-tauri/tests/mux_throughput.rs`
- Related (prior) feature: `doc/tasks/mux-output-throughput/`
