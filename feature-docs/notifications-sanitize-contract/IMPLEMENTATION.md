# Implementation Plan: notifications-sanitize-contract

## Overview

Unify the tab-title sanitization contract of the two notification body builders in `crate::notifications` (`notification_body`, `agent_notification_body`) behind a `SanitizedTitle` newtype that, outside the module, only `sanitize_title` can produce. A raw tab title reaching either builder from outside the module becomes a compile-time type error. Notification text and notification behavior stay byte-for-byte unchanged.

## Technology Stack

- **Language / Framework**: Rust, the `emterm` crate under `src-tauri/`. `crate::notifications` stays behind the `gui` feature (NFR2).
- **Key libraries**: none added. The existing CSI regex use inside `sanitize_title` is unchanged.
- **New dependencies and their licenses**: none. `project.license` (MIT) is unaffected.

## Layer Structure

| Layer | Module | Responsibility in this feature |
|-------|--------|--------------------------------|
| Notification policy | `crate::notifications` (`src-tauri/src/notifications.rs`) | Owns `SanitizedTitle`, `sanitize_title`, both body builders and `AgentTransition`. The only place that can produce a `SanitizedTitle`. |
| Callers | `crate::app` (`App::pump_all` in `src-tauri/src/app/mod.rs`, `App::maybe_notify_agent_transition` in `src-tauri/src/app/agent_status.rs`) | Turn the raw tab title into a `SanitizedTitle` through `sanitize_title` before calling a body builder. |
| Upstream name source | `crate::agent_status` (core parser) | Sanitizes the agent name at parse time (private `sanitize_name`). Not modified. |
| Downstream sink | `crate::callbacks` (`NotificationSink`, markup escape) | Consumes the finished body string. Only its tests change. |

Allowed dependency direction: `app` → `notifications`. `notifications` never depends on `app`. The agent name flows `agent_status` → `app` → `AgentTransition::name` → `agent_notification_body` without re-sanitization.

## Shared Components

The feature is one task (see D1). The contract is still pinned here because any later rework task that touches notification bodies has to implement against it.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `SanitizedTitle` (type in `crate::notifications`) | Carries tab-title text that has passed through `sanitize_title` | **Invariant**: the text has no CSI sequence of the form the existing CSI pattern matches and no C0 / DEL / C1 control character, and it is at most 100 characters long. **Public surface**: (1) a read-only accessor that returns the text as a borrowed string slice (`as_str`), and (2) `Display` formatting that writes exactly the same text. **Excluded from the public surface**: a public field, any public constructor that takes raw text, conversion traits in either direction between the type and `String` / `&str` (`From`, `Into`, `FromStr`, `Deref`, `AsRef`, `Borrow`), `Default`, and deserialization. **Permitted derives**: debug formatting, cloning, and equality between two `SanitizedTitle` values. None of these can produce a value from raw text. Code inside `crate::notifications` may construct the type directly (SPEC a4), but only `sanitize_title` does so. | task0001 |
| `sanitize_title(raw title text) -> SanitizedTitle` | The only producer of `SanitizedTitle` outside the module | **Pre**: any string, including multi-MiB OSC payloads. **Post**: for every input, the contained text is byte-identical to what the function returns today: the input is capped at 4096 characters, CSI sequences are stripped, C0/C1/DEL characters are removed, and the result is cut to 100 characters (FR6). | task0001 |
| `notification_body(title: &SanitizedTitle, kind: ActivityKind, locale: Locale) -> String` | Tab-activity notification body | **Pre**: the title comes from `sanitize_title`. **Post**: returns `"{title text}: {localized kind message}"`, byte-identical to today's output for the same sanitized text, in both locales. | task0001 |
| `agent_notification_body(transition: &AgentTransition, title: &SanitizedTitle, locale: Locale) -> String` | Agent-status notification body | **Pre**: the title comes from `sanitize_title`, and `transition.name` was sanitized upstream. **Post**: returns `"{name or locale fallback}: {title text} ({localized state message})"`. For a raw title `r`, calling it with `sanitize_title(r)` gives the same body that today's function gives for `r`. The function sanitizes nothing itself, neither the title nor the name. | task0001 |
| `AgentTransition::name: Option<String>` | Agent name shown in the agent body (type unchanged) | **Trust contract**: sanitized at parse time by `crate::agent_status`'s `sanitize_name`. Body builders use it as-is and do not re-sanitize it (FR4). | task0001 |

