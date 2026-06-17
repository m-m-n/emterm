/**
 * Terminal state action processing.
 *
 * Functions for processing terminal actions (CSI, ESC, modes)
 * and managing grapheme buffer flushing.
 * Extracted from TerminalState for separation of concerns.
 */

import type { CharSet, CsiAction, EscAction, EraseMode, TerminalAction } from "../types/terminal.ts";
import type { UnifiedBuffer } from "./unified-buffer.ts";
import type { CursorState } from "./cursor.ts";
import { cloneAttributes } from "./attributes.ts";
import { setDecPrivateMode, syncModesFromWasm, syncModesToWasm, type TerminalModes } from "./modes.ts";
import { isEmojiPresentation } from "./wasm/unicode.ts";
import type { WasmGrid } from "./wasm/terminal-core.ts";
import type { Cell } from "./grid.ts";

import {
  handleOsc,
  handleApc,
  handleDcs,
} from "./handlers/index.ts";
import type { TerminalStateAccessor } from "./handlers/types.ts";

// ── WASM Sentinel Constants ─────────────────────────────
const WASM_BEL_SENTINEL = 0xFE;
const WASM_SCROLLBACK_SENTINEL = 0xFF;

// ── WASM Mode Action Codes (mirror Rust constants) ──────
const MODE_ACTION_NONE = 0;
const MODE_ACTION_SWITCH_TO_ALT = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT = 2;
const MODE_ACTION_SWITCH_TO_MAIN = 3;
const MODE_ACTION_SAVE_CURSOR = 4;
const MODE_ACTION_RESTORE_CURSOR = 5;
const MODE_ACTION_TS_FALLBACK = 0xFF;

/**
 * Convert CharSet string to numeric byte for WASM ESC handler.
 */
function charSetToByte(charset: CharSet): number {
  switch (charset) {
    case "Ascii": return 0;
    case "DecLineDrawing": return 1;
    case "Uk": return 2;
    default: return 0;
  }
}

/**
 * Convert EraseMode string to numeric mode byte for WASM.
 */
function eraseModeToByte(mode: EraseMode): number {
  switch (mode) {
    case "Below": return 0;
    case "Above": return 1;
    case "All": return 2;
    case "Scrollback": return 3;
  }
}

/**
 * Context needed for action processing.
 * Properties are accessed by reference, so mutations flow back to the owner.
 */
export interface ActionContext {
  getActiveWasmGrid(): WasmGrid | null;
  getActiveBuffer(): UnifiedBuffer;
  readonly cursor: CursorState;
  readonly modes: TerminalModes;
  readonly cols: number;
  readonly rows: number;
  graphemeBuffer: number[];
  wrapPending: boolean;
  readonly onBell?: () => void;
  switchToAlternateBuffer(saveCursor: boolean): void;
  switchToPrimaryBuffer(restoreCursor: boolean): void;
  addPendingResponse(response: Uint8Array): void;
  reset(): void;
}

/**
 * Process a terminal action.
 *
 * Delegates to external handlers in the handlers module.
 *
 * @param state - TerminalStateAccessor for handler access
 * @param ctx - Action context for internal operations
 * @param action - The action to process
 */
