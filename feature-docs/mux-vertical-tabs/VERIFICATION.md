# Verification Document: mux Vertical Tabs (Sidebar Window List)

## Overview
**Feature**: mux-vertical-tabs / **SPEC.md**:
`feature-docs/mux-vertical-tabs/SPEC.md` / **IMPLEMENTATION.md**:
`feature-docs/mux-vertical-tabs/IMPLEMENTATION.md`

## Build Verification
- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (web): `bun run typecheck`
- Expected: exit code 0, no errors

## Test Verification
- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (web): `bun test`
- Note: `tabs.rs` replay tests are non-deterministic in parallel; re-run
  single-threaded on flake (test/README.md)

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Top-tab consolidation | One tab cell titled `mux: <active window name>`; OSC rename of the active window updates the title | Unit |
| TS-2 | Sidebar list model | Entries carry number + name + single active flag, flat and ordered; 0/1-entry lists valid | Unit |
| TS-3 | Click switch | Clicked entry index routes to the same window-switch path as the former sub-tab click | Unit |
| TS-4 | Overlay toggle action | `toggle-window-sidebar` defaults to Ctrl+W; toggles the flag in overlay mode; strict no-op in persistent mode; existing six actions unchanged | Unit |
| TS-5 | Settings field | Default false; null/missing handled; Rust ⇄ JSON round-trip; TS mirror typechecks | Unit |
| TS-6 | Resize discipline | Overlay open/close and window switch leave grid size unchanged; placement-setting flip triggers exactly one pending-resize cycle | Unit |
| TS-7 | Sidebar visibility | Visibility matrix over (mode, flag, tab mux-state); local tabs never show a sidebar | Unit |
| TS-8 | Settings round-trip | Toggling the mux-section switch saves the mux settings object with the field and preserves sibling fields | Unit (bun) |
| TS-9 | Right-edge persistent placement | Persistent panel renders on the right; grid x-origin identical with/without sidebar; usable width shrinks by the width function's value; overlay geometry unchanged | Unit |
| TS-10 | Overlay floating card | Overlay card rect inset 16 px from the terminal area's top/right/bottom edges; 12 px corner radius; 92%-alpha `surface_container_high` fill; no separator line; zero grid inset and toggle behavior unchanged | Unit |
| TS-11 | Overlay card paint geometry | The card background's PAINTED rect equals the computed card rect; rows are inset 8 px (horizontal) / 12 px (vertical) from the card edge on all four sides — right edge included; few-entry lists still span the full inset height | Unit |
| TS-12 | Overlay follows window resize | Across consecutive frames with different screen sizes (grow and shrink), the painted card rect matches each frame's computed rect and the width function's current value | Unit |
| TS-13 | Sidebar wheel routing | The hit-region helper matches the drawn sidebar geometry in both placements; a wheel over the visible sidebar forwards to egui and skips the terminal scroll path; wheel behavior outside the region / with the sidebar hidden / on local tabs is unchanged | Unit |
| TS-14 | Overlay press suppression | A left press inside the visible overlay card never starts a terminal selection; with the overlay closed / on local tabs the press behaves as before; a terminal-started drag released over the sidebar still commits; press and wheel guards share the same hit-region helper | Unit |

## Code Quality Verification
- Format: not enforced project-wide (rustfmt non-mandatory; do not run
  crate-wide fmt)
- Static analysis: covered by the build commands above

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FR1–FR6 implemented and tested | TS-1..TS-9 pass |
| SC-2 | Resize discipline (NFR1) | TS-6 + manual M-2 |
| SC-3 | Local tab behavior unchanged (NFR2) | TS-7 + existing suite green + M-3 |
| SC-4 | Visuals match the design decisions (NFR3) | Manual mockup comparison (below) |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0005 | TS-1 |
| FR2 | task0004, task0005, task0009, task0010 | TS-2, TS-7, TS-12, TS-13 |
| FR3 | task0004, task0005 | TS-3 |
| FR4 | task0003, task0005, task0006 | TS-4, TS-7, TS-9 |
| FR5 | task0003, task0005, task0007, task0008, task0009, task0011 | TS-4, TS-10, TS-11, TS-12, TS-14 |
| FR6 | task0001, task0002 | TS-5, TS-8 |
| NFR1 | task0005 | TS-6 |
| NFR2 | task0003, task0005, task0010, task0011 | TS-4, TS-7, TS-13, TS-14 |
| NFR3 | task0004, task0006, task0007, task0008 | M-1 (mockup comparison), TS-9, TS-10, TS-11 |

## Manual Testing (E2E Not Possible)

No automated E2E infrastructure exists (test/README.md). Human scenarios:

- [ ] M-1: Attach mux with 3+ windows → sidebar lists all windows (number +
      name + active pill) on the RIGHT edge in both placement modes; click
      switches; top tab shows `mux: <name>` and follows a Claude Code title
      rewrite. **Mockup visual comparison**:
      compare against
      `feature-docs/mux-vertical-tabs/design/mockups/screen-mux-vertical-tabs.html`
      (states: persistent / overlay-open / many-windows / empty)
- [ ] M-2: Overlay mode → `Ctrl+Z Ctrl+W` opens/closes the right floating
      card (16 px margins, rounded translucent card, no separator line); a
      full-screen TUI (e.g. Claude Code) does NOT reflow on toggle; flipping
      the setting reflows exactly once
- [ ] M-3: Persistent mode → `Ctrl+Z Ctrl+W` does nothing; local tabs show
      no sidebar; settings keybind grid lists the new action with a
      translated label; existing mux chords still work
- [ ] M-4: Resize the app window with the overlay open → the card follows
      the new size/position (grow and shrink). With 15+ windows, hover the
      sidebar (both placements) and wheel-scroll → the LIST scrolls, the
      terminal scrollback does not move; wheel over the terminal area
      still scrolls the terminal. Drag on the open overlay card → no
      terminal selection appears underneath; drag on the terminal area
      still selects normally

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit/Integration | TS-1..TS-14 | 14 | 0 | 0 |
| Manual | M-1..M-4 | 0 | 0 | 4 |
