/**
 * APC (Application Program Command) handlers.
 *
 * Handles APC sequences for Kitty Graphics Protocol and mux inband protocol.
 */

import type { TerminalStateAccessor } from "./types.ts";
import type { ApcAction } from "../../types/terminal.ts";
import { MUX_APC_PREFIX, decodeApcPayload } from "../mux/mux-client.ts";
import type { MuxClient } from "../mux/mux-client.ts";

/** Context for mux APC handling. */
export interface MuxApcContext {
  getMuxClient: () => MuxClient | null;
}

/** Module-level mux APC context, set by TerminalApp. */
let muxApcContext: MuxApcContext | null = null;

/** Register the mux APC context for handling incoming mux messages. */
export function setMuxApcContext(ctx: MuxApcContext | null): void {
  muxApcContext = ctx;
}

/**
 * Check if raw APC data is a mux message and handle it.
 * Returns true if the message was handled (mux APC), false otherwise.
 *
 * This is called from the APC callback chain with the raw APC body bytes.
 */
export function handleMuxApc(data: Uint8Array): boolean {
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
    console.warn("[WARN][FRONTEND] Failed to decode mux APC payload");
    return true; // consumed but invalid
  }

  const muxClient = muxApcContext?.getMuxClient();
  if (muxClient) {
    muxClient.handleIncomingApc(parsed.msgType, parsed.paneId, parsed.data);
  } else {
    console.warn("[WARN][FRONTEND] Mux APC received but no MuxClient active");
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