export function processAction(
  state: TerminalStateAccessor,
  ctx: ActionContext,
  action: TerminalAction,
): void {
  // Flush grapheme buffer before non-Print actions
  if (action.type !== "Print") {
    const grid = ctx.getActiveWasmGrid();
    const hasBufferedContent = grid
      ? grid.core.get_grapheme_buffer_len() > 0
      : ctx.graphemeBuffer.length > 0;
    if (hasBufferedContent) {
      flushGraphemeBuffer(ctx);
    }
  }

  switch (action.type) {
    case "Print": {
      const grid = ctx.getActiveWasmGrid();
      const cp = action.value.codePointAt(0);
      if (grid && cp !== undefined) {
        const scrollCount = grid.core.handle_print(cp);
        if (scrollCount > 0) {
          const buffer = ctx.getActiveBuffer();
          for (let i = 0; i < scrollCount; i++) {
            buffer.scrollUp();
          }
        }
      }
      break;
    }
    case "Execute": {
      const grid = ctx.getActiveWasmGrid();
      if (grid) {
        const result = grid.core.handle_execute(action.value);
        if (result === WASM_BEL_SENTINEL) {
          ctx.onBell?.();
        } else if (result > 0) {
          const buffer = ctx.getActiveBuffer();
          for (let i = 0; i < result; i++) {
            buffer.scrollUp();
          }
        }
      }
      break;
    }
    case "Csi": {
      const grid = ctx.getActiveWasmGrid();
      if (grid) {
        handleCsiWasm(ctx, grid, action.value);
      }
      break;
    }
    case "Esc": {
      const grid = ctx.getActiveWasmGrid();
      if (grid) {
        handleEscWasm(ctx, grid, action.value);
      }
      break;
    }
    case "Osc":
      handleOsc(state, action.value);
      break;
    case "Apc":
      handleApc(state, action.value);
      break;
    case "Dcs":
      handleDcs(state, action.value);
      break;
  }
}

/**
 * Flush the grapheme cluster buffer.
 *
 * Converts buffered codepoints to a cell string and places it on the grid.
 * Called when a non-extending codepoint arrives or on non-Print actions.
 */
export function flushGraphemeBuffer(ctx: ActionContext): void {
  // WASM path: delegate to WASM core
  const wasmGrid = ctx.getActiveWasmGrid();
  if (wasmGrid) {
    if (wasmGrid.core.get_grapheme_buffer_len() === 0) return;
    const scrollCount = wasmGrid.core.flush_grapheme_buffer();
    if (scrollCount > 0) {
      const buffer = ctx.getActiveBuffer();
      for (let i = 0; i < scrollCount; i++) {
        buffer.scrollUp();
      }
    }
    return;
  }

  // JS fallback path
  if (ctx.graphemeBuffer.length === 0) return;

  const clusterString = String.fromCodePoint(...ctx.graphemeBuffer);
  // Determine width based on presentation properties
  const hasFE0E = ctx.graphemeBuffer.includes(0xfe0e);
  const hasFE0F = ctx.graphemeBuffer.includes(0xfe0f);
  let width: number;
  if (hasFE0E) {
    // Explicit text presentation selector -> narrow
    width = 1;
  } else if (hasFE0F) {
    // Explicit emoji presentation selector -> wide
    width = 2;
  } else if (ctx.graphemeBuffer.length === 1) {
    // Single codepoint: only Emoji_Presentation=Yes characters are wide
    width = isEmojiPresentation(ctx.graphemeBuffer[0]!) ? 2 : 1;
  } else {
    // Multi-codepoint cluster (ZWJ sequence, skin tone, RI pair) -> wide
    width = 2;
  }

  ctx.graphemeBuffer = [];

  const buffer = ctx.getActiveBuffer();
  const { bottom } = buffer.getEffectiveScrollRegion();

  // Handle wrap pending
  if (ctx.wrapPending) {
    ctx.wrapPending = false;
    ctx.cursor.carriageReturn();
    if (ctx.cursor.lineFeed(bottom)) {
      buffer.scrollUp();
    }
    buffer.getLine(ctx.cursor.row).wrapped = true;
  }

  // Wide char wrap: if width 2 and at last column, wrap first
  if (width === 2 && ctx.cursor.col >= ctx.cols - 1) {
    if (ctx.modes.autoWrap) {
      ctx.cursor.carriageReturn();
      if (ctx.cursor.lineFeed(bottom)) {
        buffer.scrollUp();
      }
      buffer.getLine(ctx.cursor.row).wrapped = true;
    }
  }

  // Create cell with cluster string
  const cell: Cell = {
    char: clusterString,
    width: width,
    attrs: cloneAttributes(ctx.cursor.attrs),
    dirty: true,
  };
  buffer.setCell(ctx.cursor.col, ctx.cursor.row, cell);

  // For wide characters, set placeholder in next cell
  if (width === 2 && ctx.cursor.col < ctx.cols - 1) {
    const placeholder: Cell = {
      char: "",
      width: 0,
      attrs: cloneAttributes(ctx.cursor.attrs),
      dirty: true,
    };
    buffer.setCell(ctx.cursor.col + 1, ctx.cursor.row, placeholder);
  }

  // Advance cursor
  const newCol = ctx.cursor.col + width;
  if (newCol >= ctx.cols) {
    if (ctx.modes.autoWrap) {
      ctx.cursor.col = ctx.cols - 1;
      ctx.wrapPending = true;
    }
  } else {
    ctx.cursor.col = newCol;
  }
}

