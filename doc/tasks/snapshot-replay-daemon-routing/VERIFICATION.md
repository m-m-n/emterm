# Verification Document: snapshot-replay-daemon-routing

## Overview

**Feature**: snapshot-replay-daemon-routing
**SPEC.md**: `doc/tasks/snapshot-replay-daemon-routing/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/snapshot-replay-daemon-routing/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors.

### CLI-only feature check (NFR4)

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0.

### Release build (NFR1 measurement prerequisite)

- Command: `make build`
- Expected: produces `src-tauri/target-host/release/emterm` with the new daemon path AND the existing `[mux-perf]` instrumentation in place.

### Windows cross-build (NFR4)

- Command: `make win-build`
- Expected: produces `src-tauri/target-win/x86_64-pc-windows-msvc/release/emterm.exe`.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Coverage target: existing test set MUST stay green; new TS-2 and TS-3 tests added under `src-tauri/src/mux/ipc/handlers.rs::tests`.
- Note: per project convention, `tabs.rs` replay tests may be non-deterministic under parallelism — re-run flaky failures with `--test-threads=1`.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Existing ordering-invariant tests in `handlers.rs:725-925` and `merge_efficiency_*` in `connection.rs` | All green after the implementation change. `merge_consecutive_chunks` now keys on `(pane_id, kind)` and never folds a Snapshot chunk into PtyOutput chunks; payload-byte assertions in `handle_set_visibility_*` tests still hold. | Unit / regression |
| TS-2 | `handle_request_pane_snapshot` is invoked on a session with shadow + scrollback seeded (same setup as `snapshot_bytes_unchanged_after_lock_scope_guardrail`); `pane_output_rx` is drained | Resulting chunk has `kind == ChunkKind::Snapshot`. Chunk payload bytes byte-identical to the predecessor task's snapshot assembly (clear+home prefix, scrollback, shadow screen, in that order). | Unit (new) |
| TS-3 | On a single-pane session: push one `PtyOutputChunk::pty_output(pane_id, "PRE")`, call `handle_request_pane_snapshot`, push one `PtyOutputChunk::pty_output(pane_id, "POST")`, drain `pane_output_rx` in order | Three chunks in the order `[PRE(kind=PtyOutput), snapshot(kind=Snapshot), POST(kind=PtyOutput)]`. `merge_consecutive_chunks` does NOT fold across kinds. | Unit (new) |
| TS-4 | `cargo test --manifest-path src-tauri/Cargo.toml` on the implementation branch | All tests green across `crates/term_core`, `src-tauri/src/mux`, `src-tauri/src/tabs.rs`, `crates/mux_ipc`. | Integration (full suite) |
| TS-5 | `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | Exit code 0. CLI-only build still type-checks (no GUI-only crates leaked into the changes). | Cross-build |
| TS-6 | `make win-build` | Windows release binary built successfully. | Cross-build |
| TS-7 | Manual production wall-time measurement | `[mux-perf]` log lines present in order: `request_pane_snapshot SENT` → `snapshot RECEIVED type=Snapshot` → `build_from_snapshot START` → `build_from_snapshot DONE` → `offthread swap START` → `offthread swap DONE`. `RECEIVED → swap DONE` delta < 1000 ms (MUST). SHOULD / STRETCH numbers recorded. | Manual + `[mux-perf]` instrumentation |
| TS-8 | Version-skew functional: prior-build client × freshly-built daemon | Tab switch works (no crash, no desync). Performance is at the old client's level (no improvement until client is also upgraded — expected). | Manual |
| TS-9 | Version-skew functional: freshly-built client × prior-build daemon | Tab switch works (no crash, no desync). New client receives `PtyOutput`-delivered snapshots via the live-input path; no performance improvement until daemon is also upgraded — expected. | Manual |
| EC-1 | Empty snapshot payload (pane just attached, no PTY output yet) | New reply path delivers a valid `MessageType::Snapshot` message; client processes without panic. Existing client `Snapshot` arm already handles empty payloads. | Implicit (covered by TS-7 first-ever switch + code review) |
| EC-2 | Snapshot reply size >= 64 KiB (off-thread path) | `apply_mux_message::Snapshot` dispatches to `dispatch_offthread_replay`; this is the main perf path exercised by TS-7. | Manual (covered by TS-7) |
| EC-3 | Snapshot reply size < 64 KiB (synchronous path) | `apply_mux_message::Snapshot` dispatches to `reset_frame_for_replay`; change of opcode does not affect this branch. | Manual (small-pane manual switch; also covered indirectly by TS-1 / TS-4 unit fixtures that build < 64 KiB snapshots) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` (optional; project does not enforce clippy on CI but the change should not introduce warnings)

## File Structure Verification

### Files to Modify

- `src-tauri/src/mux/session/pane.rs` — add `ChunkKind` enum, extend `PtyOutputChunk` with `kind` field, add named constructors (`pty_output(...)`, `snapshot(...)`).
- `src-tauri/src/mux/ipc/handlers.rs` — swap struct-literal construction in `handle_request_pane_snapshot` for `PtyOutputChunk::snapshot(...)`; refresh the FR3 / FR5 doc-comment block; add TS-2 and TS-3 unit tests.
- `src-tauri/src/mux/ipc/connection.rs` — branch the drain dispatch on `chunk.kind`; gate `merge_consecutive_chunks` merges on matching `kind`; extend the existing `merge_efficiency_*` tests with a kind-aware case.
- `crates/mux_ipc/src/protocol.rs` — add `MuxMessage::snapshot(pane_id, payload)` constructor helper (no `MessageType` change).

### Files Explicitly Unchanged

- `src-tauri/src/tabs.rs` — NO change in the implement phase. The `[mux-perf]` instrumentation (5 sites) remains in place to support TS-7 / NFR1 measurement and the `apply_mux_message::Snapshot|SnapshotRestore` arm already exists.
- `crates/mux_ipc/src/protocol.rs::MessageType` enum — no variant added or renumbered (NFR2).
- `src-tauri/src/mux/session/pane.rs::resume_pane_with_permit` (line 378) — NO behavioral change. Uses `PtyOutputChunk::pty_output(...)` (the default `kind == PtyOutput`). The `SetVisibility(true)` resume snapshot remains on the live-input path per SPEC §Out of Scope.
- `src-tauri/src/mux/ipc/reattach.rs::send_reattach_data` (line 285) — NO change. The reattach buffered output keeps `MessageType::PtyOutput` per SPEC §Out of Scope.
- WebView frontend (`src/`) — out of scope per `project_native_poc_branch_policy`.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-FR1..FR5 | All functional requirements implemented | Cargo test (TS-1..TS-4) + manual log inspection (TS-7) |
| SC-tests | TS-1..TS-9 all pass | Run each test scenario in order |
| SC-cargo | `cargo test` green | TS-4 |
| SC-cli | `cargo check --no-default-features` green | TS-5 |
| SC-win | `make win-build` green | TS-6 |
| SC-nfr1 | NFR1 MUST achieved + SHOULD / STRETCH recorded | TS-7, record into `VERIFICATION_RESULT.md` during `sdd.6-verify` |
| SC-revert | `[mux-perf]` reverted after `sdd.6-verify` | Final cleanup commit (post-verify; see "Post-verify cleanup" below) |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — daemon emits `MessageType::Snapshot` reply, ordering preserved | Phase 1, 2, 3 | TS-1, TS-2, TS-3, TS-7 |
| FR2 — client routes through existing arm | Phase 0 (no client change) | TS-7 (presence of `build_from_snapshot START/DONE` log) |
| FR3 — `PtyOutput`-as-snapshot fallback removed for the `RequestPaneSnapshot` reply (resume / reattach paths remain on `PtyOutput` per SPEC §Out of Scope) | Phase 3 | TS-1, TS-2, TS-4 (no remaining `handle_request_pane_snapshot` callsite produces a snapshot-shaped `PtyOutput`) |
| FR4 — version-skew compatibility both directions | (no code change required; semantic guarantee) | TS-8, TS-9 |
| FR5 — ordering invariants preserved | Phase 1, 2, 3 | TS-1, TS-3 |

## Performance Verification (NFR1)

- Measurement source: `[mux-perf]` log lines in `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.
- Metric: `RECEIVED → swap DONE` wall-clock delta for a ~2 MiB snapshot tab switch.
- Threshold gates:
  - **MUST**: < 1000 ms
  - **SHOULD**: < 200 ms
  - **STRETCH**: < 100 ms
