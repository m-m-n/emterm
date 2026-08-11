# Feature: agent-notification-sanitize-title

## Overview

Agent status notifications embed the tab title into the notification body. That
title originates from OSC 0 / 2 and is untrusted input, yet it currently reaches
`notify_rust::Notification::body` — and therefore D-Bus / the OS notification
server — without sanitization. This feature routes the embedded title through
the existing `sanitize_title` inside `agent_notification_body`, resolving review
finding 7dd413bdd9289905 (severity: medium, category: security).

Requirements source: `feature-docs/agent-notification-sanitize-title/REQUIREMENTS.md`.

## Objectives

- Pass the tab title embedded in the agent status notification body through the
  existing `sanitize_title`, so that unsanitized untrusted input (OSC 0/2
  derived tab titles) never reaches D-Bus / the OS notification server via
  `notify_rust::Notification::body` — resolving review finding
  7dd413bdd9289905 (severity medium / security).

## Technical Requirements

### Functional Requirements

- **FR1 - Title sanitization in `agent_notification_body`** (status: resolved):
  Inside `agent_notification_body`, pass the embedded `tab_title` through the
  existing `sanitize_title`. Do this inside `agent_notification_body` as the
  choke point that closes both call sites in one place. Do not write a new
  sanitization implementation.
- **FR2 - Unit test pinning the sanitization** (status: resolved): Add a unit
  test to `notifications::tests` (following the inline `#[cfg(test)] mod tests`
  convention) pinning that tab titles containing CSI sequences / control
  characters do not survive into the notification body.

### Non-Functional Requirements

- **NFR1 - Reuse of the existing sanitizer** (status: resolved): Use the
  existing `sanitize_title` function; add no new sanitization implementation
  (preserving behavioral parity with the tab-activity notification path).
- **NFR2 - No impact on existing paths** (status: resolved): Do not change the
  behavior of the tab-activity notification path (which already passes through
  `sanitize_title`). Do not make the notification path asynchronous either
  (out of scope).

## Implementation Approach

### Data Flow

```
OSC 0/2 → tab title (untrusted)
        → agent_notification_body
            → sanitize_title (existing)   ← FR1: the single choke point
            → notification body string
        → notify_rust::Notification::body → D-Bus / OS notification server
```

Both call sites of `agent_notification_body` are covered because the
sanitization happens inside the function rather than at either call site.

### Dependencies

**Internal Dependencies:**

- `sanitize_title`: the existing sanitizer reused unchanged (NFR1). Per the
  recorded assumption it lives in `src-tauri/src/notifications.rs` and is
  already used by the tab-activity notification path (unverified — that path is
  outside `resolved_input_paths`).
- `agent_notification_body`: the function that assembles the notification body
  and embeds the tab title. Per the recorded assumption it has two call sites,
  both closed by sanitizing inside the function.

**External Dependencies:**

- `notify-rust`: the notification crate whose `Notification::body` carries the
  embedded title to D-Bus / the OS notification server. It is a `gui`-feature
  optional dependency, so the `notifications` module sits behind the `gui`
  feature and its tests run as default-feature unit tests.

## Test Scenarios

### Unit Tests

- [ ] TS1 (FR1, FR2, NFR1): Given a tab title containing a CSI sequence (e.g.
  `ESC [ ... m`), the value returned by `agent_notification_body` contains no
  escape / CSI bytes.
- [ ] TS2 (FR1, FR2): Given a tab title containing C0 control characters, no
  control character survives into the body.
- [ ] TS3 (FR1, NFR2): A normal tab title is still embedded into the body as
  before (no regression).

### Regression

- [ ] TS4 (NFR2): The whole existing `--lib` suite stays green. If the
  `tabs.rs` replay tests flake, re-run with `-- --test-threads=1`.

## Security Considerations

- **Input Validation:** OSC 0/2 derived tab titles are treated as untrusted
  input and passed through the existing `sanitize_title` before being embedded
  into the notification body (FR1 / NFR1).
- **Data Protection:** With the sanitization in place, unsanitized untrusted
  input no longer reaches D-Bus / the OS notification server through
  `notify_rust::Notification::body`.

## Success Criteria

- [ ] `agent_notification_body` passes the title through the existing
  `sanitize_title` internally (the choke point that closes both call sites in
  one place).
- [ ] A unit test pinning that CSI / control-character titles do not survive
  into the body exists in `notifications::tests`.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes.

## Constraints and Assumptions

- `sanitize_title` exists in `src-tauri/src/notifications.rs` and is currently
  used by the tab-activity notification path (stated in the task description;
  unread / unverified because it is outside `resolved_input_paths`).
- `agent_notification_body` has two call sites, and sanitizing inside that
  function closes both (stated in the task description).
- The finding targets PR #29 (`em-workflow/active-window-agent-notification/integration`,
  not yet merged into main). Confirming the state of main at the time work
  starts is a prerequisite task on the implementation side (constraint from the
  task description).
- The `notifications` module sits behind the GUI feature (`notify-rust` is a
  `gui` optional dependency), so its tests run as default-feature unit tests.

## Design Phase

Skipped: this is a security fix confined to notification-body assembly in the
Rust backend, with no change whatsoever to UI, visuals, layout, or design
tokens.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is `resolved`.

## References

- Requirements document: `feature-docs/agent-notification-sanitize-title/REQUIREMENTS.md`
- Review finding: 7dd413bdd9289905 (severity: medium, category: security)
