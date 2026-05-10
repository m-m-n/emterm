# Feature: New Tab (Global Settings) Shortcut

## Overview

Add a dedicated keybind `new_tab_global` (default `Ctrl+Shift+G`) that always opens a new tab using global settings, bypassing any profile selector or default-profile logic. The existing `new_tab` (`Ctrl+Shift+T`) behavior is preserved.

## Objectives

- Provide a single-stroke shortcut to open a new tab with global settings regardless of profile state
- Maintain full backward compatibility with existing `new_tab` behavior and existing config files
- Integrate with the existing keybind customization UI and i18n infrastructure

## User Stories

### US1: Open a new tab with global settings instantly
As a user who has multiple profiles registered, I want to press `Ctrl+Shift+G` to open a new tab with the global settings, so that I can bypass the profile selector menu when I want a vanilla terminal.

**Acceptance Criteria:**
- [ ] Pressing `Ctrl+Shift+G` opens a new tab using global settings
- [ ] No profile selector menu is shown
- [ ] The new tab becomes active
- [ ] Behavior is identical regardless of whether profiles are registered or a default profile is set

### US2: Customize the shortcut
As a user, I want to remap `new_tab_global` to a different key combination, so that it fits my workflow.

**Acceptance Criteria:**
- [ ] The Settings panel exposes the shortcut under Keybinds → Tab Management
- [ ] Label is "New Tab (Global)" (en) / "新しいタブ (グローバル設定)" (ja)
- [ ] Editing and saving the shortcut takes effect immediately
- [ ] Older config files without the field load with the default `Ctrl+Shift+G`

## Technical Requirements

### Functional Requirements
- **FR1:** Add a `new_tab_global` field to `KeybindSettings` (Rust + TypeScript) with default value `"Ctrl+Shift+G"`.
- **FR2:** In `TabKeyboardHandler.handleKeyDown`, when the event matches `keybinds.new_tab_global`, call `tabManager.createTab()` directly (no profile selector, no default-profile branch).
- **FR3:** Add a keybind input row for `new_tab_global` in the Settings → Keybinds → Tab Management subsection, immediately after `new_tab`.
- **FR4:** Add i18n entry `settings.keybinds.newTabGlobal` to `src/i18n/locales/{en,ja}.json`.
- **FR5:** Existing `new_tab` (`Ctrl+Shift+T`) behavior, including default-profile auto-selection, must remain unchanged.

### Non-Functional Requirements
- **NFR1 - Performance:** Latency from key press to tab creation must match existing `new_tab` (single additional `matchKeybindStr` branch in the keydown handler).
- **NFR2 - Compatibility:** Older `config.json` files lacking `new_tab_global` (or with `null`) must load successfully via the existing `serde(default)` + `deserialize_null_with!` pattern.
- **NFR3 - Maintainability:** The Rust field must be declared via the existing `define_keybinds!` macro to avoid drift.
- **NFR4 - Cross-platform:** Must work identically on Linux and Windows (no platform-specific code paths required).

## Implementation Approach

### Architecture

**Layered View:**
```
┌──────────────────────────────────────────────┐
│ Frontend (TypeScript)                        │
│  - TabKeyboardHandler  (handle Ctrl+Shift+G) │
│  - keybinds-section.ts (Settings UI row)     │
│  - i18n locales        (label strings)       │
│  - KeybindSettings (types.ts)                │
├──────────────────────────────────────────────┤
│ Backend (Rust / Tauri)                       │
│  - KeybindSettings struct (define_keybinds!) │
│  - default_keybind_new_tab_global()          │
│  - deserialize_null_keybind_new_tab_global() │
└──────────────────────────────────────────────┘
```

**Component Diagram:**
```
KeyboardEvent
   │
   ▼
TabKeyboardHandler.handleKeyDown
   │
   ├── matchKeybindStr(event, keybinds.new_tab_global ?? "Ctrl+Shift+G")
   │     └─ matched ──▶ tabManager.createTab()  (no profile)
   │
   └── matchKeybindStr(event, keybinds.new_tab ?? "Ctrl+Shift+T")
         └─ matched ──▶ handleNewTab()  (existing profile-aware path)
```

### Data Flow

```
User presses Ctrl+Shift+G
   ↓
KeyboardEvent → TabKeyboardHandler.handleKeyDown
   ↓ (matches new_tab_global)
event.preventDefault()
   ↓
TabManager.createTab({})   // no profile argument
   ↓
PTY spawn (global settings: shell, cwd, env)
   ↓
New tab activated and rendered
```

### API Design

No IPC or HTTP API changes. The feature is fully contained in:
- Rust struct field via `define_keybinds!` macro (serialized to existing `config.json`)
- Frontend keyboard handler dispatch
- Frontend Settings panel rendering

The `KeybindSettings` JSON shape extends with one optional field:

```json
{
  "keybinds": {
    "new_tab": "Ctrl+Shift+T",
    "new_tab_global": "Ctrl+Shift+G",
    "close_tab": "Ctrl+Shift+W"
  }
}
```

### Database Schema

