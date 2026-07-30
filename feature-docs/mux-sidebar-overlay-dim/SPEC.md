# Feature: mux Window List Overlay Auto-Dim

## Overview

The mux window list (sidebar) gains an automatic opacity behavior when shown in overlay mode: it is
fully legible while the user is interacting with it (pointer hovering the card, or right after a mux
window switch) and fades to a low opacity otherwise so the terminal text underneath stays readable.
Overlay also becomes the default display mode, replacing the persistent right-hand panel.

## Objectives

- Make overlay the default display mode for the mux window list, open on startup.
- Keep the card at its current (opaque) look while the user is looking at or acting on it.
- Drop the whole card — background, text, badges, shadow — to a low opacity when idle, so terminal
  output behind it can be read without toggling the sidebar off.
- Keep the existing show/hide toggle intact.

## User Stories

### US1: Switch mux windows by keyboard and read the list

As a mux user, I want the overlay window list to become fully legible the moment I switch windows and
to get out of the way shortly afterwards, so that I can confirm where I landed without hiding the
sidebar by hand.

**Acceptance Criteria:**

- [ ] Pressing the next/prev/select-window binding renders the card at the bright opacity on the very
      next frame.
- [ ] With no further interaction, the card is at the dim opacity 3 seconds after that switch.
- [ ] A second switch within the 3 seconds restarts the countdown from the later switch.

### US2: Inspect or click the list with the mouse

As a mux user, I want the card to brighten while my pointer is over it, so that I can read and click
rows.

**Acceptance Criteria:**

- [ ] Moving the pointer into the overlay card rect brightens the card.
- [ ] Moving the pointer out of the rect dims the card, unless the post-switch hold window is still
      active.
- [ ] Clicking a row still switches windows, and the click is treated as a switch for the hold window.

### US3: Read terminal output behind the sidebar

As a mux user, I want the idle overlay to be transparent enough to read the terminal characters it
covers, so that I never need to toggle it off just to read output.

**Acceptance Criteria:**

- [ ] With no hover and no switch in the last 3 seconds, terminal glyphs under the card are legible.
- [ ] The window list itself is still discernible enough to tell which entries exist.
- [ ] No continuous repaint occurs once the card has settled at the dim opacity.

## Technical Requirements

### Functional Requirements

- **FR1:** The persisted setting `mux.window_sidebar_overlay` defaults to `true`. This changes the
  `serde` default in `crates/app_settings` and the mirrored default in the app-side settings struct.
  A `settings.json` that stores `false` explicitly keeps the persistent panel.
- **FR2:** The runtime overlay open flag (`App::mux_sidebar_overlay_open`) starts in the open state, so
  that a default-configured session shows the window list without a toggle press.
- **FR3:** While the pointer is inside the overlay card rect, the card renders at the bright opacity.
  The hover hit test uses the same card rect that drives click routing.
- **FR4:** A mux window switch sets a "last switch" timestamp and renders the card at the bright
  opacity. Both keyboard switches (next / prev / select-by-digit) and sidebar row clicks count as
  switches. A later switch overwrites the timestamp.
- **FR5:** When the pointer is outside the card rect AND at least `OVERLAY_BRIGHT_HOLD` has elapsed
  since the last switch, the card renders at the dim opacity. With no switch recorded yet (fresh
  start), the card is dim.
- **FR6:** The opacity applies to the entire card content — fill, row text, badges, icons, and the
  elevation shadow — not to the background fill alone. Dimming only the fill would leave opaque text
  over the terminal and defeat the purpose.
- **FR7:** The transition into the dim state is a fade over `OVERLAY_DIM_FADE`; the transition into
  the bright state is immediate (no fade-in), so interaction feels responsive.
- **FR8:** Frame scheduling covers every transition:
  - The frame-skip gate (`window_host::should_skip_frame`, fed by the `overlay_work` expression built
    in `window_host.rs`) treats "dim fade in progress" and "bright-hold deadline reached" as work, so
    intermediate frames are not dropped on a clean grid.
  - The wait deadline computation (`next_wait_deadline` / `control_flow_for`) includes the bright-hold
    expiry and the next fade frame, using the `ControlFlow::WaitUntil` path (`ctx.request_repaint_after`
    does not reach winit in release builds).
  - A change in the pointer-inside-card-rect predicate triggers a redraw even though
    `has_actionable_egui_input` deliberately ignores bare `PointerMoved`.
- **FR9:** The toggle action `PrefixAction::ToggleWindowSidebar` keeps its current behavior and default
  binding (prefix then `Ctrl+W`): it opens and closes the overlay. Nothing in this feature depends on
  the sidebar being open — when it is closed there is no card to dim.
