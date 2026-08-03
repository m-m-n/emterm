# Feature: mux-daemon-binary-update-detect

## Overview

The mux daemon keeps running the binary image it was started with, so updating the eMterm binary leaves the daemon on the old image (confirmed in practice on 2026-08-02). This feature adds a binary-update detection condition that fires even when the protocol is compatible, and hands the replacement to the existing hot-upgrade path (in-place `execve` with PTY master FD handover) so pane shells survive with their PIDs intact. Requirements source: `REQUIREMENTS.md` in this directory.

## Objectives

- Make the mux daemon switch to the new binary whenever the eMterm binary is updated, by adding a detection condition that fires even under protocol compatibility.
- Perform the replacement through the existing hot-upgrade mechanism (in-place `execve`, PTY master FD handover), preserving pane shell PIDs.
- Eliminate the "rebuilt and reinstalled but the fix is not there" state in which the daemon keeps running the old binary.

## User Stories

### US1: Attach after a binary update
As a mux user, I want `emterm mux attach` to notice that the daemon's binary was replaced, so that I am not talking to a stale daemon image.

**Acceptance Criteria:**
- [ ] AC-1: The daemon start binary and the currently installed binary being different is detected (including the case where the daemon's `/proc/<pid>/exe` reads `(deleted)`).
- [ ] AC-2: On detection, the hot-upgrade (in-place `execve`) runs and pane shells survive with unchanged PIDs.
- [ ] AC-3: Replacement happens even when the protocol is compatible, as long as the binary was updated.
- [ ] AC-6: The replaced daemon runs the newly installed binary image (the exec target does not resolve to a `(deleted)` path).
- [ ] AC-8: On firing, a one-line notice about the replacement is shown to the attaching client.

### US2: Start mux after a binary update
As a mux user, I want `emterm mux` startup to apply the same detection as attach, so that either entry point brings the daemon up to date.

**Acceptance Criteria:**
- [ ] AC-1 / AC-2 / AC-3 / AC-6 hold on the `ensure_daemon_running` recovery-probe path as well.
- [ ] AC-8: The one-line replacement notice is also shown on the mux-start path.

### US3: No churn when the binary is unchanged
As a mux user, I want nothing to happen when the binary is identical, so that attaching never causes an unnecessary restart.

**Acceptance Criteria:**
- [ ] AC-4: With an identical binary, no replacement occurs — no restart, no process replacement, no `Upgrading` broadcast.
- [ ] AC-7: When identity information is missing or malformed, detection does not misfire and falls back to the previous behaviour.

### US4: Connect to a daemon without hot-upgrade support
As a user of an environment running an older daemon generation, I want to be told that my panes will be recreated, so that the fallback is not silent.

**Acceptance Criteria:**
- [ ] AC-5: With an old daemon that silently discards `Upgrade` frames, a warning that panes may be destroyed is shown, and the existing fallback then runs.

## Technical Requirements

### Functional Requirements

- **FR1 — Binary-update detection via an identity file:** The daemon records, immediately after start, the identity of its own start binary — the executable path it was started from and that file's `(device, inode)` — into an identity file inside the same owner-only (0o700) directory as the listen socket. The client-side recovery probe reads this identity file, stats the actual file at the recorded path, and compares against the current `(device, inode)`. A mismatch (or `ENOENT` on the recorded path) is judged as "the binary became a different file". The comparison basis is exclusively the daemon's own start path (daemon-own-path); the attaching client's own binary is never part of the judgement — attaching from a development build to a daemon started from `/usr/bin` does not fire, and no upgrade ping-pong arises from mixed-build clients. This approach also decides the case where the daemon's `/proc/self/exe` points at `(deleted)` (post-start `rename(2)` replacement), and costs about one small-file read plus one `stat` per attach.
- **FR2 — Automatic hot-upgrade on detection (with notice):** On detection, fire the in-place replacement unconditionally through the existing Upgrade path (`send_upgrade` → `prepare_upgrade` → `execve`, daemon.rs:961-1087 / cli.rs:332-362), with no confirmation prompt and no opt-out setting. On firing, show the attaching (or mux-starting) client a one-line message stating that the daemon was replaced with the new binary, and record the same in the log (auto-with-notice). The trigger fires even when the protocol is compatible and `recover_from_legacy_daemon` returns `Compatible`, as long as a binary update is detected. Pane shells survive with unchanged PIDs. The firing sites are both paths sharing the same recovery probe — `emterm mux attach` (cli.rs:494, `resolve_attach_socket_with`) and `emterm mux` startup (daemon.rs:219, `ensure_daemon_running`) — i.e. attach-and-mux-start. Binary-update detection on the GUI client side is out of scope.
- **FR3 — No replacement for an identical binary:** When the identity comparison matches (same binary), do nothing as before. Induce no unnecessary restart, no process replacement and no `Upgrading` broadcast at all.
- **FR4 — Correct resolution of the upgrade target path:** The hot-upgrade exec target resolves to the clean start-time executable path the daemon recorded in the identity file (i.e. the real path where the new binary was installed over the old one). The current implementation's `current_exe()`-based candidate resolution (daemon.rs:1355, `self_exec::self_exe_path`) returns `…/emterm (deleted)` after replacement, making `exec` fail with `ENOENT` and re-entering on the old image, so that route is not used here. The path recorded by FR1's identity mechanism is the single source of truth for target resolution.
- **FR5 — Handling of daemons without hot-upgrade support (warn and proceed):** When an old daemon without hot-upgrade support (a generation that silently discards `Upgrade` frames) is running, explicitly print to the attaching client's standard error that panes cannot be preserved and will be recreated because the daemon is of an older generation, then fall back to the existing shutdown→respawn as before (warn-and-proceed). The behaviour itself is identical to the current fallback at daemon.rs:647-679; only user-facing visibility is added.
- **FR6 — Preservation of the existing safety gates:** The existing handoff schema-range probe (`probe_candidate_handoff_range` inside `prepare_upgrade`, daemon.rs:976-984) that gates candidate-binary compatibility remains in force under the new firing condition; for an incompatible candidate the upgrade is refused and the original daemon keeps running with its panes intact. The `run_daemon_in_handoff_mode` re-entry on `execve` failure (cli.rs:332-362) also keeps working as before.
- **FR7 — Fallback when identity information is absent:** When the identity file is missing, unreadable or corrupt (including the transition period in which a daemon started before this feature keeps running), do not misfire: fall back to the previous behaviour (protocol judgement only). An undecidable comparison is never interpreted as "updated".

### Non-Functional Requirements

- **NFR1 — Performance:** The detection cost is paid on every attach and therefore must stay light. Keep it to about an identity-file read plus a `stat` of the recorded path; do not read the whole binary or compute a hash on every attach.
- **NFR2 — Compatibility / platform:** This feature is Unix/Linux only (the `execve`-based hot-upgrade is Unix-only). Windows behaviour is unchanged.
- **NFR3 — Security:** The identity file follows the same hardening rules as the existing handoff file (owner-only 0o600, `O_NOFOLLOW`, 0o700 directory, following the precedent of `create_handoff_file` in upgrade.rs). The exec target path is derived solely from values the daemon itself recorded; a path writable by others, or a value declared by a connecting client, is never exec'd unverified.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────────────┐
│  CLI entry points                            │
│  `emterm mux attach` / `emterm mux`          │
├─────────────────────────────────────────────┤
│  Recovery probe (shared)                     │
│  resolve_attach_socket_with (cli.rs:494)     │
│  ensure_daemon_running     (daemon.rs:219)   │
├─────────────────────────────────────────────┤
│  Binary-identity comparison (FR1 / FR7)      │
│  read identity file -> stat recorded path    │
├─────────────────────────────────────────────┤
│  Existing Upgrade path (FR2 / FR4 / FR6)     │
│  send_upgrade -> prepare_upgrade -> execve   │
├─────────────────────────────────────────────┤
│  Runtime directory (owner-only 0o700)        │
│  listen socket + identity file (0o600)       │
└─────────────────────────────────────────────┘
```

**Component Diagram:**
```
daemon (start)      -> writes identity file: { exec path, device, inode }   [FR1]
recovery probe      -> reads identity file, stats recorded path             [FR1, NFR1]
                       match       -> no action                             [FR3]
                       mismatch    -> fire upgrade                          [FR2]
                       ENOENT      -> fire upgrade                          [FR1]
                       unreadable  -> legacy behaviour (protocol only)      [FR7]
upgrade path        -> exec target taken from the recorded path             [FR4]
                       handoff schema-range probe gates the candidate       [FR6]
legacy daemon path  -> warn on stderr, then shutdown -> respawn             [FR5]
```

### Data Flow

```
daemon start        → record { path, device, inode } → identity file (0o600, in the 0o700 socket dir)
client attach/start → read identity file → stat(recorded path) → compare (device, inode)
                    → mismatch/ENOENT → send_upgrade → prepare_upgrade (schema-range probe)
                                       → execve(recorded path) → new daemon image, pane PIDs unchanged
                                       → one-line notice to the client + log entry
                    → match           → no action
                    → unreadable      → protocol judgement only (previous behaviour)
```

### API Design

No new IPC message type is introduced. The existing Upgrade path (`send_upgrade` → `prepare_upgrade` → `execve`) is reused unchanged; this feature only adds a firing condition and fixes exec-target path resolution.

**Client-visible output:**

```
notice (FR2, AC-8): one line stating the daemon was replaced with the new binary
                    (also written to the log)
warning (FR5, AC-5): stderr line stating that panes cannot be preserved and will be
                     recreated because the daemon is of an older generation
```

### Identity File

The only persistent data this feature introduces.

| Field | Description | Required |
|-------|-------------|----------|
| executable path | The daemon's start-time executable path; the single source of truth for exec-target resolution (FR4) | Yes |
| device | The `device` of the start binary at daemon start | Yes |
| inode | The `inode` of the start binary at daemon start | Yes |

**Placement and hardening (NFR3):**
- Located in the same owner-only (0o700) directory as the listen socket.
- File mode 0o600, opened with `O_NOFOLLOW`, following the `create_handoff_file` precedent in upgrade.rs.

### Dependencies

**Internal Dependencies:**
- Existing hot-upgrade mechanism (snapshot / handoff / `execve` / restore): reused unchanged; this feature adds only the firing condition and the exec-target path fix.
- Recovery probe shared by `resolve_attach_socket_with` (cli.rs:494) and `ensure_daemon_running` (daemon.rs:219): the integration point for detection.
- `probe_candidate_handoff_range` inside `prepare_upgrade` (daemon.rs:976-984): the compatibility gate that must remain in force.
- `run_daemon_in_handoff_mode` re-entry (cli.rs:332-362): the `execve`-failure path that must keep working.
- `create_handoff_file` (upgrade.rs): the file-hardening precedent the identity file follows.
- `self_exec::self_exe_path` (daemon.rs:1355): the `current_exe()`-based candidate resolution that is explicitly *not* used for this path (FR4).

**External Dependencies:**
- Unix/Linux platform APIs (`stat`, `execve`, `O_NOFOLLOW`). Windows is out of scope (NFR2).

### File Structure

Files referenced by the resolved requirements (paths as cited there):

```
daemon.rs      # ensure_daemon_running (:219), legacy fallback (:647-679),
               # Upgrade path (:961-1087), handoff range probe (:976-984),
               # self_exec::self_exe_path (:1355, not used for this path)
cli.rs         # resolve_attach_socket_with (:494),
               # run_daemon_in_handoff_mode re-entry (:332-362),
               # spawn_fake_legacy_daemon-style fixtures
upgrade.rs     # create_handoff_file (file-hardening precedent)
self_exec.rs   # is_missing test group (table style reused by TS-1)
mux_hot_upgrade.rs  # integration tests extended by TS-3 / TS-4 / TS-5
```

## Test Scenarios

### Unit Tests
- [ ] TS-1 (FR1, FR3, FR7): Identity comparison predicate — recorded `(dev, ino)` equal to the current value → does not fire; unequal → fires; recorded path `ENOENT` → fires; any other `stat` error → does not fire (a table in the same style as the `is_missing` test group in self_exec.rs).
- [ ] TS-2 (FR1, FR7, NFR3): Identity-file write/read round trip, and fallback in each of the missing / truncated / malformed cases (AC-7). Verification of file permissions (0o600) and symlink rejection.

### Integration Tests
- [ ] TS-3 (FR1, FR2, FR4) — mux_hot_upgrade.rs extension, main line for AC-1/2/3/6: start a real daemon under an isolated `XDG_RUNTIME_DIR` and record the real shell's PID → replace the daemon's start binary with a new copy via `rename(2)` (reproducing the state where `/proc/<pid>/exe` reads `(deleted)`) → attach → verify the hot-upgrade fires, the shell PID survives unchanged, and the daemon runs the new binary image (confirmed through the handoff log / an identifying marker).
- [ ] TS-4 (FR3) — mux_hot_upgrade.rs extension, AC-4: attach without replacing the binary → verify no upgrade fires (no `Upgrading` broadcast, no handoff-start log).
- [ ] TS-5 (FR2) — mux_hot_upgrade.rs extension, trigger scope: verify the recovery probe on the `emterm mux` fresh-start side (`ensure_daemon_running` path) fires the same way after a binary replacement.
- [ ] TS-6 (FR5) — cli.rs fake-daemon style, AC-5: against a stand-in daemon that silently discards `Upgrade` frames (in the style of the `spawn_fake_legacy_daemon` fixtures), verify the pane-destruction warning is emitted to the user and the shutdown→respawn fallback then runs.
- [ ] TS-7 (FR7) — AC-7: attach to a daemon with no identity file (simulating the pre-feature generation) → verify no misfire and the previous protocol-judgement-only behaviour.
- [ ] TS-8 (FR2) — unit/integration, AC-8: verify a one-line replacement notice is emitted to the attaching client when the trigger fires.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] The daemon's `/proc/self/exe` points at `(deleted)` after a post-start `rename(2)` replacement — detection still decides correctly via the identity file (FR1), and the exec target is taken from the recorded path rather than the `(deleted)` one (FR4, AC-6).
- [ ] The recorded path returns `ENOENT` — judged as "the binary became a different file" and fires (FR1).
- [ ] A `stat` error other than `ENOENT` — does not fire (TS-1).
- [ ] The identity file is missing, unreadable or corrupt, including a pre-feature daemon still running — falls back to protocol judgement only, never interpreting undecidability as "updated" (FR7, AC-7).
- [ ] A mixed-build client (development build attaching to a `/usr/bin`-started daemon) — does not fire, because the comparison basis is daemon-own-path only, structurally excluding upgrade ping-pong (FR1).
- [ ] The candidate binary fails the handoff schema-range probe — the upgrade is refused and the original daemon keeps running with its panes intact (FR6).
- [ ] `execve` fails — `run_daemon_in_handoff_mode` re-entry works as before (FR6).
- [ ] The protocol is compatible (`recover_from_legacy_daemon` returns `Compatible`) but the binary was updated — the upgrade still fires (FR2, AC-3).

### Performance Tests
- [ ] Detection cost per attach stays at roughly one small-file read plus one `stat`; the whole binary is not read and no hash is computed on each attach (NFR1).

## Security Considerations

- **Authentication:** Not applicable — the identity file lives inside the same owner-only (0o700) runtime directory as the listen socket (FR1, NFR3).
- **Authorization:** File access is restricted by the owner-only 0o700 directory and the 0o600 file mode (NFR3).
- **Input Validation:** The exec target path is derived solely from values the daemon itself recorded; a path writable by others, or a value declared by a connecting client, is never exec'd unverified (NFR3). Malformed or truncated identity content is rejected into the FR7 fallback rather than acted upon.
- **Data Protection:** The identity file follows the existing handoff-file hardening rules: owner-only 0o600, `O_NOFOLLOW`, 0o700 directory, per the `create_handoff_file` precedent in upgrade.rs (NFR3).
- **Symlink Handling:** Opening with `O_NOFOLLOW` rejects symlinked identity files (NFR3, verified by TS-2).
- **Candidate Binary Gate:** The existing handoff schema-range probe keeps gating which candidate binary may be exec'd (FR6).
- **XSS Prevention:** Not applicable (no web surface in this feature).
- **SQL Injection Prevention:** Not applicable (no database).
- **CSRF Protection:** Not applicable (no HTTP surface).

## Error Handling

### Error Conditions

| Condition | Requirement | Handling | User-visible result |
|-----------|-------------|----------|---------------------|
| Recorded `(dev, ino)` differs from current | FR1, FR2 | Fire the hot-upgrade through the existing Upgrade path | One-line notice that the daemon was replaced with the new binary |
| Recorded path returns `ENOENT` | FR1, FR2 | Treated as "binary became a different file"; fire | Same as above |
| `stat` fails with an error other than `ENOENT` | FR7 | Do not fire | No change from previous behaviour |
| Identity file missing / unreadable / corrupt | FR7 | Fall back to protocol judgement only | No change from previous behaviour |
| Candidate binary fails the handoff schema-range probe | FR6 | Refuse the upgrade | The original daemon keeps running with panes intact |
| `execve` fails | FR6 | `run_daemon_in_handoff_mode` re-entry (existing behaviour) | Session continues |
| Daemon silently discards `Upgrade` frames (older generation) | FR5 | Warn, then shutdown→respawn fallback | stderr warning that panes cannot be preserved and will be recreated |

### Error Flow

```
Detection undecidable → do not interpret as "updated" → previous behaviour (protocol judgement only)
Upgrade refused/failed → original daemon keeps panes → session continues
Legacy daemon → warn on stderr → shutdown → respawn
```

## Performance Optimization

### Performance Goals
- Per-attach detection cost: about one small-file read plus one `stat` (NFR1).

### Optimization Strategies
- Identity-file comparison instead of binary content hashing: compare the recorded `(device, inode)` against the stat result rather than reading or hashing the binary on each attach (NFR1).
- Reuse the existing recovery probe on both entry paths so detection is performed once per attach / mux start rather than on a separate schedule (FR2).

### Caching Strategy
- The identity file is written once at daemon start (FR1); no additional cache is introduced.

## Success Criteria

- [ ] All functional requirements (FR1–FR7) are implemented and tested
- [ ] All test scenarios (TS-1–TS-8) pass
- [ ] AC-1: The daemon start binary and the currently installed binary being different is detected (including `/proc/<pid>/exe` reading `(deleted)`)
- [ ] AC-2: On detection the hot-upgrade runs and pane shells survive with unchanged PIDs
- [ ] AC-3: Replacement happens even when the protocol is compatible, as long as the binary was updated
- [ ] AC-4: With an identical binary no replacement occurs (no unnecessary restart is induced)
- [ ] AC-5: With an old daemon lacking hot-upgrade support, a warning that panes may be destroyed is shown before the existing fallback runs
- [ ] AC-6: The replaced daemon runs the newly installed binary image (the exec target does not resolve to a `(deleted)` path)
- [ ] AC-7: Missing or invalid identity information causes no misfire and falls back to the previous behaviour
- [ ] AC-8: On firing, a one-line replacement notice is shown to the attaching / mux-starting client
- [ ] Performance meets NFR1 (light per-attach detection)
- [ ] Security requirements (NFR3) are satisfied
- [ ] Platform scope (NFR2, Unix/Linux only; Windows unchanged) is respected

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement (FR1–FR7, NFR1–NFR3) is `status: resolved`.

## References

- Requirements document: `feature-docs/mux-daemon-binary-update-detect/REQUIREMENTS.md`
- Upgrade path: daemon.rs:961-1087 (`send_upgrade` → `prepare_upgrade` → `execve`), cli.rs:332-362 (`run_daemon_in_handoff_mode` re-entry)
- Handoff schema-range probe: daemon.rs:976-984 (`probe_candidate_handoff_range`)
- Attach-side recovery probe: cli.rs:494 (`resolve_attach_socket_with`)
- Mux-start-side recovery probe: daemon.rs:219 (`ensure_daemon_running`)
- Candidate resolution not used by this path: daemon.rs:1355 (`self_exec::self_exe_path`)
- Legacy-daemon fallback: daemon.rs:647-679
- File-hardening precedent: `create_handoff_file` in upgrade.rs
- Existing test assets: `is_missing` test group in self_exec.rs, mux_hot_upgrade.rs, `spawn_fake_legacy_daemon`-style fixtures in cli.rs
