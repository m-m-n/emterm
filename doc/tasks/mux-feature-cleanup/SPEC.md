# Feature: Mux Feature Cleanup

## Overview

Remove non-working and unused multiplexer features from eMterm. Pane split, pane navigation, zoom, and copy mode are deleted. Window management, detach/reattach, and clipboard paste remain. Related frontend files, backend IPC handlers, settings keys, i18n entries, and E2E test cases are pruned accordingly. `doc/tasks/terminal-multiplexer/SPEC.md` is updated to reflect the new shape.

## Objectives

- Remove the `split-vertical`, `split-horizontal`, `next-pane`, `prev-pane`, `close-pane`, `zoom-toggle`, and `copy-mode` mux actions.
- Remove the `SplitPane` IPC message type and its daemon handler.
- Delete pane-layout, pane-border, pane-manager, drag-resize, and copy-mode modules on the frontend.
- Keep one pane per window by construction; do not refactor the `Pane` struct itself.
- Update `doc/tasks/terminal-multiplexer/SPEC.md` so it matches the remaining mux surface.

## User Stories

### US1: Cleaner Mux Keybinds
As an eMterm user, I want the settings panel's mux keybind list to show only working actions, so that I do not see broken bindings.

**Acceptance Criteria:**
- [ ] Settings panel > Mux > Keybinds shows only: detach, new-window, next-window, prev-window, rename-window, paste.
- [ ] `settings.mux.keybind.splitVertical`, `splitHorizontal`, `nextPane`, `prevPane`, `closePane`, `zoomToggle`, `copyMode` are absent from both `en.json` and `ja.json`.

### US2: No Dead Mux Actions
As an eMterm user, I want `prefix + %` / `prefix + "` / `prefix + o` / `prefix + ;` / `prefix + x` / `prefix + z` / `prefix + [` to have no effect, so that mux mode does not expose broken functionality.

**Acceptance Criteria:**
- [ ] Pressing the above keys after the prefix consumes the prefix state but does not send any IPC or change the UI.
- [ ] `prefix + d`, `prefix + c`, `prefix + n`, `prefix + p`, `prefix + ,`, `prefix + ]`, and `prefix + prefix` continue to work.

### US3: Consistent Docs
As a developer, I want `doc/tasks/terminal-multiplexer/SPEC.md` to describe only the features that exist, so that onboarding and code review match reality.

**Acceptance Criteria:**
- [ ] The updated SPEC.md contains no references to pane split, pane layout, drag-resize, zoom, or copy mode.
- [ ] The IPC message table no longer lists `SplitPane (0x11)`.
- [ ] The file-structure section matches the post-cleanup tree.

## Technical Requirements

### Functional Requirements

- **FR1 (Actions removed):** Delete the following variants from `MuxAction` and keys from `DEFAULT_ACTION_BINDINGS` in `src/terminal/mux/prefix-key.ts`:
  - `split-vertical`, `split-horizontal`, `next-pane`, `prev-pane`, `close-pane`, `zoom-toggle`, `copy-mode`
- **FR2 (Actions retained):** Keep:
  - `detach`, `new-window`, `next-window`, `prev-window`, `rename-window`, `paste`, `prefix-passthrough`
- **FR3 (Frontend files deleted):**
  - `src/terminal-app/mux/mux-multi-pane.ts`
  - `src/terminal-app/mux/mux-drag-resize.ts`
  - `src/terminal-app/mux/mux-copy-mode.ts`
  - `src/terminal/mux/layout.ts`
  - `src/terminal/mux/layout.test.ts`
  - `src/terminal/mux/pane-manager.ts`
  - `src/terminal/mux/pane-border.ts`
  - `src/terminal/mux-copy-mode/index.ts`
  - `src/terminal/mux-copy-mode/index.test.ts`
  - `src/terminal/mux-copy-mode/emacs-keybinds.ts`
  - `src/terminal/mux-copy-mode/vi-keybinds.ts`