- Procedure (TS-7):
  1. `make build`
  2. `pkill -f "emterm.*mux.*daemon"` (kill any old daemon so the new client spawns the new daemon)
  3. Launch the new client; create or reattach a tab; run `seq 1 10000000`.
  4. Switch to another tab, then switch back.
  5. `grep "\[mux-perf\]" ~/.local/share/net.laser5.app.emterm/logs/emterm.log`
  6. Record `RECEIVED → swap DONE` delta; compare against MUST / SHOULD / STRETCH.

## Security Verification

Not applicable per SPEC §Security Considerations. No new trust boundary, no parsing change, no auth path change. The existing cross-session pane-id refusal in `handle_request_pane_snapshot` (handlers.rs:442) remains intact (the change is downstream of the resolution check).

## Verification Summary

| Category | Items | Automated | Manual / E2E |
|----------|-------|-----------|--------------|
| Unit tests | TS-1, TS-2, TS-3 | Yes (cargo test) | – |
| Integration | TS-4 | Yes (cargo test full suite) | – |
| Cross-build | TS-5, TS-6 | Yes (cargo check / make win-build) | – |
| Manual (perf) | TS-7 / NFR1 | – | Yes (log grep + measurement) |
| Manual (version skew) | TS-8, TS-9 | – | Yes (functional) |
| Edge cases | EC-1, EC-2, EC-3 | Partial (EC-1 implicit, EC-3 implicit) | EC-2 manual |

