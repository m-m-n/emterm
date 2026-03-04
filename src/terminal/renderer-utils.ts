/**
 * Utility functions for terminal rendering.
 *
 * Pure functions and types extracted from canvas-renderer.ts
 * for reuse and testability.
 */

import type { CellAttributes, Color } from "./attributes.ts";
import {
	attributesEqual,
	getEffectiveBackground,
	unpackStyleFlags,
} from "./attributes.ts";
import type { LineAccessor } from "./grid.ts";
import type { TerminalState } from "./state.ts";

/**
 * A span of text with uniform attributes.
 */
export interface TextSpan {
	text: string;
	attrs: CellAttributes;
	/** Starting column index of the span. */
	startCol: number;
	/** Number of cells this span occupies (for wide chars, this may differ from text length). */
	cellCount: number;
	/** Cell boundaries: array of [charString, cellWidth] for each cell in the span. */
	cells: Array<[string, number]>;
}

/**
 * Group cells in a line into spans with uniform attributes.
 *
 * Handles:
 * - Wide character placeholders (width=0): skipped
 * - Combining marks (width=0 with non-empty char): merged with previous span
 *
 * @param line - The line to process
 * @returns Array of text spans with their attributes
 */
export function groupCellsIntoSpans(line: LineAccessor): TextSpan[] {
	const spans: TextSpan[] = [];
	let currentText = "";
	let currentAttrs: CellAttributes | null = null;
	let currentStartCol = 0;
	let currentCellCount = 0;
	let currentCells: Array<[string, number]> = [];

	for (let i = 0; i < line.length; i++) {
		const cell = line.getCell(i);

		// Handle zero-width cells
		if (cell.width === 0) {
			// Wide character placeholder (empty char) - skip entirely
			if (cell.char === "" || cell.char === " ") {
				continue;
			}
			// Combining mark (has a character) - merge with last cell entry
			if (currentCells.length > 0) {
				const last = currentCells[currentCells.length - 1]!;
				last[0] += cell.char;
				currentText += cell.char;
			}
			continue;
		}

		if (currentAttrs === null) {
			// First cell
			currentAttrs = cell.attrs;
			currentText = cell.char;
			currentStartCol = i;
			currentCellCount = cell.width;
			currentCells = [[cell.char, cell.width]];
		} else if (attributesEqual(cell.attrs, currentAttrs)) {
			// Same attributes - extend current span
			currentText += cell.char;
			currentCellCount += cell.width;
			currentCells.push([cell.char, cell.width]);
		} else {
			// Different attributes - save current span and start new one
			spans.push({
				text: currentText,
				attrs: currentAttrs,
				startCol: currentStartCol,
				cellCount: currentCellCount,
				cells: currentCells,
			});
			currentAttrs = cell.attrs;
			currentText = cell.char;
			currentStartCol = i;
			currentCellCount = cell.width;
			currentCells = [[cell.char, cell.width]];
		}
	}

	// Don't forget the last span
	if (currentAttrs !== null) {
		spans.push({
			text: currentText,
			attrs: currentAttrs,
			startCol: currentStartCol,
			cellCount: currentCellCount,
			cells: currentCells,
		});
	}

	return spans;
}

const utf8Decoder = new TextDecoder("utf-8");

/**
 * Fast inline comparison of 10 packed attribute bytes.
 */
export function packedAttrsEqual(buf: Uint8Array, a: number, b: number): boolean {
	return buf[a] === buf[b] && buf[a + 1] === buf[b + 1] && buf[a + 2] === buf[b + 2] &&
		buf[a + 3] === buf[b + 3] && buf[a + 4] === buf[b + 4] && buf[a + 5] === buf[b + 5] &&
		buf[a + 6] === buf[b + 6] && buf[a + 7] === buf[b + 7] && buf[a + 8] === buf[b + 8] &&
		buf[a + 9] === buf[b + 9];
}

/**
 * Unpack CellAttributes from 10 binary bytes at the given offset.
 * Layout: fg(4: tag,r,g,b) + bg(4: tag,r,g,b) + flags(2: LE u16)
 */
