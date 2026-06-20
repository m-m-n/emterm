# Feature: mux move-window

## Overview

Add a `move-window` action to mux mode that reorders the active window to a user-specified 1-origin position. Triggered by `prefix + Ctrl+T`, it opens a modal number input and applies insert/move semantics (equivalent to tmux `move-window -t N`). Tab labels render each mux window with a leading `[N]` number badge.

## Objectives

- Provide a keyboard-driven way to reorder mux windows inside a session.
- Display 1-origin window numbers in tab labels for visibility.
- Keep the implementation aligned with existing mux action patterns (rename-window).

## User Stories

### US1: Reorder the active window

As a mux user, I want to move the active window to a specific numbered position, so that I can organize my windows without detaching or recreating them.

**Acceptance Criteria:**
- [ ] `prefix + Ctrl+T` opens a modal number input. With a single window the action is a no-op — no dialog opens (the only window cannot be moved); `dispatch_mux_action` returns early.
- [ ] Entering a valid 1-origin number and confirming moves the active window using insert/move semantics.
- [ ] Invalid input closes the dialog without changes to window order:
  - Out of range / non-numeric / empty, **or equal to the current position**: `resolve_move_confirm` resolves the dialog as canceled, so no MoveWindow IPC is sent.
- [ ] `Esc` or the Cancel button closes the dialog without changes.

### US2: Identify window positions in tabs

As a mux user, I want each mux window tab to show its current 1-origin position, so that I can decide the target number before invoking move-window.

**Acceptance Criteria:**
- [ ] Each mux window tab label is rendered as `[N] title`.
- [ ] The number badge is shown even when only one window exists in the session.

## Technical Requirements

### Functional Requirements

- **FR1:** Add `PrefixAction::MoveWindow` to the prefix action enum and register `("move-window", ctrl_letter('t'))` (Ctrl+T) in `DEFAULT_ACTION_BINDINGS` in `src-tauri/src/mux/prefix.rs`.
- **FR2:** Implement the move dialog in the native egui UI (`src-tauri/src/ui/mux_dialogs.rs::draw_move`), mirroring the rename dialog (`draw_rename`): Enter confirms, Esc cancels, a `DragValue` number input clamped to `[1, windowCount]`.
- **FR3:** Dispatch a new IPC message `MoveWindow` carrying the 0-based target index to the daemon.
- **FR4:** Add `MuxSession::move_window(window_id, target_index)` that reorders windows with insert/move semantics while preserving `active_window_id`.
- **FR5:** Render mux tab labels as `[N] title` for all mux-managed tabs (including single-window case).
- **FR6:** Localize the dialog strings (title, current-position label, move-to label, OK/Cancel) via the native `crate::i18n` layer: `draw_move` receives the resolved `Locale` and switches strings inline with a `t(ja, en)` helper, matching the existing native UI i18n pattern (e.g. `render::draw_sftp_overlay`). No separate locale JSON files.
- **FR7:** On invalid input, no MoveWindow message is issued.
  - Dialog-level invalid (non-integer, out of `[1, windowCount]`, empty, or equal to the current 1-origin position): `resolve_move_confirm` (`src-tauri/src/ui/mux_dialogs.rs`) resolves the dialog as canceled, so no IPC is sent. `App::confirm_mux_move` re-checks the same conditions as defense-in-depth.
  - Stale target (the target window no longer exists by the time the dialog resolves): `refresh_mux_dialog` closes the dialog before it draws; `App::confirm_mux_move` additionally no-ops without showing an error.

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
│ Native UI: prefix Latch → App::observe_mux_  │
│            key → App::dispatch_mux_action    │
│            → ui::mux_dialogs::draw_move      │
│            → App::confirm_mux_move (IPC send)│
│            → ui::tab_bar ([N] rendering)     │
├──────────────────────────────────────────────┤
│ IPC: MessageType::MoveWindow                 │
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
User presses prefix + Ctrl+T
  → prefix Latch yields PrefixAction::MoveWindow (App::observe_mux_key)
  → App::dispatch_mux_action returns MuxActionOutcome::OpenMove { window_id,
     current_position, window_count }
  → App::open_mux_move_dialog sets MuxDialogState::Move { target = current }
  → ui::mux_dialogs::draw_move renders the modal; DragValue clamps input to
     [1, window_count]; Enter/OK resolves ConfirmMove { window_id, target },
     Esc/Cancel resolves Cancelled
  → App::confirm_mux_move validates (target in [1, count], target != current),
     optimistically reorders local window order (tab labels re-render with
     updated [N] numbers), then sends MuxMessage { type: MoveWindow,
     pane_id: <active pane id>, payload: bincode(MoveWindowMsg { target_index }) }
  → Daemon handle_move_window resolves pane_id → (session_id, window_id)
  → SessionManager calls MuxSession::move_window(window_id, target_index)
  → Daemon updates its own window_order; no broadcast to attached GUI
     (reconciled on next attach via Welcome payload)
