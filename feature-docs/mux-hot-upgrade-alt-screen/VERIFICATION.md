# Verification Document: mux-hot-upgrade-alt-screen

## Overview

**Feature**: mux-hot-upgrade-alt-screen
**SPEC.md**: `feature-docs/mux-hot-upgrade-alt-screen/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-hot-upgrade-alt-screen/IMPLEMENTATION.md`

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (mux_ipc): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/mux_ipc/Cargo.toml`
- Command (CLI feature gate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for all three.

## Test Verification

- Command (main, unit): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (mux_ipc, unit): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/mux_ipc/Cargo.toml --lib`
- Command (integration): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test mux_hot_upgrade -- --test-threads=1`
- Coverage target: no coverage tooling is configured in this project; the
  criterion is scenario completeness — every automatable TS row below has
  its tests present and passing (TS5 is manual).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Private-group predicate: every FR1/FR2 branch in `src-tauri/src/mux/identity.rs` | The three-condition accept case passes for both candidate and parent roles; extra group member, non-primary gid, name mismatch, failed lookup, and world-write are each refused; pre-existing rows unchanged | Unit |
| TS2 | Handoff V2-to-V3 migration and widened version range in `crates/mux_ipc/src/handoff.rs` | V2 (and V1 via the chain) documents decode with alt flag false + empty dump, other fields preserved; version-3 documents round-trip alt state byte-for-byte; supported range is 1..=3 | Unit |
| TS3 | `snapshot_pane` / `refresh_live_agent_state` / `restore_pane` / `from_restored` alt round-trip in `src-tauri/src/mux/{upgrade.rs,session/pane.rs}` | An alt pane round-trips to an active alternate screen and the reattach snapshot takes the alt branch; main-buffer and exited panes are unchanged; the refresh pass re-captures buffer switches in both directions; the D1 size cap replaces an over-cap dump with an empty one (flag preserved, warn log) | Unit |
| TS4 | Integration: alt-screen pane across a real daemon hot-upgrade (`src-tauri/tests/mux_hot_upgrade.rs`, `--test-threads=1`) | After upgrade + reconnect + reattach, the snapshot's visible screen reflects the alternate screen and the pre-alt marker does not surface; pre-existing scenarios stay green. Fallback if infeasible: documented per IMPLEMENTATION.md D2 | Integration |
| TS5 | Manual: MT-1 (AC-2) and MT-2 (AC-5) below | See Manual Testing | Manual |

## Code Quality Verification

- Format: no format command is configured for this project
  (workflow.yaml `format_command` is empty; rustfmt is deliberately
  non-enforced).
- Static analysis: none configured beyond the build-verification
  `cargo check` commands above.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The group-write predicate accepts the owner's private per-user-group 0o775 path and rejects extra members, non-primary gid, name mismatch, failed lookup, and any world-writable path | TS1 unit tests |
| AC-2 | On a umask 002 machine, a daemon started from the dev build (`src-tauri/target-host/release/emterm`) fires hot-upgrade | Manual MT-1 |
| AC-3 | A restored pane with alt flag true reports an active alternate screen after `from_restored`, and the next reattach's snapshot takes the alt branch | TS3 unit tests; TS4 integration |
| AC-4 | V2 documents migrate to V3 with alt = false / empty dump and restore exactly as before; version range 1..=3 is advertised | TS2 unit tests |
| AC-5 | With an alt-screen TUI open, a hot-upgrade leaves the screen uncorrupted after reattach, with no pre-alt-screen shell fragments showing through | Manual MT-2; TS4 integration (automated analogue) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 (unit); TS5/MT-1 (manual) |
| FR2 | task0001 | TS1 |
| FR3 | task0002 | TS2, TS3 |
| FR4 | task0002 | TS2 |
| FR5 | task0002, task0003 | TS3, TS4 |
| FR6 | task0002, task0003 | TS3, TS4, TS5/MT-2 |
| FR7 | task0002, task0003 | TS3, TS4, TS5/MT-2 |
| FR8 | task0002 | TS3 (over-cap and at-cap cases) |
| NFR1 | task0001 | TS1 (pure-predicate/capture split makes the one-lookup-each bound structurally reviewable) |
| NFR2 | task0001 | TS1 (fail-closed rows) |
| NFR3 | task0002 | TS2 (V1 and V2 decode identical to today) |
| NFR4 | task0002 | TS3 + code review (no new dependency; existing main/alt split and migration pattern reused — cross-checked against IMPLEMENTATION.md's "New dependencies: none") |

## E2E Testing

The project has no separate E2E framework; the integration harness is the
E2E surface for this feature.

- [ ] TS4: alt-screen hot-upgrade scenario via
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test mux_hot_upgrade -- --test-threads=1`

## Manual Testing (E2E Not Possible)

Perform MT-1 BEFORE MT-2 (IMPLEMENTATION.md D3: a dev build cannot fire
hot-upgrade until the FR1 fix is active, so MT-2 is not exercisable on a
dev build until MT-1 holds).

- [ ] MT-1 (AC-2): On a umask 002 machine, start the mux daemon from the
      dev build (`src-tauri/target-host/release/emterm`), then update the
      binary so binary-update detection triggers. Confirm the hot-upgrade
      fires: upgrade log lines appear in `emterm.log`, and existing panes
      survive the upgrade.
- [ ] MT-2 (AC-5): With an alt-screen TUI (Claude Code / glances) running
      in a mux pane, trigger a hot-upgrade, then reattach. The TUI's
      screen is intact and no pre-alt-screen shell fragments show through.

(The design step was skipped for this feature — no mockup comparison item
applies.)

## Performance / Security Verification

- NFR1 (performance): the private-group decision performs one group
  lookup and one user lookup, never a passwd-database enumeration —
  covered by TS1's structural split (pure predicate + thin capture) and
  code review.
- NFR2 (security): every unverifiable condition or failed lookup refuses
  the path (fail-closed) — TS1 rows.
- FR2 (security): world-writable paths refused unconditionally — TS1 row.
- FR8 (robustness): an alt-screen dump over the IMPLEMENTATION.md D1 cap
  is stored empty with the flag preserved and a warn-level log — TS3 row.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 check commands | 3 | - | - |
| Unit tests | TS1, TS2, TS3 | 3 | - | - |
| Integration | TS4 | - | 1 | - |
| Manual | TS5 (MT-1, MT-2) | - | - | 2 |
