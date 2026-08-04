# Feature: test-tmp-file-cleanup

## Overview

Tests in this repository write temporary files and directories under `/tmp` (`std::env::temp_dir()`) and, at four sites, never remove them on success. This feature makes every such site clean up its own temporary artifacts when the test finishes normally, so that repeated test runs stop accumulating residue in `/tmp`. Cleanup on abnormal termination (process crash / kill) is explicitly out of scope.

Requirements source: `feature-docs/test-tmp-file-cleanup/REQUIREMENTS.md`.

## Objectives

- Remove the residue that a normally-finishing test run leaves in `/tmp`, eliminating a recurrence factor for the `/tmp`-at-100% condition that made Claude Code tooling unrunnable.
- Fix the four known non-cleaning test sites (`settings_store.rs`, `settings_window/commands.rs`, `mux/tmux_import.rs`, `viewer/launch.rs`).
- Keep every already-cleaning site, and all production `/tmp` behaviour, unchanged.

## User Stories

### US1: A test run leaves no residue in `/tmp`

As a developer or CI runner, I want a successfully completed test run to leave no new entries under `/tmp`, so that repeated runs do not exhaust `/tmp` and break tooling that depends on it.

**Acceptance Criteria:**

- [ ] AC-1: When tests finish normally, the temporary files they wrote under `/tmp` are cleaned up. Concretely, for each of FR1–FR4, the corresponding `/tmp` path does not exist after the relevant test succeeds.
- [ ] AC-2: Comparing the listing of `/tmp` entries before and after a fully successful run of all documented test commands shows no new run-induced residue.
- [ ] AC-3: Missed cleanup when the program dies mid-run is accepted (no requirement on this path).
- [ ] AC-4: The existing test suites (Rust `--lib` / integration tests / `bun test`) continue to pass in full.

## Technical Requirements

### Functional Requirements

- **FR1 — Remove the temporary directory created by `settings_store.rs` tests:** The test helper `tmp_path()` in `src-tauri/src/settings_store.rs` creates `/tmp/emterm-settings-store-test-{pid}-{name}/` (containing `settings.json`, and the `.bak` file in the corrupt test). Delete that directory when each test finishes normally. Today only pre-deletion (`remove_file`) exists and there is no post-deletion at all, so several directories are left behind on every successful run (`settings_store.rs:101-108`; the deletions at 132/197/272 are pre-run cleanups only).
- **FR2 — Remove the temporary directory created by the `settings_window/commands.rs` roundtrip test:** `app_settings_full_roundtrip_through_patch_save` (lines 473-503) in `src-tauri/src/settings_window/commands.rs` creates `/tmp/emterm-settings-window-test-{pid}/`; delete it on normal completion. Other tests in the same file (lines 565/591/626/662 etc.) already call `remove_dir_all` at the end; only this test lacks the deletion.
- **FR3 — Remove the temporary directories created by `mux/tmux_import.rs` tests:** The five tests (missing / latch / apply / preserve / keybinds_nonobject) that use the helper `tmp_settings_path()` (lines 174-182, whose `remove_dir_all` is a pre-creation cleanup only) in `src-tauri/src/mux/tmux_import.rs` create `/tmp/emterm-tmux-import-test-{pid}-{name}/`; delete these on normal completion. Because `pid` changes per run, the pre-run cleanup never reclaims them and five directories accumulate per run. `auto_import_tmux_conf_skips_oversized_file` (line 324) already deletes its directory and needs no change.
- **FR4 — Remove the payload file left by the `viewer/launch.rs` spawn-error test:** In `src-tauri/src/viewer/launch.rs`, `launch_with_propagates_spawn_error` (lines 280-291) leaves `/tmp/emterm-viewer-{pid}-{nanos}-{n}.json` behind because `launch_with` does not delete the payload file when spawn fails (`launch.rs:169-178`). Delete that file when the test finishes normally. (`launch_with_writes_payload_and_invokes_spawn_once` in the same file already deletes it at line 277.)
- **FR5 — Zero residue for the suite as a whole on a successful run:** When the documented test commands (`src-tauri --lib`, `--test cli_subcommands`, each `crates` `--lib`, `bun test`) all complete successfully, no new files or directories attributable to that run remain directly under `/tmp`. The behaviour of the sites that already clean up is preserved: `render/font/user_dir.rs`; `viewer/html_window.rs`, `html_resolver.rs`, `image_resolver.rs`, `image_window.rs`, `html.rs`, `data_payload.rs`, `image_payload.rs`; `settings.rs`; `git_branch.rs`; the `tempfile`-crate call sites; the `DaemonGuard` in `tests/mux_hot_upgrade.rs` and `mux_throughput.rs`; and the `finally` `rmSync` in `notify-status.test.ts`.

