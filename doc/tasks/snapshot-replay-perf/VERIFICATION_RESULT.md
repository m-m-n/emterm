# Verification Result: snapshot-replay-perf

**Verified**: 2026-06-21 (sdd.6-verify)
**Feature**: snapshot-replay-perf
**SPEC.md**: `doc/tasks/snapshot-replay-perf/SPEC.md`
**VERIFICATION.md**: `doc/tasks/snapshot-replay-perf/VERIFICATION.md`

---

## 1. Executive Summary

All four perf-gated benches (TS-6 / TS-7 / TS-8 / TS-9) pass with **substantial
headroom** against their `assert!` thresholds. The headline TS-6
`snapshot_replay_bench_2mib_seq` now runs at **~51 ms/call**, against the
**1000 ms MUST** threshold — a **~79× speedup** over the pre-feature
baseline of ~4040 ms. Both the SHOULD (< 200 ms) and STRETCH (< 100 ms)
targets are also achieved. The supporting benches (attribution at
~52 ms, strip at ~2.5 ms, scrollback read at ~65 µs) all confirm no
collateral regression. All six expected source files were modified, the
deviation noted in IMPLEMENTATION.md (snapshot.rs literal init +
grid_fingerprint helper) is consistent with the staged changes. SC-1..SC-6
are satisfied automatically; SC-7 (memory file update) and TS-12 (manual
tab-switch confirmation) remain as user-side follow-ups.

---

## 2. Performance Bench Results

All benches were run from project root with:
- `CARGO_TARGET_DIR=src-tauri/target`
- `cargo test --release ... -- --nocapture --include-ignored`

### Table

| ID    | Bench                                                      | Threshold (assert) | Measured per-call | Result | MUST | SHOULD | STRETCH |
| ----- | ---------------------------------------------------------- | ------------------ | ----------------- | ------ | ---- | ------ | ------- |
| TS-6  | `snapshot_replay_bench_2mib_seq` (main replay)             | < 1000 ms          | **51.32 ms**      | PASS   | met  | met (< 200 ms) | met (< 100 ms) |
| TS-7  | `snapshot_replay_attribution_2mib_seq` (scrollback-disabled) | < 200 ms         | **51.62 ms**      | PASS   | n/a  | n/a    | n/a     |
| TS-8  | `strip_replayable_rich_content_bench_2mib_plain`           | < 30 ms            | **2.50 ms**       | PASS   | n/a  | n/a    | n/a     |
| TS-9  | `scrollback_read_all_bench_2mib_wrapped`                   | < 1 ms             | **65.33 µs**      | PASS   | n/a  | n/a    | n/a     |

### TS-6 speedup vs. baseline

| Phase                            | per-call    | Notes |
| -------------------------------- | ----------- | ----- |
| Pre-feature baseline (from SPEC) | ~4040 ms    | seq 1 N 2 MiB, 200×50, scrollback=10000 |
| **Post-feature (TS-6 measured)** | **51.32 ms**| ~79× speedup; matches scrollback-disabled ceiling (51.62 ms in TS-7) |

The post-feature TS-6 number is **within ~1 ms** of the scrollback-disabled
configuration in TS-7 (51.32 ms vs 51.62 ms). This is exactly the upper-bound
predicted by SPEC.md "Overview" — once the per-row SlimCell intern + dec-ref
hot loop is bypassed during replay, replay cost converges on the
"scroll only, no scrollback compression" cost.

### TS-7 attribution detail (all three configurations)

| Configuration                                                  | per-call     | Threshold | Result |
| -------------------------------------------------------------- | ------------ | --------- | ------ |
| baseline (scroll + scrollback compression)                     | 4094.61 ms   | reported only | n/a |
| scroll only (no scrollback compression) — `assert!`-gated      | 51.62 ms     | < 200 ms  | PASS |
| no scroll at all (huge grid)                                   | 526.55 ms    | reported only | n/a |

The "scroll only" branch matches the post-feature TS-6 number (51.62 ms vs
51.32 ms), corroborating that the bypass is doing exactly what FR1 specifies.

### Raw bench output (key lines)

```
[bench] build_from_snapshot 2MiB seq-N payload (200x50, 10k scrollback):
  3 iters / 153.967489ms → 51.322496ms/call (39.0 MiB/s)
[bench] process_pty_data_fully  2MiB seq-N payload (200x50, 10k scrollback):
  3 iters / 12.246439136s → 4.082146378s/call (0.5 MiB/s)

[bench] baseline (scroll + scrollback compression)       (200x50, sb=10000, payload=2048 KiB):
  3 iters / 12.283833729s → 4.094611243s/call (0.5 MiB/s)
[bench] scroll only (no scrollback compression)          (200x50, sb=    0, payload=2048 KiB):
  3 iters / 154.869651ms → 51.623217ms/call (38.7 MiB/s)
[bench] no scroll at all (huge grid)                     (200x65000, sb=0, payload=380 KiB):
  3 iters / 1.579654007s → 526.551335ms/call (0.7 MiB/s)

[bench] strip_replayable_rich_content 2MiB plain:
  5 iters / 12.50819ms → 2.501638ms/call (799.5 MiB/s)
[bench] ScrollbackRingBuffer::read_all 2MiB wrapped:
  50 iters / 3.266663ms → 65.333µs/call (30612.3 MiB/s)
```