- **FR10:** Persistent (non-overlay) mode is untouched: it stays fully opaque, and none of the hover /
  hold / fade state affects it.

**Concrete values** (single definition site, `src-tauri/src/ui/mux_sidebar.rs` unless the state lives
on `App`):

| Name | Value | Meaning |
| --- | --- | --- |
| `OVERLAY_FILL_ALPHA` | `0.92` (unchanged) | state-layer alpha of the card fill in the bright state |
| `OVERLAY_IDLE_OPACITY` | `0.35` | whole-card opacity multiplier in the dim state |
| `OVERLAY_DIM_FADE` | `200 ms` | duration of the bright → dim fade |
| `OVERLAY_BRIGHT_HOLD` | `3000 ms` | how long after a switch the card stays bright |

The bright state keeps the whole-card multiplier at `1.0`, so its rendered appearance is byte-identical
to today's overlay (`state_layer(surface_container_high(), 0.92)` fill, 12px radius, elevation shadow).

### Non-Functional Requirements

- **NFR1 - Performance:** Once the fade has completed and neither hover nor hold is active, no wait
  deadline is armed for this feature and no repaint is requested. Hover tracking and the fade must not
  cost more than the existing visual-bell fade path.
- **NFR2 - Maintainability:** Opacity values and durations are named constants defined once. Colors are
  produced through the existing `md3` helpers — `mux_sidebar.rs` must not gain raw color constructors,
  which an existing test (`ac5_no_hardcoded_color_constructors_in_module_source`) forbids.
- **NFR3 - Compatibility:** Behavior is identical on Linux and Windows; no platform-gated logic is
  introduced. Existing `settings.json` files that pin `window_sidebar_overlay: false` keep the
  persistent panel. The existing test that pins the 12px radius and the 0.92 fill alpha
  (`overlay_card_has_12px_corner_radius_and_92_percent_alpha_surface_container_high`) keeps passing.
- **NFR4 - Usability:** At the dim opacity, terminal glyphs behind the card are readable, and the card
  is still visible enough to show that a window list is there.

## Implementation Approach

### Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ winit layer (window_host.rs)                                 │
│  · cursor_pos / PointerMoved → hover predicate               │
│  · overlay_work  → should_skip_frame                         │
│  · next_wait_deadline → ControlFlow::WaitUntil               │
├──────────────────────────────────────────────────────────────┤
│ App state (app.rs)                                           │
│  · mux_sidebar_overlay_open (init: true)                     │
│  · last_window_switch: Option<Instant>                       │
│  · sidebar hover state + fade origin                         │
│  · sidebar_overlay_opacity() -> f32                          │
├──────────────────────────────────────────────────────────────┤
│ egui draw (render/mod.rs → ui/mux_sidebar.rs)                 │
│  · draw_overlay(..., opacity)                                │
│  · whole-card opacity multiplier applied to the Area/Ui      │
├──────────────────────────────────────────────────────────────┤
│ Settings (crates/app_settings, src-tauri/src/settings.rs)     │
│  · window_sidebar_overlay default → true                     │
└──────────────────────────────────────────────────────────────┘
```

**Component Diagram:**

- `crates/app_settings::MuxSettings::window_sidebar_overlay` — persisted default flips to `true`.
- `App` — owns the new runtime state (last-switch instant, hover flag, fade bookkeeping) and exposes a
  single resolver that returns the current whole-card opacity plus whether animation is still running.
- `App::dispatch_mux_action` — records the switch timestamp for `NextWindow` / `PrevWindow` /
  `SelectWindow`; the `TabEvent::MuxSwitch` path (row click) records it too.
- `render/mod.rs` — passes the resolved opacity into `mux_sidebar::draw`.
- `ui/mux_sidebar.rs::draw_overlay` — applies the opacity to the whole card; `draw_persistent` ignores it.
- `window_host.rs` — feeds the hover predicate, extends `overlay_work`, and arms the wait deadline.

### Data Flow

```
PointerMoved ──► cursor_pos ──► point_in_sidebar(card rect) ──┐
                                                             ├─► App opacity resolver ─► draw_overlay(opacity)
key/click switch ──► last_window_switch = Instant::now() ─────┘            │
                                                                          ├─► needs_repaint / overlay_work
                                                                          └─► next deadline (hold expiry, fade step)
```

Opacity resolution (pure function of state, no side effects):

```
bright  = hover || last_switch.map(|t| t.elapsed() < OVERLAY_BRIGHT_HOLD).unwrap_or(false)
target  = if bright { 1.0 } else { OVERLAY_IDLE_OPACITY }
current = bright ? target                      // immediate brighten
                 : lerp(bright_value, target, clamp01(fade_elapsed / OVERLAY_DIM_FADE))
