# Feature: mux move-window

## Overview

Add a `move-window` action to mux mode that reorders the active window to a user-specified 1-origin position. Triggered by `prefix + m`, it opens a modal number input and applies insert/move semantics (equivalent to tmux `move-window -t N`). Tab labels render each mux window with a leading `[N]` number badge.

## Objectives

- Provide a keyboard-driven way to reorder mux windows inside a session.
- Display 1-origin window numbers in tab labels for visibility.
- Keep the implementation aligned with existing mux action patterns (rename-window).

## User Stories

### US1: Reorder the active window

As a mux user, I want to move the active window to a specific numbered position, so that I can organize my windows without detaching or recreating them.

**Acceptance Criteria:**
- [ ] `prefix + m` opens a modal number input.
- [ ] Entering a valid 1-origin number and confirming moves the active window using insert/move semantics.
- [ ] Invalid input closes the dialog without changes to window order:
  - Out of range / non-numeric / empty: dialog itself resolves as canceled.
  - Equal to current position: dialog resolves as confirmed, but the action handler suppresses IPC and reorder.
- [ ] `Esc` or the Cancel button closes the dialog without changes.

### US2: Identify window positions in tabs

As a mux user, I want each mux window tab to show its current 1-origin position, so that I can decide the target number before invoking move-window.

**Acceptance Criteria:**
- [ ] Each mux window tab label is rendered as `[N] title`.
- [ ] The number badge is shown even when only one window exists in the session.
- [ ] The number part renders smaller than the tab title (around `0.85em`).

## Technical Requirements

### Functional Requirements

- **FR1:** Add `{ type: "move-window" }` to `MuxAction` and register `"move-window": "m"` in `DEFAULT_ACTION_BINDINGS`.
- **FR2:** Implement a `move-window-dialog.ts` modal that mirrors the rename dialog pattern (Enter/Esc, IME handling, focus restore).
- **FR3:** Dispatch a new IPC message `MoveWindow` carrying the 0-based target index to the daemon.
- **FR4:** Add `MuxSession::move_window(window_id, target_index)` that reorders windows with insert/move semantics while preserving `active_window_id`.
- **FR5:** Render mux tab labels as `[N] title` for all mux-managed tabs (including single-window case).
- **FR6:** Add i18n keys `mux.moveDialog.{title,label,cancel,confirm}` to both `src/i18n/locales/en.json` and `src/i18n/locales/ja.json`.
- **FR7:** On invalid input, no MoveWindow message is issued.
  - Dialog-level invalid (non-integer, out of `[1, windowCount]`, empty): dialog resolves with `{ confirmed: false }`.
  - Caller-level invalid (value equal to the current 1-origin position, or target window no longer exists by the time the dialog resolves): the action handler (`mux-action-handler.ts`) suppresses the IPC send without showing an error.

### Non-Functional Requirements

- **NFR1 - Platform:** Works on Linux and Windows (no OS-specific branches required for this feature).
- **NFR2 - UI Consistency:** Reuses `sftp-dialog-*` styles; follows `doc/UI-DESIGN-GUIDELINES.yaml`.
- **NFR3 - Responsiveness:** Reorder completes within 200ms under typical window counts.
- **NFR4 - Non-destructive on failure:** A failed reorder leaves the previous window order intact.

## Implementation Approach

### Architecture

**Affected layers:**

```
┌──────────────────────────────────────────────┐
│ Frontend: prefix-key → action-handler        │
│           → move-window-dialog               │
│           → mux-client (IPC send)            │
│           → tab-bar-ui (number rendering)    │
├──────────────────────────────────────────────┤
│ IPC: MuxMessageType.MoveWindow               │
│      + MoveWindowMsg { target_index: u32 }   │
├──────────────────────────────────────────────┤
│ Backend: handlers::handle_move_window        │
│          → SessionManager                    │
│          → MuxSession::move_window           │
│          (no broadcast; reconciled on next   │
│           attach via Welcome — see 論点D)     │
└──────────────────────────────────────────────┘
```

### Data Flow

```
User presses prefix + m
  → PrefixKeyHandler dispatches MuxAction { type: "move-window" }
  → mux-action-handler opens move-window-dialog
  → User enters number and confirms
  → Dialog resolves with confirmed: true, value: N (1-origin)
  → Handler validates (1 <= N <= windows.length, N !== currentIndex+1)
  → Handler converts to target_index = N - 1 (0-based)
  → Handler reorders local muxWindows/muxPaneIds and emits state change
     (optimistic; tab labels re-render immediately with updated [N] numbers)
  → Handler sends MuxMessage { type: MoveWindow, pane_id: <active pane id>,
                               payload: bincode(MoveWindowMsg { target_index }) }
  → Daemon handle_move_window resolves pane_id → (session_id, window_id)
  → SessionManager calls MuxSession::move_window(window_id, target_index)
  → Daemon updates its own window_order; no broadcast to attached GUI
     (reconciled on next attach via Welcome payload)
```

