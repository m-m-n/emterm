# Implementation Plan: mux Window List Overlay Auto-Dim

## Overview

Two independent tasks: one flips the persisted display-mode default to overlay, the other adds the
hover / post-switch opacity behavior to the overlay card and the frame scheduling it needs.

## Technology Stack

- **Language**: Rust (existing crate set; no new dependencies, no license impact)
- **UI toolkit**: egui via the existing `ui::mux_sidebar` module — whole-widget opacity is taken from
  egui's own facilities, not hand-multiplied per color
- **Timing**: monotonic instants from the standard library, following the existing visual-bell fade
  precedent in `app.rs`
- **Settings**: serde defaults in `crates/app_settings` mirrored by the app-side settings struct

No new dependency is introduced, so the `project.license: MIT` constraint needs no evaluation.

## Layer Structure

| Layer | Responsibility | Allowed dependencies |
| --- | --- | --- |
| Settings (`crates/app_settings`, `src-tauri/src/settings.rs`) | Persisted display-mode default | none of the below |
| App state (`src-tauri/src/app.rs`) | Owns dim state (hover flag, last-switch instant, fade bookkeeping), resolves opacity, provides the wait deadline | settings |
| Draw (`src-tauri/src/render/mod.rs`, `src-tauri/src/ui/mux_sidebar.rs`) | Applies a supplied opacity to the overlay card; never computes it | app state (read-only, via parameters) |
| winit host (`src-tauri/src/window_host.rs`) | Feeds pointer position into the hover flag, includes dim work in the frame-skip gate, arms the wait deadline | app state |

The draw layer must stay a pure consumer: it receives an opacity value and renders it. Keeping the
decision in the app layer is what makes the behavior unit-testable without a GPU surface.

## Shared Components

Both tasks are file-disjoint and neither calls into the other, so there is no cross-task contract to
pin. The table is retained to state that explicitly.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

The one shared *fact* between the tasks is the existing settings field `mux.window_sidebar_overlay`,
already present in both settings structs. task0001 changes only its default value; task0002 reads it
through the existing accessor path (`App::mux_sidebar_visibility`). No signature changes, so the tasks
cannot conflict.

## Conventions

- **Naming**: the dim behavior's constants and state carry an `overlay`-prefixed, intent-revealing name
  (idle opacity, dim fade duration, bright hold duration). Avoid names that describe the mechanism
  ("timer", "alpha2").
- **Constant placement**: all newly introduced opacity and duration constants live in one place, next to
  the resolver that consumes them (NFR2). The pre-existing bright fill alpha stays where it is and keeps
  its value.
- **Colors**: produced only through the existing MD3 helpers. `ui/mux_sidebar.rs` must not gain raw color
  constructors — an existing test in that module scans its own source for them.
- **Time**: monotonic instants only; never wall-clock time for elapsed comparisons.
- **Logging**: none added. This is a per-frame render path; log calls there would flood the log file.
- **Error handling**: no fallible operation is introduced. Absent state (no card rect, no recorded
  switch) resolves to the safe default (not hovered / not recently switched) rather than an error.

## Cross-task Design Decisions

### D1: Opacity is decided in the app layer, applied in the draw layer

The resolver is a pure function of stored state (hover flag, last-switch instant, fade origin) returning
a clamped opacity plus whether animation is still in flight. The draw layer takes that value as an
argument. Rationale: it makes every acceptance criterion about timing and precedence testable in plain
unit tests, and it keeps the frame-skip gate and the draw call reading the same single source of truth.
Affected task: task0002.

### D2: Opacity multiplies the whole card, not individual colors

The dim state is expressed as one whole-card opacity multiplier applied at the card's container level,
so fill, text, badges, icons and the elevation shadow all fade together (FR6). Rationale: dimming only
the fill leaves opaque glyphs over the terminal and defeats the feature; multiplying each color by hand
would both duplicate the factor and risk introducing raw color construction that the module's own test
forbids. Affected task: task0002.

### D3: Only the absent-key default changes

The persisted-setting change is limited to the default used when the key is missing (and when it is
explicitly null, which the loader resolves through the same default). A stored `false` continues to
select the persistent panel. Rationale: users who deliberately chose the persistent panel must not have
it changed under them. Affected task: task0001.

### D4: Bright is immediate, dim is faded

Entering the bright state applies the full value on the very next frame; leaving it interpolates to the
idle value over the fade duration. Rationale: responsiveness matters on the way in, smoothness on the
way out. Affected task: task0002.

### D5: Scheduling changes are additive

The frame-skip gate's "overlay work" input and the wait-deadline computation gain one additional
contributor each, and the pointer path gains one redraw trigger. No existing contributor is removed or
reordered. Rationale: those two expressions gate every frame in the application; a subtractive change
there risks regressions far outside this feature. Affected task: task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Intermediate fade frames dropped by the clean-grid frame-skip gate | High | Feature appears to snap instead of fade, or never dims | Treat "fade in flight" and "hold expired" as work in the gate; cover with a test at the gate's input level |
| Hover produces no frame because bare pointer motion is not "actionable input" | High | Card never brightens on hover | Add an explicit redraw trigger on hover-predicate transitions |
| Idle repaint loop introduced by always arming a deadline | Medium | Continuous CPU use, regressing an area that was deliberately optimized | Arm a deadline only while the hold is pending or a fade is running; test that the settled state yields no deadline |
| Existing pinned expectations (12px radius, bright fill alpha, no raw color constructors) broken | Medium | Test failures unrelated to the intended change | Keep the bright appearance byte-identical; apply opacity at container level via toolkit facilities |
| Default flip surprises users on upgrade | Medium | Unexpected UI change | Limit the change to the absent/null key case (D3) and keep the toggle working |
| Dim value too faint to recognize the list | Medium | Usability regression | Single named constant, retunable after hands-on review; verification includes a manual legibility check |

## Open Questions

- [ ] The idle opacity value and the fade duration are chosen assumptions (SPEC.md A2 / A3) and may need
      retuning after the feature is seen on a real display. Both are single named constants so the
      adjustment is a one-line change.
