# Implementation Plan: notification-summary-markup-escape

## Overview

Apply the existing body-markup escape to the notification summary (title) at
the single D-Bus egress (`NotifyRustSink::send`), under the same per-send
capability gate the body already uses. This closes the summary half of the
dunst `markup=full` notification-phishing surface; PR #35 already covers the
body.

## Technology Stack

- **Rust** (existing `src-tauri` crate) — no new module, no new public
  interface; the change is confined to the notification egress.
- **notify-rust** (existing dependency) — D-Bus notification delivery and the
  Unix-only capability query. Reused as-is.
- **New dependencies: none.** Dependency licenses: no additions — project
  license (MIT) unaffected.

## Layer Structure

Single layer touched: the GUI-side notification egress in
`src-tauri/src/callbacks.rs`. No cross-layer changes, no new dependency
directions.

## Shared Components

This is a single-task feature, so there are no cross-task contracts. The
components below are existing code **reused unchanged**; their observable
behavior is a fixed constraint on implementation, review, and verification:

| Component | Location | Contract (fixed, unchanged) | Used by tasks |
|-----------|----------|------------------------------|---------------|
| `escape_body_markup` | `src-tauri/src/callbacks.rs:183-188` | Pure transform, no I/O. Precondition: any string. Postcondition: `&` is replaced first, then `<` and `>`; output contains no raw `<` or `>`; pre-existing entities are double-escaped (accepted, pinned by existing tests) | task0001 |
| `body_markup_confirmed` | `src-tauri/src/callbacks.rs:156-163` | Capability gate. Resolves "confirmed" only for a successful capability list containing `body-markup`; every other outcome (fetch failure, list without it) is "unconfirmed" | task0001 |
| `sanitize_title` | `src-tauri/src/notifications.rs:145-159` | Read-only in this feature; NOT modified (approach (a), per the answered gate `requirement.title-escape-approach`) | task0001 |

## Conventions

- **Test placement**: new unit tests live in
  `src-tauri/src/callbacks/tests.rs`, beside the PR #35 body-escape tests
  they mirror (`:695`, `:707`, `:751`, `:772`).
- **Formatting**: this project has no crate-wide format command
  (`format_command` is intentionally empty; a project hook formats individual
  edited files). No plan document introduces one.
- **Build/test invocation**: always from the project root with
  `--manifest-path src-tauri/Cargo.toml` and an explicit
  `CARGO_TARGET_DIR` — the exact approved commands are quoted in
  VERIFICATION.md.

## Cross-task Design Decisions

Feature-level decisions binding implementation, review, and verification:

### D1 — Single egress point (NFR1)

The escape is applied exactly once, at the sole D-Bus egress
(`NotifyRustSink::send`). No per-producer escaping is introduced; escaping at
the egress covers every summary producer (OSC 9, tab activity, agent status,
link-hover) for free, consistent with PR #35's D1 single-choke-point design.

### D2 — One gate evaluation per send

The existing per-send capability query is evaluated once per `send`, and that
single decision is applied to BOTH summary and body. No second capability
query is added, so the two fields can never receive divergent escape
decisions within one send.

### D3 — Fail-open parity with the body path (FR1, FR3)

When the gate resolves "unconfirmed", both summary and body pass through
byte-for-byte unchanged — identical to the existing body behavior. Fail-open
on capability-retrieval failure itself is out of scope (tracked separately).

### D4 — Escape at the egress only (assume.escape-at-egress-only)

Internal copies keep the raw title: `pending_notifications`
(`src-tauri/src/callbacks.rs:454-457`) and rate-limiter keys are not escaped.

### D5 — Platform scope (NFR2, assume.unix-only-gate)

The fix lives inside the existing Unix-only conditional-compilation scope
(`#[cfg(unix)]`), exactly where the body escape already sits. The Windows
toast path is unchanged (notify-rust exposes no capability query there).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Display degradation (`&lt;` shown literally) on spec-compliant servers that do not markup-render the summary | Medium | Low | Accepted trade-off of approach (a) (`assume.approach-a-escape-at-sink`); reversible |
| Behavioral drift in the body path while modifying `send` | Low | High | FR3 regression gate: existing PR #35 body-escape tests must pass unchanged (VERIFICATION.md TS5) |

## Open Questions

None. Every FR/NFR is `status: ok`; the escape-approach gate was already
resolved in create-spec (recorded as `assume.approach-a-escape-at-sink`).
