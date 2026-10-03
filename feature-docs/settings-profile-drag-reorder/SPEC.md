# Feature: settings-profile-drag-reorder

## Overview

On Linux, items in the settings panel's Profiles list cannot be reordered by
dragging. This feature makes the Profiles list and the SSH connection list
dragstart handlers set drag data, and adds per-list regression tests.
Requirements document: `feature-docs/settings-profile-drag-reorder/REQUIREMENTS.md`.

## Objectives

- On Linux, the user can reorder items in the settings panel's Profiles list by dragging, and the new order is saved.
- On Linux, the user can reorder items in the settings panel's SSH connection list by dragging, and the new order is saved.
- Each list has a regression test that detects a dragstart handler that sets no drag data.

## User Stories

### US1: Reorder the Profiles list by dragging on Linux
As a Linux user, I want to drag a Profiles list item to another position, so that the profile order changes and is saved.

**Acceptance Criteria:**
- [ ] AC-1: Dispatching dragstart on the Profiles list item with data-index i leaves dataTransfer holding `"application/x-emterm-profile-index"` = `String(i)`, with effectAllowed `"move"`.
- [ ] AC-3: After dragstart on either list, dataTransfer holds no text/plain entry.
- [ ] AC-4: dragstart on Profiles item i followed by drop on item j (i != j) moves the profile from i to j in `currentSettings.profiles`, calls `saveSetting("profiles", reordered)` once, and calls `reRender`.
- [ ] AC-6: On either list, a drop with no preceding dragstart (dragIndex null), or a drop onto the dragged item itself, does not call `saveSetting`.
- [ ] AC-8: Manual check on Linux (WebKitGTK): in the settings panel, dragging a Profiles item to another position reorders the list, and the new order is still there after reopening the settings panel. The same holds for the SSH connection list.

### US2: Reorder the SSH connection list by dragging on Linux
As a Linux user, I want to drag an SSH connection list item to another position, so that the connection order changes and is saved.

**Acceptance Criteria:**
- [ ] AC-2: Dispatching dragstart on the SSH connection list item with data-index i leaves dataTransfer holding `"application/x-emterm-ssh-index"` = `String(i)`, with effectAllowed `"move"`.
- [ ] AC-3: After dragstart on either list, dataTransfer holds no text/plain entry.
- [ ] AC-5: dragstart on SSH item i followed by drop on item j (i != j) moves the connection from i to j in `currentSettings.ssh_connections`, calls `saveSetting("ssh_connections", reordered)` once, and calls `reRender`.
- [ ] AC-6: On either list, a drop with no preceding dragstart (dragIndex null), or a drop onto the dragged item itself, does not call `saveSetting`.
- [ ] AC-8: Manual check on Linux (WebKitGTK), as stated under US1.

### US3: Build and test commands succeed
**Acceptance Criteria:**
- [ ] AC-7: `bun test`, `bun run typecheck` and `bun run build:settings` succeed.

## Technical Requirements

