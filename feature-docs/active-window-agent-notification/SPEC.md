# Feature: active-window-agent-notification

## Overview

Agent status notifications (blocked / done) are currently suppressed for panes that are
visible — that is, panes in the focused window's active tab. This feature turns that
suppression into a user-controlled setting: when the new toggle is ON (the default),
blocked/done transitions in a visible pane fire a desktop notification; when it is OFF,
the existing suppression applies unchanged. The toggle is added to the settings panel's
Agent section and persisted in `settings.json`.

Requirements source: `feature-docs/active-window-agent-notification/REQUIREMENTS.md`.

## Objectives

- Let a user running agents across multiple tabs notice blocked/done transitions that
  happen in the focused window's active tab (the visible pane).
- Make the visible-pane notification behaviour controllable from the settings panel and
  persisted to `settings.json`.

## User Stories

### US1: Notification for a visible pane
As a user running agents in multiple tabs, I want blocked/done transitions in the focused
window's active tab to raise a desktop notification, so that I do not miss them.

**Acceptance Criteria:**
- [ ] With the new setting ON (default), a blocked/done transition in a pane of the focused
      window's active tab fires a desktop notification.
- [ ] With the new setting OFF, visible-pane notifications stay suppressed as before, and
      non-visible-pane notifications are unaffected.
- [ ] The existing master / global / event-type toggles and the per-pane 30 s rate limit can
      still suppress the notification.
- [ ] Tab activity notifications (output / bell / process-exit) behave exactly as before.

### US2: Controlling the behaviour from settings
As a user running agents in multiple tabs, I want a toggle in the settings panel's Agent
section, so that I can turn visible-pane notifications on or off myself.

**Acceptance Criteria:**
- [ ] The Agent section toggle changes the behaviour and the change is persisted to
      `settings.json` (label available in both en and ja).
- [ ] An existing `settings.json` that lacks the new key, or has it as null, resolves to the
      default (ON).

## Technical Requirements

### Functional Requirements

