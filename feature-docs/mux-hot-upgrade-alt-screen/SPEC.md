# Feature: mux-hot-upgrade-alt-screen

## Overview

A mux daemon hot-upgrade (self-exec on binary-update detection) currently
corrupts alt-screen TUIs: after the upgrade and a subsequent reattach,
applications running on the alternate screen (Claude Code, glances,
nethogs, other ncurses TUIs) show pre-alt-screen scrollback fragments
instead of their real screen. This feature carries alt-screen state across
the handoff so the restored parser reports `alternate_screen() == true`,
and it corrects the NFR3 group-write path check that today prevents a
umask 002 development build from firing hot-upgrade at all — without which
the alt-screen fix cannot be verified on a dev build. Requirement source:
`REQUIREMENTS.md` in this directory.

## Objectives

- Prevent a mux daemon hot-upgrade from corrupting alt-screen TUIs, so that
  after the upgrade and a subsequent reattach the alternate-screen
  application renders correctly instead of showing pre-alt-screen
  scrollback fragments.
- Make development builds under umask 002 (Debian/Ubuntu default) able to
  trigger hot-upgrade at all, so the fix above is verifiable on a dev
  build. This requires correcting the NFR3 group-write check that currently
  rejects private per-user-group 0o775 paths
  (`src-tauri/src/mux/identity.rs:455`, deferred high finding
  `sid-nfr3-group-write-blocks-dev-builds`).

## User Stories

### US1: Keep an alt-screen TUI intact across a hot-upgrade

As a user running an alt-screen TUI in a mux pane, I want the daemon's
hot-upgrade and my subsequent reattach to leave the TUI's screen intact, so
that I do not see pre-alt-screen shell fragments where the application
should be.

**Acceptance Criteria:**
- [ ] A restored pane whose handoff record has alt flag = true yields
      `shadow_parser.screen().alternate_screen() == true` after
      `from_restored`, and the next reattach's `build_snapshot_bytes` takes
      the alt branch (AC-3)
- [ ] With an alt-screen TUI open (Claude Code / glances), triggering a
      hot-upgrade leaves the screen uncorrupted after reattach, with no
      pre-alt-screen shell fragments showing through (AC-5)
- [ ] V2 documents migrate to V3 with alt = false / empty dump and restore
      exactly as before; version range 1..=3 is advertised (AC-4)

### US2: Fire hot-upgrade from a dev build under umask 002

As a developer verifying the fix, I want a daemon started from the dev
build on a umask 002 machine to fire hot-upgrade, so that the alt-screen
behavior above can be checked on a real machine.

**Acceptance Criteria:**
- [ ] On a umask 002 machine, a daemon started from the dev build
      (`src-tauri/target-host/release/emterm`) fires hot-upgrade (AC-2)
- [ ] The group-write predicate accepts a 0o775 path whose group is the
      owner's private per-user group (all three FR1 conditions), and rejects
      a group with extra `gr_mem` members, a group whose gid is not the
      owner's primary gid, a group whose name differs from the owner's user
      name (e.g. gid=100 "users" as primary), any failed lookup, and any
      S_IWOTH path (AC-1)

## Technical Requirements

### Functional Requirements

- **FR1 - Private per-user group exemption in the NFR3 path-writability check:**
  The binary/parent-directory writability check in
  `src-tauri/src/mux/identity.rs` permits group-write (S_IWGRP) only when
  ALL three conditions hold: (a) the group's `gr_mem` contains no name
  other than the owner's; (b) the owner's primary gid equals that group's
  gid; (c) the group's name equals the owner's user name. If any condition
  fails or cannot be confirmed, the path is rejected as today
  (fail-closed).
- **FR2 - World-write rejection unchanged:**
  A path with S_IWOTH set is rejected unconditionally, exactly as in the
  current implementation.
- **FR3 - HandoffPane carries alt-screen state; schema version 3:**
  `HandoffPane` (`crates/mux_ipc/src/handoff.rs`) gains an alt-screen flag
  and an alt-screen screen dump, and `HANDOFF_SCHEMA_VERSION` is raised
  from 2 to 3.
