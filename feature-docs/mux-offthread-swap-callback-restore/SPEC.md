# Feature: mux-offthread-swap-callback-restore

## Overview

When a mux window switch delivers a snapshot of 64 KB or more
(`OFFTHREAD_REPLAY_THRESHOLD_BYTES`, `src-tauri/src/tabs.rs:44`), the snapshot
is replayed off-thread and `apply_offthread_swap` (`tabs.rs:853`) replaces the
tab's live `TerminalCore` with the worker-built core. The worker core is
intentionally built with no callbacks and no app-layer OSC registrations (to
be `Send`), and the swap installs it verbatim — the old core's `callbacks`
(installed at `tabs.rs:468`) and the OSC 9999 registration
(`register_osc_app_param(MUX_OSC_PARAM, OSC_MUX_INBAND)`, `tabs.rs:450-453`)
are silently discarded. This feature restores that wiring at swap time.

Consequences fixed: (1) after detach, re-attach hangs because the pre-mux
Welcome frame is never parsed, so the GUI never sends `Attach`; (2)
callback-driven inner content (viewer OSC 777, Kitty images, title, bell,
theme OSCs) is ignored after a swap (intermittent viewer-launch-loss).

Root-cause investigation: `tmp/discussion-mux-detach-attach-failure.md`.

## Objectives

- Preserve the tab's `TerminalCore` callbacks across the off-thread core swap.
- Preserve the app-layer OSC 9999 (`MUX_OSC_PARAM` → `OSC_MUX_INBAND`)
  registration across the swap.
- Restore detach → attach and in-mux rich content behavior for tabs that have
  gone through an off-thread replay.

## Technical Requirements

### Functional Requirements

- **FR1:** `apply_offthread_swap` MUST transplant the old core's `callbacks`
  (`Option<Box<dyn TerminalCallbacks>>`, public field
  `crates/term_core/src/terminal_core.rs:327`) onto the swapped-in core. The
  transplant happens on the main thread at swap time; the worker-side
  `build_from_snapshot` contract (core built with `callbacks: None`) is
  unchanged.
- **FR2:** After the swap, the live core MUST have the app-layer OSC
  registration `MUX_OSC_PARAM` → `OSC_MUX_INBAND` in effect. Acceptable
  implementations: re-register via
  `register_osc_app_param(mux_ipc::protocol::MUX_OSC_PARAM,
  crate::callbacks::OSC_MUX_INBAND)`, or transplant the old core's whole
  registration map if the implementation exposes it. The registration must
  behaviorally match a freshly constructed tab core.
- **FR3:** After an off-thread swap, a pre-mux mux transport Welcome arriving
  on the outer stream (both the OSC 9999 form and the APC form emitted by the
  bridge) MUST reach `Tab::apply_mux_message` via the
  `process_outer_via_core` path, exactly as it does on a tab that never
  swapped.
- **FR4:** After an off-thread swap, callback-driven processing (e.g.
  `TerminalCallbacks` invocations for title/bell/OSC handlers) MUST fire for
  subsequent PTY output, exactly as before the swap.

### Non-Functional Requirements

- **NFR1 - Synchronous path unchanged:** The synchronous replay path
  (`reset_and_replay`, snapshots < 64 KB) and the FR7 worker-panic fallback
  (`reset_frame_for_replay`) already preserve callbacks and MUST NOT change
  behavior.
- **NFR2 - CLI build:** The CLI-only build (`--no-default-features`) still
  compiles.

## Implementation Approach

### Change Site

`Tab::apply_offthread_swap`, `src-tauri/src/tabs.rs:853` — step 1 of the swap
currently does:

```
*self.core.lock() = replay.core;
```

The fix wires the old core's state across the assignment, in the spirit of:

```
let mut core = self.core.lock();
let callbacks = core.callbacks.take();          // save from old core
*core = replay.core;                            // swap
core.callbacks = callbacks;                     // transplant
core.register_osc_app_param(                    // restore OSC 9999
    mux_ipc::protocol::MUX_OSC_PARAM,
    crate::callbacks::OSC_MUX_INBAND,
);
```