### IPC Design

#### New message type

In `src-tauri/src/mux/ipc/protocol.rs`:

```rust
pub enum MessageType {
    // ... existing variants
    MoveWindow = 0x1A, // next free identifier after RequestPaneSnapshot (0x19)
}
```

Add matching arm to `MessageType::from_u8`, and extend the `test_message_type_round_trip` and `test_apc_round_trip_all_message_types` tests to include `0x1A`.

In `src/terminal/mux/mux-client.ts`:

```ts
export const MuxMessageType = {
  // ... existing keys
  MoveWindow: 0x1A,
};
```

#### Payload

```rust
/// Move window request. The window is identified by `msg.pane_id`
/// (active pane id from GUI, or window id from future CLI use) and moved
/// to `target_index` (0-based) within its session's ordered window list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveWindowMsg {
    pub target_index: u32,
}
```

Bincode layout for `MoveWindowMsg`: 4 bytes little-endian `u32`.

#### Frame

Reuses the existing frame format: `[len: u32][type: u8][pane_id: u32][payload]`.
The frontend sends `pane_id = active pane id`; the daemon resolves pane → window (mirroring `handle_rename_window`).

### Backend Design

#### `MuxSession::move_window`

The current storage `windows: BTreeMap<WindowId, MuxWindow>` orders by `WindowId`, which is not compatible with arbitrary reordering. Two implementation options:

**Option A (preferred): introduce an explicit order vector.**

Add `window_order: Vec<WindowId>` to `MuxSession`. Update `add_window` / `remove_window` to maintain this vector. Iterate via `window_order` in `session_list()` and any caller that enumerates windows by display order. `BTreeMap` is retained for O(log n) lookup by id.

**Option B: replace storage with `IndexMap<WindowId, MuxWindow>`.**

Adds a dependency (`indexmap`). Preserves insertion order natively; supports `swap_indices` / `move_index`.

Option A is preferred because it minimizes the dependency surface and keeps the existing `BTreeMap` lookup semantics. The implementation plan (to be produced by `/sdd.2-create-plan`) finalizes this choice.

Signature:

```rust
impl MuxSession {
    /// Move the window identified by `window_id` to `target_index` (0-based)
    /// within the ordered window list. Uses insert/move semantics: the window
    /// is removed from its current position and inserted at `target_index`.
    /// `target_index` is clamped into `[0, window_count - 1]`.
    /// Returns `true` if the order changed, `false` otherwise.
    /// Does not modify `active_window_id`.
    pub fn move_window(&mut self, window_id: WindowId, target_index: usize) -> bool;
}
```

Behavior details:
- If `window_id` does not exist: return `false`.
- If `target_index` equals the current position: return `false` (no-op).
- If `target_index >= window_count`: clamp to `window_count - 1`.
- Update any callers that materialize `WindowInfo` from iteration order (e.g., `SessionManager::session_list`) so that they read from `window_order`.

#### IPC handler

Add to `src-tauri/src/mux/ipc/handlers.rs`:

```rust
pub(super) async fn handle_move_window(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) {
    let move_msg: MoveWindowMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid MoveWindow payload");
            return;
        }
    };
    let id = msg.pane_id;
    let target_index = move_msg.target_index as usize;

    let mut mgr = session_manager.lock().await;

    // Try as pane_id first (GUI sends active pane_id)
    if let Some((sid, wid)) = mgr.find_pane(id) {
        if let Some(session) = mgr.get_session_mut(sid) {
            session.move_window(wid, target_index);
            log::info!(
                "MoveWindow: pane {} -> window {} -> index {}",
                id, wid, target_index
            );
        }
        return;
    }

    // Fall back to window_id
    if let Some(sid) = mgr.find_window_session(id) {
        if let Some(session) = mgr.get_session_mut(sid) {
            session.move_window(id, target_index);
            log::info!("MoveWindow: window {} -> index {}", id, target_index);
        }
    } else {
        log::warn!("MoveWindow: id {} not found as pane or window", id);
    }
}
```

Wire into the existing IPC dispatch (same pattern as `RenameWindow`).

The daemon does not broadcast the reordered list. The GUI applies an optimistic local reorder (with rollback on IPC failure) and the authoritative order is reconciled on the next attach via the `Welcome` payload. See 論点D in IMPLEMENTATION.md.

