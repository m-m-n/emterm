/**
 * WASM-backed terminal grid adapter.
 *
 * Wraps the Rust TerminalCore exported via wasm_bindgen, providing
 * Line/Cell-compatible interfaces for the existing TS codebase.
 */

import { TerminalCore } from "../../../wasm/pkg/emterm_wasm.js";
import type { Cell, LineAccessor } from "../grid.ts";
import { Line } from "../grid.ts";
import {
	type CellAttributes,
	type Color,
	packColor,
	packStyleFlags,
	unpackColor,
	unpackStyleFlags,
} from "../attributes.ts";

// ── WasmCellProxy ───────────────────────────────────────

/**
 * Read-through proxy for a single WASM cell.
 * Implements the Cell interface by reading from TerminalCore on access.
 */
export class WasmCellProxy implements Cell {
	char: string;
	width: number;
	attrs: CellAttributes;
	dirty = false; // Row-level dirty only in WASM

	constructor(core: TerminalCore, col: number, row: number) {
		this.char = core.get_cell_char(col, row);
		this.width = core.get_cell_width(col, row);
		const fg = unpackColor(core.get_cell_fg(col, row));
		const bg = unpackColor(core.get_cell_bg(col, row));
		const flags = core.get_cell_flags(col, row);
		this.attrs = { ...unpackStyleFlags(flags), fg, bg };
	}
}

// ── WasmLineProxy ───────────────────────────────────────

/**
 * Read-through proxy for a WASM grid row.
 * Provides Line-compatible interface backed by TerminalCore.
 */
export class WasmLineProxy implements LineAccessor {
	constructor(
		private readonly core: TerminalCore,
		private readonly row: number,
	) {}

	get dirty(): boolean {
		return this.core.is_row_dirty(this.row);
	}

	set dirty(_: boolean) {
		// No-op: dirty is managed by WASM core
	}

	get length(): number {
		return this.core.cols();
	}

	get wrapped(): boolean {
		return this.core.get_line_wrapped(this.row);
	}

	set wrapped(v: boolean) {
		this.core.set_line_wrapped(this.row, v);
	}

	getCell(index: number): Cell {
		return new WasmCellProxy(this.core, index, this.row);
	}

	setCell(index: number, cell: Cell): void {
		const fg = packColor(cell.attrs.fg);
		const bg = packColor(cell.attrs.bg);
		const flags = packStyleFlags(cell.attrs);
		this.core.set_cell(
			index,
			this.row,
			cell.char,
			cell.width,
			fg.tag,
			fg.r,
			fg.g,
			fg.b,
			bg.tag,
			bg.r,
			bg.g,
			bg.b,
			flags,
		);
	}

	clear(): void {
		this.core.clear_line(this.row);
	}

	clearRange(start: number, end: number): void {
		this.core.clear_line_range(this.row, start, end);
	}

	isEmpty(): boolean {
		return this.core.is_line_empty(this.row);
	}

	getText(): string {
		return this.core.get_line_text(this.row);
	}

	/**
	 * Materialize WASM row data into a JS Cell array (for reflow).
	 */
	toCells(): Cell[] {
		const cols = this.core.cols();
		const cells: Cell[] = [];
		for (let i = 0; i < cols; i++) {
			const proxy = new WasmCellProxy(this.core, i, this.row);
			cells.push({
				char: proxy.char,
				width: proxy.width,
				attrs: proxy.attrs,
				dirty: false,
			});
		}
		return cells;
	}

	getCells(): Cell[] {
		return this.toCells();
	}

	setCells(cells: Cell[]): void {
		for (let i = 0; i < cells.length && i < this.length; i++) {
			this.setCell(i, cells[i]!);
		}
	}

	markDirty(): void {
		this.core.mark_row_dirty(this.row);
	}

	clearDirty(): void {
		// No-op: dirty cleared at renderer level via core.clear_dirty()
	}
}

// ── WasmGrid ────────────────────────────────────────────

/**
 * WASM-backed viewport grid.
 * Wraps TerminalCore and provides Line-compatible access to WASM data.
 */
export class WasmGrid {
	readonly core: TerminalCore;

	constructor(cols: number, rows: number, scrollbackLines: number = 0) {
		this.core = new TerminalCore(cols, rows, scrollbackLines);
	}

	/**
	 * Create a WasmGrid wrapping an existing TerminalCore.
	 * Used for restoring from snapshots where the core is already created.
	 */
	static fromCore(core: TerminalCore): WasmGrid {
		const grid = Object.create(WasmGrid.prototype) as WasmGrid;
		(grid as { core: TerminalCore }).core = core;
		return grid;
	}

	get cols(): number {
		return this.core.cols();
	}

	get rows(): number {
		return this.core.rows();
	}

