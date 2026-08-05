# Feature: mux-tab-switch-bypass-refix

## Overview

This feature is a fix-forward continuation of the merged
`mux-tab-switch-replay-latency` feature (PR #12, merged as `1b6d2bd`). The
shipped bypass-split gate rejects the scrollback shape that was actually
measured in the bug — after head folding the shape has
`middle_segment_count = 26`, while the gate admits at most 24 — so that
feature's FR1/FR2 latency goal does not hold for the real workload. In
addition, four review-round-2 high findings were closed as "deferred (batch
mode: rework cap reached)", and the round-2 same-round auto-fix of a
critical finding was never re-reviewed by a fresh pass.

Requirements source: `feature-docs/mux-tab-switch-bypass-refix/REQUIREMENTS.md`.

## Objectives

- Make the prior feature's FR1/FR2 latency goal actually hold for the real
  measured scrollback shape: switching to a 2 MiB heavy pane with a
  tail-adjacent resize-marker cluster completes in the tens-of-ms order.
- Resolve all four review-round-2 high findings that were closed as
  "deferred (batch mode: rework cap reached)": `b6a60c440da70e79`,
  `81507f39e384b34e`, `a82206113b8160fd`, `aba5ebbdf9a9addb`.
- Confirm the correctness of the round-2 same-round auto-fix of critical
  finding `5c6ae6b507b6f638` (D8 empty-MIDDLE), which was never re-reviewed
  by a fresh pass.

## User Stories

### US1: Fast switch to the real measured heavy-pane shape
As a mux user, I want switching to a pane whose scrollback matches the
actually measured bug shape to complete in the tens-of-ms order, so that
the merged feature's latency goal holds for the workload it was written
for.

**Acceptance Criteria:**
- [ ] A replay of the real measured shape (2 MiB payload, 31 segments,
  `k=27`, `middle_segment_count=26` after head fold) engages the split (or
  otherwise completes within the ceiling) — a unit test at the `term_core`
  level demonstrates engagement, and a release-mode bench asserts the
  latency ceiling relative to bypass-engaged cost.
- [ ] The new bench fixture reproduces 26 MIDDLE segments; the existing
  24-segment bench remains green unchanged.

### US2: The resize-settle window does not burn CPU
As a mux user, I want the render loop to stay bounded while the resize
settler is awaiting its decision, so that startup and every
attach/reattach/tab-switch does not spin the render loop at full frame
rate against the off-thread snapshot replay worker.

**Acceptance Criteria:**
- [ ] While the settler awaits a decision, redraw self-wakes are
  rate-limited (unit-testable predicate in `window_host.rs`, mirroring
  `toast_redraw_due`'s testable form) and the settler still reaches a
  decision on an idle window within `RESIZE_SETTLE_MAX_DURATION`.

### US3: Status-bar insets stay in sync with the status-bar height
As a mux user, I want a status-bar height change to take effect even when
it does not change the derived grid size, so that terminal content and
mux-sidebar pointer routing do not use a permanently stale inset.

**Acceptance Criteria:**
- [ ] A status-bar height change with an unchanged derived grid size
  applies the new inset values (unit test on the inset-application
  predicate); no PTY reshape storm is reintroduced (prior FR6 stays green).

### US4: The un-re-reviewed critical fix is confirmed correct
As a maintainer, I want the round-2 same-round auto-fix of the D8
empty-MIDDLE critical to receive a regression pin and a fresh review pass,
so that a fix that shipped without re-review is not carried forward
unverified.

**Acceptance Criteria:**
- [ ] A regression test pins the `h == k` empty-MIDDLE shape: built core at
  caller-requested dims with reference-matching `scrollback_populated`.
- [ ] All four deferred round-2 high findings and the un-re-reviewed
  critical are addressed and pass this feature's own review.

### US5: The merged feature's guarantees still hold
As a mux user, I want the guarantees the merged feature already delivered
to remain intact, so that this fix-forward round does not trade one
regression for another.

**Acceptance Criteria:**
- [ ] Ordinary-switch bench baseline does not regress; bypass equivalence
  tests and `snapshot_replay_bench_2mib_seq` stay green.

## Technical Requirements

### Functional Requirements

- **FR1 — Real measured marker-cluster shape (26-segment MIDDLE) engages
  the split and meets the latency goal:** A pane whose scrollback matches
  the actually measured bug shape (2 MiB payload, 31 segments, `k=27`;
  adjacent scrollback-marker dims always differ, so head folding leaves
  `middle_segment_count = 26`) replays in the tens-of-ms order, on par with
  bypass-engaged replay for the same payload size. The current gate
  `middle_segment_count <= BYPASS_PREFIX_MAX_SEGMENTS` (24,
  `crates/term_core/src/terminal_core.rs:1244`/`2166`) rejects this shape,
  leaving the ~800–1000 ms full non-bypass drain (finding
  `b6a60c440da70e79`). The fix must honor NFR1 (no reintroduced double
  2nd-pass cost).
- **FR2 — Regression bench reproduces the SPEC-cited 26-segment cluster:**
  A bench/regression test reproduces the SPEC-cited 26-segment
  marker-cluster shape (not the 24-segment fixture that
  `marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost` in
  `crates/term_core/src/bench.rs:286` deliberately narrowed to) and asserts
  a latency ceiling consistent with bypass-engaged replay cost.
- **FR3 — ResizeSettler self-wake is rate-limited:** While
  `ResizeSettler::awaiting_decision()` is true, `refresh_status_bar_insets`
  (`src-tauri/src/window_host.rs:1270-1271`) must not call
  `request_redraw()` unconditionally on every frame. The self-wake is
  rate-limited (analogous to the file's existing `toast_redraw_due`
  pattern, `window_host.rs:2477`) so the render loop does not spin at full
  speed for up to `RESIZE_SETTLE_MAX_DURATION` (1 s) during startup and
  every mux attach/reattach/tab-switch, while an otherwise idle window
  still reliably reaches the settler's decision (finding
  `81507f39e384b34e`; must not regress round-1 findings
  `02546e5e10deb500` / `5b1878c41d3e02d6-perf-P2`, which the unconditional
  wake was added to fix).
