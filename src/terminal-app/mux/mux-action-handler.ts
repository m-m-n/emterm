/**
 * Mux action handler functions extracted from TerminalApp.
 * Handles mux prefix-key action dispatch, control message sending,
 * prefix key byte conversion, and active pane ID resolution.
 */

import { MuxMessageType } from "../../terminal/mux/mux-client";
import { muxLog } from "../../terminal/mux/mux-logger";
import type { MuxClient } from "../../terminal/mux/mux-client";
import type { MuxAction } from "../../terminal/mux/prefix-key";
import type { PtyClient } from "../../pty/client";
import { SettingsService } from "../../settings/settings-service";
import { showRenameWindowDialog } from "./rename-window-dialog";

/** Guard against concurrent rename dialogs. Module-local state. */
let renameDialogOpen = false;

/** Subset of TerminalApp state needed by mux action handler functions. */
export interface MuxActionContext {
  getMuxClient: () => MuxClient | null;
  getPtyClient: () => PtyClient | null;
  getMuxWindows: () => { id: number; name: string }[];
  getActiveMuxWindowIndex: () => number;
  setActiveMuxWindowIndex: (index: number) => void;
  getMuxPaneIds: () => number[];
  getMuxPendingWindowCount: () => number;
  setMuxPendingWindowCount: (count: number) => void;

  // Delegate methods that call into other mux modules via TerminalApp wrappers
  switchMuxWindow: (previousIndex?: number) => void;
  emitMuxStateChange: () => void;
  exitMuxMode: () => void;
}

/** Handle mux action dispatched by PrefixKeyHandler. */
export function handleMuxAction(ctx: MuxActionContext, action: MuxAction): void {
  muxLog.info(`Mux action: ${action.type}`);

  switch (action.type) {
    case "detach":
      // Send Detach to daemon; exitMuxMode is triggered by the onDetached callback
      // when the daemon responds with Detached.
      muxLog.info("Sending Detach, waiting for Detached response via onDetached callback");
      ctx.getMuxClient()?.sendControl(MuxMessageType.Detach, 0).catch(() => {});
      break;
    case "new-window": {
      // Actual pane ID will arrive via PaneCreated event
      ctx.setMuxPendingWindowCount(ctx.getMuxPendingWindowCount() + 1);
      sendMuxControl(ctx, MuxMessageType.CreateWindow, 0);
      break;
    }
    case "next-window": {
      const muxWindows = ctx.getMuxWindows();
      if (muxWindows.length > 1) {
        const prev = ctx.getActiveMuxWindowIndex();
        ctx.setActiveMuxWindowIndex((prev + 1) % muxWindows.length);
        ctx.switchMuxWindow(prev);
      }
      break;
    }
    case "prev-window": {
      const muxWindows = ctx.getMuxWindows();
      if (muxWindows.length > 1) {
        const prev = ctx.getActiveMuxWindowIndex();
        ctx.setActiveMuxWindowIndex((prev - 1 + muxWindows.length) % muxWindows.length);
        ctx.switchMuxWindow(prev);
      }
      break;
    }
    case "rename-window": {
      if (renameDialogOpen) break;
      const activeIndex = ctx.getActiveMuxWindowIndex();
      const target = ctx.getMuxWindows()[activeIndex];
      if (!target) break;
      // Capture stable window id so we can re-resolve after the async dialog,
      // even if the active index shifts due to concurrent mux state changes.
      const targetWinId = target.id;
      const currentName = target.name;
      renameDialogOpen = true;
      try {
        showRenameWindowDialog({ currentName }).then((result) => {
          if (!result.confirmed || result.name === "") return;
          const currentWindows = ctx.getMuxWindows();
          const currentIdx = currentWindows.findIndex((w) => w.id === targetWinId);
          if (currentIdx < 0) return; // window was closed during the dialog
          const paneId = ctx.getMuxPaneIds()[currentIdx];
          if (paneId === undefined) return; // no pane for this window — abort
          const win = currentWindows[currentIdx];
          if (win) {
            win.name = result.name;
            ctx.emitMuxStateChange();
          }
          // Notify daemon: RenameWindowMsg { name: String }
          // bincode for String = u64 length (LE) + UTF-8 bytes
          // Send active pane ID — daemon resolves pane→window internally.
          const nameBytes = new TextEncoder().encode(result.name);
          const payload = new Uint8Array(8 + nameBytes.length);
          const view = new DataView(payload.buffer);
          view.setBigUint64(0, BigInt(nameBytes.length), true);
          payload.set(nameBytes, 8);
          sendMuxControl(ctx, MuxMessageType.RenameWindow, paneId, payload);
        }).catch((e) => {
          muxLog.error(`Rename dialog failed: ${e}`);
        }).finally(() => {
          renameDialogOpen = false;
        });
      } catch (e) {
        // Synchronous throw before the Promise chain was established
        // (e.g., DOM setup failure in showRenameWindowDialog).
        renameDialogOpen = false;
        muxLog.error(`Rename dialog setup failed: ${e}`);
      }
      break;
    }
    case "prefix-passthrough":
      // Send the prefix key itself to PTY
      {
        const ptyClient = ctx.getPtyClient();
        if (ptyClient) {
          const muxSettings = SettingsService.getCached()?.mux;
          const prefix = muxSettings?.prefix ?? "Ctrl+B";
          const byte = prefixKeyToByte(prefix);
          if (byte !== null) {
            ptyClient.write(new Uint8Array([byte])).catch(() => {});
          }
        }
      }
      break;
  }
}

/** Send a control message to the mux daemon. */
export function sendMuxControl(ctx: MuxActionContext, msgType: number, paneId: number, payload?: Uint8Array): void {
  const muxClient = ctx.getMuxClient();
  if (!muxClient) return;
  muxClient.sendControl(msgType, paneId, payload).catch((e) => {
    muxLog.error(`Mux control failed (type=0x${msgType.toString(16)}): ${e}`);
  });
}

/** Convert a prefix keybind string to a control byte (e.g., "Ctrl+B" -> 0x02). */
export function prefixKeyToByte(prefix: string): number | null {
  const match = prefix.match(/^Ctrl\+([A-Z])$/i);
  if (match) {
    return match[1]!.toUpperCase().charCodeAt(0) - 0x40; // Ctrl+A=1, Ctrl+B=2, etc.
  }
  return null;
}

/** Get the active mux pane ID (single-pane mode). */
export function getActiveMuxPaneId(ctx: MuxActionContext): number | null {
  const paneIds = ctx.getMuxPaneIds();
  return paneIds[ctx.getActiveMuxWindowIndex()] ?? null;
}