export function unpackAttrsFromBinary(buf: Uint8Array, offset: number): CellAttributes {
	const fgTag = buf[offset]!;
	const fgR = buf[offset + 1]!;
	const fgG = buf[offset + 2]!;
	const fgB = buf[offset + 3]!;
	let fg: Color | null;
	if (fgTag === 0) fg = null;
	else if (fgTag === 1) fg = { type: "indexed", index: fgR };
	else fg = { type: "rgb", r: fgR, g: fgG, b: fgB };

	const bgTag = buf[offset + 4]!;
	const bgR = buf[offset + 5]!;
	const bgG = buf[offset + 6]!;
	const bgB = buf[offset + 7]!;
	let bg: Color | null;
	if (bgTag === 0) bg = null;
	else if (bgTag === 1) bg = { type: "indexed", index: bgR };
	else bg = { type: "rgb", r: bgR, g: bgG, b: bgB };

	const flagsLo = buf[offset + 8]!;
	const flagsHi = buf[offset + 9]!;
	const flags = flagsLo | (flagsHi << 8);

	return { ...unpackStyleFlags(flags), fg, bg };
}

/**
 * Parse packed binary row data directly into TextSpan array.
 * Avoids creating Cell, CellAttributes, or Line objects except for span attributes.
 *
 * Binary format per cell:
 *   Inline: char_len(1) + char_data(char_len) + width(1) + fg(4) + bg(4) + flags(2 LE)
 *   Overflow: 0xFF(1) + len_hi(1) + len_lo(1) + utf8_data(len) + width(1) + fg(4) + bg(4) + flags(2 LE)
 */
export function groupPackedCellsIntoSpans(packed: Uint8Array, cols: number): TextSpan[] {
	const spans: TextSpan[] = [];
	let offset = 0;

	let currentText = "";
	let currentStartCol = 0;
	let currentCellCount = 0;
	let currentCells: Array<[string, number]> = [];
	let prevAttrOffset = -1;
	let currentAttrs: CellAttributes | null = null;

	for (let col = 0; col < cols; col++) {
		if (offset + 12 > packed.length) break;

		// Parse character
		const charLen = packed[offset++]!;
		let ch: string;
		if (charLen === 0xFF) {
			if (offset + 2 > packed.length) break;
			const lenHi = packed[offset++]!;
			const lenLo = packed[offset++]!;
			const byteLen = (lenHi << 8) | lenLo;
			if (offset + byteLen + 11 > packed.length) break; // byteLen + width(1) + attrs(10)
			ch = utf8Decoder.decode(packed.subarray(offset, offset + byteLen));
			offset += byteLen;
		} else if (charLen === 0) {
			ch = "";
		} else if (charLen === 1) {
			ch = String.fromCharCode(packed[offset++]!);
		} else {
			ch = utf8Decoder.decode(packed.subarray(offset, offset + charLen));
			offset += charLen;
		}

		// Read width
		const width = packed[offset++]!;

		// Attribute bytes start here (10 bytes)
		const attrStart = offset;
		offset += 10;

		// Handle zero-width cells
		if (width === 0) {
			if (ch === "" || ch === " ") continue; // wide char placeholder
			// Combining mark - merge with previous cell
			if (currentCells.length > 0) {
				const last = currentCells[currentCells.length - 1]!;
				last[0] += ch;
				currentText += ch;
			}
			continue;
		}

		// Fast attribute comparison: compare 10 bytes inline
		const attrsMatch = prevAttrOffset >= 0 &&
			packedAttrsEqual(packed, prevAttrOffset, attrStart);

		if (currentAttrs === null || !attrsMatch) {
			// Save previous span
			if (currentAttrs !== null) {
				spans.push({
					text: currentText,
					attrs: currentAttrs,
					startCol: currentStartCol,
					cellCount: currentCellCount,
					cells: currentCells,
				});
			}
			// Start new span
			currentAttrs = unpackAttrsFromBinary(packed, attrStart);
			currentText = ch;
			currentStartCol = col;
			currentCellCount = width;
			currentCells = [[ch, width]];
		} else {
			// Extend current span
			currentText += ch;
			currentCellCount += width;
			currentCells.push([ch, width]);
		}

		prevAttrOffset = attrStart;
	}

	// Final span
	if (currentAttrs !== null && currentText.length > 0) {
		spans.push({
			text: currentText,
			attrs: currentAttrs,
			startCol: currentStartCol,
			cellCount: currentCellCount,
			cells: currentCells,
		});
	}

	return spans;
}

