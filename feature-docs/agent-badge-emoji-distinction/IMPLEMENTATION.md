# Implementation Plan: agent-badge-emoji-distinction

## Overview

Make the `working` / `idle` agent-state badges in the tab bar and the mux
sidebar distinguishable at a glance by rendering them as color emoji
(working = HIGH VOLTAGE U+26A1, idle = ZZZ U+1F4A4) through the swash
rasterization path already established for the status bar, with the current
filled-circle rendering retained as the fallback and for all other states.

## Technology Stack

- **Language / UI**: Rust, egui (in-process UI), swash (glyph rasterization)
  — all already in use; this feature engages no new technology.
- **New dependencies**: none. No new crate or package is introduced, so no
  new dependency license enters the project (`project.license: MIT`
  unaffected).

## Layer Structure

- `src-tauri/src/ui/` — widget layer (tab bar, mux sidebar, status bar,
  emoji texture cache). Widgets peer-import shared badge helpers from the
  tab-bar module (established pattern: the sidebar already imports the
  shared state-color / fill helpers from there).
- `src-tauri/src/render/mod.rs` — frame composition; builds per-frame
  view-models and calls the widget draw functions. Dependency direction:
  render → ui, never the reverse.
- All touched modules are GUI-only (behind the `gui` feature gate in
  `lib.rs`); the CLI build must remain unaffected (NFR2).

## Shared Components

Single-task feature — no cross-task contracts are needed. All component
contracts live in `tasks/task0001.md`.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none — single task) | — | — | — |

## Conventions

- Colors exclusively via the `ui::md3` accessor functions; no raw color
  constructors in widget modules (existing module convention).
- Unit tests inline as `#[cfg(test)] mod tests`, named
  `<subject>_<scenario>_<expected>` (project `test/README.md` convention).
- GUI-only code stays under the `gui` feature gate; CLI-shared code depends
  only on always-built crates.

## Cross-task Design Decisions

### D1: Single-task decomposition

The feature is delivered as one task (task0001) covering both surfaces plus
the shared decision logic and call-site plumbing. Rationale: the design
requires the state→presentation choice to live in ONE shared pure function
that both painters consume; splitting tab bar and sidebar into parallel
tasks would force both to implement against a contract for that function and
guarantee merge conflicts in the file that hosts it, with no size
justification — the entire change is well within one implementer session
(two painter changes, one shared pure decision pair, resource plumbing at
the frame-composition call sites).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Glyph pair reads poorly at 12px on real hardware (subjective criterion) | Medium | Medium | Final gate is the user's on-device visual check (TS3); alternates are held in reserve and a design revisit is the fallback path |
| Emoji font lacks a glyph on some systems | Low | Low | Pure fallback rule: render the current filled circle for that state — never a blank slot |
| Unified 12px badge slot shifts titles vs. today | Certain (one-time ~2px) | Low | Accepted by design: constant slot trades a one-time shift for stability across state transitions; covered by a layout assertion |

## Open Questions

- [ ] Final ⚡ / 💤 sign-off is inherently subjective ("distinguishable at a
      glance") — resolved only by the user's on-device visual check (TS3)
      after implementation.
