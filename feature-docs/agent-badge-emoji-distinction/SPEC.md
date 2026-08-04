# Feature: agent-badge-emoji-distinction

## Overview

The agent state badge shown in the tab bar and the mux sidebar currently
distinguishes `working` from `idle` with same-family colors and filled /
stroked circles only, which was found hard to tell apart during hands-on
verification (M-1, emterm-plugin-runtime-fixes; recorded in
POC-RESULTS.md). This feature makes `working` and `idle` distinguishable
at a glance by a means that does not rely on color or shape (such as a
color emoji), in both the tab bar and the sidebar. Requirement source:
`REQUIREMENTS.md` in this directory.

## Objectives

- Make `working` and `idle` distinguishable at a glance in the tab bar and
  sidebar agent state badges by a non-color-dependent means (such as a
  color emoji).
- Keep the tab bar and the sidebar badge presentations consistent with each
  other.
- Reuse the color-emoji rendering path already established in
  `status_bar.rs` rather than relying on egui's default glyph rendering.

## User Stories

### US1: Distinguish agent state in the tab bar

As an eMterm user, I want the tab bar agent state badge to show `working`
and `idle` differently by more than color and shape, so that I can tell the
two states apart at a glance.

**Acceptance Criteria:**
- [ ] `working` and `idle` are distinguishable at a glance in the tab bar badge
- [ ] Agent states other than `working` / `idle` (blocked / done, etc.) keep their current badge presentation

### US2: Distinguish agent state in the sidebar

As an eMterm user, I want the sidebar agent state badge to carry the same
`working` / `idle` distinction as the tab bar, so that both surfaces read
the same way.

**Acceptance Criteria:**
- [ ] The sidebar badge reflects the same `working` / `idle` distinction as the tab bar
- [ ] The tab bar and the sidebar presentations do not disagree

## Technical Requirements

### Functional Requirements

- **FR1 - Distinguish `working` / `idle` in the tab bar badge by a non-color-dependent means:**
  The tab bar agent state badge (`paint_agent_badge` in `tab_bar.rs`,
  currently `circle_filled` / `circle_stroke` only) must present `working`
  and `idle` so they are distinguishable at a glance by a means other than
  color and shape (such as a color emoji).
- **FR2 - Reflect the same distinction in the sidebar badge:**
  The mirrored badge implementation in `mux_sidebar.rs` must reflect the
  same `working` / `idle` visual distinction as FR1.
- **FR3 - Reuse the existing swash path for color emoji rendering:**
  Because egui's default glyph rasterizer (ab_glyph) cannot render color
  emoji, the badge rendering in the tab bar and the sidebar must reuse the
  path already established in `status_bar.rs`: `EmojiTextureCache`
  (`emoji_cache.rs`) + swash rasterization → `egui::Image` blit.

### Non-Functional Requirements

- **NFR1 - Consistency across the mirrored implementations:**
  `tab_bar.rs` and `mux_sidebar.rs` share the color logic
  (`agent_state_color` / `agent_badge_filled`) but implement their drawing
  functions independently. The change must be applied consistently to both
  files so their presentations do not disagree.
- **NFR2 - Preserve the GUI feature gate:**
  The affected modules are GUI-only (under `ui/`). The CLI build
  (`--no-default-features`) must keep compiling.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────┐
│  Tab bar badge (tab_bar.rs)         │
│  Sidebar badge (mux_sidebar.rs)     │  ← independent drawing functions
├─────────────────────────────────────┤
│  Shared color logic                 │
│  (agent_state_color /               │
│   agent_badge_filled)               │
├─────────────────────────────────────┤
│  Color emoji rendering path         │
│  EmojiTextureCache (emoji_cache.rs) │
│  + swash rasterization              │
├─────────────────────────────────────┤
│  egui::Image blit                   │
└─────────────────────────────────────┘
```

**Component Diagram:**
```
tab_bar.rs::paint_agent_badge ─┐
                               ├─ shared: agent_state_color / agent_badge_filled
mux_sidebar.rs (mirrored)    ──┘
                               └─ color emoji: emoji_cache.rs (EmojiTextureCache) + swash
                                  (path already used by status_bar.rs)