```

### IPC Design

#### New message type

In `crates/mux_ipc/src/protocol.rs`:

```rust
pub enum MessageType {
    // ... existing variants
    MoveWindow = 0x1A, // next free identifier after RequestPaneSnapshot (0x19)
}
```

Add matching arm to `MessageType::from_u8`, and extend the `test_message_type_round_trip` and `test_apc_round_trip_all_message_types` tests to include `0x1A`.

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

### Native UI Design

#### Prefix binding (`src-tauri/src/mux/prefix.rs`)

`PrefixAction::MoveWindow` already exists in the action enum alongside the other
mux actions:

```rust
enum PrefixAction {
    None, Literal, Detach,
    NextWindow, PrevWindow, SelectWindow(u8),
    NewWindow, RenameWindow, MoveWindow,
}
```

`DEFAULT_ACTION_BINDINGS: &[(&str, PrefixChord)]` binds `("move-window",
ctrl_letter('t'))` (Ctrl+T), next to the other defaults (`detach` = Ctrl+D,
`new-window` = Ctrl+C, `next-window` = Ctrl+N, `prev-window` = Ctrl+P,
`rename-window` = Ctrl+R). The `Latch` state machine yields
`PrefixAction::MoveWindow`; no state-machine change is required.

#### Keystroke dispatch (`src-tauri/src/app.rs`)

- `App::observe_mux_key` runs the prefix `Latch`.
- `App::dispatch_mux_action` maps `PrefixAction::MoveWindow` →
  `MuxActionOutcome::OpenMove { window_id, current_position, window_count }`.
- `App::open_mux_move_dialog` sets `MuxDialogState::Move { target }` (seeded with
  the current position).
- `App::confirm_mux_move` validates (`target` in `[1, count]`, `target !=
  current`), optimistically reorders the local window order so the tab labels
  re-render with updated `[N]` numbers, then sends the IPC message. If the IPC
  send fails, it rolls back the optimistic local reorder.

The per-frame dialog driver is `src-tauri/src/window_host.rs::drive_mux_dialogs`.

#### Dialog state (`src-tauri/src/mux/dialog.rs`)

```rust
enum MuxDialogState {
    // ... other dialogs (Rename, ...)
    Move { window_id, current_position, window_count, target },
}

