/**
 * CSI mode handlers.
 *
 * Handles DEC private mode set/reset sequences.
 */

import type { TerminalStateAccessor } from "./types.ts";
import { setDecPrivateMode } from "../modes.ts";

/**
 * Handle SetMode/ResetMode (CSI ? Pm h / CSI ? Pm l).
 *
 * Enable or disable DEC private modes.
 * Modes are processed in order, with actions collected and executed after all mode updates.
 *
 * @param state - Terminal state accessor
 * @param modes - Array of mode numbers
 * @param enable - true for SetMode, false for ResetMode
 */
export function handleSetMode(
  state: TerminalStateAccessor,
  modes: number[],
  enable: boolean
): void {
  // Collect actions to execute after mode state updates
  const actions: Array<() => void> = [];

  // Update mode state for all modes first
  for (const mode of modes) {
    const result = setDecPrivateMode(state.modes, mode, enable);

    if (result.action) {
      // Queue action for execution after all mode updates
      const action = result.action;
      actions.push(() => {
        switch (action) {
          case "saveAndSwitchToAlt":
            state.switchToAlternateBuffer(true);
            break;
          case "switchToAlt":
            state.switchToAlternateBuffer(false);
            break;
          case "switchToMain":
            state.switchToPrimaryBuffer(true);
            break;
          case "saveCursor":
            state.cursor.save();
            break;
          case "restoreCursor":
            state.cursor.restore();
            break;
        }
      });
    }
  }

  // Execute actions in order after all mode state is updated
  // (must run before syncModesToWasm so modes are synced to the correct active buffer)
  for (const action of actions) {
    action();
  }

  // Sync boolean modes to WASM bitfield of the now-active buffer
  state.syncModesToWasm();
}
