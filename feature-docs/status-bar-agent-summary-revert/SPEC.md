# Feature: Revert the status bar's agent-status summary

## Overview

The `mux-agent-status-api` feature's task0006 (commit `c0a20fa`) added a
right-aligned blocked/working/done/idle dot+count summary to the status bar's
App Line 1, and made the summary's presence count as App Line 1 content for
visibility purposes. This regressed the status bar: a row that should stay
hidden when the user configured no App Line 1 template now appears as soon as
any pane reports an agent state. This feature restores the status bar to its
pre-`c0a20fa` behavior and appearance. The status bar displays no agent state
at all; tab-bar and mux-sidebar badges are untouched.

## Objectives

- Remove the agent-status summary from the status bar entirely.
- Restore App Line 1's visibility rule to "resolved template content only".
- Remove the code that existed solely to feed the summary, leaving no dead code.

## User Stories

### US1: The status bar's row count is decided only by my templates
As an eMterm user, I want the status bar's visible rows to depend only on the
templates I configured, so that agent activity never changes the terminal's
layout behind my back.

**Acceptance Criteria:**
- [ ] With no App Line 1 template configured, App Line 1 stays hidden even when
      panes report agent states.
- [ ] The status bar's visible row count is identical before and after an agent
      state transition.

### US2: The status bar shows no agent state
As an eMterm user, I want no agent-state indicator in the status bar, so that
the bar looks exactly as it did before the agent-status feature landed.

**Acceptance Criteria:**
- [ ] No dot, count, or agent-state-colored element is painted in any status-bar
      row.
- [ ] App Line 1's right section can use the full row width again (no space
      reserved for a summary).

## Technical Requirements

### Functional Requirements

- **FR1:** `src-tauri/src/ui/status_bar.rs` paints no agent-status summary.
  `draw_agent_summary` and its call site in the App-row drawing path are
  removed; an App row renders only its template-derived left/right sections.
- **FR2:** App Line 1's visibility condition is `view_model.app_line1.has_content()`
  alone. The `|| has_agent_summary` disjunction is removed from both
  `visible_row_count` and `draw`.
- **FR3:** The public status-bar API returns to its pre-`c0a20fa` signatures:
  `visible_row_count(&StatusBarViewModel) -> u32`,
  `panel_height_logical(&StatusBarViewModel) -> f32`, and
  `draw(&egui::Context, &StatusBarViewModel, Option<&EmojiResources<'_>>)`.
  The internal `draw_app_row` helper loses its `agent_summary` parameter.
- **FR4:** All summary-only declarations in `status_bar.rs` are deleted: the
  `AgentSummarySegment` struct, the `agent_summary_segments` function, the
  `AGENT_SUMMARY_FONT_SIZE` / `AGENT_SUMMARY_DOT_DIAMETER` /
  `AGENT_SUMMARY_DOT_GAP` / `AGENT_SUMMARY_SEGMENT_GAP` /
  `AGENT_SUMMARY_EDGE_GAP` constants, and the imports that only they used
  (`egui::Rect`, `crate::agent_status_model::Counts`,
  `crate::ui::tab_bar::agent_state_color`).
- **FR5:** `src-tauri/src/render/mod.rs` no longer computes an agent summary and
  calls `status_bar::draw` with three arguments.
- **FR6:** `src-tauri/src/window_host.rs` no longer computes `has_agent_summary`
  and calls `status_bar::panel_height_logical` with one argument.
- **FR7:** `App::agent_status_counts()` in `src-tauri/src/app.rs` is removed
  together with its `agent_status_counts_passthrough` unit test, since its only
  callers were FR5's and FR6's summary paths. The other agent-status query
  methods used by the tab bar and the mux sidebar
  (`agent_status_badge_for`, `agent_status_pane_badge`, and the pane-ID map)
  are retained unchanged.
- **FR8:** The summary-specific unit tests in `status_bar.rs` are removed:
  `agent_summary_segments_empty_counts_yields_empty_list`,
  `agent_summary_segments_orders_blocked_working_done_idle_and_omits_zeros`,
  `agent_summary_segments_omits_zero_count_groups`,
  `agent_summary_segments_colors_match_agent_state_color`,
  `visible_row_count_agent_summary_makes_app_line1_visible_with_no_template`,
  `draw_with_empty_agent_summary_paints_no_extra_dot_and_matches_baseline`,
  `draw_with_agent_summary_shows_the_count_text_and_is_absent_when_empty`,
  `draw_agent_summary_alone_reserves_a_visible_row_when_app_line1_template_is_empty`.
  Every other `status_bar.rs` test is retained; call sites are adjusted for the
  FR3 signatures without weakening assertions.
- **FR9:** `doc/AGENT-STATUS.md` no longer advertises a status-bar summary. Its
  overview and its security note are reworded to name only the tab/window
  badges as the visual surfaces.

### Non-Functional Requirements

- **NFR1 - Performance:** Per-frame agent-status aggregation for the status bar
  is gone; status-bar drawing cost is at most what it was before `c0a20fa`.
- **NFR2 - Compatibility:** No change to `settings.json` schema, the OSC 777
  `agent-status` protocol, or the mux IPC protocol.
- **NFR3 - Maintainability:** The build introduces no new warnings (no unused
  imports, no dead code) for either the default feature set or
  `--no-default-features`.
- **NFR4 - Scope containment:** `tab_bar.rs`, `mux_sidebar.rs`,
  `agent_status.rs`, `agent_status_model.rs`, `notifications.rs`, and the mux
  agent API are not modified.

## Implementation Approach

### Architecture

The change is confined to the GUI presentation layer:

