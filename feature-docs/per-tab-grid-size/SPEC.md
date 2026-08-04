# Feature: per-tab-grid-size

## Overview

Each tab holds its own PTY/core grid size instead of receiving a single app-wide grid size force-distributed to every tab. This removes the confirmed bug where tab switching propagates a resize to inactive tabs' PTYs and leaks XTWINOPS response fragments (e.g. `;51;171t816;1368t`) into tmux-hosted shells. Requirement source: `feature-docs/per-tab-grid-size/REQUIREMENTS.md`.

## Objectives

- Eliminate the confirmed bug where tab switching leaks XTWINOPS response fragments (e.g. `;51;171t816;1368t`) into tmux-hosted shells, caused by resize propagation to inactive tabs' PTYs.
- Make grid sizing structurally independent of mux-specific UI (mux status bar, sidebar Persistent mode), so present and future cell-area-changing UI cannot reintroduce the leak.

## User Stories

### US1: Switching tabs does not disturb the tabs left behind
As an eMterm user running mux, tmux and a plain shell in separate tabs, I want to switch tabs without any resize reaching the tabs I am leaving, so that no XTWINOPS response fragment appears in my tmux screen.

**Acceptance Criteria:**
- [ ] Each tab holds its own grid size independently.
- [ ] Tab switch, window resize, and UI visibility-state changes never change an inactive tab's PTY size.
- [ ] In a 3-tab mux/tmux/normal setup, switching mux→tmux leaks no XTWINOPS response fragment into the tmux screen.

### US2: Toggling cell-area-affecting UI stays local to the visible tab
As an eMterm user, I want toggling the mux status bar or the sidebar Persistent mode to affect only the tab I am looking at, so that hidden tabs are not resized behind my back.

**Acceptance Criteria:**
- [ ] Toggling the mux status bar or sidebar Persistent mode does not propagate resize to hidden tabs.
- [ ] Existing tests (`resize_clamps_to_the_wire_domain_before_resizing_the_core` etc.) pass or are updated with justification.

## Technical Requirements

### Functional Requirements

