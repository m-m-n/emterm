# Implementation Plan: Background OSC Notification Detection

## Overview
Detect `OSC 9 ; <message>` desktop-notification sequences on the background PTY processing paths (hidden window and mux detached panes) and fire an OS desktop notification from the GUI process, without changing foreground behavior.

## Objectives
- Recognize OSC 9 notifications on the hidden / detached scan paths, separated from replayable passthrough bytes.
- Deliver detected notifications to the single GUI notification sink (`sendNotification`).
- Guarantee non-active regular-tab notifications and avoid duplicate firing on resume/reattach.

## Prerequisites

### Development Environment
- Rust toolchain (workspace `src-tauri/`), Bun, WASM build chain (unchanged).
- Docker for tests (project policy).

### Dependencies
- `@tauri-apps/plugin-notification` (already present) — OS notification + permission check.
- Existing visibility-aware streaming (`process_hidden`, `PassthroughScanner`, reader visibility gate).
- Existing mux cross-client message forwarding (daemon → GUI client).

## Architecture Overview

### Technology Stack
- **Backend**: Rust (`src-tauri/`), including the mux daemon (separate process).
- **Frontend**: TypeScript (`src/`).

### Design Approach
Both background paths already run a per-session/per-pane scan of raw PTY bytes while not streaming live to the GUI (`process_hidden` for the in-process backend; the mux daemon `pty_reader_loop` for panes). The plan adds OSC 9 notification recognition to that scan, routed as a **side-effect message** kept strictly separate from the replayable passthrough buffer (so it is fired once and never replayed). The OS notification is always raised in the GUI process via the existing frontend `sendNotification`, reached by:
- in-process backend → a Tauri event toward the frontend (mirrors the existing `pty_exit` event), and
- mux daemon → a new control message forwarded to the GUI client (mirrors the existing daemon-detected `RenameWindow` title path).

### Component Interaction
1. PTY bytes on a background path are scanned.
2. A recognized `OSC 9 ; <message>` (non-progress) yields a notification message.
3. The message reaches the GUI frontend (Tauri event or mux control message).
4. The frontend calls `sendNotification("eMterm", message)` after the permission check.
Foreground (visible/active) bytes continue through the unchanged WASM → `osc-handler.ts` path.

## Implementation Phases

### Phase 1: Shared OSC 9 notification recognition (backend scan)

**Goal**: The background scan can recognize `OSC 9 ; <message>` notifications and surface them separately from replayable passthrough bytes.

**Files to Modify**:
- `src-tauri/src/pty/passthrough_scanner.rs` — recognize OSC 9 notification sequences in addition to the existing image/Markdown passthrough sequences, returning notifications as a distinct output from the replay bytes.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Background scanner | Recognize `OSC 9 ; <message>`; exclude `9;4;` progress; keep notifications out of the replay output | Receives raw PTY byte chunks (possibly split) | Reports notification message(s); replay-passthrough output unchanged for image/Markdown |

**Processing Flow** (diagram-convertible):
1. Scan bytes for an OSC introducer and collect an OSC body until BEL or ST.
2. On a completed OSC body:
   - Param prefix is `9999;` → existing replayable passthrough (unchanged).
   - Param prefix is `9;` and body-after-`9;` begins with `4;` → progress → ignore for notification.
   - Param prefix is `9;` otherwise → emit a notification message = body after `9;`.
3. A partial sequence exceeding `PARTIAL_SEQUENCE_MAX` is dropped (single warn), consistent with current behavior.

**Implementation Steps**:
1. **Extend OSC recognition** — within the existing OSC collection state, branch on `9;` vs `9999;`.
2. **Separate output contract** — return recognized notification messages distinctly from the replayable passthrough byte output so callers route them to different sinks.
3. **Progress exclusion** — treat `9;4;…` as non-notification.
4. **Preserve existing behavior** — image/SIXEL/Markdown extraction and overflow handling unchanged.

**Dependencies**: Blocks Phase 2 and Phase 3.

**Testing Approach**:
- Unit: BEL/ST termination, chunk-split recovery, progress exclusion, non-OSC-9 ignored, overflow drop, existing passthrough still extracted.

**Acceptance Criteria**:
- [ ] `OSC 9 ; msg` (BEL and ST) recognized as a notification.
- [ ] `OSC 9 ; 4 ; …` not recognized as a notification.
- [ ] Notification output is separate from replay-passthrough output.
- [ ] Existing image/Markdown extraction tests still pass.

**Estimated Effort**: medium

---

### Phase 2: Hidden-window notification delivery (in-process, non-mux)

**Goal**: A notification recognized while the window is hidden fires an OS desktop notification once.

