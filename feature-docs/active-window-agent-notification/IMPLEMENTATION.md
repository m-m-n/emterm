# Implementation Plan: active-window-agent-notification

## Overview

Turn the hard-coded visible-pane suppression of agent status notifications
(blocked / done) into a persisted user setting, default ON: one new boolean in
the settings schema, one new input to the pure notification gate, and one new
toggle in the settings panel's Agent section.

## Technology Stack

- **Rust** — existing settings chain (`crates/app_settings` serde schema +
  `src-tauri/src/settings/` runtime loader) and the notification gate. No new
  crates.
- **TypeScript (vanilla) + Bun** — existing settings-panel WebView
  (`src-tauri/web-shared/settings`) and its i18n. No new packages.
- **License impact**: no new dependencies are introduced; `project.license`
  (MIT) is unaffected.

## Layer Structure

| Layer | Location | Responsibility |
|-------|----------|----------------|
| Persisted schema | `crates/app_settings` (always-built) | `AppSettings` serde shape of `settings.json`; owns defaults and null/missing resolution |
| Runtime settings | `src-tauri/src/settings/` (`mod.rs`, `raw.rs`) | GUI runtime `Settings` struct + raw loader that folds `settings.json` keys into it |
| Notification gating | `src-tauri/src/notifications.rs` | Pure gating decision (`should_fire_agent_notification`) — no GUI types, unit-testable headless |
| App wiring | `src-tauri/src/app/agent_status.rs` | Resolves pane visibility / rate-limit key, reads runtime settings, calls the gate, dispatches the notification |
| Settings UI | `src-tauri/web-shared/settings` + `src-tauri/web-shared/i18n` | TypeScript `AppSettings` mirror, Agent section toggles, en/ja labels |

Allowed dependency directions: app wiring → gating + runtime settings; runtime
settings ← `settings.json` → persisted schema; settings UI ↔ persisted schema
**by JSON key-name convention only** (no compile-time link — the key string is
the contract).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Settings field `agent_notify_visible_pane` | Persisted preference: may blocked/done transitions in a *visible* pane fire a desktop notification | JSON key `agent_notify_visible_pane` in `settings.json`, type boolean. Missing key or explicit null resolves to the default `true`; explicit `false` round-trips. Semantics: `true` = visible-pane transitions are notification-eligible (every existing gate still applies and takes precedence); `false` = visible-pane transitions stay suppressed (pre-feature behaviour). The value never affects non-visible-pane behaviour, tab-activity notifications, `mark_seen`/badge behaviour, or the rate-limit key/duration. | task0001 (Rust schema + gate), task0002 (TS mirror + toggle) |

## Conventions

- **Sibling-field pattern**: the new field follows `agent_notify_on_blocked`
  verbatim at every layer — same serde default-fn + null-wrapper pattern, same
  runtime-loader shape, same TS interface grouping, same `renderToggle` usage,
  same i18n key family (`settings.agent.*`). No new error handling or logging.
- **Naming family**: `agent_notify_*` (matches `agent_notify_on_done` /
  `agent_notify_on_blocked`).
- Requirement IDs referenced in tests/docs are hyphen-less (`FR1`, `NFR2`),
  matching workflow.yaml and SPEC.md.

## Cross-task Design Decisions

### D1: Field name is `agent_notify_visible_pane` (default ON)

SPEC.md deliberately left the field name open. Chosen name joins the existing
`agent_notify_*` family; default `true` per the recorded requirement decision
(`requirement.visible-pane-default`). Affects task0001 (declares it in Rust)
and task0002 (mirrors the exact key string in TS and saves under it).

### D2: The visibility gate is switched, not removed

The pure gate keeps its visibility conjunct but generalizes it: the visibility
condition passes when the pane is **not** visible OR the new setting is ON.
All other conjuncts (qualifying state, master, global, event-type, rate-limit)
are unchanged and continue to suppress independently of the new setting.
Affects task0001 (implements it) and task0002 (the toggle's description text
must describe exactly this semantics — an enabling toggle that cannot bypass
the other gates).

### D3: Mirror completeness

Every layer that declares the sibling field `agent_notify_on_blocked` must
declare the new field: the serde schema and its default set, the runtime
`Settings` struct and its constructor defaults, the raw loader
(optional-field + apply-if-present), the TS `AppSettings` interface, and every
test fixture that constructs a full settings object (Rust round-trip fixture
and the TS `makeSettings` helpers). Rationale: the project has two independent
Rust settings stacks plus a TS mirror; missing one layer silently resolves to
the default and produces a toggle with no effect. Affects both tasks (each
owns the layers inside its own file set).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Rust ↔ TS key-string drift (`agent_notify_visible_pane`) | Low | Toggle saves a key the backend ignores | Pinned Shared Component contract; TS-5 asserts the exact saved key; TS-4 asserts the exact JSON key |
| A settings layer missed (e.g. raw loader) → toggle has no runtime effect | Medium | Setting silently inert | D3 mirror completeness; loader-level tests are an explicit acceptance criterion of task0001 |
| Existing tests assert "visible pane never fires" under default settings — the new default flips that expectation | Certain | Test suite breaks if updated carelessly | task0001 deliberately updates those expectations AND keeps an explicit setting-OFF case pinning the legacy behaviour |
| Default ON increases notification volume for existing users | Medium | Low | Accepted requirement decision (`requirement.visible-pane-default`); the toggle turns it off |

## Open Questions

- None. All requirements are resolved (`status: ok`); design step was skipped
  (no DESIGN.md open items exist).
