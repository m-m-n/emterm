# Feature: mux-probe-scrollback-capacity

## Overview

The daemon's wrap-restore replay-state probe builds its scratch terminal at a
fixed 10,000-line scrollback capacity, while the GUI tab replays the snapshot
at its own capacity. This feature passes the replaying tab's own capacity to
the daemon through a new additive control message and runs the probe at
`min(reported, 10_000)`, without changing the snapshot byte layout, any
existing message encoding, or PROTOCOL_VERSION (3).

Requirements document: `feature-docs/mux-probe-scrollback-capacity/REQUIREMENTS.md`.

## Objectives

- After a snapshot restore of a wrapped-ring mux pane (tab switch / reattach /
  visibility resume / on-demand snapshot), continued output lands on the
  correct row whatever the client's `scrollback_lines` setting is, including
  small values and 0.
- The daemon's replay-state probe uses the same scrollback capacity as the GUI
  tab that will replay the snapshot, so a grow-resize inside the payload moves
  the cursor the same way on both sides.
- The snapshot byte layout, every existing message encoding, and
  PROTOCOL_VERSION (3) stay unchanged. Mixed old/new GUI, bridge, and daemon
  combinations keep working.

## User Stories

### US1: Restore with a small scrollback setting
As a mux user with `scrollback_lines` set to 0 or another small value, I want
continued output after a snapshot restore of a wrapped-ring pane to land on
the correct row, so that restored panes behave the same as a replay at my
tab's own capacity.

**Acceptance Criteria:**
- [ ] AC-1: With scrollback_lines 0 or another small value, a wrapped-ring
  pane with a grow-resize inside the payload restores through tab switch,
  reattach, or visibility resume, and continued output lands on the same row
  as a replay at the client's own capacity (FR2, FR5, FR9).
- [ ] AC-2: The new MessageType round-trips through MuxMessage frame,
  APC/OSC, and EMUX plaintext encodings. Every existing MessageType
  discriminant and PROTOCOL_VERSION == 3 stay unchanged (FR1, FR7).
- [ ] AC-3: On the first accepted Welcome, the GUI sends the capacity message
  before Attach, CreateWindow, and RequestPaneSnapshot, and its value equals
  the tab core's `scrollback_capacity()` (FR2).
- [ ] AC-4: The bridge forwards the capacity message. On the Unix
  upgrade-announced reconnect it sends the stored capacity before the re-sent
  Attach (FR3).
- [ ] AC-5: The daemon's probe capacity resolves as follows: unreported ->
  10,000; reported 0 -> 0; reported 50 -> 50; reported 10,000 -> 10,000;
  reported 10,001 or u32::MAX -> 10,000 with the dump block still present;
  malformed payload -> value unchanged and connection still open. The value is
  per connection: two connections with different reports each get their own
  capacity (FR4, FR5, FR6).
- [ ] AC-6: All four assembly sites use the requesting connection's probe
  capacity, and so does a visibility resume that goes through the
  deferred-output queue (FR5).
- [ ] AC-7: With probe capacity 10,000, `build_snapshot_bytes_for_ring` and
  `build_resume_snapshot_bytes_for_ring` produce byte-identical output to the
  current implementation for the same inputs. All existing
  `wrap_restore_tests`, `dump_block` tests, and apt-progress-bar-related
  snapshot tests pass unchanged apart from the added capacity argument (FR7).
- [ ] AC-8: An old daemon receiving the new message drops it and keeps the
  connection working. An existing codec unknown-type test pattern covers this
  (NFR1).
- [ ] AC-9: The known limits (above 10,000, and mixed versions) are
  documented in code next to the cap and in SPEC (FR8).
