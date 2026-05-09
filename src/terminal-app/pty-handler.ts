/**
 * PTY data handler functions extracted from TerminalApp.
 * Handles PTY data flow with WASM processing, error recovery / watchdog,
 * and the entire PTY data receive pipeline.
 */

import type { TerminalState } from "../terminal/state";
import type { ITerminalRenderer } from "../terminal";
import type { PtyClient } from "../pty/client";
import type { ImeHandler } from "./handlers/ime";
import type { ImageHandler } from "./handlers/image";
import type { CharSize } from "./types";
import { reinitWasm } from "../terminal/wasm/loader";
import { handleMuxApc } from "../terminal/handlers/apc_handlers";
import type { MuxApcContext } from "../terminal/handlers/apc_handlers";
import { muxLog } from "../terminal/mux/mux-logger";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/**
 * Stateful APC/OSC parser that buffers incomplete sequences across PTY read chunks.
 *
 * Used during suppressOriginalPty mode (mux) where WASM processing is skipped.
 * The bridge writes complete APC/OSC sequences, but PTY reads may split them
 * at arbitrary boundaries (especially for large PtyOutput payloads > 4KB).
 *
 * APC format: ESC _ <body> ESC \
 * OSC format: ESC ] 9999 ; <body> ESC \ (or BEL)
 */
class MuxMessageExtractor {
  private leftover: Uint8Array | null = null;
  private truncationCount = 0;

  extract(data: Uint8Array, muxCtx: MuxApcContext | null): void {
    // Merge leftover from previous chunk if present
    let buf: Uint8Array;
    if (this.leftover) {
      buf = new Uint8Array(this.leftover.length + data.length);
      buf.set(this.leftover);
      buf.set(data, this.leftover.length);
      const leftoverLen = this.leftover.length;
      this.leftover = null;
      muxLog.debug(`[DIAG-APC] merged leftover (${leftoverLen}) + new (${data.length}) = ${buf.length} bytes`);
    } else {
      buf = data;
    }

    const ESC = 0x1b;
    const UNDERSCORE = 0x5f; // '_'
    const BACKSLASH = 0x5c; // '\'
    const BRACKET = 0x5d;   // ']'
    const BEL = 0x07;
    let i = 0;
    while (i < buf.length - 1) {
      if (buf[i] === ESC) {
        if (buf[i + 1] === UNDERSCORE) {
          // APC: ESC _ <body> ESC \
          const bodyStart = i + 2;
          let j = bodyStart;
          while (j < buf.length - 1) {
            if (buf[j] === ESC && buf[j + 1] === BACKSLASH) {
              const body = buf.subarray(bodyStart, j);
              handleMuxApc(body, muxCtx);
              i = j + 2;
              break;
            }
            j++;
          }
          if (j >= buf.length - 1) {
            // Incomplete APC -- save from ESC _ onward for next chunk
            this.leftover = buf.slice(i);
            this.truncationCount++;
            muxLog.warn(`[DIAG-APC] APC truncated at chunk boundary: saved ${this.leftover.length} bytes (total truncations: ${this.truncationCount})`);
            return;
          }
        } else if (buf[i + 1] === BRACKET) {
          // OSC: ESC ] 9999 ; <body> ESC \ (or BEL)
          let paramEnd = i + 2;
          let param = 0;
          while (paramEnd < buf.length && buf[paramEnd]! >= 0x30 && buf[paramEnd]! <= 0x39) {
            param = param * 10 + (buf[paramEnd]! - 0x30);
            paramEnd++;
          }
          if (param === 9999 && paramEnd < buf.length && buf[paramEnd] === 0x3b) {
            const bodyStart = paramEnd + 1;
            let j = bodyStart;
            let found = false;
            while (j < buf.length) {
              if (j < buf.length - 1 && buf[j] === ESC && buf[j + 1] === BACKSLASH) {
                const body = buf.subarray(bodyStart, j);
                handleMuxApc(body, muxCtx);
                i = j + 2;
                found = true;
                break;
              }
              if (buf[j] === BEL) {
                const body = buf.subarray(bodyStart, j);
                handleMuxApc(body, muxCtx);
                i = j + 1;
                found = true;
                break;
              }
              j++;
            }
            if (!found) {
              // Incomplete OSC -- save from ESC ] onward for next chunk
              this.leftover = buf.slice(i);
              this.truncationCount++;
              muxLog.warn(`[DIAG-APC] OSC truncated at chunk boundary: saved ${this.leftover.length} bytes (total truncations: ${this.truncationCount})`);
              return;
            }
          } else {
            i++;
          }
        } else {
          i++;
        }
      } else {
        i++;
      }
    }

    // Save a trailing ESC byte as leftover — it may be the start of an
    // APC/OSC sequence that continues in the next chunk.
    if (i === buf.length - 1 && buf[i] === ESC) {
      this.leftover = buf.slice(i);
    }
  }

  /** Discard any buffered leftover (e.g., on mode exit). */
  reset(): void {
    this.leftover = null;
  }
}

/**
 * Context needed by PTY handler functions.
 */
export interface PtyHandlerContext {
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getPtyClient: () => PtyClient | null;
  getImeHandler: () => ImeHandler | null;
  getImageHandler: () => ImageHandler | null;
  getCharSize: () => CharSize;
  registerCoreCallbacks: (core: ReturnType<TerminalState["getActiveCore"]>) => void;
  processPendingOscQueue: () => void;
  getOutputActivityCallback: () => (() => void) | null;
  getSessionExitCallback: () => ((sessionId: string) => void) | null;
  getMuxApcContext: () => MuxApcContext | null;
  isTabActive: () => boolean;
  /**
   * Invoked after WASM recovery finishes successfully (fast or slow path).
   * `viaReinit=true` means the entire WASM module was reloaded — all saved
   * WasmGrid references (e.g., mux inactive-pane grids) now point to dead
   * memory and must be discarded. Caller is responsible for triggering
   * content replay (e.g., mux RequestPaneSnapshot) if applicable.
   */
  onRecovered?: (viaReinit: boolean) => void;
}

/**
 * Sets up PTY output handlers using WASM parser + binary Channel IPC.
 *
 * The onData handler uses a while loop to support buffer switch interruption:
 * when process_pty_data encounters a mode 47/1047/1049 switch, it stops early
 * so the TS side can perform the buffer switch, then the remaining data is
 * routed to the correct (alternate or primary) core.
 */
