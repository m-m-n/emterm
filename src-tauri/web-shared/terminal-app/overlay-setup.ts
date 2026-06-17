/**
 * Overlay-related wiring for TerminalApp.
 *
 * Sets up the Markdown / DataViewer / Download session managers and the
 * IME blur/focus handlers that surround their fullscreen views. All of
 * these depend on the overlay root and TerminalState being ready, but
 * none affect the hot path — they're orchestration glue only.
 *
 * Extracted from TerminalApp to keep `init()` focused on lifecycle
 * sequencing rather than dependency wiring.
 */

import type { TerminalState } from "../terminal/state";
import type { PtyClient } from "../pty/client";
import type { ImeHandler } from "./handlers/ime";
import { DownloadSessionManager } from "../download";

export interface OverlaySetupResult {
  downloadManager: DownloadSessionManager;
}

export interface OverlaySetupContext {
  state: TerminalState;
  overlayRoot: HTMLElement;
  getPtyClient(): PtyClient | null;
  getImeHandler(): ImeHandler | null;
}

/**
 * Wire the overlay-bound managers (markdown, data viewer, download)
 * into the freshly created state + overlay root, plus the IME
 * blur/focus pairing for the markdown and data-viewer fullscreen views.
 *
 * Returns the constructed `DownloadSessionManager` so the host can
 * dispose it later. The other two managers live on `TerminalState` and
 * are torn down with the state itself.
 */
export function setupOverlayBindings(ctx: OverlaySetupContext): OverlaySetupResult {
  const { state, overlayRoot } = ctx;

  // Set markdown session manager's container for fullscreen view
  state.getMarkdownManager().setContainer(overlayRoot);

  // Wire PTY write callback for markdown navigation (navigate/image/quit commands)
  state.getMarkdownManager().setPtyWriteCallback((data: string) => {
    ctx.getPtyClient()?.write(new TextEncoder().encode(data));
  });

  // Set data viewer session manager's container
  state.getDataViewerManager().setContainer(overlayRoot);

  // Initialize download session manager
  const downloadManager = new DownloadSessionManager();
  downloadManager.setContainer(overlayRoot);

  // Wire up IME blur/focus for fullscreen markdown view (same pattern as ImageViewer)
  const fullscreenView = state.getMarkdownManager().getFullscreenView();
  fullscreenView.onShow(() => {
    ctx.getImeHandler()?.blur();
  });
  fullscreenView.onHide(() => {
    ctx.getImeHandler()?.focus();
  });

  // Wire up IME blur/focus for data viewer
  const dataViewerFullscreen = state.getDataViewerManager().getFullscreenView();
  dataViewerFullscreen.onShow(() => {
    ctx.getImeHandler()?.blur();
  });
  dataViewerFullscreen.onHide(() => {
    ctx.getImeHandler()?.focus();
  });

  return { downloadManager };
}