- [ ] AC-10: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
  src-tauri/Cargo.toml --lib -- --test-threads=1`, the mux_ipc crate tests,
  and the `--no-default-features` cargo check all pass (NFR3).

## Technical Requirements

### Functional Requirements
- **FR1: Additive client-capacity control message** — Add one new
  client-to-daemon MessageType (the next unused discriminant after
  `Upgrading` = 0x26) that carries the reporting tab's scrollback capacity as
  a fixed-size u32 payload, with pane_id 0. Existing MessageType values and
  payload encodings do not change, and PROTOCOL_VERSION stays 3. The daemon
  sends no response. An old daemon drops the unknown type through the
  existing unknown-frame path (`MessageType::from_u8` -> `None`) and keeps the
  connection open.
- **FR2: GUI reports the tab's own capacity** — The GUI tab sends the new
  message with that tab's own `core.scrollback_capacity()`, not the latest
  global `scrollback_lines` setting. It goes out over the existing
  `send_control` path (tab PTY -> optional SSH -> bridge -> daemon; APC/OSC on
  Linux, `EMUX;<base64>` plaintext on Windows). The tab sends it on the first
  accepted Welcome of an attach, before any Attach, CreateWindow, or
  RequestPaneSnapshot that the tab sends in response to that Welcome.
- **FR3: Bridge forwards and re-sends the capacity** — The bridge decodes and
  forwards the new message to the daemon. It also stores the last value
  received, the same way it keeps `last_attach`. On the Unix auto-reconnect
  after an upgrade announcement, it sends the stored capacity before the
  re-sent Attach. Windows never reconnects, and that stays unchanged.
- **FR4: Per-connection capacity in the daemon** — The daemon stores the
  reported capacity once per connection, like the existing per-connection
  `visible_state`. It is not stored per session, not persisted on the pane,
  and not carried through a hot-upgrade handoff. The daemon validates the
  payload as a fixed-size u32 before allocating anything. A malformed payload
  is ignored: it leaves the connection's current value unchanged and does not
  close the connection. A connection with no report uses the legacy 10,000
  lines. An explicit report of 0 means 0 and is kept separate from "no
  report". This covers old GUIs, old bridges, and system-origin paths.
- **FR5: Probe capacity reaches all four snapshot assembly sites** — The probe
  capacity is `min(reported, 10_000)`, or 10,000 when nothing was reported. It
  is passed down to the probe's scratch TerminalCore (`dump_block.rs`
  `probe_replay_state`) through all four wrap-aware assembly sites:
  (1) the visible reattach, `collect_reattach_data` ->
  `build_snapshot_bytes_for_ring` (`reattach.rs:273`); (2) the on-demand
  RequestPaneSnapshot, `build_shadow_parser_snapshot_for_ring`
  (`reattach.rs:103`, called from `handlers/mod.rs:529`); (3) the visibility
  resume, `resume_pane_with_permit` -> `build_resume_snapshot_bytes_for_ring`
  (`output_target.rs:432`), including resumes deferred through the
  connection's `DeferredOutputQueue`; (4) the `evaluate_output_target` resume
  branch (`output_target.rs:270`). In every case the value used belongs to the
  connection that the snapshot is sent to.
- **FR6: Cap above 10,000 keeps the dump** — When the reported capacity is
  above 10,000, the probe runs at 10,000 and the wrap-restore dump block is
  still appended, which is today's behaviour. The dump is never dropped just
  because the cap was exceeded, and the GUI tab's own capacity is never
  changed.
- **FR7: Snapshot byte shape unchanged** — The snapshot payload layout
  (EMSNAP2 envelope, segments, the pre-fix delegated payload, and the
  dump-block composition order) stays as it is. No marker or new field is
  added to the snapshot. For a probe capacity of 10,000, including the
  unreported case, the output is byte-identical to the current output for the
  same inputs.
- **FR8: Known limits documented** — Two limits are documented next to the cap
  constant in code and in this feature's SPEC (see "Known Limits" below):
  (a) for client capacities above 10,000, a residual cursor-row mismatch after
  a grow-resize remains; (b) in mixed old/new GUI, bridge, or daemon
  combinations the connection keeps working but the capacity fix is not
  guaranteed and the legacy 10,000 applies. The cap limits load amplification
  only. It does not change lock-hold time (follow-up `3b5bbd839c74d67b`).
- **FR9: Regression test for small capacities with a grow-resize** — Add
  builder-level tests that combine a wrapped ring, a grow-resize segment in
  the payload (rows-only, rows+cols, and `current_dims` larger than the last
  segment), and a client capacity C in {0, a small non-zero value}. The
  snapshot is built with probe capacity C and replayed on a client
  TerminalCore built at capacity C. The tests assert that the cursor row/col,
  and the row where continued output is written, equal an oracle core at
  capacity C that replays the delegated pre-dump payload and then the same
  continued output. Run with a fixed 10,000-line probe, at least one case
  fails (red); after the fix, all pass.

### Non-Functional Requirements
- **NFR1 - Protocol compatibility:** No PROTOCOL_VERSION bump and no change to
  existing encodings. Old and new peers never stall, because no response is
  required. A new GUI with an old daemon, or an old GUI with a new daemon,
  keeps working at the legacy 10,000.
- **NFR2 - Bounded probe cost:** A capacity reported by a client is untrusted.
  Probe scratch memory and time are bounded by the 10,000-line cap, and
  validation happens before any allocation.
- **NFR3 - Platforms and feature gates:** Linux and Windows. The daemon,
  bridge, and protocol changes compile in the CLI-only build
  (`--no-default-features`). GUI-only code stays under the `gui` feature.
- **NFR4 - Diagnostics:** Any new diagnostic that needs to appear in release
  logs uses warn level or higher, because release builds drop debug and info.
- **NFR5 - Formatting scope:** Do not run cargo fmt on the whole crate. Format
  only the files this feature touches.

## Implementation Approach

### Architecture

**System Architecture:**
```
┌─────────────────────────────────────┐
│ GUI tab (core.scrollback_capacity())│  FR2: send capacity on first Welcome
├─────────────────────────────────────┤
│ send_control -> tab PTY -> (SSH)    │  APC/OSC (Linux), EMUX;<base64> (Windows)
├─────────────────────────────────────┤
│ bridge (stores last capacity)       │  FR3: forward; re-send before Attach on reconnect
├─────────────────────────────────────┤
│ daemon connection (per-connection   │  FR4: validate u32, unreported vs explicit 0
│   reported capacity)                │
├─────────────────────────────────────┤
│ 4 snapshot assembly sites           │  FR5: probe_capacity = min(reported, 10_000)
├─────────────────────────────────────┤
│ probe scratch TerminalCore          │  dump_block.rs probe_replay_state
└─────────────────────────────────────┘
```

**Component Diagram:**
```
GUI tab ──capacity msg──> bridge ──capacity msg──> daemon connection state
                                                      │
          ┌───────────────────────┬───────────────────┼──────────────────────────┐
          v                       v                   v                          v
 collect_reattach_data   build_shadow_parser_   resume_pane_with_permit   evaluate_output_target
 -> build_snapshot_      snapshot_for_ring      -> build_resume_snapshot_ resume branch
    bytes_for_ring       (reattach.rs:103)         bytes_for_ring         (output_target.rs:270)
 (reattach.rs:273)                                 (output_target.rs:432,
                                                    incl. DeferredOutputQueue)
          └───────────────────────┴───────────────────┴──────────────────────────┘
                                       │ probe capacity
                                       v
                          probe_replay_state (dump_block.rs)
