/**
 * APC (Application Program Command) handlers.
 *
 * Handles APC sequences for Kitty Graphics Protocol and mux inband protocol.
 */

import type { TerminalStateAccessor } from "./types.ts";
import type { ApcAction } from "../../types/terminal.ts";
import { MUX_APC_PREFIX, MuxMessageType, decodeApcPayload } from "../mux/mux-client.ts";
import type { MuxClient } from "../mux/mux-client.ts";
import { muxLog } from "../mux/mux-logger";

/** Context for mux APC handling. */
export interface MuxApcContext {
  getMuxClient: () => MuxClient | null;
  /** Called when a Welcome APC arrives but no MuxClient exists yet (user ran `emterm mux` manually). */
  onWelcomeWithoutClient?: (msgType: number, paneId: number, data: Uint8Array) => void;
}

/**
 * Check if raw APC data is a mux message and handle it.
 * Returns true if the message was handled (mux APC), false otherwise.
 *
 * This is called from the APC callback chain with the raw APC body bytes.
 * The caller provides the per-tab MuxApcContext so that each tab's callbacks
 * are correctly dispatched (no global state).
 */
export function handleMuxApc(data: Uint8Array, ctx: MuxApcContext | null): boolean {
  // Quick prefix check: "emterm-mux;" is ASCII
  if (data.length < MUX_APC_PREFIX.length) return false;

  // Check for ASCII prefix match
  const prefixBytes = MUX_APC_PREFIX;
  for (let i = 0; i < prefixBytes.length; i++) {
    if (data[i] !== prefixBytes.charCodeAt(i)) return false;
  }

  // It's a mux APC message
  const payloadStr = new TextDecoder().decode(data);
  const parsed = decodeApcPayload(payloadStr);
  if (!parsed) {
    muxLog.warn("Failed to decode mux APC payload");
    return true; // consumed but invalid
  }

  muxLog.info(`APC received: type=0x${parsed.msgType.toString(16)} pane=${parsed.paneId} (${data.length} bytes)`);

  const muxClient = ctx?.getMuxClient();
  if (muxClient) {
    muxClient.handleIncomingApc(parsed.msgType, parsed.paneId, parsed.data);
  } else if (parsed.msgType === MuxMessageType.Welcome && ctx?.onWelcomeWithoutClient) {
    // Welcome arrived before MuxClient exists (user typed `emterm mux` manually).
    // Trigger auto-enter mux mode with the bridge already running.
    // MUST defer: this runs inside process_pty_data's APC callback,
    // and enterMuxMode accesses WASM core (cols/rows/swapGrid).
    // Calling core synchronously here causes a recursive borrow deadlock.
    muxLog.info("Welcome received without client, deferring auto-enter mux mode");
    const cb = ctx.onWelcomeWithoutClient;
    ctx.onWelcomeWithoutClient = undefined; // prevent duplicate Welcome (Linux delivers both APC and OSC)
    const { msgType: mt, paneId: pid, data: d } = parsed;
    queueMicrotask(() => cb(mt, pid, d));
  } else {
    muxLog.warn("Mux APC received but no MuxClient active");
  }

  return true;
}

/**
 * Dispatch APC action to specific handler.
 *
 * @param state - Terminal state accessor
 * @param action - APC action to dispatch
 */
export function handleApcDispatch(
  _state: TerminalStateAccessor,
  action: ApcAction
): void {
  switch (action.action) {
    case "KittyGraphics":
      // Store image action for frontend processing
      // The ImageProcessor on the backend will handle actual decoding
      // Frontend receives this for display coordination
      // console.debug("Kitty Graphics command:", action.data.action);
      break;

    case "Unknown":
      // Unknown APC sequences are ignored
      break;
  }
}
