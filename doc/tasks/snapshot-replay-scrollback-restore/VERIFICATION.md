# Verification Document: snapshot-replay-scrollback-restore

## Overview

**Feature**: snapshot-replay-scrollback-restore
**SPEC.md**: `doc/tasks/snapshot-replay-scrollback-restore/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/snapshot-replay-scrollback-restore/IMPLEMENTATION.md`

The feature restores the `scrollback_slim` / `scrollback_wrapped`
history that the off-thread bypass replay path leaves empty. A 2nd-pass
worker rebuilds the scrollback from the same payload (bypass off) and
the result is prepended onto the live `TerminalCore`. Verification
exercises the term_core merge primitive, the tabs.rs polling /
cancel / panic paths, performance non-regression vs. the
`snapshot-replay-perf` baseline, and a manual smoke that the end user
actually sees history after the visible grid paints.

## Build Verification

- **Command**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- **Expected**: exit code 0, no new warnings.
- **Actual (sdd.4-implement)**: exit code 0, no new warnings observed.

- **CLI-only build (NFR8 + CLI parity)**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- **Expected**: exit code 0; the new entry points are dead-code-pruned
  out of the CLI build (no warnings).
- **Actual (sdd.4-implement)**: exit code 0, no new warnings.

- **Format**:
  `cargo fmt --manifest-path src-tauri/Cargo.toml`
- **Expected**: no diff (the PostToolUse hook enforces formatting on
  edited files; this is a final sweep).
- **Actual (sdd.4-implement)**: PostToolUse hook ran after every Edit
  touching `*.rs`; no manual `cargo fmt` was issued at the end (per
  project policy, crate-wide fmt is avoided).

## Test Verification

- **Unit + integration**:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
  - `--test-threads=1` is mandatory because the `tabs.rs` replay
    tests share `std::env` state and fail nondeterministically in
    parallel (per `project_test_execution_notes` in MEMORY.md).
  - **Expected**: all tests pass including the new ones below.
  - **Actual (sdd.4-implement)**: 1903 passed; 0 failed; 3 ignored.

- **term_core unit tests only** (faster signal during Phase 1):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
  - **Expected**: all new merge / build_scrollback_only tests pass.
  - **Actual (sdd.4-implement)**: 682 passed; 0 failed; 7 ignored
    (`scrollback_restore_bench_2mib_seq` is now part of the ignored
    set, joining `snapshot_replay_bench_2mib_seq`).

- **Coverage target**: ≥ 90 % on `merge_scrollback_from` and
  `build_scrollback_only_from_snapshot`; ≥ 80 % on the new tabs.rs
  paths.

### Test Scenarios from SPEC.md

