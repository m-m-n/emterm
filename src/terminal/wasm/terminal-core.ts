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
	createDefaultAttributes,
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
	dirty = true;

	constructor(
		private readonly core: TerminalCore,
		private readonly row: number,
	) {}

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
		this.dirty = true;
	}

	clearDirty(): void {
		this.dirty = false;
	}
}

// ── WasmGrid ────────────────────────────────────────────

/**
 * WASM-backed viewport grid.
 * Wraps TerminalCore and provides Line-compatible access to WASM data.
 */
export class WasmGrid {
	readonly core: TerminalCore;

	constructor(cols: number, rows: number) {
		this.core = new TerminalCore(cols, rows);
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

	// ── Resize / Reset ───────────────────────────────

	resize(cols: number, rows: number): void {
		this.core.resize(cols, rows);
	}

	reset(): void {
		this.core.reset();
	}

	// ── Batch row read ───────────────────────────────

	getRowPacked(row: number): Uint8Array {
		return this.core.get_row_packed(row);
	}

	dispose(): void {
		this.core.free();
	}
}

// ── Conversion utilities ────────────────────────────────

/**
 * Read a WASM row and create a JS Line object (for scroll-out to scrollback).
 */
export function wasmRowToLine(core: TerminalCore, row: number): Line {
	const cols = core.cols();
	const line = new Line(cols);

	for (let col = 0; col < cols; col++) {
		const ch = core.get_cell_char(col, row);
		const width = core.get_cell_width(col, row);
		const fg = unpackColor(core.get_cell_fg(col, row));
		const bg = unpackColor(core.get_cell_bg(col, row));
		const flags = core.get_cell_flags(col, row);
		const attrs: CellAttributes = { ...unpackStyleFlags(flags), fg, bg };
		line.setCell(col, { char: ch, width, attrs, dirty: false });
	}

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
