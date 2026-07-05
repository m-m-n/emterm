# Implementation Plan: GUI Render CPU Optimization

## Overview

Eliminate the three confirmed CPU hotspots (dead dirty-row skip, full per-frame
cell reprocessing, permanent 60Hz polling) in the wgpu render path and the
winit event loop, verified by unit tests on the CPU-side logic and before/after
`/proc` sampling.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate, `gui` feature)
- **Key libraries**: winit (event loop), wgpu (grid pass), egui (overlay),
  swash (shaping) — all already in use; no new dependencies.

## Layer Structure

Rendering stays two-layered (unchanged direction of dependency):

1. **wgpu grid pass** (`render/terminal_grid_pass.rs`) — instanced cell quads,
   1 draw call. Consumes `CellInput` built by `render/mod.rs`.
2. **egui overlay** (`render/mod.rs` + `render/cursor.rs`) — tab bar, status
   bar, cursor, IME preedit, selection chrome. Drawn with `LoadOp::Load` on
   top of the grid.

Cross-task invariant introduced by this feature: **grid instance data is a
pure function of terminal content + theme + selection/hover/search state —
never of cursor position, cursor blink phase, or window focus.** All cursor
visuals (including the filled block shape) belong to the egui overlay layer.
This invariant is what makes per-row caching and honest dirty tracking
possible; every task must preserve it.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `App::dirty_rows_this_frame` | compute rows needing repaint | post: empty on a frame with no content / cursor / selection / status change; sorted + deduplicated | task0002 (owner), task0003 (consumer semantics), task0004 (wakeup consumer) |
| Draw-skip decision | decide "skip this frame" from dirty count + status-bar change | pure function; no side effects; testable without a window | task0002 (owner), task0004 (relies on skip staying honest) |
| Per-row instance cache | reuse `CellInstance` rows across frames | post: concatenation of cached + rebuilt rows is byte-identical to a full rebuild of the same state | task0003 (owner) |
| Perf counters (`EMTERM_RENDER_PERF`) | frames-drawn / rows-rebuilt counters at warn level | env checked once (same idiom as `EMTERM_FONT_PERF`); zero overhead when unset | task0002 (frames), task0003 (rows) |

## Conventions

- Perf logging: `[EMTERM_RENDER_PERF]` prefix, `log::warn!` level, env gate
  read once and cached — mirrors the existing `EMTERM_FONT_PERF` idiom in
  `render/font/cache.rs` / `render/font/resolver.rs`.
- GUI-only code stays behind the existing `gui` feature gating; nothing in
  this feature may leak into the `--no-default-features` build.
- Tests: CPU-side logic only, in `--lib` unit tests next to the code they
  cover (existing style: `#[cfg(test)]` modules). No wgpu device in tests.
- Comment / naming style: match the surrounding heavily-documented style of
  `window_host.rs` / `render/mod.rs`.

## Cross-task Design Decisions

### D1: Block cursor moves to the overlay before dirty tracking is fixed

The unconditional cursor-row dirty push exists *because* the block cursor is
baked into grid instances (grid content changes whenever the cursor moves or
blinks). Removing the bake-in (task0001) is therefore a strict prerequisite of
the honest dirty set (task0002). Wave order encodes this.

### D2: Dirty-row information flows into the render path

Today `dirty_rows_this_frame` is only used as a boolean gate (count == 0 →
skip). Stage 2 (task0003) additionally consumes the actual row set to decide
which cached rows to rebuild. The dirty set therefore must stay **sound**
(never omit a changed row) — over-approximation is acceptable, silence is not.
This soundness rule binds task0002 and task0003.

### D3: Invalidation vocabulary for the row cache

Fixed rules (from SPEC FR3): scroll → all rows dirty; resize / font / theme
change → full cache drop; selection / hover / search highlight change →
affected rows dirty; fold layout active + selection present → all rows dirty
(matches the existing conservative fallback in `dirty_rows_this_frame`).

### D4: Wakeup sources replace the 16ms rearm

True `ControlFlow::Wait` is entered only when no timed work is pending. Timed
work keeps using `WaitUntil` with an explicit deadline: cursor blink (only
when blink enabled + window focused), visual-bell decay, active toasts.
Event-driven wakeups use the existing `EventLoopProxy` user-event channel
(already proven by the status-bar provider wake chain): PTY output readers,
child-window IPC, settings-save watcher. User input wakes winit natively.

### D5: Design provenance (SPEC NFR3)

Approach validated against Alacritty (damage tracking) and WezTerm (shaped
line cache / event loop) official documentation and issue trackers at plan
time; findings recorded in `research/benchmark-validation.md` in this feature
directory. No code from either project is read or reused; implementers work
from the task plans only.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Row-cache invalidation gap → stale cells on screen | medium | high | equivalence tests (TS-4) + invalidation matrix (TS-5); D2 soundness rule; conservative full-invalidation fallbacks |
| Overlay block cursor differs visibly from baked-in inversion | medium | medium | TS-2 geometry tests; TS-9 visual pass at final verify |
| Missed wakeup source → input/IME/PTY latency or frozen UI | medium | high | D4 enumerates every existing `about_to_wait` responsibility; TS-8 idle check + TS-9 interactive pass |
| Blink/bell/toast timers stop firing under true Wait | medium | medium | timed-work deadlines stay on `WaitUntil` (D4); explicit edge-case test: blink disabled → no periodic wakeups |
| Serial waves (all tasks touch `window_host.rs`) slow delivery | high | low | accepted; tasks kept small and strictly ordered |

## Open Questions

- [ ] None at plan time.