- **FR1 — Make the visible-pane notification gate configurable:** Change the `!pane_visible`
  gate in `should_fire_agent_notification` (`src-tauri/src/notifications.rs:263-278`) so that
  it is driven by a setting input rather than removed. When the new setting is ON,
  blocked/done transitions in a pane that is visible (a pane for which
  `agent_status_pane_visible`, `src-tauri/src/app/agent_status.rs:35-47`, returns true — the
  focused window's active tab) fire a desktop notification. When it is OFF, visible-pane
  notifications remain suppressed as today.
- **FR2 — Add the setting field (default ON):** Add a field for the visible-pane notification
  toggle to `AppSettings` in `crates/app_settings/src/settings.rs`, following the same pattern
  as the existing agent notification toggles (`default_true` + `deserialize_null_*`, same file
  lines 434-460), with default `true`. A missing key or a null value in `settings.json`
  resolves to the default (`true`).
- **FR3 — Mirror the schema in TypeScript:** Mirror the new field in the `AppSettings`
  interface in `src-tauri/web-shared/settings/types.ts` (the existing agent notification
  fields are at lines 73-75).
- **FR4 — Add the settings-panel toggle:** Add one toggle labelled "notify even for the
  visible pane" to the settings panel's Agent section
  (`src-tauri/web-shared/settings/sections/agent-section.ts`, existing order master → done →
  blocked), reusing the existing `renderToggle` component and i18n (en/ja,
  `src-tauri/web-shared/i18n/locales/{en,ja}.json`). The change is persisted to
  `settings.json`.
- **FR5 — Preserve the existing gates:** The master (`agent_status_notifications`), global
  (`notification_enabled`) and event-type (`agent_notify_on_done` / `agent_notify_on_blocked`)
  toggles, and the per-pane 30 s rate limit (`AGENT_NOTIFICATION_RATE_LIMIT`), keep applying as
  before and take precedence over the new setting in suppressing a notification.
- **FR6 — Scope limited to agent status notifications:** Only agent status notifications
  (blocked / done) change. The focus / visibility gates of tab activity notifications
  (output / bell / process-exit) are not modified.

### Non-Functional Requirements

- **NFR1 - Maintainability:** Follow the existing Settings pattern (serde default fn +
  `deserialize_null` wrapper, mirrored Rust and TypeScript schemas, reuse of `renderToggle` in
  the agent section).
- **NFR2 - Testability:** Keep `should_fire_agent_notification` a pure function that is unit
  testable without a GUI.
- **NFR3 - Compatibility:** Do not break the CLI build (`--no-default-features`);
  `app_settings` is an always-built crate.
- **NFR4 - Resource use:** Preserve the existing behaviour that the transition queue does not
  grow without bound while notifications are disabled.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────────────────────────┐
│ Settings panel (WebView)                                │
│   agent-section.ts  (renderToggle) + i18n en/ja         │
├─────────────────────────────────────────────────────────┤
│ web-shared/settings/types.ts  (AppSettings mirror)      │
├─────────────────────────────────────────────────────────┤
│ crates/app_settings  AppSettings  (serde, default true) │
│   ← persisted in settings.json                          │
├─────────────────────────────────────────────────────────┤
│ src-tauri/src/notifications.rs                          │
│   should_fire_agent_notification(pure fn)               │
│     global → master → event-type → visibility gate      │
│     → per-pane rate limit                               │
├─────────────────────────────────────────────────────────┤
│ src-tauri/src/app/agent_status.rs                       │
│   agent_status_pane_visible(pane)                       │
└─────────────────────────────────────────────────────────┘
```

**Component Diagram:**
```
agent-section.ts  --toggle-->  AppSettings (TS) --mirror--> AppSettings (Rust)
                                                                 |
                                     settings.json <--persist----+
                                                                 |
agent_status_pane_visible(pane) ----pane_visible---> should_fire_agent_notification
                                                                 |
                                                        desktop notification
```

### Data Flow

```
agent state transition (blocked / done / Clear)
  → global toggle (notification_enabled)
  → master toggle (agent_status_notifications)
  → event-type toggle (agent_notify_on_done / agent_notify_on_blocked)
  → visibility gate: pane_visible ? new setting : (pass)
  → per-pane 30 s rate limit (AGENT_NOTIFICATION_RATE_LIMIT)
  → fire desktop notification
```

Clear transitions (`new_state: None`) are not notification candidates, regardless of the new
setting.

### API Design

Not applicable — this feature adds no endpoint. The only interface change is one boolean field
in the `AppSettings` schema, mirrored between Rust and TypeScript.

### Database Schema

Not applicable — the persisted store is `settings.json`.

**Settings field:**

| Field | Type | Null | Default | Description |
|-------|------|------|---------|-------------|
| visible-pane notification toggle (field name not fixed by this specification) | bool | key may be absent or null | `true` | When true, blocked/done transitions in a visible pane fire a notification |

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/notifications.rs`: hosts `should_fire_agent_notification`, the gate being made
  configurable.
- `src-tauri/src/app/agent_status.rs`: supplies pane visibility via
  `agent_status_pane_visible`.
- `crates/app_settings`: owns the `AppSettings` serde schema and its defaults.
- `src-tauri/web-shared/settings`: TypeScript schema mirror and the Agent section UI.
- `src-tauri/web-shared/i18n/locales/{en,ja}.json`: toggle label strings.

**External Dependencies:**
- None added.

### File Structure

```
crates/app_settings/src/settings.rs                       # FR2: AppSettings field (default true)
src-tauri/src/notifications.rs                            # FR1/FR5: gate + rate limit
src-tauri/src/app/agent_status.rs                         # pane visibility source (unchanged)
src-tauri/web-shared/settings/types.ts                    # FR3: TypeScript mirror
src-tauri/web-shared/settings/sections/agent-section.ts   # FR4: toggle
src-tauri/web-shared/i18n/locales/en.json                 # FR4: en label
src-tauri/web-shared/i18n/locales/ja.json                 # FR4: ja label
```

## Test Scenarios

### Unit Tests
- [ ] **TS-1** (FR1, FR2, FR5, NFR2): `should_fire_agent_notification` unit test —
      (pane_visible × new setting ON/OFF) × (blocked/done) × master/global/event-type toggle
      combinations, verifying fire vs. suppress.
- [ ] **TS-2** (FR1): Clear transitions (`new_state: None`) are not notification candidates even
      with the new setting ON.
- [ ] **TS-3** (FR5): Rate-limit sharing — a notification fired for a visible pane consumes that
      same pane's 30 s rate limit.
- [ ] **TS-4** (FR2, NFR1): `app_settings` serde test — missing key / null resolves to `true`,
      and an explicit `false` round-trips.

### Integration Tests
- [ ] **TS-5** (FR4, NFR1): `agent-section.test.ts` — the new toggle renders, saves, and sits
      alongside the existing three toggles (following the existing test pattern).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] Covered by TS-2: Clear transition (`new_state: None`) with the new setting ON — no
      notification.
