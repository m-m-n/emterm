/**
 * Diagnostic timers for TerminalApp.
 *
 * Two timers run as long as the terminal is initialized:
 *
 * 1. **Heartbeat** (5s) — emits a single `[DIAG-PTY-HEALTH]` line summarising
 *    main-thread loop lag, max rAF gap, WASM heap size, PTY chunk receive
 *    counters, pending queue depth, and 30s sliding-window counters for
 *    mux switches / slow renders / slow processes. Used to correlate UI
 *    pauses with backend signals.
 *
 * 2. **Event-loop watchdog** (200ms) — records the delta between expected
 *    and actual fire time. When the JS event loop is stuck (e.g. an
 *    11-second hang during a mux-switch storm), the tick that finally runs
 *    after the loop unblocks reports the gap as `[DIAG-EVENTLOOP] hang Xms`.
 *    Without this, long event-loop hangs are invisible because the 5s
 *    heartbeat misses several intervals silently.
 *
 * Extracted from TerminalApp to keep the orchestrator class focused on
 * lifecycle wiring rather than diagnostic plumbing.
 */

import type { ITerminalRenderer } from "../terminal";
import type { PtyClient } from "../pty";
import type { PtyHandlerHandle } from "./pty-handler";
import { getMuxSwitchCountWithin, getLastMuxSwitchAt } from "./mux/mux-window-manager";
import { getSlowRenderCountWithin, getSlowProcessCountWithin } from "./pty-handler";
import { getWasmMemoryBytes } from "../terminal/wasm/loader";

/**
 * Read-only access the diagnostics timers need from the host TerminalApp.
 *
 * Everything is a getter so the controller always reads the live value
 * even after fields rotate during recovery.
 */
export interface DiagnosticsContext {
  getRenderer(): ITerminalRenderer | null;
  getPtyClient(): PtyClient | null;
  getPtyHandlerHandle(): PtyHandlerHandle | null;
  getInMuxMode(): boolean;
  getMuxWindowsLength(): number;
  getActiveMuxWindowIndex(): number;
  getMuxPaneIds(): readonly number[];
}

/**
 * Owns the heartbeat and watchdog timers. `start()` is idempotent (no-op
 * if already running); `stop()` clears both timers and resets the lag
 * accumulator so a future `start()` produces clean numbers.
 */
export class DiagnosticsController {
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private heartbeatLastFiredAt = 0;

  private watchdogTimer: ReturnType<typeof setInterval> | null = null;
  private watchdogLastFiredAt = 0;
  private watchdogMaxLagSinceHeartbeat = 0;

  constructor(private readonly ctx: DiagnosticsContext) {}

  /** Start both timers. Safe to call multiple times. */
  start(): void {
    this.startHeartbeat();
    this.startEventLoopWatchdog();
  }

  /** Stop both timers and reset internal counters. */
  stop(): void {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
    if (this.watchdogTimer) {
      clearInterval(this.watchdogTimer);
      this.watchdogTimer = null;
    }
    this.watchdogMaxLagSinceHeartbeat = 0;
  }

  private startHeartbeat(): void {
    if (this.heartbeatTimer) return;
    this.heartbeatLastFiredAt = performance.now();
    this.heartbeatTimer = setInterval(() => this.fireHeartbeat(), 5000);
  }

  private startEventLoopWatchdog(): void {
    if (this.watchdogTimer) return;
    const INTERVAL_MS = 200;
    const HANG_THRESHOLD_MS = 500;
    this.watchdogLastFiredAt = performance.now();
    this.watchdogTimer = setInterval(() => {
      try {
        const now = performance.now();
        const lag = now - this.watchdogLastFiredAt - INTERVAL_MS;
        this.watchdogLastFiredAt = now;
        if (lag > HANG_THRESHOLD_MS) {
          if (lag > this.watchdogMaxLagSinceHeartbeat) {
            this.watchdogMaxLagSinceHeartbeat = lag;
          }
          // Resume timestamp is `now`; the loop was stuck from
          // (now - INTERVAL_MS - lag) up to `now`. Reporting the lag is
          // sufficient — the wall-clock log timestamp gives the resume
          // moment, so the start can be derived by subtraction.
          console.warn(
            `[DIAG-EVENTLOOP] hang ${Math.round(lag)}ms (resume at perf=${Math.round(now)}ms)` +
            ` document.hidden=${document.hidden}`,
          );
        }
      } catch { /* never let a watchdog tick throw */ }
    }, INTERVAL_MS);
  }

