/**
 * DCS (Device Control String) handlers.
 *
 * Handles DCS sequences, primarily for SIXEL graphics.
 */

import type { TerminalStateAccessor } from "./types.ts";
import type { DcsAction } from "../../types/terminal.ts";

/**
 * Dispatch DCS action to specific handler.
 *
 * @param state - Terminal state accessor
 * @param action - DCS action to dispatch
 */
export function handleDcsDispatch(
  _state: TerminalStateAccessor,
  action: DcsAction
): void {
  switch (action.action) {
    case "Sixel":
      // Store SIXEL action for frontend processing
      // The backend decodes SIXEL to RGBA and sends via image event
      // console.debug("SIXEL data received");
      break;

    case "Unknown":
      // Unknown DCS sequences are ignored
      break;
  }
}
