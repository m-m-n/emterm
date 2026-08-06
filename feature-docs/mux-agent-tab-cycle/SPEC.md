# Feature: mux-agent-tab-cycle

## Overview

A dedicated key operation cycles the active mux tab through only those tabs
that have an agent running. Tabs without a running agent are skipped, and the
operation is a no-op when the current tab is not a mux tab or when no
qualifying tab exists. Requirement details are defined in
`feature-docs/mux-agent-tab-cycle/REQUIREMENTS.md` (Japanese).

## Objectives

- Let a key operation step through only the mux tabs that have an agent running.
- Remove, through that cycling operation, the state where the user cannot tell
  which tab is running which agent.

## User Stories

### US1: Cycle through agent tabs

As a mux user, I want to press a dedicated key to move to the next mux tab that
has an agent running, so that I can reach agent tabs without scanning the tab
bar manually.

**Acceptance Criteria:**
- [ ] The key operation switches only among mux tabs that have an agent running.
- [ ] Tabs without a running agent are skipped while cycling.
- [ ] Cycling follows tab-bar display order, and the tab after the last
      qualifying tab is the first qualifying tab.

### US2: Predictable no-op behaviour

As a mux user, I want the key operation to do nothing when there is nothing to
cycle to, so that the operation never changes the active tab unexpectedly.

**Acceptance Criteria:**
- [ ] When the current tab is not a mux tab, the key operation is a no-op.
- [ ] When no tab has an agent running, the key operation is a no-op.

## Technical Requirements

### Functional Requirements

- **FR1 — Agent tab cycle key operation** (status: tbd): A dedicated key
  operation switches sequentially to mux tabs that have an agent running. The
  concrete key binding is undetermined.
  - TBD reason: Key binding undetermined (the Codex consultation for gate
    `create-spec.requirement-clarification` did not take place because of a usage
    limit, so `record_tbd` was applied; it is settled during create-plan's
    tbd-resolution). Analyst recommendation: `planning-default-configurable` —
    choose, at planning time, a default binding that does not collide with the
    existing key map, and make it configurable.
- **FR2 — Cycle order and wrap-around** (status: ok): Qualifying tabs are
  traversed in tab-bar display order, and the tab after the last qualifying tab
  is the first qualifying tab (wrap-around).
- **FR3 — Exclusion of normal tabs** (status: ok): Tabs without a running agent
  (normal tabs) are not included in the cycle.
- **FR4 — No-op on a non-mux tab** (status: ok): When the current tab is not a
  mux tab, the cycle key operation does nothing (no-op).
- **FR5 — No-op when no qualifying tab exists** (status: ok): When no tab has an
  agent running, the cycle key operation does nothing (no-op).
- **FR6 — Qualifying agent-state set** (status: tbd): The set of agent states
  that makes a tab count as "a tab with an agent running" for cycling purposes.
  Undetermined.
  - TBD reason: The qualifying agent-state set is undetermined (the Codex
    consultation for gate `create-spec.requirement-clarification` did not take
    place because of a usage limit, so `record_tbd` was applied; it is settled
    during create-plan's tbd-resolution). Analyst recommendation:
    `any-reported-state` — every mux tab containing a pane whose agent-status has
    not been cleared (regardless of idle / working / blocked / done).

### Non-Functional Requirements

- **NFR1 - GUI feature gate:** The implementation lives under the `gui` feature
  and does not break the `--no-default-features` (CLI-only) build.
- **NFR2 - Event-driven:** Determining the cycle targets is event-driven; no
  polling is introduced.
- **NFR3 - i18n:** Any user-visible string that is added follows the
  `crate::i18n` inline `t(ja, en)` convention.

## Implementation Approach

### Architecture

Behavioural addition to the existing tab-switching and key-input mechanisms.
The feature introduces no new UI surface (the design step is skipped for this
reason). Concrete module placement, the default key binding (FR1) and the
qualifying agent-state set (FR6) are decided in create-plan.

### Data Flow

```
Key event → cycle-target resolution (mux tabs with a qualifying agent state,
            in tab-bar display order) → activate next target tab
                                      → or no-op (FR4 / FR5)
```

### API Design

Not applicable — this feature exposes no network or CLI API surface.

### Database Schema

Not applicable — this feature persists no data.

### Dependencies

**Internal Dependencies:**
- Existing mux tab-switching and key-input mechanisms: the cycle operation is
  added to them.
- Per-pane agent status and its per-tab aggregation: the source of the
  qualifying-state judgement whose exact state set is FR6 (tbd).

**External Dependencies:**
- None beyond the crates the existing GUI build already uses.

### File Structure

To be determined in create-plan.

## Test Scenarios

### Unit Tests
- [ ] TS-1: Repeating the key operation while some of several mux tabs have an
      agent running transitions only through qualifying tabs, in display order —
      covers FR1, FR2, FR3, FR6.
- [ ] TS-2: With qualifying and non-qualifying tabs alternating, the
      non-qualifying tabs are skipped — covers FR3, FR6.
- [ ] TS-3: Operating from the last qualifying tab wraps around to the first
      qualifying tab — covers FR2.
- [ ] TS-4: With exactly one qualifying tab, behaviour matches the definition —
      covers FR1, FR2.

### Integration Tests
- [ ] TS-5: With zero qualifying tabs, the key operation leaves the active tab
      unchanged (no-op) — covers FR5.
- [ ] TS-6: With a non-mux tab active, the key operation does nothing (no-op) —
      covers FR4.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] Exactly one qualifying tab (TS-4).
- [ ] Zero qualifying tabs — no-op (TS-5).
- [ ] Active tab is not a mux tab — no-op (TS-6).
- [ ] Wrap-around from the last qualifying tab (TS-3).

### Performance Tests
Not applicable.

## Security Considerations

Not applicable — the feature adds no input parsing, no persistence and no
external interface.

## Error Handling

The two defined out-of-range conditions (non-mux active tab, no qualifying tab)
resolve as no-ops rather than errors; see FR4 and FR5.

## Performance Optimization

Target resolution is event-driven and introduces no polling (NFR2).

## Success Criteria

- [ ] The key operation switches only among mux tabs that have an agent running.
- [ ] Normal tabs without a running agent are skipped while cycling.
- [ ] When the current tab is not a mux tab, the key operation is a no-op.
- [ ] When no tab has an agent running, the key operation is a no-op.
- [ ] Cycling follows tab-bar display order and wraps from the last qualifying
      tab back to the first.
- [ ] All test scenarios (TS-1 … TS-6) pass.
- [ ] The `--no-default-features` (CLI-only) build still succeeds (NFR1).

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- [ ] FR1: Agent tab cycle key operation - Key binding undetermined (the Codex
      consultation for gate `create-spec.requirement-clarification` did not take
      place because of a usage limit, so `record_tbd` was applied; it is settled
      during create-plan's tbd-resolution). Analyst recommendation:
      `planning-default-configurable`.
- [ ] FR6: Qualifying agent-state set - The qualifying agent-state set is
      undetermined (same gate and same `record_tbd` resolution; settled during
      create-plan's tbd-resolution). Analyst recommendation: `any-reported-state`.

## Assumptions

- Cycling is required only in the forward direction (toward the next tab).
- Cycle order is tab-bar display order, with wrap-around.
- "No-op unless it is a mux tab" means the key operation does nothing when the
  currently active tab is not a mux tab.
- When no qualifying tab exists, the operation is a no-op.

## References

- Requirements document (Japanese): `feature-docs/mux-agent-tab-cycle/REQUIREMENTS.md`
- Phase state: `feature-docs/mux-agent-tab-cycle/phase-state/create-spec.yaml`
