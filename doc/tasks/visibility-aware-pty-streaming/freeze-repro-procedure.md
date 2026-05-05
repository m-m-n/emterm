# Manual freeze-reproduction procedure

The visibility-aware streaming work targets a real-world freeze symptom
that only manifests after the eMterm window is hidden for several
minutes (typically a screen lock or another application taking focus
for 10 + minutes). The CI proxy (`freeze-regression.e2e.js` /
`visibility-aware-streaming.e2e.js`) covers the deterministic
mechanism, but a human still needs to confirm the observable user
behaviour on a real desktop. This document is the SOP.

## Pre-conditions

- A debug or release build of eMterm with this branch applied
  (`bun tauri build --debug --no-bundle` or `bun tauri build`).
- Linux (WebKitGTK) for the primary run; repeat on Windows
  (WebView2) for NFR4 compatibility coverage.
- The diagnostic log file is reachable at
  `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.

## Steps

1. **Launch eMterm** and open one tab. Wait for the prompt to settle.
2. **Drive a continuous workload.** Run a command that keeps producing
   PTY output for the full duration of the test:

   ```sh
   while true; do date; sleep 0.1; done
   ```

3. **Hide the window.** Either:
   - Switch focus to another application and ensure eMterm is fully
     occluded, or
   - Lock the desktop session (Super+L on most desktops), or
   - Minimize the window.

4. **Wait 10 + minutes.** 30 minutes is recommended for a thorough
   pass; the original freeze symptom required cumulative `in_flight`
   build-up, which the new architecture should structurally prevent.

5. **Restore focus.** Bring eMterm back to the foreground.

## Observation points

After restoring focus, check the following:

- The terminal screen visibly redraws to the latest `date` output
  immediately. The redraw should be perceived as a single repaint, not
  a multi-second replay of every line generated while hidden.
- Keyboard input responds instantly. Type a few characters and confirm
  echo with no perceptible delay.
- Tab switching, pane switching (in mux mode), and selection respond
  without UI freeze.

## Log-side checks

Tail the log while restoring focus and verify:

```sh
tail -f ~/.local/share/net.laser5.app.emterm/logs/emterm.log
```

Expected entries during / after the hidden window:

- `[DEBUG][BACKEND] visibility: visible -> hidden` (logged once when
  the frontend confirms the hide).
- During the hidden interval, no `[DIAG-PTY-HEALTH]` line should
  report a growing `chunkRecv` counter — frontend should NOT receive
  any chunks (the backend is not sending). `lastChunkAgoMs` will grow
  with each heartbeat. `pending` should stay at 0c/0b.
- On resume: `[DEBUG][BACKEND] visibility: hidden -> visible (building
  snapshot)` followed by exactly one channel.send for the snapshot
  payload.
- No `backpressure stalled` warnings should appear, because the
  backend never queues PTY data while hidden.

## mux variant

Repeat the same procedure with mux mode active and at least two panes
running independent workloads (e.g. one `date` loop, one `htop`).

Additional check for mux:

- After resume, every pane (not only the active one) shows the latest
  state. The mux daemon log under
  `~/.local/share/net.laser5.app.emterm/logs/emterm.log` should
  include `evaluate_output_target` transitions for every active pane.

## Pass criteria (US1, US2, SC-3, SC-5)

- [ ] Hidden ≥ 10 minutes followed by focus return shows the latest
      screen with no perceptible UI block.
- [ ] Mux variant: all panes' latest screens visible after resume.
- [ ] CLI image (`emterm image …`) issued while hidden re-appears
      after resume (within `HIDDEN_PASSTHROUGH_CAPACITY = 4 MiB`
      non-mux / 1 MiB per mux pane).
- [ ] CLI Markdown (`emterm markdown …`) issued while hidden
      re-appears after resume (same capacity rule).
- [ ] No `backpressure stalled` warnings or vt100 panic backtraces in
      the log during the run.
- [ ] Repeat OK on Windows (WebView2).

## Recording the result

Append a row to `perf-results.md` (Hidden RSS table) and to
VERIFICATION.md (Manual Testing section) once the run is complete.
