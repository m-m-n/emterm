# Implementation Plan: Remove the pane-ID copy button from the mux sidebar

## Overview

Remove the mux sidebar's copy-to-clipboard affordance together with every
piece of code that exists only to serve it, across the sidebar widget, the
frame-event plumbing, and the post-frame application site.

## Technology Stack

- **Language**: Rust (existing `emterm` crate — no new dependency)
- **UI layer**: egui in-process widgets (`src-tauri/src/ui/`)
- **New dependencies**: none, therefore no license check is required
  (`project.license: MIT` stays as-is)

## Layer Structure

The affected chain is one direction only, top to bottom:

1. **Widget layer** (`ui::mux_sidebar`) — draws rows and reports what the
   user interacted with as a per-frame outcome value. Never touches the OS
   clipboard, never sends mux messages.
2. **Frame aggregation layer** (`render`) — collects widget outcomes for one
   frame into a single frame-event value.
3. **Host layer** (`window_host`) — applies frame events against `App` and
   OS resources after the frame.

The removal deletes one channel that runs through all three layers. The
direction and responsibilities of the layers themselves do not change.

## Shared Components

Only one task exists, so there is no cross-task component contract to pin.
The two contracts the task changes are recorded here because they are the
seams between the three layers above:

| Component | Responsibility | Contract after this change | Used by tasks |
|-----------|----------------|----------------------------|---------------|
| Sidebar draw entry point | Draw the sidebar for a placement and report this frame's interaction | Takes the drawing context, the entry list, the placement, and the panel width. Reports at most one window-switch index and nothing else. No locale input. | task0001 |
| Frame event value | Carry one frame's widget outcomes to the host layer | Carries no sidebar-originated clipboard request. Its "any event fired" predicate enumerates only the remaining fields. | task0001 |

## Conventions

- Naming, comment style, and module layout follow the surrounding code.
- Comments that reference the removed affordance (including its originating
  task marker) are removed along with the code they describe; comments that
  describe surviving behavior are kept and, where they mention the icon's
  reserved region, corrected to describe the row without it.
- Existing tests are the specification of the surviving behavior: they may be
  updated only where they assert the removed affordance.

## Cross-task Design Decisions

### D1: Remove the whole channel, not just its visible end

The sidebar icon is the only producer of the clipboard request that flows
through the frame-event value, and the host layer's application site is its
only consumer. Removing only the icon would leave a permanently-unfed field
and an unreachable application site. Both are removed so no dead channel
remains (SPEC.md FR3, NFR3, Assumption A1).

### D2: Preserve the pane-ID state that has another consumer

The application-level map from internal pane id to daemon-minted public pane
id keeps its accessor and its tests: agent-notification rate limiting still
keys off it. Only the sidebar's consumption of it is removed (SPEC.md NFR4,
Assumption A3).

### D3: Drop the locale input from the sidebar widget

The widget's locale input existed solely to localize the removed icon's hover
text. It is removed from the widget's function signatures and from both call
sites, rather than left unused (SPEC.md FR4, NFR3, Assumption A2).

### D4: The row stays a single hit target

After the removal the row registers no nested interaction region. A click
anywhere in the row — including the area the icon occupied — reports the
window switch, and the suppression flag that used to stop a switch when the
icon was clicked disappears with it (SPEC.md FR2).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Removing the locale input misses a call site and breaks the build | Medium | Low | The build command surfaces it immediately; both production call sites and all test call sites are inside the task's file set |
| The name column's width calculation regresses when the reserved icon region is dropped | Medium | Medium | Keep the existing row-layout tests asserting number/badge/name positions; they fail if the name origin or the number column moves |
| A surviving comment or test name still claims the copy affordance exists | Medium | Low | Search the touched files for references to the affordance before finishing |
| Removing the frame-event field silently changes the "any event fired" predicate | Low | Medium | Cover the predicate with a test over a default value and over each remaining field |

## Open Questions

- [ ] None. The decisions taken without user confirmation are recorded as
      Assumptions A1–A6 in SPEC.md.
