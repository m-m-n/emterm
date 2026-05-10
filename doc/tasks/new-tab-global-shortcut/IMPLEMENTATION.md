# Implementation Plan: New Tab (Global Settings) Shortcut

## Overview

Add a dedicated keybind `new_tab_global` (default `Ctrl+Shift+G`) that always opens a new tab using global settings, bypassing any profile selector or default-profile logic. Existing `new_tab` (`Ctrl+Shift+T`) behavior is preserved.

## Objectives

- Provide a single-stroke shortcut that opens a global-settings tab regardless of profile state.
- Preserve existing `new_tab` behavior and existing `config.json` files (forward/backward compatible).
- Integrate with the existing keybind customization UI and i18n infrastructure.

## Prerequisites

### Development Environment

- Rust toolchain matching the project (`rustup`, edition stable).
- Bun (latest), used for TypeScript build/test.
- Docker + docker compose (preferred for tests, per project policy).
- WebKitWebDriver / tauri-driver (only for E2E, not used in TDD inner loop).

### Dependencies

- Existing internal components only — no new crates or npm packages.
- The following must already exist (verified):
  - `define_keybinds!` macro and `deserialize_null_with!` helper in `src-tauri/src/commands/config/settings.rs`.
  - `matchKeybindStr` in `src/keybind/matcher.ts`.
  - `TabManager.createTab` accepting an optional profile argument.
  - `renderKeybindInput` helper used by `keybinds-section.ts`.

## Architecture Overview

### Technology Stack

- **Language**: Rust (backend) + TypeScript (frontend).
- **Framework**: Tauri 2 (desktop runtime), Bun (frontend tooling).
- **Key Libraries**:
  - `serde` / `serde_json` — config persistence (existing).
  - `rust-i18n` — backend i18n (existing).
  - Vanilla TypeScript + WASM — no new framework added.

### Design Approach

- Treat the change as a pure additive extension of the existing `KeybindSettings` model, not a behavioral change to existing handlers.
- Reuse `define_keybinds!` so the macro generates the field, default function, null-deserializer, and the `Default` impl entry consistently.
- Keep dispatch logic flat: a new branch in `TabKeyboardHandler.handleKeyDown` placed BEFORE the existing `new_tab` branch so that the more specific binding is matched first when keys collide.
- Mirror the Rust field into the TypeScript `KeybindSettings` interface to maintain shape parity. All test mocks of `KeybindSettings` need the new field to keep typecheck green.

### Component Interaction

- Settings load: `config.json` -> serde (Rust) -> Tauri command -> frontend `SettingsService` cache -> `TabKeyboardHandler.handleKeyDown` reads `keybinds.new_tab_global`.
- Key press: DOM `KeyboardEvent` -> `TabKeyboardHandler` matches `new_tab_global` -> calls `TabManager.createTab` with no profile -> backend spawns PTY using global settings -> tab activated.
- Settings UI: User opens settings panel -> `keybinds-section.ts` renders a row for `new_tab_global` using existing `renderKeybindInput` -> edit propagates through standard settings save pipeline.

## Implementation Phases

### Phase 1: Backend keybind field (Rust)

**Goal**: Extend the Rust `KeybindSettings` with `new_tab_global` (default `"Ctrl+Shift+G"`) using the existing `define_keybinds!` macro, and assert the default value via unit test.

**Files to Create**: None.

**Files to Modify**:

- `src-tauri/src/commands/config/settings.rs` — add new entry inside `define_keybinds!` immediately after `new_tab`.
- `src-tauri/src/commands/config/tests/defaults.rs` — extend default-value assertion to cover `new_tab_global == "Ctrl+Shift+G"`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `KeybindSettings` (struct) | Persisted keybind table | Existing macro is intact | Has new field `new_tab_global: String` |
| `default_keybind_new_tab_global` | Provide default value `"Ctrl+Shift+G"` | None | Returns the constant string |
| `deserialize_null_keybind_new_tab_global` | Map JSON `null` -> default | Field present | Returns default if input is null |
| `KeybindSettings::default` | Construct fully-populated default | All field default fns exist | New field initialized to default |

**Processing Flow** (config load):

1. serde reads `config.json`.
2. For `keybinds.new_tab_global`:
   - Field missing -> serde uses macro-generated default function.
   - Field is `null` -> macro-generated null deserializer substitutes default.
   - Field is a string -> stored verbatim.
3. The resulting `KeybindSettings` propagates through the existing config command pipeline unchanged.

**Implementation Steps** (5–7 max):