**Files to Modify**:
- `src-tauri/src/pty/visibility.rs` — `process_hidden` surfaces recognized notification messages to its caller (kept out of the passthrough buffer).
- `src-tauri/src/reader.rs` — when hidden processing surfaces a notification, emit a Tauri event toward the frontend (alongside the existing event-emitting pattern used for `pty_exit`).
- `src/terminal-app/` (frontend listener) — subscribe to the notification event and call the existing notification sink.
- `src/terminal/osc-notification.ts` — reused as the sink (`sendNotification`); no behavioral change.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `process_hidden` | Run the notification recognition while hidden; surface messages | Session hidden; receives byte batch | Notification messages returned/surfaced; passthrough buffer unchanged by them |
| reader hidden branch | Forward surfaced messages to the frontend as an event | Hidden batch processed | Frontend receives a notification event carrying the message |
| frontend listener | Fire the OS notification | Notification event received; permission granted | OS desktop notification shown once |

**Processing Flow**:
1. Reader reads a batch; session is hidden.
2. Hidden processing scans the batch (Phase 1) → notification message(s).
3. Reader emits a notification event toward the frontend for each message.
4. Frontend listener calls the sink → OS notification (permission permitting).

**Implementation Steps**:
1. **Surface messages from `process_hidden`** without adding them to the replay buffer.
2. **Emit a frontend-bound event** from the reader’s hidden branch using the existing app event mechanism.
3. **Add a frontend listener** that maps the event to the existing notification sink.
4. **Confirm no double-fire** on resume (messages are not part of the snapshot/replay stream).

**Dependencies**: Requires Phase 1.

**Testing Approach**:
- Unit (Rust): hidden processing surfaces a notification for `OSC 9 ; msg`; none for progress; passthrough buffer unaffected.
- Unit (TS): listener maps event → sink; permission denied → no notification.
- Manual: minimize window, emit sequence, observe single notification; restore window, confirm no second notification.

**Acceptance Criteria**:
- [ ] Notification fires while window hidden.
- [ ] No duplicate on window restore.
- [ ] Foreground path unchanged.

**Estimated Effort**: medium

---

### Phase 3: mux detached-pane notification delivery

**Goal**: A notification recognized in a detached (non-active) pane fires an OS desktop notification via the GUI client.

**Files to Modify**:
- `src-tauri/src/mux/ipc/pty_spawn.rs` — the pane reader loop recognizes OSC 9 notifications (Phase 1) and forwards them **only when the pane output target is `Detached`** (active/`Connected` panes are handled by the GUI foreground WASM path, so forwarding them would double-fire).
- `src-tauri/src/mux/ipc/protocol.rs` — add a notification control message type (mirrors existing daemon-originated control messages such as window rename).
- `src-tauri/src/mux/ipc/connection.rs` / `handlers.rs` — forward the notification control message to the connected GUI client (reuse the cross-client notification forwarding path).
- `src/terminal/mux/mux-client.ts` — handle the new message type in the incoming-message dispatch and call the existing notification sink.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| pane reader loop | Recognize OSC 9 notifications; forward only for `Detached` output | Pane producing output | Notification forwarded only when output is NOT streamed to a connected GUI client |
| daemon forward path | Send a notification control message to the GUI client | Notification recognized | GUI client receives the message |
| `mux-client` dispatch | Map the message to the notification sink | Message received | OS notification shown once (permission permitting) |

**Processing Flow**:
1. Pane reader loop processes pane output.
2. Recognized `OSC 9 ; msg` on a `Detached` pane → notification control message to the GUI client. `Connected`/active panes are NOT forwarded (the GUI foreground WASM path already fires them) — prevents double-fire.
3. `mux-client` dispatch maps it to the sink → OS notification.

**Implementation Steps**:
1. **Recognize OSC 9 in the pane reader loop** using the Phase 1 recognition.
2. **Add a notification control message type** to the mux protocol.
3. **Forward to the GUI client for `Detached` output only** via the existing daemon→GUI notification path; skip `Connected`/active panes to avoid double-firing.
4. **Handle in `mux-client`** dispatch → notification sink.
5. **Confirm no double-fire** on reattach (notification is fire-and-forget, not part of reattach replay data).

**Dependencies**: Requires Phase 1.

**Testing Approach**:
- Unit (Rust): detached pane output with `OSC 9 ; msg` produces a forwarded notification message; progress excluded.
- Unit (TS): `mux-client` notification case → sink; permission denied → no notification.
- Manual: mux with a non-active pane/window emitting the sequence → single notification; reattach → no second notification.

**Acceptance Criteria**:
- [ ] Notification fires for a detached-pane sequence.
- [ ] No double-fire for the active/`Connected` pane (only `Detached` output forwards a notification).
- [ ] No duplicate on reattach.
- [ ] mux protocol change is backward-compatible (unknown type tolerated by peers).

