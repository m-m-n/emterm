# Implementation Plan: scroll-region-scrollback

## Overview

Give the region-scroll branch of the terminal core a Ghostty-compatible
transcription condition, so that lines leaving a scroll region whose top
margin is the topmost screen row are written to scrollback instead of being
discarded. Expose the behaviour as one boolean setting that is seeded into
newly spawned tabs and applied live to already-open tabs.

## Technology Stack

- **Rust — `crates/term_core`**: terminal grid, scroll semantics, scrollback
  ring. Owns the transcription condition and the per-core gate value.
- **Rust — `crates/app_settings`**: the settings.json serde schema. Built in
  both the GUI configuration and the CLI-only configuration, so every schema
  change must hold under both (NFR3).
- **Rust — `src-tauri`**: native settings mirror and overlay merge, tab
  construction, application-wide settings application.
- **TypeScript — `src-tauri/web-shared`** (bundled by Bun): settings panel
  section rendering, the settings interface mirror, and the locale files.
- **New external dependencies: none.** No library is added by this feature,
  so there is no new dependency license to record and `project.license: MIT`
  is unaffected.

## Layer Structure

One-way dependency chain, top to bottom. No layer below may reach upward.

| Layer | Responsibility | May depend on |
|-------|----------------|---------------|
| Settings panel (TypeScript) | Renders the toggle, mirrors the key string and its type, supplies the localized label and description | Nothing in Rust at build time — coupling is by key string only |
| settings.json schema (`crates/app_settings`) | Declares the persisted key, its default, and its null-tolerant deserialization | Nothing in this feature |
| Native settings mirror (`src-tauri/src/settings`) | Holds the runtime value and merges an optional overlay onto the default | The settings schema crate |
| Application / tab layer (`src-tauri/src/app`, `src-tauri/src/tabs`) | Pushes the runtime value into every terminal core — at tab construction and on settings save | The native settings mirror, and the terminal core's public setter |
| Terminal core (`crates/term_core`) | Holds the gate as a plain boolean and consumes it inside the scroll-up decision | Nothing above it |

The terminal core never learns about settings.json, the settings crate, or
the settings panel: the value crosses the boundary as a plain boolean pushed
in from the application layer. This keeps the core buildable in isolation and
keeps the CLI-only configuration unaffected by the core change.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Transcription gate on the terminal core | Per-core boolean deciding whether the region-scroll branch transcribes | **State**: one boolean member of the terminal core, **enabled at construction** so a core that is never seeded still behaves Ghostty-compatibly. **Setter**: one public method on the terminal core taking exactly one boolean and returning nothing; name it after the settings key so the five mirrors read alike. Precondition: none — callable at any point in a core's life, including while a scroll region is active. Postcondition: scroll operations processed *after* the call observe the new value; the call itself never mutates scrollback, never marks rows dirty, never emits a scroll event, and never triggers a redraw. **Read**: side-effect free and allocation free, so it can be evaluated on the region branch without violating NFR2 | task0001 (reads it in the region branch), task0002 (defines the seeding and live-apply call sites) |
| Settings key `scroll_region_scrollback_enabled` | The one key string that appears verbatim in all five mirrors | Type boolean everywhere. Absent → `true`; explicit null → `true` (via the existing null-tolerant deserialization helper); explicit `false` → `false`. The same string is the persisted JSON key, the Rust schema field, the native mirror field, the TypeScript interface member, and the identifier the panel toggle saves under. Renaming it later is a five-mirror edit and must be done in one change | task0002, task0003 |
| Settings-panel label and description keys | The localized strings the toggle renders | Two keys under the terminal settings namespace, each defined in **both** locale files with the same key path, placed beside the existing alternate-scroll toggle's keys. A key present in one locale only is a defect | task0003 |

**File overlap between parallel tasks.** `crates/term_core/src/terminal_core.rs`
is declared by both task0001 and task0002 because both need the gate member
and its setter to exist in their own worktree. Each task creates them exactly
as this table specifies if they are not already present; because the contract
is identical on both sides, a merge conflict on that file resolves safely by
adopting the parent side. No other production file is claimed by more than one
task.

**Wiring ownership.** task0002 owns every call site that pushes a settings
value into a core (tab construction and settings save) and must not leave a
placeholder for someone else to replace; task0001 owns the consumption of the
gate inside the scroll decision. Neither task depends on the other's ordering.

## Conventions

- **Naming**: the settings key string above is used verbatim in every layer;
  the core's setter is named after it. Follow the existing alternate-scroll
  setting end to end rather than inventing a new wiring shape — it is the
  nearest precedent for a boolean terminal-behaviour setting and already has
  all five mirrors.
- **Error-handling policy**: this feature defines no error case. A condition
  that does not hold is ordinary behaviour (the pre-change in-place region
  shift), never an error, never a fallback log line, never a user-visible
  message. Malformed settings input is already reduced to a boolean by the
  existing null-tolerant deserialization.
- **Logging policy**: no new log output at any level. The decision sits on the
  output hot path, where logging would violate NFR2.
- **Performance policy (NFR2)**: the added work is a small number of integer
  and boolean comparisons on a branch that is already taken. No allocation may
  be introduced on the non-transcribing path, and neither the full-screen
  branch, the ASCII fast path, nor the bulk-scroll skip-ahead may gain any
  per-byte work.
