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
  pasteFromClipboard: () => void;
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
      const muxWindows = ctx.getMuxWindows();
      const activeIndex = ctx.getActiveMuxWindowIndex();
      const currentName = muxWindows[activeIndex]?.name ?? "";
      const newName = prompt("Rename window:", currentName);
      if (newName != null && newName !== "") {
        const win = muxWindows[activeIndex];
        if (win) {
          win.name = newName;
          ctx.emitMuxStateChange();
        }
        // Notify daemon: RenameWindowMsg { name: String }
        // bincode for String = u64 length (LE) + UTF-8 bytes
        // Send active pane ID — daemon resolves pane→window internally.
        const nameBytes = new TextEncoder().encode(newName);
        const payload = new Uint8Array(8 + nameBytes.length);
        const view = new DataView(payload.buffer);
        view.setBigUint64(0, BigInt(nameBytes.length), true);
        payload.set(nameBytes, 8);
        const paneId = ctx.getMuxPaneIds()[activeIndex] ?? 0;
        sendMuxControl(ctx, MuxMessageType.RenameWindow, paneId, payload);
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
    case "paste":
      ctx.pasteFromClipboard();
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
