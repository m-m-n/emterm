# Feature: Plus-Menu tmux Attach

## Overview

Add live tmux sockets to eMterm's new-tab chooser (the modal opened by the tab bar's "+" button). Each detected socket appears as a selectable row; confirming it spawns a new tab whose PTY runs `tmux -S {socket_path} attach`, attaching to that tmux server.

## Objectives

- Detect live tmux sockets without spawning external processes
- Surface them in the existing new-tab chooser, refreshed on every open
- One-click attach into a new tab

## User Stories

### US1: Attach to a tmux socket from the + menu
As an eMterm user, I want the + menu to list running tmux sockets, so that I can attach to a session (e.g. a Notion-triggered Claude Code run) with one click.

**Acceptance Criteria:**
- [ ] Live tmux sockets are listed in the chooser when present
- [ ] Selecting a socket row opens a new tab attached to that socket
- [ ] With zero live sockets the chooser looks exactly as today

## Technical Requirements

### Functional Requirements
- **FR1:** tmux socket discovery — enumerate socket files under `$TMUX_TMPDIR` (fallback `/tmp/tmux-{uid}/`), keep only entries of type socket that accept a Unix-domain connection (stale sockets are filtered out). Missing directory or permission errors yield an empty/partial list, never a failure. No external process is spawned during discovery.
- **FR2:** Chooser integration — every time the new-tab chooser opens (`App::open_new_tab_chooser`), run discovery and append one row per live socket to the chooser list (after Global Settings and profiles), labeled so the socket name is identifiable (e.g. `tmux: default`). Zero sockets → no tmux rows and no tmux heading.
- **FR3:** Attach action — confirming a tmux row spawns a new tab via the existing overrides path (`spawn_new_tab_with_overrides` + `SpawnOverrides`) with `shell_path = "tmux"` and `shell_args = ["-S", {socket_path}, "attach"]`. The socket path is passed as a discrete argv element (no shell string interpolation).

### Non-Functional Requirements
- **NFR1 - Performance:** Discovery must not perceptibly delay chooser opening: directory scan + connect-probe only, no subprocess, probes complete quickly (non-blocking connect or short timeout).
- **NFR2 - Portability:** The feature is Unix-only (`#[cfg(unix)]`). The Windows cross-build (`cargo xwin check`) and the CLI-only build (`--no-default-features`) keep compiling; on Windows the chooser behavior is unchanged.
- **NFR3 - No regression:** Existing chooser behavior (Global Settings row, profile rows, default preselection, keyboard navigation) is unchanged when tmux rows are absent and remains correct when they are present.

## Assumptions

Decisions taken in batch mode (no user to ask; Codex CLI unavailable, so resolved by Claude with recorded rationale):

- **A1 — Discovery scope:** Only the standard tmux socket directory `$TMUX_TMPDIR`/`/tmp/tmux-{uid}/` is scanned (covers `tmux` default and `tmux -L {name}`). Sockets created at arbitrary paths with `tmux -S` are out of scope.
- **A2 — "監視" (monitoring) semantics:** Satisfied by on-demand rescan each time the chooser opens. No background watcher/daemon is added.
- **A3 — Liveness check:** A socket is "live" when a Unix-domain connect succeeds. tmux is not invoked (`tmux list-sessions` would spawn a process per socket on every chooser open).
- **A4 — Granularity:** One row per socket, matching the acceptance criteria wording. Per-session breakdown within a socket is out of scope.
- **A5 — Attach command:** `tmux -S {socket_path} attach` (not `attach-session -t`), attaching to the server's current/most-recent session. tmux missing from PATH surfaces as the PTY's own error output in the new tab; no dedicated error dialog.
- **A6 — UI placement:** Rows are appended to the existing profile-selector modal (no new dropdown widget). Visual design follows existing chooser rows, hence the design step is skipped.

## Implementation Approach

### Architecture

```
tab_bar "+" click
  → TabEvent::New
  → App::open_new_tab_chooser        (src-tauri/src/app.rs:1496)
      ├─ discover tmux sockets       (new module, e.g. src-tauri/src/tmux_sockets.rs)
      └─ profile_selector.open_with_global(...) + tmux rows
  → user confirms row
  → App::confirm_profile_selection   (src-tauri/src/app.rs:1525)
      ├─ Choice::Global / Choice::Profile(i)   (existing)
      └─ Choice::TmuxSocket(i)                 (new)
          → spawn_new_tab_with_overrides(SpawnOverrides {
                shell_path: "tmux",
                shell_args: ["-S", path, "attach"], .. })
```

### Affected Code (from code survey)

- `src-tauri/src/ui/profile_selector.rs` — `ProfileSelectorState`, `Choice` enum (`:149`), `row_to_choice` (`:126`), row rendering: add tmux row kind
- `src-tauri/src/app.rs` — `open_new_tab_chooser` (`:1496`), `confirm_profile_selection` (`:1525`): discovery call + attach spawn
- `src-tauri/src/tmux_sockets.rs` (new) — discovery module (`#[cfg(unix)]`, declared in `lib.rs` under the `gui` feature as appropriate)
- `src-tauri/src/profiles.rs` — `SpawnOverrides` (`:21`) is used as-is (no changes expected)

### Dependencies

**Internal Dependencies:**
- `profile_selector` chooser modal (rows + selection decode)
- `SpawnOverrides` / `spawn_new_tab_with_overrides` / `Tab::spawn_shell` PTY launch path

**External Dependencies:**
- None new. `std::os::unix::net::UnixStream` for the connect probe.

## Test Scenarios

### Unit Tests
- [ ] TS-1: Discovery on a temp dir with a live listening socket, a stale socket file, and a regular file → only the live socket is returned
- [ ] TS-2: Discovery with a missing directory → empty list, no error
- [ ] TS-3: Chooser row/choice decode with tmux rows present → indices map to the correct socket (Global / profiles / tmux ordering preserved)
- [ ] TS-4: Attach override construction → `shell_path == "tmux"`, `shell_args == ["-S", path, "attach"]`

### Integration Tests
- [ ] Existing `--lib` test suite passes (no chooser regressions)

### E2E Tests
**Existing E2E tests**: None (no E2E infrastructure detected)
**Run command**: Not detected
- [ ] Manual: M-1 — with a running `tmux -L test` server, + menu lists `tmux: test`; clicking it opens a tab attached to the session

### Edge Cases
- [ ] Zero sockets → chooser unchanged (no heading, profile-empty fast path `spawn_new_tab()` still applies only when profiles AND tmux sockets are both absent — decided: when profiles are empty but tmux sockets exist, the chooser must open)
- [ ] Socket disappears between listing and click → tmux prints its error in the new tab; app stays healthy
- [ ] Windows build: no tmux code compiled; chooser unchanged

## Security Considerations

- **Input Validation:** Socket paths come from the user's own `$TMUX_TMPDIR`/`/tmp/tmux-{uid}` directory listing; passed as discrete argv elements, never through a shell string.
- **Authorization:** Only the current UID's socket directory is scanned.

## Error Handling

| Case | Handling |
|------|----------|
| Socket dir missing / unreadable | Empty list; chooser opens without tmux rows |
| Individual entry unreadable / not a socket / connect refused | Entry skipped |
| tmux binary missing at attach | PTY error output visible in the new tab |

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All unit test scenarios pass
- [ ] Windows cross-build and CLI-only build keep compiling
- [ ] Manual scenario M-1 confirmed

## Open Questions

- None (all points resolved as Assumptions above).

## References

- REQUIREMENTS.md (this feature)
- Notion task: https://www.notion.so/3a53509ec8ee80289d9df0787776a65a