- [ ] Covered by TS-3: two blocked/done transitions in the same visible pane within 30 s — the
      second is suppressed by the shared per-pane rate limit.
- [ ] Covered by TS-4: an existing `settings.json` without the new key, or with it null —
      resolves to the default (ON).

### Type / Build Checks
- [ ] **TS-6** (FR3, NFR1): `bun run typecheck` — the `types.ts` mirror is consistent.
- [ ] **TS-7** (NFR3): CLI build non-regression (`cargo check --no-default-features`).

### Performance Tests
Not applicable.

## Security Considerations

- **Authentication / Authorization / SQL Injection / CSRF:** Not applicable — no network or
  database surface is involved.
- **Input Validation:** The new setting is a boolean; a missing key or a null value resolves to
  the default (`true`) through the existing `deserialize_null_*` wrapper (FR2).
- **Data Protection:** The only persisted value is a boolean preference in `settings.json`.
- **XSS Prevention:** The toggle reuses the existing `renderToggle` component and i18n strings;
  no new markup path is introduced.

## Error Handling

No new error codes. A malformed or absent value for the new setting key resolves to the default
(`true`) via the existing serde default path (FR2).

## Performance Optimization

- The transition queue must not grow without bound while notifications are disabled (NFR4) —
  the existing behaviour is preserved.
- No caching change; the per-pane 30 s rate limit (`AGENT_NOTIFICATION_RATE_LIMIT`) is unchanged
  and its key remains shared between visible and non-visible panes.

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and tested.
- [ ] All test scenarios (TS-1 – TS-7) pass.
- [ ] With the new setting ON (default), a blocked/done transition in the focused window's
      active tab fires a desktop notification.
- [ ] With the new setting OFF, visible-pane notifications are suppressed as before and
      non-visible-pane notifications are unaffected.
- [ ] The Agent section toggle changes the behaviour and persists to `settings.json` (en/ja).
- [ ] The existing master / global / event-type toggles and the per-pane 30 s rate limit still
      suppress notifications.
- [ ] An existing `settings.json` lacking the new key, or with it null, resolves to the default
      (ON).
- [ ] Tab activity notifications (output / bell / process-exit) are unchanged.
- [ ] The CLI build (`--no-default-features`) still compiles.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. Every requirement (FR1-FR6, NFR1-NFR4) is `resolved`.

## Assumptions

- The per-pane 30 s rate limit is unchanged, and its key stays shared between visible and
  non-visible panes (one notification per pane per 30 s). Reversible.
- Firing a notification for a visible pane does not change `mark_seen` / badge behaviour
  (task0005 AC-5). Reversible.
- Control shape: one toggle added to the Agent section, with the gate switched by the setting
  rather than removed — decided in batch-codex-consultation
  (question_id: `requirement.visible-pane-control-shape`). Reversible.
- The new setting defaults to ON — decided in batch-codex-consultation
  (question_id: `requirement.visible-pane-default`). Reversible.
- Scope is agent status notifications only; tab activity notifications are out of scope —
  decided in batch-codex-consultation (question_id: `requirement.notification-scope`).
  Reversible.

## References

- Requirements document: `feature-docs/active-window-agent-notification/REQUIREMENTS.md`
- `should_fire_agent_notification`: `src-tauri/src/notifications.rs:263-278`
- `agent_status_pane_visible`: `src-tauri/src/app/agent_status.rs:35-47`
- Existing agent notification toggles: `crates/app_settings/src/settings.rs:434-460`
- Existing agent notification fields (TS mirror): `src-tauri/web-shared/settings/types.ts:73-75`
- Settings panel Agent section: `src-tauri/web-shared/settings/sections/agent-section.ts`
- i18n locales: `src-tauri/web-shared/i18n/locales/{en,ja}.json`
