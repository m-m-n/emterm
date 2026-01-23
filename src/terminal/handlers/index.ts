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

// Export CSI character attributes handler
export { handleSgr } from "./csi_char_attrs.ts";

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

// Export print handler
export {
  handlePrintDispatch,
  translateCharacter,
  translateLineDrawing,
} from "./print_handler.ts";

// Export APC handlers
export { handleApcDispatch } from "./apc_handlers.ts";

// Export DCS handlers
export { handleDcsDispatch } from "./dcs_handlers.ts";

// Internal imports for CSI dispatch (renamed to avoid export conflicts)
import {
  handleCursorUp as cursorUp,
  handleCursorDown as cursorDown,
  handleCursorForward as cursorForward,
  handleCursorBack as cursorBack,
  handleCursorNextLine as cursorNextLine,
  handleCursorPreviousLine as cursorPreviousLine,
  handleCursorHorizontalAbsolute as cursorHorizontalAbsolute,
  handleCursorVerticalAbsolute as cursorVerticalAbsolute,
  handleCursorPosition as cursorPosition,
} from "./csi_cursor.ts";
import {
  handleEraseInDisplay as eraseInDisplay,
  handleEraseInLine as eraseInLine,
  handleEraseCharacters as eraseCharacters,
} from "./csi_screen.ts";
import {
  handleInsertLines as insertLines,
  handleDeleteLines as deleteLines,
  handleInsertCharacters as insertCharacters,
  handleDeleteCharacters as deleteCharacters,
} from "./csi_edit.ts";
import {
  handleScrollUp as scrollUp,
  handleScrollDown as scrollDown,
  handleSetScrollRegion as setScrollRegion,
} from "./csi_scrolling.ts";
import { handleSgr as sgr } from "./csi_char_attrs.ts";
import { handleSetMode as setMode } from "./csi_modes.ts";
import {
  handleDeviceStatusReport as deviceStatusReport,
  handlePrimaryDeviceAttributes as primaryDeviceAttributes,
  handleSecondaryDeviceAttributes as secondaryDeviceAttributes,
} from "./csi_device.ts";

// Internal imports for other handlers
import { handleEscDispatch as escDispatch } from "./esc_handlers.ts";
import { handleOscDispatch as oscDispatch } from "./osc_handlers.ts";
import { handleExecuteDispatch as executeDispatch } from "./c0_handlers.ts";
import { handlePrintDispatch as printDispatch } from "./print_handler.ts";
import { handleApcDispatch as apcDispatch } from "./apc_handlers.ts";
import { handleDcsDispatch as dcsDispatch } from "./dcs_handlers.ts";

import type { TerminalStateAccessor } from "./types.ts";
import type {
  CsiAction,
  EscAction,
  OscAction,
  ApcAction,
  DcsAction,
} from "../../types/terminal.ts";

/**
 * Handle Print action.
 */
export function handlePrint(state: TerminalStateAccessor, char: string): void {
  printDispatch(state, char);
}

/**
 * Handle Execute (C0 control) action.
 */
export function handleExecute(state: TerminalStateAccessor, code: number): void {
  executeDispatch(state, code);
}

/**
 * Handle CSI action.
 * Dispatches to specific CSI handlers.
 */
export function handleCsi(state: TerminalStateAccessor, action: CsiAction): void {
  switch (action.action) {
    case "CursorUp":
      cursorUp(state, action.data);
      break;
    case "CursorDown":
      cursorDown(state, action.data);
      break;
    case "CursorForward":
      cursorForward(state, action.data);
      break;
    case "CursorBack":
      cursorBack(state, action.data);
      break;
    case "CursorNextLine":
      cursorNextLine(state, action.data);
      break;
    case "CursorPreviousLine":
      cursorPreviousLine(state, action.data);
      break;
    case "CursorHorizontalAbsolute":
      cursorHorizontalAbsolute(state, action.data);
      break;
    case "CursorVerticalAbsolute":
      cursorVerticalAbsolute(state, action.data);
      break;
    case "CursorPosition":
      cursorPosition(state, action.data.row, action.data.col);
      break;
    case "EraseInDisplay":
      eraseInDisplay(state, action.data);
      break;
    case "EraseInLine":
      eraseInLine(state, action.data);
      break;
    case "EraseCharacters":
      eraseCharacters(state, action.data);
      break;
    case "InsertLines":
      insertLines(state, action.data);
      break;
    case "DeleteLines":
      deleteLines(state, action.data);
      break;
    case "InsertCharacters":
      insertCharacters(state, action.data);
      break;
    case "DeleteCharacters":
      deleteCharacters(state, action.data);
      break;
    case "ScrollUp":
      scrollUp(state, action.data);
      break;
    case "ScrollDown":
      scrollDown(state, action.data);
      break;
    case "SetScrollRegion":
      setScrollRegion(state, action.data.top, action.data.bottom);
      break;
    case "Sgr":
      sgr(state, action.data);
      break;
    case "SetMode":
      setMode(state, action.data, true);
      break;
    case "ResetMode":
      setMode(state, action.data, false);
      break;
    case "DeviceStatusReport":
      deviceStatusReport(state, action.data);
      break;
    case "PrimaryDeviceAttributes":
      primaryDeviceAttributes(state);
      break;
    case "SecondaryDeviceAttributes":
      secondaryDeviceAttributes(state);
      break;
    case "TertiaryDeviceAttributes":
      // Currently ignored
      break;
    case "Unknown":
      // Log unknown sequences for debugging
      // console.debug("Unknown CSI:", action.data);
      break;
  }
}

/**
 * Handle ESC action.
 */
export function handleEsc(state: TerminalStateAccessor, action: EscAction): void {
  escDispatch(state, action);
}

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