  /** Emit one heartbeat warn line. Hot path is intentionally minimal — no
   *  Tauri IPC, no per-pane WASM calls (we only touch the active core).
   *  Lag is the difference between the actual interval and the expected
   *  5000 ms; large positive values mean the timer was held back by a
   *  blocked main thread. */
  private fireHeartbeat(): void {
    try {
      const now = performance.now();
      const lag = Math.round(now - this.heartbeatLastFiredAt - 5000);
      this.heartbeatLastFiredAt = now;

      const ctx = this.ctx;
      const panes = ctx.getMuxWindowsLength();
      const activeIdx = ctx.getActiveMuxWindowIndex();
      const paneIds = ctx.getMuxPaneIds();
      const activePaneId = paneIds[activeIdx] ?? -1;

      const lastSwitchAt = getLastMuxSwitchAt();
      const lastSwitchAgoMs = lastSwitchAt > 0 ? Math.round(now - lastSwitchAt) : -1;

      const renderer = ctx.getRenderer();
      const rafGap = (renderer as unknown as {
        getAndResetMaxRafGap?: () => number;
      })?.getAndResetMaxRafGap?.() ?? -1;

      let wasmHeapMB = -1;
      try {
        const bytes = getWasmMemoryBytes();
        if (bytes >= 0) wasmHeapMB = Math.round(bytes / (1024 * 1024));
      } catch { /* loader not initialized */ }

      // IPC layer observability: chunkRecv* counters tell us whether the
      // backend → frontend Channel listener is firing at all (= distinguish
      // "IPC stuck" from "scheduling stuck"). pending* counters reveal
      // whether listener-delivered chunks are piling up because
      // processPendingData isn't running. lastChunkAgoMs == -1 means no
      // chunk has been received since spawn.
      const recv = ctx.getPtyClient()?.getRecvStats();
      const recvCount = recv?.count ?? -1;
      const recvBytes = recv?.bytes ?? -1;
      const lastChunkAgoMs = recv && recv.lastRecvAt > 0
        ? Math.round(now - recv.lastRecvAt)
        : -1;
      const pending = ctx.getPtyHandlerHandle()?.getPendingStats();
      const pendingChunks = pending?.chunks ?? -1;
      const pendingBytes = pending?.bytes ?? -1;
      const pendingLeftover = pending?.hasLeftover ?? false;

      // Sliding-window counters over the last 30s. Surface mux-switch
      // storms and slow-render / slow-process accumulation directly in the
      // heartbeat so we can correlate with hangs without scanning every
      // slow-render line in the log.
      const W = 30_000;
      const muxSw30s = getMuxSwitchCountWithin(W);
      const slowR30s = getSlowRenderCountWithin(W);
      const slowP30s = getSlowProcessCountWithin(W);

      // Largest event-loop hang observed since the previous heartbeat.
      // Reported here (rather than only inside the watchdog tick) so that
      // a hang that ends just before the heartbeat is preserved alongside
      // the surrounding heartbeat counters for correlation.
      const evLoopMaxLag = Math.round(this.watchdogMaxLagSinceHeartbeat);
      this.watchdogMaxLagSinceHeartbeat = 0;

      console.warn(
        `[DIAG-PTY-HEALTH]` +
        ` mux=${ctx.getInMuxMode()}` +
        ` panes=${panes}` +
        ` activeIdx=${activeIdx} activePaneId=${activePaneId}` +
        ` lastSwitchAgoMs=${lastSwitchAgoMs}` +
        ` loopLag=${lag}ms` +
        ` rafMaxGap=${Math.round(rafGap)}ms` +
        ` wasmHeapMB=${wasmHeapMB}` +
        ` chunkRecv=${recvCount}/${recvBytes}b` +
        ` lastChunkAgoMs=${lastChunkAgoMs}` +
        ` pending=${pendingChunks}c/${pendingBytes}b leftover=${pendingLeftover}` +
        ` muxSw30s=${muxSw30s} slowR30s=${slowR30s} slowP30s=${slowP30s}` +
        ` evLoopMaxLag=${evLoopMaxLag}ms`,
      );
    } catch (err) {
      console.warn(`[DIAG-PTY-HEALTH] heartbeat threw: ${err instanceof Error ? err.message : String(err)}`);
    }
  }
}