### Frontend Design

#### `src/terminal/mux/prefix-key.ts`

```ts
export type MuxAction =
  | { type: "detach" }
  | { type: "new-window" }
  | { type: "next-window" }
  | { type: "prev-window" }
  | { type: "rename-window" }
  | { type: "move-window" }          // new
  | { type: "prefix-passthrough" };

export const DEFAULT_ACTION_BINDINGS: Record<string, string> = {
  "detach": "d",
  "new-window": "c",
  "next-window": "n",
  "prev-window": "p",
  "rename-window": ",",
  "move-window": "m",                // new
};
```

No changes to the state machine are required.

#### `src/terminal-app/mux/move-window-dialog.ts` (new)

Mirrors `rename-window-dialog.ts`:

```ts
export interface MoveWindowDialogOptions {
  currentIndex: number;  // 1-origin, for display only
  windowCount: number;   // used for validation hint (min=1, max=windowCount)
}

export interface MoveWindowDialogResult {
  confirmed: boolean;
  /** 1-origin target index, parsed from input. Undefined if canceled or invalid. */
  value?: number;
}

export function showMoveWindowDialog(
  options: MoveWindowDialogOptions,
): Promise<MoveWindowDialogResult>;
```

Implementation notes:
- Reuse the `sftp-dialog-overlay` / `sftp-dialog` / `sftp-dialog-*` classes.
- Use an `<input type="text">` with `inputmode="numeric"` and `pattern="[0-9]*"`. Do not use `<input type="number">` to keep consistent styling across Linux/Windows WebView renderers.
- On Enter: parse `input.value.trim()` with `Number.parseInt(v, 10)`; if `Number.isNaN(n)` or outside `[1, windowCount]`, resolve with `{ confirmed: false }`. Otherwise resolve with `{ confirmed: true, value: n }` — the dialog does NOT compare against `currentIndex` (that is caller responsibility).
- On Esc / Cancel: resolve with `{ confirmed: false }`.
- Preserve the `e.isComposing || e.keyCode === 229` guard for IME.
- Restore focus to `previouslyFocused` on close.
- `maxLength` on the input is sufficient (e.g., 4). No live error indicator is shown.

#### `src/terminal-app/mux/mux-action-handler.ts`

- Add a module-local guard `moveDialogOpen = false` analogous to `renameDialogOpen`.
- In the `switch (action.type)` add a `case "move-window"` that:
  1. Captures current `windows`, `activeIndex`, and target window id.
  2. Opens `showMoveWindowDialog({ currentIndex: activeIndex + 1, windowCount: windows.length })`.
  3. On `confirmed: true`:
     - Re-resolve the current index of `targetWinId` (it may have shifted).
     - If `value === currentIndex + 1`: do nothing (same as current).
     - If `value < 1 || value > currentCount`: do nothing.
     - Otherwise compute `target_index = value - 1` and send:
       - `MuxMessageType.MoveWindow` with `pane_id = active pane id` and bincode-encoded `MoveWindowMsg { target_index }` (4-byte LE `u32`).
  4. Use the existing `sendMuxControl` helper.
- **Optimistic local update is required** (see IMPLEMENTATION.md §論点D). The current IPC protocol has no path for the daemon to broadcast an updated window order to attached GUI clients, so the frontend must reorder its local `muxWindows` / `muxPaneIds` arrays immediately on confirm and call `emitMuxStateChange`. The daemon-side `MuxSession.window_order` is the authority, but it is reconciled only at the next attach via the `Welcome` payload. This mirrors the existing `RenameWindow` pattern (GUI-initiated rename is optimistically applied locally without waiting for a broadcast).

#### `src/tab-bar/tab-bar-ui.ts` — `renderMuxSubTabs`

Update to render a number prefix for each window, and also show the number when `windows.length <= 1` (currently falls through to `restoreMuxOriginalTab`).

Rendering:

```ts
// Per window: tab label inner DOM becomes
// <span class="mux-window-number">[N]</span><span class="tab-title">title</span>
```

CSS (added to the existing tab-bar stylesheet):

```css
.mux-window-number {
  font-size: 0.85em;
  opacity: 0.75;
  margin-right: 0.25em;
  font-variant-numeric: tabular-nums;
}
```

Single-window case:
- Do not call `restoreMuxOriginalTab` when in mux mode. Instead, render as a mux-tab-group with a single `mux-window-tab` child that still shows `[1] title`.
- Alternatively, keep the original tab element and inject the `mux-window-number` span in front of the existing title span when the tab represents a mux session. The implementation plan picks one approach.

### Dependencies