/**
 * Handle a CSI action via WASM.
 * Returns true if handled, false if TS fallback needed.
 */
export function handleCsiWasm(ctx: ActionContext, grid: WasmGrid, action: CsiAction): boolean {
  switch (action.action) {
    case "CursorUp":
      grid.core.handle_cursor_up(action.data || 1);
      return true;
    case "CursorDown":
      grid.core.handle_cursor_down(action.data || 1);
      return true;
    case "CursorForward":
      grid.core.handle_cursor_forward(action.data || 1);
      return true;
    case "CursorBack":
      grid.core.handle_cursor_back(action.data || 1);
      return true;
    case "CursorNextLine":
      grid.core.handle_cursor_next_line(action.data || 1);
      return true;
    case "CursorPreviousLine":
      grid.core.handle_cursor_previous_line(action.data || 1);
      return true;
    case "CursorHorizontalAbsolute":
      grid.core.handle_cursor_horizontal_absolute(action.data || 1);
      return true;
    case "CursorPosition":
      grid.core.handle_cursor_position(
        action.data.row || 1,
        action.data.col || 1
      );
      return true;
    case "CursorVerticalAbsolute":
      grid.core.handle_cursor_vertical_absolute(action.data || 1);
      return true;
    case "EraseInDisplay": {
      const mode = eraseModeToByte(action.data);
      const result = grid.core.handle_erase_in_display(mode);
      if (result === WASM_SCROLLBACK_SENTINEL) {
        // Scrollback: call clearScrollback() directly
        const buffer = ctx.getActiveBuffer();
        buffer.clearScrollback();
      }
      return true;
    }
    case "EraseInLine": {
      const mode = eraseModeToByte(action.data);
      grid.core.handle_erase_in_line(mode);
      return true;
    }
    case "EraseCharacters":
      grid.core.handle_erase_characters(action.data || 1);
      return true;

    // ── SGR ───────────────────────────────
    case "Sgr": {
      const params = new Uint16Array(action.data);
      grid.core.handle_sgr(params);
      return true;
    }

    // ── Edit operations ───────────────────
    case "InsertLines":
      grid.core.handle_insert_lines(action.data || 1);
      return true;
    case "DeleteLines":
      grid.core.handle_delete_lines(action.data || 1);
      return true;
    case "InsertCharacters":
      grid.core.handle_insert_characters(action.data || 1);
      return true;
    case "DeleteCharacters":
      grid.core.handle_delete_characters(action.data || 1);
      return true;

    // ── Scroll operations ─────────────────
    case "ScrollUp": {
      const scrollCount = grid.core.handle_scroll_up(action.data || 1);
      if (scrollCount > 0) {
        const buffer = ctx.getActiveBuffer();
        for (let i = 0; i < scrollCount; i++) {
          buffer.scrollUp();
        }
      }
      return true;
    }
    case "ScrollDown":
      grid.core.handle_scroll_down(action.data || 1);
      return true;
    case "SetScrollRegion": {
      grid.core.handle_decstbm(action.data.top, action.data.bottom);
      // Sync scroll region to UnifiedBuffer (WASM sets its own, buffer needs its copy)
      const top = action.data.top === 0 ? 0 : action.data.top - 1;
      const bottom = action.data.bottom === 0 ? ctx.rows - 1 : action.data.bottom - 1;
      ctx.getActiveBuffer().setScrollRegion(top, bottom);
      return true;
    }

    // ── Mode handling ─────────────────────
    case "SetMode":
      return handleModesWasm(ctx, grid, action.data, true);
    case "ResetMode":
      return handleModesWasm(ctx, grid, action.data, false);

    // ── Device responses ──────────────────
    case "DeviceStatusReport": {
      const len = grid.core.handle_device_status_report(action.data);
      if (len > 0) {
        readAndSendResponse(ctx, grid);
      }
      return true;
    }
    case "PrimaryDeviceAttributes": {
      const len = grid.core.handle_primary_device_attributes();
      if (len > 0) {
        readAndSendResponse(ctx, grid);
      }
      return true;
    }
    case "SecondaryDeviceAttributes": {
      const len = grid.core.handle_secondary_device_attributes();
      if (len > 0) {
        readAndSendResponse(ctx, grid);
      }
      return true;
    }
    case "TertiaryDeviceAttributes":
      return false; // No WASM handler, fallback to TS

    default:
      return false;
  }
}