**Estimated Effort**: medium

---

### Phase 4: Non-active regular tab verification (FR3)

**Goal**: Confirm that a non-active regular tab (mux off, window visible) already fires OSC 9 notifications, and lock it with a regression test.

**Files to Modify**:
- (Expected none.) Add a regression test only. If verification shows it does not fire, add minimal wiring and record it here.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| per-tab PTY → WASM path | Parse OSC 9 for a non-active tab while window visible | Window visible; tab not active | Notification fires through the existing foreground sink |

**Processing Flow**:
1. Non-active tab receives PTY bytes while the window is visible.
2. Existing per-tab WASM parse fires the OSC 9 notification.

**Implementation Steps**:
1. **Add a regression test** asserting the OSC 9 notification path fires for a non-active tab.
2. **If it does not fire**, identify the gating and add minimal wiring (update this phase).

**Outcome (verified)**: No wiring was needed. The per-window `VisibilityController`
marks the backend hidden only when the whole window is hidden/minimized; it is
NOT driven by tab switching. A non-active tab in a visible window therefore keeps
streaming its PTY bytes through the normal reader → WASM → `handleOscCallback`
path. `handleOscCallback` (src/terminal-app/osc-handler.ts, case 9) has no
active-tab parameter and no active-tab gate, so it fires `sendNotification` for
OSC 9 regardless of which tab is displayed. Locked by a regression test
(`src/terminal-app/osc-handler-notification.test.ts`: TS-13 fires, SC-4 progress
excluded, FR6 permission gate). No production code change in Phase 4.

**Dependencies**: Independent of Phases 1–3.

**Testing Approach**:
- Unit/Integration (TS): non-active tab OSC 9 triggers the notification callback.
- Manual: two tabs (mux off), emit from the non-active tab → notification.

**Acceptance Criteria**:
- [ ] Non-active regular tab fires the notification (verified by test).

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
├── pty/
│   ├── passthrough_scanner.rs   # MODIFY: recognize OSC 9 notifications (separate output)
│   └── visibility.rs            # MODIFY: process_hidden surfaces notification messages
├── reader.rs                    # MODIFY: emit notification event to frontend (hidden path)
└── mux/ipc/
    ├── pty_spawn.rs             # MODIFY: pane reader loop recognizes OSC 9 notifications
    ├── protocol.rs              # MODIFY: add notification control message type
    ├── connection.rs            # MODIFY: forward notification to GUI client
    └── handlers.rs              # MODIFY (if needed): wire forwarding

src/
├── terminal/
│   ├── osc-notification.ts      # REUSE: sendNotification sink (unchanged behavior)
│   └── mux/mux-client.ts        # MODIFY: handle notification message → sink
└── terminal-app/                # MODIFY: listen to in-process notification event → sink
```

## Testing Strategy
- Unit (Rust): scanner recognition, progress exclusion, termination/chunking, overflow, hidden-path surfacing, mux detached forwarding.
- Unit (TS): event listener and mux-client dispatch map to the sink; permission gating.
- Integration/Manual: the three in-scope situations and the no-double-fire guarantees (OS notification firing is verified manually; headless E2E lacks a notification daemon).
- Regression: foreground OSC 9 notification/progress unchanged.

## Dependencies
| Package | Version | Purpose |
|---------|---------|---------|
| @tauri-apps/plugin-notification | existing (^2.3.3) | OS notification + permission |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Double-fire on resume/reattach | Medium | Medium | Keep notifications out of replay/snapshot data; fire-and-forget only |
| Sequence straddling visible↔hidden transition | Medium | Low | Each byte handled by exactly one path; straddling single sequence is best-effort (documented) |
| mux protocol incompatibility | Low | Medium | New message type; peers tolerate unknown types (`mux-client` has a default ignore branch) |
| Active mux pane double-fire | Medium | High | Daemon forwards notifications only for `Detached` output; `Connected`/active panes fire via the GUI foreground path only |
| Persistent daemon version skew | Low | Low | A long-lived pre-feature mux daemon will not detect OSC 9 until restarted; feature activates after the daemon is relaunched |
| Notification flooding from chatty programs | Low | Low | Matches existing foreground behavior (no new throttle); out of scope per requirements |

## Open Questions
- [ ] Phase 1 shape: extend `PassthroughScanner` to emit notifications as a separate output, vs a dedicated sibling scanner on the same path. Decision deferred to verify-plan; both satisfy FR5 (separation from replay).

## Success Metrics
- [ ] All FRs implemented; background notifications fire for the three situations.
- [ ] No duplicate firing; foreground unchanged.
- [ ] Rust + TS tests and TS typecheck green.
