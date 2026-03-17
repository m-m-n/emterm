/**
 * Scroll operations extracted from UnifiedBuffer.
 *
 * Standalone functions that receive buffer state as parameters,
 * enabling UnifiedBuffer to delegate scroll-related work here.
 */
import type { LineAccessor } from "./grid.ts";
import { Line } from "./grid.ts";
import type { WasmGrid } from "./wasm/terminal-core.ts";

/**
 * Scroll region definition.
 * Uses 0-indexed row numbers.
 */
export interface ScrollRegion {
	/** Top margin (inclusive, 0-indexed). */
	top: number;
	/** Bottom margin (inclusive, 0-indexed). */
	bottom: number;
}

/**
 * Internal buffer accessors needed by scroll operations.
 * UnifiedBuffer implements this interface to expose its private state
 * in a controlled way.
 */
export interface BufferScrollAccess {
	readonly rows: number;
	readonly cols: number;
	readonly scrollbackLength: number;
	readonly wasmGrid: WasmGrid | null;
	scrollRegion: ScrollRegion | null;
	getLine(row: number): LineAccessor;
	getAbsolute(index: number): Line;
	setAbsolute(index: number, line: Line): void;
	push(line: Line): void;
}

// ===== Scroll Region Management =====

/**
 * Set the scroll region.
 *
 * @param buf - Buffer access
 * @param top - Top margin (0-indexed, inclusive)
 * @param bottom - Bottom margin (0-indexed, inclusive)
 */
export function setScrollRegion(buf: BufferScrollAccess, top: number, bottom: number): void {
	if (top < 0) top = 0;
	if (bottom >= buf.rows) bottom = buf.rows - 1;

	if (top === 0 && bottom === buf.rows - 1) {
		buf.scrollRegion = null;
	} else if (top < bottom) {
		buf.scrollRegion = { top, bottom };
	}

	// Sync to WASM scroll region (only valid regions)
	if (buf.wasmGrid && top < bottom) {
		buf.wasmGrid.core.set_scroll_region(top, bottom);
	}
}

/**
 * Clear the scroll region (reset to full screen).
 *
 * @param buf - Buffer access
 */
export function clearScrollRegion(buf: BufferScrollAccess): void {
	buf.scrollRegion = null;

	// Sync to WASM: full screen = (0, rows-1)
	if (buf.wasmGrid) {
		buf.wasmGrid.core.set_scroll_region(0, buf.rows - 1);
	}
}

/**
 * Get the current scroll region.
 *
 * @param buf - Buffer access
 * @returns Scroll region or null if full screen
 */
export function getScrollRegion(buf: BufferScrollAccess): ScrollRegion | null {
	return buf.scrollRegion;
}

/**
 * Get effective scroll region bounds.
 *
 * @param buf - Buffer access
 * @returns Effective scroll region (full screen if none set)
 */
export function getEffectiveScrollRegion(buf: BufferScrollAccess): ScrollRegion {
	return buf.scrollRegion ?? { top: 0, bottom: buf.rows - 1 };
}

// ===== Scroll Operations =====

/**
 * Scroll the buffer up by the specified number of lines.
 * Respects the scroll region if set.
 *
 * For full-screen scroll (top=0 AND bottom=rows-1): pushes blank lines
 * to the ring buffer, making old top lines become scrollback implicitly.
 *
 * For partial scroll region: rearranges lines in-place within the region.
 *
 * @param buf - Buffer access
 * @param count - Number of lines to scroll (default: 1)
 */
export function scrollUp(buf: BufferScrollAccess, count: number = 1): void {
	if (count <= 0) return;

	const { top, bottom } = getEffectiveScrollRegion(buf);
	const regionHeight = bottom - top + 1;
	const actualCount = Math.min(count, regionHeight);

	if (buf.wasmGrid) {
		// WASM Ring Buffer mode: scroll handled entirely within WASM
		buf.wasmGrid.core.handle_scroll_up(actualCount);
		return;
	}

	// JS mode: original implementation
	// Full-screen scroll: use ring buffer push (implicit scrollback)
	if (top === 0 && bottom === buf.rows - 1) {
		for (let i = 0; i < actualCount; i++) {
			buf.push(new Line(buf.cols));
		}
		// Mark all viewport lines as dirty
		for (let r = 0; r < buf.rows; r++) {
			buf.getLine(r).dirty = true;
		}
		return;
	}

	// Partial scroll region: rearrange viewport lines in-place
	const sbLen = buf.scrollbackLength;

	// Shift remaining lines up within region
	for (let i = top; i <= bottom - actualCount; i++) {
		buf.setAbsolute(sbLen + i, buf.getAbsolute(sbLen + i + actualCount));
	}

	// Insert blank lines at bottom of region
	for (let i = 0; i < actualCount; i++) {
		buf.setAbsolute(sbLen + bottom - actualCount + 1 + i, new Line(buf.cols));
	}

	// Mark affected lines as dirty
	for (let i = top; i <= bottom; i++) {
		buf.getLine(i).dirty = true;
	}
}