### Non-Functional Requirements

- **NFR1 — Scope of the cleanup guarantee:** Missed cleanup when the program dies mid-run (abnormal process termination / kill) is accepted. Residue on the test-failure (panic) path is also outside the requirement, though an implementation that cleans up naturally via RAII is not precluded.
- **NFR2 — No new dependencies:** Do not add any new dependency. Where RAII-based deletion is needed, `src-tauri`'s existing dev-dependency `tempfile = "3"` (`src-tauri/Cargo.toml:207`) may be used.
- **NFR3 — Production behaviour untouched:** Do not change production `/tmp` write behaviour (parent writes the viewer payload and the child deletes it after reading; the `/tmp/emterm-mux-daemon.log` fallback log).

## Implementation Approach

### Affected sites

| Requirement | File | Temporary path | Current state |
|---|---|---|---|
| FR1 | `src-tauri/src/settings_store.rs` (helper `tmp_path()`, lines 101-108; pre-cleanups at 132/197/272) | `/tmp/emterm-settings-store-test-{pid}-{name}/` | Pre-deletion only; no post-deletion |
| FR2 | `src-tauri/src/settings_window/commands.rs` (`app_settings_full_roundtrip_through_patch_save`, lines 473-503) | `/tmp/emterm-settings-window-test-{pid}/` | Sibling tests at 565/591/626/662 already `remove_dir_all`; this one does not |
| FR3 | `src-tauri/src/mux/tmux_import.rs` (helper `tmp_settings_path()`, lines 174-182; 5 tests) | `/tmp/emterm-tmux-import-test-{pid}-{name}/` | Pre-creation cleanup only; `pid` varies per run so it never reclaims prior runs |
| FR4 | `src-tauri/src/viewer/launch.rs` (`launch_with_propagates_spawn_error`, lines 280-291; `launch_with` at 169-178) | `/tmp/emterm-viewer-{pid}-{nanos}-{n}.json` | Payload not deleted on spawn failure |

### Cleanup model

- Cleanup runs on the normal-completion path of each test (NFR1). An RAII-based approach that also cleans on panic is permitted but not required.
- Where RAII is chosen, use the existing dev-dependency `tempfile = "3"`; no new dependency is introduced (NFR2).
- Deletion is confined to the temporary path the test itself created.

### Out of scope

- Production `/tmp` writers (NFR3): the viewer payload path, where the child process deletes the payload after reading it (`viewer/html_window.rs:68`, `viewer/window.rs:49`, `viewer/image_payload.rs:191`, `viewer/data_payload.rs:126`) and `viewer/mod.rs:278` deletes it on spawn failure; and `/tmp/emterm-mux-daemon.log` (`mux/daemon.rs:271`), an intentional persistent fallback log used only when the log cannot be opened in the socket directory.
- The abnormal-termination path (NFR1, AC-3).

### Dependencies

**Internal Dependencies:**

- None beyond the four test modules listed above.

**External Dependencies:**

- `tempfile = "3"` — already a `src-tauri` dev-dependency (`src-tauri/Cargo.toml:207`); usable for RAII cleanup. No new dependency is added.

### File Structure

```
src-tauri/src/
├── settings_store.rs                 # FR1
├── settings_window/commands.rs       # FR2
├── mux/tmux_import.rs                # FR3
└── viewer/launch.rs                  # FR4
```