(Exact code is up to the implementation; transplanting the registration map
instead of re-registering is equally acceptable per FR2.)

### Invariants to verify during implementation

- The 2nd-pass scrollback restore (`spawn_scrollback_restore` →
  `apply_scrollback_restore`) merges into the live core rather than swapping
  it; confirm it cannot reintroduce the loss. If it also replaces the core,
  apply the same transplant there.
- The stale comment at `tabs.rs:448-449` ("off-thread snapshot replay cores
  process inner content, which carries no mux transport frames") describes the
  broken assumption; update it to reflect that swapped-in cores become the
  live core and get the wiring transplanted.

### Dependencies

**Internal:**
- `crates/term_core` (`TerminalCore.callbacks`, `register_osc_app_param`) —
  read-only usage; extend only if the registration-map transplant variant
  needs an accessor.
- `src-tauri/src/tabs.rs` — the change site and its unit tests.

**External:** none.

### File Structure

```
src-tauri/src/tabs.rs                     # fix in apply_offthread_swap + tests
crates/term_core/src/terminal_core.rs     # only if a registration accessor is needed
```

## Test Scenarios

### Unit Tests

- [ ] TS-1: After `apply_offthread_swap`, the live core's `callbacks` is
  `Some` and is the pre-swap callbacks instance (observable via a recording
  `TerminalCallbacks` test double).
- [ ] TS-2: After `apply_offthread_swap`, feeding an OSC 9999
  (`MUX_OSC_PARAM`) sequence to the live core triggers the registered
  app-param action (`OSC_MUX_INBAND`), matching a never-swapped tab.
- [ ] TS-3: After an off-thread swap, a pre-mux Welcome frame in OSC 9999 form
  fed through the outer-stream path reaches `apply_mux_message` (attach
  bootstrap works).
- [ ] TS-4: After an off-thread swap, a pre-mux Welcome frame in APC form is
  also processed (bridge emits both forms).
- [ ] TS-5: After an off-thread swap, a callback-driven OSC (e.g. title
  change) on subsequent output invokes the transplanted callbacks.
- [ ] TS-6: Regression — synchronous replay (< 64 KB) keeps callbacks without
  any new code path (guards NFR1).
- [ ] TS-7: Regression — existing off-thread replay tests (mark backfill,
  2nd-pass restore, supersede) still pass.

### Manual Tests (user-performed, post-merge)

- [ ] MT-1: Real-machine repro — window with ≥ 64 KB snapshot: switch →
  detach → attach succeeds (no hang).
- [ ] MT-2: In-mux `emterm markdown <file>` launches the viewer after having
  displayed a ≥ 64 KB-snapshot window (viewer-launch-loss resolution).

### E2E Tests

**Existing E2E tests**: None (no e2e suite in this repo).
**Run command**: Not detected.

### Edge Cases

- [ ] Swap while the tab is in mux mode (extractor path active): mux
  operation continues, and the transplanted wiring serves the post-detach
  pre-mux phase.
- [ ] Old core with `callbacks: None` (should not occur for live tabs, but
  the transplant must not panic): swapped core simply has `None`.

## Error Handling

No new error paths: the transplant is infallible field movement plus an
in-memory registration. No new logging is required by this spec.

## Success Criteria

- [ ] All functional requirements are implemented and tested (TS-1 … TS-7).
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
  src-tauri/Cargo.toml --lib -- --test-threads=1` passes.
- [ ] `--no-default-features` build compiles (NFR2).
- [ ] MT-1 / MT-2 are handed to the user for later real-machine verification.

## Open Questions

None.

## References

- Root-cause report: `tmp/discussion-mux-detach-attach-failure.md`
- Related memory: project_mux_offthread_swap_callback_loss,
  project_mux_viewer_launch_loss_probe
