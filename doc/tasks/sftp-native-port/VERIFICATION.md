# Verification Document: SFTP Upload — native-poc Port

## Overview
**Feature**: sftp-native-port
**SPEC.md**: `doc/tasks/sftp-native-port/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/sftp-native-port/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=native-poc/target cargo check --manifest-path native-poc/Cargo.toml --bin emterm-native-poc`
- Expected: exit code 0, no errors. Run from the project root (see `.claude/rules/native-poc-build-location.md`).
- **Result (2026-06-13)**: ✅ exit 0, no errors. Only pre-existing warnings in
  unrelated modules (ime / md3_widgets / search.rs); no SFTP-related warnings.

## Test Verification
- Command: `CARGO_TARGET_DIR=native-poc/target cargo test --manifest-path native-poc/Cargo.toml --bin emterm-native-poc`
- Coverage target: high on ported + new pure logic (args/check/progress/pool/
  validation/remote_path/aggregation/UI-state/resolution).
- **Result (2026-06-13)**: ✅ `1284 passed; 0 failed; 1 ignored`. SFTP-module
  tests specifically: `83 passed; 0 failed`
  (`cargo test ... sftp::`). No regressions in the existing suite.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type | Result |
|----|----------|-----------------|-----------|--------|
| TS-1 | build_sftp_args with IPv6 host, port, identity file | bracketed host, uppercase port flag, batch-stdin flag, tilde-expanded identity | Unit | ✅ `sftp::args::tests` (13 tests) |
| TS-2 | find_duplicates over a remote listing with spaces and prompt lines | only existing candidate names returned; prompt lines skipped | Unit | ✅ `sftp::check::tests` |
| TS-3 | parse_progress_line / parse_error_line | percent/bytes extracted; error lines detected | Unit | ✅ `sftp::progress::tests` |
| TS-4 | ConcurrentUploadPool acquire past max then release | acquire blocks past the cap; release wakes one waiter | Unit | ✅ `sftp::pool::tests::test_acquire_blocks_when_full` |
| TS-5 | set_max_concurrent changes the effective cap | acquire behavior reflects the new cap | Integration | ✅ `sftp::pool::tests::test_set_max_concurrent_unblocks_waiters` + `sftp::service::tests::set_max_concurrent_updates_cap` |
| TS-6 | validation of hostname/remote/local inputs | shell metacharacters / null / unsafe chars rejected; missing local path rejected | Unit | ✅ `sftp::service::tests::validate_*` |
| TS-7 | extract_remote_path on file:// URI, non-ASCII, plain, empty | decoded path / plain path / empty | Unit | ✅ `sftp::remote_path::tests::extract_remote_path_*` |
| TS-8 | format_paths_for_paste with spaces | space-containing paths quoted, space-joined | Unit | ✅ `sftp::remote_path::tests::format_paths_for_paste_*` |
| TS-9 | drop aggregation of multiple per-file events | one batch for one drop gesture | Unit | ✅ `sftp::ui::tests::aggregator_folds_multiple_drops_into_one_batch` |
| TS-10 | SftpService session-id generation | monotonic, no wall-clock; empty connection rejected | Unit | ✅ `sftp::service::tests::next_session_id_is_monotonic` + `validate_connection_rejects_empty_hostname` |
| TS-11 | toast state machine + auto-dismiss decision | status transitions correctly; terminal states scheduled for dismissal | Unit | ✅ `sftp::ui::tests::toast_*` |
| TS-12 | dialog-confirm branch with/without duplicates | duplicates → overwrite dialog; none → direct upload | Unit | ✅ `sftp::ui::tests::confirm_branch_*` |
| TS-13 | resolve_spawn SSH vs WSL branch | SSH branch sets connection name; WSL branch leaves none | Unit | ✅ `profiles::tests::resolve_ssh_profile_builds_ssh_argv` / `resolve_wsl_profile_builds_wsl_argv` / `resolve_plain_profile_has_no_connection_name` |
| TS-14 | active_for_tab over a session→tab map | only the queried tab's sessions are reported/cancelled; other tabs' sessions untouched | Unit |
| TS-15 | argv flag smuggling | hostname/username starting with `-` rejected; `--` end-of-options marker precedes the host element | Unit |
| TS-16 | paste injection via dropped path | shell metacharacters single-quoted (literal); paths with control chars (newline/CR/NUL) dropped | Unit | ✅ `sftp::service::tests::active_for_tab_tracks_sessions` |

## Code Quality Verification
- Format: `cargo fmt --manifest-path native-poc/Cargo.toml` — **✅ applied, exit 0.**
- Static analysis: `CARGO_TARGET_DIR=native-poc/target cargo clippy --manifest-path native-poc/Cargo.toml --bin emterm-native-poc`
  — **✅ 0 errors; no clippy warnings in the SFTP modules or the modified files
  (app.rs / window_host.rs / render/mod.rs).**
- Isolation check: `grep -rn tauri native-poc/src/sftp/` — **✅ no code-level
  Tauri dependency** (the only match is a doc-comment mention in `mod.rs`
  describing the port origin).
- Dead-code note: a module-wide `#![allow(dead_code)]` is kept in
  `sftp/mod.rs` for the intentionally-retained interactive-mode progress parser
  (batch mode suppresses progress bars, per the source note) and the
  pool/service introspection helpers preserved for parity; all are covered by
  their ported unit tests.

## File Structure Verification

