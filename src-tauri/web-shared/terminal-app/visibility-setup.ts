/**
 * Build and start the visibility-aware streaming controller.
 *
 * The controller watches `document.visibilityState` and Tauri focus
 * events, debounces hide transitions, and forwards confirmed
 * visibility changes to the backend / mux daemon so they can pause
 * streaming while the tab is hidden (FR1, FR2, FR5, NFR5).
 *
 * Extracted from TerminalApp purely to slim `init()` — behaviour is
 * unchanged.
 */

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { VisibilityController } from "../pty";
import type { PtyClient } from "../pty";
import type { MuxClient } from "../terminal/mux/mux-client";

export interface VisibilitySetupContext {
  getPtyClient(): PtyClient | null;
  getMuxClient(): MuxClient | null;
}

/**
 * Construct the controller and kick off its `start()` (errors are
 * logged but non-fatal). Returns the instance so the host can `stop()`
 * it during dispose.
 */
export function buildVisibilityController(
  ctx: VisibilitySetupContext,
): VisibilityController {
  const controller = new VisibilityController({
    getPtyClient: () => ctx.getPtyClient(),
    getMuxClient: () => ctx.getMuxClient(),
    getDocumentVisible: () => document.visibilityState === "visible",
    subscribeFocus: async (cb) => {
      const win = getCurrentWebviewWindow();
      const unlisten = await win.onFocusChanged(({ payload: focused }) => cb(focused));
      return unlisten;
    },
  });
  controller.start().catch((err) => {
    console.warn("[WARN][FRONTEND] VisibilityController.start failed:", err);
  });
  return controller;
}
