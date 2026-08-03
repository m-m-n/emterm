# Verification Document: mux-daemon-binary-update-detect

## Overview

**Feature**: mux-daemon-binary-update-detect
**SPEC.md**: `feature-docs/mux-daemon-binary-update-detect/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-daemon-binary-update-detect/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors
- Additional (platform gating, NFR2 — see TS-10):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0 (the Unix-gated identity/trigger code does not
  break the CLI-only build)

## Test Verification

- Command (unit): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (integration / E2E): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test mux_hot_upgrade -- --test-threads=1`
- Coverage target: every TS row below has at least one passing test;
  no numeric line-coverage floor is imposed (project convention).

### Test Scenarios from SPEC.md

TS-1 through TS-8 are SPEC.md's scenarios verbatim; TS-9 and TS-10 are
verification-plan additions closing FR6 / NFR2 traceability.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Identity comparison predicate table: (dev, ino) match / mismatch / recorded-path ENOENT / other stat error | match → no fire; mismatch → fire; ENOENT → fire; other stat error → no fire (style of `self_exec.rs`'s `is_missing` table) | Unit (task0001) |
| TS-2 | Identity file write/read round trip; missing / truncated / malformed content; permissions (0o600) and symlink refusal | Round trip preserves path + (device, inode); every degraded case yields the Undecidable fallback (AC-7); hardening holds (NFR3) | Unit (task0001) |
| TS-3 | Real daemon under isolated XDG_RUNTIME_DIR; start binary rename-replaced with a valid new copy ("(deleted)" exe state reproduced); attach | Hot-upgrade fires; shell PID unchanged and alive; daemon runs the new image (handoff-start marker; exe link no longer "(deleted)") — AC-1/2/3/6 main line | Integration (task0003) |
| TS-4 | Attach without replacing the binary | No upgrade fires: no Upgrading broadcast, no handoff-start log — AC-4 | Integration (task0003) |
| TS-5 | Binary replaced, then the `emterm mux` start-side probe (`ensure_daemon_running` path, driven via `mux script`) | Fires the same way as TS-3; notice line emitted | Integration (task0003) |
| TS-6 | Stand-in daemon that silently discards Upgrade frames (`spawn_fake_legacy_daemon` style) | Pinned pane-destruction warning emitted to the user, then the existing shutdown→respawn fallback runs — AC-5 | Unit (task0002) |
| TS-7 | Attach to a daemon with no identity file (pre-feature generation simulated), binary replaced | No misfire; previous protocol-judgement-only behavior — AC-7 | Integration (task0003) |
| TS-8 | Trigger fires | Exactly one pinned replacement-notice line emitted to the attaching / mux-starting client — AC-8 | Unit (task0002) + folded into TS-3 / TS-5 assertions (task0003) |
| TS-9 | Existing hot-upgrade regression suite (schema-gate abort, zero-pane upgrade, handoff logging, exec-failure behavior) | All pre-existing `mux_hot_upgrade` scenarios and existing `recover_from_legacy_daemon` unit tests still pass unmodified — FR6 safety gates remain in force | Integration + Unit (regression) |
| TS-10 | Platform gating check | GUI build and CLI-only (`--no-default-features`) build both compile; all new code Unix-gated; no Windows code path altered (verified by build + review of cfg gating) — NFR2 | Build |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: none configured beyond the compiler (project has no
  clippy gate in workflow.yaml); compiler warnings in touched files should
  not increase.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Replacement of the daemon's start binary is detected, including the "(deleted)" exe state | TS-3 (real rename replacement) + TS-1 (predicate) |
| AC-2 | On detection the hot-upgrade runs and pane shells survive with unchanged PIDs | TS-3 shell-PID assertion |
| AC-3 | Fires even when the protocol is compatible | TS-3/TS-5 run against a current-protocol daemon (the probe's Compatible arm) |
| AC-4 | Identical binary → no replacement, no Upgrading broadcast | TS-4 + TS-1 (match row) |
| AC-5 | Legacy daemon without upgrade support → visible pane-destruction warning, then existing fallback | TS-6 |
| AC-6 | Replaced daemon runs the newly installed image; exec target never resolves to a "(deleted)" path | TS-3 exe-link assertion + task0001 AC-6/AC-7 (recorded-path-only resolution) |
| AC-7 | Missing/invalid identity → no misfire, previous behavior | TS-2, TS-7, TS-1 (error rows) |
| AC-8 | One-line replacement notice shown to the client | TS-8 (unit) + TS-3/TS-5 stderr assertions |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002, task0003 | TS-1, TS-2, TS-3 |
| FR2 | task0002, task0003 | TS-3, TS-5, TS-8 |
| FR3 | task0001, task0002, task0003 | TS-1, TS-4 |
| FR4 | task0001, task0003 | TS-3 |
| FR5 | task0002 | TS-6 |
| FR6 | task0001, task0002 | TS-3, TS-9 |
| FR7 | task0001, task0002, task0003 | TS-1, TS-2, TS-7 |
| NFR1 | task0001, task0002 | TS-1, TS-2 (the check API's input surface is one file read + one stat by construction); reviewer confirms no per-attach binary read / hashing exists |
| NFR2 | task0001, task0002 | TS-10 |
| NFR3 | task0001, task0002 | TS-2 (0o600 + symlink refusal); task0001 AC-6 (exec target derived solely from daemon-recorded values); task0002 AC-7 / reviewer confirms the client transmits no path (bare Upgrade frame only) |

## E2E Testing

Project E2E command (workflow.yaml):
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test mux_hot_upgrade -- --test-threads=1`

- [ ] TS-3 — attach-path fire with PID survival and new-image proof
- [ ] TS-4 — no-churn on identical binary
- [ ] TS-5 — mux-start-path fire
- [ ] TS-7 — no misfire without identity file
- [ ] TS-9 — pre-existing hot-upgrade scenarios still pass

## Manual Testing (E2E Not Possible)

- [ ] MT-1 (real-environment smoke): with a mux session running from the
      installed binary, rebuild and reinstall eMterm (real rename
      replacement by the package/copy step), then `emterm mux attach` from
      a terminal — confirm the one-line replacement notice appears, the
      shell in the pane keeps its PID (`echo $$` before/after matches),
      and the daemon process's `/proc/<pid>/exe` no longer shows
      `(deleted)`.
- [ ] MT-2 (no-churn smoke): immediately attach again without
      reinstalling — confirm no notice appears and the daemon PID's start
      time is unchanged (no restart occurred).

## Performance / Security Verification

- NFR1: per-attach detection cost is one small-file read + one stat —
  verified structurally by the check API's contract (TS-1/TS-2 exercise
  exactly that input surface) and by review: no binary read, no hashing,
  no extra connections added per attach.
- NFR3: identity file 0o600 + symlink refusal in the 0o700 socket
  directory (TS-2); exec target derived solely from the daemon's own
  recorded identity, refusal when unrecorded (task0001 AC-6); connecting
  clients never transmit a path (bare Upgrade frame, task0002 AC-7); the
  existing handoff schema-range gate remains the candidate-binary
  authority (TS-9).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit (TS-1, TS-2, TS-6, TS-8) | 4 | 4 | 0 | 0 |
| Integration/E2E (TS-3, TS-4, TS-5, TS-7, TS-9) | 5 | 5 | 5 | 0 |
| Build/platform (TS-10) | 1 | 1 | 0 | 0 |
| Manual smoke (MT-1, MT-2) | 2 | 0 | 0 | 2 |
| **Total** | **12** | **10** | **5** | **2** |
