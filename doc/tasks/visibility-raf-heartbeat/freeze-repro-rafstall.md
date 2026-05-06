# Manual repro procedure: rAF stall hidden detection

This document captures the manual repro recipes that exercise the
`visibility-raf-heartbeat` feature on a real Linux desktop. Each
scenario has explicit pre-conditions, steps, expected log evidence,
and a pass criterion. Use it whenever a freeze regression is suspected
or after material changes to `VisibilityController`.

## Common setup

1. Build a release binary: `bun tauri build` (or use a freshly installed
   `.deb` / `.rpm`).
2. Launch eMterm and confirm the terminal accepts input.
3. Tail the log file in another terminal:
   ```
   tail -F ~/.local/share/net.laser5.app.emterm/logs/emterm.log
   ```
4. In the emterm pane, start a busy producer so backend bytes would
   accumulate if the hidden short-circuit failed:
   ```
   yes hb-payload | head -c 50000000 > /dev/null &
   ```

## Pass criteria (apply to every scenario unless noted)

- `[DIAG-IDLE] visibility→hidden ... reason=raf-stall` appears within
  ~12 s of the hidden trigger (10 s health-tick + ~1 s hide debounce +
  slack).
- No `backpressure stalled` warnings appear in the log for the duration
  of the hidden state.
- On returning to the foreground, `[DIAG-IDLE] visibility→visible`
  appears within ~100 ms of the rAF resumption.
- The terminal viewport reflects the latest PTY output (no stale
  pre-hide screen).

## Scenarios

### S1: Workspace switch

Pre-conditions: a multi-workspace desktop (GNOME / KDE / Sway etc.)
with at least two workspaces.

Steps:
1. Confirm eMterm is on workspace 1 and visible.
2. Switch to workspace 2 (`Super+Page Down`, `Super+2`, or equivalent).
3. Wait at least 15 s.
4. Switch back to workspace 1.

Expected log evidence:
```
grep "reason=raf-stall" ~/.local/share/net.laser5.app.emterm/logs/emterm.log
grep "backpressure stalled" ~/.local/share/net.laser5.app.emterm/logs/emterm.log   # must be empty
```

Pass criterion: hidden line with `reason=raf-stall` recorded; visible
line on return; no backpressure stall.

### S2: Occluded window

Pre-conditions: another application window large enough to fully cover
the eMterm window.

Steps:
1. Place the other window over eMterm so eMterm is fully covered (no
   pixel of the eMterm window is visible).
2. Keep the cover in place for at least 15 s.
3. Bring eMterm back to the front.

Pass criterion: same as S1 — `reason=raf-stall` recorded, no
backpressure stall, screen up to date on return.

Notes: some compositors do NOT report occlusion via
`visibilitychange`; the rAF heartbeat path is what catches this case.

### S3: Screen lock

Pre-conditions: a screen lock is configured (`loginctl lock-session`,
GNOME `Super+L`, etc.).

Steps:
1. With eMterm visible and the busy producer running, lock the screen.
2. Leave the screen locked for at least 30 s (long enough that any
   wallpaper-only fallback would also throttle rAF).
3. Unlock.

Pass criterion: hidden `reason=raf-stall` recorded during the locked
period (timestamps in the log fall between lock and unlock); visible
on unlock; no `backpressure stalled` accumulation.

Notes: some lockers may also fire `visibilitychange`; in that case the
reason field may include `document` instead of `raf-stall`. Both forms
prove the hidden short-circuit engaged.

### S4: Laptop suspend / resume

Pre-conditions: a laptop with a working suspend (`systemctl suspend`,
lid close, etc.).

Steps:
1. With eMterm visible and the busy producer running, suspend the
   system (close the lid).
2. Wait at least 60 s while suspended.
3. Resume the system (open the lid).
4. Inspect the log around the resume timestamp.

Pass criterion: NO spurious `[DIAG-IDLE] visibility→hidden` line is
emitted around the resume timestamp. The first health-tick after
resume falls into the suspend-gap branch (`tick gap > 30 s`) and
quietly resets the rAF baseline. Subsequent ticks return to the
normal cadence.

Failure signature: a hidden line dated immediately after the resume
timestamp followed quickly by a visible line. If this appears,
investigate whether `lastRafPerfMs` reset logic or `nowFn` were
disturbed.

## Quick log-evidence cheatsheet

```
LOG=~/.local/share/net.laser5.app.emterm/logs/emterm.log

# 1. Did rAF stall fire?
grep "reason=raf-stall" $LOG

# 2. Did the multi-cause path fire (workspace switch + focus)?
grep "reason=document+focus" $LOG
grep "reason=document+focus+raf-stall" $LOG

# 3. Backpressure stall — must be empty for a healthy run.
grep "backpressure stalled" $LOG

# 4. Recent hidden / visible transitions.
grep "DIAG-IDLE" $LOG | tail -20
```

## Cross-references

- REQUIREMENTS.md section 11.1 — referenced from this manual procedure.
- SPEC.md FR2 / FR5 / FR6 — define the threshold values exercised by
  these scenarios.
- `e2e-tests/specs/visibility-raf-heartbeat.e2e.js` — automated proxy
  that exercises the same path in a headless environment.