/** Sliding-window timestamp logs (perf clock) for slow-render and
 *  slow-processPendingData events. Trimmed lazily in their getters. Module
 *  scope rather than per-handle because the heartbeat should see counts even
 *  when handlers are torn down and re-created (e.g. after WASM recovery). */
const _slowRenderTimestamps: number[] = [];
const _slowProcessTimestamps: number[] = [];
function recordSlowRender(now: number): void {
  _slowRenderTimestamps.push(now);
  if (_slowRenderTimestamps.length > 1024) _slowRenderTimestamps.shift();
}
function recordSlowProcess(now: number): void {
  _slowProcessTimestamps.push(now);
  if (_slowProcessTimestamps.length > 1024) _slowProcessTimestamps.shift();
}
export function getSlowRenderCountWithin(windowMs: number): number {
  const cutoff = performance.now() - windowMs;
  while (_slowRenderTimestamps.length > 0 && _slowRenderTimestamps[0]! < cutoff) {
    _slowRenderTimestamps.shift();
  }
  return _slowRenderTimestamps.length;
}
export function getSlowProcessCountWithin(windowMs: number): number {
  const cutoff = performance.now() - windowMs;
  while (_slowProcessTimestamps.length > 0 && _slowProcessTimestamps[0]! < cutoff) {
    _slowProcessTimestamps.shift();
  }
  return _slowProcessTimestamps.length;
}

/** Handle returned by setupPtyHandlers for injecting external data into the pipeline. */
export interface PtyHandlerHandle {
  /** Inject data into the PTY processing pipeline (same path as onData). */
  injectData: (data: Uint8Array) => void;
  /** Suppress original PTY output (set true during mux mode). */
  suppressOriginalPty: boolean;
  /** Discard all buffered data (pending chunks + leftover). Call on mux window switch. */
  flushPendingData: () => void;
  /** Process all buffered data synchronously (for reattach: process before saving state). */
  processNow: () => void;
  /** Notify that the tab has become active — triggers forceRender to repaint skipped frames. */
  notifyTabActivated: () => void;
  /**
   * Shared WASM crash recovery entry point.
   *
   * Returns `true` when the error was a WASM crash (or related wasm-bindgen /
   * uninitialized state) and recovery was attempted or was already in progress.
   * Returns `false` when the error was unrelated to WASM — callers should then
   * handle or rethrow as appropriate.
   *
   * Optional `onComplete` callback fires once recovery finishes with `success`
   * indicating whether a fresh WASM core is available. It fires synchronously
   * when recovery succeeds on the fast path (recreateWasmCore), asynchronously
   * after `reinitWasm` on the slow path, or immediately with `false` when the
   * terminal is already marked unrecoverable. Callbacks queued while another
   * recovery is in flight all fire when that recovery finishes.
   *
   * Safe to call concurrently from multiple error sites; gated by
   * `wasmRecoveryInProgress` / `wasmUnrecoverable` flags to ensure idempotency.
   * Never throws.
   */
  tryRecoverFromWasmCrash: (
    error: unknown,
    onComplete?: (success: boolean) => void,
  ) => boolean;
  /**
   * User-initiated WASM reinitialization. Resets the automatic recovery
   * guards (attempt counter, unrecoverable flag) before invoking the shared
   * recovery path with a synthetic `WebAssembly.RuntimeError`, so it works
   * even after the terminal has been marked unrecoverable.
   *
   * `onComplete` fires once recovery finishes with the success flag.
   */
  forceReinitWasm: (onComplete?: (success: boolean) => void) => void;
  /** Remove event listeners and clean up resources. */
  destroy: () => void;
  /** Diagnostic snapshot for the heartbeat: pending-queue depth + total
   *  bytes still in the queue + leftover indicator. Combined with
   *  PtyClient.getRecvStats() this lets the heartbeat distinguish "IPC
   *  receive layer stuck" (chunks not arriving) from "scheduling stuck"
   *  (chunks arrived and piled up but processPendingData not running). */
  getPendingStats: () => { chunks: number; bytes: number; hasLeftover: boolean };
}

