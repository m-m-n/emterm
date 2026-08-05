# Verification Document: mux-tab-switch-bypass-refix

## Overview

**Feature**: mux-tab-switch-bypass-refix
**SPEC.md**: `feature-docs/mux-tab-switch-bypass-refix/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-tab-switch-bypass-refix/IMPLEMENTATION.md`

All commands below are the exact strings configured in `workflow.yaml`
`project.components` (enforced verbatim; run from the project root, never
`cd` first).

## Build Verification

- `rust_app`: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- `term_core`: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- `term_core_bench`: `CARGO_TARGET_DIR=src-tauri/target cargo check --release --manifest-path crates/term_core/Cargo.toml`
- `cli_feature_gate`: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- `web`: `bun run typecheck` (no TypeScript file changes expected in this
  feature — regression gate only)
- Expected: exit code 0, no errors, for every command.

## Test Verification

- `rust_app`: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
  (tests live in `--lib`; the command is already single-threaded per
  `test/README.md`'s `tabs.rs` replay-test note)
- `term_core`: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- `term_core_bench` (release benches, `#[ignore]`d):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --release --manifest-path crates/term_core/Cargo.toml --lib -- --ignored --nocapture`
- `web`: `bun test` (regression gate only)
- Coverage target: every Acceptance Criterion across task0001–task0006 has
  a corresponding automated test (fixed enumerable AC set; no percentage
  target — this is a bug-fix feature).

**Baseline-comparison note**: 7 `tabs.rs` off-thread tests are chronically
flaky on this host even on `main` (prior feature's retrospect). A
`rust_app` test failure that reproduces identically on `main` is baseline
noise, not a feature regression.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | 26-segment MIDDLE marker cluster (2 MiB, 31 segments, `k = 27`, adjacent dims all differing, oscillating at-or-above the settled target) | Split engages post-fix (confirmed failing pre-fix); built core matches the reference non-bypass build (viewport/cursor, `scrollback_populated`) | Unit (`term_core`) |
| TS2 | Boundary of the new segment-count treatment | Exactly-at engages; one-past does not; prior 24-boundary tests' intent preserved at the new bound | Unit (`term_core`) |
| TS3 | `h == k` empty-MIDDLE shape, bypass requested | Built core at caller-requested dims; `scrollback_populated` matches the reference build (FR5 pin) | Unit (`term_core`) |
| TS4 | Settler-wake rate limit while `awaiting_decision` | Within-interval frames do not request redraw; past-interval frames do; idle window still reaches the decision within `RESIZE_SETTLE_MAX_DURATION` | Unit (`rust_app` `window_host`) |
| TS5 | Status-bar height change with unchanged derived grid size | New inset values applied; nothing-changed frames leave insets untouched and never set `pending_resize` (no reshape storm) | Unit (`rust_app` `window_host`) |
| TS6 | Release bench suite | New 26-segment-shape bench asserts its ceiling relative to segment-free cost; `snapshot_replay_bench_2mib_seq`, `marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost`, `large_prefix_small_suffix_bench_does_not_engage_the_split`, `daemon_cap_prefix_with_small_suffix_bench_does_not_engage_the_split`, `ordinary_switch_bench_950kib_matches_segment_free_cost` all stay green | Bench (release, `term_core_bench` command) |
| TS7 | Real-machine reattach + heavy-pane switch (carried over from the prior feature's MT-1) | Display appears in the tens-of-ms order | Manual |

### Rework Scenarios (review round 1)

Added by the round-1 review rework (tasks task0004–task0006); IDs continue
the TS sequence.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS8 | `h == 0` (fold-degraded) column-changing MIDDLE at the admitted segment count | The shape's replay cost is demonstrably bounded: a unit test pins a tightened/conditional gate at both sides of the `h == 0` bound, and/or a release bench packs the MIDDLE to the `BYPASS_PREFIX_MAX_BYTES` limit with a dominating suffix and asserts a ceiling relative to bypass-engaged cost; the gate's doc rationale matches the demonstrated behavior (no same-width-by-construction claim) | Unit (`term_core`) and/or Bench (release, `term_core_bench` command) |
| TS9 | Segment-bound constant purpose and cross-crate pin coherence | Reverting the gate bound to 24 without a matching daemon-cap decision still fails a test (drift caught); the pin (or its dedicated daemon-cap mirror) no longer misdiagnoses a deliberate cost-policy change as the round-7/round-8 regression; the segment-count condition's declared purpose matches what its tests enforce (or its removal is covered by the byte-bound + suffix-dominance guards) | Unit (`term_core`, test-time pin) |
| TS10 | Non-settler `pending_resize` during the settling window (mux-attach firing order: settler reset + transient inset write → sidebar inset change raises `pending_resize` → `apply_pending_resize`; also compositor `Resized` / scale change) | The applied/broadcast grid size is never computed from a not-yet-settled status-bar inset — it equals the settler's last forwarded size or the resize defers until the settler forwards; the settler-recorded and actually-applied sizes stay equal; no stale lock-in after settlement | Unit (`rust_app` `window_host`) |
| TS11 | Inset-change predicate tolerance at representable magnitudes | The pinned tolerance is actually reachable: either a representable within-tolerance nonzero perturbation is asserted unchanged (a case a plain-inequality predicate would fail) plus a past-tolerance case asserted changed, or the predicate is an exact comparison with the epsilon claim removed from the doc; no test perturbation rounds back to its baseline | Unit (`rust_app` `window_host`) |

## Code Quality Verification

- Format (`rust_app`): `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Format (`term_core`): `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
- Format (`web`): `bunx biome check .`
- Static analysis: the build-verification `cargo check` commands above are
  the project's configured static gates.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR6 implemented and tested | TS1–TS7 all pass |
| SC-2 | NFR1–NFR3 satisfied | TS4 (NFR2), TS6 (NFR1/NFR3), plus the test commands above all green |
| SC-3 | TS1–TS7 pass | Test Verification commands above, exit 0; TS7 manually |
| SC-4 | All four deferred round-2 highs (`b6a60c440da70e79`, `81507f39e384b34e`, `a82206113b8160fd`, `aba5ebbdf9a9addb`) and the un-re-reviewed critical (`5c6ae6b507b6f638`) addressed and passing this feature's review | Finding-ID doc-comment traceability (IMPLEMENTATION.md D-C) + this feature's review phase over the full diff |
| SC-5 | Code review completed | `workflow.yaml` `review` step completed |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0004 | TS1, TS2, TS7, TS8 |
| FR2 | task0001 | TS6 |
| FR3 | task0002 | TS4 |
| FR4 | task0002, task0005, task0006 | TS5, TS10, TS11 |
| FR5 | task0003 | TS3 |
| FR6 | task0001, task0002, task0004, task0005 | TS5, TS6, TS7, TS10 |
| NFR1 | task0001, task0004 | TS6, TS8 |
| NFR2 | task0002 | TS4 |
| NFR3 | task0001, task0002, task0003, task0004, task0005, task0006 | TS6 (benches), TS9 + the `rust_app` / `term_core` `--lib` commands |

## E2E Testing

No E2E infrastructure exists in this repository (`e2e_test_command` is
empty for every component). Not applicable.

## Manual Testing (E2E Not Possible)

- [ ] TS7 (MT-1 carry-over): on a real machine, restart the eMterm client,
  reattach to a mux daemon session with a heavy pane matching the measured
  shape class (substantial scrollback with a tail-adjacent resize-marker
  cluster), and switch to that pane — the display appears in the
  tens-of-ms order, not after a near-second stall. The full startup →
  attach → switch sequence on a live daemon is not automatable without
  live GUI + daemon orchestration; TS1/TS4/TS5/TS6 verify the underlying
  mechanisms in isolation.

(Design step was skipped for this feature — no mockup visual comparison
applies.)

## Performance / Security Verification

- FR1 / TS1 / TS6: the measured 26-segment shape replays within the bench's
  ceiling relative to bypass-engaged (segment-free) cost — the "tens of
  ms" goal is asserted as a same-host relative bound, per SPEC 9.2.
- FR6 / TS6: ordinary switch latency unregressed
  (`ordinary_switch_bench_950kib_matches_segment_free_cost` green).
- NFR1 / TS6: no reintroduced double 2nd-pass non-bypass cost (large- and
  daemon-cap-prefix rejection benches green).
- NFR2 / TS4: self-wake redraws during the settle window bounded to the
  predicate's minimum interval, not full frame rate.
- Security: not applicable (internal performance/correctness fix; no new
  external input surface).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 5 commands | Yes | - | - |
| Test scenarios | 11 (TS1–TS11) | TS1–TS6, TS8–TS11 | - | TS7 |
| Code quality | 3 format commands | Yes | - | - |
| Success criteria | 5 (SC-1–SC-5) | SC-1–SC-3 (automated portions) | - | SC-4/SC-5 via review phase; TS7 manual |
