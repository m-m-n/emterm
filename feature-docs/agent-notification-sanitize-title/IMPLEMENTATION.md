# Implementation Plan: agent-notification-sanitize-title

## Overview

Route the tab title that `agent_notification_body` embeds into the agent-status
notification body through the existing `sanitize_title`, so untrusted OSC 0/2
derived titles never reach D-Bus / the OS notification server unsanitized
(review finding 7dd413bdd9289905, severity medium / security). This is a
single-task feature; all per-task detail lives in `tasks/task0001.md`.

## Technology Stack

- **Language**: Rust (`src-tauri` crate, `notifications` module, behind the
  `gui` feature; its tests run as default-feature `--lib` unit tests)
- **Key libraries**: existing only — `notify-rust` (notification dispatch,
  unchanged), `regex` (already backs `sanitize_title`, unchanged)

### Dependency licenses

No new dependency is introduced by this feature. `project.license` (MIT) is
unaffected; there is nothing to license-check.

## Layer Structure

Unchanged. The change is confined to the pure body-assembly function inside
`src-tauri/src/notifications.rs`; no module, layer, or dependency-direction
change.

## Shared Components

None — this feature is a single task, so no cross-task contract is needed.

## Conventions

Existing project conventions apply as-is (inline `#[cfg(test)] mod tests` in
the file under test; test names `<subject>_<scenario>_<expected>`). They are
restated in the task plan; no cross-task convention is introduced.

## Cross-task Design Decisions

None (single task). The feature's one design decision — sanitize inside
`agent_notification_body` as the choke point that closes both call sites,
reusing the existing `sanitize_title` — is recorded in `tasks/task0001.md`.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Existing `tabs.rs` replay tests flake under parallel execution | Medium | Low (false red on the regression run) | Re-run with a single test thread, as documented in VERIFICATION.md |
| Sanitization alters the embedded title's appearance for extreme titles (raw-input cap, 100-character truncation) | Low | Low | Intended behavioral parity with the tab-activity path (NFR1); the normal-title regression scenario (TS3) pins that ordinary titles are unaffected |

## Open Questions

None.
