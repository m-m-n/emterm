# Verification Document: settings-profile-drag-reorder

## Overview
**Feature**: settings-profile-drag-reorder / **SPEC.md**: `feature-docs/settings-profile-drag-reorder/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/settings-profile-drag-reorder/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/settings-profile-drag-reorder/THREAT-MODEL.md`

All commands run from the project root.

## Build Verification
- Command: `bun run typecheck` (workflow.yaml `project.components.webview-ts.build_command`)
- Command: `bun run build:settings` (NFR3)
- Expected: exit code 0, no errors, for both commands

## Test Verification
- Command: `bun test` (workflow.yaml `project.components.webview-ts.test_command`; uses the `test-setup.ts` happy-dom preload)
- Expected: exit code 0; the new files `src-tauri/web-shared/settings/sections/profiles-section.test.ts` and `src-tauri/web-shared/settings/sections/ssh-section.test.ts` are collected and pass
- Coverage target: no percentage target is set by SPEC.md; every task Acceptance Criterion maps to at least one test

### Test Scenarios from SPEC.md
TS-1 to TS-7 come from SPEC.md. TS-8 and TS-9 are added here so that FR5, NFR1 and NFR2 each have a verifying scenario.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Profiles: render the section with 3 profiles, dispatch dragstart with a DataTransfer on the item at index 1 (AC-1, AC-3; FR1, FR3) | The DataTransfer returns `"1"` for `application/x-emterm-profile-index`, effectAllowed is `"move"`, and it holds no `text/plain` entry (exactly one entry) | Unit |
| TS-2 | SSH: render the section with 3 connections and an empty `ssh_command_path`, dispatch dragstart on the item at index 1 (AC-2, AC-3; FR2, FR3) | The DataTransfer returns `"1"` for `application/x-emterm-ssh-index`, effectAllowed is `"move"`, and it holds no `text/plain` entry (exactly one entry) | Unit |
| TS-3 | Profiles: dragstart on index 0, then drop on index 2 (AC-4; FR4, FR6) | Order becomes [B, C, A]; `saveSetting` is called once, with key `"profiles"` and that array; `reRender` is called | Unit |
| TS-4 | SSH: dragstart on index 0, then drop on index 2 (AC-5; FR4, FR6) | Order becomes [B, C, A]; `saveSetting` is called once, with key `"ssh_connections"` and that array; `reRender` is called | Unit |
| TS-5 | Both lists: a drop without dragstart, and a drop onto the dragged item itself (AC-6; FR4) | `saveSetting` is not called in either case, on either list | Unit |
| TS-6 | Run `bun test`, `bun run typecheck` and `bun run build:settings` (AC-7; NFR3, NFR4) | All three exit 0; the regression tests run under the existing preload without a real WebKitGTK | Command |
| TS-7 | Linux (WebKitGTK): open settings > Profiles and drag an item to a new position, then reopen the settings panel; repeat in settings > SSH connections (AC-8; FR1, FR2, FR4) | The order changes on drop and is still there after reopening, for both lists | Manual |
| TS-8 | Inspect the integrated diff against `workflow.implement.base_commit` (FR5, NFR1) | Source changes are limited to `profiles-section.ts`, `ssh-section.ts` and the two new test files under `src-tauri/web-shared/settings/sections/`; nothing under `src-tauri/src/` and `test-setup.ts` changes; no shared drag-reorder helper and no new export is added | Inspection |
| TS-9 | Windows (WebView2): in settings > Profiles and settings > SSH connections, drag an item to a new position, then reopen the settings panel (NFR2) | Drag reorder works and the order is kept, as before the change | Manual |

## Code Quality Verification
- Format: no format command is configured (workflow.yaml `project.components.webview-ts.format_command` is empty)
- Static analysis: `bun run typecheck`

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage table below; TS-1 to TS-5 pass |
| SC-2 | All test scenarios pass | TS-1 to TS-9 |
| SC-3 | `bun test`, `bun run typecheck` and `bun run build:settings` succeed (NFR3) | TS-6 |
| SC-4 | Rust/wry side unchanged (NFR1) | TS-8 |
| SC-5 | Code review is completed | Review phase result in workflow.yaml |
| SC-6 | Manual Linux check (AC-8) passes for both lists | TS-7 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-7 |
| FR2 | task0002 | TS-2, TS-7 |
| FR3 | task0001, task0002 | TS-1, TS-2 |
| FR4 | task0001, task0002 | TS-3, TS-4, TS-5, TS-7 |
| FR5 | task0001, task0002 | TS-8 |
| FR6 | task0001, task0002 | TS-1, TS-2, TS-3, TS-4 |
| NFR1 | task0001, task0002 | TS-8 |
| NFR2 | task0001, task0002 | TS-9 |
| NFR3 | task0001, task0002 | TS-6 |
| NFR4 | task0001, task0002 | TS-6 |

## E2E Testing
No E2E framework is configured for the `webview-ts` component (`e2e_test_command` is empty), and SPEC.md lists no existing E2E tests.

## Manual Testing (E2E Not Possible)
Prerequisite: a GUI build that embeds the rebuilt settings bundle (`make build` runs `bun run build:settings` and writes the binary to `src-tauri/target-host`).

- [ ] TS-7: Linux (WebKitGTK) — settings > Profiles: drag an item to another position; the list reorders; close and reopen the settings panel; the new order is kept. Repeat in settings > SSH connections.
- [ ] TS-9: Windows (WebView2) — the same steps as TS-7 for both lists; drag reorder works as before the change.

## Performance / Security Verification (if applicable)
- Performance: not applicable (SPEC.md Performance Optimization: N/A)
- TM-1: drop handlers act only on the in-memory `dragIndex` set by a dragstart on the same list and never read the drop event's DataTransfer — checked by the TS-5 unit tests in which a drop without dragstart carries a DataTransfer pre-filled with the list's custom MIME type (task0001 AC-4, task0002 AC-4) and leaves `saveSetting` uncalled, and by review of both drop handlers for any DataTransfer read
- TM-2: dragstart sets exactly one DataTransfer entry, the list's custom MIME type holding the index, with no `text/plain` entry and no other type — checked by the TS-1 / TS-2 unit tests asserting the full type list (task0001 AC-2, task0002 AC-2)

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (`bun run typecheck`, `bun run build:settings`) | 2 | 0 | 0 |
| Unit tests | 5 (TS-1 to TS-5) | 5 | 0 | 0 |
| Command | 1 (TS-6) | 1 | 0 | 0 |
| Inspection | 1 (TS-8) | 0 | 0 | 1 |
| Manual | 2 (TS-7, TS-9) | 0 | 0 | 2 |
| Security | 2 (TM-1, TM-2) | 2 | 0 | 0 |
| **Total** | **13** | **10** | **0** | **3** |
