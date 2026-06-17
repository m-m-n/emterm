/**
 * Terminal handlers entry point.
 *
 * Re-exports all handler functions for use by TerminalState.
 */

// Export types
export type { TerminalStateAccessor, ActiveCharSet } from "./types.ts";

// Export semantics utilities
export { CSI_DEFAULTS, toZeroIndexed, clampPosition } from "./semantics.ts";

// Export CSI cursor handlers
export {
  handleCursorUp,
  handleCursorDown,
  handleCursorForward,
  handleCursorBack,
  handleCursorNextLine,
  handleCursorPreviousLine,
  handleCursorHorizontalAbsolute,
  handleCursorVerticalAbsolute,
  handleCursorPosition,
} from "./csi_cursor.ts";

// Export CSI screen handlers
export {
  handleEraseInDisplay,
  handleEraseInLine,
  handleEraseCharacters,
} from "./csi_screen.ts";

// Export CSI edit handlers
export {
  handleInsertLines,
  handleDeleteLines,
  handleInsertCharacters,
  handleDeleteCharacters,
} from "./csi_edit.ts";

// Export CSI scrolling handlers
export {
  handleScrollUp,
  handleScrollDown,
  handleSetScrollRegion,
} from "./csi_scrolling.ts";

// Export CSI modes handler
export { handleSetMode } from "./csi_modes.ts";

// Export CSI device handlers
export {
  handleDeviceStatusReport,
  handlePrimaryDeviceAttributes,
  handleSecondaryDeviceAttributes,
  handleTertiaryDeviceAttributes,
} from "./csi_device.ts";

// Export ESC handlers
export {
  handleEscDispatch,
  handleSaveCursor,
  handleRestoreCursor,
  handleIndex,
  handleNextLine,
  handleReverseIndex,
  handleHorizontalTabSet,
  handleResetToInitialState,
  handleSetG0CharSet,
  handleSetG1CharSet,
} from "./esc_handlers.ts";

// Export OSC handlers
export {
  handleOscDispatch,
  handleSetTitle,
  handleSetIconName,
  handleSetTitleAndIcon,
  handleSetWorkingDirectory,
  handleHyperlink,
  handleEmtermExtension,
} from "./osc_handlers.ts";

// Export C0 handlers
export {
  handleExecuteDispatch,
  handleBel,
  handleBackspace,
  handleTab,
  handleLineFeed,
  handleCarriageReturn,
  handleShiftOut,
  handleShiftIn,
} from "./c0_handlers.ts";

// Export APC handlers
export { handleApcDispatch } from "./apc_handlers.ts";

// Export DCS handlers
export { handleDcsDispatch } from "./dcs_handlers.ts";

// Internal imports for dispatch wrappers
import { handleOscDispatch as oscDispatch } from "./osc_handlers.ts";
import { handleApcDispatch as apcDispatch } from "./apc_handlers.ts";
import { handleDcsDispatch as dcsDispatch } from "./dcs_handlers.ts";

import type { TerminalStateAccessor } from "./types.ts";
import type {
  OscAction,
  ApcAction,
  DcsAction,
} from "../../types/terminal.ts";

/**
 * Handle OSC action.
 */
export function handleOsc(state: TerminalStateAccessor, action: OscAction): void {
  oscDispatch(state, action);
}

/**
 * Handle APC action.
 */
export function handleApc(state: TerminalStateAccessor, action: ApcAction): void {
  apcDispatch(state, action);
}

/**
 * Handle DCS action.
 */
export function handleDcs(state: TerminalStateAccessor, action: DcsAction): void {
  dcsDispatch(state, action);
}
