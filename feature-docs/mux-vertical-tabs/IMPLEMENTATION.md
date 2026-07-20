# Implementation Plan: mux Vertical Tabs (Sidebar Window List)

## Overview

Collapse the mux window group's inline top-tab expansion into a single top tab
titled `mux: <active window name>`, and present the window list in a new
egui-native vertical sidebar with two placement modes (persistent right /
right overlay — both on the right edge since the 2026-07-20 spec update).

## Technology Stack

- **Language**: Rust (egui native UI) + TypeScript (settings WebView)
- **No new dependencies** — license check not applicable (nothing added).

## Layer Structure

- `crates/app_settings` — persisted settings schema (serde shape)
- `src-tauri/src/settings.rs` — resolved GUI runtime settings + loader
- `src-tauri/src/mux/prefix.rs` — prefix-key action SSOT
- `src-tauri/src/app.rs` — mux action dispatch, runtime UI state
- `src-tauri/src/ui/` — egui components (new: `mux_sidebar.rs`)
- `src-tauri/src/render/mod.rs` + `src-tauri/src/window_host.rs` — frame
  composition and terminal grid geometry
- `src-tauri/web-shared/` — settings panel TS mirror + i18n

Dependency direction: render/window_host → ui components → md3 tokens;
app.rs → prefix.rs; everything reads settings, nothing writes it outside the
settings pipeline.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Settings field `mux.window_sidebar_overlay` | Persisted placement mode | Boolean; serde default `false` (= persistent mode). Present in `crates/app_settings` `MuxSettings`, resolved `Settings.mux` in `src-tauri/src/settings.rs`, and TS `MuxSettings` mirror under the SAME name. `false` = persistent right panel, `true` = right overlay | task0001 (defines Rust), task0002 (TS mirror + UI), task0003 (no-op guard), task0005 (placement), task0006 (right-edge move) |
| Prefix action id `toggle-window-sidebar` | Overlay open/close action | New row in `DEFAULT_ACTION_BINDINGS` bound to Ctrl+W (after the Ctrl+Z prefix); new `PrefixAction` variant named `ToggleWindowSidebar`. The id string is what `settings.mux.keybinds` and `get_mux_action_defaults` expose | task0003 (defines), task0002 (i18n label for the keybind grid) |
| App overlay state | Runtime open/closed flag for the overlay | Single per-App boolean field `mux_sidebar_overlay_open`, initial `false`. Toggle entry point: dispatching `ToggleWindowSidebar` flips it ONLY when `mux.window_sidebar_overlay` is `true`; otherwise it is a strict no-op (FR4). Postcondition: the flag is reset to `false` whenever the mux group of the focused tab is torn down (detach / session end). Readers treat "overlay visible" = flag AND overlay mode AND active tab is mux-attached | task0003 (owns field + toggle + reset), task0005 (reads for rendering) |
| Sidebar width function | One width formula for both placements | Pure function in the new `ui::mux_sidebar` module: given the window inner width in physical/logical px, returns clamp(180 px, 22% of width, 320 px). Deterministic, no state | task0004 (defines), task0005 (grid inset + overlay rect) |
| Sidebar widget `ui::mux_sidebar` | Draw the window list, report clicks | Input: an egui paint context/UI region, an ordered entry list (window index `usize`, display name, active flag), and a placement variant (persistent / overlay). Output: optionally the clicked entry's window index. It draws ONLY the list (MD3 styling per the design decisions folded into task0004); it never sends mux messages itself | task0004 (implements), task0005 (invokes + routes result into the existing `TabEvent::MuxSwitch` application path) |

## Conventions

- Mirror the `tab_always_expand` boolean end-to-end pattern for the new
  settings field (serde default fn, Option overlay in the loader, TS mirror).
- New egui component follows `ui/tab_bar.rs` module conventions: pure draw
  function + small view-model structs, colors via `ui::md3` accessors only
  (no hardcoded hex).
- Japanese/English locale keys live under the existing `settings.mux.*`
  namespace.

## Cross-task Design Decisions

### 1. Single top tab replaces inline mux cell expansion

When the feature is active (always — this feature removes the old behavior),
`TabBarItem.mux_cells` stays unset so the mux tab renders as one cell. The
tab's label becomes `mux: <active window name>` (active window name already
tracks OSC renames via the existing group state). The `MuxSubTabCell`
expansion path and `TabEvent::MuxSwitch` event stay in place — the sidebar
becomes the new emitter of window-switch intents, reusing
`App::apply_tab_event`'s existing `MuxSwitch` arm. Affects task0004, task0005.

### 2. Terminal grid geometry gains a horizontal inset

The wgpu terminal grid is computed outside egui (`grid_size` /
`cell_metrics_px` in `window_host.rs`, plus duplicated origin math for cursor
and search overlays in `render/`). A persistent sidebar therefore requires an
explicit width-inset term in these computations: inset = sidebar width when
(persistent mode AND active tab mux-attached), else 0. Overlay mode
contributes 0 inset (draws over the grid). Mode changes flip the existing
`pending_resize` coalescing flag — the same mechanism used when the tab bar
visibility changes — producing exactly one PTY reshape (NFR1).

2026-07-20 update (right-edge persistent placement): the panel sits on the
RIGHT edge, so the inset reduces only the usable grid WIDTH — the grid's
x-origin, cursor, and search-overlay origins are identical with and without
the sidebar (no x-origin term anywhere). Affects task0005 (original
implementation), task0006 (right-edge move), task0004 (width function
input).

### 3. Overlay is an egui right side surface drawn after the central panel

The overlay variant renders on the right edge over the terminal area without
affecting grid geometry, styled per the design step (surface-container-high,
elevation shadow, no scrim, no animation). Placement variant selection is
data-driven (settings bool + runtime flag), not duplicated widget code.
Affects task0004, task0005.

### 4. No protocol changes

The sidebar renders exclusively from the existing client-side
`MuxWindowGroup` state (windows, names, active index). Bell/activity
indicators are explicitly out of scope. All tasks.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Grid origin math duplicated in 3+ places drifts (cursor/search misaligned next to sidebar) | Medium | High | task0005 acceptance criteria explicitly cover cursor & search origins; verify TS-6/M-1 |
| Unexpected PTY resizes on toggle (regression of known resize bugs) | Medium | High | Resize discipline pinned as NFR1 with dedicated test TS-6; toggle path never touches grid inset in overlay mode |
| Settings field name drift between Rust and TS | Low | Medium | Name pinned in Shared Components; TS mirror test |
| Keybind grid in settings panel not picking up the new action label | Low | Low | task0002 adds locale keys; manual M-3 checks the grid |

## Open Questions

- [ ] None.