**Internal Dependencies:**
- `src/terminal/mux/prefix-key.ts` — action type and default bindings.
- `src/terminal-app/mux/mux-action-handler.ts` — dispatch and IPC send.
- `src/terminal/mux/mux-client.ts` — `MuxMessageType` constant.
- `src/tab-bar/tab-bar-ui.ts` — tab label rendering.
- `src/i18n/locales/{en,ja}.json` — dialog strings.
- `src-tauri/src/mux/session/session.rs` — `MuxSession::move_window`.
- `src-tauri/src/mux/session/manager.rs` — iteration order source for `WindowInfo`.
- `src-tauri/src/mux/ipc/protocol.rs` — `MessageType::MoveWindow`, `MoveWindowMsg`.
- `src-tauri/src/mux/ipc/handlers.rs` — `handle_move_window` dispatch.

**External Dependencies:**
- None new (if Option A is chosen). Option B adds `indexmap`.

### File Structure

New files:
```
src/terminal-app/mux/move-window-dialog.ts          # Modal dialog component
doc/tasks/mux-move-window/requirements.md           # Japanese requirements
doc/tasks/mux-move-window/SPEC.md                   # This document
doc/tasks/mux-move-window/sdd.yaml                  # Workflow state
```

Modified files:
```
src/terminal/mux/prefix-key.ts
src/terminal/mux/prefix-key.test.ts
src/terminal/mux/mux-client.ts
src/terminal-app/mux/mux-action-handler.ts
src/tab-bar/tab-bar-ui.ts
src/tab-bar/tab-bar.css                             # or equivalent stylesheet
src/i18n/locales/en.json
src/i18n/locales/ja.json
src-tauri/src/mux/session/session.rs
src-tauri/src/mux/session/manager.rs                # window_order aware enumeration
src-tauri/src/mux/ipc/protocol.rs
src-tauri/src/mux/ipc/handlers.rs
src-tauri/src/mux/ipc/mod.rs                        # dispatch registration
```

## Test Scenarios

### Unit Tests

Frontend (`bun test`):

- [ ] `prefix-key.test.ts`: `prefix + m` dispatches `{ type: "move-window" }`.
- [ ] `prefix-key.test.ts`: `"move-window"` is included in the `all tmux-compatible bindings are present` loop (or a dedicated test) with key `"m"`.
- [ ] `prefix-key.test.ts`: the `removed tmux action keys` negative test is updated if `"m"` was previously listed there (it is not, per current file).
- [ ] `move-window-dialog.test.ts` (new):
  - [ ] Enter with a valid integer in range resolves `{ confirmed: true, value }`.
  - [ ] Enter with a non-integer resolves `{ confirmed: false }`.
  - [ ] Enter with a value `< 1` or `> windowCount` resolves `{ confirmed: false }`.
  - [ ] Enter with a value equal to `currentIndex` resolves `{ confirmed: true, value }` (the dialog is responsibility-agnostic about "same position"; the caller decides).
  - [ ] Esc resolves `{ confirmed: false }`.
  - [ ] Cancel button resolves `{ confirmed: false }`.
  - [ ] Confirm button with valid input resolves `{ confirmed: true, value }`.
  - [ ] IME composition: Enter during `isComposing` does not close the dialog.
  - [ ] Focus is restored to `previouslyFocused` after close.

Backend (`cargo test --manifest-path src-tauri/Cargo.toml`):

- [ ] `session::tests::test_move_window_to_first`: `[A,B,C,D]`, move D → index 0 → `[D,A,B,C]`.
- [ ] `session::tests::test_move_window_to_last`: `[A,B,C,D]`, move A → index 3 → `[B,C,D,A]`.
- [ ] `session::tests::test_move_window_to_middle`: `[A,B,C,D]`, move B → index 2 → `[A,C,B,D]` (remove-then-insert: B removed leaves `[A,C,D]`, then insert at 0-based index 2 → `[A,C,B,D]`).
- [ ] `session::tests::test_move_window_same_position`: move X → current index → order unchanged, returns `false`.
- [ ] `session::tests::test_move_window_out_of_range_clamps`: `target_index >= window_count` clamps to last.
- [ ] `session::tests::test_move_window_unknown_id`: returns `false`, order unchanged.
- [ ] `session::tests::test_move_window_preserves_active`: active window id is unchanged after move.
- [ ] `session::tests::test_move_window_single_window_noop`: single-window session returns `false`.
- [ ] `manager::tests::test_session_list_reflects_move_window_order`: after move, `session_list()[0].windows` is in the new order.
- [ ] `protocol::tests::test_move_window_message_type` + extension of `test_message_type_round_trip` and `test_apc_round_trip_all_message_types` to include `0x1A`.
- [ ] `protocol::tests::test_move_window_msg_round_trip`: `MoveWindowMsg { target_index: 3 }` serializes and deserializes correctly via bincode and APC.