1. **Add macro entry** — Insert a new `define_keybinds!` row for `new_tab_global` directly after the `new_tab` row, using the macro's existing 3-line pattern (function names, string literal pair, default).
2. **Write failing default test first (TDD-Red)** — Extend the existing default-values test in `tests/defaults.rs` to assert `keybinds.new_tab_global == "Ctrl+Shift+G"`. Confirm it fails before adding the macro entry.
3. **Run macro entry, observe Green** — Execute Rust unit tests under Docker; the new assertion should now pass.
4. **Verify deserialization edge cases** — Add (or confirm existing coverage) two assertions: missing field uses default, JSON `null` uses default. These are straightforward additions to `tests/defaults.rs` or a sibling test module if one exists.
5. **Run cargo fmt** — Ensure style consistency.

**Dependencies**: Blocks Phase 2 (TS interface mirrors this field) and Phase 3 (Settings UI references the field name).

**Testing Approach**:

- Unit: assert default value equals `"Ctrl+Shift+G"`; assert deserialization of missing field and `null` value yields default.
- Integration: covered indirectly by existing settings load/save tests once the field exists.
- E2E: deferred to `sdd.6-verify`.
- Manual: none in this phase.

**Acceptance Criteria**:

- [ ] `KeybindSettings` exposes a `new_tab_global: String` field with default `"Ctrl+Shift+G"`.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes including the new assertion.
- [ ] `cargo fmt` produces no diff.
- [ ] Field naming follows the existing snake_case + `Ctrl+Shift+...` pattern.

**Estimated Effort**: small.

---

### Phase 2: Frontend type sync, dispatch, and test mocks

**Goal**: Mirror the new field in the TypeScript `KeybindSettings` interface, dispatch `Ctrl+Shift+G` to a profile-less `TabManager.createTab` call from `TabKeyboardHandler`, and update every test fixture that constructs a `KeybindSettings` mock.

**Files to Create**: None.

**Files to Modify**:

- `src/settings/types.ts` — add `new_tab_global: string` to `KeybindSettings`.
- `src/tab-bar/keyboard-handler.ts` — add a new branch in `handleKeyDown` placed BEFORE the `new_tab` branch.
- `src/tab-bar/keyboard-handler.test.ts` — add focused tests for the new dispatch (TDD).
- `src/settings/settings-panel.test.ts` — extend mock `KeybindSettings` to include the new field.
- `src/settings/settings-applier.test.ts` — same.
- `src/tab-bar/tab-manager.test.ts` — same.
- `src/tab-bar/tab-bar-ui.test.ts` — same.
- `src/tab-bar/drag-handler.test.ts` — same.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `KeybindSettings` (TS interface) | Frontend shape mirror | Rust struct has the field (Phase 1) | Interface includes `new_tab_global: string` |
| `TabKeyboardHandler.handleKeyDown` | Map keyboard events to tab actions | Settings cache returns `KeybindSettings` (or undefined) | When `new_tab_global` matches, calls `tabManager.createTab()` with no profile and returns `true` |
| Existing test mocks | Provide complete `KeybindSettings` fixtures | Tests typecheck under strict mode | Each fixture includes `new_tab_global` |

**Processing Flow** (key dispatch):

1. `KeyboardEvent` arrives at `TabKeyboardHandler.handleKeyDown`.
2. Read `keybinds = SettingsService.getCached()?.keybinds`.
3. Branch order in `handleKeyDown`:
   - Match `keybinds?.new_tab_global ?? "Ctrl+Shift+G"`?
     - Yes -> `event.preventDefault()`, call `tabManager.createTab()` (no profile), return `true`.
     - No -> continue to existing branches (`profile_selector`, `new_tab` profile-aware, etc.) unchanged.
4. The branch is placed before the existing `new_tab` branch so collisions resolve to the more specific binding.

**Function Contracts** (no code, contracts only):

- `handleKeyDown(event)` postcondition (added clause):
  - Precondition: `event` is a `KeyboardEvent` produced by the focused tab area.
  - Postcondition: If `event` matches the configured `new_tab_global` keybind (or default `Ctrl+Shift+G` when unset), the function calls `tabManager.createTab()` with no profile argument exactly once, prevents default, and returns `true`. The `tabBarUI.createTabWithProfile` path is not invoked.

**Implementation Steps**:

1. **Write failing keyboard handler tests first (TDD-Red)** — Add tests covering: (a) `Ctrl+Shift+G` triggers `tabManager.createTab` with no profile when no profiles exist, (b) same when profiles exist with default profile (must NOT call `tabBarUI.createTabWithProfile`), (c) `Ctrl+Shift+T` regression preserved, (d) custom keybind override (e.g. `Ctrl+Alt+N`) works after settings change. Run tests — they fail.
2. **Add field to `KeybindSettings` interface** — Update `src/settings/types.ts`.
3. **Update test mocks across the project** — Add `new_tab_global: "Ctrl+Shift+G"` (or appropriate value for the test) to every mock object that constructs `KeybindSettings` in the listed test files. Run typecheck to confirm no other usages remain.
4. **Add dispatch branch in `handleKeyDown`** — Place the new branch before `new_tab`. Use `matchKeybindStr` with the same `?? "Ctrl+Shift+G"` fallback pattern used by sibling branches.
5. **Run TDD-Green** — `bun test` should pass for the new and existing tests; `bun run typecheck` should pass.
6. **Refactor pass** — Confirm branch ordering documented inline, no duplicated logic; collapse only if it does not reduce clarity.

**Dependencies**: Requires Phase 1 (field name finalized). Blocks Phase 3 (Settings UI label references the same field).

**Testing Approach**:

- Unit (TS): four scenarios listed above (Ctrl+Shift+G with/without profiles, Ctrl+Shift+T regression, override).
- Integration: indirectly covered by existing settings-applier / settings-panel tests once mocks are updated.
- E2E: deferred to `sdd.6-verify`.
- Manual: none in this phase.

**Acceptance Criteria**:

- [ ] `KeybindSettings` interface includes `new_tab_global: string`.
- [ ] All existing TS tests still pass after mocks are extended.
- [ ] New keyboard handler tests pass and document the four scenarios above.
- [ ] `bun run typecheck` passes with no errors anywhere in the repo.
- [ ] No call to `tabBarUI.createTabWithProfile` happens when `Ctrl+Shift+G` matches.

**Estimated Effort**: medium.

---

### Phase 3: Settings UI row + i18n labels

**Goal**: Surface `new_tab_global` as an editable row in Settings → Keybinds → Tab Management (immediately after `new_tab`), with localized labels in English and Japanese.

**Files to Create**: None.

**Files to Modify**:

- `src/settings/sections/keybinds-section.ts` — insert a `renderKeybindInput` row for `new_tab_global` between the existing `new_tab` and `close_tab` rows.
- `src/i18n/locales/en.json` — add `settings.keybinds.newTabGlobal` = `"New Tab (Global)"`, placed adjacent to `newTab`.
- `src/i18n/locales/ja.json` — add `settings.keybinds.newTabGlobal` = `"新しいタブ (グローバル設定)"`, placed adjacent to `newTab`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `keybinds-section.ts` (Tab Management subsection) | Render keybind editing rows in correct order | `KeybindSettings` interface includes `new_tab_global` (Phase 2) | A row labeled by `settings.keybinds.newTabGlobal` appears between `new_tab` and `close_tab` |
| English locale | Provide `newTabGlobal` label | i18n structure under `settings.keybinds` exists | `settings.keybinds.newTabGlobal` resolves to `"New Tab (Global)"` |
| Japanese locale | Provide `newTabGlobal` label | Same as above | `settings.keybinds.newTabGlobal` resolves to `"新しいタブ (グローバル設定)"` |

**Processing Flow** (UI render):

1. User opens Settings → Keybinds.
2. The Tab Management subsection iterates keybind rows.
3. After rendering `new_tab`, the new row is rendered using the same helper:
   - Reads `kb.new_tab_global` from current settings.
   - Resolves the label via `t("settings.keybinds.newTabGlobal")` (locale fallback: English).
4. User edits and saves the value via the existing keybind input pipeline.

**Implementation Steps**:

1. **Add i18n keys first** — Update both locale files; this avoids any temporary "missing translation" warnings during UI work.
2. **Add UI row** — Insert the new `renderKeybindInput` invocation between `new_tab` and `close_tab` in `keybinds-section.ts`, mirroring the surrounding rows' argument shape.
3. **Verify ordering** — Visual order top-to-bottom in the Tab Management subsection: `new_tab` → `new_tab_global` → `close_tab` → `next_tab` → `prev_tab` → `profile_selector`.
4. **Run typecheck and unit tests** — Existing settings panel tests should not need behavioral changes (mocks were already updated in Phase 2). Confirm `bun run typecheck` and `bun test` are green.
5. **Format pass** — Run `bun x prettier --write` on touched files.

**Dependencies**: Requires Phase 2 (interface field). No downstream blockers within this feature.

**Testing Approach**:

