---
title: "settings-profile-remove-launch"
created_date: 2026-10-03
status: draft
---

# settings-profile-remove-launch - Requirements

## 1. Overview

### 1.1 Background
The Profiles section of the settings panel renders a Launch action for each profile.

### 1.2 Purpose
- Remove the profile Launch operation from the settings panel's Profiles section on Windows and Linux.
- Remove the code belonging to the Launch feature.

### 1.3 Scope
- `src-tauri/web-shared/settings/sections/profiles-section.ts`: the Launch button creation block.
- `src-tauri/web-shared/i18n/locales/en.json` and `src-tauri/web-shared/i18n/locales/ja.json`: the `settings.profiles.launch` key.

## 2. Business Requirements

### 2.1 Business Objectives
- Remove the profile Launch operation from the settings panel's Profiles section on Windows and Linux.
- Remove the code belonging to the Launch feature.

### 2.2 Target Users
Not specified.

### 2.3 Expected Effects
- The Profiles section renders no Launch action.
- The Launch code and its i18n key are removed from the source tree.

## 3. Use Cases

None. This feature removes an existing operation.

## 4. Functional Requirements

### 4.1 Function List
| ID | Name | Description |
|----|------|-------------|
| FR1 | Launch button is not rendered | The Profiles section renders no Launch action for any profile |
| FR2 | Launch event dispatch code is removed | The Launch button creation block and the `profile:launch` dispatch are deleted |
| FR3 | Launch i18n key is removed | `settings.profiles.launch` is removed from en.json and ja.json |
| FR4 | Remaining profile actions keep their behavior | Add Profile, default toggle, Edit, Duplicate, Delete, and drag reorder behave as before |

### 4.2 Function Details

#### FR1: Launch button is not rendered

**Description**: The Profiles section of the settings panel renders no Launch action for any profile. Each profile list item's action area contains exactly the Set as Default / Unset Default toggle, Edit, Duplicate, and Delete buttons, in that order.

#### FR2: Launch event dispatch code is removed

**Description**: The Launch button creation block in `src-tauri/web-shared/settings/sections/profiles-section.ts` is deleted. The block consists of:
- the `// Launch (open new tab with this profile)` comment
- the `createActionButton` call labelled `t("settings.profiles.launch")`
- the `profile:launch` CustomEvent dispatch on `document`

No `profile:launch` event is dispatched from the Profiles section.

#### FR3: Launch i18n key is removed

**Description**: The `settings.profiles.launch` key is removed from:
- `src-tauri/web-shared/i18n/locales/en.json` (`"Launch"`)
- `src-tauri/web-shared/i18n/locales/ja.json` (`"起動"`)

#### FR4: Remaining profile actions keep their behavior

**Description**: The Add Profile button, default toggle, Edit, Duplicate, Delete, and drag reorder in the Profiles section behave as before the change.

## 5. Non-Functional Requirements

### 5.1 Build and Test (NFR1)
- `bun test` and `bun run typecheck` pass after the change.

### 5.2 Target Platforms (NFR2)
- Windows and Linux. The change is to the shared settings WebView bundle (`src-tauri/web-shared`), which serves both platforms.

## 6. UI/UX Requirements

### 6.1 Screen Requirements
Each profile list item's action area contains exactly four buttons, in this order: Set as Default / Unset Default toggle, Edit, Duplicate, Delete.

The design step is skipped: the change only removes an existing button from the profile action row; no new UI element, layout, or design token is introduced.

## 7. Data Requirements

None.

## 8. External Integration

None.

## 9. Constraints

### 9.1 Technical Constraints
- The shared `createActionButton` helper and the `.profile-action-btn` / `.profile-item-actions` CSS rules (`settings-panel.css:1236-1269`) stay, because the default toggle, Edit, Duplicate and Delete buttons still use them.
- ja.json's `sshNotConfigured` message (`"...SSH接続を起動できません。"`, line 354) is unrelated to the Launch feature and stays unchanged.

### 9.2 Declared Change Set

Feature-specific paths are not listed by hand here; create-plan derives them from each task's `files` in `workflow.yaml` (`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author explicitly removes them):
- `feature-docs/settings-profile-remove-launch/**`
- `test-docs/settings-profile-remove-launch/**`