- **FR1 - Per-tab grid-size ownership:** Each tab holds its own PTY/core grid size independently; a single app-wide grid size is no longer force-distributed to all tabs (replacing the for-loop over `self.tabs` in `App::set_grid_size`, src-tauri/src/app.rs:4090-4122).
- **FR2 - Resize applies to the active tab only:** Window resize, cell-size changes, and UI visibility changes (mux status bar / sidebar Persistent ON-OFF) resize only the active tab's PTY and core; inactive tabs' PTYs receive no TIOCSWINSZ and no core resize.
- **FR3 - Reconcile size on tab activation:** When a tab becomes active and its stored grid size differs from the current display area, it is resized at that moment; if sizes match, no resize is issued.
- **FR4 - Mux pane Resize control-frame consistency:** For mux tabs, the per-pane `Resize` control frames (src-tauri/src/tabs.rs:3701-3711) follow the same per-tab rule: frames are sent only when that tab itself is resized (active resize or activation-time reconcile), keeping daemon-side pane PTYs consistent with the owning tab's size.
- **FR5 - Wire-domain clamp preserved:** `clamp_dims_to_wire_domain` continues to be applied identically on both the app-side size record and `Tab::resize` (src-tauri/src/tabs.rs:3637, src-tauri/src/app.rs:4104), so client and daemon still agree on accepted dimensions without a wire round trip.
- **FR6 - Per-tab reflow invalidation:** Width-change reflow invalidation currently done globally in `set_grid_size` (clearing app selection/pending anchor and each tab's prompt/fold marks, src-tauri/src/app.rs:4114-4135) is applied per-tab at the moment that tab is actually resized, so tabs that were not resized keep their trackers.

### Non-Functional Requirements

- **NFR1 - Root-cause independence:** Root-cause fix independent of mux-specific UI: correctness must not depend on which cell-area-affecting UI elements exist or their ON/OFF state.
- **NFR2 - Test-suite integrity:** Existing test suite passes or is justifiably updated — specifically `resize_clamps_to_the_wire_domain_before_resizing_the_core` (src-tauri/src/tabs.rs:6117), `spawn_shell_clamps_the_initial_core_to_the_wire_domain` (tabs.rs:6146), and the `set_grid_size` tests (src-tauri/src/app.rs:9600-9684).
- **NFR3 - Build compatibility:** GUI-only change: the CLI build (`--no-default-features`) still compiles (`cargo check --no-default-features` green).

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────┐
│  Window / UI (winit, sidebar,       │
│  mux status bar) — cell area        │
├─────────────────────────────────────┤
│  App (src-tauri/src/app.rs)         │
│  set_grid_size / tab activation     │
├─────────────────────────────────────┤
│  Tab (src-tauri/src/tabs.rs)        │
│  per-tab grid size + Tab::resize    │
├─────────────────────────────────────┤
│  term_core grid   │   PTY (TIOCSWINSZ)
├─────────────────────────────────────┤
│  mux daemon (pane Resize frames)    │
└─────────────────────────────────────┘
```

**Component Diagram:**
```
App
 ├─ owns the current display area (cell area → grid dims)
 ├─ resizes ONLY the active Tab (FR2)
 └─ on tab activation, compares the tab's stored dims with the
    display area and resizes only on mismatch (FR3)

Tab
 ├─ owns its grid size (FR1)
 ├─ Tab::resize applies clamp_dims_to_wire_domain (FR5)
 ├─ emits mux pane Resize control frames when itself resized (FR4)
 └─ performs its own width-change reflow invalidation (FR6)
```

### Data Flow

```
Resize trigger (window resize / cell-size change / UI visibility change)
  → App computes grid dims from the display area
  → clamp_dims_to_wire_domain (app-side record)          [FR5]
  → active Tab::resize → clamp_dims_to_wire_domain       [FR5]
      → term_core resize + PTY TIOCSWINSZ                [FR2]
      → per-tab reflow invalidation on width change      [FR6]
      → mux pane Resize control frame (mux tabs only)    [FR4]
  → inactive tabs: no PTY write, no core resize          [FR2]

Tab activation
  → compare the tab's stored dims with the display area  [FR3]
  → equal  → no resize issued
  → differ → the same resize path above, for that tab only
```

### API Design

No network or HTTP API surface is involved. The only cross-process interface touched is the mux daemon control channel:

#### Control frame: pane `Resize`

**Emission rule (FR4):**
```
Emitted only when the owning tab is itself resized:
  - the tab is active and a resize trigger fires, or
  - the tab is being activated and its stored dims differ from the display area
Never emitted for inactive mux tabs.
```

Source location: `src-tauri/src/tabs.rs:3701-3711`.

### Database Schema

Not applicable — no persistent schema change. The only state relocation is in-memory: the grid size moves from a single app-level record to per-`Tab` ownership (FR1).

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/app.rs`: `App::set_grid_size` (src-tauri/src/app.rs:4090-4122), the global reflow invalidation block (src-tauri/src/app.rs:4114-4135), the app-side `clamp_dims_to_wire_domain` call (src-tauri/src/app.rs:4104), and tab activation.
- `src-tauri/src/tabs.rs`: `Tab::resize` and its clamp (src-tauri/src/tabs.rs:3637), the mux pane `Resize` control-frame emission (src-tauri/src/tabs.rs:3701-3711).
- Renderer / hit-testing / `window_host::grid_size` consumers of the app-level `self.cell_size`, which are read as "the active tab's grid".

**External Dependencies:**
- None added. This is a GUI-side (`gui` feature) change only; the CLI build must remain compiling (NFR3).

### File Structure

```
src-tauri/src/
├── app.rs      # grid-size trigger points, tab activation reconcile (FR1-FR3, FR5, FR6)
└── tabs.rs     # per-tab grid size, Tab::resize, mux pane Resize frames (FR1, FR4, FR5, FR6)
```

## Test Scenarios

### Unit Tests
- [ ] **TS1** (app.rs) — covers FR1, FR2: after a grid-size change with multiple tabs, only the active tab's core dims change; an inactive tab's core reports its prior dims.
- [ ] **TS2** (app.rs) — covers FR3: activating a tab whose stored size differs from the current display area resizes it exactly then; activating a matching-size tab issues no resize.
- [ ] **TS3** (tabs.rs) — covers FR5, NFR2: wire-domain clamp still holds on both the initial spawn and every later resize (existing tests, updated as needed).
- [ ] **TS4** (tabs.rs/app.rs) — covers FR4: mux `Resize` control frames are emitted only for the tab being resized, never for inactive mux tabs.
- [ ] **TS5** — covers FR6: width-change reflow invalidation (selection / prompt / fold clearing) fires only for the tab actually resized.

### Integration Tests
- [ ] **TS6** (regression) — covers NFR2: full `--lib` suite via `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` (tabs.rs replay tests re-run with `-- --test-threads=1` if they flake). Rust tests for this project live under `--lib`.

### E2E Tests
**Existing E2E tests**: None — the project has no E2E infrastructure (test/README.md).
**Run command**: Not detected.
- [ ] **TS7** (manual, no E2E infra) — covers FR1, FR2, NFR1: reproduce the 2026-08-03 scenario — sidebar Persistent ON and mux status bar ON, 3 tabs (mux/tmux/normal), switch tabs, confirm no `;R;Ct` fragments appear in the tmux shell.

### Edge Cases
- [ ] Activation with matching size: a tab whose stored size already equals the display area is activated and no resize is issued at all (FR3, covered by TS2).
- [ ] Dimensions outside the wire domain: both the initial spawn and later resizes clamp identically on the app-side record and in `Tab::resize`, so client and daemon agree without a wire round trip (FR5, covered by TS3).
- [ ] Untouched tabs keep their trackers: a tab that was not resized retains its selection / pending anchor / prompt / fold marks across another tab's width change (FR6, covered by TS5).

### Performance Tests
Not applicable — no performance requirement is specified for this feature.

## Security Considerations

Not applicable — this change has no authentication, authorization, network input, or persistence surface. It relocates in-process grid-size state and restricts which PTYs receive TIOCSWINSZ.

## Error Handling

No new error codes or error flows are introduced. The behavioural rule that replaces the previous unconditional propagation is: when a tab's stored size matches the display area, no resize is issued (FR3); inactive tabs receive no PTY write and no core resize (FR2).

## Performance Optimization

Not applicable — no performance goals are specified. Restricting resize to the active tab incidentally removes per-tab PTY writes on every grid-size change, but this is a correctness change, not a performance target.

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and tested.
- [ ] All test scenarios (TS1-TS7) pass, TS7 being verified manually.
- [ ] Each tab holds its own grid size independently.
- [ ] Tab switch, window resize, and UI visibility-state changes never change an inactive tab's PTY size.
- [ ] In a 3-tab mux/tmux/normal setup, switching mux→tmux leaks no XTWINOPS response fragment into the tmux screen.
- [ ] Toggling the mux status bar or sidebar Persistent mode does not propagate resize to hidden tabs.
- [ ] Existing tests (`resize_clamps_to_the_wire_domain_before_resizing_the_core` etc.) pass or are updated with justification.
- [ ] The CLI build stays green: `cargo check --no-default-features` (NFR3).
- [ ] Code review is completed.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. Every functional requirement (FR1-FR6) is `resolved`.

## Assumptions

- Renderer / hit-testing / `window_host::grid_size` consumers of the app-level `self.cell_size` are to be read as "the active tab's grid"; per-tab storage keeps that semantic while inactive tabs may lag until activation.
- Rendering-consistency polish for TUIs running in inactive tabs (top/glances) is out of scope per the task description (follow-up if needed).
- Sidebar UI itself and the mux status bar's removal are out of scope (the latter is a separate task).

## Implementation Phases (if applicable)

Not applicable — this is a single-scope bug fix delivered as one change across `src-tauri/src/app.rs` and `src-tauri/src/tabs.rs`.

## References

- Requirements document (Japanese): feature-docs/per-tab-grid-size/REQUIREMENTS.md
- Current app-wide distribution loop: src-tauri/src/app.rs:4090-4122
- Current global reflow invalidation: src-tauri/src/app.rs:4114-4135
- App-side wire-domain clamp: src-tauri/src/app.rs:4104
- `Tab::resize` wire-domain clamp: src-tauri/src/tabs.rs:3637
- Mux pane `Resize` control frames: src-tauri/src/tabs.rs:3701-3711
- Existing tests: src-tauri/src/tabs.rs:6117, src-tauri/src/tabs.rs:6146, src-tauri/src/app.rs:9600-9684
- E2E infrastructure absence: test/README.md
- Design step: skipped — pure internal resize-plumbing bug fix in the Rust terminal stack (app.rs/tabs.rs); no user-facing visual surface, no new design-token involvement.
