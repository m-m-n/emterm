# Implementation Plan: mux-daemon-binary-update-detect

## Overview

Add a binary-update detection condition to the shared mux recovery probe so
that a daemon still running an old binary image is replaced through the
existing hot-upgrade path (in-place execve, pane shell PIDs preserved), even
when the protocol is compatible. Detection is driven by an identity file the
daemon records at startup; that recorded path also becomes the single source
of truth for the upgrade exec target.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate, `mux` module family)
- **Platform APIs**: Unix-only file/stat/exec semantics already used by the
  hot-upgrade feature (rename-replacement, O_NOFOLLOW hardening)
- **New third-party dependencies**: NONE. All work uses std + the already-
  present `libc` dependency. Project license (MIT) is unaffected; nothing to
  record for license review beyond this statement.

## Layer Structure

Unchanged from the existing mux architecture. This feature only touches:

| Layer | Component | Change |
|-------|-----------|--------|
| CLI entry | `mux::cli` (`resolve_attach_socket_with`, test fixtures) | unchanged control flow; new unit tests |
| Recovery probe | `mux::daemon::recover_from_legacy_daemon` | detection trigger added (Compatible arm), FR5 warning added (legacy arm) |
| Daemon runtime | `mux::daemon::run_daemon` (startup + upgrade branch) | identity recording; exec-target resolution switched to the recorded identity |
| Identity mechanism | `mux::identity` (NEW module) | record / read / compare |
| Upgrade machinery | `mux::upgrade`, `perform_upgrade_replacement`, `probe_candidate_handoff_range`, `send_upgrade`, `read_upgrade_response`, `wait_for_daemon_reachable_at_current_version` | reused UNCHANGED (FR6) |

Dependency direction: `cli` → `daemon` → `identity`. `identity` depends on
nothing inside `mux` beyond path derivation from a socket path.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `mux::identity` — identity file path derivation | Locate the identity file for a daemon socket | Pure function of the socket path. Returns the sibling file named `mux-identity.txt` in the socket's own (owner-only 0o700) directory. | task0001 (owner), task0002, task0003 (black-box: file name/location only) |
| `mux::identity` — record own identity (daemon side) | Persist the running daemon's start-binary identity | Precondition: the socket's parent directory exists (daemon startup has already ensured it). Behavior: capture the running process's executable path and that file's (device, inode); if the captured path stats cleanly, atomically replace the identity file (owner-only 0o600, never following a symlink at the destination or at any temporary path) and return the recorded identity for in-process use; if capture or the stat fails, remove any existing identity file and report "not recorded". Postcondition: the on-disk identity file describes the running image, or is absent. | task0001 (owner; called from daemon startup) |
| `mux::identity` — check recorded identity (client side) | Decide whether the daemon's binary was replaced | Cost bound (NFR1): at most one small-file read plus one stat of the recorded path. Returns a three-valued verdict: **Unchanged** (recorded (device, inode) equals the current stat of the recorded path), **Updated** (mismatch, or the recorded path no longer exists) carrying the recorded clean executable path, or **Undecidable** (identity file missing / unreadable / malformed / truncated, or the stat failed with an error other than not-found). A verdict of Updated is NEVER produced from a parse failure or a non-not-found stat error. | task0001 (owner), task0002 (trigger consumer) |
| Recorded identity value | In-process copy of what was persisted | Exposes the clean recorded executable path. This value — not any fresh executable-path resolution — is the upgrade exec target (FR4, NFR3). | task0001 (daemon upgrade branch) |
| Replacement notice line (FR2 / AC-8) | User-visible one-line notice when the trigger fires and the daemon becomes reachable again | Exactly this line on the client's standard error, and the same content logged at warn level: `Mux daemon upgraded in place to the newly installed binary` | task0002 (emits), task0003 (asserts) |
| Legacy-daemon warning line (FR5 / AC-5) | User-visible warning before the shutdown→respawn fallback | Exactly this line on the client's standard error, and the same content logged at warn level: `The running mux daemon predates in-place upgrade support; panes cannot be preserved and will be recreated.` | task0002 (emits), task0003 (may assert if it adds a black-box legacy scenario; primary assertion lives in task0002's unit tests) |
| Existing upgrade machinery | Fire and gate the replacement | Reused unchanged: request via the existing bare Upgrade frame (no payload — the client never transmits a path, NFR3); daemon-side compatibility gate `probe_candidate_handoff_range` stays in force; exec-failure re-entry via `run_daemon_in_handoff_mode` stays in force (FR6). | task0001, task0002, task0003 |

## Conventions

- **Platform gating (NFR2)**: every new item is Unix-only (`identity` module
  declared Unix-gated in `mux/mod.rs`, matching the existing `upgrade` /
  `inherited_pty` precedent). Windows and CLI-only (`--no-default-features`)
  builds compile with zero behavior change.
- **User-visible messages**: English, one line, written to standard error
  (the bridge takes over stdout; log output goes to files, not the
  terminal). Every user-visible line is also logged at warn level, because
  release builds persist only warn and higher.
- **Error policy**: detection is best-effort. No identity failure may ever
  make attach / mux start fail, and an undecidable comparison never fires
  (FR7). A refused or timed-out upgrade attempt warns and continues against
  the existing daemon (FR6 availability).
- **File hardening**: the identity file follows the handoff-file precedent —
  owner-only 0o600, refuse to follow symlinks, owner-only 0o700 directory
  (NFR3).

## Cross-task Design Decisions

### D1: Detection lives inside the shared recovery probe