```

### Data Flow

```
First accepted Welcome
  GUI tab -> [capacity = core.scrollback_capacity()] -> Attach -> (CreateWindow / RequestPaneSnapshot)
  bridge  -> forward capacity, store as last value
  daemon  -> validate fixed-size u32, store per connection

Unix upgrade-announced auto-reconnect
  bridge  -> stored capacity -> re-sent Attach

Snapshot assembly (any of the four sites)
  daemon  -> probe_capacity = reported ? min(reported, 10_000) : 10_000
          -> probe scratch TerminalCore at probe_capacity
          -> snapshot bytes (layout unchanged) -> the requesting connection
```

### API Design

#### Message: client-capacity control message (new MessageType)

**Request (client -> daemon):**
```
MessageType: next unused discriminant after Upgrading (0x26)
pane_id:     0
payload:     fixed-size u32 — the reporting tab's scrollback capacity
Transport:   existing send_control path; APC/OSC on Linux, EMUX;<base64> plaintext on Windows
```

**Response:** none. The daemon sends no response.

**Daemon handling:**
| Input | Result |
|---|---|
| No report on the connection | probe capacity 10,000 |
| Reported 0 | probe capacity 0 |
| Reported 50 | probe capacity 50 |
| Reported 10,000 | probe capacity 10,000 |
| Reported 10,001 or u32::MAX | probe capacity 10,000, dump block still present |
| Malformed payload (not exactly one u32) | ignored; connection value unchanged; connection stays open |

**Old daemon:** decodes the unknown type to `None` through
`MessageType::from_u8` and drops the frame while keeping the connection open.

### Database Schema

Not applicable. No persisted data. The daemon keeps the reported capacity as
per-connection in-memory state; it is not persisted on the pane and not
carried through a hot-upgrade handoff (FR4).

### Dependencies

**Internal Dependencies:**
- mux_ipc protocol (MessageType, MuxMessage frame, APC/OSC, EMUX plaintext):
  carries the new message (FR1).
- GUI tabs (mux_link, `send_control`): sends the tab's capacity (FR2).
- bridge: forwards, stores, and re-sends the capacity (FR3).
- daemon connection state, `reattach.rs`, `output_target.rs`,
  `handlers/mod.rs`: store the value and pass it to the assembly sites (FR4,
  FR5).
- `dump_block.rs` `probe_replay_state`: builds the scratch TerminalCore at the
  probe capacity (FR5).
- term_core `TerminalCore`: `scrollback_capacity()` (FR2).

**External Dependencies:**
- None.

### File Structure

Touch points named in the requirements:

```
dump_block.rs                 # probe_replay_state, cap constant, known-limit comment (FR5, FR6, FR8)
wrap_restore_tests.rs         # regression tests (FR9)
reattach.rs                   # sites (1) :273 and (2) :103 (FR5)
handlers/mod.rs               # RequestPaneSnapshot caller :529 (FR5)
output_target.rs              # sites (3) :432 and (4) :270 (FR5)
src-tauri/src/tabs/replay.rs  # client replay core construction (reference for FR2)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-probe-scrollback-capacity/**`
- `test-docs/mux-probe-scrollback-capacity/**`

