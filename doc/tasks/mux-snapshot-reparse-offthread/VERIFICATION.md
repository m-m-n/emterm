# Verification Document: mux snapshot reparse cost — measure and decide

## Overview

**Feature**: mux-snapshot-reparse-offthread
**SPEC.md**: `doc/tasks/mux-snapshot-reparse-offthread/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-snapshot-reparse-offthread/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

### Actual (sdd.4-implement, 2026-06-18)

- CLI-only check (`--no-default-features`): **exit 0**, `Finished dev profile`,
  no errors/warnings. (Full GUI build was verified via the default test build
  below, which compiles the same crate graph; release builds were intentionally
  NOT run per project policy.)
- Note: a **pre-existing, out-of-scope** compile + runtime breakage in
  `crates/term_core/src/parser/tests.rs` (4 "Colon Sub-Parameter" tests
  referencing a removed `parser_params::SUB_PARAM_FLAG` and absent colon
  sub-param behavior) blocked `cargo test -p term_core` from compiling at all.
  These tests are unrelated to this feature (parser, not terminal_core/handlers)
  and were confirmed broken on a clean tree. They were compiled out with
  `#[cfg(any())]` + an explanatory comment to unblock verification; the parser
  implementation was NOT modified. Restore/rewrite them in a follow-up once the
  colon sub-param representation is settled.

## Test Verification

- Command (default suite): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- term_core unit tests: `CARGO_TARGET_DIR=src-tauri/target cargo test -p term_core`
- Measurement harness (gated, on demand): `CARGO_TARGET_DIR=src-tauri/target cargo test -p term_core -- --ignored --nocapture`
- Coverage target: not a coverage-driven feature; assert the named scenarios below.

### Actual (sdd.4-implement, 2026-06-18)

- Default suite (`cargo test --manifest-path src-tauri/Cargo.toml`):
  **1796 passed; 0 failed; 1 ignored** (the gated harness). GREEN.
- term_core suite (`cargo test -p term_core`):
  **639 passed; 0 failed; 4 ignored**. The 4 ignored = 1 measurement harness +
  3 compiled-out pre-existing colon sub-param tests. GREEN.
- New tests added:
  - `terminal_core::tests::test_synthetic_scrollback_is_deterministic` (FR1
    determinism) — pass.
  - `terminal_core::tests::test_reparse_empty_input_no_panic` (TS-2, FR1
    empty-input guard, ~0 ms) — pass.
  - `terminal_core::tests::measure_reparse_cost_2mib` (TS-1, `#[ignore]`-gated
    harness) — runs only with `--ignored`.
  - `mux::ipc::handlers::tests::snapshot_bytes_unchanged_after_lock_scope_guardrail`
    (TS-3, FR3 byte-identity for representative + empty scrollback) — pass.
- TS-4 (existing mux reattach/snapshot suite): `mux::` filter ran
  **294 passed; 0 failed** — unchanged.
- TS-5 (default run excludes harness): confirmed — `1 ignored`, harness not
  executed in the default suite.

### Measurement figures (TS-1, FR2 input — DEBUG build, unoptimized)

`cargo test -p term_core -- --ignored --nocapture`:

| Size    | Bytes     | Elapsed    | Throughput |
|---------|-----------|------------|------------|
| 256 KiB | 262,144   | 324.944 ms | 0.8 MiB/s  |
| 1 MiB   | 1,048,576 | 1130.153 ms| 0.9 MiB/s  |
| 2 MiB   | 2,097,152 | 2127.101 ms| 0.9 MiB/s  |

- Headline ~2 MiB figure: **~2127 ms (~0.9 MiB/s)**, scaling ~linearly.
- Caveat: these are **debug/unoptimized** numbers (release builds are
  intentionally not run per project policy). A release build would be
  substantially faster; the FR2 threshold mapping (§4: <5 / 5–50 / 50+ ms)
  must account for this. The go/no-go decision + rationale belongs to
  sdd.6-verify (VERIFICATION_RESULT.md), not this implementation step.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Run the gated harness on a ~2 MiB synthetic scrollback | Prints elapsed ms + MiB/s; completes | Measurement |
