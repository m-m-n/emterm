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
}

/**
 * Sets up PTY output handlers using WASM parser + binary Channel IPC.
 *
 * The onData handler uses a while loop to support buffer switch interruption:
 * when process_pty_data encounters a mode 47/1047/1049 switch, it stops early
 * so the TS side can perform the buffer switch, then the remaining data is
 * routed to the correct (alternate or primary) core.
 */
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
}

export async function setupPtyHandlers(ctx: PtyHandlerContext): Promise<PtyHandlerHandle> {
  const ptyClient = ctx.getPtyClient();
  const state = ctx.getState();
  const noopHandle: PtyHandlerHandle = { injectData: () => {}, suppressOriginalPty: false, flushPendingData: () => {}, processNow: () => {} };
  if (!ptyClient || !state) return noopHandle;

  // Register callbacks on primary core
  ctx.registerCoreCallbacks(state.getWasmCore());

  // Track which core has callbacks registered
  let registeredCore = state.getWasmCore();

  // Buffer for incoming PTY data -- processed in rAF with frame budgeting
  // "Video approach": process data within time budget, render at 60fps
  let pendingChunks: Uint8Array[] = [];
  let leftoverData: Uint8Array | null = null;
  let rafScheduled = false;
  let rafWatchdog: ReturnType<typeof setTimeout> | null = null;
  let rafDegraded = false; // true when rAF is not being delivered
  const FRAME_BUDGET_MS = 12; // Leave ~4ms for rendering within 16.67ms frame
  const DEGRADED_BUDGET_MS = 100; // Generous budget when rAF is broken
  const RAF_WATCHDOG_MS = 500; // Fallback if rAF stops being delivered
  const DEGRADED_INTERVAL_MS = 50; // setTimeout interval in degraded mode

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
          ` | rafScheduled=${rafScheduled}` +
          ` | rafDegraded=${rafDegraded}`,
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

  const processPendingData = (fromWatchdog = false) => {
    const processingStart = performance.now();
    rafScheduled = false;

    // During async WASM reinitialization or after exhausting retries, skip processing
    if (wasmRecoveryInProgress || wasmUnrecoverable) return;
    if (rafWatchdog !== null) {
      clearTimeout(rafWatchdog);
      rafWatchdog = null;
    }

    const currentState = ctx.getState();
    const currentRenderer = ctx.getRenderer();

    // Diagnostic: track rAF callback timing
    const now = performance.now();
    const sinceLastRaf = lastRafCallbackTime > 0 ? now - lastRafCallbackTime : -1;
    const sinceSchedule = lastScheduleTime > 0 ? now - lastScheduleTime : -1;
    lastRafCallbackTime = now;
    const queuedBytes = totalBytesQueued;
    const queuedChunks = pendingChunks.length;
    const dataCallsSinceRaf = onDataCountSinceLastRaf;
    totalBytesQueued = 0;
    onDataCountSinceLastRaf = 0;

    if (fromWatchdog && !rafDegraded) {
      rafDegraded = true;
      console.warn(
        `[WARN][FRONTEND] rAF not delivered — switching to degraded (setTimeout) mode` +
        ` | sinceSchedule=${sinceSchedule.toFixed(0)}ms` +
        ` | sinceLastRaf=${sinceLastRaf.toFixed(0)}ms` +
        ` | pendingChunks=${queuedChunks}` +
        ` | pendingBytes=${queuedBytes}` +
        ` | onDataCalls=${dataCallsSinceRaf}` +
        ` | document.hidden=${document.hidden}` +
        ` | document.visibilityState=${document.visibilityState}`,
      );
      if (currentState && currentRenderer) {
        try {
          const forceRenderStart = performance.now();
          // Force full re-render to recover from potential canvas buffer loss
          // (WebKitGTK may discard canvas contents when rAF stops being delivered)
          currentRenderer.forceRender(currentState);
          const forceRenderTime = performance.now() - forceRenderStart;
          console.warn(
            `[WARN][FRONTEND] degraded-mode forceRender completed: ${forceRenderTime.toFixed(1)}ms`,
          );
        } catch (error) {
          console.error("[ERROR][FRONTEND] forceRender in degraded mode switch failed:", error);
        }
      }
      startRafRecoveryCheck();
    }
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
      // In degraded mode (rAF broken), use a larger budget to avoid falling behind
      let remaining = merged;
      const budget = rafDegraded ? DEGRADED_BUDGET_MS : FRAME_BUDGET_MS;
      const deadline = performance.now() + budget;
      let processed = false;
      const charSize = ctx.getCharSize();
      // Hard timeout: absolute maximum time for the entire processing loop.
      // If exceeded, abort to prevent UI freeze.
      const HARD_TIMEOUT_MS = 2000;
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
            ` | remainingBytes=${remaining.length}` +
            ` | fromWatchdog=${fromWatchdog}`,
          );
          leftoverData = remaining;
          break;
        }
      }

      if (processed) {
        ctx.getOutputActivityCallback()?.();

        // Synchronized Output (mode 2026): suppress rendering while active.
        // Dirty rows accumulate in WASM; flush happens when mode is cleared.
        if (!currentState.modes.synchronizedOutput) {
          const renderStart = performance.now();
          currentRenderer.renderImmediate(currentState);
          const renderTime = performance.now() - renderStart;
          ctx.getImeHandler()?.updatePosition();

          // Diagnostic: log slow renders
          if (renderTime > 30) {
            console.warn(
              `[WARN][FRONTEND] slow-render: ${renderTime.toFixed(1)}ms` +
              ` | rafDegraded=${rafDegraded}`,
            );
          }
        }
      }

      // Diagnostic: log total processing time
      const processingTime = performance.now() - processingStart;
      if (processingTime > 50) {
        longProcessingCount++;
        console.warn(
          `[WARN][FRONTEND] slow-processPendingData: ${processingTime.toFixed(1)}ms` +
          ` | inputBytes=${queuedBytes}` +
          ` | chunks=${queuedChunks}` +
          ` | hasLeftover=${leftoverData !== null}` +
          ` | fromWatchdog=${fromWatchdog}` +
          ` | longProcessingTotal=${longProcessingCount}`,
        );
      }

      // Diagnostic: log completion when entering degraded mode (to detect freeze point)
      if (fromWatchdog && rafDegraded) {
        console.warn(
          `[WARN][FRONTEND] degraded-mode processPendingData completed: ${processingTime.toFixed(1)}ms` +
          ` | hasLeftover=${leftoverData !== null}`,
        );
      }

      lastProcessingEndTime = performance.now();

      // If there's leftover data, schedule next frame to continue
      if (leftoverData && !rafScheduled) {
        scheduleProcessing();
      }
    } catch (error) {
      console.error("[ERROR][FRONTEND] processPendingData failed:", error);
      leftoverData = null;

      // Detect WASM crash or uninitialized state:
      // - RuntimeError: memory corruption (e.g., after long idle)
      // - "recursive use of an object": wasm-bindgen borrow flag stuck after crash
      // - "WASM not initialized": previous recovery failed, primaryWasmGrid is null
      const isWasmCrash = error instanceof WebAssembly.RuntimeError;
      const msg = error instanceof Error ? error.message : String(error);
      const isBorrowError = msg.includes("recursive use of an object");
      const isWasmUninitialized = msg.includes("WASM not initialized");
      if (isWasmCrash || isBorrowError || isWasmUninitialized) {
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
          return;
        }
        console.warn(
          `[WARN][FRONTEND] WASM crash detected — attempting recovery (${wasmRecoveryAttempts}/${MAX_WASM_RECOVERY_ATTEMPTS})`,
        );

        // Stop cursor blink during recovery to prevent WASM access on stale/freed state
        ctx.getRenderer()?.stopCursorBlink();

        const finishRecovery = () => {
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
        };

        // Step 1: Try recreating WASM core (works if WASM engine is healthy)
        if (currentState?.recreateWasmCore()) {
          finishRecovery();
        } else if (currentState) {
          // Step 2: WASM engine itself is corrupted -- reinitialize the module
          console.warn("[WARN][FRONTEND] WASM core recreation failed — reinitializing WASM module");
          wasmRecoveryInProgress = true;
          (async () => {
            try {
              await reinitWasm();
              const recoveryState = ctx.getState();
              if (recoveryState?.recreateWasmCore()) {
                finishRecovery();
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
            }
          })();
        }
      }
    }
  };

  // ── rAF recovery check (single instance) ──────────────────
  // Detect when rAF delivery resumes after degraded mode.
  // Only one chain runs at a time to avoid flapping.
  let rafRecoveryActive = false;
  const RAF_RECOVERY_THRESHOLD = 3;

  const startRafRecoveryCheck = () => {
    if (rafRecoveryActive) return;
    rafRecoveryActive = true;
    let recoveryCount = 0;
    const checkRecovery = () => {
      recoveryCount++;
      if (recoveryCount >= RAF_RECOVERY_THRESHOLD) {
        rafDegraded = false;
        rafRecoveryActive = false;
        const renderer = ctx.getRenderer();
        const currentState = ctx.getState();
        if (currentState && renderer) {
          renderer.forceRender(currentState);
        }
        console.info("[INFO][FRONTEND] rAF delivery resumed — switching back to normal mode");
      } else {
        requestAnimationFrame(checkRecovery);
      }
    };
    requestAnimationFrame(checkRecovery);
  };

  const scheduleProcessing = () => {
    if (rafScheduled) return;
    rafScheduled = true;
    lastScheduleTime = performance.now();

    if (rafDegraded) {
      // In degraded mode, use setTimeout directly (rAF is not working)
      setTimeout(() => processPendingData(false), DEGRADED_INTERVAL_MS);
    } else {
      requestAnimationFrame(() => processPendingData(false));
      // Watchdog: fallback if rAF callback is not delivered (e.g. WebKitGTK bug)
      if (rafWatchdog !== null) clearTimeout(rafWatchdog);
      rafWatchdog = setTimeout(() => {
        if (rafScheduled) {
          const elapsed = performance.now() - lastScheduleTime;
          const pendingBytes = pendingChunks.reduce((sum, c) => sum + c.length, 0);
          console.warn(
            `[WARN][FRONTEND] rAF watchdog triggered — forcing data processing` +
            ` | elapsed=${elapsed.toFixed(0)}ms` +
            ` | pendingChunks=${pendingChunks.length}` +
            ` | pendingBytes=${pendingBytes}` +
            ` | document.hidden=${document.hidden}`,
          );
          processPendingData(true);
        }
      }, RAF_WATCHDOG_MS);
    }
  };

  // Stateful extractor for mux APC/OSC messages -- buffers across chunk boundaries
  const muxExtractor = new MuxMessageExtractor();

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
  };

  // Register binary data handler -- just buffer and schedule rAF
  ptyClient.onData((data: Uint8Array) => {
    // During mux mode, skip WASM processing but still extract mux APC messages.
    // Bridge sends PaneCreated/PtyOutput as APC sequences over the PTY.
    if (handle.suppressOriginalPty) {
      muxExtractor.extract(data, ctx.getMuxApcContext());
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
        ` | rafScheduled=${rafScheduled}` +
        ` | rafDegraded=${rafDegraded}` +
        ` | healthCheckCount=${healthCheckCount}` +
        ` | document.hidden=${document.hidden}`,
      );
    }

    const sinceLastRaf = lastRafCallbackTime > 0 ? now - lastRafCallbackTime : -1;
    const sinceLastData = lastOnDataTime > 0 ? now - lastOnDataTime : -1;
    const sinceLastSchedule = lastScheduleTime > 0 ? now - lastScheduleTime : -1;

    // Log if rAF hasn't fired in a while but data is flowing
    if (sinceLastRaf > 2000 && sinceLastData < 2000 && sinceLastData > 0) {
      console.warn(
        `[WARN][FRONTEND] health-check: rAF stalled` +
        ` | sinceLastRaf=${sinceLastRaf.toFixed(0)}ms` +
        ` | sinceLastData=${sinceLastData.toFixed(0)}ms` +
        ` | sinceLastSchedule=${sinceLastSchedule.toFixed(0)}ms` +
        ` | rafScheduled=${rafScheduled}` +
        ` | rafDegraded=${rafDegraded}` +
        ` | pendingChunks=${pendingChunks.length}` +
        ` | document.hidden=${document.hidden}`,
      );
    }

    // Log if rafScheduled is stuck true (scheduled but never fired)
    if (rafScheduled && sinceLastSchedule > 3000) {
      console.warn(
        `[WARN][FRONTEND] health-check: rafScheduled stuck for ${sinceLastSchedule.toFixed(0)}ms` +
        ` | rafDegraded=${rafDegraded}` +
        ` | document.hidden=${document.hidden}`,
      );
    }

    lastHealthCheckTime = now;
    lastHealthCheckWall = wallNow;
    setTimeout(healthCheck, HEALTH_CHECK_INTERVAL_MS);
  };
  lastHealthCheckTime = performance.now();
  lastHealthCheckWall = Date.now();
  setTimeout(healthCheck, HEALTH_CHECK_INTERVAL_MS);

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
