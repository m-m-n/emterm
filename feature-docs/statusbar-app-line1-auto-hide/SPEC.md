# Feature: Status Bar App Line 1 Auto-Hide

## Overview

When the status bar toggle (`statusbar_enabled`) is on, App Line 1 currently renders unconditionally, producing an empty row when no display content is configured. This feature makes App Line 1 auto-hide when its resolved content is empty (the same rule App Line 2 already uses), and collapses the status-bar panel entirely (height 0) when no row is visible.

## Objectives

- Eliminate the empty App Line 1 row when only the mux OSC row is wanted
- Unify App Line 1 / App Line 2 visibility rules
- Return the status-bar height to the terminal grid when nothing is visible

## User Stories

### US1: Mux-only status bar
As a user who only wants the mux OSC status row, I want the app rows to disappear when they have no content, so that no empty band is shown.

**Acceptance Criteria:**
- [ ] With `statusbar_enabled: true` and all rows empty, the bar occupies zero height
- [ ] When the mux daemon pushes OSC content, only the OSC row appears

### US2: Normal app-row usage unchanged
As a user with App Line 1 templates configured, I want the row to keep rendering as before.

**Acceptance Criteria:**
- [ ] App Line 1 renders whenever its resolved runs have content

## Technical Requirements

### Functional Requirements
- **FR1:** `visible_row_count` in `src-tauri/src/ui/status_bar.rs` treats App Line 1 as visible only when `view_model.app_line1.has_content()` is true (replacing the unconditional `app1_visible = true`). The same rule applies to the row-drawing branch in `draw`.
- **FR2:** When all three rows (OSC, App Line 1, App Line 2) are hidden, `visible_row_count` returns 0, `panel_height_logical` returns 0.0, and `draw` inserts no panel — the terminal grid regains the full height.
- **FR3:** Existing behavior is preserved: `enabled = false` short-circuit, App Line 2 `has_content()` auto-hide, and the OSC row `should_render()` rules (`forced_visible` overrides, content-based auto-show) are unchanged.

### Non-Functional Requirements
- **NFR1 - Compatibility:** The CLI-only build (`--no-default-features`) still compiles (the touched module is GUI-gated; no CLI-visible API changes).

## Implementation Approach

### Architecture

Single-file change in the egui draw layer:

- `src-tauri/src/ui/status_bar.rs`
  - `visible_row_count`: `app1_visible = view_model.app_line1.has_content()`
  - `draw`: same substitution for the local `app1_visible`

The view model (`status_bar/view_model.rs`) already exposes `AppRow::has_content()`; no runtime/template-engine changes are needed. Panel collapse (FR2) falls out of the existing `visible_rows == 0` early return and `panel_height_logical` multiplication.

Visibility is judged on **resolved content** (per-frame `RichTextRun`s), not on the raw settings strings — a configured template that resolves to an empty string (e.g. `{git_branch}` outside a repository) also hides the row.

### Dependencies

**Internal Dependencies:**
- `status_bar::view_model::AppRow::has_content()` — existing helper, reused as-is

**External Dependencies:** none

### File Structure

```
src-tauri/src/ui/status_bar.rs   # visibility logic + unit tests
```

## Test Scenarios

### Unit Tests
- [ ] TS-1: `visible_row_count` returns 0 when enabled and all rows empty
- [ ] TS-2: App Line 1 empty + OSC row content present → count 1, only OSC row drawn
- [ ] TS-3: App Line 1 has content → row counted and drawn (regression)
- [ ] TS-4: App Line 1 empty + App Line 2 has content → App Line 1 hidden, App Line 2 shown
- [ ] TS-5: existing status-bar tests (App Line 2 auto-hide, OSC rules, disabled short-circuit) pass unchanged

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] Edge 1: templates configured but resolve to empty runs → App Line 1 hidden
- [ ] Edge 2: `enabled = false` still returns 0 regardless of content

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All unit test scenarios pass
- [ ] `cargo check --no-default-features` passes
- [ ] Code review is completed

## Open Questions

None.

## References

- REQUIREMENTS.md: feature-docs/statusbar-app-line1-auto-hide/REQUIREMENTS.md
- Current visibility logic: src-tauri/src/ui/status_bar.rs (`visible_row_count`, `draw`)
