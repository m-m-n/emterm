# Verification Document: notifications-sanitize-contract

## Overview

**Feature**: notifications-sanitize-contract / **SPEC.md**: `feature-docs/notifications-sanitize-contract/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/notifications-sanitize-contract/IMPLEMENTATION.md`

This document covers the integrated verification run. Task-level acceptance criteria are in `tasks/task0001.md`.

Test scenario IDs are hyphen-less (`TS1` … `TS7`). They string-match SPEC.md and the `tests` lists in workflow.yaml `requirements`.

## Build Verification

- **Command (default features)**, from workflow.yaml `project.components.main.build_command`:
  `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- **Command (CLI-only, NFR2)**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- **Expected**: both exit with code 0 and no errors. The touched files add no new compiler warnings.

## Test Verification

- **Command**, from workflow.yaml `project.components.main.test_command`:
  `bash scripts/fetch-fonts.sh && bun install && bun run build:viewer && bun run build:settings && CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- **Expected**: exit code 0 and no failed tests.
- **Coverage target**: the project has no coverage tool configured. Coverage is judged by scenarios: every FR/NFR below maps to at least one scenario, and every changed function has at least one named unit test.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Pass `sanitize_title("\x1b[31mred\x1b[0m title")` to `notification_body` for every `ActivityKind` (new unit test in `notifications.rs`) | Returns `red title: <message>` for each kind, with no ESC byte and no CSI remnant | Unit |
| TS2 | Run the existing `body_formats_match_webview_strings` and `body_formats_match_webview_ja_strings` with input built by `sanitize_title("tab")` | Pass with unchanged expected output (En and Ja) | Unit |
| TS3 | Run the existing `agent_notification_body_*` tests, including the CSI and control-character tests from agent-notification-sanitize-title, with the raw title passed through `sanitize_title` by the caller | Pass with unchanged expected bodies | Unit |
| TS4 | Run the existing `sanitize_*` tests, comparing through the `SanitizedTitle` read accessor | Pass with unchanged expected text (CSI strip, control-character strip, 100-character cut, plain passthrough, pathological-input bound) | Unit |
| TS5 | Run the `body_markup_escape` / `summary_markup_escape` tests in `callbacks/tests.rs` that use a `sanitize_title` result, reading it through the accessor | Assertions hold: 100-character boundary, trailing `&lt;`, `escape_for_send` result | Unit |
| TS6 | Build after every call site that passed a raw title is migrated | The crate compiles with default features and with `--no-default-features`. Every caller passing `SanitizedTitle` is the compile-time evidence. | Integration (build) |
| TS7 | Review the doc comments in `notifications.rs` | The phrase "single choke point for both existing call sites and any future one" is gone. The `AgentTransition::name` and `agent_notification_body` docs state the upstream `sanitize_name` contract. | Code review by the verifier (text search + reading) |

## Code Quality Verification

- **Format**: no format command is configured (workflow.yaml `format_command` is empty). Do not run crate-wide formatting. Only the files declared in `task0001` may change.
- **Static analysis**: no lint command is configured. Confirm that the build commands above report no new compiler warnings in the touched files.
- **Public-surface inspection (NFR4, FR1)**: the verifier reads `src-tauri/src/notifications.rs` and confirms that `SanitizedTitle` has none of the following: a public field, a public constructor from raw text, a conversion trait to or from `String` / `&str`, `Default`, or deserialization. Its only read paths are the accessor and `Display`.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | `notification_body` and `agent_notification_body` both take the tab title as `&SanitizedTitle`, never a raw `&str` | Read the signatures, then TS1, TS3, TS6 |
| AC2 | Callers outside `crate::notifications` cannot pass a raw `&str` / `String` title to either builder. `SanitizedTitle` is obtainable only from `sanitize_title`. | TS6 (every caller compiles against the typed signatures) plus the public-surface inspection above |
| AC3 | The doc comments explain the type-enforced tab-title contract and the upstream agent-name contract, and the "single choke point" phrase is gone | TS7 |
| AC4 | `cargo test --manifest-path src-tauri/Cargo.toml --lib` passes | Test Verification command (TS1-TS5 included) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS3, TS4, TS6, plus public-surface inspection |
| FR2 | task0001 | TS1, TS3, TS6 |
| FR3 | task0001 | TS1, TS6 |
| FR4 | task0001 | TS1, TS3, TS7 |
| FR5 | task0001 | TS7 |
| FR6 | task0001 | TS1, TS2, TS3, TS4, TS5 |
| NFR1 | task0001 | TS1, TS2, TS3, TS4, TS5 (Test Verification command) |
| NFR2 | task0001 | TS6 (both cargo check commands) |
| NFR3 | task0001 | TS4 (100-character cut and input cap), TS6 (`pending_notifications` holds `SanitizedTitle`, which only `sanitize_title` produces) |
| NFR4 | task0001 | TS1, TS6 |

## E2E Testing

Not applicable. The project has no E2E framework (workflow.yaml `e2e_test_command` is empty), and the feature has no user-visible change.

## Manual Testing (E2E Not Possible)

None required. Notification text and behavior are unchanged, and TS1-TS5 pin them (FR6). TS7 and the public-surface inspection are code-reading checks the verifier performs. They need no human and no running application.

## Performance / Security Verification

- **NFR3 (stored title length)**: `pending_notifications` in `App::pump_all` stores `SanitizedTitle` values sanitized at the existing capture point. The type can only come from `sanitize_title`, which caps input at 4096 characters and output at 100 (TS4), so each stored title is at most 100 characters and no raw OSC title is cloned into the list. Evidence: TS4, plus TS6 (the element type) and reading the capture point.
- **Input handling (untrusted tab titles)**: a tab title reaches either body builder only as a `SanitizedTitle` (TS6). CSI and control characters are removed on both paths (TS1, TS3). The agent path through `App::maybe_notify_agent_transition` is also covered by task0001's App-level test.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (default features, `--no-default-features`) | 2 | 0 | 0 |
| Unit tests | 5 (TS1-TS5) | 5 | 0 | 0 |
| Integration (compile evidence) | 1 (TS6) | 1 | 0 | 0 |
| Code review by the verifier | 2 (TS7, public-surface inspection) | 2 (code reading and text search) | 0 | 0 |