`feature-docs/mux-probe-scrollback-capacity/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-probe-scrollback-capacity/**` covers
`test-docs/mux-probe-scrollback-capacity/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites
it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mux-probe-scrollback-capacity/` directory at all; the declared
`test-docs/mux-probe-scrollback-capacity/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (covers FR9, AC-1): `wrap_restore_tests.rs` / `dump_block.rs`:
  wrapped ring (small ring capacity), payload with a rows-only grow segment,
  then with a rows+cols grow segment, then with `current_dims` larger than the
  last segment. For C in {0, a small non-zero value}: build with probe
  capacity C, replay on a client `TerminalCore::new(.., C)` via
  `reset_and_replay_segments`, and also via `build_from_snapshot` to mirror
  the off-thread client path. Feed continued output such as
  `\r\nCONTINUED`. Assert the cursor row/col and the row holding the continued
  text equal an oracle core at capacity C that replays the delegated pre-dump
  payload plus the same continuation. Confirm red with a fixed 10,000 probe.
- [ ] TS-2 (covers FR5, FR6, AC-5): Probe-capacity resolution: unreported, 0,
  small, 10,000, 10,001, and u32::MAX map to 10,000, 0, small, 10,000, 10,000,
  and 10,000. Above the cap the dump block is still appended.
- [ ] TS-3 (covers FR7, AC-7): Byte identity: at probe capacity 10,000 the
  output equals the current implementation's output for the existing
  fixtures. Existing `dump_block` probe-equality tests keep passing with
  capacity passed in explicitly.
- [ ] TS-4 (covers FR1, AC-2, AC-8): mux_ipc protocol: the new type decodes
  through `from_u8`, `from_frame_body`, `from_apc`, and the plaintext path,
  with the u32 payload round-tripping. Discriminants and PROTOCOL_VERSION stay
  unchanged. Codec: an unknown-type frame is still dropped without closing the
  stream.
- [ ] TS-5 (covers FR2, AC-3): tabs (mux_link) test: on the first accepted
  Welcome, the sent control frames are ordered capacity, then Attach, then
  (when applicable) CreateWindow and RequestPaneSnapshot, and the capacity
  value equals the tab core's `scrollback_capacity()` for a non-default value
  such as 0.
- [ ] TS-6 (covers FR3, AC-4): bridge test: the capacity message is captured
  the way `capture_if_attach` captures Attach, and the reconnect path writes
  capacity before the stored Attach.
- [ ] TS-7 (covers FR4, FR5, AC-5, AC-6): daemon connection / reattach /
  output_target tests: the per-connection value reaches
  `collect_reattach_data`, `handle_request_pane_snapshot`,
  `resume_pane_with_permit` (direct and deferred), and the
  `evaluate_output_target` resume branch. A malformed payload is ignored. Two
  connections keep independent values.

### Integration Tests
- [ ] Covered by the unit-level scenarios above (TS-1 to TS-7).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] TS-8 (manual, covers AC-1): Reproduce the ticket steps on a release
  build with `scrollback_lines = 0` and with a small value: fill a pane past a
  ring wrap, grow the window mid-output, switch tab or reattach, then continue
  output. The output lands on the expected row.

### Edge Cases
- [ ] Reported capacity 0 is kept separate from "no report" (FR4, TS-2).
- [ ] Reported capacity above 10,000 (10,001, u32::MAX) probes at 10,000 and
  keeps the dump block (FR6, TS-2).
- [ ] Malformed capacity payload is ignored without closing the connection
  (FR4, TS-7).
- [ ] Visibility resume deferred through the `DeferredOutputQueue` uses the
  requesting connection's capacity (FR5, TS-7).

### Performance Tests
- [ ] Probe scratch memory and time are bounded by the 10,000-line cap (NFR2,
  TS-2).

## Security Considerations

- **Authentication:** Not applicable.
- **Authorization:** Not applicable.
- **Input Validation:** A client-reported capacity is untrusted. The daemon
  validates the payload as a fixed-size u32 before any allocation, ignores a
  malformed payload without closing the connection, and clamps the probe
  capacity at 10,000 (FR4, NFR2).
- **Data Protection:** Not applicable.
- **XSS Prevention:** Not applicable.
- **SQL Injection Prevention:** Not applicable.
- **CSRF Protection:** Not applicable.

## Error Handling

### Error Codes

Not applicable. The new message has no response and no error code.

| Condition | Handling |
|---|---|
| Malformed capacity payload | Ignored; the connection's current value is unchanged and the connection stays open (FR4) |
| No capacity reported | Legacy 10,000 lines (FR4) |
| Old daemon receives the new type | Dropped through the unknown-frame path; connection stays open (FR1, NFR1) |
| Reported capacity above 10,000 | Probe at 10,000, dump block kept (FR6) |

### Error Flow

```
Capacity frame -> validate fixed-size u32
  valid   -> store per connection
  invalid -> ignore (value unchanged, connection open)
