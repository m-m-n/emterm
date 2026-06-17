/**
 * Resize handler functions extracted from TerminalApp.
 * Handles container resize observation and character size change propagation.
 */

import {
  calculateTerminalSize,
  observeContainerResize,
} from "../pty";
import type { TerminalState } from "../terminal/state";
import type { ITerminalRenderer } from "../terminal";
import type { PtyClient } from "../pty/client";
import type { ImeHandler } from "./handlers/ime";
import type { MouseHandler } from "./handlers/mouse";
import type { SelectionController } from "../selection-v2";
import type { CharSize } from "./types";

/**
 * Context needed by resize handler functions.
 */
export interface ResizeHandlerContext {
  container: HTMLElement;
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getPtyClient: () => PtyClient | null;
  getImeHandler: () => ImeHandler | null;
  getMouseHandler: () => MouseHandler | null;
  getSelectionController: () => SelectionController | null;
  getCharSize: () => CharSize;
  getDisconnectResizeObserver: () => (() => void) | null;
  setDisconnectResizeObserver: (fn: (() => void) | null) => void;
  setupResizeObserver: () => void;
  /** Optional callback to propagate resize to mux daemon panes. */
  onMuxResize?: (cols: number, rows: number) => void;
  /**
   * Optional shared WASM crash recovery entry point. The resize path calls
   * into WASM (state.resize, setCellSizePx, forceRender) and can surface
   * `WebAssembly.RuntimeError` after system suspend; route such errors
   * here so recovery runs instead of the terminal silently freezing.
   *
   * The optional `onComplete` callback fires when recovery finishes; the
   * resize handler uses it to retry the resize so state and renderer
   * dimensions stay in sync after an async reinit.
   */
  tryRecoverFromWasmCrash?: (
    error: unknown,
    onComplete?: (success: boolean) => void,
  ) => boolean;
}

/**
 * Sets up resize observer for the container.
 * Returns a disconnect function to clean up.
 */
export function setupResizeObserver(ctx: ResizeHandlerContext): (() => void) | null {
  const charSize = ctx.getCharSize();
  return observeContainerResize(
    ctx.container,
    charSize.width,
    charSize.height,
    async (newCols, newRows) => {
      // Skip resize if container is hidden (tab not active)
      // This prevents buffer content from being lost when a tab becomes hidden
      // and ResizeObserver reports 0x0 dimensions (leading to 1x1 resize)
      if (ctx.container.style.display === "none" ||
          ctx.container.clientWidth === 0 || ctx.container.clientHeight === 0) {
        return;
      }

      const state = ctx.getState();
      const renderer = ctx.getRenderer();
      const cs = ctx.getCharSize();

      // Always update local terminal state/renderer (even if PTY not ready)
      if (state && renderer) {
        // Replays the resize pipeline. Used both for the initial attempt and
        // to retry after WASM recovery succeeds — without a retry, `state`
        // and `renderer` dimensions can diverge if an error occurs mid-way.
        const applyResize = (s: typeof state, r: typeof renderer): void => {
          s.resize(newCols, newRows);
          // Update cell size for CSI 14t/16t XTWINOPS responses
          s.setCellSizePx(
            Math.round(cs.width),
            Math.round(cs.height),
          );
          r.resize(newCols, newRows);
          r.forceRender(s);
        };

        try {
          applyResize(state, renderer);
        } catch (error) {
          console.error("Failed to resize terminal:", error);
          // Route into shared WASM recovery — resize hits WASM, so a
          // RuntimeError here indicates memory corruption after suspend.
          const handled = ctx.tryRecoverFromWasmCrash?.(error, (success) => {
            if (!success) return;
            const recoveredState = ctx.getState();
            const recoveredRenderer = ctx.getRenderer();
            if (!recoveredState || !recoveredRenderer) return;
            try {
              applyResize(recoveredState, recoveredRenderer);
            } catch (retryError) {
              // Second failure — give up to avoid infinite retry loops.
              console.error("[ERROR][FRONTEND] resize retry after WASM recovery failed:", retryError);
            }
          }) ?? false;
          if (!handled) {
            // Not a WASM crash (or recovery declined) — at least try to repaint.
            try {
              renderer.forceRender(state);
            } catch {
              // Rendering failed too - nothing we can do
            }
          }
        }
        ctx.getImeHandler()?.updatePosition();
        ctx.getMouseHandler()?.updateCharSize(
          cs.width,
          cs.height,
        );

        // Update selection controller dimensions (clears selection)
        ctx.getSelectionController()?.resize(
          newCols,
          newRows,
          cs.width,
          cs.height,
        );
      }

      // Resize PTY if session is active (returns false if not ready)
      const ptyClient = ctx.getPtyClient();
      if (ptyClient) {
        const resized = await ptyClient.resize(newCols, newRows);
        if (!resized && import.meta.env?.DEV) {
          console.debug("PTY resize skipped - session not yet started");
        }
      }

      // Propagate resize to mux daemon panes if in mux mode
      ctx.onMuxResize?.(newCols, newRows);
    },
  );
}

/**
 * Recalculate terminal size after character dimensions change (e.g. font change).
 * Updates charSize, resizes state/renderer/selection/PTY, and reconnects ResizeObserver.
 */
export function handleCharSizeChange(ctx: ResizeHandlerContext): CharSize | null {
  const state = ctx.getState();
  const renderer = ctx.getRenderer();
  if (!renderer || !state) return null;
  // Skip resize if container is hidden (e.g. inactive tab) - dimensions would be 0x0
  if (ctx.container.style.display === "none") return null;

  const newWidth = renderer.getCharWidth();
  const newHeight = renderer.getCharHeight();
  const currentCharSize = ctx.getCharSize();

  // Skip if dimensions didn't actually change
  if (newWidth === currentCharSize.width && newHeight === currentCharSize.height) {
    return null;
  }

  const newCharSize: CharSize = { width: newWidth, height: newHeight };

  // Recalculate terminal dimensions with new character size
  const { cols, rows } = calculateTerminalSize(
    ctx.container,
    newWidth,
    newHeight,
  );

  // Resize state, renderer, and selection
  state.resize(cols, rows);
  state.setCellSizePx(Math.round(newWidth), Math.round(newHeight));
  renderer.resize(cols, rows);
  renderer.forceRender(state);

  ctx.getMouseHandler()?.updateCharSize(newWidth, newHeight);
  ctx.getImeHandler()?.updateCharSize(newWidth, newHeight);
  ctx.getSelectionController()?.resize(cols, rows, newWidth, newHeight);

  // Reconnect ResizeObserver with new character dimensions
  ctx.getDisconnectResizeObserver()?.();
  // Caller is responsible for setting new charSize and calling setupResizeObserver again

  // Resize PTY
  ctx.getPtyClient()?.resize(cols, rows);

  // Propagate resize to mux daemon panes if in mux mode
  ctx.onMuxResize?.(cols, rows);

  return newCharSize;
}