	// ── Cell access ──────────────────────────────────

	setCell(col: number, row: number, cell: Cell): void {
		const fg = packColor(cell.attrs.fg);
		const bg = packColor(cell.attrs.bg);
		const flags = packStyleFlags(cell.attrs);
		this.core.set_cell(
			col,
			row,
			cell.char,
			cell.width,
			fg.tag,
			fg.r,
			fg.g,
			fg.b,
			bg.tag,
			bg.r,
			bg.g,
			bg.b,
			flags,
		);
	}

	setCellAscii(
		col: number,
		row: number,
		byte: number,
		attrs: CellAttributes,
	): void {
		const fg = packColor(attrs.fg);
		const bg = packColor(attrs.bg);
		const flags = packStyleFlags(attrs);
		this.core.set_cell_ascii(
			col,
			row,
			byte,
			fg.tag,
			fg.r,
			fg.g,
			fg.b,
			bg.tag,
			bg.r,
			bg.g,
			bg.b,
			flags,
		);
	}

	// ── Line access ──────────────────────────────────

	getLine(row: number): WasmLineProxy {
		return new WasmLineProxy(this.core, row);
	}

	clearLine(row: number): void {
		this.core.clear_line(row);
	}

	clearLineRange(row: number, start: number, end: number): void {
		this.core.clear_line_range(row, start, end);
	}

	// ── Row operations ───────────────────────────────

	shiftRowsUp(startRow: number, endRow: number, count: number): void {
		this.core.shift_rows_up(startRow, endRow, count);
	}

	shiftRowsDown(startRow: number, endRow: number, count: number): void {
		this.core.shift_rows_down(startRow, endRow, count);
	}

	copyRow(src: number, dst: number): void {
		this.core.copy_row(src, dst);
	}

	fillRowDefault(row: number): void {
		this.core.fill_row_default(row);
	}

	/**
	 * Write JS Cell array into a WASM row (for reflow write-back).
	 */
	setRowFromCells(row: number, cells: Cell[]): void {
		const cols = this.core.cols();
		for (let i = 0; i < cells.length && i < cols; i++) {
			this.setCell(i, row, cells[i]!);
		}
		// Clear remaining columns if cells is shorter
		if (cells.length < cols) {
			this.core.clear_line_range(row, cells.length, cols);
		}
	}

	// ── Dirty tracking ───────────────────────────────

	getDirtyRows(): Uint16Array {
		return this.core.get_dirty_rows();
	}

	isRowDirty(row: number): boolean {
		return this.core.is_row_dirty(row);
	}

	markRowDirty(row: number): void {
		this.core.mark_row_dirty(row);
	}

	markAllDirty(): void {
		this.core.mark_all_dirty();
	}

	clearDirty(): void {
		this.core.clear_dirty();
	}

	// ── Scroll Event ────────────────────────────────

	/** Returns scroll direction: 1=Up, 0=none. */
	getScrollEventDirection(): number {
		return this.core.get_scroll_event_direction();
	}

	/** Returns scroll count (0 if no event). */
	getScrollEventCount(): number {
		return this.core.get_scroll_event_count();
	}

	/** Clears the pending scroll event. */
	clearScrollEvent(): void {
		this.core.clear_scroll_event();
	}

	// ── Resize / Reset ───────────────────────────────

	resize(cols: number, rows: number): void {
		this.core.resize(cols, rows);
	}

	/**
	 * Resize with full reflow (for primary buffer).
	 * Returns packed cursor: { col: high 16 bits, row: low 16 bits }.
	 */
	resizeReflow(
		cols: number,
		rows: number,
		scrollbackLines: number,
	): { col: number; row: number } {
		const packed = this.core.resize_reflow(cols, rows, scrollbackLines);
		return {
			col: (packed >>> 16) & 0xffff,
			row: packed & 0xffff,
		};
	}

	/**
	 * Simple resize without reflow (for alternate buffer).
	 */
	resizeNoReflow(cols: number, rows: number): void {
		this.core.resize_no_reflow(cols, rows);
	}

	reset(): void {
		this.core.reset();
	}

	// ── Batch row read ───────────────────────────────

	getRowPacked(row: number): Uint8Array {
		return this.core.get_row_packed(row);
	}

	// ── Scrollback access ────────────────────────────

	getScrollbackLength(): number {
		return this.core.get_scrollback_length();
	}

	clearScrollback(): void {
		this.core.clear_scrollback();
	}

	getScrollbackRowPacked(index: number): Uint8Array {
		return this.core.get_scrollback_row_packed(index);
	}

	getScrollbackLineWrapped(index: number): boolean {
		return this.core.get_scrollback_line_wrapped(index);
	}

	dispose(): void {
		this.core.free();
	}
}