| ID    | Scenario | Expected Result | Test Type |
|-------|----------|-----------------|-----------|
| TS-1  | `merge_scrollback_from` re-interns SlimCell ids | Merged row's style_id / char_id resolve in `self.styles` / `self.chars` to byte-equal entries from `other` | Unit (term_core) |
| TS-2  | `merge_scrollback_from` preserves `scrollback_evicted_total` | Counter unchanged before/after merge | Unit (term_core) |
| TS-3  | `merge_scrollback_from` respects ring capacity | Combined length capped at `scrollback_capacity`; front-most *incoming* rows dropped; `self`'s pre-existing rows kept | Unit (term_core) |
| TS-4  | `merge_scrollback_from` no-ops on cols mismatch | `self` unchanged; `log::warn` emitted | Unit (term_core) |
| TS-5  | `build_scrollback_only_from_snapshot` matches synchronous build | `scrollback_slim`, `scrollback_wrapped`, `scrollback_evicted_total`, and viewport grid byte-equal to a freshly-reset-and-replayed reference core | Unit (term_core) |
| TS-6  | `bypass_plus_merge_equivalence` (the NFR6 / FR1 gate) | After 1st-pass + 2nd-pass + merge with `live_growth = 0`, state observably equal to synchronous bypass-off build | Unit (term_core) |
| TS-7  | Off-thread switch then scrollback restored | After dispatching ≥ 64 KiB payload, polling swap, and polling restore, `scrollback_slim` content matches the synchronous reference | Integration (tabs.rs) |
| TS-8  | Supersede cancels in-flight restore | Dispatching switch B before A's restore arrives drops A's receiver; A's payload absent from final scrollback | Integration (tabs.rs) |
| TS-9  | Concurrent live drain reconciles (FR3) | Final scrollback = (historical prepended) ∪ (live appended), no duplication | Integration (tabs.rs) |
| TS-10 | Resize during restore cancels (FR5 / UC03) | `Tab::resize` clears `pending_scrollback_restore`; no rows merged; no respawn | Integration (tabs.rs) |
| TS-11 | Worker panic → `log::warn!` + state cleared (FR7) | mpsc `Disconnected` arm clears state, app continues | Integration (tabs.rs) |
| TS-12 | Threshold parity: below 64 KiB no 2nd-pass (FR6) | Payload of `OFFTHREAD_REPLAY_THRESHOLD_BYTES - 1` takes synchronous path; `pending_scrollback_restore` not installed | Integration (tabs.rs) |
| TS-13 | Threshold parity: at-or-above 64 KiB installs restore (FR6) | Payload of `OFFTHREAD_REPLAY_THRESHOLD_BYTES` takes off-thread path; `pending_scrollback_restore` installed after swap | Integration (tabs.rs) |
| TS-14 | `live_growth` exceeds rebuilt scrollback → no-op (edge case from 要件定義書 §4.2 F02) | `apply_scrollback_restore` clears state without panicking | Unit (tabs.rs) |
| TS-15 | `merge_scrollback_from` keeps `prompt_marks` / `fold_marks` from 2nd-pass out of live (FR8) | After merge, live core's prompt/fold marks unchanged from the 1st-pass post-swap state | Integration (tabs.rs) |

## Code Quality Verification

- **Format**: `cargo fmt --manifest-path src-tauri/Cargo.toml` (project
  policy: rustfmt style_edition=2024; PostToolUse hook enforces on
  edited files).
  - **Actual (sdd.4-implement)**: PostToolUse formatter ran on every
    edited Rust file; no end-of-phase `cargo fmt` sweep was issued.
- **Static analysis**: `cargo check` (lints are not strictly gated in
  this repo; treat new `dead_code` / `unused` warnings as failures
  unless explicitly justified).
  - **Actual (sdd.4-implement)**: both `cargo check` (default `gui`
    features) and `cargo check --no-default-features` finished with
    zero warnings. The Phase-2 intermediate `dead_code` warnings on
    `PendingScrollbackRestore` / `ScrollbackRestoreOutcome` /
    `poll_pending_scrollback_restore` cleared as soon as
    `App::pump_all` wired the new poll in (task `wire-app-pump-all`).

## File Structure Verification

### Files to Create

- (none — all changes are additive method/struct additions in existing
  files)

### Files to Modify

