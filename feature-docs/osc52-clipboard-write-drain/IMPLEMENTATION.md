# Implementation Plan: osc52-clipboard-write-drain

## Overview

OSC 52 writes and clears that `NativeCallbacks::handle_clipboard` already queues are drained once per pump on every tab, collected by `App::pump_all`, and applied to the system clipboard on the UI thread through `WindowHost`; the OSC 52 read queue is drained and discarded. Snapshot replay content never reaches the clipboard.

## Technology Stack

- **Language**: Rust (the `src-tauri` crate, `gui` feature)
- **Key libraries**: arboard - system clipboard access (already a dependency, used by `WindowHost`)
- **New dependencies**: none (no license check required)

## Layer Structure

| Layer | Responsibility in this feature | May depend on |
|-------|--------------------------------|---------------|
| Parse (term_core + `NativeCallbacks`) | Accept or reject OSC 52 (existing gates, FR9) and queue `(target, text)` writes / `target` reads. Unchanged. | - |
| Tab (`src-tauri/src/tabs/`) | Owns the per-tab queues through `NativeCallbackState`; exposes the per-pump drain; keeps snapshot replay out of the queues | Parse |
| App (`src-tauri/src/app/`) | Collects every tab's drained writes into one per-pump batch | Tab |
| WindowHost (`src-tauri/src/window_host/`, UI thread) | Maps targets to destinations, applies the batch to the single arboard instance, logs failures | App |

Dependency direction is top-down only: WindowHost reads from App, App reads from Tab, Tab reads from the callback state. arboard is touched only in the WindowHost layer (NFR2). All touched modules are already behind the `gui` feature, so no new `lib.rs` entry is needed (NFR4).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| OSC 52 clipboard queues (`NativeCallbackState.pending_clipboard_writes` / `pending_clipboard_reads`) | Hold accepted OSC 52 requests between a parse and the pump's drain | Producer: only `NativeCallbacks::handle_clipboard` appends, unchanged (write entry = `(target, text)`, empty text = clear; read entry = `target`). Per pump: after `App::pump_all` returns, both queues are empty on every tab. Replay rule: a synchronous snapshot replay leaves both queues exactly as they were before it (same entries, same order). No other code reads, appends to or removes from these queues. | task0001, task0002 |
| Synchronous snapshot replay recipe (`Tab::reset_frame_for_replay`) | The single recipe every synchronous snapshot application uses (sync Snapshot / SnapshotRestore arm, live-queue overflow fallback, worker-disconnect fallback, worker spawn-failure fallback, Detached / PaneCreated blank resets) | Pre: none. Post: completes before control returns to `Tab::pump` / `Tab::poll_pending_switch`, so it always finishes before the pump's drain point (D1). Live output applied after the recipe (queued live output after a fallback reparse) is live output, not snapshot content. | task0001, task0002 |

## Conventions

- **Logging (NFR3)**: lines this feature adds or changes carry only the OSC 52 target or destination, a text length and an error description. They never carry the decoded text or the base64 payload. Failures use `warn` (release logs persist `warn` and above). The existing `LOG_OSC52_DENIED` lines stay as they are.
- **Feature gating (NFR4)**: new code lives in the already `gui`-gated `window_host`, `app` and `tabs` modules.
- **Tests**: Rust unit tests in the `--lib` target. Off-thread replay tests use the existing blocked-ready helpers (`test_block_worker_ready`, `test_force_worker_disconnect`) and never reproduce timing by loading the CPU.
- **Comments**: reference the SPEC requirement IDs (`FR1` ... `NFR6`) the code implements.

## Cross-task Design Decisions

### D1: The drain point is `App::pump_all`, per tab, at the viewer-OSC drain position

Each tab's clipboard queues are drained inside the `App::pump_all` per-tab loop at the same position as the existing `drain_osc` call - after `tab.pump()`, `tab.poll_pending_switch()` and `tab.poll_pending_scrollback_restore()`.

- Rationale: FR5. Every parse path that can reach a tab's `NativeCallbacks` in one pump (pre-mux outer parse, mux frame-apply loop, post-Detached tail re-route, live output applied on an off-thread swap or fallback) has run by then. The early `cb_state` block in `Tab::process_combined` runs before the mux frame-apply loop and is not a valid drain point.
- Affected tasks: task0001 implements it. task0002 relies on it: its replay guard runs inside the synchronous replay recipe, which always finishes before this point.

### D2: Snapshot replay is kept out of the clipboard on the GUI side, inside the synchronous replay recipe

The guard for FR7 lives in `Tab::reset_frame_for_replay`, not in the daemon's snapshot strip.

- Rationale: the daemon strip (`src-tauri/src/mux/scrollback_filter.rs`, `strip_replayable_rich_content`) does not remove OSC 52 today, so snapshot content can carry historic OSC 52. A daemon-side change would not cover an older or remote daemon. The off-thread replay worker builds its core without callbacks; `apply_offthread_swap` transplants the callbacks only after the build, so off-thread snapshot content cannot reach the queues.
- Affected tasks: task0002 implements and tests it. task0001 adds no drain or apply inside any replay path and does not read the queues anywhere except at the D1 drain point.

### D3: Output applied after a replay is live output and is applied

Queued live output replayed after an off-thread swap, and the live queue applied after a fallback reparse, are ordinary live output: their OSC 52 entries stay queued and reach the clipboard in the same pump (FR5).

- Affected tasks: task0001 (collects them at the D1 drain point), task0002 (its guard removes only entries the replay itself added).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A burst of OSC 52 writes in one pump makes many OS clipboard calls on the UI thread | Medium | Medium (UI stall) | At most one clipboard operation per destination per pump (THREAT-MODEL.md TM-4) |
| arboard is unavailable or a set / clear fails on X11 / Wayland | Low | Low | Queues still drain every pump; failures are logged without text (FR8, TM-3) |
| The replay guard removes a live entry queued earlier in the same pump | Low | Medium (lost copy) | The guard removes only entries appended during the replay's own parse (Shared Components, replay rule) |
| A program allowed to print to the terminal can set the clipboard (paste-jacking) | Medium | Medium | Inherent to OSC 52 write support and accepted as the feature's purpose. Existing acceptance gates stay in front of the queue (TM-1). |
| The parallel tasks both add helpers to `src-tauri/src/tabs/tests.rs` | Low | Low | Merge conflicts are resolved by the parent-side adoption protocol |

## Open Questions

- [ ] NFR2 (UI-thread-only arboard access) has no automated test. It is checked by code review of the single call site in `about_to_wait`.
- [ ] The Windows build is not covered by the workflow's automated commands. The PRIMARY-to-CLIPBOARD substitution is unit-tested through the platform parameter (TS-1). The Windows cross-build is a user-run manual item.