```

### API Design

No network or IPC surface. Internal signatures that change:

- `ui::mux_sidebar::draw(ctx, entries, placement, width, locale)` gains an opacity parameter (or a
  small struct carrying it) that only the overlay branch consumes.
- `App` gains a resolver returning the current opacity and whether a further frame is needed, plus a
  hover setter called from the winit pointer path.

### Database Schema

Not applicable — no persisted data is added. `settings.json` gains no new key.

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/ui/mux_sidebar.rs`: overlay card geometry, fill, rows, and its existing tests.
- `src-tauri/src/app.rs`: `MuxSidebarVisibility`, `mux_sidebar_overlay_open`, `dispatch_mux_action`,
  and the existing `Instant`-based fade precedent (`visual_bell_progress`).
- `src-tauri/src/window_host.rs`: `cursor_pos`, `point_in_sidebar` call sites, `should_skip_frame`,
  `overlay_work`, `next_wait_deadline`, `control_flow_for`, `wakeup::wake`.
- `src-tauri/src/render/mod.rs`: overlay draw call site.
- `crates/app_settings/src/settings.rs` and `src-tauri/src/settings.rs`: the default flip.
- `src-tauri/src/ui/md3.rs`: `state_layer`, surface colors.

**External Dependencies:**

- egui (already in use) — whole-widget opacity support for the card.
- No new crates.

### File Structure

```
crates/app_settings/src/settings.rs        # window_sidebar_overlay default → true
src-tauri/src/settings.rs                  # mirrored default
src-tauri/src/app.rs                       # overlay_open init, last-switch state, hover flag,
                                           #   opacity resolver + deadline provider
src-tauri/src/ui/mux_sidebar.rs            # opacity constants, draw_overlay opacity application
src-tauri/src/render/mod.rs                # pass opacity into the overlay draw
src-tauri/src/window_host.rs               # hover feed, overlay_work, wait deadline
src-tauri/web-shared/settings/...          # only if a mirrored default/test needs updating
```

## Test Scenarios

### Unit Tests

- [ ] Settings default: deserializing `{}` for `MuxSettings` yields `window_sidebar_overlay == true`.
- [ ] Settings override: deserializing `{"window_sidebar_overlay": false}` yields `false`.
- [ ] Opacity resolver — fresh state, no hover, no switch → `OVERLAY_IDLE_OPACITY`.
- [ ] Opacity resolver — hover true → `1.0` regardless of the switch timestamp.
- [ ] Opacity resolver — switch just now, no hover → `1.0`.
- [ ] Opacity resolver — switch `OVERLAY_BRIGHT_HOLD` + fade ago, no hover → `OVERLAY_IDLE_OPACITY`.
- [ ] Opacity resolver — mid-fade value lies strictly between `OVERLAY_IDLE_OPACITY` and `1.0`.
- [ ] Opacity resolver — a second switch inside the hold window extends brightness past the first
      expiry.
- [ ] Opacity resolver — hover released before the hold expires keeps `1.0` until the hold expires.
- [ ] Deadline provider — returns `None` when settled (no hover, hold expired, fade complete).
- [ ] Deadline provider — returns a deadline while the hold is pending or the fade is running.
- [ ] `dispatch_mux_action` records the switch timestamp for next / prev / select-by-digit.
- [ ] Overlay bright rendering still matches the pinned 12px radius / 0.92 fill-alpha expectation.
- [ ] Persistent placement ignores the opacity input (fully opaque regardless of the value passed).

### Integration Tests

- [ ] `overlay_work` (or its equivalent input to `should_skip_frame`) is true while the fade runs on an
      otherwise clean grid, and false once settled.
- [ ] A hover transition across the card boundary requests a redraw even with no other input.
- [ ] Toggling the sidebar closed while dim, then open again, resolves to a defined opacity without
      leaving stale fade state.

### E2E Tests

**Existing E2E tests**: none for GUI rendering; the repo's integration tests are Rust
(`src-tauri/tests/cli_subcommands.rs`) and cover CLI subcommands only.
**Run command**: not applicable to this feature.

- [ ] Existing Rust test suite passes without regression.

### Edge Cases

- [ ] Pointer exactly on the card rect boundary — the hover predicate agrees with the click hit test
      (no state that flickers between the two).
- [ ] Sidebar closed (toggled off) — no fade state advances, no deadline is armed.
- [ ] Tab not attached to mux — overlay is not drawn and no deadline is armed.
- [ ] Switching windows while the pointer hovers the card — stays bright, and the hold is recorded so
      that releasing hover later still honors the remaining hold.