Not applicable. Settings persist via the existing config file (`config.json`) managed by `src-tauri/src/commands/config/`.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/commands/config/settings.rs` — `KeybindSettings` struct (extend via `define_keybinds!`)
- `src-tauri/src/commands/config/tests/defaults.rs` — default-value assertion test (extend with new field)
- `src/settings/types.ts` — `KeybindSettings` TypeScript interface (add field)
- `src/tab-bar/keyboard-handler.ts` — `TabKeyboardHandler.handleKeyDown` (add branch)
- `src/settings/sections/keybinds-section.ts` — Settings UI (add row)
- `src/i18n/locales/{en,ja}.json` — add `settings.keybinds.newTabGlobal`
- `src/keybind/matcher.ts` — `matchKeybindStr` (no change, reused)
- `src/tab-bar/tab-manager.ts` — `TabManager.createTab` (no change, reused)
- Test fixtures in `src/settings/settings-panel.test.ts`, `src/settings/settings-applier.test.ts`, `src/tab-bar/{tab-manager,tab-bar-ui,drag-handler}.test.ts` (add `new_tab_global` to mock `KeybindSettings`)

**External Dependencies:** None added. All changes use the existing stack (serde, Tauri, Bun).

### File Structure

Files to be modified (no new files required):

```
src-tauri/src/commands/config/
├── settings.rs                         # add new_tab_global to define_keybinds!
└── tests/
    └── defaults.rs                     # assert default = "Ctrl+Shift+G"

src/
├── settings/
│   ├── types.ts                        # add new_tab_global: string
│   ├── settings-panel.test.ts          # add new_tab_global to mock
│   ├── settings-applier.test.ts        # add new_tab_global to mock
│   └── sections/
│       └── keybinds-section.ts         # add UI row after new_tab
├── tab-bar/
│   ├── keyboard-handler.ts             # add branch in handleKeyDown
│   ├── keyboard-handler.test.ts        # add tests for Ctrl+Shift+G dispatch
│   ├── tab-manager.test.ts             # add new_tab_global to mock
│   ├── tab-bar-ui.test.ts              # add new_tab_global to mock
│   └── drag-handler.test.ts            # add new_tab_global to mock
└── i18n/locales/
    ├── en.json                         # "newTabGlobal": "New Tab (Global)"
    └── ja.json                         # "newTabGlobal": "新しいタブ (グローバル設定)"
```

### Implementation Sketches

**Rust (`src-tauri/src/commands/config/settings.rs`):**

Insert immediately after the `new_tab` entry inside `define_keybinds!`:

```rust
new_tab_global,   default_keybind_new_tab_global,   deserialize_null_keybind_new_tab_global,
                  "default_keybind_new_tab_global", "deserialize_null_keybind_new_tab_global",
                  "Ctrl+Shift+G";