enum MuxDialogOutcome {
    Pending,
    ConfirmMove { window_id, target },
    Cancelled,
}
```

#### Dialog rendering (`src-tauri/src/ui/mux_dialogs.rs::draw_move`)

The move dialog is drawn in egui (it is a Rust function, not a web component),
mirroring `draw_rename` in the same file:

- `draw_move(state, ctx, locale)` renders an `egui::Window`.
- A `DragValue` number input clamped to `[1, window_count]`.
- Enter / OK → `MuxDialogOutcome::ConfirmMove`; Esc / Cancel → `Cancelled`.
- `resolve_move_confirm` rejects out-of-range and same-position targets,
  returning `Cancelled` in those cases.

#### Localization (`crate::i18n`)

The dialog strings (title, current-position label, move-to label, OK/Cancel) are
localized via `crate::i18n::Locale` (`En` / `Ja`), resolved once at startup from
the `language` setting via `crate::i18n::resolve`. Strings are switched inline
with a closure:

```rust
let t = |ja, en| match locale { Locale::Ja => ja, Locale::En => en };
```

This is the same pattern as `src-tauri/src/render/mod.rs::draw_sftp_overlay`.
There are no locale JSON files for the native UI.

#### Tab `[N]` rendering (`src-tauri/src/ui/tab_bar.rs`)

- `mux_group_render_model` builds one `MuxSubTabCell` per window — always,
  including the single-window case.
- `mux_sub_tab_label` returns `format!("[{}] {}", cell.index + 1, cell.name)`,
  rendered as a single egui string. (There is no separate smaller-font number
  span; that was a WebView CSS cosmetic detail and does not apply to the native
  UI.)
- Covered by the `sub_tab_label_is_numbered` test.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/mux/prefix.rs` — `PrefixAction::MoveWindow` and the `move-window` default binding.
- `src-tauri/src/app.rs` — `observe_mux_key` / `dispatch_mux_action` / `open_mux_move_dialog` / `confirm_mux_move`.
- `src-tauri/src/window_host.rs` — `drive_mux_dialogs` per-frame driver.
- `src-tauri/src/mux/dialog.rs` — `MuxDialogState::Move`, `MuxDialogOutcome`.
- `src-tauri/src/ui/mux_dialogs.rs` — `draw_move` egui dialog + `resolve_move_confirm`.
- `src-tauri/src/ui/tab_bar.rs` — `[N]` tab label rendering.
- `crate::i18n` (`src-tauri/src/`, module `i18n.rs`) — `Locale` resolution for the dialog strings.
- `src-tauri/src/mux/session/session.rs` — `MuxSession::move_window`.
- `src-tauri/src/mux/session/manager.rs` — iteration order source for `WindowInfo`.
- `crates/mux_ipc/src/protocol.rs` — `MessageType::MoveWindow`, `MoveWindowMsg`.
- `src-tauri/src/mux/ipc/handlers.rs` — `handle_move_window`.
- `src-tauri/src/mux/ipc/connection.rs` — dispatch registration.

**External Dependencies:**
- None new. Option A (an explicit `window_order: Vec<WindowId>`) was chosen, so no `indexmap` dependency is introduced.

### File Structure

No new source files are added; the move dialog lives in the existing
`src-tauri/src/ui/mux_dialogs.rs` alongside `draw_rename`.

New files (docs only):
```
doc/tasks/mux-move-window/requirements.md           # Japanese requirements
doc/tasks/mux-move-window/SPEC.md                   # This document
doc/tasks/mux-move-window/sdd.yaml                  # Workflow state
```

Modified files:
```
src-tauri/src/mux/prefix.rs                         # PrefixAction::MoveWindow + binding
src-tauri/src/app.rs                                # dispatch + dialog state/outcome + confirm
src-tauri/src/window_host.rs                        # drive_mux_dialogs
src-tauri/src/mux/dialog.rs                         # MuxDialogState::Move, MuxDialogOutcome
src-tauri/src/ui/mux_dialogs.rs                     # draw_move + resolve_move_confirm
src-tauri/src/ui/tab_bar.rs                         # [N] tab label rendering
src-tauri/src/mux/session/session.rs               # MuxSession::move_window + window_order
src-tauri/src/mux/session/manager.rs               # window_order aware enumeration
crates/mux_ipc/src/protocol.rs                  # MessageType::MoveWindow, MoveWindowMsg
src-tauri/src/mux/ipc/handlers.rs                  # handle_move_window
src-tauri/src/mux/ipc/connection.rs                # dispatch registration
```

## Test Scenarios

### Unit Tests

Native UI (`cargo test --manifest-path src-tauri/Cargo.toml`):

- [ ] `app.rs` `observe_mux_key_*`: `prefix + Ctrl+T` dispatches `PrefixAction::MoveWindow` and produces `MuxActionOutcome::OpenMove`.
- [ ] `tab_bar.rs::sub_tab_label_is_numbered`: a mux window tab label renders as `[N] title`, including the single-window case.
- [ ] `mux_dialogs.rs::resolve_move_rejects_out_of_range_and_same_position`: out-of-range and same-position targets resolve as `Cancelled`; a valid distinct target resolves as `ConfirmMove`.

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
- [ ] Rollback path: when the IPC send from `App::confirm_mux_move` fails, the optimistic local window reorder is reverted to its prior order.