```

## Performance Optimization

### Performance Goals
- Probe scratch memory and time are bounded by the 10,000-line cap (NFR2).

### Optimization Strategies
- Cap: `probe_capacity = min(reported, 10_000)`. The cap limits load
  amplification only; it does not change lock-hold time (follow-up
  `3b5bbd839c74d67b`).

### Caching Strategy
- Not applicable.

## Known Limits

- **Client capacities above 10,000:** the probe runs at 10,000, so a residual
  cursor-row mismatch after a grow-resize remains for these capacities. The
  fix is guaranteed for capacities 0..=10,000 (FR8 (a)).
- **Mixed old/new versions:** in mixed old/new GUI, bridge, or daemon
  combinations the connection keeps working, but the capacity fix is not
  guaranteed and the legacy 10,000 applies (FR8 (b)).
- **Lock-hold time:** the cap limits load amplification only and does not
  change lock-hold time (follow-up `3b5bbd839c74d67b`).

## Assumptions

- **A-1:** Fix approach pass_client_capacity: the replaying tab's own capacity
  is sent to the daemon through a new additive control message, and the probe
  runs at that capacity. The alternative approaches were not chosen:
  client-side restore would need a snapshot-shape change the ticket forbids,
  and a documented limit only cannot meet DoD item 1.
- **A-2:** Protocol compatibility avoid_bump_preferred: only a new message
  type is added, and PROTOCOL_VERSION stays 3. Unreported maps to the legacy
  10,000, kept separate from an explicit 0. Mixed old/new combinations keep
  working without a guaranteed fix. No response is required from the peer.
- **A-3:** Large capacities cap_with_fallback, refined: the u32 is validated
  before allocation, `probe_capacity = min(reported, 10,000)`, and above the
  cap the probe runs at 10,000 with the dump kept. The fix is guaranteed for
  0..=10,000. Above 10,000 the residual mismatch is a documented limit, and
  lock-hold time is left to follow-up `3b5bbd839c74d67b`.
- **A-4:** The design step is skipped: there is no UI or visual change.
- **A-5:** A tab's scrollback capacity is fixed when its TerminalCore is
  constructed. TerminalCore has `scrollback_capacity()` and no setter, and the
  off-thread replay builds its replacement core at the same capacity. So one
  report per attach, plus a re-send on bridge reconnect, is enough, and a
  later settings change affects only new tabs.
- **A-6:** A malformed capacity payload (not exactly one u32) is ignored
  without changing the connection's value and without closing the connection.
  This follows from the answered "validated before any allocation" rule.
- **A-7:** Constraint from the ticket: the snapshot byte shape is not changed,
  because of the known apt progress-bar residue issue.
- **A-8:** Out of scope: the probe's scratch core always uses the default
  `scroll_region_scrollback_enabled = true`, while the client seeds that flag
  from settings (`settings/mod.rs:148`). That is a separate possible
  probe/client divergence and is not addressed by this capacity-only fix.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals
- [ ] Security requirements are satisfied
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] AC-1 to AC-10 are met

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. Every requirement is resolved.

## Implementation Phases (if applicable)

Not applicable. The task breakdown is produced at create-plan.

## References

- Requirements: `feature-docs/mux-probe-scrollback-capacity/REQUIREMENTS.md`
- Probe fixed capacity: `dump_block.rs:109`
- Client replay core construction: `src-tauri/src/tabs/replay.rs:303-325`
- `TerminalCore::scrollback_capacity()`: `crates/term_core/src/terminal_core.rs:402`
- `scroll_region_scrollback_enabled` seeding: `settings/mod.rs:142-148`
