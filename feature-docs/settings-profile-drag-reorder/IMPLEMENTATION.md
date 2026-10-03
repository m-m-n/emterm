# Implementation Plan: settings-profile-drag-reorder

## Overview

The dragstart handlers of the settings panel's Profiles list and SSH connection
list put one drag-data entry on the event's DataTransfer, so that WebKitGTK
starts the drag on Linux (SPEC.md A1). Each list gets its own bun regression
test file. The work is split into two independent tasks, one per list.

## Technology Stack

- **Language / Framework**: TypeScript (vanilla, no framework) in the settings
  child WebView bundle
- **Test runner**: bun test with the existing happy-dom preload (`test-setup.ts`)
- **Key libraries**: happy-dom (existing devDependency) - DOM, event and
  DataTransfer emulation for the tests
- **New dependencies**: none. `project.license` (MIT) is unaffected and no
  license check is needed.

## Layer Structure

| Layer | Responsibility | Changed by |
|-------|----------------|------------|
| Settings sections (`src-tauri/web-shared/settings/sections/`) | Render one settings section into a panel and wire its DOM listeners through `SectionContext` | task0001 (Profiles), task0002 (SSH connections) |
| `SectionContext` (existing seam, `src-tauri/web-shared/settings/sections/types.ts`) | Gives a section its settings state, listener registration, persistence (`saveSetting`) and re-render (`reRender`) | Nobody (unchanged) |
| Rust / wry host (`src-tauri/src/`) | Hosts the settings WebView | Nobody (NFR1) |

Allowed dependency direction: section -> `SectionContext`. Tests -> the
section's exported render function + a stub `SectionContext`. The drag-reorder
setup functions stay module-private; tests reach them only by rendering the
section and dispatching events on the rendered list items.

## Shared Components

No shared drag-reorder code exists or is introduced (FR5). The two tasks share
only the existing context seam and one naming convention:

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `SectionContext` (existing, unchanged) | Injection seam between a section and the settings panel | Pre: `addContentListener(element, type, handler)` registers `handler` for `type` on `element`. Post: `saveSetting(key, value)` persists `value` under `key`; `reRender()` re-renders the panel. In tests it is replaced by a stub whose listener registration attaches the handler to the element, and whose `saveSetting` / `reRender` record every call (arguments and count). | task0001, task0002 |
| List drag-payload convention (a rule, not code) | Identifies the dragged item on the DataTransfer | Post (dragstart on a list item whose `data-index` is i, when the event has a DataTransfer): the DataTransfer holds exactly one entry; its type is `application/x-emterm-profile-index` for the Profiles list and `application/x-emterm-ssh-index` for the SSH connection list; its value is i as a decimal string; effectAllowed is `"move"`. No code inside the app reads this entry. | task0001, task0002 |

## Conventions

- **Drag payload**: exactly the one entry described above per list. No
  `text/plain` entry and no other type (FR3).
- **Drop handling**: drop handlers act only on the in-memory `dragIndex` set by
  a dragstart on the same list, return when it is null, and never read data
  from the drop event's DataTransfer (FR4). Reorder, `saveSetting`
  (`"profiles"` / `"ssh_connections"`) and `reRender` behave as today.
- **Error handling**: no new error path. The existing early returns (no list
  item resolved, `dragIndex` null, drop onto the dragged item itself) stay.
- **Logging**: none added.
- **Test files**: one file per list, colocated next to the section file and
  named after it with a `.test.ts` suffix. Each file builds its own settings
  fixture and stub `SectionContext`, following the style of the existing
  section tests in the same directory. No test fixture module is shared
  between the two files.
- **Test DOM environment**: the tests run under the existing `test-setup.ts`
  preload, which is left unchanged (NFR1, NFR4). DragEvent / DataTransfer are
  taken from the happy-dom Window class or replaced by a minimal stub object
  that records entries in insertion order and exposes the type list, the
  per-type value and effectAllowed (SPEC.md A5).
- **Event dispatch in tests**: listeners are registered on the list container
  and resolve the item from the event target, so every dispatched drag event
  bubbles and is dispatched on the list item element (or a descendant of it).
- **Test isolation**: every test renders a fresh section, so the in-memory
  `dragIndex` of one test never leaks into another.

## Cross-task Design Decisions

### D1: Drag data is added only at dragstart; the drop path is untouched

The fix is additive inside each dragstart handler: effectAllowed stays
`"move"` and one drag-data entry is added. dragover, dragend and drop keep
their current behavior (FR1, FR2, FR4). This keeps WebView2 behavior unchanged
(NFR2, SPEC.md A7).

Affected tasks: task0001, task0002.

### D2: Each list is fixed in its own function, with no shared helper

The change lives inside the existing per-list setup function of each section
file. No helper is extracted and no new function is exported (FR5).

Affected tasks: task0001, task0002.

### D3: One regression test file per list

Each list is tested in its own file through the section's exported render
function (FR6). Each file covers the dragstart payload, the dragstart -> drop
reorder, and the no-save drops (SPEC.md TS-1 to TS-5).

Affected tasks: task0001, task0002.

### D4: No Rust/wry change and no visual change

Only TypeScript under `src-tauri/web-shared/settings/sections/` changes
(NFR1). The `dragging` class and the list styles in the project design system
(`src-tauri/web-shared/styles.css`) are not touched.

Affected tasks: task0001, task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A drag carrying only a custom MIME type still does not start on the target WebKitGTK version (SPEC.md A3) | Low | High | Manual Linux check TS-7. A failure there goes back through verify; a different payload type would need a SPEC change because FR3 excludes `text/plain`. |
| happy-dom does not expose DragEvent / DataTransfer on globalThis (SPEC.md A5) | Medium | Low | Tests take the classes from the happy-dom Window or use the stub described under Conventions. |
| Rendering the SSH section triggers a Tauri IPC call (SPEC.md A6) | Low | Low | SSH tests set `ssh_command_path` to an empty string. |
| Drag behavior on Windows (WebView2) changes (NFR2, SPEC.md A7) | Low | Medium | Additive change only (D1); manual Windows check TS-9. |

## Open Questions

- None.
