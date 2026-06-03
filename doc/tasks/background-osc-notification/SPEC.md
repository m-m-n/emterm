# Feature: Background OSC Notification Detection

## Overview

OSC 9 desktop notification sequences (`OSC 9 ; <message>`) are currently parsed only inside the frontend WASM parser (`process_pty_data`), so notifications emitted from a backgrounded tab/pane or a minimized window are never delivered. This feature scans the background processing paths for `OSC 9 ; <message>` and fires an OS desktop notification regardless of tab/window visibility.

## Objectives

- Detect `OSC 9 ; <message>` notifications emitted while the window is hidden/minimized.
- Detect `OSC 9 ; <message>` notifications emitted by mux detached (non-active) panes/windows.
- Ensure non-active regular tabs (mux off, window visible) deliver notifications.
- Avoid duplicate notifications when the window is re-shown or a pane is reattached.

## User Stories

### US1: Notification while window minimized
As a terminal user, I want OSC 9 notifications emitted while my window is minimized to surface as OS desktop notifications, so that I do not miss task completion.

**Acceptance Criteria:**
- [ ] An `OSC 9 ; msg` emitted while the window is hidden fires an OS desktop notification (title `eMterm`, body `msg`).
- [ ] Re-showing the window does not re-fire the same notification.

### US2: Notification from a mux detached pane
As a mux user, I want OSC 9 notifications from a non-active pane/window to surface, so that background panes are not silent.

**Acceptance Criteria:**
- [ ] An `OSC 9 ; msg` emitted by a Detached pane fires an OS desktop notification via the GUI client.
- [ ] Reattaching the pane does not re-fire the same notification.

### US3: Notification from a non-active regular tab
As a multi-tab user (mux off), I want OSC 9 notifications from a non-active tab to surface while the window is visible.

**Acceptance Criteria:**
- [ ] An `OSC 9 ; msg` emitted by a non-active tab fires an OS desktop notification.
- [ ] Foreground tab behavior is unchanged.

## Technical Requirements

### Functional Requirements

