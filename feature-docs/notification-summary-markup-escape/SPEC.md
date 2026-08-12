# Feature: notification-summary-markup-escape

## Overview

dunst with `markup=full` interprets markup in a notification's summary. Attacker-controlled titles (OSC 9 titles, OSC 0/2 tab-title fallbacks) therefore reach the summary as interpretable markup, while PR #35 already protects the body. This feature applies the existing `escape_body_markup` to the summary at the single D-Bus egress, `NotifyRustSink::send`, under the same per-send capability gate the body already uses.

Requirements source: `feature-docs/notification-summary-markup-escape/REQUIREMENTS.md`.

## Objectives

- Close the notification-phishing surface where dunst `markup=full` interprets markup in the notification summary, so attacker-controlled titles are not interpretable as markup in the summary, matching the protection PR #35 gave the body.

## User Stories

### US1: OSC 9 notification with an attacker-controlled title

As a user receiving desktop notifications emitted by a process running in the terminal, I want the notification summary to be free of interpretable markup, so that a crafted title cannot render as markup in the notification popup.

**Acceptance Criteria:**

- [ ] A tag-bearing title (e.g. containing `<a href>`) is escaped in the summary when capabilities confirm body-markup.
- [ ] The same title is left unchanged when capabilities are unconfirmed (fail-open parity with the body path).

### US2: Notification with a fallback title

As the same user, I want titles that come from the fallback branch (current tab title from untrusted OSC 0/2, or `"emterm"`) to be neutralized identically, so that the fallback is not a bypass.

**Acceptance Criteria:**

- [ ] The OSC 9 fallback-title branch flows through the same escaped summary path.

## Technical Requirements

### Functional Requirements

- **FR1 — Neutralize markup meta characters in the notification summary:** In `NotifyRustSink::send` (`src-tauri/src/callbacks.rs:148-174`), when the existing per-send capability gate `body_markup_confirmed(get_capabilities())` resolves "confirmed", apply `escape_body_markup` to the summary (title) exactly as is already done for the body, under the same `#[cfg(unix)]` scope. The unescaped `.summary(title)` at `src-tauri/src/callbacks.rs:166` is the fix site. `sanitize_title` (`src-tauri/src/notifications.rs:145-159`) is NOT changed (approach (a), per the answered gate).
- **FR2 — Cover the OSC 9 title path end to end:** The escape covers titles from `parse_osc9` (`src-tauri/src/callbacks.rs:681-693`) via `handle_notify` (`src-tauri/src/callbacks.rs:451-462`) into `sink.send`, including the fallback branch (current tab title from untrusted OSC 0/2, or `"emterm"`). Escaping at the single D-Bus egress also covers the other summary producers (tab activity, agent status, link-hover) for free, consistent with PR #35's D1 single-choke-point design.
- **FR3 — Body escaping unchanged:** The existing body escape behavior (escape order `&` → `<` → `>`, capability gate, fail-open on unconfirmed) is byte-for-byte unchanged; existing PR #35 tests in `src-tauri/src/callbacks/tests.rs` continue to pass.

### Non-Functional Requirements

- **NFR1 — Single egress point:** The escape is applied once, at the sole D-Bus egress (`NotifyRustSink::send`); no per-producer escaping is introduced.
- **NFR2 — Platform scope:** The fix lives under the existing `#[cfg(unix)]` gate; the Windows toast path is unchanged (notify-rust exposes no `get_capabilities()` there).

## Implementation Approach

### Architecture

The change is confined to the notification egress inside the Rust binary. No new component, module, or public interface is introduced.

```
PTY output (untrusted)
        │  OSC 9 / OSC 0/2
        ▼
parse_osc9            src-tauri/src/callbacks.rs:681-693
        ▼
handle_notify         src-tauri/src/callbacks.rs:451-462
        ▼
NotifyRustSink::send  src-tauri/src/callbacks.rs:148-174   ← single D-Bus egress (fix site :166)
        ▼
notify-rust → D-Bus → notification server (dunst, ...)
```

### Data Flow

```
title (raw, possibly attacker-controlled)
  → sanitize_title (unchanged; truncation etc.)
  → NotifyRustSink::send
      → body_markup_confirmed(get_capabilities())
          confirmed   → escape_body_markup(title) → .summary(...)
                        escape_body_markup(body)  → .body(...)      [unchanged, FR3]
          unconfirmed → title / body passed through byte-for-byte   [fail-open]
  → D-Bus
```

Internal copies keep the raw title: `pending_notifications` (`src-tauri/src/callbacks.rs:454-457`) and rate-limiter keys are not escaped; neutralization happens only at the egress.

### Dependencies

**Internal:**