/**
 * Get visible lines based on scroll offset.
 *
 * @param state - Terminal state
 * @param scrollOffset - Number of lines scrolled back (0 = current view)
 * @returns Array of lines to render
 */
export function getVisibleLines(state: TerminalState, scrollOffset: number): LineAccessor[] {
	const buffer = state.getActiveBuffer();
	const visibleRows = state.rows;

	// If not scrolled (at bottom), return current screen buffer
	if (scrollOffset === 0) {
		const linesToRender: LineAccessor[] = [];
		for (let screenRow = 0; screenRow < visibleRows; screenRow++) {
			linesToRender.push(buffer.getLine(screenRow));
		}
		return linesToRender;
	}

	// When scrolled back, use index-based access (O(visibleRows), not O(scrollbackLength))
	const scrollbackLength = state.getScrollbackLength();
	// Clamp startIndex to 0 in case scrollOffset > scrollbackLength (stale offset after clear)
	const startIndex = Math.max(0, scrollbackLength - scrollOffset);

	const linesToRender: LineAccessor[] = [];
	for (let i = 0; i < visibleRows; i++) {
		const lineIndex = startIndex + i;
		if (lineIndex < scrollbackLength) {
			linesToRender.push(state.getScrollbackLine(lineIndex));
		} else {
			linesToRender.push(buffer.getLine(lineIndex - scrollbackLength));
		}
	}

	return linesToRender;
}

/**
 * Calculate the starting index for rendering based on scroll position.
 *
 * @param scrollOffset - Number of lines scrolled back
 * @param scrollbackLength - Total number of lines in scrollback
 * @returns Starting index in the combined buffer
 */
export function calculateScrollPosition(scrollOffset: number, scrollbackLength: number): number {
	return scrollbackLength - scrollOffset;
}

/**
 * Text attribute styles for rendering.
 */
export interface TextAttributeStyles {
	/** Global alpha for dim effect (0.5 for dim, 1 otherwise). */
	globalAlpha: number;
	/** Whether text should be hidden. */
	hidden: boolean;
	/** Whether to draw underline. */
	underline: boolean;
	/** Whether to draw strikethrough. */
	strikethrough: boolean;
	/** Whether text should blink. */
	blink: boolean;
}

/**
 * Build a CSS font string from cell attributes.
 *
 * @param attrs - Cell attributes
 * @param fontSize - Font size in pixels
 * @param fontFamily - Font family name
 * @returns CSS font string (e.g., "italic bold 13px monospace")
 */
export function buildFontString(
	attrs: CellAttributes,
	fontSize: number,
	fontFamily: string,
): string {
	const parts: string[] = [];

	if (attrs.italic) {
		parts.push("italic");
	}
	if (attrs.bold) {
		parts.push("bold");
	}
	parts.push(`${fontSize}px`);
	parts.push(fontFamily);

	return parts.join(" ");
}

/**
 * Apply text attributes and return style information.
 *
 * @param attrs - Cell attributes
 * @returns Style information for rendering
 */
export function applyTextAttributes(attrs: CellAttributes): TextAttributeStyles {
	return {
		globalAlpha: attrs.dim ? 0.5 : 1,
		hidden: attrs.hidden,
		underline: attrs.underline,
		strikethrough: attrs.strikethrough,
		blink: attrs.blink,
	};
}

/**
 * Selection range with start and end positions.
 */
export interface SelectionRange {
	start: { col: number; row: number };
	end: { col: number; row: number };
}

/**
 * Normalize selection range so that start comes before end.
 *
 * @param selection - Selection range to normalize
 * @returns Normalized selection range
 */
export function normalizeSelection(selection: SelectionRange): SelectionRange {
	const { start, end } = selection;

	// If start is before or equal to end, return as-is
	if (
		start.row < end.row ||
		(start.row === end.row && start.col <= end.col)
	) {
		return { start, end };
	}

	// Swap start and end
	return { start: end, end: start };
}