- **Existing machinery (NFR1)**: reuse the existing blank-push eviction routine
  for transcription. Its compressed row format, interning, reference counting,
  wrap-flag handling and overflow-table clearing are used as-is and are not
  redesigned; the snapshot-replay contract and the parked-view follow rules are
  likewise untouched.
- **Test placement**: new tests live beside the existing tests of the same
  subject. Terminal-core behaviour is verified by feeding byte sequences
  through the core's PTY-data entry point and asserting on the resulting
  scrollback and screen state, never by calling internal scroll helpers
  directly — that is what makes the tests regression evidence for real TUI
  input.
- **Formatting (NFR5)**: format only the files the task touched (rustfmt for
  Rust, Biome for TypeScript). Never run a crate-wide or repository-wide
  format write.

## Cross-task Design Decisions

### D1: One decision point, inside the scroll-up routine

The transcription condition is evaluated inside the shared scroll-up routine,
not at each caller (line feed, index, next line, scroll up). Rationale: all
four entry points must behave identically (FR3), and a per-caller condition
would drift the moment a fifth caller appears. Consequence for planning: the
downward-scroll and line-insert/delete paths reach the grid through different
helpers and therefore inherit nothing — they must be left untouched, which is
what keeps FR7 true by construction rather than by an added exclusion.
Affected tasks: task0001.

### D2: The core stays independent of the settings crates

The gate is a plain boolean member of the terminal core, pushed in from the
application layer, rather than a reference to a settings structure. Rationale:
the core is consumed by the CLI-only build path's dependency graph
expectations and by off-thread snapshot workers that construct cores without
any settings context; a settings dependency in the core would couple all of
them. Consequence: an unseeded core exists and must behave correctly — hence
D3. Affected tasks: task0001, task0002.

### D3: Default-enabled at every layer

`true` is the default of the persisted schema key, of the native mirror, of
the TypeScript mirror, and of the core member itself. Rationale: the feature
goal is Ghostty parity with no configuration (Ghostty applies the behaviour
unconditionally), and a core that was constructed without seeding must not
silently differ from a seeded one. Consequence: disabling is the only state
that requires the value to actually travel end to end, which is why the live
apply path (FR12) is load-bearing rather than a convenience. Affected tasks:
task0001, task0002, task0003.

### D4: Parallel execution and the shared-member duplication it implies

All tasks are implemented in parallel from the same base commit. task0002's
call sites cannot compile unless the core's gate setter exists in its own
worktree, so both task0001 and task0002 introduce that member against the
contract pinned in Shared Components above. This duplication is deliberate and
bounded to one member plus one setter; anything larger would have been a
sequencing problem instead. Affected tasks: task0001, task0002.

### D5: Scope boundary — mux panes and the off-thread replay core

Multiplexer panes hold no terminal core of their own and need no propagation;
the off-thread snapshot replay worker constructs cores without settings
context and its swap path does not carry the new member across. Both are out
of scope. The consequence of the second one is a known limitation: a user who
*disables* the setting may see transcription resume on a core produced by a
snapshot swap until the next settings application. Default-enabled users never
observe it (D3). Neither task may attempt to fix this; it is recorded as a
known limitation and re-surfaced below. Affected tasks: task0001, task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The eviction routine rotates the ring so that the new bottom viewport row is blanked, moving rows that lie *below* the region's bottom margin | High | High | Restoring the out-of-region trailing rows to their original screen positions is part of the transcription operation itself, and is an acceptance criterion of task0001 rather than a detail left to implementation |
| The alternate screen shares one ring and one scrollback with the main screen, so an omitted mode gate would pour full-screen application content into scrollback | Medium | High | The mode gate is one of the five conditions and has its own negative-case acceptance criterion driven by real mode-switch byte sequences |
| Reusing the full-screen canvas-shift render optimization on the region path would shift the whole canvas for a partial-screen scroll | Medium | Medium | The region path marks only the region's rows dirty and emits no full-screen scroll event; the full-screen branch is left untouched and is covered by an unchanged-behaviour criterion |
| The five settings mirrors drift (key present in Rust but not TypeScript, or in one locale only) | Medium | Medium | The key string and the two locale keys are pinned in Shared Components; task0003's criteria include the type-check and the panel test, task0002's include the CLI-only build check |
| Parallel worktrees each introduce the core's gate member, and the two versions differ | Low | Medium | The contract (default, signature shape, postcondition) is pinned in Shared Components so both versions are interchangeable at merge time |
| Region scrolls now contribute a non-zero scrollback delta where they previously contributed zero, feeding the parked-view follow correction | Medium | Low | Transcribed lines are counted as ordinary scrollback lines by every existing consumer; the follow rules themselves are not modified, and this is asserted rather than assumed |

## Open Questions

- [ ] FR6 (the left/right-margin clause) has no automated test: the terminal
      does not implement horizontal margins, so the clause is trivially true
      today and exists as a forward constraint. It is verified by review of the
      condition set, and is recorded as an uncovered requirement.
- [ ] NFR5 (formatting) maps to no test scenario; it is verified by the format
      commands in VERIFICATION.md rather than by a scenario ID.
- [ ] The TypeScript settings-interface mirror may force the new member into
      settings fixtures of section tests beyond the one named in task0003's
      file set. The planner could not enumerate those files; task0003 states
      the expectation, and any extra fixture file is a reportable plan
      deviation rather than a scope expansion.
- [ ] Known limitation (D5): a user who disables the setting may see
      transcription resume on a core produced by an off-thread snapshot swap
      until the next settings application. Out of scope for this feature.
