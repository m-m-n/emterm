# Feature: mux-bridge-capacity-capture-validation

## Overview

The mux bridge stores a ClientScrollbackCapacity frame body in `last_capacity`
only when its payload is exactly 4 bytes, so a length-invalid report no longer
overwrites the last valid value. Every ClientScrollbackCapacity frame is still
forwarded to the daemon unchanged. After an upgrade-driven reconnect
(`reconnect_and_reattach`), the bridge resends the last valid capacity body.

## Objectives

- After an upgrade-driven reconnect (`reconnect_and_reattach`), the mux bridge
  resends the last valid ClientScrollbackCapacity it accepted, so the new
  daemon connection keeps the reported probe capacity instead of falling back
  to the default 10,000 lines.

## Technical Requirements

### Functional Requirements

- **FR1: Store only length-valid capacity bodies** — `capture_if_capacity`
  (`src-tauri/src/mux/bridge/mod.rs`) returns the frame body for storage in
  `last_capacity` only when `msg.msg_type` is `ClientScrollbackCapacity` AND
  `ClientScrollbackCapacityPayload::from_payload(&msg.payload).is_some()`
  (payload exactly 4 bytes). A `ClientScrollbackCapacity` message whose payload
  is not exactly 4 bytes returns `None`, so `last_capacity` keeps its previous
  value (the last valid body, or `None` if no valid report has arrived yet).
- **FR2: Forwarding unchanged** — The stdin->daemon path in `forward.rs`
  (around line 294) keeps forwarding every `ClientScrollbackCapacity` frame to
  the daemon socket byte-identical, including length-invalid ones. Only the
  storage decision changes.
- **FR3: Value is not interpreted** — The bridge checks only the payload
  length. Any 4-byte value (including 0) is stored as-is; the bridge performs
  no clamping or substitution of the value.

### Non-Functional Requirements

- **NFR1 - Scope:** The change is limited to `src-tauri/src/mux/bridge/mod.rs`
  (`capture_if_capacity` and its import of `ClientScrollbackCapacityPayload`)
  and tests in `src-tauri/src/mux/bridge/tests.rs`. `reconnect_and_reattach`,
  the daemon, and `mux_ipc` protocol are not changed.
- **NFR2 - Build compatibility:** The change compiles for default features and
  for `--no-default-features` (the bridge is part of the CLI-only build).

## Acceptance Criteria

- [ ] **AC-1:** Following the reproduction steps (valid 4-byte capacity report,
  then length-invalid capacity report, then upgrade-driven reconnect), the
  frame resent first after the reconnect handshake is the earlier valid
  capacity body, not the invalid one.
- [ ] **AC-2:** A test in `src-tauri/src/mux/bridge/tests.rs` feeds a valid
  capacity report followed by a length-invalid capacity report through the
  bridge, then runs `reconnect_and_reattach` against a stand-in daemon, and
  asserts the resent capacity frame decodes via
  `ClientScrollbackCapacityPayload::from_payload` to the valid value.
- [ ] **AC-3:** `capture_if_capacity` returns `None` for
  `ClientScrollbackCapacity` messages whose payload length is not 4 (e.g. 0,
  3, 5 bytes), and still returns `Some(body)` for a 4-byte payload.
- [ ] **AC-4:** Both the valid and the length-invalid capacity frames are
  forwarded to the daemon socket unchanged.
- [ ] **AC-5:** Existing bridge tests
  (`capture_if_capacity_captures_capacity_and_only_capacity`,
  `forward_loop_forwards_and_captures_capacity_frame_unchanged`,
  `forward_loop_capacity_capture_replaces_and_is_unaffected_by_other_messages`,
  `reconnect_and_reattach_*` tests) continue to pass.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths are derived at create-plan from every task's
`files` entries in `workflow.yaml` (`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries
in addition to the feature-specific paths:

- `feature-docs/mux-bridge-capacity-capture-validation/**`
- `test-docs/mux-bridge-capacity-capture-validation/**`

`feature-docs/mux-bridge-capacity-capture-validation/**` covers
`REQUIREMENTS.md`, `SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`,
`phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`,
`retrospect.yaml`, and the design artifacts the design step produces. These are
generated and owned by the phase documents and by `references/phase-state.md`;
this section cites them and restates none of their rules.

`test-docs/mux-bridge-capacity-capture-validation/**` covers
`test-docs/mux-bridge-capacity-capture-validation/{T}.tests.yaml`, the per-task
test record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is
a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it. A
feature that produces no implement tasks generates no
`test-docs/mux-bridge-capacity-capture-validation/` directory at all; the
declared `test-docs/mux-bridge-capacity-capture-validation/**` entry is still
correct in that case — a declared path that never materializes is not a
violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (covers AC-3; FR1, FR3): `capture_if_capacity` with
  `ClientScrollbackCapacity` payloads of length 0, 3 and 5 returns `None`; with
  a 4-byte payload (value 0 and a non-zero value) returns `Some(frame body)`.
- [ ] **TS-3** (covers AC-1; FR1): A length-invalid capacity report with no
  prior valid report leaves `last_capacity` as `None` (unit or async).

### Integration Tests

- [ ] **TS-2** (covers AC-1, AC-2, AC-4; FR1, FR2, FR3): Unix async —
  `forward_loop` receives a valid capacity (e.g. lines 5) then a length-invalid
  `ClientScrollbackCapacity`; both frames reach the daemon side unchanged;
  `last_capacity` holds the valid body; `reconnect_and_reattach` against a
  stand-in daemon (`accept_and_handshake_blocking`) resends a capacity frame
  that decodes to 5.

### Regression and Build Checks

- [ ] **TS-4** (covers AC-5; NFR1, NFR2): Run
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::bridge`
  and the `--no-default-features` cargo check
  (`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`).

## Assumptions

- Frame forwarding of length-invalid `ClientScrollbackCapacity` to the daemon
  is preserved as-is. (reversible)
- The daemon ignores length-invalid capacity payloads and keeps the previous
  valid value; this feature does not change that behavior. (reversible)
- The in-tree sender `send_client_scrollback_capacity` always sends exactly 4
  bytes; no sender-side change is needed. (reversible)