Note: `process_pty_data_fully` in the same TS-6 invocation still reports
~4082 ms/call — this is the *non*-bypassed live path measured for
attribution purposes, NOT a regression. Only `build_from_snapshot`
enables the bypass.

---

## 3. SPEC.md Success Criteria Status

| ID   | Criterion                                                                    | Method of verification                                              | Status |
| ---- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------- | ------ |
| SC-1 | All functional requirements (FR1–FR5) implemented                            | VERIFICATION.md Functional Requirements Coverage table; code spot-check confirms `scrollback_bypass` + `virtual_scrollback_len` on `TerminalCore` and bypass branch in `ring_push_blank`; bench `assert!` thresholds present in 4 sites | PASS (auto) |
| SC-2 | All test scenarios TS-1..TS-12 pass                                          | TS-1..TS-5, TS-10, TS-11, TS-13, TS-14, TS-15 covered by sdd.5 unit test run (all green); TS-6..TS-9 verified here; TS-12 manual | PASS (auto) for TS-1..TS-11, TS-13..TS-15; **TS-12 pending user** |
| SC-3 | `cargo test --manifest-path src-tauri/Cargo.toml` green                      | sdd.5-check (1889 passed, 0 failed under `--test-threads=1`)        | PASS (auto, prior phase) |
| SC-4 | `cargo check --no-default-features` green                                    | sdd.5-check                                                          | PASS (auto, prior phase) |
| SC-5 | MUST perf goal (< 1000 ms) achieved on local machine                         | TS-6 here: 51.32 ms (~79× speedup vs 4040 ms baseline)               | PASS (auto) |
| SC-6 | Manual TS-12 qualitatively confirms predicted improvement                    | User-side check; manual                                              | **Pending user (TS-12)** |
| SC-7 | `memory/project_mux_output_pipeline_perf.md` updated                         | User-side memory-file update                                         | **Pending user** — suggested content in §6 below |

---

## 4. File Structure Verification

### Files to Modify (from VERIFICATION.md plan)

All 5 planned files were modified and exist:

| File                                            | Modified | Exists |
| ----------------------------------------------- | -------- | ------ |
| `crates/term_core/src/ring_buffer.rs`           | yes      | yes    |
| `crates/term_core/src/terminal_core.rs`         | yes      | yes    |
| `crates/term_core/src/bench.rs`                 | yes      | yes    |
| `src-tauri/src/mux/scrollback_filter.rs`        | yes      | yes    |
| `src-tauri/src/mux/scrollback_buffer.rs`        | yes      | yes    |

### Additional File Modified (per IMPLEMENTATION.md deviation)

| File                                  | Reason                                                                                       | Modified | Exists |
| ------------------------------------- | -------------------------------------------------------------------------------------------- | -------- | ------ |
| `crates/term_core/src/snapshot.rs`    | Added `scrollback_bypass: false` and `virtual_scrollback_len: 0` to the two literal initializers in `from_snapshot_v2` and `from_snapshot_v1` (mechanical fixup for the two new `RingBuffer` fields). | yes      | yes    |

`git status` snapshot at verification time (only feature-related files):

```
 M crates/term_core/src/bench.rs
 M crates/term_core/src/ring_buffer.rs
 M crates/term_core/src/snapshot.rs
 M crates/term_core/src/terminal_core.rs
 M src-tauri/src/mux/scrollback_buffer.rs
 M src-tauri/src/mux/scrollback_filter.rs
?? doc/tasks/snapshot-replay-perf/
```

No collateral changes to unrelated files.

### Code presence spot-checks

- `crates/term_core/src/terminal_core.rs:243` — `pub(crate) scrollback_bypass: bool,`
- `crates/term_core/src/terminal_core.rs:252` — `pub(crate) virtual_scrollback_len: u32,`
- `crates/term_core/src/terminal_core.rs:659` — `self.scrollback_bypass = true;` (enable in replay)
- `crates/term_core/src/terminal_core.rs:669` — `self.scrollback_bypass = false;` (disable post-replay)
- `crates/term_core/src/ring_buffer.rs:136` — `if self.scrollback_bypass { ... }` (bypass branch in `ring_push_blank`)
- `crates/term_core/src/ring_buffer.rs:537–538` — `get_scrollback_length` returns `virtual_scrollback_len` while bypass is on (FR1 mark-stamping invariant)
- `crates/term_core/src/bench.rs:212` — `assert!(per < threshold, ... )` TS-6 1000 ms
- `crates/term_core/src/bench.rs:322` — `assert!(per < threshold, ... )` TS-7 scrollback-disabled 200 ms
- `src-tauri/src/mux/scrollback_filter.rs:421` — `assert!(per < threshold, ... )` TS-8 30 ms
- `src-tauri/src/mux/scrollback_buffer.rs:240` — `assert!(per < threshold, ... )` TS-9 1 ms
- `crates/term_core/src/terminal_core.rs:1071` — `test_build_from_snapshot_restores_scrollback_capacity` (TS-5)
- `crates/term_core/src/terminal_core.rs:1128` — `test_build_from_snapshot_bypass_preserves_evicted_total` (TS-13)
- `crates/term_core/src/terminal_core.rs:1161` — `test_build_from_snapshot_bypass_preserves_mark_stamping` (TS-15)