### Functional Requirements
- **FR1:** Profiles list dragstart sets drag data. In `setupDragReorder` (`src-tauri/web-shared/settings/sections/profiles-section.ts`), the dragstart handler calls `e.dataTransfer.setData("application/x-emterm-profile-index", String(index))` with the dragged item's `data-index` value, and keeps setting `e.dataTransfer.effectAllowed = "move"`.
- **FR2:** SSH connection list dragstart sets drag data. In `setupSshDragReorder` (`src-tauri/web-shared/settings/sections/ssh-section.ts`), the dragstart handler calls `e.dataTransfer.setData("application/x-emterm-ssh-index", String(index))` with the dragged item's `data-index` value, and keeps setting `e.dataTransfer.effectAllowed = "move"`.
- **FR3:** No text/plain drag data. Neither dragstart handler sets a text/plain entry (or any type other than its list's custom MIME type).
- **FR4:** Drop handlers keep using in-memory dragIndex. Both drop handlers keep their current logic: they use the in-memory `dragIndex`, return when `dragIndex` is null, and never read data from `e.dataTransfer`. Reorder, `saveSetting` (`"profiles"` / `"ssh_connections"`) and `reRender` behave as they do today.
- **FR5:** Each list fixed in its own file, no shared helper. The fix is applied separately inside `setupDragReorder` and `setupSshDragReorder`. No shared drag-reorder helper is extracted.
- **FR6:** Regression tests per list. bun tests cover both lists separately. For each list they check (a) that dragstart on an item sets the list's custom MIME type to the item's index and sets effectAllowed `"move"`, and (b) that a dragstart -> drop sequence reorders the array, calls `saveSetting` with the reordered array, and calls `reRender`.

### Non-Functional Requirements
- **NFR1 - Change scope:** Only TypeScript under `src-tauri/web-shared/settings/sections/` (plus new test files) changes. The Rust/wry side (`src-tauri/src/webview_host/linux.rs`, `settings_launcher.rs`) stays unchanged.
- **NFR2 - Windows compatibility:** Drag reorder on Windows (WebView2) keeps working as it does today.
- **NFR3 - Build and test:** `bun test`, `bun run typecheck` and `bun run build:settings` all succeed.
- **NFR4 - Test environment:** The regression tests run under the existing bun test + happy-dom preload (`test-setup.ts`) and do not need a real WebKitGTK.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────┐
│ Settings WebView (wry; WebKitGTK / WebView2) │
├──────────────────────────────────────────────┤
│ settings sections (TypeScript)               │
│   profiles-section.ts: setupDragReorder      │
│   ssh-section.ts:      setupSshDragReorder   │
├──────────────────────────────────────────────┤
│ saveSetting / reRender (unchanged)           │
└──────────────────────────────────────────────┘
```

**Component Diagram:**
```
setupDragReorder     -- dragstart: setData("application/x-emterm-profile-index", String(index)), effectAllowed = "move"
                     -- drop: in-memory dragIndex -> reorder currentSettings.profiles -> saveSetting("profiles", ...) -> reRender
setupSshDragReorder  -- dragstart: setData("application/x-emterm-ssh-index", String(index)), effectAllowed = "move"
                     -- drop: in-memory dragIndex -> reorder currentSettings.ssh_connections -> saveSetting("ssh_connections", ...) -> reRender
```

### Data Flow

```
dragstart(item i) → dragIndex = i, dataTransfer.setData(<list MIME>, String(i)), effectAllowed = "move"
drop(item j)      → dragIndex null or j == i: return (no saveSetting)
                  → otherwise: move i to j → saveSetting(<key>, reordered) → reRender
```

### Assumptions

- **A1:** Root cause: both dragstart handlers set only effectAllowed and call no setData (profiles-section.ts:190-200, ssh-section.ts:365-375). WebKitGTK does not start a drag whose DataTransfer holds no data, so dragover and drop never fire on Linux.
- **A2:** No wry/GTK drag-drop handler in `src-tauri/src/webview_host/linux.rs` intercepts HTML5 drag inside the settings WebView. The only drag/drop match there is an unrelated IPC comment.
- **A3:** It is not yet verified that a drag carrying only a custom MIME type actually starts on the target WebKitGTK version. AC-8's manual Linux check covers this.
- **A4:** The SSH connection list bug has not been reproduced on Linux. It is assumed to fail the same way because its dragstart handler is identical.
- **A5:** `test-setup.ts` does not expose DragEvent/DataTransfer on globalThis. Tests take them from happy-dom's Window or attach a stub dataTransfer object to a dispatched Event.
- **A6:** `renderSshSection` calls `invoke("load_ssh_config_hosts")` only when `ssh_command_path` is non-empty, so SSH tests set `ssh_command_path` to `""` to avoid the Tauri IPC call.
- **A7:** Windows WebView2 already starts drags without setData. Adding setData does not change behavior there.

### API Design

N/A

### Database Schema

N/A. The reordered data are the existing `currentSettings.profiles` and `currentSettings.ssh_connections` arrays.

### Dependencies

**Internal Dependencies:**
- `saveSetting`: persists the reordered array under `"profiles"` / `"ssh_connections"` (unchanged).
- `reRender`: re-renders the section after a reorder (unchanged).
- `test-setup.ts`: bun test happy-dom preload used by the regression tests.

**External Dependencies:**
- None added.

### File Structure

```
src-tauri/web-shared/settings/sections/
├── profiles-section.ts   # setupDragReorder: dragstart sets application/x-emterm-profile-index
└── ssh-section.ts        # setupSshDragReorder: dragstart sets application/x-emterm-ssh-index
(new bun test files for both lists)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/settings-profile-drag-reorder/**`
- `test-docs/settings-profile-drag-reorder/**`

`feature-docs/settings-profile-drag-reorder/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/settings-profile-drag-reorder/**` covers `test-docs/settings-profile-drag-reorder/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (AC-1, AC-3; FR1, FR3): Render the Profiles section with 3 profiles, dispatch dragstart on the item at index 1 with a DataTransfer - `getData("application/x-emterm-profile-index") === "1"`, `effectAllowed === "move"`, and no text/plain entry.
- [ ] TS-2 (AC-2, AC-3; FR2, FR3): Render the SSH section with 3 connections and an empty `ssh_command_path`, dispatch dragstart on the item at index 1 - `getData("application/x-emterm-ssh-index") === "1"`, `effectAllowed === "move"`, and no text/plain entry.
- [ ] TS-3 (AC-4; FR4, FR6): Profiles: dragstart on index 0, then drop on index 2 - the order becomes [B, C, A], `saveSetting("profiles", ...)` is called once with that array, and `reRender` is called.
- [ ] TS-4 (AC-5; FR4, FR6): SSH: dragstart on index 0, then drop on index 2 - the order becomes [B, C, A], `saveSetting("ssh_connections", ...)` is called once with that array, and `reRender` is called.

### Integration Tests
- [ ] TS-6 (AC-7; NFR3): Run `bun test`, `bun run typecheck` and `bun run build:settings` - all succeed.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression
- [ ] TS-7 (AC-8; FR1, FR2, FR4, manual): On Linux, open settings > Profiles and drag an item to a new position. Check the order changes and is kept after reopening the settings panel. Repeat in settings > SSH connections.

### Edge Cases
- [ ] TS-5 (AC-6; FR4): For both lists, a drop without dragstart and a drop onto the dragged item itself each leave `saveSetting` uncalled.

### Performance Tests
- N/A

## Security Considerations

- **Authentication:** N/A
- **Authorization:** N/A
- **Input Validation:** N/A
- **Data Protection:** N/A
- **XSS Prevention:** N/A
- **SQL Injection Prevention:** N/A
- **CSRF Protection:** N/A

## Error Handling

### Error Codes

N/A

### Error Flow

```
drop with dragIndex null → return without saveSetting
drop onto the dragged item itself → no saveSetting
```

## Performance Optimization

N/A

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] `bun test`, `bun run typecheck` and `bun run build:settings` succeed (NFR3)
- [ ] Rust/wry side unchanged (NFR1)
- [ ] Code review is completed
- [ ] Manual Linux check (AC-8) passes for both lists

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. All requirements are resolved.

## Out of Scope

- Changes to the Rust/wry side (webview_host, settings_launcher)
- Extracting a shared drag-reorder helper
- Reading dataTransfer data in the drop handlers
- Adding a text/plain drag payload
- Drag visual styling or the dragging class
- Drag behavior of other lists (for example the read-only .ssh/config host list)

## Design Step

Skipped: Logic-only fix in dragstart handlers. No UI layout, visual, or interaction design change.

## References

- Requirements: `feature-docs/settings-profile-drag-reorder/REQUIREMENTS.md`
- `src-tauri/web-shared/settings/sections/profiles-section.ts`
- `src-tauri/web-shared/settings/sections/ssh-section.ts`
- `test-setup.ts`