The identity check and the firing logic are added to
`recover_from_legacy_daemon`'s Compatible arm (daemon.rs), NOT to its two
callers. Both entry points — `emterm mux attach`
(`resolve_attach_socket_with`, cli.rs) and `emterm mux` / `emterm mux
script` (`ensure_daemon_running`, daemon.rs) — call this one probe, so a
single change point covers FR2's attach-and-mux-start scope with no
duplication, and callers keep their existing signatures.
Affected: task0002 (implements), task0003 (verifies both entry points).

### D2: `mux::identity` has a single owner; consumers build to the contract

task0001 owns the module file end to end (creation, format, hardening,
unit tests). task0002 compiles against the Shared Components contract in
its own worktree using a clearly-marked minimal stand-in that matches the
pinned contract exactly (a stand-in whose check always reports
Undecidable is sufficient for task0002's own unit tests, which inject
verdicts). Integration keeps task0001's real module (parent-side adoption
at merge). **Wiring ownership**: task0002 owns wiring the trigger's
production call site to the pinned check API (stated in its Acceptance
Criteria), so once the real module is present at merge, the wiring is real
— nothing is left "to be wired later".
Affected: task0001, task0002.

### D3: Exec target comes exclusively from the recorded identity

The daemon's upgrade branch (the point that today resolves the candidate
via fresh executable-path resolution, daemon.rs:1355) switches to the
in-process recorded identity's clean path — the same value persisted in
the identity file, satisfying FR4's "single source of truth" and NFR3's
"derived solely from values the daemon itself recorded". When no identity
was recorded at startup (capture failed), the upgrade request is REFUSED
with a clear reason through the existing refusal reply channel — the
daemon keeps running with panes intact. There is deliberately no fallback
to fresh executable-path resolution: after a rename-replacement that route
resolves to a "(deleted)" path and re-enters on the old image, which is
the exact failure FR4 exists to remove.
Affected: task0001 (implements), task0002 (its trigger surfaces the
refusal reason to the user), task0003 (TS-3 proves the new image runs).

### D4: Identity lifecycle — record-or-invalidate at every daemon start

At EVERY pass through daemon startup — fresh bind, post-execve handoff
start, and re-entry after a failed exec — the daemon applies one uniform
rule: capture own executable identity; if the captured path stats cleanly,
(re)write the identity file; otherwise remove any existing identity file.
Consequences, in order of importance:

1. After a SUCCESSFUL upgrade, the new daemon re-records its own (new)
   identity, so the next probe correctly reports Unchanged.
2. After a FAILED exec (re-entry on the old image), the running process's
   executable path resolution reports a "(deleted)" target under rename
   semantics, the stat fails, and the identity file is REMOVED — so
   subsequent probes fall back to Undecidable (FR7) instead of re-firing a
   doomed upgrade on every attach. This is the loop-breaker: without it,
   attach → fire → exec fails → attach → fire … forever.
3. A daemon started before this feature simply has no identity file —
   probes report Undecidable and behave exactly as today (FR7 transition
   period).

Affected: task0001 (implements), task0002 (relies on Undecidable → no
fire), task0003 (TS-7).

### D5: Client-side firing flow (fire once, then continue regardless)

On a Compatible handshake, the probe consults the identity verdict:
Unchanged or Undecidable → return exactly as today (no Upgrade frame, no
broadcast — FR3/FR7). Updated → send the existing bare Upgrade frame on
the already-handshaked probe connection, read the daemon's reply through
the existing response reader, then poll the existing bounded
reachability wait. Outcomes:

1. Reachable again → print the pinned notice line, return success.
2. Daemon refused (reply carries a reason, e.g. D3's "no recorded
   identity" or the FR6 schema-gate rejection) → print a warning including
   the daemon's reason, return success — the original daemon keeps
   serving with panes intact.
3. Not reachable within the bound → print a warning, return success — the
   client proceeds to connect; if the daemon truly died, the existing
   downstream connection error surfaces as before.

The probe fires AT MOST once per invocation and never converts a
detection or upgrade failure into an attach failure.
Affected: task0002 (implements), task0003 (TS-3/TS-4/TS-5 observe the
externally visible halves).

### D6: FR5 warning placement

The legacy-daemon arm of the probe (previous-protocol daemon; Upgrade
frame silently discarded or the send failed) prints the pinned FR5
warning line to standard error at the point where it commits to the
shutdown→respawn fallback — covering both the ignored-upgrade timeout
route and the failed-send route. The fallback behavior itself is
byte-for-byte the existing one; only visibility is added.
Affected: task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Fire loop when exec keeps failing | Low | High | D4 rule 2: failed-exec re-entry invalidates the identity file, so the next probe is Undecidable and does not fire |
| Firing on a vanished binary (recorded path removed, nothing installed) | Low | Medium | The daemon-side schema probe cannot run the candidate, refuses the upgrade (FR6); client warns and continues; per-attach cost is one refused round, spec-mandated behavior |
| Concurrent probes from two clients both detect and fire | Medium | Low | The daemon serializes upgrade requests through its existing single upgrade-signal channel; the second request either arrives at the already-replaced daemon (Unchanged → its client had already fired; the daemon refuses nothing because the second client's probe re-checks after handshake) or is refused/ignored harmlessly; replacement is idempotent at the identity level |
| Identity file inconsistent with running image in untested corner states | Low | Medium | D4's uniform record-or-invalidate rule is the only writer; TS-2 covers malformed/truncated content; FR7 makes every undecided state safe (no fire) |
| Merge conflicts on daemon.rs between task0001 and task0002 | Medium | Low | Edits target disjoint regions (startup/upgrade branch vs probe arms); implementer parent-side-adoption protocol resolves overlaps; review/verify catch integration mismatches |

## Open Questions

- [ ] None. All requirements are resolved; no TBD, no new dependencies, no
      existing planning artifacts to reconcile.