- **FR4 (Frontend files reduced):**
  - `src/terminal-app/mux/mux-action-handler.ts`: drop `case` branches for the removed actions; drop context fields used only by them (`getMuxLayoutRoot`, `setActiveMuxPane`, `toggleMuxZoom`, `enterCopyMode`, `setMuxPendingSplitCount`, `setMuxPendingSplitDirection`, related callers).
  - `src/terminal-app/mux/mux-session.ts`, `src/terminal-app/mux/mux-window-manager.ts`: remove pane-related fields / method calls.
  - `src/terminal/mux/mux-client.ts`: drop `MuxMessageType.SplitPane = 0x11` and any call sites.
  - `src/terminal/mux/mux-client.test.ts`, `src/terminal/mux/prefix-key.test.ts`: keep only the retained actions.
  - `src/settings/sections/mux-section.ts`: drop removed entries from `ACTION_I18N_KEYS`.
  - `src/i18n/locales/en.json`, `src/i18n/locales/ja.json`: drop `settings.mux.keybind.splitVertical`, `splitHorizontal`, `nextPane`, `prevPane`, `closePane`, `zoomToggle`, `copyMode`.
- **FR5 (Backend protocol removed):**
  - `src-tauri/src/mux/ipc/protocol.rs`: remove `MessageType::SplitPane = 0x11`, remove `SplitPaneMsg`, remove the `0x11` arm in `MessageType::from_u8`. Adjust tests that iterate the numeric range (e.g. `test_message_type_round_trip`, `test_apc_round_trip_all_message_types`).
  - `src-tauri/src/mux/ipc/handlers.rs`: remove `handle_split_pane` and any dispatch to it in `connection.rs` (or wherever SplitPane is matched).
- **FR6 (Backend data model kept):** `src-tauri/src/mux/session/pane.rs` keeps the existing `Pane` struct. The invariant "one pane per window" is enforced by callers: after cleanup there is no code path that creates a second pane in a window. `Window.panes: HashMap<PaneId, Pane>` remains but is expected to hold exactly one entry after `handle_create_window`.
- **FR7 (Settings migration policy):** No migration code. User settings that contain removed `mux.keybinds.*` keys are dropped on deserialization via serde's default field handling. Rust structs stop listing the removed keys and serde silently ignores unknown fields.
- **FR8 (E2E test policy):**
  - In-scope specs for editing: `e2e-tests/specs/mux.e2e.js`, `e2e-tests/specs/mux-multi-session.e2e.js`, `e2e-tests/specs/mux-reattach.e2e.js`, `e2e-tests/specs/viewer-tab-switch-keyboard.e2e.js`.
  - Test cases that assert pane split, pane navigation, zoom, or copy mode are deleted.
  - Test cases that assert window (sub-tab) creation/switch/rename, detach/reattach, and clipboard paste are kept.
  - Concrete case-level edits are resolved in `/sdd.2-create-plan`.
- **FR9 (doc/tasks/terminal-multiplexer/SPEC.md update):** Update in place.
  - Delete US3 (Split Panes) and US5 (Copy Mode).
  - Delete FR7 (Pane Layout) and FR10 (Copy Mode) and renumber remaining FRs as needed (or keep original numbers with a note — decide in `/sdd.2-create-plan`).
  - Delete the "Pane Layout" section (binary tree model, preset layouts).
  - Remove `SplitPane (0x11)` from the message-type table.
  - Remove Phase 3 (Pane Split + Layout) and Phase 6 (Copy Mode) from "Implementation Phases".
  - Remove `layout.ts`, `pane-manager.ts`, and `mux-copy-mode/` from the "File Structure" diagram.
  - Keep: daemon/IPC foundation, single-pane detach/reattach, window management, status bar, tmux.conf conversion, Windows compatibility, OSC extensions, snapshot-based state sync.

### Non-Functional Requirements

- **NFR1 - Behavior preservation:** Retained mux actions behave identically before and after cleanup. No user-visible change for detach, window management, or paste.
- **NFR2 - Test coverage:** `cargo test`, `bun test`, `bun run typecheck`, and `./scripts/run-e2e-docker.sh` all pass after cleanup.
- **NFR3 - No migration surprises:** A `settings.json` file that contains removed `mux.keybinds.*` keys loads without error. Warnings in logs are acceptable; hard errors are not.