`feature-docs/settings-profile-remove-launch/**` covers `REQUIREMENTS.md`, `SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design artifacts the design step produces. Their producers are the phase documents and `references/phase-state.md` (cited only; rules are not restated).

`test-docs/settings-profile-remove-launch/**` covers `{T}.tests.yaml` (path form: `test-docs/settings-profile-remove-launch/{T}.tests.yaml`). Its producer is `implement-phase.md` (cited only; rules are not restated).

**Semantics**:
- Default members are part of the declaration unless the SPEC author explicitly removes them. Removal is a deliberate narrowing, not an omission.
- The declaration is a superset assertion: the actual change set must be CONTAINED IN the declaration. A declared path that is never produced is not a violation. A feature that produces no implement tasks generates no `test-docs/settings-profile-remove-launch/` directory, and the declared `test-docs/settings-profile-remove-launch/**` is still correct.

## 10. Issues and Risks

### 10.1 Technical Issues
None.

### 10.2 Business Risks
None.

## 11. Success Criteria

### 11.1 Acceptance Criteria
- [ ] AC-1 (FR1): Rendering the Profiles section with one or more profiles produces no button labelled Launch / 起動; each profile item's `.profile-item-actions` contains exactly four buttons: default toggle, Edit, Duplicate, Delete.
- [ ] AC-2 (FR2): Clicking every action button of a rendered profile item dispatches no `profile:launch` event on `document`, and the source tree contains no `profile:launch` string.
- [ ] AC-3 (FR3): en.json and ja.json contain no `settings.profiles.launch` key.
- [ ] AC-4 (FR4, NFR1): The existing `profiles-section.test.ts` drag reorder tests (AC-1 to AC-5 of the predecessor feature) still pass unchanged, and `bun run typecheck` passes.

## 12. Test Scenarios

### 12.1 Test Viewpoints
- [ ] TS-1 (unit, bun test, happy-dom; `src-tauri/web-shared/settings/sections/profiles-section.test.ts`; covers AC-1): Render the Profiles section with profiles [A (is_default), B]; for each `.profile-list-item`, the `.profile-item-actions` button labels equal [unsetDefault|setDefault, edit, duplicate, delete] via `t()`, and no button text equals "Launch" or "起動".
- [ ] TS-2 (unit, bun test, happy-dom; `src-tauri/web-shared/settings/sections/profiles-section.test.ts`; covers AC-2): Register a `profile:launch` listener on `document`, render the section with a non-default profile, click each action button, and assert the listener was never called.
- [ ] TS-3 (unit, bun test; covers AC-3): Load en.json and ja.json and assert `settings.profiles` has no `launch` property.
- [ ] TS-4 (static check; covers AC-2, AC-3): `git grep -n 'profile:launch'` and `git grep -n 'settings.profiles.launch'` over `src-tauri` return no matches.
- [ ] TS-5 (regression; covers AC-4): Run `bun test` and `bun run typecheck`; all existing tests, including the profiles-section drag reorder tests, pass.

## 13. Glossary

| Term | Definition |
|------|------------|
| Launch | The per-profile action in the Profiles section that dispatches the `profile:launch` CustomEvent |

## 14. Confirmed Items

### 14.1 Confirmed
- [x] `profile:launch` listeners: `profile:launch` has no listener in TypeScript or Rust (the only occurrence is the dispatch at `profiles-section.ts:160`; no match in `settings-panel.ts`, `settings/web/entry.ts`, or `settings_window/commands.rs`), so removing the dispatch removes no working behavior elsewhere.
- [x] Shared helper and CSS: the shared `createActionButton` helper and the `.profile-action-btn` / `.profile-item-actions` CSS rules (`settings-panel.css:1236-1269`) stay, because the default toggle, Edit, Duplicate and Delete buttons still use them.
- [x] `sshNotConfigured`: ja.json's `sshNotConfigured` message (`"...SSH接続を起動できません。"`, line 354) is unrelated to the Launch feature and stays unchanged.
- [x] Recoverability: deleting the Launch code and i18n key is a source change recoverable from git history.

### 14.2 Open Items
None.

## 15. References

- `feature-docs/settings-profile-remove-launch/SPEC.md`
