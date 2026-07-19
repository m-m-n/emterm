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

## Code Quality Verification
- Format: not enforced project-wide (rustfmt non-mandatory; do not run
  crate-wide fmt)
- Static analysis: covered by the build commands above

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FR1–FR6 implemented and tested | TS-1..TS-8 pass |
| SC-2 | Resize discipline (NFR1) | TS-6 + manual M-2 |
| SC-3 | Local tab behavior unchanged (NFR2) | TS-7 + existing suite green + M-3 |
| SC-4 | Visuals match the design decisions (NFR3) | Manual mockup comparison (below) |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0005 | TS-1 |
| FR2 | task0004, task0005 | TS-2, TS-7 |
| FR3 | task0004, task0005 | TS-3 |
| FR4 | task0003, task0005 | TS-4, TS-7 |
| FR5 | task0003, task0005 | TS-4 |
| FR6 | task0001, task0002 | TS-5, TS-8 |
| NFR1 | task0005 | TS-6 |
| NFR2 | task0003, task0005 | TS-4, TS-7 |
| NFR3 | task0004 | M-1 (mockup comparison) |

## Manual Testing (E2E Not Possible)

No automated E2E infrastructure exists (test/README.md). Human scenarios:

- [ ] M-1: Attach mux with 3+ windows → sidebar lists all windows (number +
      name + active pill); click switches; top tab shows `mux: <name>` and
      follows a Claude Code title rewrite. **Mockup visual comparison**:
      compare against
      `feature-docs/mux-vertical-tabs/design/mockups/screen-mux-vertical-tabs.html`
      (states: persistent / overlay-open / many-windows / empty)
- [ ] M-2: Overlay mode → `Ctrl+Z Ctrl+W` opens/closes the right overlay; a
      full-screen TUI (e.g. Claude Code) does NOT reflow on toggle; flipping
      the setting reflows exactly once
- [ ] M-3: Persistent mode → `Ctrl+Z Ctrl+W` does nothing; local tabs show
      no sidebar; settings keybind grid lists the new action with a
      translated label; existing mux chords still work

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit/Integration | TS-1..TS-8 | 8 | 0 | 0 |
| Manual | M-1..M-3 | 0 | 0 | 3 |