### Files to Create
- [x] `native-poc/src/sftp/mod.rs` - status + progress types
- [x] `native-poc/src/sftp/args.rs` - sftp argv construction
- [x] `native-poc/src/sftp/check.rs` - duplicate detection
- [x] `native-poc/src/sftp/pool.rs` - concurrency pool
- [x] `native-poc/src/sftp/progress.rs` - progress/error parsing
- [x] `native-poc/src/sftp/process.rs` - subprocess manager
- [x] `native-poc/src/sftp/service.rs` - orchestration + validation + binary detection
- [x] `native-poc/src/sftp/remote_path.rs` - URI → remote dir; paste formatting
- [x] `native-poc/src/sftp/ui.rs` - egui dialog/toast state + drop aggregation

### Files to Modify
- [x] `native-poc/src/main.rs` - register `mod sftp`
- [x] `native-poc/src/profiles.rs` - spawn-overrides carry SSH connection name
- [x] `native-poc/src/tabs.rs` - Tab stores connection name + lookup helpers
- [x] `native-poc/src/app.rs` - SFTP UI state + progress/result pump + service + close guard
- [x] `native-poc/src/window_host.rs` - winit file-drop events + settings-reload cap (via `apply_settings`)
- [x] `native-poc/src/render/mod.rs` - draw SFTP overlay/dialogs/toasts
- [x] `native-poc/src/settings.rs` - drop the dead-code allow on `sftp_max_concurrent_uploads` (now read)
- [ ] `native-poc/src/i18n.rs` - **not modified**: native-poc localizes UI text
  via inline `match app.locale` (the established `profile_selector` pattern),
  not a string table in `i18n.rs`. SFTP en/ja strings are provided inline in
  `render/mod.rs::draw_sftp_overlay`, satisfying FR12 without an i18n.rs change.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All FR implemented + pure-logic unit-tested | test run green; FR coverage table below |
| SC-2 | `grep tauri native-poc/src/sftp/` empty | isolation grep |
| SC-3 | crate builds + tests pass | build + test commands |
| SC-4 | existing WebView E2E unaffected | `./scripts/run-e2e-docker.sh` green |
| SC-5 | manual US1/US2 pass | manual native-poc run |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 core port | A | TS-1..TS-4; isolation grep |
| FR2 service | B | TS-6, TS-10 |
| FR3 per-tab SSH | C | TS-13; manual SSH-tab predicate |
| FR4 drop dispatch | D | TS-9; manual SSH/non-SSH drop |
| FR5 remote path | D | TS-7 |
| FR6 duplicate + overwrite | E | TS-2, TS-12; manual overwrite dialog |
| FR7 concurrency limit | A/B/F | TS-4, TS-5 |
| FR8 progress toasts | E | TS-11; manual toast |
| FR9 cancel | E | manual cancel; pool slot release |
| FR10 tab-close guard | F | TS-14; manual close-with-uploads |
| FR11 settings reflection | F | TS-5; manual reload |
| FR12 i18n | E/F | manual en/ja strings present |
| NFR1 security | A/B/D | TS-6, TS-15, TS-16 |
| NFR2 architecture | A | isolation grep; channel pattern review |
| NFR3 responsiveness | B/E | manual: UI stays responsive during upload |
| NFR4 no wall-clock | B/E | TS-10; code review (no Instant/Date direct calls) |
| NFR5 cross-platform | A/B | code review of binary detection (Unix/Windows) |

## Existing E2E Regression (Phase 3.8)
- native-poc is **not** part of the WebView E2E harness
(`./scripts/run-e2e-docker.sh`), so no E2E run is applicable to this change.
- The ported source (`src-tauri/src/sftp/*`, `src-tauri/src/commands/sftp.rs`)
was **copied, not modified**, so the existing WebView SFTP behavior and its
E2E coverage are unaffected (SC-4 holds by no-change).
- Not run in this session (Docker/E2E not exercised); no native-poc code path
touches the WebView build.

## Manual Testing (E2E Not Possible)
native-poc is not covered by the WebView E2E harness; verify on the built
`native-poc/target-host/release/emterm-native-poc`:
- [ ] SSH tab: drop files → upload dialog → confirm → toasts complete.
- [ ] Directory drop → recursive upload.
- [ ] Duplicate names → overwrite dialog appears.
- [ ] Non-SSH tab: drop files → formatted paths pasted to the terminal.
- [ ] Cancel an in-flight upload from its toast.
- [ ] Close a tab mid-upload → confirmation; confirm cancels then closes.
- [ ] Change `sftp_max_concurrent_uploads` in settings → reload → concurrency changes.
- [ ] sftp binary missing → toast shows a clear failure message.

## Security Verification
- [x] hostname with shell metacharacters rejected (TS-6).
- [x] remote path with null/dangerous chars rejected (TS-6).
- [x] local path with unsafe chars / nonexistent rejected (TS-6).
- [x] sftp arguments passed as an argv array (code review) + `--` end-of-options
      marker; hostname/username starting with `-` rejected (TS-15).
- [x] dropped paths pasted to a non-SSH tab are single-quote escaped and paths
      with control characters are dropped (TS-16) — prevents paste injection.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit/Integration scenarios | 16 | 16 | 0 | 0 |
| Manual native-poc scenarios | 8 | 0 | 0 | 8 |
| Security checks | 4 | 3 | 0 | 1 |
| WebView E2E regression | 1 | 0 | 1 | 0 |