- **FR4 - V2-to-V3 migration with preserved legacy behavior:**
  `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` widens to 1..=3 and a V2-to-V3
  migration is added in the established pattern (per-version struct +
  `From` impl, as V1-to-V2 did). Documents originating from V2 (and V1 via
  the existing chain) are filled with alt flag = false and an empty dump,
  preserving existing restore behavior.
- **FR5 - `snapshot_pane` captures with the existing main/alt split:**
  `mux::upgrade::snapshot_pane` (`src-tauri/src/mux/upgrade.rs:547`)
  captures pane state using the same main/alt split contract as
  `build_snapshot_bytes` (`src-tauri/src/mux/session/pane.rs:1073` and
  `1229`): alt-screen panes contribute the alt flag and a
  `contents_formatted()`-style dump; main-buffer panes are unchanged.
- **FR6 - Restore replays alt-screen state into the `shadow_parser`:**
  `restore_pane` (`src-tauri/src/mux/upgrade.rs:687`) /
  `MuxPane::from_restored` (`src-tauri/src/mux/session/pane.rs:1971`)
  replay the restored scrollback first, then, when the alt-screen flag is
  true, feed `ESC[?1049h` followed by the captured screen dump into the
  `shadow_parser`, so the restored parser reports `alternate_screen() ==
  true`.
- **FR7 - Alt-state re-capture to narrow the tearing window:**
  The alt-screen flag and dump are re-captured at the same point as the
  existing `refresh_live_agent_state` pass
  (`src-tauri/src/mux/upgrade.rs:384`, invoked after the client-ack wait),
  so a buffer switch between snapshot and exec is narrowed to the same
  window as agent-state staleness.
- **FR8 - Alt-screen dump size upper-bound policy:**
  The handoff document's alt-screen dump is governed by an explicitly
  documented upper-bound policy (`contents_formatted()` can balloon at
  extreme screen dimensions — `src-tauri/src/mux/session/pane.rs:1282`; the
  handoff is a file, so snapshot-frame limits do not apply). The concrete
  limit value and overflow behavior are decided in the design/plan step.

### Non-Functional Requirements

- **NFR1 - Bounded identity lookups:**
  The private-group decision uses only one `getgrgid` call and one
  `getpwuid` call; it never enumerates the passwd database (no
  `getpwent`), because full scans are slow/unstable under LDAP/SSSD.
- **NFR2 - Fail-closed security posture:**
  Any lookup failure or unverifiable condition in the group-write check
  results in rejection. The threat model: the daemon `execve()`s the path,
  so any write access by a non-owner (other than root) means arbitrary code
  execution with daemon privileges; parent directories remain in scope.
- **NFR3 - Backward-compatible handoff decoding:**
  V1 and V2 handoff documents continue to decode and restore with behavior
  identical to today (alt flag false, empty dump).
- **NFR4 - No new concepts or external dependencies:**
  The fix reuses the existing main/alt split contract and the existing
  per-version handoff migration pattern; no external process-migration
  tooling (reptyr rejected — the defect is in-memory `shadow_parser` state,
  which such tools cannot carry).

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────┐
│  Hot-upgrade trigger (binary-update detect)  │
│  NFR3 path-writability check (identity.rs)   │  ← FR1 / FR2 / NFR1 / NFR2
├──────────────────────────────────────────────┤
│  Upgrade sequence (mux/upgrade.rs)           │
│   snapshot_pane        (FR5)                 │
│   refresh pass after client-ack wait  (FR7)  │
│   self-exec                                  │
│   restore_pane         (FR6)                 │
├──────────────────────────────────────────────┤
│  Handoff document (mux_ipc/handoff.rs)       │
│   HandoffPane + alt flag + alt dump   (FR3)  │
│   schema v3, supported 1..=3          (FR4)  │
│   dump size upper-bound policy        (FR8)  │
├──────────────────────────────────────────────┤
│  Pane state (mux/session/pane.rs)            │
│   build_snapshot_bytes main/alt split (FR5)  │
│   MuxPane::from_restored → shadow_parser     │
│                                       (FR6)  │
└──────────────────────────────────────────────┘
```

**Component Diagram:**
```
identity.rs:455  ── group-write predicate (FR1/FR2, NFR1 bounded lookups, NFR2 fail-closed)

