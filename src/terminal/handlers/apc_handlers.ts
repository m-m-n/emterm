/**
 * APC (Application Program Command) handlers.
 *
 * Handles APC sequences, primarily for Kitty Graphics Protocol.
 */

import type { TerminalStateAccessor } from "./types.ts";
import type { ApcAction } from "../../types/terminal.ts";

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