- [ ] Window resize while dim — the recomputed card rect is used for the hover test on the next frame.
- [ ] Clock behavior: elapsed-time comparisons use monotonic `Instant`, never wall clock.

### Performance Tests

- [ ] Idle (settled, non-hovered, sidebar open) — no repaint requests attributable to this feature over
      a multi-second observation of the deadline provider.
- [ ] Fade window — the number of frames requested is bounded by `OVERLAY_DIM_FADE` and the redraw rate
      limit already in place.

## Security Considerations

- **Authentication / Authorization:** not applicable — local rendering only.
- **Input Validation:** the opacity resolver clamps its output to `0.0..=1.0`; durations are compile-time
  constants, not user input.
- **Data Protection:** no new persisted or transmitted data.
- **XSS / SQL Injection / CSRF:** not applicable — no WebView surface or query is involved.

## Error Handling

No fallible operations are introduced. Defensive behavior instead of error codes:

| Situation | Handling |
| --- | --- |
| Card rect unavailable (sidebar not drawn) | Treat hover as false; skip fade advancement |
| Missing last-switch timestamp | Treat as "not recently switched" → dim |
| Opacity computed out of range | Clamped to `0.0..=1.0` at the resolver boundary |

## Performance Optimization

### Performance Goals

- Zero feature-attributable repaints in the settled state.
- Fade limited to `OVERLAY_DIM_FADE` of animated frames, subject to the existing redraw rate limit.

### Optimization Strategies

- Resolve opacity as a pure function of stored state so the draw path stays allocation-free.
- Arm wait deadlines only for the hold expiry and while a fade is in progress.
- Recompute hover only when the pointer moves or the card rect changes.

### Caching Strategy

Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1–FR10) are implemented and covered by tests.
- [ ] All test scenarios pass.
- [ ] Idle repaint behavior matches NFR1.
- [ ] Existing `mux_sidebar` tests (12px radius / 0.92 alpha, no raw color constructors) still pass.
- [ ] CLI-only build (`--no-default-features`) still checks cleanly.
- [ ] Documentation of the constants lives with their definition.

## Assumptions

Recorded because this run was unattended: the Notion task fixed the behavior but not these details, and
no user confirmation was available (Codex was unavailable in this environment, so the batch consultation
loop was skipped and these are Claude's decisions).

- **A1 — Overlay opens on startup.** FR1 alone would make a default-configured session show no window
  list at all (the runtime open flag starts closed today), which is strictly less information than the
  current persistent default. FR2 therefore flips that initial flag to open.
- **A2 — Dim opacity is `0.35`.** The task says "terminal characters are comfortably readable" without a
  number. `0.35` is the chosen starting point; it is a single named constant so it can be retuned after
  hands-on review.
- **A3 — Fade timing is asymmetric.** Bright → dim fades over `200 ms`; dim → bright is immediate. The
  task specifies neither; an instant brighten keeps interaction feeling responsive while the fade avoids
  a visible pop when the card recedes.
- **A4 — Opacity covers the whole card.** The task says "raise the transparency"; applying it to the fill
  only would leave opaque text on top of the terminal, so the multiplier covers text, badges, icons and
  shadow as well.
- **A5 — Hover wins over the hold timer, and the hold is not cancelled by releasing hover.** The card is
  bright if *either* condition holds. Releasing hover inside the 3-second post-switch window keeps it
  bright for the remainder.
- **A6 — Row clicks count as switches.** The task names only key-driven switching. Treating a row click
  the same way keeps the state machine uniform, and is unobservable while the pointer still hovers.
- **A7 — No new user-facing settings.** Opacity values and durations stay internal constants; the task
  did not ask for configurability.
- **A8 — Persistent mode is untouched**, including its full opacity.
- **A9 — Only the absent-key default changes.** Users with `window_sidebar_overlay: false` already saved
  keep the persistent panel.
- **A10 — The design step is skipped.** The visual change is one opacity multiplier plus a fade on an
  existing component built from existing MD3 helpers; no new screens or components need a look defined.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。

None. Every requirement is actionable; the judgment calls are recorded above as assumptions rather than
deferred.

## References

- Notion task: [https://www.notion.so/3ab3509ec8ee808fbf9bedd14ee73eba](https://www.notion.so/3ab3509ec8ee808fbf9bedd14ee73eba)
- Requirements document: `feature-docs/mux-sidebar-overlay-dim/REQUIREMENTS.md`
- Existing overlay implementation: `src-tauri/src/ui/mux_sidebar.rs`
- Frame scheduling: `src-tauri/src/window_host.rs`
- Design tokens: `doc/UI-DESIGN-GUIDELINES.yaml`, `src-tauri/src/ui/md3.rs`