## Conventions

- **Where sanitization happens**: a caller sanitizes a raw tab title with `sanitize_title` at the point it hands the title to the notification path, and passes a reference to the result to the builder. The builders themselves never sanitize.
- **Runtime error handling**: none is added. Misuse is a compile-time type error (SPEC Error Handling).
- **Tests**: tests obtain a `SanitizedTitle` only through `sanitize_title`, including tests inside `crate::notifications`, and compare sanitized text through the read accessor.
- **Doc comments**: English. Items that are private in another module, such as `agent_status`'s `sanitize_name`, are named as plain code-formatted paths, not as intra-doc links, so that no private-item doc-link warning is introduced.
- **Build / test hygiene**: run cargo from the worktree root with `--manifest-path src-tauri/Cargo.toml` and `CARGO_TARGET_DIR=src-tauri/target`. Do not run release builds. Do not run crate-wide formatting. Only files in the task's declared file set may change.

## Cross-task Design Decisions

### D1: One task for the whole change set

Changing the return type of `sanitize_title` and the title parameter type of both builders breaks every caller and several tests at compile time. No proper subset of the change compiles in an isolated worktree, and tasks run fully in parallel with no ordering between them. The type, the builders, the doc comments, the two call sites and the migrated tests are therefore one task (task0001).

### D2: Newtype, not internal re-sanitization

The SPEC fixes option (b), the `SanitizedTitle` newtype (FR1, FR2). This plan does not revisit that choice. The type-level guarantee applies at the module boundary (SPEC a4).

### D3: Explicit read access only

The only read paths are the explicit accessor and `Display` (FR1). There is no implicit deref or `AsRef` to a string slice. This keeps every use of sanitized text visible as an accessor call at the call site, which matches the migrated tests in TS4 and TS5. It also prevents a `SanitizedTitle` from silently decaying into a plain string slice in code where a raw title is in scope.

### D4: Where the agent path sanitizes

`App::maybe_notify_agent_transition` keeps its raw `tab_title: &str` parameter. It sanitizes only on the branch where the notification fires, just before it builds the body, so suppressed transitions do no sanitization work (FR3, SPEC a2). The tab-activity path keeps sanitizing at capture time and stores the result in `pending_notifications` as a `SanitizedTitle` (FR3, NFR3).

### D5: No compile_fail doctest

The `--lib` test command does not run doctests. The compile-time guarantee is shown by (1) the whole crate compiling with every caller passing a `SanitizedTitle`, and (2) unit tests on builder output (NFR4, SPEC a3). The "no raw-text constructor" part of the public surface is verified by inspection during review.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `sanitize_title` text output drifts during the refactor | Low | High (FR6) | The existing `sanitize_*` tests keep their expected text and compare through the accessor (TS4). The callbacks truncation-boundary tests keep their assertions (TS5). |
| The agent path loses sanitization when it moves out of the builder | Low | High (untrusted OSC title reaches the OS notification) | The builder's parameter type forces a `sanitize_title` call before the builder. A task-level App test sends a CSI-bearing title through `maybe_notify_agent_transition` and checks the delivered body. |
| A later change adds a raw-text constructor or conversion to `SanitizedTitle` and bypasses the invariant | Low | High | The public-surface exclusion list is pinned in Shared Components and stated in the type's doc comment. Review checks the surface. |
| The `--no-default-features` build breaks | Low | Medium | The module stays gui-gated and no dependency is added. TS6 runs both cargo checks. |
| Unrelated `--lib` tests are flaky under parallel execution | Medium | Low | The configured test command runs with `--test-threads=1`. |

## Open Questions

None.
