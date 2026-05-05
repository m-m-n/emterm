# Performance results — visibility-aware PTY streaming

This file records measurements that back the NFR1 / NFR2 / NFR3 success
criteria from SPEC.md. Values come from a mix of automated E2E specs
and manual procedures.

## TS-25 — visible-mode throughput regression (NFR1)

**Spec:** `e2e-tests/specs/visibility-throughput-bench.e2e.js`

**Run command (Docker):**
```
./scripts/run-e2e-docker.sh test visibility-throughput-bench.e2e.js
```

The spec drives a fixed-size shell workload (`for i in $(seq 1 40); do echo bench-line-$i; done`)
and prints the following counters:

- `deltaBytes` — bytes the reader thread sent on the PTY channel during the
  workload (`pty_get_send_stats.bytes` after − before)
- `deltaCount` — number of `channel.send` invocations
- `elapsed_ms` — wall-clock from typing start to settle
- `bytes/sec` — `deltaBytes / elapsed_ms × 1000`

### Latest measurement (visibility-aware-pty-streaming branch)

| Metric | Value |
|---|---|
| deltaCount | 51 |
| deltaBytes | 644 |
| elapsed_ms | 5523 |
| bytes/sec | 117 |

NFR1 calls for ≤ ±5 % regression vs the pre-change baseline. A
formal baseline value should be captured by running the same spec on a
checkout immediately prior to the visibility-aware-pty-streaming
landing commit. Because `bytes/sec` is dominated by the Xvfb display
loop and the `typeSlowly` keystroke pacing (30 ms/char), this number is
useful as a smoke-test rather than a precise micro-benchmark; use the
recorded value as a reference and re-run on the same hardware before
investigating any deviation.

## TS-27 — visible resume main-thread block (NFR2)

**Spec:** `e2e-tests/specs/visibility-resume-block.e2e.js`

**Run command (Docker):**
```
./scripts/run-e2e-docker.sh test visibility-resume-block.e2e.js
```

The spec hides the active session, drives a small workload through the
shell so the shadow parser holds non-trivial state, then brackets the
`pty_set_visibility(true)` invoke + two rAF ticks with
`performance.now()` to capture the end-to-end resume cost.

### Latest measurement

| Metric | Value |
|---|---|
| resumeMs (single-pane, ~30 lines hidden workload) | 27.00 |
| NFR2 budget | 200 |

The measurement is well within the 200 ms NFR2 budget. The spec's hard
ceiling is intentionally relaxed to 1000 ms to absorb Xvfb / Docker
jitter; review the printed `resumeMs` against the 200 ms target when
auditing manually.

**Manual fallback procedure** (when running outside the E2E harness):

1. Build the app: `bun tauri build --debug --no-bundle`.
2. Launch and open one tab; let the prompt settle.
3. From the developer machine, invoke:

   ```js
   const sid = window.terminalApp.ptyClient.getSessionId();
   await window.__TAURI_INTERNALS__.invoke("pty_set_visibility", { sessionId: sid, visible: false });
   await new Promise((r) => setTimeout(r, 5000));
   const t0 = performance.now();
   await window.__TAURI_INTERNALS__.invoke("pty_set_visibility", { sessionId: sid, visible: true });
   await new Promise((r) => requestAnimationFrame(() => r()));
   await new Promise((r) => requestAnimationFrame(() => r()));
   console.warn(`[NFR2] resumeMs=${(performance.now() - t0).toFixed(2)}`);
   ```

4. Inspect frontend logs for the next `[DIAG-PTY-HEALTH]` line after
   resume; `loopLag` (timer skew) and `rafMaxGap` reflect the
   main-thread block from snapshot delivery.

## TS-26 — hidden 1-hour backend RSS growth (NFR3)

**Tooling:** `scripts/measure-hidden-rss.sh` (created below).

**Procedure (manual, requires ~1 hour wall-clock):**

1. Launch the dev build (`bun tauri dev`) and let it settle for ~30 s.
2. In a side terminal, `pgrep -f 'src-tauri/target/.*emterm$'` to find
   the eMterm pid (use the process owning the Tauri webview, not the
   `bun` wrapper).
3. Run `./scripts/measure-hidden-rss.sh <PID>` in another terminal.
4. Hide the eMterm window (minimize / occlude). Leave it hidden for at
   least 60 minutes.
5. Stop the script (Ctrl-C) — it will leave a CSV at
   `tmp/hidden-rss-<pid>-<timestamp>.csv`.
6. Compute `delta = max(VmRSS) − initial(VmRSS)` over the run.

**Pass criterion (NFR3):** delta < 10 MiB for a 1-session non-mux setup
or 1–2 active mux panes. Document the actual measurement here when
performed:

| Date | Session type | Initial (KiB) | Peak (KiB) | Delta (KiB) | Pass |
|------|--------------|---------------|------------|-------------|------|
| TBD  | non-mux      |               |            |             |      |
| TBD  | mux 2-pane   |               |            |             |      |
