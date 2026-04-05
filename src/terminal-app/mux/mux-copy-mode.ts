/**
 * Mux copy mode functions extracted from TerminalApp.
 * Handles entering/exiting copy mode, key handling, clipboard operations.
 */

import type { TerminalState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import type { PtyClient } from "../../pty/client";
import {
  CopyModeManager,
  ViKeybinds,
  EmacsKeybinds,
  type CopyModeSelection,
} from "../../terminal/mux-copy-mode";
import { SettingsService } from "../../settings/settings-service";
import { muxLog } from "../../terminal/mux/mux-logger";

/** Subset of TerminalApp state needed by copy mode functions. */
export interface MuxCopyModeContext {
  readonly state: TerminalState | null;
  readonly renderer: ITerminalRenderer | null;
  readonly ptyClient: PtyClient | null;
  readonly inMuxMode: boolean;
  copyModeManager: CopyModeManager | null;
  copyModeKeybinds: ViKeybinds | EmacsKeybinds | null;
  onCopyModeIndicatorChange?: (active: boolean) => void;
}

/** Enter mux copy mode with vi or emacs keybindings. */
export function enterCopyMode(ctx: MuxCopyModeContext): void {
  if (!ctx.state || !ctx.inMuxMode) return;

  const core = ctx.state.getWasmCore();
  const cols = core.cols();
  const rows = core.rows();

  ctx.copyModeManager = new CopyModeManager();

  // Default to vi keybindings (no copy_mode setting exists yet)
  const muxSettings = SettingsService.getCached()?.mux;
  const mode = (muxSettings as unknown as Record<string, unknown> | undefined)?.copy_mode as string | undefined ?? "vi";

  if (mode === "emacs") {
    ctx.copyModeKeybinds = new EmacsKeybinds(ctx.copyModeManager, cols, rows);
  } else {
    ctx.copyModeKeybinds = new ViKeybinds(ctx.copyModeManager, cols, rows);
  }

  ctx.copyModeManager.setOnStateChange((state) => {
    if (state === "inactive") {
      exitCopyMode(ctx);
    }
  });

  ctx.copyModeManager.setOnSelectionChange(() => {
    if (ctx.renderer && ctx.state) {
      ctx.renderer.forceRender(ctx.state);
    }
  });

  ctx.copyModeManager.enter();
  ctx.onCopyModeIndicatorChange?.(true);
  muxLog.info("Entered mux copy mode");
}

/** Exit mux copy mode. */
export function exitCopyMode(ctx: MuxCopyModeContext): void {
  ctx.copyModeManager = null;
  ctx.copyModeKeybinds = null;
  ctx.onCopyModeIndicatorChange?.(false);
  if (ctx.renderer && ctx.state) {
    ctx.renderer.forceRender(ctx.state);
  }
  muxLog.info("Exited mux copy mode");
}

/** Handle keyboard input during copy mode. Returns true if the key was consumed. */
export function handleCopyModeKey(ctx: MuxCopyModeContext, event: KeyboardEvent): boolean {
  if (!ctx.copyModeManager || !ctx.copyModeKeybinds) return false;
  if (!ctx.copyModeManager.isActive) return false;

  // Save selection before handling key (yank clears it and exits copy mode)
  const preYankSelection = ctx.copyModeManager.getSelection();

  let consumed: boolean;
  if (ctx.copyModeKeybinds instanceof EmacsKeybinds) {
    consumed = ctx.copyModeKeybinds.handleKeyEvent(event);
  } else {
    consumed = (ctx.copyModeKeybinds as ViKeybinds).handleKeyEvent(event);
  }

  if (!consumed) return false;

  // If copy mode just exited and we had a selection, it was a yank/copy action
  // Note: yank() -> exit() -> onStateChange -> exitCopyMode sets ctx.copyModeManager to null
  if (!ctx.copyModeManager?.isActive && preYankSelection) {
    copySelectionToClipboard(ctx, preYankSelection);
  }

  return true;
}

/** Extract text from the terminal grid for the given selection and copy to clipboard. */
export async function copySelectionToClipboard(ctx: MuxCopyModeContext, selection: CopyModeSelection): Promise<void> {
  if (!ctx.state) return;

  const text = ctx.state.extractText(
    selection.startCol,
    selection.startRow,
    selection.endCol,
    selection.endRow,
  );

  if (!text) return;

  try {
    await navigator.clipboard.writeText(text);
    muxLog.info(`Copy mode: copied ${text.length} chars to clipboard`);
  } catch (e) {
    muxLog.error(`Copy mode clipboard write failed: ${e}`);
  }
}

/** Paste clipboard text into the active PTY (mux paste action). */
export async function pasteFromClipboard(ctx: MuxCopyModeContext): Promise<void> {
  try {
    const text = await navigator.clipboard.readText();
    if (text && ctx.ptyClient) {
      const data = new TextEncoder().encode(text);
      await ctx.ptyClient.write(data);
      muxLog.info(`Mux paste: ${text.length} chars`);
    }
  } catch (e) {
    muxLog.error(`Mux paste failed: ${e}`);
  }
}