## Post-verify Cleanup (NFR5)

After `sdd.6-verify` has written `VERIFICATION_RESULT.md` with the measured NFR1 numbers, a final cleanup commit MUST revert the 5 `log::warn!("[mux-perf] ...")` instrumentation sites in `src-tauri/src/tabs.rs`. This cleanup is OUT OF SCOPE for `sdd.4-implement` and tracked here so it is not forgotten.

- [ ] `[mux-perf]` instrumentation reverted (post-`sdd.6-verify` cleanup commit).

## Verification Result (filled in by sdd.4 and sdd.6)

### sdd.4-implement (2026-06-21)

#### Build Verification

- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` → exit 0, zero warnings.
- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` → exit 0, zero warnings (NFR4 partial; CLI-only build still type-checks).
- `make build` (release): **not run** in sdd.4-implement (deferred to sdd.5-check / sdd.6-verify per implement hand-off; project rule forbids unsolicited release builds).
- `make win-build`: **not run** in sdd.4-implement (deferred; same reason).

#### Test Verification

- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`
  - emterm bin lib: 1894 passed, 0 failed, 3 ignored.
  - mux_ipc crate: 50 passed, 0 failed.
  - term_core crate: 12 passed, 0 failed (counted in the other test-result lines).
  - Doc tests: 0.
- `--test-threads=1` is required for `tabs.rs` replay tests (existing project-wide caveat); under default parallelism four `tabs::tests::*` panic with "off-thread replay worker did not complete in time". Serialised run is green.
- New unit tests added (TS-2, TS-3 + Phase 1 / Phase 2 ergonomics + merge guards):
  - `mux::session::pane::tests::test_chunk_kind_constructors_round_trip` (Phase 1)
  - `mux::ipc::connection::tests::merge_does_not_fold_across_kind` (Phase 2)
  - `mux::ipc::connection::tests::merge_does_not_collapse_consecutive_snapshots` (Phase 2)
  - `mux::ipc::handlers::tests::handle_request_pane_snapshot_emits_snapshot_kind` (TS-2 / FR1+FR3)
  - `mux::ipc::handlers::tests::handle_request_pane_snapshot_preserves_fifo_ordering` (TS-3 / FR1+FR5)
- TS-1 review: existing ordering tests at `handlers.rs:725-925` pass unchanged. The `handle_set_visibility_*` tests inspect chunks via `rx.try_recv()` payload bytes (`b"\x1b[H\x1b[2J"`), not on `MuxMessage::msg_type`, so no `msg_type` assertion update was needed (resume path stays on `kind == PtyOutput` per SPEC §Out of Scope).

#### Code Quality Verification

- `rustfmt --edition 2024` applied only to the five modified files (project rule: no crate-wide fmt). Cargo check still green after fmt.
- No clippy run (project does not enforce clippy on CI; SPEC notes it as optional).

#### File Structure Verification

- Files modified (matches §Files to Modify):
  - `src-tauri/src/mux/session/pane.rs` — added `ChunkKind` enum, added `kind` field on `PtyOutputChunk`, added named constructors `pty_output(...)` / `snapshot(...)`. Migrated `resume_pane_with_permit` and in-file tests to the new constructors. Added `test_chunk_kind_constructors_round_trip`.
  - `src-tauri/src/mux/ipc/handlers.rs` — `handle_request_pane_snapshot` now constructs the reply via `PtyOutputChunk::snapshot(pane_id, snapshot)`. Doc-comment block refreshed to explain the on-wire framing change and reaffirm the FIFO invariant. Added TS-2 and TS-3 tests.
  - `src-tauri/src/mux/ipc/connection.rs` — `merge_consecutive_chunks` is now kind-aware (folds only when both sides are `ChunkKind::PtyOutput` for the same pane). Drain loop branches on `chunk.kind`: `Snapshot` → `MuxMessage::snapshot(...)`; `PtyOutput` empty → `PtyExited`; `PtyOutput` non-empty → `MuxMessage::pty_output(...)`. Added `merge_does_not_fold_across_kind` and `merge_does_not_collapse_consecutive_snapshots` tests, plus a `snapshot_chunk(...)` test helper.
  - `src-tauri/src/mux/ipc/pty_spawn.rs` — migrated the two `PtyOutputChunk { ... }` struct literals (reader-thread chunk and PTY-exit empty chunk) to `PtyOutputChunk::pty_output(...)`. No behavior change.
  - `crates/mux_ipc/src/protocol.rs` — added `MuxMessage::snapshot(pane_id, data)` helper alongside `pty_output(...)`. No `MessageType` enum / opcode change (NFR2).
- Files explicitly unchanged (matches §Files Explicitly Unchanged):
  - `src-tauri/src/tabs.rs` (`[mux-perf]` instrumentation retained, no functional change).
  - `crates/mux_ipc/src/protocol.rs::MessageType` enum (no variant added or renumbered).
  - `src-tauri/src/mux/session/pane.rs::resume_pane_with_permit` (still emits `kind == PtyOutput`).
  - `src-tauri/src/mux/ipc/reattach.rs::send_reattach_data` (still emits `MessageType::PtyOutput` via `framed.send` directly, bypassing the channel — out of scope).
  - WebView frontend (`src/`).

#### Existing E2E Regression (Phase 3.8)

- `sdd.yaml.project.components.main.e2e_test_command` is empty for this project; project README/CLAUDE.md do not define a self-contained E2E runner outside of TS-7 / TS-8 / TS-9 (manual binary-driven). No E2E regression run in sdd.4-implement — deferred to sdd.6-verify.

#### Known Limitations / Deferred to sdd.5-check / sdd.6-verify

- TS-5 (`cargo check --no-default-features`): green here, but the success criterion should be re-verified by `sdd.5-check`.
- TS-6 (`make win-build`): not run. Requires cargo-xwin + Windows MSVC target. Deferred.
- TS-7 (NFR1 wall-time): release binary not built. Manual measurement (`[mux-perf]` log grep) will be performed by `sdd.6-verify`.
- TS-8 / TS-9 (version-skew functional): manual; deferred to `sdd.6-verify`.
- EC-1 / EC-2 / EC-3: implicit / covered by TS-7 and the new unit tests; explicit edge-case checks deferred to `sdd.6-verify`.

#### Out of Scope confirmed in code

- `resume_pane_with_permit` (`pane.rs:378`) uses `PtyOutputChunk::pty_output(...)` — `SetVisibility(true)` resume snapshot remains on the live-input path.
- `send_reattach_data` (`reattach.rs:285`) uses `MuxMessage::pty_output(...)` via direct `framed.send` (does not even go through `pane_output_tx`) — reattach buffered output remains on `MessageType::PtyOutput`.
