# Feature: settings-profile-remove-launch

## Overview

Remove the per-profile Launch action from the Profiles section of the settings panel, together with its `profile:launch` event dispatch and its `settings.profiles.launch` i18n key. The remaining profile actions keep their behavior.

Requirements document: `feature-docs/settings-profile-remove-launch/REQUIREMENTS.md`.

## Objectives

- Remove the profile Launch operation from the settings panel's Profiles section on Windows and Linux.
- Remove the code belonging to the Launch feature.

## Acceptance Criteria

- [ ] **AC-1** (FR1): Rendering the Profiles section with one or more profiles produces no button labelled Launch / 起動; each profile item's `.profile-item-actions` contains exactly four buttons: default toggle, Edit, Duplicate, Delete.
- [ ] **AC-2** (FR2): Clicking every action button of a rendered profile item dispatches no `profile:launch` event on `document`, and the source tree contains no `profile:launch` string.
- [ ] **AC-3** (FR3): en.json and ja.json contain no `settings.profiles.launch` key.
- [ ] **AC-4** (FR4, NFR1): The existing `profiles-section.test.ts` drag reorder tests (AC-1 to AC-5 of the predecessor feature) still pass unchanged, and `bun run typecheck` passes.

## Technical Requirements

### Functional Requirements
- **FR1:** Launch button is not rendered. The Profiles section of the settings panel renders no Launch action for any profile. Each profile list item's action area contains exactly the Set as Default / Unset Default toggle, Edit, Duplicate, and Delete buttons, in that order.
- **FR2:** Launch event dispatch code is removed. The Launch button creation block in `src-tauri/web-shared/settings/sections/profiles-section.ts` (the `// Launch (open new tab with this profile)` comment, the `createActionButton` call labelled `t("settings.profiles.launch")`, and the `profile:launch` CustomEvent dispatch on `document`) is deleted. No `profile:launch` event is dispatched from the Profiles section.
- **FR3:** Launch i18n key is removed. The `settings.profiles.launch` key is removed from `src-tauri/web-shared/i18n/locales/en.json` (`"Launch"`) and `src-tauri/web-shared/i18n/locales/ja.json` (`"起動"`).
- **FR4:** Remaining profile actions keep their behavior. The Add Profile button, default toggle, Edit, Duplicate, Delete, and drag reorder in the Profiles section behave as before the change.

### Non-Functional Requirements
- **NFR1 - Build and test pass:** `bun test` and `bun run typecheck` pass after the change.
- **NFR2 - Target platforms:** Windows and Linux. The change is to the shared settings WebView bundle (`src-tauri/web-shared`), which serves both platforms.

## Implementation Approach

### Changes

| File | Change | Requirement |
|------|--------|-------------|
| `src-tauri/web-shared/settings/sections/profiles-section.ts` | Delete the Launch button creation block: the `// Launch (open new tab with this profile)` comment, the `createActionButton` call labelled `t("settings.profiles.launch")`, and the `profile:launch` CustomEvent dispatch on `document` | FR1, FR2 |
| `src-tauri/web-shared/i18n/locales/en.json` | Remove `settings.profiles.launch` (`"Launch"`) | FR3 |
| `src-tauri/web-shared/i18n/locales/ja.json` | Remove `settings.profiles.launch` (`"起動"`) | FR3 |
| `src-tauri/web-shared/settings/sections/profiles-section.test.ts` | Add TS-1 and TS-2 | FR1, FR2 |

### Kept As-Is

- The shared `createActionButton` helper and the `.profile-action-btn` / `.profile-item-actions` CSS rules (`settings-panel.css:1236-1269`), used by the default toggle, Edit, Duplicate and Delete buttons.
- ja.json's `sshNotConfigured` message (`"...SSH接続を起動できません。"`, line 354), which is unrelated to the Launch feature.

### Profile Item Action Row (after the change)

```
[Set as Default | Unset Default] [Edit] [Duplicate] [Delete]
```

The design step is skipped: the change only removes an existing button from the profile action row; no new UI element, layout, or design token is introduced.

### Dependencies

**Internal Dependencies:**
- `profile:launch` has no listener in TypeScript or Rust (the only occurrence is the dispatch at `profiles-section.ts:160`; no match in `settings-panel.ts`, `settings/web/entry.ts`, or `settings_window/commands.rs`), so removing the dispatch removes no working behavior elsewhere.

**External Dependencies:**
- None.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/settings-profile-remove-launch/**`
- `test-docs/settings-profile-remove-launch/**`

`feature-docs/settings-profile-remove-launch/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/settings-profile-remove-launch/**` covers
`test-docs/settings-profile-remove-launch/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/settings-profile-remove-launch/` directory at all; the declared
`test-docs/settings-profile-remove-launch/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] **TS-1** (bun test, happy-dom; `src-tauri/web-shared/settings/sections/profiles-section.test.ts`; covers AC-1 / FR1): Render the Profiles section with profiles [A (is_default), B]; for each `.profile-list-item`, the `.profile-item-actions` button labels equal [unsetDefault|setDefault, edit, duplicate, delete] via `t()`, and no button text equals "Launch" or "起動".
- [ ] **TS-2** (bun test, happy-dom; `src-tauri/web-shared/settings/sections/profiles-section.test.ts`; covers AC-2 / FR2): Register a `profile:launch` listener on `document`, render the section with a non-default profile, click each action button, and assert the listener was never called.
- [ ] **TS-3** (bun test; covers AC-3 / FR3): Load en.json and ja.json and assert `settings.profiles` has no `launch` property.

### Static Checks
- [ ] **TS-4** (covers AC-2, AC-3 / FR2, FR3): `git grep -n 'profile:launch'` and `git grep -n 'settings.profiles.launch'` over `src-tauri` return no matches.

### Regression
- [ ] **TS-5** (covers AC-4 / FR4, NFR1): Run `bun test` and `bun run typecheck`; all existing tests, including the profiles-section drag reorder tests, pass.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass

## Open Questions

None.

## References

- Requirements: `feature-docs/settings-profile-remove-launch/REQUIREMENTS.md`