upgrade.rs:547  snapshot_pane ──┐
upgrade.rs:384  refresh pass  ──┤→ handoff.rs HandoffPane {alt flag, alt dump} (FR3/FR4/FR8)
upgrade.rs:687  restore_pane  ──┘        │
                                         └→ pane.rs:1971 MuxPane::from_restored
                                              → scrollback replay
                                              → ESC[?1049h + dump → shadow_parser (FR6)
                                              → next reattach: pane.rs:1073/1229
                                                build_snapshot_bytes alt branch (FR5)
```

### Data Flow

```
Alt-screen pane
  → snapshot_pane (main/alt split, FR5)
  → re-capture at the refresh_live_agent_state point after client-ack wait (FR7)
  → handoff document v3 {alt flag, alt dump} (FR3, size-bounded per FR8)
  → self-exec
  → restore_pane / from_restored: scrollback replay, then ESC[?1049h + dump (FR6)
  → shadow_parser reports alternate_screen() == true
  → next reattach: build_snapshot_bytes takes the alt branch
```

### API Design

Not applicable — this feature adds no API surface. Per NFR4 no external
process-migration tooling is introduced (reptyr rejected: the defect is
in-memory `shadow_parser` state, which such tools cannot carry).

### Database Schema

Not applicable — no database. The persisted structure this feature changes
is the handoff document:

| Field | Introduced by | Legacy (V1/V2-originated) value |
|-------|---------------|---------------------------------|
| `HandoffPane` alt-screen flag | FR3 | false (FR4 / NFR3) |
| `HandoffPane` alt-screen screen dump | FR3 | empty (FR4 / NFR3) |
| `HANDOFF_SCHEMA_VERSION` | FR3 | raised 2 → 3 |
| `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` | FR4 | widened to 1..=3 |

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/mux/identity.rs` (`:455`): the NFR3 path-writability check
  — target of FR1 / FR2, constrained by NFR1 / NFR2
- `crates/mux_ipc/src/handoff.rs`: `HandoffPane`,
  `HANDOFF_SCHEMA_VERSION`, `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` — target of
  FR3 / FR4, constrained by NFR3
- `src-tauri/src/mux/upgrade.rs` (`:384` / `:547` / `:687`):
  `refresh_live_agent_state` / `snapshot_pane` / `restore_pane` — targets
  of FR7 / FR5 / FR6
- `src-tauri/src/mux/session/pane.rs` (`:1073` / `:1229` / `:1971` /
  `:1282`): `build_snapshot_bytes` main/alt split, `MuxPane::from_restored`,
  `contents_formatted()` — FR5 / FR6 / FR8
- The existing per-version handoff migration pattern (per-version struct +
  `From` impl, as V1-to-V2 did) — reused per FR4 / NFR4

**External Dependencies:**
- libc (identity checks): this is a Unix-only surface; Windows behavior is
  unaffected
- No new external dependency is introduced (NFR4)

### File Structure

```
src-tauri/src/mux/
├── identity.rs              # NFR3 path-writability check — FR1, FR2, NFR1, NFR2
├── upgrade.rs               # snapshot_pane (FR5), refresh pass (FR7), restore_pane (FR6)
└── session/pane.rs          # build_snapshot_bytes split (FR5), from_restored (FR6),
                             # contents_formatted (FR8)
crates/mux_ipc/src/
└── handoff.rs               # HandoffPane, schema v3, 1..=3, V2→V3 migration — FR3, FR4, NFR3
src-tauri/tests/
└── mux_hot_upgrade.rs       # integration coverage (run with --test-threads=1)
```

## Test Scenarios

### Unit Tests
- [ ] TS1 (FR1, FR2, NFR1, NFR2): Unit tests in
      `src-tauri/src/mux/identity.rs` for every FR1/FR2 branch of the
      private-group predicate (accept and each reject reason), following the
      existing inline `#[cfg(test)]` convention.
- [ ] TS2 (FR3, FR4, NFR3): Unit tests in `crates/mux_ipc/src/handoff.rs`
      for the V2 to V3 `From` migration and the widened
      `SUPPORTED_HANDOFF_SCHEMA_VERSIONS` range, mirroring the existing V1
      to V2 tests.
- [ ] TS3 (FR3, FR5, FR6, FR7): Unit tests around `snapshot_pane` /
      `restore_pane` / `from_restored` in
      `src-tauri/src/mux/{upgrade.rs,session/pane.rs}`: alt pane round-trips
      to `alternate_screen() == true`; main pane behavior unchanged;
      refresh-position re-capture updates a pane that switched buffers after
      snapshot.

### Integration Tests
- [ ] TS4 (FR5, FR6, FR7): Integration coverage via
      `src-tauri/tests/mux_hot_upgrade.rs` (run with `--test-threads=1`)
      extended with an alt-screen pane scenario if feasible in that harness;
      otherwise unit-level fixation plus the manual AC-2/AC-5 checks.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] TS5 (FR1, FR6, FR7): Manual verification — AC-2 (dev-build
      hot-upgrade fires under umask 002) and AC-5 (alt-screen TUI intact
      after upgrade + reattach), performed by the user on a real machine.

### Edge Cases
- [ ] A group with extra `gr_mem` members, a group whose gid is not the
      owner's primary gid, and a group whose name differs from the owner's
      user name (e.g. gid=100 "users" as primary) are all rejected (AC-1).
- [ ] Any failed identity lookup is rejected (fail-closed, NFR2).
- [ ] Any S_IWOTH path is rejected unconditionally (FR2).
- [ ] Parent directories remain in scope for the writability check (NFR2).
- [ ] V1 and V2 handoff documents decode and restore identically to today,
      with alt flag false and empty dump (NFR3 / FR4).
- [ ] A pane that switched buffers between snapshot and the refresh point
      is updated by the re-capture (FR7).
- [ ] `contents_formatted()` can balloon at extreme screen dimensions; the
      dump is governed by the documented upper-bound policy (FR8).

### Performance Tests
Not applicable as a load/stress exercise. The only performance constraint is
NFR1 (one `getgrgid` and one `getpwuid`, never `getpwent`), covered by TS1.

## Security Considerations

- **Authentication:** Not applicable.
- **Authorization:** The path-writability check is the authorization
  surface. Group-write is permitted only under the three FR1 conditions;
  S_IWOTH is rejected unconditionally (FR2).
- **Input Validation:** The group-write decision validates `gr_mem`
  membership, the owner's primary gid, and the group name against the
  owner's user name (FR1).
- **Data Protection:** Not applicable — the handoff document carries pane
  state only.
- **Threat model (NFR2):** The daemon `execve()`s the path, so any write
  access by a non-owner (other than root) means arbitrary code execution
  with daemon privileges. Parent directories remain in scope. Any lookup
  failure or unverifiable condition results in rejection (fail-closed).
- **XSS Prevention:** Not applicable — no web surface.
- **SQL Injection Prevention:** Not applicable — no database.
- **CSRF Protection:** Not applicable — no web surface.

## Error Handling

### Error Codes

Not applicable — no error-code surface is introduced. The decision outcomes
are:

| Condition | Outcome | Requirement |
|-----------|---------|-------------|
| All three private-group conditions hold on an S_IWGRP path | Accept | FR1 |
| Any of the three conditions fails | Reject | FR1 |
| Any condition cannot be confirmed (e.g. lookup failure) | Reject (fail-closed) | FR1 / NFR2 |
| S_IWOTH set | Reject unconditionally | FR2 |
| Handoff document originating from V1 / V2 | Migrate with alt flag false, empty dump | FR4 / NFR3 |
| Alt-screen dump exceeding the documented upper bound | Per the FR8 policy; overflow behavior decided in the design/plan step | FR8 |

### Error Flow

```
Writability check → condition confirmable? → no  → Reject (fail-closed)
                                           → yes → all three FR1 conditions? → no → Reject
                                                                             → yes → Accept
S_IWOTH set → Reject (evaluated unconditionally)
```

## Performance Optimization

### Performance Goals

- NFR1: the private-group decision performs exactly one `getgrgid` and one
  `getpwuid`, and never enumerates the passwd database (no `getpwent`),
  because full scans are slow/unstable under LDAP/SSSD.

### Optimization Strategies

- Bounded identity lookups instead of a passwd database scan (NFR1).
- Re-capture the alt-screen flag and dump at the existing
  `refresh_live_agent_state` point rather than adding a separate pass, so
  the tearing window matches the agent-state staleness window (FR7).

### Caching Strategy

Not applicable — no caching requirement was raised for this feature.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Security requirements are satisfied (FR1 / FR2 / NFR2)
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] AC-1: the group-write predicate accepts the owner's private per-user
      group 0o775 path and rejects extra `gr_mem` members, a non-primary
      gid, a name mismatch, any failed lookup, and any S_IWOTH path