## Implementation Approach

### Removal Order (recommended)

1. Remove frontend action handlers (`mux-action-handler.ts` case branches).
2. Remove frontend action variants and default bindings (`prefix-key.ts`).
3. Remove frontend modules (layout, pane-manager, pane-border, copy-mode, multi-pane, drag-resize).
4. Remove settings section entries and i18n keys.
5. Remove backend `SplitPane` message type and handler.
6. Update frontend / backend tests.
7. Prune E2E spec cases.
8. Update `doc/tasks/terminal-multiplexer/SPEC.md`.

### File Structure (post-cleanup)

```
src/
├── terminal/
│   └── mux/
│       ├── index.ts
│       ├── mux-client.ts
│       ├── mux-client.test.ts
│       ├── mux-logger.ts
│       ├── prefix-key.ts
│       ├── prefix-key.test.ts
│       ├── tab-group.ts
│       └── tab-group.test.ts
├── terminal-app/
│   └── mux/
│       ├── mux-action-handler.ts   (reduced)
│       ├── mux-session.ts          (reduced)
│       └── mux-window-manager.ts   (reduced)
└── settings/
    └── sections/
        └── mux-section.ts           (reduced)

src-tauri/
└── src/
    └── mux/
        ├── bridge.rs
        ├── cli.rs
        ├── daemon.rs
        ├── ipc/
        │   ├── protocol.rs          (SplitPane removed)
        │   ├── handlers.rs          (handle_split_pane removed)
        │   └── …
        ├── ring_buffer.rs
        ├── session/
        │   ├── pane.rs              (struct kept)
        │   └── …
        ├── snapshot.rs
        ├── tmux_conf
        └── tmux_import.rs
```

### Retained mux action mapping

| Action | Default key (after prefix) | Handler |
|---|---|---|
| `detach` | `d` | Sends `Detach` IPC; exits mux mode on daemon's `Detached` reply. |
| `new-window` | `c` | Sends `CreateWindow` IPC; receives `PaneCreated` asynchronously. |
| `next-window` | `n` | Advances active mux window index; triggers `switchMuxWindow`. |
| `prev-window` | `p` | Reverse of `next-window`. |
| `rename-window` | `,` | Prompts for name; sends `RenameWindow` IPC with active pane id. |
| `paste` | `]` | Calls `pasteFromClipboard`. |
| `prefix-passthrough` | (prefix key again) | Writes prefix control byte to the active PTY. |

### Deleted IPC protocol items

| Item | File | Action |
|---|---|---|
| `MessageType::SplitPane = 0x11` | `src-tauri/src/mux/ipc/protocol.rs` | Delete enum variant and `from_u8` arm. |
| `SplitPaneMsg` | `src-tauri/src/mux/ipc/protocol.rs` | Delete struct. |
| `handle_split_pane` | `src-tauri/src/mux/ipc/handlers.rs` | Delete function and dispatch arm. |
| `MuxMessageType.SplitPane` | `src/terminal/mux/mux-client.ts` | Delete constant and call sites. |

### Deleted settings / i18n keys

`src/i18n/locales/{en,ja}.json`:

- `settings.mux.keybind.splitVertical`
- `settings.mux.keybind.splitHorizontal`
- `settings.mux.keybind.nextPane`
- `settings.mux.keybind.prevPane`
- `settings.mux.keybind.closePane`
- `settings.mux.keybind.zoomToggle`
- `settings.mux.keybind.copyMode`

Other locale entries under `settings.mux.*` (title, general, prefix, mouse, statusPosition, keybinds, and the retained keybind labels) stay.

## Test Scenarios

### Unit Tests (Rust)
- [ ] `MessageType::from_u8(0x11)` returns `None`.
- [ ] `test_message_type_round_trip` iterates only over retained message types.
- [ ] Round-trip tests for retained control messages still pass.