## Test Scenarios

### Unit Tests

- [ ] **TS3** (FR1, FR2, FR3, FR4): Run each of the fixed tests individually and confirm that, after it succeeds, the corresponding temporary path (`emterm-settings-store-test-*` / `emterm-settings-window-test-*` / `emterm-tmux-import-test-*` / `emterm-viewer-*.json`) does not exist.

### Integration Tests

- [ ] **TS1** (FR1, FR2, FR3, FR4, FR5): Record the entries directly under `/tmp`, run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` to a successful completion, then confirm no new `emterm-*` entries attributable to the run remain in `/tmp`.
- [ ] **TS2** (FR5): Do the same before/after comparison for `--test cli_subcommands`, the `--lib` suites of `crates/{term_core,term_images,app_settings,mux_ipc}`, and `bun test`, confirming zero residue.
- [ ] **TS4** (FR1, FR2, FR3, FR4, FR5): Confirm that all targeted tests still pass after the fix (the `tabs.rs` replay tests have a known parallel-execution flake, so use `--test-threads=1` if needed).

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] Abnormal termination (process crash / kill) and test failure via assertion panic: residue is accepted, no cleanup is required on these paths (NFR1, AC-3).
- [ ] `pid` differs on every run, so a pre-run cleanup keyed on `pid` cannot reclaim earlier runs' directories — cleanup must happen at the end of the run that created them (FR3).
- [ ] `auto_import_tmux_conf_skips_oversized_file` (`mux/tmux_import.rs:324`) already deletes its directory and must not be changed (FR3).

## Security Considerations

- **Input Validation:** Deletion targets are the temporary paths the tests themselves constructed; no externally supplied path is deleted.
- Otherwise not applicable — this change touches test-only cleanup logic.

## Error Handling

Not applicable beyond the requirement itself: cleanup runs on the normal-completion path, and failures caused by abnormal termination are accepted (NFR1).

## Success Criteria

- [ ] FR1–FR5 are implemented and verified.
- [ ] TS1–TS4 pass.
- [ ] AC-1 holds: after each of the FR1–FR4 tests succeeds, the corresponding `/tmp` path does not exist.
- [ ] AC-2 holds: the before/after listing of `/tmp` shows no run-induced residue for a fully successful run of all documented test commands.
- [ ] AC-4 holds: the existing Rust `--lib`, integration, and `bun test` suites all still pass.
- [ ] NFR2 holds: no new dependency was added.
- [ ] NFR3 holds: production `/tmp` write behaviour is unchanged.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — all requirements are resolved.

## Assumptions

- **A-1:** The scope is test-code residue only; production `/tmp` writes are out of scope. Rationale: on the evidence, the production paths already clean up during normal operation — the viewer payload is deleted by the child after reading (`viewer/html_window.rs:68`, `viewer/window.rs:49`, `viewer/image_payload.rs:191`, `viewer/data_payload.rs:126`), and `viewer/mod.rs:278` deletes it on spawn failure; `/tmp/emterm-mux-daemon.log` (`mux/daemon.rs:271`) is a fallback used only when the log cannot be opened in the socket directory, and is an intentional persistent log. Impact: low. Reversible: yes.
- **A-2:** Residue from a test failure (assertion panic) counts as "the program died mid-run" and is within the accepted range. Rationale: the acceptance condition explicitly states that missed cleanup when the program dies mid-run is accepted. Impact: low. Reversible: yes.
- **A-3:** The residue targeted here is small (JSON files and small directories); the several-hundred-MB daemon binary copies that were the largest contributor to the `/tmp` 100% exhaustion are assumed already fixed. Rationale: the comment and implementation at `tests/mux_hot_upgrade.rs:152-192` show the binary copy destination was moved from `/tmp` to a directory adjacent to the cargo build output, and `DaemonGuard::Drop` (lines 106-113) removes the runtime dir / bin dir even on panic. Impact: medium. Reversible: yes.

## References

- Requirements document: `feature-docs/test-tmp-file-cleanup/REQUIREMENTS.md`