- [x] `crates/term_core/src/terminal_core.rs` — `build_from_snapshot_inner`
  helper, `build_scrollback_only_from_snapshot`, `merge_scrollback_from`
  (signature evolved to take `live_trim_rows: usize` so FR3 trim happens
  inside the merge, keeping the caller's lock window smaller)
- [x] `crates/term_core/src/ring_buffer.rs` — `prepend_scrollback_rows`
- [x] `crates/term_core/src/bench.rs` — `scrollback_restore_bench_2mib_seq`
- [x] `src-tauri/src/tabs.rs` — `PendingScrollbackRestore`,
  `ScrollbackBuild`, `ScrollbackRestoreOutcome`,
  `Tab::poll_pending_scrollback_restore`,
  `Tab::apply_scrollback_restore`,
  `Tab::spawn_scrollback_restore`,
  `Tab::cancel_pending_scrollback_restore`, extensions to
  `apply_offthread_swap` (now also receives `payload: Vec<u8>` so it can
  hand it to the 2nd-pass worker) / `dispatch_offthread_replay` /
  `Tab::resize`, test helpers
  (`test_has_pending_scrollback_restore`,
  `test_drain_pending_scrollback_restore_for_blocking_recv`,
  `test_force_scrollback_restore_disconnect`,
  `test_scrollback_length`)
- [x] `src-tauri/src/app.rs` — `App::pump_all` new poll wiring (Merged
  or Failed → `changed = true`; active tab → `active_changed = true`)
- [x] `src-tauri/src/window_host.rs` — `WindowEvent::CloseRequested`
  shutdown cancel sweep immediately before `self.app.tabs.clear()`
- `doc/tasks/snapshot-replay-scrollback-restore/sdd.yaml` — FR2 status
  flipped to `ok`, `tbd_reason` removed; `requirements.*.tasks` and
  `requirements.*.tests` populated

## SPEC.md Compliance

### Success Criteria

| ID    | Criterion | How to Verify |
|-------|-----------|---------------|
| SC-1  | All FRs implemented and covered | TS-1 … TS-15 + grep `merge_scrollback_from` / `pending_scrollback_restore` references |
| SC-2  | NFR1 (1st-pass non-regression) | Run `snapshot_replay_bench_2mib_seq`; compare per-call to pre-feature baseline |
| SC-3  | NFR2 (2nd-pass within budget) | Run `scrollback_restore_bench_2mib_seq`; assert < 5 s |
| SC-4  | NFR6 (equivalence with sync build) | TS-6 (`bypass_plus_merge_equivalence`) passes |
| SC-5  | Threshold contract drift eliminated | TS-12 + TS-13 + TS-7 jointly demonstrate same observable state across the boundary |
| SC-6  | `scrollback_evicted_total` monotonicity | TS-2 (unit) + TS-9 (integration) |
| SC-7  | No new `cargo` warnings | `cargo check` is clean (both `--features gui` default and `--no-default-features`) |
| SC-8  | CLI-only build unaffected | `--no-default-features` cargo check passes |
| SC-9  | WebView (`src/`) untouched (NFR8) | `git diff --stat refactor/native-terminal-hybrid…HEAD -- src/` is empty |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — 2nd-pass spawn after bypass-on swap | Phase 2 | TS-7, TS-13 (state installed); log inspection (info line at spawn) |
| FR2 — Merge primitive with id re-intern | Phase 1 | TS-1, TS-2, TS-3, TS-4, TS-6 |
| FR3 — Live-drain reconciliation via `base_evicted_total` | Phase 2 | TS-9, TS-14 |
| FR4 — `poll_pending_scrollback_restore` non-blocking polling | Phase 2 | TS-7, TS-8 (poll never blocks); grep `App::pump_all` for the new poll call |
| FR5 — Cancel / supersede | Phase 2 | TS-8 (new switch), TS-10 (resize); shutdown sweep — log inspection only |
| FR6 — Threshold parity (no 2nd-pass below 64 KiB) | Phase 2 | TS-12, TS-13 |
| FR7 — Spawn-fail / panic → warn only | Phase 2 | TS-11; log inspection (warn line at spawn fail / disconnect) |
| FR8 — Mark non-duplication | Phase 2 | TS-15 |
| NFR1 — 1st-pass swap ≤ 60 ms for 2 MiB | Phase 3 | `snapshot_replay_bench_2mib_seq` per-call < 1 s (existing gate); reference-machine timing < 60 ms |
| NFR2 — 2nd-pass + merge ≤ 5 s for 2 MiB | Phase 3 | `scrollback_restore_bench_2mib_seq` per-call < 5 s |
| NFR3 — UI non-blocking | Phase 2 | Code inspection: `try_recv` only on the UI thread; no `recv`/`join` in `App::pump_all` |
| NFR4 — One in-flight 2nd-pass per tab | Phase 2 | TS-8 (supersede); code inspection: `pending_scrollback_restore: Option<…>` |
| NFR5 — `scrollback_evicted_total` monotonic | Phase 1+2 | TS-2 (unit), TS-9 (integration) |
| NFR6 — Equivalence with synchronous bypass-off build | Phase 1 | TS-5, TS-6 |
| NFR7 — Logging at spawn / cancel / completion / failure | Phase 3 | Manual log scan with `RUST_LOG=info` |
| NFR8 — WebView untouched | All phases | SC-9 |

## E2E Testing

This project has no E2E framework (`sdd.yaml` →
`project.components.main.e2e_test_command = ""`; no
`docker-compose.e2e.yml`; no `e2e-tests/` directory). E2E section
intentionally omitted.

### Existing E2E Regression (sdd.4-implement Phase 3.8)

- **Detection result**: skipped — `sdd.yaml.e2e_test_command` is empty,
  no `e2e-tests/README.md`, no `docker-compose.e2e.yml`. Nothing to
  regress against.

## Manual Testing (E2E Not Possible)

- [ ] **Mux smoke (UC01 from 要件定義書.md §3.2)**:
  1. Start emterm: `make dev`.
  2. Open a mux session; in window A, generate ≥ 2 MiB of scrollback
     (e.g. `seq 1 500000`).
  3. Switch to window B; do something else; switch back to window A.
  4. **As soon as the visible grid paints** (within ~50 ms), scroll
     up. Expected: scrollback may briefly be empty.
  5. Wait ~5 s, scroll up again. Expected: history visible.
  6. Inspect `~/.local/share/net.laser5.app.emterm/logs/emterm.log`
     with `RUST_LOG=info make dev`. Expected: `mux-scrollback-restore`
     spawn line + `scrollback restored: N rows prepended` info line.
- [ ] **Rapid switch supersede (UC02)**:
  1. With two heavy mux windows, switch A → B → A in < 1 s.
  2. Confirm log shows a `cancelled (superseded)` warn for the first
     2nd-pass and a fresh spawn + merge for the second switch.
- [ ] **Resize cancel (UC03)**:
  1. Switch to a heavy mux window.
  2. Within the 5 s restore window, resize the emterm window (drag
     border).
  3. Confirm log shows a `cancelled (resize)` warn; scroll up shows
     no history (or only live drain that arrived after the swap).
- [ ] **Small-payload regression (UC05)**:
  1. Switch to a mux window with very little scrollback (< 64 KiB
     payload).
  2. Confirm scrollback is available immediately after the switch
     (synchronous path unchanged); confirm no `mux-scrollback-restore`
     line in the log.

## Performance Verification

- **NFR1 — 1st-pass swap ≤ 60 ms for 2 MiB seq-N**:
  - Command:
    `CARGO_TARGET_DIR=src-tauri/target cargo test --release --manifest-path crates/term_core/Cargo.toml --lib snapshot_replay_bench_2mib_seq -- --nocapture --include-ignored`
  - Expected: `[bench] build_from_snapshot 2MiB seq-N payload …
    {per-call}` ≤ 60 ms on the reference machine; the existing in-test
    `MUST < 1000 ms` assertion passes (regression guard).

- **NFR2 — 2nd-pass + merge ≤ 5 s for 2 MiB seq-N**:
  - Command:
    `CARGO_TARGET_DIR=src-tauri/target cargo test --release --manifest-path crates/term_core/Cargo.toml --lib scrollback_restore_bench_2mib_seq -- --nocapture --include-ignored`
  - Expected: per-call total (build bypass-on + build bypass-off +
    merge) < 5 s; in-test assertion enforces.

- **Memory peak (informational, no automated gate)**: while running
  the manual smoke (UC01), observe RSS. The 2nd-pass adds a second
  `TerminalCore` of the same scrollback capacity (2 MiB compressed
  bound). Expected: transient ~2× live-core RSS spike, returning to
  baseline within ~5 s.

## Security Verification

The feature does not widen trust (snapshot payload originates from
the same trust domain as today). No security-specific tests.

- [ ] Code inspection: `merge_scrollback_from` and
  `apply_scrollback_restore` perform no I/O, no allocation from
  attacker-controlled lengths beyond the existing scrollback cap.
- [ ] The 2nd-pass worker holds no references to the live core (the
  payload is cloned at dispatch).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios | 15 (TS-1 … TS-15) | 15 | 0 | 0 |
| Functional requirements | 8 (FR1 … FR8) | 8 | 0 | 0 |
| Non-functional requirements | 8 (NFR1 … NFR8) | 7 | 0 | 1 (NFR7 — log inspection) |
| Success criteria | 9 (SC-1 … SC-9) | 9 | 0 | 0 |
| Manual smokes | 4 (UC01, UC02, UC03, UC05) | 0 | 0 | 4 |
| Performance gates | 2 (NFR1, NFR2 benches) | 2 | 0 | 0 |
| Build configurations | 2 (default + `--no-default-features`) | 2 | 0 | 0 |