### Unit Tests (TypeScript)
- [ ] `prefix-key.test.ts`: unknown action keys (`%`, `"`, `o`, `;`, `x`, `z`, `[`) reset the handler to idle but dispatch no action.
- [ ] `prefix-key.test.ts`: retained keys (`d`, `c`, `n`, `p`, `,`, `]`) dispatch the corresponding action.
- [ ] `mux-client.test.ts`: `MuxMessageType` no longer exposes `SplitPane`.

### Integration Tests
- [ ] Frontend compiles without the deleted imports.
- [ ] `bun run typecheck` passes.
- [ ] Daemon starts, accepts `CreateWindow`, `SwitchWindow`, `RenameWindow`, `DestroyWindow`, `Resize`, `Detach`, `Attach`, `RequestPaneSnapshot`.

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` (26 specs)
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing specs pass after pane-split/zoom/copy-mode cases are removed.
- [ ] `mux.e2e.js`: detach and new-window scenarios still pass.
- [ ] `mux-multi-session.e2e.js`: multi-session window-only scenarios still pass.
- [ ] `mux-reattach.e2e.js`: detach-reattach scenarios still pass.
- [ ] `viewer-tab-switch-keyboard.e2e.js`: window-switch keyboard scenarios still pass.

### Edge Cases
- [ ] A `settings.json` containing legacy `mux.keybinds.split-vertical` loads without error and exposes no broken UI row for it.
- [ ] `prefix + %` on a live mux session is a no-op (no IPC, no log entry at `error`).
- [ ] Attempting to send `SplitPane` from a stale binary would be rejected as an unknown message type (`MessageType::from_u8` returns `None`); the daemon logs a warning and continues.

## Security Considerations

No change. Mux cleanup does not introduce new trust boundaries. Socket-path validation, file permissions, and nesting prevention remain as specified in `doc/tasks/terminal-multiplexer/SPEC.md`.

## Error Handling

| Scenario | Behavior |
|---|---|
| Legacy client sends `SplitPane` (0x11) | Daemon decodes via `MessageType::from_u8` → `None` → warning log, frame discarded. |
| `settings.json` contains removed `mux.keybinds.*` keys | serde ignores unknown fields; settings load succeeds. |
| `prefix + <removed key>` pressed | Prefix handler consumes the event, resets to idle; no user-visible effect. |

## Success Criteria

- [ ] All functional requirements (FR1-FR9) are implemented.
- [ ] All deleted files are absent from the working tree.
- [ ] All reduced files compile and pass their tests.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes.
- [ ] `bun test` passes.
- [ ] `bun run typecheck` passes.
- [ ] `./scripts/run-e2e-docker.sh` passes.
- [ ] Settings panel Mux section renders only retained keybinds.
- [ ] `doc/tasks/terminal-multiplexer/SPEC.md` reflects the post-cleanup state.
- [ ] Code review completed.

## Open Questions

> **Note**: No TBD items. Concrete per-case E2E edits are deferred to `/sdd.2-create-plan`.

## Implementation Phases

### Phase 1: Frontend cleanup
**Goals:** Remove frontend mux actions, modules, and settings/i18n entries.
**Deliverables:**
- Deleted files listed in FR3.
- Reduced files listed in FR4 (frontend portion).
- Updated unit tests.

### Phase 2: Backend cleanup
**Goals:** Remove `SplitPane` IPC message type and handler.
**Deliverables:**
- Updated `protocol.rs`, `handlers.rs`, related dispatch site(s).
- Updated Rust unit tests.

### Phase 3: E2E prune and SPEC update
**Goals:** Align E2E specs and `doc/tasks/terminal-multiplexer/SPEC.md` with the post-cleanup state.
**Deliverables:**
- Edited E2E specs per FR8.
- Updated `doc/tasks/terminal-multiplexer/SPEC.md` per FR9.

## References

- `doc/tasks/terminal-multiplexer/SPEC.md`: existing (pre-cleanup) mux specification, to be updated as part of this task.
- `src/terminal/mux/prefix-key.ts`: source of the `MuxAction` union and default bindings.
- `src-tauri/src/mux/ipc/protocol.rs`: source of the IPC `MessageType` enum.