- [ ] AC-2: on a umask 002 machine, a daemon started from the dev build
      (`src-tauri/target-host/release/emterm`) fires hot-upgrade
- [ ] AC-3: a restored pane with alt flag = true yields
      `shadow_parser.screen().alternate_screen() == true` after
      `from_restored`, and the next reattach's `build_snapshot_bytes` takes
      the alt branch
- [ ] AC-4: V2 documents migrate to V3 with alt = false / empty dump and
      restore exactly as before; version range 1..=3 is advertised
- [ ] AC-5: with an alt-screen TUI open (Claude Code / glances), a
      hot-upgrade leaves the screen uncorrupted after reattach, with no
      pre-alt-screen shell fragments showing through

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

No requirement carries `status: tbd`; FR1–FR8 and NFR1–NFR4 are all
resolved. Items deliberately deferred to the design/plan step (the concrete
alt-screen dump size limit and its overflow behavior, FR8) and the open
feasibility of an alt-screen scenario in the
`src-tauri/tests/mux_hot_upgrade.rs` harness are recorded in
`REQUIREMENTS.md` 14.2.

## Implementation Phases (if applicable)

Implementation order is (1) the NFR3 fix then (2) the alt-screen handoff —
a process constraint enabling dev-build verification of (2), not a runtime
requirement.