### E2E Tests

**Existing E2E tests**: `e2e-tests/specs/*.e2e.js` (Docker + tauri-driver).
**Run command**: `./scripts/run-e2e-docker.sh test`

- [ ] Existing E2E tests pass without regression.
- [ ] Scenario: start session with 3 mux windows (via `prefix + Ctrl+C` twice), press `prefix + Ctrl+T`, enter `1`, confirm → active window is now at position 1.
- [ ] Scenario: press `prefix + Ctrl+T`, press Esc → dialog closes and the order is unchanged.
- [ ] Scenario: press `prefix + Ctrl+T`, enter a non-numeric string, press Enter → dialog closes and the order is unchanged.
- [ ] Scenario: press `prefix + Ctrl+T`, enter `999`, press Enter → dialog closes and the order is unchanged.
- [ ] Scenario: with a single mux window, verify the tab label shows a `[1]` prefix.

### Edge Cases

- [ ] Rapid reordering: two `prefix + Ctrl+T` invocations in quick succession — while a `MuxDialogState::Move` is already active a second invocation does not stack another dialog.
- [ ] Window closed while dialog is open: on confirm, the target window is re-resolved; if it no longer exists, the move is aborted silently.
- [ ] Empty input on Enter: treated as invalid → cancel.
- [ ] Leading/trailing whitespace: trimmed before parsing.
- [ ] IME commit Enter: does not trigger confirm.

### Performance Tests

- [ ] Not required beyond the NFR3 informal check (<200ms perceived).

## Security Considerations

- **Input Validation:** The egui `DragValue` clamps input to `[1, window_count]` in the dialog; the daemon additionally clamps `target_index` in `MuxSession::move_window`.
- **No markup injection:** The tab label is built with `format!("[{}] {}", ...)` and drawn as a plain egui string; no HTML is constructed from user input.
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
- The optimistic local reorder in `App::confirm_mux_move` updates the in-memory window order; the egui tab bar (`ui::tab_bar`) re-renders the labels on the next frame.

## Success Criteria

- [ ] All FR1–FR7 implemented and tested.
- [ ] Unit tests (native UI + backend, `cargo test`) pass.
- [ ] E2E scenarios pass in Docker.
- [ ] Linux and Windows builds succeed (GitHub Actions).
- [ ] `doc/UI-DESIGN-GUIDELINES.yaml` is updated if new tokens or components are introduced (run `/gen-design-guidelines`).
- [ ] No regression in existing mux actions (`detach`, `new-window`, `next-window`, `prev-window`, `rename-window`).

## Open Questions

> **Note**: Open items are tracked in `sdd.yaml` as `status: tbd` when unresolved. All items required for implementation are resolved at spec time.

- None.

## Implementation Phases

Single phase. Planning is produced by `/sdd.2-create-plan` and will cover:
1. Backend ordering model (Option A: `window_order: Vec<WindowId>`) and `MuxSession::move_window` tests.
2. IPC protocol extension and handler dispatch.
3. Native egui dialog (`ui::mux_dialogs::draw_move`) and dispatch wiring (`app.rs`).
4. Tab label `[N]` rendering update (`ui::tab_bar`).
5. Dialog string localization via `crate::i18n`.

## References

- Requirements: `doc/tasks/mux-move-window/requirements.md`
- Existing rename implementation: `src-tauri/src/ui/mux_dialogs.rs::draw_rename`, `src-tauri/src/mux/ipc/handlers.rs::handle_rename_window`
- IPC protocol: `crates/mux_ipc/src/protocol.rs`
- Session state: `src-tauri/src/mux/session/session.rs`
- Tab rendering: `src-tauri/src/ui/tab_bar.rs`
- UI design tokens: `doc/UI-DESIGN-GUIDELINES.yaml`
- Debugging constraints: `.claude/rules/debugging-constraints.md`
