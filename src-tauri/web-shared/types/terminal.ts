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
  | { action: "EmtermExtension"; data: { verb: string; params: string[] } }
  | {
      action: "SemanticPrompt";
      data: { zone_type: string; exit_code: number | null };
    }
  | { action: "Unknown"; ps: number; data: string };

/**
 * Kitty Graphics Protocol action types.
 */
export type KittyAction =
  | "Transmit"
  | "TransmitAndDisplay"
  | "Put"
  | "Delete"
  | "Query"
  | "Frame"
  | "Animate"
  | "Compose";

/**
 * Kitty image format.
 */
export type KittyFormat = "Rgb" | "Rgba" | "Png";

/**
 * Kitty delete target.
 */
export type KittyDeleteTarget =
  | "All"
  | "AllIncludingHidden"
  | "ById"
  | "ByPlacement"
  | "AtCursor"
  | "AtCursorByColumns"
  | "AtPosition"
  | "AtCell"
  | "ByZIndex";

/**
 * Kitty Graphics Protocol command.
 */
export interface KittyCommand {
  action: KittyAction;
  image_id?: number;
  placement_id?: number;
  transmission?: "Direct" | "File" | "TempFile" | "SharedMemory";
  format?: KittyFormat;
  compression?: "Zlib";
  width?: number;
  height?: number;
  more: boolean;
  columns?: number;
  rows?: number;
  x_offset?: number;
  y_offset?: number;
  z_index?: number;
  cursor_movement?: number;
  delete_target?: KittyDeleteTarget;
  quiet?: number;
  payload: string;
}

/**
 * APC (Application Program Command) actions.
 * Used for Kitty Graphics Protocol.
 */
export type ApcAction =
  | { action: "KittyGraphics"; data: KittyCommand }
  | { action: "Unknown"; data: string };

/**
 * SIXEL aspect ratio.
 */
export type SixelAspectRatio =
  | "TwoToOne"
  | "FiveToOne"
  | "ThreeToOne"
  | "OneToOne";

/**
 * SIXEL background mode.
 */
export type SixelBackgroundMode = "Transparent" | "UseColorZero";

/**
 * SIXEL image data.
 */
export interface SixelData {
  aspect_ratio: SixelAspectRatio;
  background_mode: SixelBackgroundMode;
  horizontal_grid: number;
}

/**
 * DCS (Device Control String) actions.
 * Used for SIXEL graphics.
 */
export type DcsAction =
  | { action: "Sixel"; data: SixelData }
  | { action: "Unknown"; data: string };

/**
 * A terminal action emitted by the ANSI parser.
 */
export type TerminalAction =
  | { type: "Print"; value: string }
  | { type: "Execute"; value: number }
  | { type: "Csi"; value: CsiAction }
  | { type: "Esc"; value: EscAction }
  | { type: "Osc"; value: OscAction }
  | { type: "Apc"; value: ApcAction }
  | { type: "Dcs"; value: DcsAction };

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

/**
 * Payload for image_event IPC channel.
 *
 * Contains the session ID and the image event data.
 */
export interface ImageEventPayload {
  session_id: string;
  type:
    | "ImageReady"
    | "Place"
    | "Delete"
    | "QueryResponse"
    | "Response"
    | "Animation";
  image?: {
    id: number;
    width: number;
    height: number;
    rgba_base64: string;
  };
  placement?: {
    image_id: number;
    placement_id: number;
    row: number;
    col: number;
    columns: number;
    rows: number;
    x_offset: number;
    y_offset: number;
    z_index: number;
  };
  target?: {
    type: string;
    id?: number;
    image_id?: number;
    placement_id?: number;
  };
  supported?: boolean;
  data?: unknown;
}
