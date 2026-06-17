/**
 * CSI device response handlers.
 *
 * Handles device status report and device attributes queries.
 */

import type { TerminalStateAccessor } from "./types.ts";

/**
 * Handle DeviceStatusReport (CSI Ps n).
 *
 * Respond to device status queries.
 *
 * @param state - Terminal state accessor
 * @param ps - DSR parameter (5 = status, 6 = cursor position)
 */
export function handleDeviceStatusReport(
  state: TerminalStateAccessor,
  ps: number
): void {
  let response: Uint8Array | null = null;

  switch (ps) {
    case 5:
      // Device Status Report - respond with OK
      // CSI 0 n
      response = new Uint8Array([0x1b, 0x5b, 0x30, 0x6e]); // ESC [ 0 n
      break;

    case 6: {
      // Cursor Position Report - respond with CSI row ; col R
      // Note: ANSI positions are 1-indexed
      const row = state.cursor.row + 1;
      const col = state.cursor.col + 1;
      const responseStr = `\x1b[${row};${col}R`;
      response = new TextEncoder().encode(responseStr);
      break;
    }

    default:
      // Unknown DSR, ignore
      break;
  }

  // Add to response buffer if we generated a response
  if (response) {
    state.addPendingResponse(response);
  }
}

/**
 * Handle PrimaryDeviceAttributes (CSI c / CSI 0 c).
 *
 * Respond with device capabilities.
 * Response: CSI ? 65 ; 1 ; 4 ; 22 c (VT500 with 132-col, Sixel, ANSI color)
 *
 * @param state - Terminal state accessor
 */
export function handlePrimaryDeviceAttributes(
  state: TerminalStateAccessor
): void {
  // Report as VT500 with:
  // 65 = VT500
  // 1 = 132 columns
  // 4 = Sixel graphics
  // 22 = ANSI color
  const response = "\x1b[?65;1;4;22c";
  state.addPendingResponse(new TextEncoder().encode(response));
}

/**
 * Handle SecondaryDeviceAttributes (CSI > c / CSI > 0 c).
 *
 * Respond with terminal identification.
 * Response: CSI > 65 ; 1 ; 0 c (VT500 series, version 1, no ROM cartridge)
 *
 * @param state - Terminal state accessor
 */
export function handleSecondaryDeviceAttributes(
  state: TerminalStateAccessor
): void {
  // Report as VT500 series (65), version 1, no ROM cartridge
  const response = "\x1b[>65;1;0c";
  state.addPendingResponse(new TextEncoder().encode(response));
}

/**
 * Handle TertiaryDeviceAttributes (CSI = c / CSI = 0 c).
 *
 * Tertiary DA is rarely used. Currently ignored.
 *
 * @param _state - Terminal state accessor (unused)
 */
export function handleTertiaryDeviceAttributes(
  _state: TerminalStateAccessor
): void {
  // Tertiary DA is rarely used, currently ignore
  // Could respond with DCS ! | ... ST for unit ID
}