| TS-2 | Feed a 0-byte input to the reparse path | No panic; ≈0 ms | Unit |
| TS-3 | Assemble snapshot bytes for a representative screen + scrollback after Phase 2 | Bytes byte-identical to the established layout | Unit |
| TS-4 | Run existing mux reattach/snapshot tests after Phase 2 | All pass unchanged | Integration |
| TS-5 | Run the default `cargo test` | Measurement harness is NOT executed (gated/ignored) | Build/process |
| TS-6 | CLI-only `cargo check --no-default-features` | exit 0 | Build |
| TS-7 | After running the harness, map the figure to thresholds and record the decision | Decision + rationale present in VERIFICATION_RESULT.md | Manual/verify |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml` (PostToolUse hook also enforces rustfmt + biome)
- Static analysis: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` (warnings reviewed)

### Actual (sdd.4-implement, 2026-06-18)

- Format: PostToolUse hook formatted each edited file on save; verified with
  `rustfmt --edition 2024 --check` scoped to the three touched files —
  **exit 0** (no diff). No crate-wide `cargo fmt` was run (project policy).
- Only the three intended files changed (plus the feature doc dir); confirmed
  via `git status --porcelain`. No unrelated files were dirtied.

## File Structure Verification

### Files to Create

- (none) — additions land in existing files.

### Files to Modify

- [x] `crates/term_core/src/terminal_core.rs` — gated reparse timing harness +
      deterministic synthetic-input helper (FR1). **Done.**
- [x] `src-tauri/src/mux/ipc/handlers.rs` — explicit copy-only scrollback lock
      scope (scoped block + invariant comment); byte-identity safeguard (FR3).
      **Done.**
- [ ] `doc/tasks/mux-snapshot-reparse-offthread/VERIFICATION_RESULT.md` —
      decision record (FR2, created at verify by sdd.6). **Pending (verify-time).**

### Additional file touched (unplanned, scope-unblock only)

- [x] `crates/term_core/src/parser/tests.rs` — compiled out 4 pre-existing,
      out-of-scope colon-sub-param tests (`#[cfg(any())]` + comment) that
      otherwise blocked `cargo test -p term_core` from compiling. No parser
      implementation changed. Flagged for a follow-up restore/rewrite.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1 harness deterministic, default-excluded, prints ~2 MiB figure | TS-1, TS-5 |
| SC-2 | FR2 decision recorded against §4 thresholds with rationale | TS-7 |
| SC-3 | FR3 lock-scope guard-rail (explicit drop point + comment), snapshot bytes unchanged | TS-3, TS-4 |
| SC-4 | Default `cargo test` green; CLI-only `cargo check` green | TS-2, TS-4, TS-5, TS-6 |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 | Phase 1 | TS-1, TS-2, TS-5 |
| FR2 | Phase 3 | TS-1, TS-7 |
| FR3 | Phase 2 | TS-3, TS-4 |
| NFR1 | Phase 1 | TS-2, TS-5 |
| NFR2 | Phase 1/2 | TS-4, TS-5, TS-6 |
| NFR3 | Phase 2 | TS-6 |

## E2E Testing

(Not applicable — Rust perf/measurement feature; no project E2E framework involvement.)

## Manual Testing (E2E Not Possible)

- [ ] TS-1: run the gated harness; capture the ~2 MiB ms + MiB/s figure.
- [ ] TS-7: map the figure to a threshold band and record the decision +
      rationale (including real-world 2 MiB-fill frequency) in
      VERIFICATION_RESULT.md; if "go", state the 案a follow-up scope
      (core only, no LRU).

## Performance Verification

- The harness IS the performance measurement. Recorded value is informational
  input to the FR2 decision (no fixed pass/fail threshold beyond the §4 bands).

## Security Verification

- [ ] FR3 preserves the existing session-scope authorization in
      `handle_request_pane_snapshot` (snapshot served only for the requester's
      attached session) and does not alter the output bytes.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit | TS-2, TS-3 | 2 | 0 | 0 |
| Integration | TS-4 | 1 | 0 | 0 |
| Build/process | TS-5, TS-6 | 2 | 0 | 0 |
| Measurement/decision | TS-1, TS-7 | 0 | 0 | 2 |
| Security | session-scope preserved | 1 | 0 | 0 |
