# Implementation Plan: mux-tab-switch-bypass-refix

## Overview

Fix-forward on the merged `mux-tab-switch-replay-latency` feature (PR #12,
merge `1b6d2bd`): make the bypass split gate admit the real measured
26-segment-MIDDLE scrollback shape (task0001), rate-limit the render loop's
settle-window self-wake and decouple status-bar inset application from the
settler's grid-size decision (task0002), and pin the un-re-reviewed D8
empty-MIDDLE auto-fix with a regression test (task0003).

## Technology Stack

- **Rust** — existing crates only (`crates/term_core`, `src-tauri`). **No
  new dependency is introduced by this feature**; there are no new licenses
  to record, and `project.license: MIT` is unaffected.

## Layer Structure

- `crates/term_core` — pure replay/decision logic: the D7/D8/D9 bypass
  split gate inside `TerminalCore::build_from_snapshot_inner` (including
  `leading_uniform_run_len`, the gate constants, and the `candidate_h < k`
  empty-MIDDLE guard) plus its unit and release-bench coverage
  (`crates/term_core/src/bench.rs`). No I/O, no GUI/mux awareness; it must
  not gain any dependency on `src-tauri`.
- `src-tauri/src/window_host.rs` — GUI render-loop orchestration:
  `refresh_status_bar_insets`, `ResizeSettler`, the redraw self-wake
  policy, and the `status_bar_top_inset_logical` /
  `status_bar_bot_inset_logical` fields (also read by mux-sidebar pointer
  routing).

task0001 and task0003 live entirely in `crates/term_core`; task0002 lives
entirely in `src-tauri/src/window_host.rs`. No runtime interface between
the two sides changes in this feature, so there is no integration wiring to
own beyond each task's own file set.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Bypass split gate (the `h` / `middle_segment_count` / `bypass_split` computation in `TerminalCore::build_from_snapshot_inner`, with `leading_uniform_run_len` and the gate constants) | Decide whether a snapshot replay engages the HEAD/MIDDLE/SUFFIX split or falls back to a full non-bypass drain | Precondition: `segments` reflect the scrollback's recorded resize markers in offset order. Postcondition: the decision stays a pure function of its inputs, and the caller-visible meaning of `bypass_engaged` true/false (viewport/cursor parity, `scrollback_populated` semantics) is unchanged. task0001 may change WHICH shapes engage; it must not change what engagement implies downstream | task0001 (owns every code change), task0003 (tests against it without modifying it) |
| Empty-MIDDLE degradation contract (`h == k` shapes) | When folding the HEAD would leave an empty MIDDLE, the fold is abandoned (pre-D7 path) so the final resize-to-target step always runs | Postcondition (pinned): for an input whose entire pre-suffix region is one uniform run (`h == k` candidate), the built core comes out at the caller-requested `(cols, rows)` and `scrollback_populated` matches a reference non-bypass build of the same payload/segments | task0003 (owns the regression pin), task0001 (must preserve this postcondition while changing the segment-count treatment) |

## Conventions

- Every new or changed gate constant/condition keeps the established
  doc-comment convention of citing the review finding or design decision
  that motivated it (the `BYPASS_PREFIX_MAX_BYTES` /
  `BYPASS_SUFFIX_MIN_BYTES` / D1–D9 lineage style).
- Unit-testable render-loop decisions are extracted as pure predicates —
  plain values in, plain bool out — per the file's `toast_redraw_due` /
  `should_skip_frame` precedent.
- No `log::debug!` / `log::info!` diagnostics may remain in merged code
  (release builds persist only warn and above); temporary TDD
  instrumentation is removed before a task is done.
- New tests are appended to the existing test modules of the touched files,
  following their existing naming and doc-comment style.

## Cross-task Design Decisions

### D-A: single owner for the gate region; task0003 is additive-test-only

task0001 exclusively owns every non-test code change in
`crates/term_core/src/terminal_core.rs`. task0003 touches the same file but
only appends regression tests — it must not modify non-test code. Both
proceed fully in parallel against the Empty-MIDDLE degradation contract
above; any merge conflict between them is a test-module append-level
conflict only. If task0003's pin FAILS against the current code (i.e. the
prior auto-fix is actually wrong), that is a reportable deviation for the
orchestrator — task0003 must not patch gate code itself.

### D-B: NFR1 applies only to task0001

The `BYPASS_PREFIX_MAX_BYTES` (64 KiB) and `suffix_len >= middle_len` gates
exist to keep the 2nd-pass scrollback worker from re-paying a large
prefix's non-bypass replay cost a second time. Whatever new segment-count
treatment task0001 chooses to admit the measured 26-segment MIDDLE, it must
not newly let a genuinely expensive MIDDLE/prefix engage a split whose cost
the 2nd pass then pays again. task0002 and task0003 do not touch this gate
and carry no NFR1 obligation.

### D-C: finding-ID traceability for this feature's review

Each task cites, in code doc comments, the prior-feature round-2 finding
IDs it resolves: task0001 → `b6a60c440da70e79`; task0002 →
`81507f39e384b34e` and `a82206113b8160fd` / `aba5ebbdf9a9addb` (one
mechanism, one fix); task0003 → `5c6ae6b507b6f638`. This traceability is
what lets this feature's own review confirm the SPEC success criterion
"all four deferred round-2 highs and the un-re-reviewed critical are
addressed".

### D-D: drawing insets and PTY reshape are separate concerns

The status-bar drawing inset (`status_bar_top_inset_logical` /
`status_bar_bot_inset_logical`) is applied when the inset values themselves
change, independent of the `ResizeSettler`'s grid-size decision; the PTY
reshape trigger (`pending_resize`, and through it the group-wide `Resize`
broadcast) remains gated by the settler's forwarded grid-size decision.
This split is what resolves FR4 without regressing the prior feature's FR6
(no reshape storm). The mechanism detail is task0002-only content and lives
in its task plan; it is recorded here because task0001's latency goal is
measured in exactly the attach/switch window whose CPU behavior task0002
changes (NFR2), and reviewers need the boundary stated once.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| task0001's new segment-count treatment reintroduces the double 2nd-pass non-bypass cost (NFR1) | Medium | High | Existing large/expensive-prefix rejection tests and benches stay green as explicit ACs; D-B pins the constraint to task0001 |
| task0002's rate limit starves the settler on an idle window, regressing round-1 findings `02546e5e10deb500` / `5b1878c41d3e02d6-perf-P2` | Medium | High | AC requires a simulated idle-window test reaching the decision within `RESIZE_SETTLE_MAX_DURATION`; wake cadence must be compatible with `RESIZE_SETTLE_QUIET_DURATION` quiescence detection |
| task0001/task0003 merge conflict in the shared `terminal_core.rs` test module | Medium | Low | D-A: task0003 is append-only tests; conflicts resolve mechanically via the implementer's parent-side-adoption protocol |
| `tabs.rs` off-thread tests chronically flaky on this host (7 tests, flaky on main too) | High | Low | Verification judges failures against the main baseline per the prior retrospect; not treated as a feature signal |

## Open Questions

None — no `tbd` requirements, no new dependencies, no existing planning
artifacts to merge.