```

**TypeScript types (`src/settings/types.ts`):**

```ts
export interface KeybindSettings {
  // ...
  new_tab: string;
  new_tab_global: string;   // NEW
  close_tab: string;
  // ...
}
```

**Keyboard handler (`src/tab-bar/keyboard-handler.ts`):**

Add the following branch *before* the existing `new_tab` branch (so the more specific binding takes precedence even if the user maps both to the same key):

```ts
// New tab using global settings (no profile)
if (matchKeybindStr(event, keybinds?.new_tab_global ?? "Ctrl+Shift+G")) {
  event.preventDefault();
  this.tabManager.createTab();
  return true;
}
```

**Settings UI (`src/settings/sections/keybinds-section.ts`):**

Insert between the existing `new_tab` and `close_tab` `renderKeybindInput` calls:

```ts
renderKeybindInput(
  tabGrid,
  "new_tab_global",
  t("settings.keybinds.newTabGlobal"),
  kb.new_tab_global,
  ctx.addContentListener,
  ctx.keybindCtx,
);
```

**i18n (`src/i18n/locales/en.json`, after `"newTab"`):**

```json
"newTab": "New Tab",
"newTabGlobal": "New Tab (Global)",
"closeTab": "Close Tab",
```

**i18n (`src/i18n/locales/ja.json`, after `"newTab"`):**

```json
"newTab": "新しいタブ",
"newTabGlobal": "新しいタブ (グローバル設定)",
"closeTab": "タブを閉じる",
```

## Test Scenarios

### Unit Tests

**Rust (`src-tauri/src/commands/config/tests/defaults.rs`):**
- [ ] `test_keybind_settings_default` — assert `keybinds.new_tab_global == "Ctrl+Shift+G"`
- [ ] (Existing test stays green) `keybinds.new_tab == "Ctrl+Shift+T"`

**TypeScript (`src/tab-bar/keyboard-handler.test.ts`):**
- [ ] `Ctrl+Shift+G` invokes `tabManager.createTab()` with no profile, regardless of profile state
- [ ] When profiles exist with a default profile, `Ctrl+Shift+G` does NOT call `tabBarUI.createTabWithProfile`
- [ ] `Ctrl+Shift+T` continues to use the existing profile-aware path (regression)
- [ ] When `keybinds.new_tab_global` is overridden in settings, the new key triggers the action

**TypeScript (`src/settings/settings-panel.test.ts`, `settings-applier.test.ts`, etc.):**
- [ ] Mock `KeybindSettings` objects include `new_tab_global` and tests still pass

### Integration Tests
- [ ] Loading a config JSON missing `new_tab_global` results in `keybinds.new_tab_global === "Ctrl+Shift+G"` (covered by Rust deserialization defaults; assert in `tests/defaults.rs`)
- [ ] Loading a config JSON with `"new_tab_global": null` results in the default value (covered by `deserialize_null_with!`)

### E2E Tests
**Existing E2E tests**: detected at `e2e-tests/specs/*.e2e.js`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression (run as part of `sdd.6-verify`)
- [ ] Optional new E2E spec: press `Ctrl+Shift+G` in a session with no profiles → new tab is created (verified via tab count and active tab)
  - Note: per the project TDD-scope feedback, do NOT run the full E2E suite during the inner TDD loop; defer to `sdd.6-verify`.

### Edge Cases
- [ ] User maps `new_tab_global` and `new_tab` to the same key (e.g. both `Ctrl+Shift+T`): the `new_tab_global` branch fires first because it appears earlier in `handleKeyDown`. Acceptable per spec; documented as user-controlled.
- [ ] User maps `new_tab_global` to an empty string: behavior follows existing `matchKeybindStr` semantics (no-op match). No special handling required.
- [ ] Config file from a previous version (no `new_tab_global` field) loads cleanly with default applied.
- [ ] `null` value in config JSON → default applied via `deserialize_null_with!`.

### Performance Tests
- [ ] Manual verification only: keypress-to-tab-render latency unchanged from `Ctrl+Shift+T` baseline.

## Security Considerations

- **Authentication / Authorization:** Not applicable (local desktop app).
- **Input Validation:** Keybind string validation is delegated to the existing keybind validator used for all other entries.
- **Data Protection:** Not applicable — only a string is stored.
- **XSS / Injection:** Not applicable.

## Error Handling

| Code / Case | Description | Behavior |
|-------------|-------------|----------|
| Tab creation failure | `tabManager.createTab()` returns `null` | Fall through to existing handling (no additional UX); already covered by current `new_tab` path |
| Invalid keybind string in config | Existing validator rejects bad string | Fall back to default per existing logic |
| Missing field in config | `serde(default = "default_keybind_new_tab_global")` applies default | Transparent to user |

No new error codes are introduced.

## Performance Optimization

### Performance Goals
- Keypress dispatch latency: same as existing `new_tab` (target < 1 ms additional overhead in `handleKeyDown`)

### Optimization Strategies
- Single additional `matchKeybindStr` call placed before `new_tab` branch — O(1) string match, no allocation in hot path beyond what the existing handler already performs.

### Caching Strategy
- Reuses `SettingsService.getCached()`; no new caching needed.

## Success Criteria

- [ ] All functional requirements (FR1–FR5) are implemented and tested
- [ ] All test scenarios (unit + integration) pass under Docker (`docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "..."`)
- [ ] `bun run typecheck` passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes
- [ ] `bun test` passes
- [ ] `cargo fmt` and `prettier` (or project formatter) leave the diff clean
- [ ] No regression in `Ctrl+Shift+T` behavior
- [ ] Settings UI renders the new row correctly in both English and Japanese
- [ ] Backward compatibility with existing `config.json` files verified

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。

なし — すべての要件は確認済み。

## Implementation Phases (if applicable)

### Phase 1: Backend keybind field
**Goals:** Add `new_tab_global` to Rust `KeybindSettings` and update default test.
**Deliverables:**
- Modified `settings.rs` with new `define_keybinds!` entry
- Updated `tests/defaults.rs` assertion

### Phase 2: Frontend type sync + dispatch
**Goals:** Add field to TS interface, wire `Ctrl+Shift+G` in `TabKeyboardHandler`, update mocks/tests.
**Deliverables:**
- Updated `types.ts`
- Updated `keyboard-handler.ts` + `keyboard-handler.test.ts`
- Updated existing test mocks (`tab-manager.test.ts`, `tab-bar-ui.test.ts`, `drag-handler.test.ts`, `settings-panel.test.ts`, `settings-applier.test.ts`)

### Phase 3: Settings UI + i18n
**Goals:** Expose the keybind in the Settings panel with localized labels.
**Deliverables:**
- Updated `keybinds-section.ts`
- Updated `i18n/locales/en.json` and `ja.json`

## References

- Requirements document: `doc/tasks/new-tab-global-shortcut/要件定義書.md`
- Existing keybind macro pattern: `src-tauri/src/commands/config/settings.rs:191-243`
- Existing keyboard dispatcher: `src/tab-bar/keyboard-handler.ts:54-122`
- Existing Settings UI section: `src/settings/sections/keybinds-section.ts`
- Project conventions: `CLAUDE.md` (Docker-first testing, Rust + TS + WASM stack)