```
┌──────────────────────────────────────────────┐
│ render::draw_terminal   (FR5: drop summary)  │
│ window_host             (FR6: drop summary)  │
├──────────────────────────────────────────────┤
│ ui::status_bar          (FR1-FR4, FR8)       │
├──────────────────────────────────────────────┤
│ App::agent_status_counts (FR7: removed)      │
│ AgentStatusModel         (unchanged)         │
└──────────────────────────────────────────────┘
```

### Data Flow

Before:

```
AgentStatusModel → App::agent_status_counts() → agent_summary_segments()
                 → status_bar::draw(..., &segments) → draw_agent_summary()
```

After:

```
AgentStatusModel → App::agent_status_badge_for() → tab_bar / mux_sidebar badges
StatusBarViewModel → status_bar::draw(ctx, vm, emoji)   (no agent input)
```

### Reference Point

`status_bar.rs`'s last pre-regression state is commit `44113f4`
("task0001: auto-hide App Line 1 on resolved content"). That commit's
`visible_row_count` / `panel_height_logical` / `draw` / `draw_app_row` bodies are
the target shape for FR1-FR3. The restoration is behavioral, not a blind
`git revert`: `c0a20fa` also split `draw_app_row_content` out of `draw_app_row`,
and that split may be kept or folded back as long as the rendered result and the
public signatures match the reference point.

### Dependencies

**Internal Dependencies:**
- `crate::status_bar` (view model): unchanged, consumed as before.
- `crate::ui::tab_bar::agent_state_color`: stays public for the tab bar's own
  badge; only `status_bar.rs`'s import of it is dropped.

**External Dependencies:** none added or removed.

### File Structure

```
src-tauri/src/
├── ui/status_bar.rs     # FR1-FR4, FR8
├── render/mod.rs        # FR5
├── window_host.rs       # FR6
└── app.rs               # FR7
doc/AGENT-STATUS.md      # FR9
```

## Test Scenarios

### Unit Tests
- [ ] `visible_row_count` returns 0 when the view model is disabled.
- [ ] `visible_row_count` counts only rows whose template content resolves
      non-empty (App Line 1 hidden with an empty template regardless of any
      agent state in the model).
- [ ] `panel_height_logical` equals `ROW_HEIGHT * visible_row_count`.
- [ ] Existing OSC-row and App-row rendering tests pass unchanged apart from the
      FR3 signature adjustment.
- [ ] `app.rs` compiles and its remaining agent-status tests
      (badge aggregation, pane visibility, notification gating) pass.

### Integration Tests
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
      passes with no new failures relative to the base commit.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
      succeeds with no new warnings.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      succeeds (CLI-only build unaffected).

### E2E Tests
**Existing E2E tests**: None (no e2e-tests/, playwright, or cypress config in the repo).
**Run command**: Not detected.

The status bar is drawn by the native wgpu+egui pipeline and has no automatable
UI harness; visual confirmation is left to manual inspection by the user
(see `.claude/rules/debugging-constraints.md`).

### Edge Cases
- [ ] Every row hidden: the status-bar panel is not inserted at all and
      `panel_height_logical` is `0.0`.
- [ ] Agent states present but no template configured: no status-bar row appears
      (this is the regression being fixed).
- [ ] App Line 2 configured while App Line 1 is empty: App Line 2 still renders
      and occupies the single visible row.

## Security Considerations

- **Input Validation:** unchanged; this feature removes a rendering path and
  adds no new input surface.
- **Data Protection:** removing the summary narrows, not widens, what a forged
  agent-status report can influence in the UI. `doc/AGENT-STATUS.md`'s security
  note is updated accordingly (FR9).

## Error Handling

No runtime error paths are introduced or removed. The only failure mode is a
compile error from a missed call site, which the build commands in Test
Scenarios catch.

## Success Criteria

- [ ] All functional requirements FR1-FR9 are implemented.
- [ ] All test scenarios pass.
- [ ] No agent-state element is rendered in the status bar.
- [ ] Tab-bar and mux-sidebar agent badges continue to work.
- [ ] No new build warnings in either feature configuration.

## Assumptions

Recorded because this feature ran in batch mode with no user dialogue. Codex was
unavailable (`command -v codex` found nothing), so the consultation loop was
skipped and every point below was decided by Claude from the task text and the
codebase.

- **A1 — Scope is the status bar only.** The task names only the status bar
  ("ステータスバーには何も表示しなくて良い"), so the tab-bar badges and the mux
  sidebar badges added by the same commit are retained. If the user meant to
  remove all agent-state visuals, that is a follow-up task.
- **A2 — "Pre-introduction state" means behavioral restoration, not a literal
  `git revert`.** The target is the observable behavior and public API of
  `status_bar.rs` at `44113f4`; internal refactors introduced by `c0a20fa` may
  remain where they do not change behavior.
- **A3 — Summary-only code is deleted rather than left unused.** This includes
  `App::agent_status_counts()`, whose only callers were the two summary sites.
- **A4 — No configuration toggle is added.** The task says the status bar need
  not display anything, so the summary is removed outright instead of being
  made opt-in.
- **A5 — `doc/AGENT-STATUS.md` is in scope.** Leaving it advertising a
  status-bar summary that no longer exists would be a documented contradiction.

## References

- Notion task: https://www.notion.so/3a93509ec8ee818cb58ec9cfdab64a49
- Regressing commit: `c0a20fa` — task0006: add agent-status tab/window badges, status-bar summary, pane-ID copy
- Reference commit: `44113f4` — task0001: auto-hide App Line 1 on resolved content
- `doc/AGENT-STATUS.md`
- `feature-docs/status-bar-agent-summary-revert/REQUIREMENTS.md`