---

## 5. Manual Testing Checklist (verbatim from VERIFICATION.md)

### TS-12 — Manual (E2E Not Possible)

- [ ] **TS-12**: tab-switch into heavy-output mux tab feels comparable to tmux.
  **Procedure**: `make build`; launch mux; in tab A run
  `seq 1 10000000` to completion; switch to tab B; switch back to tab A.
  **Acceptable**: no multi-second stall, viewport restores promptly.

---

## 6. Open Items

### SC-7 — `memory/project_mux_output_pipeline_perf.md` update

The current memory file says (paraphrased from MEMORY.md):

> 切替2-3秒も2MiB再送+リプレイ機構のオーバーヘッド。改善はclient側coalesceが筆頭

This is now partially outdated: the snapshot-replay portion of "切替2-3秒"
has been reduced from ~4040 ms to **~51 ms** (~79× speedup). Suggested
addendum (Japanese, to fit the existing memory file style):

```
- snapshot-replay コスト (2 MiB seq-N payload, 200×50, scrollback=10000) を
  4040 ms → 51 ms (~79×) に短縮 (2026-06-21, doc/tasks/snapshot-replay-perf)。
  build_from_snapshot に scrollback 圧縮バイパスを導入し、replay 中は
  ring_push_blank の SlimCell intern + scrollback deque 操作を skip。
  observable bookkeeping (evicted_total / 既存 marks の abs_row /
  get_scrollback_length) は byte-identical。post-replay の scrollback は
  空 (capacity だけ復元され、以降の live PTY 出力で正常に蓄積)。
- TS-6 < 1000 ms (MUST), TS-7 scrollback-disabled < 200 ms, TS-8 < 30 ms,
  TS-9 < 1 ms の assert ガードを bench に追加して以後の regression を CI で
  検出可能に。
- 切替体感の残骸 (~2-3 秒) は 2 MiB 再送 + base64 + parse#2 などの
  パイプライン側に残っており、本タスクの範囲外。
```

### TS-12 — Manual confirmation

Pending user execution (see §5). Per SPEC.md US1 acceptance criterion #2,
switching into a tab that ran `seq 1 10000000` should be visibly
comparable to tmux. The TS-6 51 ms perf number is a strong predictor that
the user-facing stall will be gone, but only TS-12 confirms it
qualitatively.

---

## 7. Deviations from Plan

Two deviations are documented in IMPLEMENTATION.md / VERIFICATION.md and are
both satisfied:

1. **`snapshot.rs` literal init fixup** (`scrollback_bypass: false,
   virtual_scrollback_len: 0,` in two places). Mechanical compile-time
   consequence of adding two new fields to `TerminalCore` — required so
   that `from_snapshot_v1` / `from_snapshot_v2` still construct the literal
   `TerminalCore { .. }`. No behavior change.

2. **`grid_fingerprint` helper in `terminal_core::tests`** — omits
   `core.get_scrollback_length()` from the fingerprint so that
   `test_build_from_snapshot_matches_reset_and_replay` (TS-1) keeps passing
   under the FR2 spec change. The built core's `scrollback_count() == 0`
   by design (FR2: capacity restored but contents empty), while the
   sync-path core retains scrollback rows. Mark stamping correctness is
   verified independently in TS-13/TS-15 (`evicted_total` and `abs_row`
   byte-identity across the two paths), so this helper change does NOT
   weaken coverage.

Both deviations are bounded, justified, and orthogonal to the perf goal.
No further deviations were introduced during sdd.6-verify.

---

## 8. Verification Boundary

This document covers sdd.6-verify scope only:
- Performance bench execution (TS-6..TS-9)
- File structure cross-check vs VERIFICATION.md
- SPEC.md Success Criteria cross-reference
- Manual testing item extraction (TS-12)

Items already verified by sdd.5-check (and NOT re-run here):
- `cargo check` (default + `--no-default-features` + term_core standalone)
- `cargo test --lib` (term_core: 672 passed; src-tauri: 1889 passed under `--test-threads=1`)
- `cargo fmt --check` (touched files only, per project policy)

Release-build verification (`make build` / `cargo build --release`) is
explicitly the user's responsibility per project rule
`.claude/rules/build-location.md`.