- **FR1 - Hidden-window OSC 9 detection:** While the session is hidden (`reader.rs` routes output to `process_hidden()`), scan the byte stream for `OSC 9 ; <message>` notification sequences and fire an OS desktop notification for each. Reuse a stateful scanner (modeled on `PassthroughScanner`) that handles chunk-split sequences and both BEL (`0x07`) and ST (`ESC \`) terminators.
- **FR2 - mux detached-pane OSC 9 detection:** In the mux daemon, scan Detached pane output for `OSC 9 ; <message>` notifications and forward a notification request to the connected GUI client over the existing cross-client notification path. The GUI fires the OS notification.
- **FR3 - Non-active regular tab notification:** Guarantee (and regression-verify) that an `OSC 9 ; <message>` emitted by a non-active tab while the window is visible fires an OS desktop notification.
- **FR4 - Progress sequences excluded:** A scanned OSC 9 whose body begins with `4;` is a progress-bar sequence and MUST NOT fire a background notification. Only plain `OSC 9 ; <message>` fires.
- **FR5 - No duplicate firing on resume/reattach:** A notification fired while hidden/detached MUST NOT fire again on window re-show or pane reattach. OSC 9 notification bytes are side-effect events and MUST NOT be added to the resume/replay passthrough byte stream.
- **FR6 - Notification content & permission:** Background notifications use the existing foreground content (title `eMterm`, body = OSC 9 message) and are fired only when notification permission is granted.
- **FR7 - Termination & chunking:** The scanner correctly extracts OSC 9 terminated by BEL or ST and recovers sequences split across PTY read chunks.

### Non-Functional Requirements

- **NFR1 - Performance:** Background OSC 9 scanning adds per-byte overhead comparable to the existing `PassthroughScanner`, and does not affect the foreground (visible/active) hot path.
- **NFR2 - Bounded memory:** Partial OSC 9 sequence buffering is bounded by the existing `PARTIAL_SEQUENCE_MAX` overflow guard (drop + single warn).
- **NFR3 - Cross-process constraint:** The OS desktop notification is sent only from the GUI (Tauri) process. The mux daemon forwards a notification request to the GUI rather than sending it directly.
- **NFR4 - Platform:** Linux and Windows only (no macOS).
- **NFR5 - Foreground unchanged:** Existing foreground OSC 9 notification/progress behavior (WASM → `osc-handler.ts`) is unchanged; no double notifications when a tab/window is active/visible.

## Implementation Approach

### Architecture

Two background processing paths must learn to detect OSC 9 notifications; both converge on a single notification sink in the GUI frontend (`sendNotification`), which already performs the permission check.

```
┌───────────────────────────── GUI process (Tauri) ─────────────────────────────┐
│  Frontend (TS)                                                                 │
│    sendNotification("eMterm", msg)  ← single OS-notification sink              │
│        ▲                                   ▲                                   │
│        │ Tauri event {msg}                 │ mux IPC notification {msg}         │
│  ┌─────┴───────────┐               ┌───────┴────────────┐                      │
│  │ in-process PTY  │               │ MuxClient (TS)     │                      │
│  │ backend (Rust)  │               └───────▲────────────┘                      │
│  │  reader.rs      │                       │ forward                           │
│  │  process_hidden │                       │                                   │
│  │   + OSC9 scan   │               ┌───────┴────────────┐                      │
│  └─────────────────┘               │  mux daemon (Rust) │  (separate process)  │
│   (hidden window)                  │  detached pane scan│                      │
│                                    │   + OSC9 scan      │                      │
│                                    └────────────────────┘                      │
└────────────────────────────────────────────────────────────────────────────────┘
```

### Data Flow

- **Hidden window (non-mux):** `reader.rs` (`!is_visible`) → `process_hidden()` → OSC 9 scan → on `OSC 9 ; msg` (non-progress) emit a Tauri event to the frontend → `sendNotification`.
- **mux detached pane:** daemon pane loop (Detached) → OSC 9 scan → cross-client notification request to GUI client → `MuxClient` (TS) → `sendNotification`.
- **Non-active regular tab (window visible):** unchanged per-tab path: PTY → channel → `process_pty_data` (WASM) → `osc-handler.ts` → `sendNotification`.

### Existing components to change/extend

- `src-tauri/src/pty/passthrough_scanner.rs` — OSC 9 notification recognition (a separate scanner or an extension that emits notification messages, distinct from the replayable passthrough output so FR5 holds).
- `src-tauri/src/pty/visibility.rs` (`process_hidden`) — invoke OSC 9 scan and surface detected messages.
- `src-tauri/src/pty/reader.rs` — wire detected messages to a Tauri event toward the frontend.
- `src-tauri/src/mux/` (pane output / ipc) — scan Detached pane output; add/extend a notification request message and forward it via the cross-client notification path (`manager.rs` broadcast, `connection.rs` forward-to-GUI).
- `src/terminal/mux/mux-client.ts` — handle the new notification request and call `sendNotification`.
- Frontend listener for the in-process Tauri notification event → `sendNotification`.
- `src/terminal/osc-notification.ts` / `src/terminal-app/osc-handler.ts` — reused as the notification sink (`sendNotification`); foreground behavior unchanged.

### OSC 9 recognition rule (shared)

Given an extracted OSC 9 body (the bytes between `ESC ] 9 ;` and the terminator):
- If the body begins with `4;` → progress → ignore (FR4).
- Otherwise → notification; message = body. Fire once (FR1/FR2).

### Dependencies

**Internal:**
- visibility-aware-pty-streaming (`process_hidden`, `PassthroughScanner`, `reader.rs` visibility gate).
- mux cross-client notification path (`manager.rs`, `connection.rs`).
- Existing OSC 9 frontend handling (`osc-notification.ts`, `osc-handler.ts`).

**External:**
- `@tauri-apps/plugin-notification` (already a dependency) — OS notification + permission check.

### File Structure (anticipated touch points)

```
src-tauri/src/pty/
├── passthrough_scanner.rs   # OSC 9 notification recognition
├── visibility.rs            # process_hidden: run OSC 9 scan
└── reader.rs                # emit notification event to frontend
src-tauri/src/mux/
├── ...                      # detached pane OSC 9 scan
└── ipc/                     # notification request message + GUI forward
src/terminal/mux/
└── mux-client.ts            # receive notification request → sendNotification
src/terminal-app/
└── ...                      # listen in-process notification event → sendNotification
```

## Test Scenarios

### Unit Tests (Rust)
- [ ] Scanner extracts `OSC 9 ; msg` (BEL-terminated) and reports it as a notification.
- [ ] Scanner extracts `OSC 9 ; msg` (ST-terminated).
- [ ] Scanner treats `OSC 9 ; 4 ; 1 ; 50` (progress) as NON-notification (no fire).
- [ ] Scanner recovers an OSC 9 split across chunk boundaries.
- [ ] Scanner drops a never-terminating sequence past `PARTIAL_SEQUENCE_MAX` (single warn).
- [ ] Other OSC (e.g. `OSC 0 ; title`) is not treated as a notification.
- [ ] mux: Detached pane output with `OSC 9 ; msg` produces a forwarded notification request.

### Unit Tests (TypeScript)
- [ ] Frontend in-process notification event handler calls `sendNotification("eMterm", msg)`.
- [ ] `MuxClient` notification request handler calls `sendNotification("eMterm", msg)`.
- [ ] Permission denied → no notification sent.

### Integration / Manual
- [ ] Window minimized: `printf '\033]9;done\007'` fires an OS notification; re-show does not re-fire.
- [ ] mux non-active pane: same sequence fires via GUI; reattach does not re-fire.
- [ ] Non-active regular tab (mux off): same sequence fires.
- [ ] Foreground (active/visible) OSC 9 notification and progress behave as before.

### Edge Cases
- [ ] OSC 9 sequence straddling the visible↔hidden transition boundary: handled by exactly one path; a single sequence crossing the exact transition is best-effort.
- [ ] Empty message (`OSC 9 ; ` then terminator): no crash (matches foreground parse).

## Security Considerations

- **Permission:** Notifications fire only when `isPermissionGranted()` is true.
- **Content:** Body is the raw OSC 9 message, identical to the existing foreground path; no new injection surface is introduced beyond what foreground already exposes.

## Success Criteria

- [ ] All functional requirements implemented and unit/integration tested.
- [ ] Background notifications fire for all three in-scope situations.
- [ ] Progress (`9;4`) does not fire background notifications.
- [ ] No duplicate firing on resume/reattach.
- [ ] Foreground behavior unchanged; Rust + TypeScript tests and typecheck green.

## Open Questions

> 未解決の要件は sdd.yaml で `status: tbd` として管理します。現時点で未解決要件はありません。

- None.

## References

- `src/terminal/osc-notification.ts`, `src/terminal-app/osc-handler.ts`
- `src-tauri/src/pty/passthrough_scanner.rs`, `src-tauri/src/pty/visibility.rs`, `src-tauri/src/pty/reader.rs`
- `doc/tasks/notification-activity-monitor/`, `doc/tasks/mux-scrollback-retention/`
