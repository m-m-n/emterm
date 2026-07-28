# Feature: tmux Session Rows in the New-Tab Chooser

## Overview

The new-tab chooser (the modal opened by the tab bar's `+` button) currently lists one row per live tmux **socket**, labeled with the socket file name. Users of tmux's default server always see a single row named `default`, regardless of how many sessions run inside that server, so sessions cannot be told apart. This feature replaces the socket rows with one row per tmux **session**, labeled by session name, and attaches directly to the chosen session.

## Objectives

- Identify running tmux by session name instead of socket name
- Show one chooser row per live session, deterministically ordered
- Attach to exactly the chosen session, with an exact-match target
- Degrade to the current socket-level row whenever session enumeration is not possible

## User Stories

### US1: Pick a specific tmux session from the + menu
As an eMterm user running several tmux sessions on the default server, I want the + menu to list them by session name, so that I can attach to the one I mean without cycling sessions after attaching.

**Acceptance Criteria:**
- [ ] With 2+ sessions on one server, the chooser shows one row per session
- [ ] Each row's label identifies the session by name
- [ ] Confirming a row opens a new tab attached to that session
- [ ] With zero live sockets the chooser looks exactly as today

### US2: Enumeration failures never break the chooser
As an eMterm user, I want the chooser to stay usable when tmux cannot be queried, so that a broken or slow tmux server does not block opening a new tab.

**Acceptance Criteria:**
- [ ] A socket whose sessions cannot be listed still appears as a single socket-labeled row
- [ ] Selecting that fallback row behaves exactly as today (`tmux -S {path} attach`)
- [ ] A non-responding socket does not stall chooser opening beyond a bounded timeout

## Technical Requirements

### Functional Requirements

- **FR1:** Session enumeration — for each live socket returned by the existing discovery (`src-tauri/src/tmux_sockets.rs`, unchanged), query tmux for that server's session names by executing `tmux -S {socket_path} list-sessions -F #{session_name}` as a subprocess with argv elements (no shell). The subprocess is bounded by a per-socket timeout; on timeout it is terminated and the socket is treated as un-enumerable. Output is parsed as one session name per non-empty line. The result is a list of `(socket_name, socket_path, session_name)` triples, sorted by `(socket_name, session_name)` for deterministic ordering. Enumeration never returns an error to the caller: any failure (tmux missing from PATH, non-zero exit, timeout, spawn error, empty output) degrades that socket to the un-enumerable state.

- **FR2:** Chooser rows — every time the new-tab chooser opens (`App::open_new_tab_chooser`), run discovery + enumeration and append one row per enumerated session after the Global Settings and profile rows, replacing the current one-row-per-socket behavior. Row label: `tmux: {session_name}` when the socket name is `default`, otherwise `tmux: {socket_name}: {session_name}`. A socket that could not be enumerated contributes exactly one fallback row labeled `tmux: {socket_name}` (today's label). Zero live sockets → no tmux rows, chooser identical to today. The row/choice index decode stays the single authority in `ProfileSelectorState::row_to_choice` so the renderer and confirm path cannot drift.

- **FR3:** Session-targeted attach — confirming a session row spawns a new tab through the existing overrides path (`spawn_new_tab_with_overrides` + `SpawnOverrides`) with `shell_path = "tmux"` and `shell_args = ["-S", {socket_path}, "attach-session", "-t", "={session_name}"]`. The `=` prefix forces tmux's exact-name target matching, so a session whose name is a prefix of another is never mistaken for it. Every element is a discrete argv element; no shell string interpolation. Confirming a fallback (un-enumerable socket) row keeps today's `["-S", {socket_path}, "attach"]`.

- **FR4:** Fast-path parity — the chooser's "skip the modal and spawn directly" fast path (`profiles.is_empty() && tmux rows are empty`) is evaluated against the new row list: it applies only when there are no profiles AND no tmux rows of either kind.

### Non-Functional Requirements

- **NFR1 - Performance:** Chooser opening must not perceptibly stall. Enumeration runs only against sockets that already passed the liveness connect probe, and each socket's `list-sessions` call is bounded by a timeout of at most 300 ms. The bound holds regardless of how the tmux server behaves.

- **NFR2 - Portability:** The feature stays Unix-only (`#[cfg(unix)]`). The Windows cross-build (`cargo xwin check`) and the CLI-only build (`--no-default-features`) keep compiling; on Windows the chooser behavior is unchanged.

- **NFR3 - No regression:** Existing chooser behavior — Global Settings row, profile rows, default-profile preselection, keyboard wrap-around navigation, row visual style — is unchanged. Socket discovery semantics (socket-type filter, stale-socket rejection, missing-directory tolerance, `$TMUX_TMPDIR` resolution) are unchanged.

- **NFR4 - Safety:** Socket paths and session names originate from the user's own tmux server and are passed as discrete argv elements, never through a shell. No `sh -c` / string interpolation is introduced on the enumeration or attach path.

## Assumptions

Decisions taken in batch mode (no user to ask; Codex CLI unavailable in this environment, so resolved by Claude with recorded rationale):

- **A1 — Enumeration mechanism:** `tmux -S {path} list-sessions -F #{session_name}` as a subprocess. The predecessor feature's rule "discovery spawns no external process" (`plus-menu-tmux-attach` SPEC A3) is deliberately relaxed here: session names live inside the tmux server and there is no way to read them without talking to it, and reimplementing tmux's control protocol is disproportionate. The cost is bounded by only querying sockets that already passed the connect probe, and by the per-socket timeout (NFR1).

- **A2 — Label format:** `tmux: {session}` for the `default` socket, `tmux: {socket}: {session}` otherwise. Rationale: the overwhelmingly common case is a single default server, where the socket name carries no information; named servers (`tmux -L foo`) are rare and there the socket name is meaningful and disambiguates same-named sessions across servers.

- **A3 — Fallback granularity:** A socket that cannot be enumerated falls back to today's single socket row rather than disappearing. Rationale: a live server is always reachable in principle, so failure is exceptional; hiding it would be a silent regression of the predecessor feature.

- **A4 — Timeout value:** 300 ms per socket, sockets processed sequentially. Rationale: a local Unix-socket round trip to a healthy tmux server is sub-millisecond; 300 ms tolerates heavy load while keeping the worst case bounded for the realistic socket counts (typically 1).

- **A5 — Attach target syntax:** `attach-session -t ={name}` rather than `attach -t {name}`. Rationale: tmux target resolution otherwise falls back to prefix/fnmatch matching, which would attach to the wrong session when one name is a prefix of another; `=` forces exact matching.

- **A6 — Attached sessions:** Sessions already attached by another client are listed like any other. tmux permits multiple clients per session, and excluding them would hide exactly the sessions a user most often wants to look in on.

- **A7 — Design step:** Skipped. Only the label text and the number of rows change; row visuals reuse the existing chooser row rendering.

## Implementation Approach

### Architecture

```
tab_bar "+" click
  → TabEvent::New
  → App::open_new_tab_chooser                      (src-tauri/src/app.rs)
      ├─ crate::tmux_sockets::discover()           (unchanged: live sockets)
      ├─ session enumeration per socket            (new: tmux list-sessions)
      │    success → N session entries
      │    failure → 1 fallback socket entry
      └─ profile_selector.open_with_global(..) + tmux rows
  → user confirms row
  → App::confirm_profile_selection                 (src-tauri/src/app.rs)
      └─ Choice::Tmux(i)
          ├─ session entry  → SpawnOverrides { tmux, ["-S", path, "attach-session", "-t", "=name"] }
          └─ fallback entry → SpawnOverrides { tmux, ["-S", path, "attach"] }
```

### Data model

The chooser's tmux row list changes from `Vec<(String, PathBuf)>` (socket name, path) to a list of entries that carry an optional session name — one variant/field shape covering both the session row and the fallback socket row. This single list drives the label, the row count, and the attach argv, so the three cannot disagree.

### Affected Code (from code survey)

- `src-tauri/src/tmux_sockets.rs` — socket discovery stays as-is; session enumeration is added here (same module owns "what tmux is running"), keeping the App layer free of tmux knowledge per the existing IMPLEMENTATION layering ("App calls this, UI never does")
- `src-tauri/src/ui/profile_selector.rs` — `ProfileSelectorState::tmux_sockets` field shape, the `tmux:{name}` label construction (`draw`), `row_to_choice` / `Choice::Tmux` decode
- `src-tauri/src/app.rs` — `open_new_tab_chooser` / `open_new_tab_chooser_with_sockets` (enumeration call + fast path), `profile_selector_row_count`, `confirm_profile_selection`, `discover_tmux_sockets`, `tmux_attach_overrides`

### Dependencies

**Internal Dependencies:**
- `profile_selector` chooser modal (rows + selection decode)
- `SpawnOverrides` / `spawn_new_tab_with_overrides` PTY launch path

**External Dependencies:**
- None new. `std::process::Command` for the enumeration subprocess; the timeout is implemented with the standard library (no new crate) unless an already-vendored dependency provides it.

## Test Scenarios

### Unit Tests
- [ ] TS-1: Parsing `list-sessions` output — multi-line output yields one entry per non-empty line, preserving names verbatim; blank/trailing lines are ignored
- [ ] TS-2: Enumeration failure paths — non-zero exit, empty output, and spawn failure each yield the un-enumerable state (fallback), never an error
- [ ] TS-3: Row label construction — `default` socket yields `tmux: {session}`; a named socket yields `tmux: {socket}: {session}`; a fallback entry yields `tmux: {socket}`
- [ ] TS-4: Attach override construction — session entry yields `["-S", path, "attach-session", "-t", "={name}"]`; fallback entry yields `["-S", path, "attach"]`
- [ ] TS-5: Row/choice decode with session rows present — `Global → profiles → tmux` ordering preserved; indices map to the right entry
- [ ] TS-6: Ordering — entries sort by `(socket_name, session_name)` deterministically
- [ ] TS-7: Fast path — profiles empty × tmux rows empty spawns directly; profiles empty × tmux rows present opens the chooser
- [ ] TS-8: Timeout — a socket that never answers is abandoned within the bound and reported as un-enumerable

### Integration Tests
- [ ] Existing `--lib` test suite passes (no chooser regressions)

### E2E Tests
**Existing E2E tests**: None (no E2E infrastructure detected)
**Run command**: Not detected
- [ ] M-1 (manual): with `tmux new -d -s alpha` and `tmux new -d -s beta` on the default server, the + menu lists `tmux: alpha` and `tmux: beta`; selecting `beta` opens a tab attached to `beta`

### Edge Cases
- [ ] Zero live sockets → chooser unchanged, no tmux rows
- [ ] Session disappears between enumeration and click → tmux's own error output appears in the new tab; app stays healthy
- [ ] Same session name on two different sockets → both rows visible and distinguishable (socket prefix on the non-default one)
- [ ] Session name that is a prefix of another (`dev` / `dev2`) → exact-match target attaches to the right one
- [ ] Session name containing spaces or non-ASCII characters → label and argv carry it verbatim
- [ ] Windows build: no tmux code compiled; chooser unchanged

## Security Considerations

- **Input Validation:** Session names are untrusted-ish input read from a subprocess's stdout; they are used only as a label string and as a discrete argv element. They are never interpolated into a shell command, a format string with side effects, or a filesystem path.
- **Authorization:** Only the current UID's socket directory is scanned (unchanged).
- **Command execution:** `tmux` is resolved via PATH by `std::process::Command` with an argv vector; no `sh -c`.

## Error Handling

| Case | Handling |
|------|----------|
| Socket dir missing / unreadable | Empty list; chooser opens without tmux rows (unchanged) |
| `tmux` not on PATH | Every socket falls back to a socket row |
| `list-sessions` non-zero exit / empty output | That socket falls back to a socket row |
| `list-sessions` exceeds the timeout | Child terminated; that socket falls back to a socket row |
| Session gone at attach time | tmux's error output visible in the new tab |

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All unit test scenarios pass
- [ ] Windows cross-build and CLI-only build keep compiling
- [ ] Manual scenario M-1 confirmed

## Open Questions

- None (all points resolved as Assumptions above).

## References

- REQUIREMENTS.md (this feature)
- `feature-docs/plus-menu-tmux-attach/SPEC.md` — the predecessor feature this one revises (its A3 "no subprocess" and A4 "one row per socket" decisions are superseded here)
- Notion task: https://www.notion.so/3a83509ec8ee80678267f11917d4d2d9
