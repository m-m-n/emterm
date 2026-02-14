/**
 * Print handler for terminal characters.
 *
 * Handles printing characters to the terminal buffer.
 */

import type { TerminalStateAccessor } from "./types.ts";
import { charWidth, isCombiningChar, isExtendedPictographic, isRegionalIndicator, isSkinToneModifier, isVariationSelector } from "../wasm/unicode.ts";
import { createAsciiCell, createCell } from "../grid.ts";

/**
 * Handle Print action.
 *
 * Print a character at the current cursor position.
 * Uses fast path for ASCII characters when possible.
 *
 * @param state - Terminal state accessor
 * @param char - Character to print
 */
export function handlePrintDispatch(
  state: TerminalStateAccessor,
  char: string
): void {
  const cp = char.codePointAt(0);
  if (cp === undefined) return;

  // Handle grapheme cluster buffering for emoji sequences
  // Safety limit: flush if buffer exceeds max size (prevents unbounded growth from malicious input)
  if (state.graphemeBuffer.length >= 64) {
    state.flushGraphemeBuffer();
  }

  if (state.graphemeBuffer.length > 0) {
    // Buffer is non-empty - check if codepoint extends the cluster
    if (cp === 0x200d) {
      // ZWJ - extends cluster
      state.graphemeBuffer.push(cp);
      return;
    }
    if (isVariationSelector(cp)) {
      state.graphemeBuffer.push(cp);
      return;
    }
    if (isSkinToneModifier(cp)) {
      state.graphemeBuffer.push(cp);
      return;
    }
    if (isRegionalIndicator(cp)) {
      // Check if buffer started with a single Regional Indicator (length 1)
      if (state.graphemeBuffer.length === 1 && isRegionalIndicator(state.graphemeBuffer[0]!)) {
        // Second RI completes the flag pair - buffer and flush
        state.graphemeBuffer.push(cp);
        state.flushGraphemeBuffer();
        return;
      }
    }
    // Check if last buffered codepoint was ZWJ and current is Extended_Pictographic
    const lastCp = state.graphemeBuffer[state.graphemeBuffer.length - 1]!;
    if (lastCp === 0x200d && isExtendedPictographic(cp)) {
      state.graphemeBuffer.push(cp);
      return;
    }
    // Combining marks extend the cluster
    if (isCombiningChar(cp)) {
      state.graphemeBuffer.push(cp);
      return;
    }

    // Codepoint does not extend cluster - flush buffer, then handle new codepoint
    state.flushGraphemeBuffer();
    // Fall through to handle new codepoint below
  } else {
    // Buffer is empty - check if codepoint should start buffering
    if (isExtendedPictographic(cp) || isRegionalIndicator(cp)) {
      state.graphemeBuffer.push(cp);
      return;
    }
  }

  // Fast path for ASCII characters without line drawing and without wrap pending
  const code = char.charCodeAt(0);
  if (
    code >= 0x20 &&
    code < 0x7f &&
    !state.wrapPending &&
    state.activeCharSet === "G0" &&
    state.g0CharSet === "Ascii"
  ) {
    const buffer = state.getActiveBuffer();
    const newCol = state.cursor.col + 1;
    if (newCol < state.cols) {
      // Simple case: ASCII char, not at end of line
      const cell = createAsciiCell(char, state.cursor.attrs);
      buffer.setCell(state.cursor.col, state.cursor.row, cell);
      state.cursor.col = newCol;
      return;
    } else if (state.modes.autoWrap) {
      // At end of line with autoWrap
      const cell = createAsciiCell(char, state.cursor.attrs);
      buffer.setCell(state.cursor.col, state.cursor.row, cell);
      state.cursor.col = state.cols - 1;
      state.wrapPending = true;
      return;
    }
  }

  // Slow path for complex cases
  handlePrintSlow(state, char);
}

/**
 * Handle printable character - slow path for complex cases.
 *
 * @param state - Terminal state accessor
 * @param char - Character to print
 */
