# Implementation Plan: mux-probe-scrollback-capacity

## Overview

A GUI tab reports its own scrollback capacity to the mux daemon through one new
additive control message, relayed by the bridge. The daemon keeps the value per
connection and runs the wrap-restore replay-state probe at `min(reported, 10,000)`
(10,000 when nothing was reported). The snapshot byte layout, every existing
message encoding and PROTOCOL_VERSION 3 stay unchanged.

## Technology Stack

- **Language**: Rust (existing workspace crates only).
- **mux_ipc** — wire protocol shared by the GUI, the bridge and the daemon; carries the new message.
- **term_core** — TerminalCore, used by the daemon probe and by the client replay; unchanged by this feature.
- **New dependencies**: none. No license entry is needed against the project license (MIT).

## Layer Structure

Report path (sender side):

1. GUI tab (gui feature) builds the message from its own core's capacity.
2. The existing mux control encoder wraps it (APC on Linux, EMUX plaintext on Windows) and writes it to the tab PTY.
3. The bridge (optionally over SSH) parses it from stdin, forwards it to the daemon socket and keeps the latest copy.

Consume path (daemon side):

1. Daemon connection loop owns the per-connection reported value.
2. The message router records reports and passes the resolved probe capacity to the attach, on-demand snapshot and visibility handlers.
3. Handlers pass it to the four wrap-aware assembly sites.
4. The sites pass it to the snapshot builders (`crate::mux::snapshot_bytes`), which pass it to the dump-block probe, which builds the scratch TerminalCore at that capacity.

Dependency rules both sides must keep:

- mux_ipc depends on nothing in `src-tauri`.
- `crate::mux::snapshot_bytes` and its `dump_block` child stay leaves: no import of `mux::ipc` or `mux::session`. The probe capacity reaches them only as a plain value argument.
- Protocol, bridge and daemon code compile in the CLI-only build (`--no-default-features`) and import no gui-only crate. GUI changes stay inside the gui-gated `tabs` module.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Client-capacity control message: `MessageType::ClientScrollbackCapacity` in `crates/mux_ipc/src/protocol.rs` | Carries the reporting tab's scrollback capacity from client to daemon | Discriminant 0x27 (the next value after `Upgrading` = 0x26). Direction client to daemon only. pane_id is 0. Payload is the capacity payload below. The daemon never replies. Every existing discriminant, payload encoding and PROTOCOL_VERSION (3) are unchanged; the first unassigned byte becomes 0x28. Placement: the enum variant sits immediately after `Upgrading`; the `from_u8` arm for 0x27 sits immediately after the 0x26 arm. | task0001 (owner: defines and tests it, GUI sends it, bridge relays and re-sends it); task0002 (daemon consumes it) |
| Capacity payload: `ClientScrollbackCapacityPayload` (one u32 field named `lines`) in `crates/mux_ipc/src/protocol.rs` | Encodes and decodes the fixed-size payload | `from_payload(bytes)`: precondition none (any byte slice); postcondition returns a payload only when the slice is exactly 4 bytes, value read little-endian, otherwise returns nothing; never panics and never allocates in proportion to the input. `to_payload()`: postcondition exactly 4 bytes, little-endian. Raw bytes, not bincode, in the same style as `SetVisibilityPayload`. Placement: immediately after the `SetVisibilityPayload` impl block. | task0001 (owner; the GUI encodes with it); task0002 (the daemon decodes with it) |

Value semantics of the message (both tasks): the value is the reporting tab's own
TerminalCore scrollback capacity, sent as-is. 0 is a legal value and values above
10,000 are legal values. The sender never clamps or substitutes; the daemon owns
resolution (D1).

## Conventions

- **Logging (NFR4)**: a new diagnostic that must reach release logs uses warn or higher. Diagnostics about a received frame log its type and length, never its payload bytes.
- **Feature gates and platforms (NFR3)**: Linux and Windows. Protocol, bridge and daemon edits compile in the CLI-only build. `cfg(windows)` / `cfg(unix)` code touched by this feature receives only the edits it needs to compile against changed signatures.
- **Formatting (NFR5)**: format only the files a task touches. Never run a crate-wide `cargo fmt`. The check-only format command may report pre-existing drift in untouched files; that drift is not this feature's to fix.
- **Test-only observers** stay under `cfg(test)`, so production builds carry none.
- **Snapshot bytes (FR7)**: no task adds a marker, field or reordering to the snapshot payload.

## Cross-task Design Decisions

### D1: The sender reports the raw capacity; the daemon owns the cap

The GUI sends its core's capacity unchanged. The bridge never interprets the value.
Resolution (`min(reported, 10,000)`, and 10,000 when unreported) happens only in the
daemon. The daemon therefore has a single source of truth for the cap and for the
above-cap behaviour (FR6), and a future cap change touches only the daemon.
Affected tasks: task0001, task0002.

### D2: "No report" and "reported 0" stay distinct end to end

The GUI always sends its actual value, including 0. The daemon keeps "unreported"
as a state separate from any reported value. Unreported resolves to the legacy
10,000 (old GUIs, old bridges, system-origin paths). An explicit 0 resolves to 0
(FR4, A-2). Affected tasks: task0001, task0002.

### D3: One protocol definition across parallel worktrees

task0001 owns the protocol definition and its tests. task0002 cannot compile its
router arm without the variant and the payload decoder. If task0002's worktree
lacks them, task0002 adds exactly the Shared Components definition: same names,
same discriminant, same payload rule, same placement. In
`crates/mux_ipc/src/protocol/tests.rs` it then moves the four existing assertions
that pin 0x27 as the first unmapped byte to 0x28. That is the same edit task0001
makes. task0002 adds no other protocol test. On a merge conflict in either protocol
file, the implementer adopts the parent side, because both versions satisfy the
same contract. task0001's Acceptance Criteria are the authority on the definition.
Affected tasks: task0001, task0002.

### D4: Compatibility stance

The feature makes no PROTOCOL_VERSION bump, changes no existing payload and adds no
response. Mixed versions behave as follows:

- An old daemon drops 0x27 through its existing unknown-type path.
- An old GUI or old bridge never sends the message, so the daemon uses the legacy 10,000.
- An old bridge that receives the message from a new GUI cannot decode it and drops it.

In every mixed case the connection keeps working without the fix, which is the
documented known limit FR8 (b). Affected tasks: task0001, task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Merge conflict in the protocol files between the two parallel tasks | High | Low | D3 pins the names, discriminant, payload rule and placement; the implementer adopts the parent side |
| An older remote bridge paired with a new GUI rejects the unknown frame and prints a one-line decode error on its stderr (the tab PTY) once per attach | Medium (SSH setups with a stale remote binary) | Low | Covered by the mixed-version known limit (FR8 (b)); the connection continues at the legacy 10,000 |
| Windows-only code paths (the bridge's Windows main loop, EMUX plaintext sending) are not compiled by any verification command available in workflow.yaml | Medium | Medium | Keep `cfg(windows)` edits limited to signature adaptation; the review phase checks them |
| Snapshot bytes drift at probe capacity 10,000, reopening the apt progress-bar residue issue | Low | High | task0002 pins byte identity against output captured before the change |

## Open Questions

- [ ] In the mixed-version case (old bridge, new GUI), it is unconfirmed whether the old bridge's decode-error line becomes visible in the tab while it is in mux mode.
- [ ] The Windows cross-build is not among the verification commands in workflow.yaml, so Windows compilation of the touched code is not verified automatically.
