/**
 * Plain-text extraction for copy operations.
 *
 * Pure function over a UnifiedBuffer; takes (start, end) coordinates,
 * normalizes them, and returns the rendered text with trailing spaces
 * trimmed per line.
 *
 * Extracted from TerminalState for separation of concerns.
 */

import type { UnifiedBuffer } from "./unified-buffer.ts";

/**
 * Extract plain text from a grid range for copy operations.
 *
 * Coordinates are automatically normalized (start comes before end).
 * Trailing spaces on each line are removed.
 * Lines are joined with '\n'.
 */
export function extractText(
  buffer: UnifiedBuffer,
  startCol: number,
  startRow: number,
  endCol: number,
  endRow: number,
): string {
  // Normalize coordinates (ensure start comes before end)
  if (startRow > endRow || (startRow === endRow && startCol > endCol)) {
    [startCol, startRow, endCol, endRow] = [
      endCol,
      endRow,
      startCol,
      startRow,
    ];
  }

  const lines: string[] = [];

  // Extract text row by row
  for (let row = startRow; row <= endRow; row++) {
    const line = buffer.getLine(row);
    const lineLength = line.length;

    let rowStartCol: number;
    let rowEndCol: number;

    if (row === startRow && row === endRow) {
      // Single line selection
      rowStartCol = startCol;
      rowEndCol = endCol;
    } else if (row === startRow) {
      // First line of multi-line selection
      rowStartCol = startCol;
      rowEndCol = lineLength - 1;
    } else if (row === endRow) {
      // Last line of multi-line selection
      rowStartCol = 0;
      rowEndCol = endCol;
    } else {
      // Middle line of multi-line selection
      rowStartCol = 0;
      rowEndCol = lineLength - 1;
    }

    // Extract characters from this row
    let rowText = "";
    for (let col = rowStartCol; col <= rowEndCol && col < lineLength; col++) {
      const cell = line.getCell(col);
      rowText += cell.char;
    }

    // Remove trailing spaces
    rowText = rowText.replace(/\s+$/, "");

    lines.push(rowText);
  }

  // Join lines with newline
  return lines.join("\n");
}
