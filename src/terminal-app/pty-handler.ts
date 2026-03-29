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

/**
 * Extract APC sequences from raw PTY data and pass mux APCs to handleMuxApc.
 * Used during suppressOriginalPty mode where WASM processing is skipped.
 * APC format: ESC _ <body> ESC \
 */
function extractAndHandleMuxApcs(data: Uint8Array): void {
  const ESC = 0x1b;
  const UNDERSCORE = 0x5f; // '_'
  const BACKSLASH = 0x5c; // '\'
  let i = 0;
  while (i < data.length - 1) {
    // Look for APC start: ESC _
    if (data[i] === ESC && data[i + 1] === UNDERSCORE) {
      const bodyStart = i + 2;
      // Find APC end: ESC \
      let j = bodyStart;
      while (j < data.length - 1) {
        if (data[j] === ESC && data[j + 1] === BACKSLASH) {
          // Found complete APC: body is data[bodyStart..j]
          const body = data.subarray(bodyStart, j);
          handleMuxApc(body);
          i = j + 2;
          break;
        }
        j++;
      }
      if (j >= data.length - 1) {
        // Incomplete APC at end of buffer — skip for now
        break;
      }
    } else {
      i++;
    }
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
  const MAX_WASM_RECOVERY_ATTEMPTS = 3;
  const RECOVERY_WINDOW_MS = 60_000; // Reset attempt counter after 60s of stability
  let wasmRecoveryAttempts = 0;
  let lastRecoveryTimestamp = 0;
  let wasmRecoveryInProgress = false;
  let wasmUnrecoverable = false;

  const processPendingData = (fromWatchdog = false) => {
    rafScheduled = false;

    // During async WASM reinitialization or after exhausting retries, skip processing
    if (wasmRecoveryInProgress || wasmUnrecoverable) return;
    if (rafWatchdog !== null) {
      clearTimeout(rafWatchdog);
      rafWatchdog = null;
    }

    const currentState = ctx.getState();
    const currentRenderer = ctx.getRenderer();

    if (fromWatchdog && !rafDegraded) {
      rafDegraded = true;
      console.warn("[WARN][FRONTEND] rAF not delivered — switching to degraded (setTimeout) mode");
      if (currentState && currentRenderer) {
        try {
          // Force full re-render to recover from potential canvas buffer loss
          // (WebKitGTK may discard canvas contents when rAF stops being delivered)
          currentRenderer.forceRender(currentState);
          // Start compositor keep-alive: WebKitGTK stops the compositor frame
          // clock when rAF is idle, preventing canvas paints from reaching the
          // screen. A running Web Animation keeps the compositor ticking.
          currentRenderer.startCompositorKeepAlive();
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

        const consumed = core.process_pty_data(remaining);

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
      }

      if (processed) {
        ctx.getOutputActivityCallback()?.();

        // Synchronized Output (mode 2026): suppress rendering while active.
        // Dirty rows accumulate in WASM; flush happens when mode is cleared.
        if (!currentState.modes.synchronizedOutput) {
          currentRenderer.renderImmediate(currentState);
          ctx.getImeHandler()?.updatePosition();
        }
      }

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
          ctx.getRenderer()?.forceRender(recoveryState);
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
        renderer?.stopCompositorKeepAlive();
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

    if (rafDegraded) {
      // In degraded mode, use setTimeout directly (rAF is not working)
      setTimeout(() => processPendingData(false), DEGRADED_INTERVAL_MS);
    } else {
      requestAnimationFrame(() => processPendingData(false));
      // Watchdog: fallback if rAF callback is not delivered (e.g. WebKitGTK bug)
      if (rafWatchdog !== null) clearTimeout(rafWatchdog);
      rafWatchdog = setTimeout(() => {
        if (rafScheduled) {
          console.warn(
            "[WARN][FRONTEND] rAF watchdog triggered — forcing data processing",
          );
          processPendingData(true);
        }
      }, RAF_WATCHDOG_MS);
    }
  };

  const handle: PtyHandlerHandle = {
    injectData: (data: Uint8Array) => {
      pendingChunks.push(data);
      scheduleProcessing();
    },
    suppressOriginalPty: false,
    flushPendingData: () => {
      pendingChunks.length = 0;
      leftoverData = null;
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
      extractAndHandleMuxApcs(data);
      return;
    }
    pendingChunks.push(data);
    scheduleProcessing();
  });

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