function handlePrintSlow(state: TerminalStateAccessor, char: string): void {
  const buffer = state.getActiveBuffer();
  const width = charWidth(char);
  const { bottom } = buffer.getEffectiveScrollRegion();

  // Apply character set translation if needed
  const translatedChar = translateCharacter(state, char);

  // Handle wrap pending (cursor was at end of line)
  if (state.wrapPending) {
    state.wrapPending = false;
    state.cursor.carriageReturn();
    if (state.cursor.lineFeed(bottom)) {
      buffer.scrollUp();
    }
    // Mark the new line as a continuation (soft wrap)
    buffer.getLine(state.cursor.row).wrapped = true;
  }

  // Check if we need to wrap before printing wide character
  if (width === 2 && state.cursor.col >= state.cols - 1) {
    if (state.modes.autoWrap) {
      state.cursor.carriageReturn();
      if (state.cursor.lineFeed(bottom)) {
        buffer.scrollUp();
      }
      // Mark the new line as a continuation (soft wrap)
      buffer.getLine(state.cursor.row).wrapped = true;
    }
  }

  // Create cell with current attributes
  const cell = createCell(translatedChar, state.cursor.attrs);
  buffer.setCell(state.cursor.col, state.cursor.row, cell);

  // For wide characters, set a placeholder in the next cell
  if (width === 2 && state.cursor.col < state.cols - 1) {
    const placeholder = createCell("", state.cursor.attrs);
    placeholder.width = 0;
    buffer.setCell(state.cursor.col + 1, state.cursor.row, placeholder);
  }

  // Advance cursor
  const newCol = state.cursor.col + width;
  if (newCol >= state.cols) {
    if (state.modes.autoWrap) {
      // Set wrap pending - next character will wrap
      state.cursor.col = state.cols - 1;
      state.wrapPending = true;
    }
  } else {
    state.cursor.col = newCol;
  }
}

/**
 * Translate a character using the active character set.
 *
 * @param state - Terminal state accessor
 * @param char - Character to translate
 * @returns Translated character
 */
export function translateCharacter(
  state: TerminalStateAccessor,
  char: string
): string {
  const charSet =
    state.activeCharSet === "G0" ? state.g0CharSet : state.g1CharSet;

  // Only translate for DEC Line Drawing character set
  if (charSet === "DecLineDrawing") {
    return translateLineDrawing(char);
  }

  return char;
}

/**
 * Translate a character using DEC Line Drawing character set.
 *
 * @param char - Character to translate
 * @returns Translated character (box drawing or original)
 */
export function translateLineDrawing(char: string): string {
  // DEC Special Graphics / Line Drawing character set
  // Maps 0x5F-0x7E to box drawing characters
  const translations: Record<string, string> = {
    _: " ", // Blank
    "`": "\u25C6", // Diamond
    a: "\u2592", // Checkerboard
    b: "\u2409", // HT
    c: "\u240C", // FF
    d: "\u240D", // CR
    e: "\u240A", // LF
    f: "\u00B0", // Degree
    g: "\u00B1", // Plus/minus
    h: "\u2424", // NL
    i: "\u240B", // VT
    j: "\u2518", // Lower right corner
    k: "\u2510", // Upper right corner
    l: "\u250C", // Upper left corner
    m: "\u2514", // Lower left corner
    n: "\u253C", // Crossing lines
    o: "\u23BA", // Horizontal line - scan 1
    p: "\u23BB", // Horizontal line - scan 3
    q: "\u2500", // Horizontal line - scan 5
    r: "\u23BC", // Horizontal line - scan 7
    s: "\u23BD", // Horizontal line - scan 9
    t: "\u251C", // Left tee
    u: "\u2524", // Right tee
    v: "\u2534", // Bottom tee
    w: "\u252C", // Top tee
    x: "\u2502", // Vertical line
    y: "\u2264", // Less than or equal
    z: "\u2265", // Greater than or equal
    "{": "\u03C0", // Pi
    "|": "\u2260", // Not equal
    "}": "\u00A3", // UK pound
    "~": "\u00B7", // Bullet
  };

  return translations[char] ?? char;
}
