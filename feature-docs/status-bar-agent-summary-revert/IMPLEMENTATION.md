# Implementation Plan: Revert the status bar's agent-status summary

## Overview

Remove the agent-status summary that `c0a20fa` added to the status bar's App
Line 1, restoring the status bar's pre-regression behavior and public API, and
bring the agent-status documentation back in line with the resulting UI.

## Technology Stack

- **Language**: Rust (edition per `src-tauri/Cargo.toml`) — no new dependency is
  introduced or removed by this feature, so no license check applies.
- **UI layer**: egui, drawn inside the wgpu render pipeline (`gui` feature).
- **Docs**: Markdown under `doc/`.

## Layer Structure

Three layers participate, and the dependency direction stays as it is today:

| Layer | Element | Role after this change |
|-------|---------|------------------------|
| Frame driver | `render::draw_terminal`, `window_host` | Call the status-bar widget with view-model + emoji resources only; no agent-status input |
| Widget | `ui::status_bar` | Renders the view model's three rows; knows nothing about agent status |
| State | `App` / `AgentStatusModel` | Still serves agent-status projections, but only to the tab bar and the mux sidebar |

The removal is strictly upward-facing: nothing below `App` changes, and the
tab-bar / sidebar consumers of `App`'s agent-status projections keep their
current contracts.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `ui::status_bar::visible_row_count` | Number of rows the bar paints this frame | Pre: a status-bar view model. Post: 0 when the bar is disabled; otherwise the count of rows whose own content resolves non-empty. Takes the view model as its only argument | task0001 |
| `ui::status_bar::panel_height_logical` | Logical height the frame driver must reserve | Pre: a status-bar view model. Post: row height × `visible_row_count` of that same view model. Takes the view model as its only argument | task0001 |
| `ui::status_bar::draw` | Paint the bar | Pre: an egui context, a view model, and optional emoji resources. Post: paints nothing when `visible_row_count` is 0; otherwise paints the visible rows in fixed top-to-bottom order (OSC, App Line 1, App Line 2). Takes exactly those three arguments | task0001 |
| `doc/AGENT-STATUS.md` | User-facing description of the agent-status surfaces | Post: names only the tab/window badges as the visual surfaces; makes no claim about a status-bar summary | task0002 |

task0002 depends on task0001's outcome only semantically (the documentation
describes the post-removal UI); it touches a disjoint file set, so the two run
in parallel without a code-level contract between them.

## Conventions

- **Restoration target**: the observable behavior and public signatures of
  `src-tauri/src/ui/status_bar.rs` as of commit `44113f4`, the last state before
  the regressing commit `c0a20fa`. Consult that commit when a detail of the
  pre-regression shape is unclear; it is the reference, not a patch to apply
  blindly.
- **No dead code**: anything that existed only to feed the summary is deleted
  together with it, rather than being left unreferenced.
- **Tests are removed only when their subject is removed**: summary-specific
  tests go away with the summary; every other status-bar test survives, with
  call sites adjusted for the restored signatures and assertions left intact.
- **Comment hygiene**: comments that explain the summary's presence or its
  interaction with App Line 1 visibility are removed along with the code they
  describe; comments describing surviving layout logic stay as they are.

## Cross-task Design Decisions

### D1 — Scope is the status bar only

The tab-bar badges, the mux sidebar badges, the pane-ID copy affordance, the
agent-status model, the OSC ingestion path, notifications, and the mux agent API
all remain exactly as they are. Only the status-bar surface and the query
method that existed solely to feed it are removed.

Affected tasks: task0001 (must not touch the retained surfaces), task0002 (must
keep describing them).

### D2 — Behavioral restoration, not a mechanical revert

`c0a20fa` mixed the summary in with a refactor that split the App row's
template-content drawing into its own helper. Restoring behavior and the public
signatures is required; preserving or folding back that internal split is left
to the implementer's judgment, provided the rendered result is unchanged for
rows that have template content.

Affected tasks: task0001.

### D3 — Documentation is corrected in the same feature

Leaving the agent-status document advertising a status-bar summary would
contradict the shipped UI. The correction is a separate task because it shares
no files with the code change and needs no build.

Affected tasks: task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A call site of a changed signature is missed | Low | Build failure | The build command in VERIFICATION.md covers both feature configurations; the two known call sites are named in task0001's scope |
| A helper becomes unreferenced and produces a warning | Medium | Noisy build | task0001's acceptance criteria require a warning-free build; the shared color helper stays referenced by the tab bar |
| A retained status-bar test is deleted along with the summary tests | Low | Silent loss of coverage | task0001 enumerates exactly which tests are removed; all others must still be present and passing |
| The removal accidentally changes App Line 2 or the OSC row | Low | Visible regression | Those rows' drawing paths are out of scope; their existing tests must keep passing unchanged |

## Open Questions

- [ ] Whether the user also wants the tab-bar / sidebar agent badges removed.
      Assumed no (SPEC.md Assumption A1); a follow-up task if that assumption is
      wrong.