### Integration Tests

- [ ] `handlers::tests` (if existing) cover `handle_move_window` with pane_id resolution. The handler does not accept a bare window_id (session-local IDs can collide across sessions).
- [ ] Frontend rollback path: when the IPC send from `mux-action-handler` fails, the optimistic local reorder is reverted by calling `reorderMuxWindows` with swapped indices.

### E2E Tests

**Existing E2E tests**: `e2e-tests/specs/*.e2e.js` (Docker + tauri-driver).
**Run command**: `./scripts/run-e2e-docker.sh test`

- [ ] Existing E2E tests pass without regression.
- [ ] Scenario: start session with 3 mux windows (via `prefix + c` twice), press `prefix + m`, enter `1`, confirm → active window is now at position 1.
- [ ] Scenario: press `prefix + m`, press Esc → dialog closes and the order is unchanged.
- [ ] Scenario: press `prefix + m`, enter a non-numeric string, press Enter → dialog closes and the order is unchanged.
- [ ] Scenario: press `prefix + m`, enter `999`, press Enter → dialog closes and the order is unchanged.
- [ ] Scenario: with a single mux window, verify the tab label shows a `[1]` prefix.

### Edge Cases

- [ ] Rapid reordering: two `prefix + m` invocations in quick succession — second invocation is guarded by `moveDialogOpen` to prevent stacked dialogs.
- [ ] Window closed while dialog is open: on confirm, re-resolve `targetWinId`; if it no longer exists, abort silently.
- [ ] Empty input on Enter: treated as invalid → cancel.
- [ ] Leading/trailing whitespace: trimmed before parsing.
- [ ] IME commit Enter: does not trigger confirm.

### Performance Tests

- [ ] Not required beyond the NFR3 informal check (<200ms perceived).

## Security Considerations

- **Input Validation:** The dialog validates integer range client-side; the daemon additionally clamps `target_index` in `MuxSession::move_window`.
- **XSS Prevention:** Tab label number is rendered via `textContent`; no HTML is constructed from user input.
- **IPC Trust Boundary:** The new message type reuses the existing authenticated IPC path (APC / OSC 9999); no new transport is introduced.

## Error Handling

Invalid input is silently ignored (no visible error surface). Logs:

| Condition | Log Level | Origin |
|-----------|-----------|--------|
| Dialog invalid input | none | — |
| `decode_payload` failure | `warn` | backend |
| pane_id/window_id not found | `warn` | backend |
| Successful reorder | `info` | backend |

## Performance Optimization

- Backend reorder uses `Vec::remove` + `Vec::insert` (O(n)); negligible for typical window counts (< 32).
- Frontend redraws only the affected tab group (existing differential DOM update in `renderMuxSubTabs`).

## Success Criteria

- [ ] All FR1–FR7 implemented and tested.
- [ ] Unit tests (frontend + backend) pass.
- [ ] E2E scenarios pass in Docker.
- [ ] Linux and Windows builds succeed (GitHub Actions).
- [ ] `doc/UI-DESIGN-GUIDELINES.yaml` is updated if new tokens or components are introduced (run `/gen-design-guidelines`).
- [ ] No regression in existing mux actions (`detach`, `new-window`, `next-window`, `prev-window`, `rename-window`).

## Open Questions

> **Note**: Open items are tracked in `sdd.yaml` as `status: tbd` when unresolved. All items required for implementation are resolved at spec time.

- None.

## Implementation Phases

Single phase. Planning is produced by `/sdd.2-create-plan` and will cover:
1. Backend ordering model (Option A vs B) and `MuxSession::move_window` tests.
2. IPC protocol extension and handler dispatch.
3. Frontend dialog and action wiring.
4. Tab label rendering update.
5. i18n strings.

## References

- Requirements: `doc/tasks/mux-move-window/requirements.md`
- Existing rename implementation: `src/terminal-app/mux/rename-window-dialog.ts`, `src-tauri/src/mux/ipc/handlers.rs::handle_rename_window`
- IPC protocol: `src-tauri/src/mux/ipc/protocol.rs`
- Session state: `src-tauri/src/mux/session/session.rs`
- Tab rendering: `src/tab-bar/tab-bar-ui.ts::renderMuxSubTabs`
- UI design tokens: `doc/UI-DESIGN-GUIDELINES.yaml`
- Debugging constraints: `.claude/rules/debugging-constraints.md`