/**
 * Scroll the buffer down by the specified number of lines.
 * Respects the scroll region if set.
 * New empty lines are added at the top of the region.
 *
 * @param buf - Buffer access
 * @param count - Number of lines to scroll (default: 1)
 */
export function scrollDown(buf: BufferScrollAccess, count: number = 1): void {
	if (count <= 0) return;

	const { top, bottom } = getEffectiveScrollRegion(buf);
	const regionHeight = bottom - top + 1;
	const actualCount = Math.min(count, regionHeight);

	if (buf.wasmGrid) {
		// WASM Ring Buffer mode: scroll handled entirely within WASM
		buf.wasmGrid.core.handle_scroll_down(actualCount);
		return;
	}

	// JS mode: original implementation
	const sbLen = buf.scrollbackLength;

	// Shift lines down within region
	for (let i = bottom; i >= top + actualCount; i--) {
		buf.setAbsolute(sbLen + i, buf.getAbsolute(sbLen + i - actualCount));
	}

	// Insert blank lines at top of region
	for (let i = 0; i < actualCount; i++) {
		buf.setAbsolute(sbLen + top + i, new Line(buf.cols));
	}

	// Mark affected lines as dirty
	for (let i = top; i <= bottom; i++) {
		buf.getLine(i).dirty = true;
	}
}

// ===== Line Manipulation =====

/**
 * Insert blank lines at the specified row.
 * Lines below are pushed down within the scroll region.
 *
 * @param buf - Buffer access
 * @param row - Row to insert at
 * @param count - Number of lines to insert
 */
export function insertLines(buf: BufferScrollAccess, row: number, count: number = 1): void {
	if (count <= 0) return;

	const { top, bottom } = getEffectiveScrollRegion(buf);
	if (row < top || row > bottom) return;

	const actualCount = Math.min(count, bottom - row + 1);

	if (buf.wasmGrid) {
		// WASM mode: shift rows down from cursor, fill with defaults
		buf.wasmGrid.shiftRowsDown(row, bottom, actualCount);
		for (let i = 0; i < actualCount; i++) {
			buf.wasmGrid.fillRowDefault(row + i);
		}
		for (let i = row; i <= bottom; i++) {
			buf.wasmGrid.markRowDirty(i);
		}
		return;
	}

	// JS mode
	const sbLen = buf.scrollbackLength;

	// Shift lines down within region (from bottom up)
	for (let i = bottom; i >= row + actualCount; i--) {
		buf.setAbsolute(sbLen + i, buf.getAbsolute(sbLen + i - actualCount));
	}

	// Insert blank lines at cursor row
	for (let i = 0; i < actualCount; i++) {
		buf.setAbsolute(sbLen + row + i, new Line(buf.cols));
	}

	// Mark affected lines as dirty
	for (let i = row; i <= bottom; i++) {
		buf.getLine(i).dirty = true;
	}
}

/**
 * Delete lines at the specified row.
 * Lines below are pulled up within the scroll region.
 *
 * @param buf - Buffer access
 * @param row - Row to delete from
 * @param count - Number of lines to delete
 */
export function deleteLines(buf: BufferScrollAccess, row: number, count: number = 1): void {
	if (count <= 0) return;

	const { top, bottom } = getEffectiveScrollRegion(buf);
	if (row < top || row > bottom) return;

	const actualCount = Math.min(count, bottom - row + 1);

	if (buf.wasmGrid) {
		// WASM mode: shift rows up from cursor, fill bottom with defaults
		buf.wasmGrid.shiftRowsUp(row, bottom, actualCount);
		for (let i = 0; i < actualCount; i++) {
			buf.wasmGrid.fillRowDefault(bottom - actualCount + 1 + i);
		}
		for (let i = row; i <= bottom; i++) {
			buf.wasmGrid.markRowDirty(i);
		}
		return;
	}

	// JS mode
	const sbLen = buf.scrollbackLength;

	// Shift lines up within region
	for (let i = row; i <= bottom - actualCount; i++) {
		buf.setAbsolute(sbLen + i, buf.getAbsolute(sbLen + i + actualCount));
	}

	// Add blank lines at bottom of region
	for (let i = 0; i < actualCount; i++) {
		buf.setAbsolute(sbLen + bottom - actualCount + 1 + i, new Line(buf.cols));
	}

	// Mark affected lines as dirty
	for (let i = row; i <= bottom; i++) {
		buf.getLine(i).dirty = true;
	}
}