### Phase 1: NFR3 group-write fix
**Goals:** Allow a umask 002 dev build to fire hot-upgrade while keeping the
security posture fail-closed.
**Deliverables:**
- FR1: private per-user group exemption in the path-writability check
- FR2: unchanged world-write rejection
- NFR1 / NFR2 satisfied by the same predicate
- TS1 unit coverage; manual AC-2

### Phase 2: Alt-screen handoff
**Goals:** Carry alt-screen state across the hot-upgrade so a restored pane
reports `alternate_screen() == true`.
**Deliverables:**
- FR3 / FR4: `HandoffPane` alt state, schema v3, 1..=3, V2→V3 migration
- FR5 / FR6 / FR7: capture, restore replay, and re-capture
- FR8: documented dump size upper-bound policy
- TS2 / TS3 / TS4 coverage; manual AC-5

## References

- Requirements document: `feature-docs/mux-hot-upgrade-alt-screen/REQUIREMENTS.md`
- `src-tauri/src/mux/identity.rs` (`:455`): NFR3 path-writability check
- `crates/mux_ipc/src/handoff.rs`: `HandoffPane`, `HANDOFF_SCHEMA_VERSION`,
  `SUPPORTED_HANDOFF_SCHEMA_VERSIONS`
- `src-tauri/src/mux/upgrade.rs` (`:384` / `:547` / `:687`):
  `refresh_live_agent_state` / `snapshot_pane` / `restore_pane`
- `src-tauri/src/mux/session/pane.rs` (`:1073` / `:1229` / `:1971` /
  `:1282`): `build_snapshot_bytes` main/alt split, `MuxPane::from_restored`,
  `contents_formatted()`
- `src-tauri/tests/mux_hot_upgrade.rs`: hot-upgrade integration tests (run
  with `--test-threads=1`)
- Deferred high finding `sid-nfr3-group-write-blocks-dev-builds`: the defect
  corrected by FR1