- **FR4 — Status-bar inset application is not gated on a derived grid-size
  change:** A status-bar height change that leaves the derived
  `(cols, rows)` candidate unchanged (e.g. cell height above `ROW_HEIGHT`
  22.0 with larger font sizes, or row clamping) still applies
  `status_bar_top_inset_logical` / `status_bar_bot_inset_logical`.
  Currently the assignment happens only when
  `resize_settler.observe(candidate)` returns `Some`
  (`src-tauri/src/window_host.rs:1265-1269`), so such a change leaves the
  inset permanently stale, also affecting mux-sidebar pointer routing which
  reads the same field (findings `a82206113b8160fd` /
  `aba5ebbdf9a9addb` — same mechanism, one fix).
- **FR5 — D8 empty-MIDDLE auto-fix confirmed correct under fresh review:**
  The round-2 same-round auto-fix for critical `5c6ae6b507b6f638` — the
  `&& candidate_h < k` guard degrading the `h == k` (empty MIDDLE) shape to
  the pre-D7 path (`crates/term_core/src/terminal_core.rs:1221`) — is
  confirmed correct: for that shape the built core comes out at the
  caller's target dims with the correct `scrollback_populated`, a
  regression test covers it, and the code receives a fresh review pass
  within this feature (the prior round's rework cap prevented re-review).
- **FR6 — Prior feature's acceptance criteria hold as non-regression:** The
  merged feature's remaining guarantees stay intact: ordinary switch
  latency does not regress from the 1.57 ms baseline; bypass equivalence is
  preserved (viewport/cursor parity with the non-bypass path,
  `scrollback_populated` meaning unchanged); the `visible_row_count` 0→1
  transition does not broadcast `Resize` to all mux panes; a grid resize
  racing an in-flight switch does not defeat bypass via target-dims
  mismatch; consecutive same-pane switches do not rebuild the off-thread
  replay twice (prior FR8 scope: decode/daemon fetch dedup remains out of
  scope).

### Non-Functional Requirements

- **NFR1 - Performance (no double 2nd-pass non-bypass cost):** The FR1 fix
  must not reintroduce the double non-bypass replay cost that
  `BYPASS_PREFIX_MAX_BYTES` (64 KiB) and `suffix_len >= split_at` (now
  `suffix_len >= middle_len`) were added in round-7/round-8 review to
  prevent: loosening gates without making the prefix side cheap to replay
  is out of bounds (carried over from prior SPEC NFR1).
- **NFR2 - Performance (bounded render-loop CPU during the settle
  window):** During the resize-settle window, self-wake redraws are bounded
  to a modest rate rather than the display's full frame rate, so the settle
  window does not compete with the off-thread snapshot replay worker for
  CPU.
- **NFR3 - Maintainability (existing test and bench guards stay green):**
  `snapshot_replay_bench_2mib_seq` (`crates/term_core/src/bench.rs:169`)
  and the existing `--lib` suites for `src-tauri` and `crates/term_core`
  remain green (`tabs.rs` replay tests may need `-- --test-threads=1` per
  `test/README.md`; 7 `tabs.rs` off-thread tests are chronically flaky on
  this host even on main, per the prior retrospect).

## Implementation Approach

### Baseline

The scope baseline is the merged PR #12 code in `main` (merge `1b6d2bd`),
confirmed by direct inspection: the D7/D8 gate, `ResizeSettler`, and the
`candidate_h < k` guard are all present in the integration worktree. This
is a fix-forward feature, not a from-scratch re-implementation.

### Defect 1 — the segment-count gate rejects the measured shape (FR1, FR2)

The measured payload is 2 MiB across 31 segments with `k = 27`. Because
adjacent scrollback-marker dims always differ, head folding cannot collapse
the cluster below 26, so `middle_segment_count = 26`. The gate
`middle_segment_count <= BYPASS_PREFIX_MAX_SEGMENTS` (24) at
`crates/term_core/src/terminal_core.rs:1244`/`2166` therefore rejects the
very shape the merged feature was written for, and the switch falls back to
the full non-bypass drain (~800–1000 ms). The existing bench
`marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost`
(`crates/term_core/src/bench.rs:286`) was deliberately narrowed to a
24-segment fixture and so does not catch this.

The fix is constrained by NFR1: the gate exists together with
`BYPASS_PREFIX_MAX_BYTES` (64 KiB) and `suffix_len >= middle_len` to stop a
large prefix from engaging the split only for the 2nd-pass worker to pay
the same non-bypass cost again. Loosening the gates without making the
prefix side cheap to replay is out of bounds.

### Defect 2 — unconditional settler self-wake (FR3, NFR2)

`refresh_status_bar_insets` (`src-tauri/src/window_host.rs:1270-1271`)
calls `request_redraw()` on every frame while
`ResizeSettler::awaiting_decision()` is true, so the render loop spins at
full speed for up to `RESIZE_SETTLE_MAX_DURATION` (1 s) at startup and on
every mux attach/reattach/tab-switch. The wake must become rate-limited —
the file already has a testable precedent in `toast_redraw_due`
(`window_host.rs:2477`) — while still guaranteeing that an otherwise idle
window reaches the settler's decision. The unconditional wake was itself
introduced to fix round-1 findings `02546e5e10deb500` and
`5b1878c41d3e02d6-perf-P2`, which must not regress.

### Defect 3 — inset application gated on a derived grid-size change (FR4)

The inset assignment at `src-tauri/src/window_host.rs:1265-1269` happens
only when `resize_settler.observe(candidate)` returns `Some`. A status-bar
height change whose derived `(cols, rows)` candidate is unchanged (cell
height above `ROW_HEIGHT` 22.0 with larger font sizes, or row clamping)
therefore leaves `status_bar_top_inset_logical` /
`status_bar_bot_inset_logical` permanently stale, which also mis-routes
mux-sidebar pointer input because it reads the same field. Findings
`a82206113b8160fd` and `aba5ebbdf9a9addb` are one defect reported from two
review perspectives and are satisfied by a single fix.

### Defect 4 — un-re-reviewed critical auto-fix (FR5)

The `&& candidate_h < k` guard at
`crates/term_core/src/terminal_core.rs:1221` degrades the `h == k` (empty
MIDDLE) shape to the pre-D7 path. It was applied as a same-round auto-fix
for critical `5c6ae6b507b6f638` and never re-reviewed because the prior
round hit its rework cap. This feature pins the shape with a regression
test and puts the code through a fresh review pass.

### Affected Components

- `crates/term_core/src/terminal_core.rs` — the D7/D8 split gate
  (`:1221`, `:1244`, `:2166`).
- `crates/term_core/src/bench.rs` — `:169`
  (`snapshot_replay_bench_2mib_seq`), `:286`
  (`marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost`), plus the
  new 26-segment fixture.
- `src-tauri/src/window_host.rs` — `:1265-1269` (inset application),
  `:1270-1271` (settler self-wake), `:2477` (`toast_redraw_due` precedent).

### Out of Scope

Per the task description: full-reflow cost reduction for cols-changing
resize storms, client-side `PtyOutput` coalesce, the `[osc-probe gui]`
release-build remnant, settings-window child-process zombies, and
decode/daemon-fetch dedup beyond the prior FR8 scope decision.

## Test Scenarios

### Unit Tests
- [ ] **TS1** (`crates/term_core`, FR1): 26-segment MIDDLE marker cluster
  (adjacent dims all differing, oscillating above the settled target per
  D8's direction) — split engages post-fix; confirmed failing pre-fix.
- [ ] **TS2** (`crates/term_core`, FR1): boundary at the new segment-count
  treatment (exactly-at and one-past bounds), preserving the intent of the
  existing 24-boundary tests.
- [ ] **TS3** (`crates/term_core`, FR5): `h == k` empty-MIDDLE shape returns
  caller target dims and correct `scrollback_populated` (FR5 pin).
- [ ] **TS4** (`src-tauri` `window_host`, FR3 / NFR2): settler-wake
  rate-limit predicate — repeated `awaiting_decision` frames within the
  limit do not request redraw; past the limit they do; decision still
  reached within `RESIZE_SETTLE_MAX_DURATION`.
- [ ] **TS5** (`src-tauri` `window_host`, FR4 / FR6): inset applied when
  height changes but derived grid size does not; inset unchanged and no
  `pending_resize` when nothing changes.

### Performance Tests
- [ ] **TS6** (bench, release, `--include-ignored`, FR2 / NFR1 / NFR3): new
  26-segment-shape bench asserts ceiling vs bypass-engaged cost;
  `snapshot_replay_bench_2mib_seq` and
  `marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost` stay green.

### Manual Tests
- [ ] **TS7** (real machine, carried over from prior VERIFICATION MT-1,
  FR1 / FR6): restart client, reattach to mux daemon, switch to a heavy
  pane — display appears in tens of ms.

### E2E Tests
**Existing E2E tests**: None detected in this repository.
**Run command**: Not applicable.
- [ ] N/A — no existing E2E suite to regress.

## Security Considerations

Not applicable — internal performance/correctness fix in the Rust terminal
core and window host; no new external inputs and no authN/authZ or
data-protection surface touched.

## Performance Optimization

### Performance Goals

- FR1: the real measured shape (2 MiB payload, 31 segments, `k=27`,
  `middle_segment_count=26`) replays in the tens-of-ms order, on par with
  bypass-engaged replay for the same payload size, instead of the current
  ~800–1000 ms full non-bypass drain.
- FR6: ordinary switch latency does not regress from the 1.57 ms baseline.
- NFR2: during the resize-settle window, self-wake redraws are bounded to a
  modest rate rather than the display's full frame rate.

The "tens-of-ms" ceiling is asserted the way the existing benches do — a
bound relative to measured bypass-engaged cost on the same host — rather
than an absolute wall-clock constant, consistent with
`marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost`.

### Optimization Constraint

See NFR1: the FR1 fix must make the prefix side cheap to replay rather than
merely loosening `BYPASS_PREFIX_MAX_BYTES` / `suffix_len >= middle_len`.

## Success Criteria

- [ ] FR1–FR6 implemented and tested.
- [ ] NFR1–NFR3 satisfied.
- [ ] TS1–TS7 pass.
- [ ] All four deferred round-2 high findings (`b6a60c440da70e79`,
  `81507f39e384b34e`, `a82206113b8160fd`, `aba5ebbdf9a9addb`) and the
  un-re-reviewed critical (`5c6ae6b507b6f638`) are addressed and pass this
  feature's own review.
- [ ] Code review is completed.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every requirement is `resolved`; no `tbd` requirements.

## References

- Requirements document: `feature-docs/mux-tab-switch-bypass-refix/REQUIREMENTS.md`
- Prior feature spec: `feature-docs/mux-tab-switch-replay-latency/SPEC.md`
- Prior feature requirements: `feature-docs/mux-tab-switch-replay-latency/REQUIREMENTS.md`
- Prior feature review round 2 (`round2.yaml`) — findings
  `b6a60c440da70e79`, `81507f39e384b34e`, `a82206113b8160fd`,
  `aba5ebbdf9a9addb`, `5c6ae6b507b6f638`
- Prior feature merge: PR #12, merge commit `1b6d2bd`
- `crates/term_core/src/terminal_core.rs:1221` — `candidate_h < k` guard
- `crates/term_core/src/terminal_core.rs:1244`, `:2166` —
  `middle_segment_count <= BYPASS_PREFIX_MAX_SEGMENTS` gate
- `crates/term_core/src/bench.rs:169` — `snapshot_replay_bench_2mib_seq`
- `crates/term_core/src/bench.rs:286` —
  `marker_cluster_tail_bench_2mib_matches_bypass_engaged_cost`
- `src-tauri/src/window_host.rs:1265-1269` — inset application
- `src-tauri/src/window_host.rs:1270-1271` — settler self-wake
- `src-tauri/src/window_host.rs:2477` — `toast_redraw_due` precedent
- `test/README.md` — `--test-threads=1` note for `tabs.rs` replay tests