- Unit (TS): no new tests strictly required; existing settings panel tests assert the section renders without error and the interface is well-typed.
- Integration: covered by `settings-panel.test.ts` once mocks include the new field.
- E2E: deferred to `sdd.6-verify`. Optional new spec: open settings, scroll to Tab Management, verify presence of the new row.
- Manual: visual confirmation that the row appears in correct position with correct label in both locales.

**Acceptance Criteria**:

- [ ] Settings UI shows the new row directly after `new_tab` and before `close_tab`.
- [ ] English label reads `New Tab (Global)`; Japanese label reads `新しいタブ (グローバル設定)`.
- [ ] Editing the row changes `keybinds.new_tab_global` in the saved config.
- [ ] `bun run typecheck` and `bun test` remain green.
- [ ] Locale JSON files remain valid JSON and pass any project-wide JSON linting.

**Estimated Effort**: small.

---

## Complete File Structure

```
emterm/
├── src-tauri/
│   └── src/commands/config/
│       ├── settings.rs                     # MODIFIED: define_keybinds! entry for new_tab_global
│       └── tests/
│           └── defaults.rs                 # MODIFIED: assert default + null-handling
└── src/
    ├── settings/
    │   ├── types.ts                        # MODIFIED: KeybindSettings.new_tab_global
    │   ├── settings-panel.test.ts          # MODIFIED: mock includes new_tab_global
    │   ├── settings-applier.test.ts        # MODIFIED: mock includes new_tab_global
    │   └── sections/
    │       └── keybinds-section.ts         # MODIFIED: render row after new_tab
    ├── tab-bar/
    │   ├── keyboard-handler.ts             # MODIFIED: dispatch Ctrl+Shift+G
    │   ├── keyboard-handler.test.ts        # MODIFIED: 4 new scenarios
    │   ├── tab-manager.test.ts             # MODIFIED: mock includes new_tab_global
    │   ├── tab-bar-ui.test.ts              # MODIFIED: mock includes new_tab_global
    │   └── drag-handler.test.ts            # MODIFIED: mock includes new_tab_global
    └── i18n/locales/
        ├── en.json                         # MODIFIED: settings.keybinds.newTabGlobal
        └── ja.json                         # MODIFIED: settings.keybinds.newTabGlobal
```

No new files are created.

## Testing Strategy

- **Unit tests** (Docker):
  - Rust: default value, missing field, null value all yield `"Ctrl+Shift+G"`.
  - TypeScript: 4 keyboard handler scenarios (with/without profiles, regression, override).
  - Test mocks across 5 TS test files updated to include the new field.
- **Integration tests**: covered by existing settings load/save tests once the field is present.
- **E2E**: per project policy (CLAUDE.md, `feedback_tdd_scope.md`), the full E2E suite is NOT run during the inner TDD loop. It is executed as part of `sdd.6-verify`. Optional new spec for `Ctrl+Shift+G` may be added but is not required.
- **Manual**: visual UI confirmation in both locales; latency comparison with `Ctrl+Shift+T` baseline.
- **Coverage targets**: keep parity with existing `new_tab` coverage. Touched files do not lower overall coverage.

All tests run under Docker:

- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"`
- `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"`

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none added) | — | All work uses existing dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Missed test mock causes typecheck failure | Medium | Low | Phase 2 step 3 explicitly enumerates and updates every mock; rely on `bun run typecheck` |
| Branch ordering causes `new_tab_global` to mask `new_tab` when keys collide | Low | Low | Document ordering inline; behavior is acceptable per spec (user-controlled) |
| Existing `Ctrl+Shift+G` collision with OS / WM | Low | Low | Spec verified — no existing global registration; user can rebind |
| serde default mismatch between Rust and TS interface | Low | Medium | Single literal `"Ctrl+Shift+G"` repeated only in (1) the Rust macro and (2) the TS handler fallback. Both are covered by tests |
| Locale JSON corruption from manual edit | Low | Low | Run JSON parser via prettier; existing settings panel tests load locales |

## Open Questions

- [ ] None. The spec confirms all decisions (`14.2 未確認・保留事項: なし`).

## Success Metrics

- [ ] FR1–FR5 all implemented and verified by tests.
- [ ] NFR1–NFR4 satisfied (latency parity, backward compat, macro use, cross-platform).
- [ ] All success criteria from SPEC.md §Success Criteria check.
- [ ] No regression in `Ctrl+Shift+T` behavior, verified by dedicated regression test.
- [ ] `cargo test`, `bun test`, `bun run typecheck`, `cargo fmt`, `prettier` all clean.
