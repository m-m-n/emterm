# Implementation Plan: settings-profile-remove-launch

## Overview
Remove the per-profile Launch action, its `profile:launch` document event
dispatch and its `settings.profiles.launch` locale key from the settings
panel's Profiles section (SPEC.md FR1-FR3), leaving the remaining profile
actions unchanged (FR4). The work is planned as a single task, task0001.

## Technology Stack
- **Language / Framework**: TypeScript (vanilla, no framework) - the settings child WebView shared modules under `src-tauri/web-shared/`
- **Test runner**: Bun test with happy-dom (`bun test`; the test setup initializes the DOM and i18n)
- **Type checking**: TypeScript compiler through `bun run typecheck`
- **New dependencies**: none (nothing to check against `project.license` MIT)

## Layer Structure
| Layer | Location | Change |
|-------|----------|--------|
| Settings WebView UI (shared bundle, Windows and Linux) | `src-tauri/web-shared/settings/sections/` | The Profiles section loses the Launch action |
| Locale data | `src-tauri/web-shared/i18n/locales/` | `settings.profiles.launch` is removed from en and ja |
| Native backend (Rust, settings window IPC) | `src-tauri/src/settings_window/` | No change |

Dependency directions are unchanged: the Profiles section depends on the
i18n lookup, the shared settings components and the profile editor; nothing
depends on the Launch action.

## Shared Components
None. The feature has one task, so no component built by one task is used by
another task.

## Conventions
- Locale parity: en.json and ja.json keep identical key sets. A key removed from one file is removed from the other in the same change.
- Existing tests in `profiles-section.test.ts` keep their names and bodies (SPEC.md AC-4). Renaming one would trigger the update duty of `.claude/rules/test-docs-records.md`.

## Cross-task Design Decisions

### D1: Single task
- **Decision**: FR1-FR4 and NFR1-NFR2 are implemented by task0001 alone.
- **Rationale**: the change deletes one block in one module and one key in two locale files, and the tests live in the module's colocated test file. Split tasks would each own a fragment whose acceptance (TS-4: neither removed string remains under `src-tauri`) depends on another fragment's edit.
- **Affected tasks**: task0001

### D2: No native (Rust) change
- **Decision**: no file under `src-tauri/src/` changes.
- **Rationale**: `profile:launch` has no listener in TypeScript or Rust (REQUIREMENTS.md 14.1), so removing the dispatch removes no behavior elsewhere.
- **Affected tasks**: task0001

### D3: The removed-string search excludes test files
- **Decision**: SPEC.md AC-2's "the source tree contains no `profile:launch` string" and TS-4 are checked over git-tracked files under `src-tauri/` excluding `*.test.ts` files.
- **Rationale**: SPEC.md TS-2 registers a `profile:launch` listener in `profiles-section.test.ts` to prove the event is never dispatched, so that test file necessarily contains the string. This is a planning assumption made in batch mode; it is reversible by changing the TS-4 scope in VERIFICATION.md.
- **Affected tasks**: task0001 (AC-4), VERIFICATION.md TS-4

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Removing the locale entry leaves a locale file that is not valid JSON (a dangling separator) | Low | High - the i18n load fails for every settings string | TS-3 parses both files; the full `bun test` run (TS-5) initializes i18n from them |
| Another code path still looks up `settings.profiles.launch` and would show the raw key | Low | Low | TS-4 static search |
| The TS-2 test clicks Edit, which opens the profile editor in the shared happy-dom document, and leaves DOM state that affects later tests | Medium | Low | task0001 requires the test to remove what the click adds |

## Open Questions
None.
