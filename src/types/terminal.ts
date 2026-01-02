/**
 * Terminal action types for ANSI sequence processing.
 *
 * These types correspond to the Rust backend types defined in
 * src-tauri/src/ansi/sequence.rs
 */

/**
 * Erase mode for ED (Erase in Display) and EL (Erase in Line).
 */
export type EraseMode = "Below" | "Above" | "All" | "Scrollback";

/**
 * Character set designations for G0/G1 switching.
 */
export type CharSet = "Ascii" | "DecLineDrawing" | "Uk";

/**
 * CSI (Control Sequence Introducer) actions.
 */
export type CsiAction =
  | { action: "Sgr"; data: number[] }
  | { action: "CursorUp"; data: number }
  | { action: "CursorDown"; data: number }
  | { action: "CursorForward"; data: number }
  | { action: "CursorBack"; data: number }
  | { action: "CursorNextLine"; data: number }
  | { action: "CursorPreviousLine"; data: number }
  | { action: "CursorHorizontalAbsolute"; data: number }
  | { action: "CursorPosition"; data: { row: number; col: number } }
  | { action: "CursorVerticalAbsolute"; data: number }
  | { action: "EraseInDisplay"; data: EraseMode }
  | { action: "EraseInLine"; data: EraseMode }
  | { action: "InsertLines"; data: number }
  | { action: "DeleteLines"; data: number }
  | { action: "InsertCharacters"; data: number }
  | { action: "DeleteCharacters"; data: number }
  | { action: "EraseCharacters"; data: number }
  | { action: "ScrollUp"; data: number }
  | { action: "ScrollDown"; data: number }
  | { action: "SetScrollRegion"; data: { top: number; bottom: number } }
  | { action: "DeviceStatusReport"; data: number }
  | { action: "PrimaryDeviceAttributes" }
  | { action: "SecondaryDeviceAttributes" }
  | { action: "TertiaryDeviceAttributes" }
  | { action: "SetMode"; data: number[] }
  | { action: "ResetMode"; data: number[] }
  | {
      action: "Unknown";
      data: { params: number[]; intermediates: number[]; final_byte: number };
    };

/**
 * ESC (Escape) sequence actions.
 */
export type EscAction =
  | { action: "SaveCursor" }
  | { action: "RestoreCursor" }
  | { action: "Index" }
  | { action: "NextLine" }
  | { action: "HorizontalTabSet" }
  | { action: "ReverseIndex" }
  | { action: "ResetToInitialState" }
  | { action: "SetG0CharSet"; data: CharSet }
  | { action: "SetG1CharSet"; data: CharSet }
  | { action: "Unknown"; data: number };

/**
 * OSC (Operating System Command) actions.
 */
export type OscAction =
  | { action: "SetTitleAndIcon"; data: string }
  | { action: "SetIconName"; data: string }
  | { action: "SetTitle"; data: string }
  | { action: "SetColorPalette"; index: number; color: string }
  | { action: "SetWorkingDirectory"; data: string }
  | { action: "Hyperlink"; params: string; uri: string }
  | { action: "SetForegroundColor"; data: string }
  | { action: "SetBackgroundColor"; data: string }
  | { action: "EmtermExtension"; verb: string; params: string[] }
  | { action: "Unknown"; ps: number; data: string };

/**
 * A terminal action emitted by the ANSI parser.
 */
export type TerminalAction =
  | { type: "Print"; value: string }
  | { type: "Execute"; value: number }
  | { type: "Csi"; value: CsiAction }
  | { type: "Esc"; value: EscAction }
  | { type: "Osc"; value: OscAction };

/**
 * Payload for the terminal_actions event.
 */
export interface TerminalActionsPayload {
  session_id: string;
  actions: TerminalAction[];
}

/**
 * C0 control character constants.
 */
export const C0 = {
  NUL: 0x00,
  BEL: 0x07,
  BS: 0x08,
  HT: 0x09,
  LF: 0x0a,
  VT: 0x0b,
  FF: 0x0c,
  CR: 0x0d,
  SO: 0x0e,
  SI: 0x0f,
  ESC: 0x1b,
} as const;