// ── Conversion utilities ────────────────────────────────

/** Shared TextDecoder for UTF-8 parsing in wasmRowToLine. */
const utf8Decoder = new TextDecoder("utf-8");

/**
 * Parse packed binary row data into a JS Line object.
 *
 * Binary format per cell:
 *   Inline: char_len(1) + char_data(char_len) + width(1) + fg(4) + bg(4) + flags(2 LE) + hyperlink_id(2 LE)
 *   Overflow: 0xFF(1) + len_hi(1) + len_lo(1) + utf8_data(len) + width(1) + fg(4) + bg(4) + flags(2 LE) + hyperlink_id(2 LE)
 */
export function parsePackedRow(packed: Uint8Array, cols: number): Line {
	const line = new Line(cols);
	let offset = 0;

	for (let col = 0; col < cols; col++) {
		// Safety: ensure minimum bytes remain (1 charLen + 1 width + 8 colors + 2 flags + 2 hyperlink_id = 14)
		if (offset + 14 > packed.length) break;

		// Read character data
		const charLen = packed[offset++]!;
		let ch: string;
		if (charLen === 0xFF) {
			// Overflow: 2-byte big-endian length + UTF-8 data
			const lenHi = packed[offset++]!;
			const lenLo = packed[offset++]!;
			const byteLen = (lenHi << 8) | lenLo;
			ch = utf8Decoder.decode(packed.subarray(offset, offset + byteLen));
			offset += byteLen;
		} else if (charLen === 0) {
			ch = "";
		} else if (charLen === 1) {
			// Fast path for ASCII (most common case)
			ch = String.fromCharCode(packed[offset++]!);
		} else {
			ch = utf8Decoder.decode(packed.subarray(offset, offset + charLen));
			offset += charLen;
		}

		// Read width (1 byte)
		const width = packed[offset++]!;

		// Read fg color (4 bytes: tag, r, g, b)
		const fgTag = packed[offset++]!;
		const fgR = packed[offset++]!;
		const fgG = packed[offset++]!;
		const fgB = packed[offset++]!;
		let fg: Color | null;
		if (fgTag === 0) {
			fg = null;
		} else if (fgTag === 1) {
			fg = { type: "indexed", index: fgR };
		} else {
			fg = { type: "rgb", r: fgR, g: fgG, b: fgB };
		}

		// Read bg color (4 bytes: tag, r, g, b)
		const bgTag = packed[offset++]!;
		const bgR = packed[offset++]!;
		const bgG = packed[offset++]!;
		const bgB = packed[offset++]!;
		let bg: Color | null;
		if (bgTag === 0) {
			bg = null;
		} else if (bgTag === 1) {
			bg = { type: "indexed", index: bgR };
		} else {
			bg = { type: "rgb", r: bgR, g: bgG, b: bgB };
		}

		// Read flags (2 bytes, little-endian)
		const flagsLo = packed[offset++]!;
		const flagsHi = packed[offset++]!;
		const flags = flagsLo | (flagsHi << 8);

		// Read hyperlink_id (2 bytes, little-endian)
		const hlLo = packed[offset++]!;
		const hlHi = packed[offset++]!;
		const hyperlinkId = hlLo | (hlHi << 8);

		const attrs: CellAttributes = { ...unpackStyleFlags(flags), fg, bg, hyperlinkId };
		line.setCell(col, { char: ch, width, attrs, dirty: false });
	}

	return line;
}

/**
 * Read a WASM viewport row and create a JS Line object.
 *
 * Uses get_row_packed() for a single WASM call per row instead of cols*5
 * individual calls.
 */
export function wasmRowToLine(core: TerminalCore, row: number): Line {
	const packed = core.get_row_packed(row);
	const line = parsePackedRow(packed, core.cols());
	line.wrapped = core.get_line_wrapped(row);
	return line;
}

/**
 * Write a JS Line into a WASM row (for restoring from scrollback).
 */
export function lineToWasmRow(
	core: TerminalCore,
	row: number,
	line: Line,
): void {
	const cols = core.cols();
	const lineCols = line.length;
	const limit = Math.min(cols, lineCols);

	for (let col = 0; col < limit; col++) {
		const cell = line.getCell(col);
		const fg = packColor(cell.attrs.fg);
		const bg = packColor(cell.attrs.bg);
		const flags = packStyleFlags(cell.attrs);
		core.set_cell(
			col,
			row,
			cell.char,
			cell.width,
			fg.tag,
			fg.r,
			fg.g,
			fg.b,
			bg.tag,
			bg.r,
			bg.g,
			bg.b,
			flags,
		);
	}

	// Clear remaining columns
	if (limit < cols) {
		core.clear_line_range(row, limit, cols);
	}

	core.set_line_wrapped(row, line.wrapped);
}