/**
 * Handle an ESC action via WASM.
 * Maps ESC action names to WASM action codes and dispatches.
 */
export function handleEscWasm(ctx: ActionContext, grid: WasmGrid, action: import("../types/terminal.ts").EscAction): void {
  let actionCode: number;
  let data = 0;

  switch (action.action) {
    case "SaveCursor":
      actionCode = 0;
      break;
    case "RestoreCursor":
      actionCode = 1;
      break;
    case "Index":
      actionCode = 2;
      break;
    case "NextLine":
      actionCode = 3;
      break;
    case "ReverseIndex":
      actionCode = 4;
      break;
    case "HorizontalTabSet":
      actionCode = 5;
      break;
    case "ResetToInitialState":
      actionCode = 6;
      break;
    case "SetG0CharSet":
      actionCode = 7;
      data = charSetToByte(action.data);
      break;
    case "SetG1CharSet":
      actionCode = 8;
      data = charSetToByte(action.data);
      break;
    case "Unknown":
      return; // No-op for unknown ESC sequences
  }

  grid.core.handle_esc(actionCode, data);

  // RIS: also reset TS-side state
  if (actionCode === 6) {
    ctx.reset();
  }
}

/**
 * Process SetMode/ResetMode via WASM with action code dispatch.
 * Handles boolean modes in WASM, falls back to TS for multi-valued modes.
 */
export function handleModesWasm(
  ctx: ActionContext,
  grid: WasmGrid,
  modes: number[],
  enable: boolean,
): boolean {
  const actions: number[] = [];

  for (const mode of modes) {
    const code = grid.core.handle_set_mode(mode, enable);
    if (code === MODE_ACTION_TS_FALLBACK) {
      // Multi-valued mode (mouse, cursor keys, etc.) - handle in TS
      setDecPrivateMode(ctx.modes, mode, enable);
    } else if (code !== MODE_ACTION_NONE) {
      actions.push(code);
    }
  }

  // Execute collected actions after all mode state is updated
  for (const code of actions) {
    executeModAction(ctx, code);
  }

  // Sync boolean modes from WASM to TS
  syncModesFromWasm(ctx.modes, grid.core);

  // Sync TS-only multi-valued modes back to WASM
  syncModesToWasm(ctx.modes, grid.core);

  return true;
}

/**
 * Execute a mode action code from WASM.
 */
export function executeModAction(ctx: ActionContext, code: number): void {
  switch (code) {
    case MODE_ACTION_SWITCH_TO_ALT:
      ctx.switchToAlternateBuffer(false);
      break;
    case MODE_ACTION_SAVE_AND_SWITCH_TO_ALT:
      ctx.switchToAlternateBuffer(true);
      break;
    case MODE_ACTION_SWITCH_TO_MAIN:
      ctx.switchToPrimaryBuffer(true);
      break;
    case MODE_ACTION_SAVE_CURSOR:
      ctx.cursor.save();
      break;
    case MODE_ACTION_RESTORE_CURSOR:
      ctx.cursor.restore();
      break;
  }
}

/**
 * Read device response from WASM and add to pending responses.
 */
export function readAndSendResponse(ctx: ActionContext, grid: WasmGrid): void {
  const bytes = grid.core.get_response_bytes();
  if (bytes.length > 0) {
    ctx.addPendingResponse(bytes);
  }
}