export async function setupPtyHandlers(ctx: PtyHandlerContext): Promise<PtyHandlerHandle> {
  const ptyClient = ctx.getPtyClient();
  const state = ctx.getState();
  const noopHandle: PtyHandlerHandle = { injectData: () => {}, suppressOriginalPty: false, flushPendingData: () => {}, processNow: () => {}, notifyTabActivated: () => {}, tryRecoverFromWasmCrash: (_err, onComplete) => { onComplete?.(false); return false; }, forceReinitWasm: (onComplete) => { onComplete?.(false); }, destroy: () => {}, getPendingStats: () => ({ chunks: 0, bytes: 0, hasLeftover: false }) };
  if (!ptyClient || !state) return noopHandle;

  // Register callbacks on primary core
  ctx.registerCoreCallbacks(state.getWasmCore());

  // Track which core has callbacks registered
  let registeredCore = state.getWasmCore();

  // Buffer for incoming PTY data -- processed on a sub-rAF scheduler with
  // frame budgeting (MessageChannel primary, setTimeout(0) fallback). Both
  // primitives are *task* schedulers (not microtask checkpoints), so they
  // keep running while the WebView is hidden / occluded AND yield between
  // drains so rendering / input can interleave. The label "microtask" in
  // the trigger union is historical shorthand for "sub-rAF scheduler that
  // keeps draining while occluded" — both paths are task-driven.
  // Canvas rendering itself remains rAF-driven and is unchanged.
  let pendingChunks: Uint8Array[] = [];
  let leftoverData: Uint8Array | null = null;
  let processScheduled = false;
  // Populated only on the setTimeout(0) fallback path so destroy() can cancel
  // it. Always null on the MessageChannel path. runScheduledCallback resets it
  // unconditionally — that is a no-op on the MessageChannel path and the
  // intentional cleanup on the timer path.
  let pendingHandle: ReturnType<typeof setTimeout> | null = null;
  // Monotonically increasing token. scheduleProcessing captures the current
  // value at queue time and passes it to the scheduler primitive; the
  // scheduled callback compares its captured token against scheduleToken
  // before running, so a stale callback queued before a direct synchronous
  // processPendingData (which bumps scheduleToken) cannot double-fire
  // processPendingData.
  let scheduleToken = 0;
  // Last token observed by a fired callback — diagnostic only.
  let pendingToken = 0;
  // Flips true at the top of destroy(); guards in-flight scheduled callbacks
  // from observing torn-down state.
  let disposed = false;
  const FRAME_BUDGET_MS = 12; // Leave ~4ms for rendering within 16.67ms frame
  // Coalesced ack state: at 60Hz a per-frame `pty_ack` IPC for tiny keystroke
  // echoes is pure overhead (HIGH_WATER is 8 MB so single-byte acks add no
  // value). Accumulate consumed bytes here and flush either at the byte
  // threshold or after a short timer, whichever comes first.
  let pendingAckBytes = 0;
  let ackFlushTimer: ReturnType<typeof setTimeout> | null = null;
  const ACK_FLUSH_BYTES = 64 * 1024;
  const ACK_FLUSH_INTERVAL_MS = 250;

  // ── Diagnostic state for rAF freeze investigation ──────────
  let lastRafCallbackTime = 0;       // When processPendingData last ran via rAF
  let lastScheduleTime = 0;          // When scheduleProcessing was last called
  let lastOnDataTime = 0;            // When onData last received PTY data
  let onDataCountSinceLastRaf = 0;   // How many onData calls between rAF callbacks
  let totalBytesQueued = 0;          // Total bytes queued since last processPendingData
  let eventLoopProbeScheduled = false;
  let longProcessingCount = 0;       // Count of processPendingData calls > 50ms
  let lastHealthCheckTime = 0;       // When healthCheck last actually ran (performance.now)
  let lastHealthCheckWall = 0;       // When healthCheck last actually ran (Date.now)
  let healthCheckCount = 0;          // Total health-check invocations
  let lastProcessingEndTime = 0;     // When processPendingData last finished

  // ── [DIAG-PTY-FLOW] 5-second rolling counters ──────────────
  // Used to expose the data-flow state in the log even when no slow-event
  // warning fires. Resets every flush window so the per-window counts (not
  // cumulative totals) stay readable. Tracks the four hops in the visible
  // PTY pipeline:
  //   onData            : how much arrived from Rust via Tauri Channel
  //   processPendingData: how much frontend actually drained from the queue
  //   ackBytes          : how much was reported back to Rust via pty_ack
  // A persistent gap between onData bytes and ackBytes is what caused the
  // 38-minute 02:06 freeze (in_flight before == after).
  let flowOnDataCalls = 0;
  let flowOnDataBytes = 0;
  let flowProcessCalls = 0;
  let flowProcessBytes = 0;
  let flowAckCalls = 0;
  let flowAckBytes = 0;
  // Wall-clock + perf timestamps of the most recent onData event so the flow
  // log can include "ago" in case nothing has arrived recently.
  let lastOnDataPerfMs = 0;
  // Wall-clock + perf timestamps of the most recent ackBytes invocation.
  let lastAckPerfMs = 0;
  let flowFlushTimer: ReturnType<typeof setInterval> | null = null;
  const FLOW_FLUSH_INTERVAL_MS = 5000;

  // Event loop health probe: measures setTimeout(0) latency to detect main thread blockage
  const probeEventLoopHealth = () => {
    if (eventLoopProbeScheduled) return;
    eventLoopProbeScheduled = true;
    const probeStart = performance.now();
    setTimeout(() => {
      eventLoopProbeScheduled = false;
      const latency = performance.now() - probeStart;
      if (latency > 100) {
        console.warn(
          `[WARN][FRONTEND] event-loop-lag: setTimeout(0) took ${latency.toFixed(1)}ms` +
          ` | pendingChunks=${pendingChunks.length}` +
          ` | processScheduled=${processScheduled}`,
        );
      }
    }, 0);
  };
  const MAX_WASM_RECOVERY_ATTEMPTS = 3;
  const RECOVERY_WINDOW_MS = 60_000; // Reset attempt counter after 60s of stability
  let wasmRecoveryAttempts = 0;
  let lastRecoveryTimestamp = 0;
  let wasmRecoveryInProgress = false;
  let wasmUnrecoverable = false;

  /**
   * Shared WASM crash recovery entry point.
   *
   * Detects WASM crashes (`WebAssembly.RuntimeError`, wasm-bindgen "recursive
   * use of an object" borrow errors, and "WASM not initialized" states) and
   * attempts to recover by first recreating the core and, on failure,
   * reinitializing the WASM module asynchronously.
   *
   * Idempotent: concurrent calls while recovery is in progress (or after the
   * module has been marked unrecoverable) are no-ops aside from returning
   * `true` so the caller knows the error was WASM-related.
   */
  // onComplete callbacks queued while an async recovery is in flight. All
  // fire once that recovery finishes, with the same success flag. A fresh
  // call that fires synchronously does not touch this queue.
  let pendingRecoveryCallbacks: Array<(success: boolean) => void> = [];
  const fireRecoveryCallback = (cb: ((success: boolean) => void) | undefined, success: boolean) => {
    if (!cb) return;
    try {
      cb(success);
    } catch (cbError) {
      console.error("[ERROR][FRONTEND] WASM recovery onComplete callback threw:", cbError);
    }
  };
  const drainPendingRecoveryCallbacks = (success: boolean) => {
    const callbacks = pendingRecoveryCallbacks;
    pendingRecoveryCallbacks = [];
    for (const cb of callbacks) fireRecoveryCallback(cb, success);
  };

  const tryRecoverFromWasmCrash = (
    error: unknown,
    onComplete?: (success: boolean) => void,
    isManual = false,
  ): boolean => {
    // Detect WASM crash or uninitialized state:
    // - RuntimeError: memory corruption (e.g., after long idle)
    // - "recursive use of an object": wasm-bindgen borrow flag stuck after crash
    // - "WASM not initialized": previous recovery failed, primaryWasmGrid is null
    const isWasmCrash = error instanceof WebAssembly.RuntimeError;
    const msg = error instanceof Error ? error.message : String(error);
    const isBorrowError = msg.includes("recursive use of an object");
    const isWasmUninitialized = msg.includes("WASM not initialized");
    if (!isWasmCrash && !isBorrowError && !isWasmUninitialized) return false;

    // Idempotency: suppress duplicate triggers while a prior recovery is
    // either in flight or has already given up permanently.
    if (wasmUnrecoverable) {
      fireRecoveryCallback(onComplete, false);
      return true;
    }
    if (wasmRecoveryInProgress) {
      // Queue the callback so it fires with the outcome of the in-flight
      // recovery — otherwise retry logic (e.g. resize) is silently dropped.
      if (onComplete) pendingRecoveryCallbacks.push(onComplete);
      return true;
    }

    const currentState = ctx.getState();
    const now = Date.now();
    // Reset counter if enough time has passed since last recovery
    if (now - lastRecoveryTimestamp > RECOVERY_WINDOW_MS) {
      wasmRecoveryAttempts = 0;
    }
    lastRecoveryTimestamp = now;
    wasmRecoveryAttempts++;
    if (wasmRecoveryAttempts > MAX_WASM_RECOVERY_ATTEMPTS) {
      wasmUnrecoverable = true;
      console.error(
        `[ERROR][FRONTEND] WASM recovery exhausted (${MAX_WASM_RECOVERY_ATTEMPTS} attempts within ${RECOVERY_WINDOW_MS / 1000}s) — terminal is unrecoverable`,
      );
      fireRecoveryCallback(onComplete, false);
      return true;
    }
    // DIAG-IDLE: include visibility/focus context so we can tell whether the
    // crash was surfaced while hidden (blink during PC lock) vs. visible
    // (post-resume render / focus probe).
    const vs = typeof document !== "undefined" ? document.visibilityState : "n/a";
    const hidden = typeof document !== "undefined" ? document.hidden : "n/a";
    const reason = isManual ? "manual reinitialization" : "WASM crash detected";
    console.warn(
      `[WARN][FRONTEND] ${reason} — attempting recovery (${wasmRecoveryAttempts}/${MAX_WASM_RECOVERY_ATTEMPTS}) | visibilityState=${vs} hidden=${hidden}`,
    );

    // Stop cursor blink during recovery to prevent WASM access on stale/freed state
    try {
      ctx.getRenderer()?.stopCursorBlink();
    } catch (stopError) {
      console.warn("[WARN][FRONTEND] stopCursorBlink during recovery failed:", stopError);
    }

    const finishRecovery = (viaReinit: boolean) => {
      const recoveryState = ctx.getState();
      if (!recoveryState) return;
      const newCore = recoveryState.getWasmCore();
      ctx.registerCoreCallbacks(newCore);
      registeredCore = newCore;
      const cs = ctx.getCharSize();
      recoveryState.setCellSizePx(
        Math.round(cs.width),
        Math.round(cs.height),
      );
      const renderer = ctx.getRenderer();
      renderer?.forceRender(recoveryState);
      renderer?.startCursorBlink();
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] finishRecovery invoking onRecovered | viaReinit=${viaReinit} hasHook=${!!ctx.onRecovered}`,
      );
      try {
        ctx.onRecovered?.(viaReinit);
      } catch (hookError) {
        console.warn("[WARN][FRONTEND] onRecovered hook threw:", hookError);
      }
    };

    try {
      // Step 1: Try recreating WASM core (works if WASM engine is healthy)
      if (currentState?.recreateWasmCore()) {
        finishRecovery(false);
        fireRecoveryCallback(onComplete, true);
      } else if (currentState) {
        // Step 2: WASM engine itself is corrupted -- reinitialize the module
        console.warn("[WARN][FRONTEND] WASM core recreation failed — reinitializing WASM module");
        wasmRecoveryInProgress = true;
        if (onComplete) pendingRecoveryCallbacks.push(onComplete);
        (async () => {
          let success = false;
          try {
            await reinitWasm();
            const recoveryState = ctx.getState();
            if (recoveryState?.recreateWasmCore()) {
              finishRecovery(true);
              success = true;
              console.warn("[WARN][FRONTEND] WASM module reinitialized — terminal recovered");
            } else {
              wasmUnrecoverable = true;
              console.error("[ERROR][FRONTEND] WASM recovery failed after module reinit — terminal is unrecoverable");
            }
          } catch (reinitError) {
            wasmUnrecoverable = true;
            console.error("[ERROR][FRONTEND] WASM module reinit failed:", reinitError);
          } finally {
            wasmRecoveryInProgress = false;
            // Process any data that arrived during recovery (only if recovered)
            if (!wasmUnrecoverable && pendingChunks.length > 0) {
              scheduleProcessing();
            }
            drainPendingRecoveryCallbacks(success);
          }
        })();
      } else {
        // No state at all — nothing to recover to.
        fireRecoveryCallback(onComplete, false);
      }
    } catch (recoveryError) {
      // Defensive: recovery itself threw synchronously. Swallow so the function
      // contract (never throws) holds; log for diagnostics.
      console.error("[ERROR][FRONTEND] WASM recovery threw:", recoveryError);
      fireRecoveryCallback(onComplete, false);
    }
    return true;
  };

  // `trigger` lets us tell which scheduling path actually drained pendingChunks:
  //   - "microtask": MessageChannel / queueMicrotask delivery (primary)
  //   - "timer":     setTimeout(0) fallback delivery
  //   - "manual":    direct invocation (processNow, health-check force-drain)
  type ProcessTrigger = "microtask" | "timer" | "manual";
  const processPendingData = (trigger: ProcessTrigger = "manual") => {
    const processingStart = performance.now();
    // Invalidate any callbacks that were already queued from the previous
    // scheduleProcessing — they will compare their captured token against
    // the new scheduleToken and bail out before re-entering this function.
    scheduleToken++;
    // Reset the schedule flag here (in addition to runScheduledCallback) so
    // the manual entry path (processNow / health-check force-drain) can
    // re-schedule via the leftover-data branch at the bottom. Without this,
    // processScheduled remains stuck-true after a manual call and the
    // leftover re-schedule is short-circuited by the early-return guard in
    // scheduleProcessing.
    processScheduled = false;
    if (pendingHandle !== null) {
      clearTimeout(pendingHandle);
      pendingHandle = null;
    }

    // During async WASM reinitialization or after exhausting retries, skip processing
    if (wasmRecoveryInProgress || wasmUnrecoverable) return;

    const currentState = ctx.getState();
    const currentRenderer = ctx.getRenderer();

    // Diagnostic: track rAF callback timing
    const now = performance.now();
    const sinceLastRaf = lastRafCallbackTime > 0 ? now - lastRafCallbackTime : -1;
    lastRafCallbackTime = now;
    const queuedBytes = totalBytesQueued;
    const queuedChunks = pendingChunks.length;
    totalBytesQueued = 0;
    onDataCountSinceLastRaf = 0;

    if (!currentState || !currentRenderer) return;

    try {
      // Take all pending chunks
      const chunks = pendingChunks;
      pendingChunks = [];

      // Include leftover from previous frame
      if (leftoverData) {
        chunks.unshift(leftoverData);
        leftoverData = null;
      }

      if (chunks.length === 0) return;

      // Merge chunks into a single buffer
      let merged: Uint8Array;
      if (chunks.length === 1) {
        merged = chunks[0]!;
      } else {
        let totalLen = 0;
        for (const c of chunks) totalLen += c.length;
        merged = new Uint8Array(totalLen);
        let offset = 0;
        for (const chunk of chunks) {
          merged.set(chunk, offset);
          offset += chunk.length;
        }
      }

      // Process data with frame budget -- stop when time is up
      const inputBytes = merged.length;
      let remaining = merged;
      const deadline = performance.now() + FRAME_BUDGET_MS;
      let processed = false;
      const charSize = ctx.getCharSize();
      // Hard timeout: absolute maximum time for the entire processing loop.
      // If exceeded, abort to prevent UI freeze. Tightened from 2000 → 200 to
      // keep the main thread responsive: visibility-aware streaming pauses
      // backend forwarding while hidden, so per-frame backlog stays small and
      // each iteration must yield quickly to keep input latency low.
      const HARD_TIMEOUT_MS = 200;
      const hardDeadline = performance.now() + HARD_TIMEOUT_MS;

      while (remaining.length > 0) {
        const core = currentState.getActiveCore();

        if (core !== registeredCore) {
          ctx.registerCoreCallbacks(core);
          currentState.setCellSizePx(
            Math.round(charSize.width),
            Math.round(charSize.height),
          );
          registeredCore = core;
        }

        const prevCursorCol = currentState.cursorCol;
        const prevCursorRow = currentState.cursorRow;
        const prevCursorVisible = currentState.cursorVisible;

        const wasmStart = performance.now();
        const consumed = core.process_pty_data(remaining);
        const wasmTime = performance.now() - wasmStart;
        if (wasmTime > 500) {
          console.warn(
            `[WARN][FRONTEND] slow WASM process_pty_data: ${wasmTime.toFixed(1)}ms` +
            ` | inputBytes=${remaining.length}` +
            ` | consumed=${consumed}`,
          );
        }

        ctx.processPendingOscQueue();
        ctx.getImageHandler()?.processPendingApcQueue();
        ctx.getImageHandler()?.processPendingDcsQueue();

        currentState.syncModesFromWasm();

        const postCursorCol = currentState.cursorCol;
        const postCursorRow = currentState.cursorRow;
        const postCursorVisible = currentState.cursorVisible;

        // Diagnostic: log when cursor becomes visible unexpectedly (conpty investigation)
        if (postCursorVisible && !prevCursorVisible) {
          const chunk = remaining.subarray(0, consumed);
          // Search for \e[?25h (1b 5b 3f 32 35 68) in entire chunk
          const showSeq = [0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x68];
          const hideSeq = [0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x6c];
          const showPositions: number[] = [];
          const hidePositions: number[] = [];
          for (let i = 0; i <= chunk.length - 6; i++) {
            if (chunk[i] === 0x1b && chunk[i+1] === 0x5b && chunk[i+2] === 0x3f &&
                chunk[i+3] === 0x32 && chunk[i+4] === 0x35) {
              if (chunk[i+5] === 0x68) showPositions.push(i);
              else if (chunk[i+5] === 0x6c) hidePositions.push(i);
            }
          }
          console.warn(
            `[WARN][FRONTEND] cursor-visible-transition: false→true` +
            ` | consumed=${consumed}` +
            ` | pos=(${postCursorCol},${postCursorRow})` +
            ` | ?25h=${showPositions.length > 0 ? showPositions.join(",") : "NONE"}` +
            ` | ?25l=${hidePositions.length > 0 ? hidePositions.join(",") : "NONE"}`,
          );
        }

        const modeActions = core.take_mode_actions();
        if (modeActions.length > 0) {
          let i = 0;
          while (i < modeActions.length) {
            const action = modeActions[i]!;
            if (action === 0xFF || action === 0xFE) {
              const mode = modeActions[i + 1]! | (modeActions[i + 2]! << 8);
              const isSet = action === 0xFF;
              currentState.setDecPrivateMode(mode, isSet);
              i += 3;
            } else {
              currentState.handleModeAction(action);
              i += 1;
            }
          }
        }

        remaining = remaining.subarray(consumed);
        processed = true;

        if (consumed === 0) break;

        // When WASM interrupted at a cursor hidden->visible transition,
        // break to render the current state (e.g., vim search wrap message)
        // before processing the subsequent redraw that may clear it.
        if (remaining.length > 0 && postCursorVisible && !prevCursorVisible) {
          leftoverData = remaining;
          break;
        }

        // Check frame budget -- defer remaining data to next frame
        if (remaining.length > 0 && performance.now() >= deadline) {
          leftoverData = remaining;
          break;
        }

        // Hard timeout: abort to prevent UI freeze
        if (remaining.length > 0 && performance.now() >= hardDeadline) {
          console.error(
            `[ERROR][FRONTEND] processPendingData hard timeout (${HARD_TIMEOUT_MS}ms) — aborting` +
            ` | remainingBytes=${remaining.length}`,
          );
          leftoverData = remaining;
          break;
        }
      }

      if (processed) {
        ctx.getOutputActivityCallback()?.();

        // Synchronized Output (mode 2026): suppress rendering while active.
        // Dirty rows accumulate in WASM; flush happens when mode is cleared.
        // Inactive tab: skip rendering entirely. WASM dirty flags persist,
        // so forceRender on tab activation will repaint correctly.
        if (!currentState.modes.synchronizedOutput && ctx.isTabActive()) {
          const renderStart = performance.now();
          currentRenderer.renderImmediate(currentState);
          const renderTime = performance.now() - renderStart;
          ctx.getImeHandler()?.updatePosition();

          // Diagnostic: log slow renders
          if (renderTime > 30) {
            recordSlowRender(performance.now());
            console.warn(
              `[WARN][FRONTEND] slow-render: ${renderTime.toFixed(1)}ms`,
            );
          }
        }
      }

      // Diagnostic: log total processing time
      const processingTime = performance.now() - processingStart;
      if (processingTime > 50) {
        longProcessingCount++;
        recordSlowProcess(performance.now());
        console.warn(
          `[WARN][FRONTEND] slow-processPendingData: ${processingTime.toFixed(1)}ms` +
          ` | trigger=${trigger}` +
          ` | inputBytes=${queuedBytes}` +
          ` | chunks=${queuedChunks}` +
          ` | hasLeftover=${leftoverData !== null}` +
          ` | longProcessingTotal=${longProcessingCount}`,
        );
      }

      lastProcessingEndTime = performance.now();

      // Backpressure: tell Rust how many bytes the frontend just consumed so
      // it can resume reading from the PTY. The "consumed" count is bytes we
      // pulled out of pendingChunks/leftover this frame minus what got
      // pushed back into leftoverData for next frame. Coalesce small acks to
      // avoid per-frame Tauri IPC overhead — flush at threshold or after a
      // short interval.
      const leftoverLen = leftoverData ? leftoverData.length : 0;
      const consumed = inputBytes - leftoverLen;
      // [DIAG-PTY-FLOW] Per-window stats for processPendingData.
      flowProcessCalls += 1;
      flowProcessBytes += consumed;
      if (consumed > 0) {
        pendingAckBytes += consumed;
        if (pendingAckBytes >= ACK_FLUSH_BYTES) {
          // [DIAG-PTY-FLOW] Track every actual ack issued so the per-window
          // log can show how often / how many bytes the frontend told Rust
          // it consumed. Discrepancy vs flowOnDataBytes is the smoking gun.
          flowAckCalls += 1;
          flowAckBytes += pendingAckBytes;
          lastAckPerfMs = performance.now();
          ptyClient.ackBytes(pendingAckBytes);
          pendingAckBytes = 0;
          if (ackFlushTimer !== null) {
            clearTimeout(ackFlushTimer);
            ackFlushTimer = null;
          }
        } else if (ackFlushTimer === null) {
          ackFlushTimer = setTimeout(() => {
            ackFlushTimer = null;
            if (pendingAckBytes > 0) {
              flowAckCalls += 1;
              flowAckBytes += pendingAckBytes;
              lastAckPerfMs = performance.now();
              ptyClient.ackBytes(pendingAckBytes);
              pendingAckBytes = 0;
            }
          }, ACK_FLUSH_INTERVAL_MS);
        }
      }

      // If there's leftover data, schedule next microtask to continue.
      // Deduplication is enforced inside scheduleProcessing via the
      // processScheduled flag.
      if (leftoverData) {
        scheduleProcessing();
      }
    } catch (error) {
      console.error("[ERROR][FRONTEND] processPendingData failed:", error);
      leftoverData = null;
      tryRecoverFromWasmCrash(error);
    }
  };

  // ── Sub-rAF scheduler ───────────────────────────────────────
  // Selects MessageChannel (primary, task-scheduled via postMessage) or
  // setTimeout(0) (fallback, also task-scheduled). Both yield between drains
  // so rendering / input can interleave under sustained PTY output. The
  // queueMicrotask path was removed because microtask chaining (leftover →
  // scheduleProcessing → queueMicrotask) cannot yield to the task queue and
  // would starve rendering during long bursts. The trigger label exposed to
  // processPendingData ("microtask" or "timer") names the primitive, not the
  // checkpoint kind.
  type Scheduler = {
    schedule: (token: number) => void;
    dispose: () => void;
  };

  // Body of the scheduled callback. Compares the token captured at queue
  // time against scheduleToken to discard stale callbacks. Resets
  // processScheduled (and pendingHandle for the timer path) BEFORE invoking
  // processPendingData so a re-entrant scheduleProcessing() from
  // leftoverData can enqueue the next tick. Bails immediately if destroy()
  // has already begun teardown.
  const runScheduledCallback = (trigger: "microtask" | "timer", capturedToken: number) => {
    pendingToken = capturedToken;
    processScheduled = false;
    pendingHandle = null;
    if (disposed) return;
    if (capturedToken !== scheduleToken) return;
    processPendingData(trigger);
  };

  const createMicrotaskScheduler = (): Scheduler => {
    if (typeof MessageChannel !== "undefined") {
      try {
        const ch = new MessageChannel();
        ch.port2.onmessage = (e) => {
          const token = typeof e.data === "number" ? e.data : 0;
          runScheduledCallback("microtask", token);
        };
        return {
          schedule: (token) => {
            try {
              ch.port1.postMessage(token);
            } catch {
              // Defensive: if postMessage throws (e.g. ports already closed
              // during teardown) drop silently — destroy() resets state.
            }
          },
          dispose: () => {
            // Detach onmessage first so any task in flight before close()
            // becomes a no-op when dispatched.
            try { ch.port2.onmessage = null; } catch { /* ignore */ }
            try { ch.port1.close(); } catch { /* ignore */ }
            try { ch.port2.close(); } catch { /* ignore */ }
          },
        };
      } catch (e) {
        console.warn("[WARN][FRONTEND] MessageChannel unavailable, falling back to setTimeout(0):", e);
        // fall through
      }
    }
    return {
      schedule: (token) => {
        pendingHandle = setTimeout(() => runScheduledCallback("timer", token), 0);
      },
      dispose: () => {
        // pendingHandle is cleared by destroy() directly; the disposed flag
        // makes any late-firing timer callback a no-op.
      },
    };
  };

  const scheduler = createMicrotaskScheduler();

  const scheduleProcessing = () => {
    if (processScheduled) return;
    processScheduled = true;
    lastScheduleTime = performance.now();
    const token = ++scheduleToken;
    pendingToken = token;
    scheduler.schedule(token);
  };

  // Stateful extractor for mux APC/OSC messages -- buffers across chunk boundaries
  const muxExtractor = new MuxMessageExtractor();

  // ── Visibility-based render recovery ─────────────────────────
  // When the page becomes visible again after being hidden (desktop lock, workspace switch),
  // ── Focus-based WASM health probe ───────────────────────────
  // After system suspend/resume or long idle, WASM linear memory may be
  // corrupted. When the window regains focus, perform a cheap read-only
  // WASM call; on RuntimeError, route through the shared recovery path.
  // FR16: this probe is retained even after visibility-aware streaming
  // takes over the drain-on-resume responsibility, because WASM grid
  // corruption is a separate failure mode that focus return can surface.
  let unlistenFocus: (() => void) | null = null;
  let focusListenerDisposed = false;
  void (async () => {
    try {
      const win = getCurrentWebviewWindow();
      const unlisten = await win.onFocusChanged(({ payload: focused }) => {
        if (!focused) return;
        if (wasmRecoveryInProgress || wasmUnrecoverable) return;
        const currentState = ctx.getState();
        if (!currentState) return;
        try {
          const core = currentState.getActiveCore();
          void core.cols();
        } catch (error) {
          console.warn("[WARN][FRONTEND] focus health probe failed — invoking WASM recovery");
          tryRecoverFromWasmCrash(error);
        }
      });
      if (focusListenerDisposed) {
        try { unlisten(); } catch { /* ignore */ }
      } else {
        unlistenFocus = unlisten;
      }
    } catch (e) {
      console.warn("[WARN][FRONTEND] failed to register focus listener:", e);
    }
  })();

  const handle: PtyHandlerHandle = {
    injectData: (data: Uint8Array) => {
      pendingChunks.push(data);
      totalBytesQueued += data.length;
      onDataCountSinceLastRaf++;
      scheduleProcessing();
    },
    suppressOriginalPty: false,
    flushPendingData: () => {
      pendingChunks.length = 0;
      leftoverData = null;
      muxExtractor.reset();
    },
    processNow: () => {
      processPendingData();
    },
    notifyTabActivated: () => {
      const currentState = ctx.getState();
      const currentRenderer = ctx.getRenderer();
      if (currentState && currentRenderer) {
        currentRenderer.forceRender(currentState);
      }
      // If data piled up while the tab was hidden, drain it now via the
      // standard scheduling path instead of waiting for the next onData chunk.
      if (pendingChunks.length > 0 || leftoverData) {
        scheduleProcessing();
      }
    },
    tryRecoverFromWasmCrash,
    forceReinitWasm: (onComplete) => {
      // Reset guards so recovery runs even after prior exhaustion.
      wasmRecoveryAttempts = 0;
      wasmUnrecoverable = false;
      console.warn("[WARN][FRONTEND] manual WASM reinitialization requested");
      const syntheticError = new WebAssembly.RuntimeError(
        "manual reinitialize requested",
      );
      tryRecoverFromWasmCrash(syntheticError, onComplete, true);
    },
    getPendingStats: () => {
      let bytes = 0;
      for (const chunk of pendingChunks) bytes += chunk.length;
      if (leftoverData) bytes += leftoverData.length;
      return {
        chunks: pendingChunks.length,
        bytes,
        hasLeftover: leftoverData !== null,
      };
    },
    destroy: () => {
      focusListenerDisposed = true;
      if (unlistenFocus) {
        try { unlistenFocus(); } catch { /* ignore */ }
        unlistenFocus = null;
      }
      // Mark the handler as torn down BEFORE disposing the scheduler so any
      // in-flight scheduled callback that fires after dispose() is observed
      // by runScheduledCallback as disposed and bails before touching state.
      // Also bump scheduleToken so the captured token of any in-flight
      // callback becomes stale even if the disposed check is bypassed.
      disposed = true;
      scheduleToken++;
      // Tear down the sub-rAF scheduler. On the MessageChannel path this
      // detaches onmessage and closes both MessagePort instances; on the
      // setTimeout path it is a no-op (the timer handle is cleared below).
      try { scheduler.dispose(); } catch { /* ignore */ }
      if (pendingHandle !== null) {
        clearTimeout(pendingHandle);
        pendingHandle = null;
      }
      processScheduled = false;
      if (ackFlushTimer !== null) {
        clearTimeout(ackFlushTimer);
        ackFlushTimer = null;
      }
      // [DIAG-PTY-FLOW] Stop the per-session 5s flow-summary timer.
      if (flowFlushTimer !== null) {
        clearInterval(flowFlushTimer);
        flowFlushTimer = null;
      }
      // Best-effort flush of any unsent ack so Rust isn't left thinking the
      // frontend has more in-flight than it really does after teardown.
      if (pendingAckBytes > 0) {
        ptyClient.ackBytes(pendingAckBytes);
        pendingAckBytes = 0;
      }
    },
  };

  // Register binary data handler -- just buffer and schedule rAF
  ptyClient.onData((data: Uint8Array) => {
    // [DIAG-PTY-FLOW] Count BOTH mux and non-mux paths so the per-window
    // log shows the true bytes-from-Rust rate even in mux mode (where the
    // bytes are extracted by muxExtractor, not pushed onto pendingChunks).
    flowOnDataCalls += 1;
    flowOnDataBytes += data.length;
    lastOnDataPerfMs = performance.now();
    // During mux mode, skip WASM processing but still extract mux APC messages.
    // Bridge sends PaneCreated/PtyOutput as APC sequences over the PTY.
    if (handle.suppressOriginalPty) {
      muxExtractor.extract(data, ctx.getMuxApcContext());
      // Mux mode ack: the bridge PTY's outer bytes (APC framing + control
      // frames like Welcome/PaneCreated/Detached) are fully consumed by
      // muxExtractor here, but the inner pane bytes that get dispatched
      // via injectData → processPendingData → ackBytes credit only the
      // EXTRACTED payload, not the wrapper overhead. Without crediting
      // the wrapper bytes the bridge PTY's Rust-side in_flight grows
      // monotonically (~10 KB per 5 s observed) and eventually hits
      // HIGH_WATER, parking the reader and freezing the mux tab.
      //
      // Acking data.length here over-credits in_flight when the inner
      // bytes path also acks for the same chunk, but the backend uses
      // saturating_sub so in_flight just clamps to 0; the bridge PTY's
      // backpressure is effectively bypassed in mux mode. That is
      // acceptable because the real flow-control choke points in mux
      // mode are (a) the bridge process's stdout kernel buffer and
      // (b) the daemon socket — the GUI PTY HIGH_WATER was redundant.
      flowAckCalls += 1;
      flowAckBytes += data.length;
      lastAckPerfMs = performance.now();
      ptyClient.ackBytes(data.length);
      return;
    }
    pendingChunks.push(data);
    totalBytesQueued += data.length;
    onDataCountSinceLastRaf++;
    lastOnDataTime = performance.now();

    // Probe event loop health periodically (every ~100 onData calls)
    if (onDataCountSinceLastRaf % 100 === 0) {
      probeEventLoopHealth();
    }

    scheduleProcessing();
  });

  // ── Periodic health monitor ─────────────────────────────────
  // Runs every 10s to detect silent stalls (rAF stops but no data arrives to trigger watchdog)
  const HEALTH_CHECK_INTERVAL_MS = 10_000;
  const STALL_THRESHOLD_MS = 3_000; // health-check interval overrun threshold for stall detection
  const healthCheck = () => {
    const now = performance.now();
    const wallNow = Date.now();
    healthCheckCount++;

    // Detect main-thread stall: if this callback fired much later than expected,
    // the event loop was blocked for (actual - expected) milliseconds.
    const sinceLastHealthCheck = lastHealthCheckTime > 0 ? now - lastHealthCheckTime : -1;
    const wallElapsed = lastHealthCheckWall > 0 ? wallNow - lastHealthCheckWall : -1;
    const expectedInterval = HEALTH_CHECK_INTERVAL_MS;
    const overrun = sinceLastHealthCheck - expectedInterval; // >0 means late delivery

    if (overrun > STALL_THRESHOLD_MS) {
      // This health-check was significantly delayed — the main thread was blocked
      const sinceLastProcessing = lastProcessingEndTime > 0 ? now - lastProcessingEndTime : -1;
      const pendingBytes = pendingChunks.reduce((sum, c) => sum + c.length, 0);
      const clockDrift = sinceLastHealthCheck > 0 && wallElapsed > 0
        ? Math.abs(sinceLastHealthCheck - wallElapsed)
        : -1;
      console.warn(
        `[WARN][FRONTEND] health-check: main-thread stall detected` +
        ` | expectedInterval=${expectedInterval}ms` +
        ` | actualInterval=${sinceLastHealthCheck.toFixed(0)}ms` +
        ` | overrun=${overrun.toFixed(0)}ms` +
        ` | wallElapsed=${wallElapsed}ms` +
        ` | clockDrift=${clockDrift.toFixed(0)}ms` +
        ` | sinceLastProcessing=${sinceLastProcessing.toFixed(0)}ms` +
        ` | pendingChunks=${pendingChunks.length}` +
        ` | pendingBytes=${pendingBytes}` +
        ` | processScheduled=${processScheduled}` +
        ` | healthCheckCount=${healthCheckCount}` +
        ` | document.hidden=${document.hidden}`,
      );
    }

    const sinceLastRaf = lastRafCallbackTime > 0 ? now - lastRafCallbackTime : -1;
    const sinceLastData = lastOnDataTime > 0 ? now - lastOnDataTime : -1;
    const sinceLastSchedule = lastScheduleTime > 0 ? now - lastScheduleTime : -1;

    // Log if processPendingData hasn't run in a while but data is flowing.
    // Note: under the sub-rAF scheduler, the data path is no longer driven
    // by rAF; this branch now tracks the symptom of "scheduled callbacks not
    // getting delivered" via the same lastRafCallbackTime field. The warn
    // text is kept verbatim for log-grep compatibility (NFR6 / FR10) — only
    // the rafScheduled→processScheduled flag rename is intended.
    if (sinceLastRaf > 2000 && sinceLastData < 2000 && sinceLastData > 0) {
      console.warn(
        `[WARN][FRONTEND] health-check: rAF stalled` +
        ` | sinceLastRaf=${sinceLastRaf.toFixed(0)}ms` +
        ` | sinceLastData=${sinceLastData.toFixed(0)}ms` +
        ` | sinceLastSchedule=${sinceLastSchedule.toFixed(0)}ms` +
        ` | processScheduled=${processScheduled}` +
        ` | pendingChunks=${pendingChunks.length}` +
        ` | document.hidden=${document.hidden}`,
      );
    }

    // Log if processScheduled is stuck true (scheduled but never fired).
    // Under the microtask scheduler this branch is not expected to fire —
    // microtasks keep running while hidden — but it is retained as
    // defense-in-depth (FR10) against future regressions where a synchronous
    // body could hold the main thread long enough for processScheduled to
    // appear stuck.
    if (processScheduled && sinceLastSchedule > 3000) {
      console.warn(
        `[WARN][FRONTEND] health-check: processScheduled stuck for ${sinceLastSchedule.toFixed(0)}ms` +
        ` | document.hidden=${document.hidden}`,
      );
      // Force a synchronous drain so backlog does not keep growing.
      if (!wasmRecoveryInProgress && !wasmUnrecoverable) {
        try { processPendingData(); } catch { /* logged inside */ }
      }
    }

    lastHealthCheckTime = now;
    lastHealthCheckWall = wallNow;
    setTimeout(healthCheck, HEALTH_CHECK_INTERVAL_MS);
  };
  lastHealthCheckTime = performance.now();
  lastHealthCheckWall = Date.now();
  setTimeout(healthCheck, HEALTH_CHECK_INTERVAL_MS);

  // [DIAG-PTY-FLOW] Per-window summary of bytes-from-Rust vs bytes-acked.
  // Always emits at warn level so release builds keep the log; rate is one
  // line per FLOW_FLUSH_INTERVAL_MS per PTY session. Skips windows where
  // nothing happened (no data arrived AND no acks issued AND no pending
  // bytes) so the log is not flooded by idle sessions.
  const flushFlow = () => {
    const sessionId = ptyClient.getSessionId() ?? "?";
    const muxMode = handle.suppressOriginalPty;
    const now = performance.now();
    const onDataAgo = lastOnDataPerfMs > 0 ? Math.round(now - lastOnDataPerfMs) : -1;
    const ackAgo = lastAckPerfMs > 0 ? Math.round(now - lastAckPerfMs) : -1;
    const pendingBytes = pendingChunks.reduce((sum, c) => sum + c.length, 0);
    const idle = flowOnDataBytes === 0 && flowAckBytes === 0
      && pendingBytes === 0 && pendingAckBytes === 0;
    if (!idle) {
      console.warn(
        `[WARN][FRONTEND] [DIAG-PTY-FLOW]` +
        ` session=${sessionId.slice(0, 8)}` +
        ` mux=${muxMode}` +
        ` onData=${flowOnDataCalls}c/${flowOnDataBytes}b` +
        ` proc=${flowProcessCalls}c/${flowProcessBytes}b` +
        ` ack=${flowAckCalls}c/${flowAckBytes}b` +
        ` pendingChunks=${pendingChunks.length}` +
        ` pendingChunkBytes=${pendingBytes}` +
        ` pendingAckBytes=${pendingAckBytes}` +
        ` lastOnDataAgoMs=${onDataAgo}` +
        ` lastAckAgoMs=${ackAgo}`,
      );
    }
    flowOnDataCalls = 0;
    flowOnDataBytes = 0;
    flowProcessCalls = 0;
    flowProcessBytes = 0;
    flowAckCalls = 0;
    flowAckBytes = 0;
  };
  flowFlushTimer = setInterval(flushFlow, FLOW_FLUSH_INTERVAL_MS);

  // Handle exit event
  await ptyClient.onExit(async (_code, _remainingSessions) => {
    // Notify session exit callback (for TabManager integration)
    const currentPtyClient = ctx.getPtyClient();
    const sessionId = currentPtyClient?.getSessionId();
    const sessionExitCb = ctx.getSessionExitCallback();
    if (sessionId && sessionExitCb) {
      sessionExitCb(sessionId);
    }
    // Note: Window close is now handled by TabManager.onLastTabClosed()
  });

  return handle;
}
