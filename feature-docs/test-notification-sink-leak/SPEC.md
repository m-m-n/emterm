# Feature: Stop the unit test suite from emitting real desktop notifications

## Overview

One unit test in `src-tauri/src/app.rs` drives a daemon agent-status update
through `App::pump_all()` while leaving `App::new()`'s production
`NotifyRustSink` in place, so every `cargo test` run pops a real desktop
notification ("eMterm / agent: (ブロック中)") over D-Bus. The fix points that
test at the existing `app_with_test_sink()` helper so the notification is
captured instead of sent. Production code is untouched.

## Objectives

- `cargo test` emits no OS desktop notification.
- The unit test suite no longer depends on the presence of a D-Bus session.
- The test's original assertion (a daemon `AgentStatusUpdate` reaches
  `App::agent_status` through `pump_all`) keeps its strength.

## User Stories

### US1: Running unit tests leaves the desktop alone

As a developer, I want `cargo test` to not pop desktop notifications, so that
running the test suite has no visible side effect on my session.

**Acceptance Criteria:**
- [ ] Running the unit test suite on a desktop with a live D-Bus session
      produces zero notifications.
- [ ] `pump_all_applies_daemon_agent_status_update_to_model` still passes.

## Technical Requirements

### Functional Requirements

- **FR1:** `pump_all_applies_daemon_agent_status_update_to_model`
  (`src-tauri/src/app.rs`) constructs its `App` via the existing
  `app_with_test_sink()` helper instead of `App::new()`, so its
  `notification_sink` is a capturing `TestNotifySink` and no notification
  reaches `NotifyRustSink`. The test's existing assertions — the
  `PaneKey::MuxPane(42)` entry exists with
  `state == Some(AgentState::Blocked)` and `revision == 7` — are preserved
  verbatim.

### Non-Functional Requirements

- **NFR1 - Test isolation:** The unit test suite performs no D-Bus / OS
  notification I/O. Its outcome does not depend on whether a notification
  daemon or D-Bus session bus is reachable.
- **NFR2 - No production change:** No file outside the `#[cfg(test)] mod tests`
  block of `src-tauri/src/app.rs` is modified. In particular `pump_all`'s
  transition drain, `App::new()`'s default sink assignment, and
  `NotifyRustSink` stay as they are.

## Implementation Approach

### Architecture

The relevant path is entirely inside `src-tauri/src/app.rs` plus the sink trait
in `src-tauri/src/callbacks.rs`:

```
Test
  └─ App (notification_sink: Arc<dyn NotificationSink>)
       ├─ production:  NotifyRustSink   → notify-rust → D-Bus → desktop
       └─ tests:       TestNotifySink   → in-memory capture
```

`App::pump_all()` drains `agent_status.drain_transitions()` and calls
`App::maybe_notify_agent_transition()`, which fires when the transition targets
`Blocked`/`Done`, the pane is not visible, both notification settings are on,
and the rate limiter allows it. The failing test satisfies all five conditions,
so the only variable that decides whether a real notification goes out is which
`NotificationSink` the `App` holds.

### Data Flow

```
AgentStatusUpdateMsg{state: Blocked}
  → Tab::apply_mux_message → App::on_mux_message
  → App::pump_all → drain_transitions → maybe_notify_agent_transition
  → App::notify → notification_sink.send(...)
```

Only the last hop changes: `notification_sink` becomes `TestNotifySink`.

### Dependencies

**Internal Dependencies:**
- `app_with_test_sink()` (`src-tauri/src/app.rs`, `#[cfg(test)] mod tests`):
  builds an `App`, asserts both notification settings default to on, and swaps
  `notification_sink` for an `Arc<TestNotifySink>`. It lives in the same
  `mod tests` as the target test, so it is directly callable.
- `TestNotifySink` (`src-tauri/src/callbacks.rs` test support): capturing sink.

**External Dependencies:** none added.

### File Structure

```
src-tauri/src/app.rs        # #[cfg(test)] mod tests — the single edited file
```

## Test Scenarios

### Unit Tests
- [ ] Test 1: `pump_all_applies_daemon_agent_status_update_to_model` passes with
      the test sink in place — the daemon update still lands in
      `App::agent_status` as `Blocked` / `revision == 7`.
- [ ] Test 2: The existing task0009 notification tests
      (`maybe_notify_agent_transition_*`) continue to pass unchanged.

### Integration Tests
- [ ] Test 1: The full library unit test suite passes
      (`cargo test --manifest-path src-tauri/Cargo.toml --lib`).

### E2E Tests
**Existing E2E tests**: None (no `e2e-tests/`, `tests/e2e/`, `playwright.config.*`,
`cypress.config.*`, or `docker-compose.e2e.yml` in the repository).
**Run command**: Not detected

### Edge Cases
- [ ] Edge case 1: Running the suite in an environment WITHOUT a D-Bus session
      must not change the outcome — no `warn` log about a failed notification is
      produced for this test, because nothing is sent at all.
- [ ] Edge case 2: No other unit test reaches the notification path with the
      production sink. Verified by scanning `mod tests` for `App::new()` uses
      that feed a `Blocked`/`Done` transition into `pump_all()`.

### Performance Tests
Not applicable.

## Security Considerations

Not applicable — the change removes an outbound IPC side effect from the test
suite and adds no new input handling.

## Error Handling

Not applicable — no error paths change.

## Success Criteria

- [ ] FR1 is implemented.
- [ ] All unit tests pass.
- [ ] `cargo fmt --check` passes for the crate.
- [ ] No desktop notification appears during a test run on a D-Bus-enabled
      desktop.
- [ ] `git diff` touches only `src-tauri/src/app.rs` (test module) and
      `feature-docs/`.

## Assumptions

Recorded because this feature was specified in batch mode without user dialogue.
Source of truth for each: the Notion task's 該当箇所 / 原因 / 期待する挙動 sections.

- **A1 — Minimal fix, not a structural guard.** The task's 期待する挙動 names the
  change explicitly ("`pump_all_applies_daemon_agent_status_update_to_model` も
  `app_with_test_sink()` を使うようにする（2〜3 行の変更）"). A broader structural
  guard — e.g. making `App::new()` default to a non-sending sink under
  `#[cfg(test)]` — was therefore NOT adopted, even though it would make the
  whole class of leak impossible. If a future audit finds further leaking tests,
  that is a separate task.
- **A2 — The report's "only one test leaks" claim is treated as a hypothesis to
  verify, not as scope.** The verification step scans for other `App::new()` +
  `pump_all()` + `Blocked`/`Done` combinations and reports what it finds; it does
  not fix additional sites under this feature.
- **A3 — The captured notification is not asserted on.** The test's subject is
  the model update, not the notification, so the returned sink is bound as `_`
  rather than asserted against. Adding a notification assertion would broaden the
  test's contract beyond what the task asks for.
- **A4 — Design step skipped.** No user-visible UI surface changes.

## References

- Notion task: [https://www.notion.so/3a83509ec8ee81d7873aec3beaaba5db](https://www.notion.so/3a83509ec8ee81d7873aec3beaaba5db)
- REQUIREMENTS.md: `feature-docs/test-notification-sink-leak/REQUIREMENTS.md`
- Commit that introduced the notification wiring: `db10cca` (2026-07-24, task0009)