```

The concrete glyph assigned to each state, whether the new presentation
replaces or accompanies the existing circle badge, and its size and
placement are decided in the design step.

### Data Flow

```
Agent state (working / idle) → state→presentation selection logic
                             → EmojiTextureCache (swash rasterization)
                             → egui::Image blit into the badge rect
```

### API Design

Not applicable — this feature adds no API surface.

### Database Schema

Not applicable — this feature stores no data.

### Dependencies

**Internal Dependencies:**
- `tab_bar.rs` (`paint_agent_badge`): tab bar badge drawing — target of FR1
- `mux_sidebar.rs`: mirrored sidebar badge drawing — target of FR2
- Shared color logic (`agent_state_color` / `agent_badge_filled`): used by both drawing paths
- `emoji_cache.rs` (`EmojiTextureCache`): color emoji rasterization cache reused per FR3
- `status_bar.rs`: existing consumer of the same color emoji path, used as the reference implementation

**External Dependencies:**
- swash: glyph rasterization used by the existing color emoji path
- egui: UI layer providing `egui::Image` for the rasterized glyph blit

Note: the structural statements about `tab_bar.rs`, `mux_sidebar.rs`,
`status_bar.rs` and `emoji_cache.rs` were taken as given from the task
description; those sources were not read during requirements analysis.

### File Structure

```
src-tauri/src/ui/          # GUI-only modules (NFR2)
├── tab_bar.rs             # paint_agent_badge — FR1
├── mux_sidebar.rs         # mirrored badge — FR2
├── status_bar.rs          # reference: existing color emoji usage
└── emoji_cache.rs         # EmojiTextureCache — FR3
```

## Test Scenarios

### Unit Tests
- [ ] TS1: State → presentation (glyph / drawing mode) selection logic — the
      correct presentation is selected for `working` and for `idle`.
      Written as inline `#[cfg(test)] mod tests` per `test/README.md`, named
      `<subject>_<scenario>_<expected>`. Run:
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`

### Integration Tests
- [ ] TS2: CLI feature gate check — the CLI build still compiles. Run:
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`

### E2E Tests
**Existing E2E tests**: None — no E2E infrastructure exists
(`test/README.md`: neither `docker-compose.e2e.yml` nor `e2e-tests/`).
**Run command**: Not detected
- [ ] TS3: Manual on-device visual confirmation by the user — "distinguishable
      at a glance" is a subjective criterion, so the final check is the user
      inspecting both the tab bar and the sidebar in both the `working` and
      `idle` states. Investigation follows the project constraint that
      DevTools are unavailable and diagnosis goes through `emterm.log`.

### Edge Cases
- [ ] Agent states other than `working` / `idle` (blocked / done, etc.) keep
      their current badge presentation.

### Performance Tests
Not applicable — no performance requirement was raised for this feature.

## Security Considerations

Not applicable — this feature introduces no authentication, authorization,
external input, or data persistence surface.

## Error Handling

Not applicable — no error cases were raised for this feature.

## Performance Optimization

Not applicable — no performance goals were raised for this feature.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Security requirements are satisfied (none apply)
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] `working` and `idle` are distinguishable at a glance
- [ ] The distinction is reflected in both the tab bar and the sidebar badges

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

No requirement carries `status: tbd`; FR1, FR2, FR3, NFR1 and NFR2 are all
resolved. Items deliberately deferred to the design step (which glyph is
assigned to each state, replacement vs. coexistence with the existing
circle badge, size and placement) are recorded as assumptions in
`REQUIREMENTS.md` 14.2.

## Implementation Phases (if applicable)

Not applicable — this feature is delivered as a single change spanning
`tab_bar.rs` and `mux_sidebar.rs`.

## References

- Requirements document: `feature-docs/agent-badge-emoji-distinction/REQUIREMENTS.md`
- POC-RESULTS.md (emterm-plugin-runtime-fixes, hands-on verification M-1): record of `working` / `idle` being hard to tell apart
- `test/README.md`: test conventions and the absence of E2E infrastructure
