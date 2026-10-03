# Verification Document: settings-profile-remove-launch

## Overview
**Feature**: settings-profile-remove-launch / **SPEC.md**: `feature-docs/settings-profile-remove-launch/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/settings-profile-remove-launch/IMPLEMENTATION.md`

All commands run from the project root of the integration worktree.

## Build Verification
- Command: `bun run typecheck` (workflow.yaml `project.components.webview-ts.build_command`)
- Expected: exit code 0, no errors

## Test Verification
- Command: `bun test` (workflow.yaml `project.components.webview-ts.test_command`)
- Coverage target: not configured (workflow.yaml declares no coverage tooling for the `webview-ts` component); every TS-n below has a passing automated test or a recorded check

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Render the Profiles section with profiles [A (default), B] and read each `.profile-list-item`'s `.profile-item-actions` buttons (`profiles-section.test.ts`) | Exactly four buttons per item, in order: Unset Default / Set as Default, Edit, Duplicate, Delete (texts from the i18n lookup); no button text equals "Launch" or "起動" | Unit (bun test, happy-dom) |
| TS-2 | Register a `profile:launch` listener on the document, render the section with a non-default profile, click every action button (`profiles-section.test.ts`) | The listener is never called | Unit (bun test, happy-dom) |
| TS-3 | Load en.json and ja.json as data | `settings.profiles` has no `launch` property in either file | Unit (bun test) |
| TS-4 | `git grep -nF 'profile:launch' -- src-tauri ':(exclude)*.test.ts'` and `git grep -nF 'settings.profiles.launch' -- src-tauri ':(exclude)*.test.ts'` | No output from either search (exit code 1). Test files are excluded per IMPLEMENTATION.md D3 | Static check |
| TS-5 | Run `bun test` and `bun run typecheck` | Both exit with code 0; the existing profiles-section drag reorder tests pass with unchanged names | Regression |
| TS-6 | `git diff --name-only {implement base commit}...HEAD`, where the base is workflow.yaml `implement.base_commit` (added by create-plan for NFR2) | Every listed path is under `src-tauri/web-shared/`, `feature-docs/settings-profile-remove-launch/` or `test-docs/settings-profile-remove-launch/`; no path under `src-tauri/src/` | Static check |

## Code Quality Verification
- Format: not declared (workflow.yaml `format_command` is empty)
- Static analysis: `bun run typecheck`

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | No Launch / 起動 button; each profile item has exactly the default toggle, Edit, Duplicate, Delete buttons | TS-1 |
| AC-2 | No `profile:launch` dispatch on clicking any action button; no `profile:launch` string in the source tree | TS-2, TS-4 |
| AC-3 | en.json and ja.json contain no `settings.profiles.launch` key | TS-3, TS-4 |
| AC-4 | Existing drag reorder tests pass unchanged; `bun run typecheck` passes | TS-5 |
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage table below: every requirement has a task and a passing TS-n |
| SC-2 | All test scenarios pass | TS-1 to TS-6 all pass |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1; MT-1, MT-2 |
| FR2 | task0001 | TS-2, TS-4 |
| FR3 | task0001 | TS-3, TS-4 |
| FR4 | task0001 | TS-1, TS-5 |
| NFR1 | task0001 | TS-5 |
| NFR2 | task0001 | TS-6; MT-1, MT-2 |

## Manual Testing (E2E Not Possible)
Prerequisite: a GUI build that embeds the updated settings bundle (`.claude/rules/core-commands.md`).

- [ ] MT-1 (FR1, NFR2): On Linux, open the settings panel's Profiles section with at least two profiles, one of them the default. Each profile row shows Unset Default / Set as Default, Edit, Duplicate and Delete, and no Launch / 起動 button, with the UI language set to English and to Japanese.
- [ ] MT-2 (FR1, NFR2): The same check as MT-1 on Windows.

## Performance / Security Verification (if applicable)
Not applicable: THREAT-MODEL.md's verdict is `no-trust-boundary` (no TM-n), and SPEC.md states no performance requirement.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (typecheck) | 1 | 1 | 0 | 0 |
| Unit tests (TS-1, TS-2, TS-3) | 3 | 3 | 0 | 0 |
| Static checks (TS-4, TS-6) | 2 | 2 | 0 | 0 |
| Regression (TS-5) | 1 | 1 | 0 | 0 |
| Manual (MT-1, MT-2) | 2 | 0 | 0 | 2 |
| **Total** | 9 | 7 | 0 | 2 |