- `escape_body_markup` (`src-tauri/src/callbacks.rs:183-188`) — reused unchanged.
- `body_markup_confirmed` capability gate (`src-tauri/src/callbacks.rs:156-163`) — reused unchanged.
- `sanitize_title` (`src-tauri/src/notifications.rs:145-159`) — read-only; not modified.

**External:**

- notify-rust — provides `get_capabilities()` on Unix only; the Windows toast path has no equivalent and is out of scope.

### File Structure

```
src-tauri/src/
├── callbacks.rs            # fix site: .summary(title) at :166, inside NotifyRustSink::send
└── callbacks/
    └── tests.rs            # new summary-escape tests alongside the PR #35 body tests
```

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1): `escape_body_markup` applied to a title with `<`, `>`, `&` produces the same entity output as the body path (order: `&` first; double-escape of pre-existing entities accepted, mirroring `src-tauri/src/callbacks/tests.rs:695`).
- [ ] **TS2** (FR1, FR3): composed sink decision — confirmed capabilities escape the title, unconfirmed leave it byte-for-byte unchanged (mirrors `src-tauri/src/callbacks/tests.rs:751`).
- [ ] **TS3** (FR1): a `sanitize_title`-truncated 100-char title ending in `<` escapes to a complete trailing entity (escape after truncation; mirrors `src-tauri/src/callbacks/tests.rs:707`, which already pins this composition for the body).
- [ ] **TS4** (FR2): the OSC 9 fallback-title branch (empty title segment → tab title or `"emterm"`) flows through the same escaped summary path.

### Regression

- [ ] Existing PR #35 body-escape tests in `src-tauri/src/callbacks/tests.rs` pass unchanged (FR3).

### Run command

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
```

### E2E Tests

**Existing E2E tests**: None (no E2E inputs were resolved for this feature).

## Security Considerations

- **Input Validation:** Markup meta characters in the summary are neutralized with `escape_body_markup` when the capability gate confirms body-markup (FR1).
- **Single choke point:** Neutralization is applied exactly once, at the sole D-Bus egress (NFR1); per-producer escaping is not introduced.
- **Fail-open behavior:** When capabilities are unconfirmed the title passes through unchanged, matching the body path. Fail-open on capability-retrieval failure itself is out of scope (tracked separately).
- **Platform scope:** Unix only, under the existing `#[cfg(unix)]` gate (NFR2).

## Out of Scope

- Body-side escaping (already handled by PR #35).
- Fail-open on capability-retrieval failure (separate task).
- Changing `sanitize_title` (excluded by the chosen approach (a)).

## Design Step

Skipped. Reason: backend-only security fix in the Rust notification egress; no UI surface, no visual inputs.

## Assumptions

- **assume.approach-a-escape-at-sink** (reversible): Remediation approach (a) escape-at-sink was selected — apply `escape_body_markup` to `summary(title)` in `NotifyRustSink::send` when the capability gate confirms body-markup; `sanitize_title` is not changed. The accepted trade-off is display degradation (`&lt;` shown literally) on spec-compliant servers that do not markup-render the summary. Source: resolved in batch mode by Codex consultation (packet `create-spec-q0001`, question `requirement.title-escape-approach`, source `batch-codex-consultation`, option `escape-at-sink`), not by the user; recorded as an assumption per `batch-policies.yaml` (`record_as_assumption: true`).
- **assume.unix-only-gate** (reversible): The fix applies under the same `#[cfg(unix)]` scope as the existing body escape; the Windows toast path stays unchanged.
- **assume.escape-at-egress-only** (reversible): Internal copies (`pending_notifications` at `src-tauri/src/callbacks.rs:454-457`, rate-limiter keys) keep the raw title; neutralization happens only at the D-Bus egress.

## Success Criteria

- [ ] A unit test fixes that a tag-bearing title (e.g. containing `<a href>`) is escaped in the summary when capabilities confirm body-markup, and left unchanged when unconfirmed (fail-open parity with the body path).
- [ ] Existing PR #35 body-escape tests still pass unchanged.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every FR and NFR has `status: resolved`.

## References

- Requirements document: `feature-docs/notification-summary-markup-escape/REQUIREMENTS.md`
- Fix site: `src-tauri/src/callbacks.rs:166` (`.summary(title)`); capability gate `src-tauri/src/callbacks.rs:156-163`; `escape_body_markup` `src-tauri/src/callbacks.rs:183-188`; `parse_osc9` `src-tauri/src/callbacks.rs:681-693`; `handle_notify` `src-tauri/src/callbacks.rs:451-462`
- Unchanged: `sanitize_title` `src-tauri/src/notifications.rs:145-159`
- Existing tests to mirror: `src-tauri/src/callbacks/tests.rs:695`, `:707`, `:751`, `:772`
- Origin: PR [https://github.com/m-m-n/emterm/pull/35](https://github.com/m-m-n/emterm/pull/35) review round1, finding `11996759d76a5041` (severity medium / category security)
