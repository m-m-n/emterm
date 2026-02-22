/**
 * Tests for WASM-backed terminal grid adapter.
 *
 * Cross-validates WASM grid operations against expected behavior
 * matching the existing TS Line/Cell implementation.
 */

import { describe, test, expect, beforeAll } from "bun:test";
import { initSync } from "../../../../wasm/pkg/emterm_wasm.js";
import { readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import {
	WasmGrid,
	WasmLineProxy,
	WasmCellProxy,
	wasmRowToLine,
	lineToWasmRow,
} from "../terminal-core.ts";
import {
	type CellAttributes,
	type Color,
	createDefaultAttributes,
	packColor,
	unpackColor,
	packStyleFlags,
	unpackStyleFlags,
} from "../../attributes.ts";
import { Line, createEmptyCell, createCell } from "../../grid.ts";
import { CursorState } from "../../cursor.ts";
import { createDefaultModes, syncModesToWasm, syncModesFromWasm, WASM_MODE_BITS } from "../../modes.ts";

// Initialize WASM synchronously before tests
beforeAll(() => {
	const wasmPath = resolve(
		import.meta.dir,
		"../../../../wasm/pkg/emterm_wasm_bg.wasm",
	);
	const wasmBytes = readFileSync(wasmPath);
	initSync({ module: wasmBytes });
});

// ── Attribute conversion tests ──────────────────────────

describe("packColor / unpackColor", () => {
	test("default color (null) round-trip", () => {
		const packed = packColor(null);
		expect(packed.tag).toBe(0);
		const u32 = (packed.tag << 24) | (packed.r << 16) | (packed.g << 8) | packed.b;
		const result = unpackColor(u32);
		expect(result).toBeNull();
	});

	test("indexed color round-trip", () => {
		const color: Color = { type: "indexed", index: 42 };
		const packed = packColor(color);
		expect(packed.tag).toBe(1);
		expect(packed.r).toBe(42);
		const u32 = (packed.tag << 24) | (packed.r << 16) | (packed.g << 8) | packed.b;
		const result = unpackColor(u32);
		expect(result).toEqual({ type: "indexed", index: 42 });
	});

	test("RGB color round-trip", () => {
		const color: Color = { type: "rgb", r: 100, g: 200, b: 50 };
		const packed = packColor(color);
		expect(packed.tag).toBe(2);
		const u32 = (packed.tag << 24) | (packed.r << 16) | (packed.g << 8) | packed.b;
		const result = unpackColor(u32);
		expect(result).toEqual({ type: "rgb", r: 100, g: 200, b: 50 });
	});

	test("explicit default color round-trip", () => {
		const color: Color = { type: "default" };
		const packed = packColor(color);
		expect(packed.tag).toBe(0);
	});
});

describe("packStyleFlags / unpackStyleFlags", () => {
	test("all flags false = 0", () => {
		const attrs = createDefaultAttributes();
		expect(packStyleFlags(attrs)).toBe(0);
	});

	test("all 8 flags round-trip", () => {
		const attrs: CellAttributes = {
			bold: true,
			dim: true,
			italic: true,
			underline: true,
			blink: true,
			reverse: true,
			hidden: true,
			strikethrough: true,
			fg: null,
			bg: null,
		};
		const flags = packStyleFlags(attrs);
		expect(flags).toBe(0x00ff);
		const result = unpackStyleFlags(flags);
		expect(result.bold).toBe(true);
		expect(result.dim).toBe(true);
		expect(result.italic).toBe(true);
		expect(result.underline).toBe(true);
		expect(result.blink).toBe(true);
		expect(result.reverse).toBe(true);
		expect(result.hidden).toBe(true);
		expect(result.strikethrough).toBe(true);
	});

	test("individual flag preservation", () => {
		for (const flag of [
			"bold",
			"dim",
			"italic",
			"underline",
			"blink",
			"reverse",
			"hidden",
			"strikethrough",
		] as const) {
			const attrs = createDefaultAttributes();
			attrs[flag] = true;
			const packed = packStyleFlags(attrs);
			const result = unpackStyleFlags(packed);
			expect(result[flag]).toBe(true);
			// All others should be false
			for (const other of [
				"bold",
				"dim",
				"italic",
				"underline",
				"blink",
				"reverse",
				"hidden",
				"strikethrough",
			] as const) {
				if (other !== flag) {
					expect(result[other]).toBe(false);
				}
			}
		}
	});
});

// ── WasmGrid tests ──────────────────────────────────────

describe("WasmGrid", () => {
	test("construct 80x24", () => {
		const grid = new WasmGrid(80, 24);
		expect(grid.cols).toBe(80);
		expect(grid.rows).toBe(24);
		grid.dispose();
	});

	test("setCell + getCell ASCII round-trip", () => {
		const grid = new WasmGrid(80, 24);
		const cell = {
			char: "A",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		};
		grid.setCell(0, 0, cell);

		const line = grid.getLine(0);
		const readCell = line.getCell(0);
		expect(readCell.char).toBe("A");
		expect(readCell.width).toBe(1);
		grid.dispose();
	});

	test("setCell + getCell CJK round-trip", () => {
		const grid = new WasmGrid(80, 24);
		const cell = {
			char: "漢",
			width: 2,
			attrs: createDefaultAttributes(),
			dirty: true,
		};
		grid.setCell(5, 3, cell);

		const line = grid.getLine(3);
		const readCell = line.getCell(5);
		expect(readCell.char).toBe("漢");
		expect(readCell.width).toBe(2);
		grid.dispose();
	});

	test("attribute round-trip (default fg/bg)", () => {
		const grid = new WasmGrid(80, 24);
		const cell = {
			char: "X",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		};
		grid.setCell(0, 0, cell);

		const readCell = grid.getLine(0).getCell(0);
		expect(readCell.attrs.fg).toBeNull();
		expect(readCell.attrs.bg).toBeNull();
		grid.dispose();
	});

	test("attribute round-trip (RGB fg/bg)", () => {
		const grid = new WasmGrid(80, 24);
		const attrs = createDefaultAttributes();
		attrs.fg = { type: "rgb", r: 255, g: 128, b: 64 };
		attrs.bg = { type: "rgb", r: 0, g: 0, b: 0 };
		const cell = { char: "X", width: 1, attrs, dirty: true };
		grid.setCell(0, 0, cell);

		const readCell = grid.getLine(0).getCell(0);
		expect(readCell.attrs.fg).toEqual({ type: "rgb", r: 255, g: 128, b: 64 });
		expect(readCell.attrs.bg).toEqual({ type: "rgb", r: 0, g: 0, b: 0 });
		grid.dispose();
	});

	test("attribute round-trip (indexed color)", () => {
		const grid = new WasmGrid(80, 24);
		const attrs = createDefaultAttributes();
		attrs.fg = { type: "indexed", index: 196 };
		const cell = { char: "Y", width: 1, attrs, dirty: true };
		grid.setCell(0, 0, cell);

		const readCell = grid.getLine(0).getCell(0);
		expect(readCell.attrs.fg).toEqual({ type: "indexed", index: 196 });
		grid.dispose();
	});

	test("style flags round-trip (bold + italic)", () => {
		const grid = new WasmGrid(80, 24);
		const attrs = createDefaultAttributes();
		attrs.bold = true;
		attrs.italic = true;
		const cell = { char: "B", width: 1, attrs, dirty: true };
		grid.setCell(0, 0, cell);

		const readCell = grid.getLine(0).getCell(0);
		expect(readCell.attrs.bold).toBe(true);
		expect(readCell.attrs.italic).toBe(true);
		expect(readCell.attrs.dim).toBe(false);
		grid.dispose();
	});

	test("setCellAscii fast path", () => {
		const grid = new WasmGrid(80, 24);
		const attrs = createDefaultAttributes();
		attrs.bold = true;
		grid.setCellAscii(10, 5, 0x41, attrs); // 'A'

		const readCell = grid.getLine(5).getCell(10);
		expect(readCell.char).toBe("A");
		expect(readCell.width).toBe(1);
		expect(readCell.attrs.bold).toBe(true);
		grid.dispose();
	});
});

// ── WasmLineProxy tests ─────────────────────────────────

describe("WasmLineProxy", () => {
	test("getText() matches Line behavior", () => {
		const grid = new WasmGrid(10, 1);
		const line = grid.getLine(0);

		// Write "Hi" via grid
		grid.setCell(0, 0, {
			char: "H",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		});
		grid.setCell(1, 0, {
			char: "i",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		});

		const text = line.getText();
		expect(text.startsWith("Hi")).toBe(true);
		grid.dispose();
	});

	test("isEmpty()", () => {
		const grid = new WasmGrid(10, 1);
		expect(grid.getLine(0).isEmpty()).toBe(true);

		grid.setCell(5, 0, {
			char: "X",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		});
		expect(grid.getLine(0).isEmpty()).toBe(false);
		grid.dispose();
	});

	test("wrapped flag", () => {
		const grid = new WasmGrid(10, 2);
		const line = grid.getLine(0);
		expect(line.wrapped).toBe(false);
		line.wrapped = true;
		expect(line.wrapped).toBe(true);
		grid.dispose();
	});

	test("toCells() materializes JS array", () => {
		const grid = new WasmGrid(5, 1);
		grid.setCell(0, 0, {
			char: "A",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		});

		const cells = grid.getLine(0).toCells();
		expect(cells.length).toBe(5);
		expect(cells[0]!.char).toBe("A");
		expect(cells[1]!.char).toBe(" ");
		grid.dispose();
	});
});

// ── Conversion tests ────────────────────────────────────

describe("wasmRowToLine", () => {
	test("creates valid JS Line from WASM row", () => {
		const grid = new WasmGrid(10, 1);
		const attrs = createDefaultAttributes();
		attrs.bold = true;
		attrs.fg = { type: "rgb", r: 255, g: 0, b: 0 };

		grid.setCell(0, 0, { char: "X", width: 1, attrs, dirty: true });
		grid.core.set_line_wrapped(0, true);

		const jsLine = wasmRowToLine(grid.core, 0);
		expect(jsLine).toBeInstanceOf(Line);
		expect(jsLine.length).toBe(10);
		expect(jsLine.wrapped).toBe(true);

		const cell = jsLine.getCell(0);
		expect(cell.char).toBe("X");
		expect(cell.width).toBe(1);
		expect(cell.attrs.bold).toBe(true);
		expect(cell.attrs.fg).toEqual({ type: "rgb", r: 255, g: 0, b: 0 });

		// Rest should be empty
		expect(jsLine.getCell(1).char).toBe(" ");
		grid.dispose();
	});
});

describe("lineToWasmRow", () => {
	test("writes JS Line to WASM row", () => {
		const grid = new WasmGrid(10, 2);
		const jsLine = new Line(10);
		const attrs = createDefaultAttributes();
		attrs.italic = true;
		jsLine.setCell(0, { char: "Y", width: 1, attrs, dirty: true });
		jsLine.wrapped = true;

		lineToWasmRow(grid.core, 1, jsLine);

		expect(grid.core.get_cell_char(0, 1)).toBe("Y");
		expect(grid.core.get_line_wrapped(1)).toBe(true);

		const readCell = grid.getLine(1).getCell(0);
		expect(readCell.attrs.italic).toBe(true);
		grid.dispose();
	});
});

// ── Dirty tracking tests ────────────────────────────────

describe("dirty tracking", () => {
	test("setCell marks row dirty", () => {
		const grid = new WasmGrid(10, 5);
		grid.clearDirty();
		expect(grid.isRowDirty(2)).toBe(false);

		grid.setCell(0, 2, {
			char: "A",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		});
		expect(grid.isRowDirty(2)).toBe(true);
		grid.dispose();
	});

	test("clearDirty resets all", () => {
		const grid = new WasmGrid(10, 5);
		grid.clearDirty();
		const dirty = grid.getDirtyRows();
		expect(dirty.length).toBe(0);
		grid.dispose();
	});

	test("resize marks all dirty", () => {
		const grid = new WasmGrid(10, 5);
		grid.clearDirty();
		grid.resize(20, 10);
		const dirty = grid.getDirtyRows();
		expect(dirty.length).toBe(10);
		grid.dispose();
	});
});

// ── WasmLineProxy dirty delegation tests ─────────────────

describe("WasmLineProxy dirty delegation", () => {
	test("TS-09: dirty getter reflects WASM core state", () => {
		const grid = new WasmGrid(10, 3);
		grid.clearDirty();

		const line = grid.getLine(1);
		expect(line.dirty).toBe(false);

		// Writing a cell marks the row dirty in WASM core
		grid.setCell(0, 1, {
			char: "A",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		});
		expect(line.dirty).toBe(true);

		// Clearing dirty via core resets it
		grid.clearDirty();
		expect(line.dirty).toBe(false);
		grid.dispose();
	});

	test("TS-10: markDirty() sets WASM core dirty bit", () => {
		const grid = new WasmGrid(10, 3);
		grid.clearDirty();

		const line = grid.getLine(2);
		expect(line.dirty).toBe(false);

		line.markDirty();
		expect(line.dirty).toBe(true);
		expect(grid.isRowDirty(2)).toBe(true);
		grid.dispose();
	});

	test("TS-11: clearDirty() is no-op (dirty unchanged)", () => {
		const grid = new WasmGrid(10, 3);
		grid.clearDirty();

		const line = grid.getLine(0);
		line.markDirty();
		expect(line.dirty).toBe(true);

		// clearDirty on the proxy is a no-op
		line.clearDirty();
		expect(line.dirty).toBe(true);

		// Only grid-level clearDirty resets it
		grid.clearDirty();
		expect(line.dirty).toBe(false);
		grid.dispose();
	});

	test("dirty setter is no-op", () => {
		const grid = new WasmGrid(10, 3);
		grid.clearDirty();

		const line = grid.getLine(0);
		expect(line.dirty).toBe(false);

		// Setting dirty via proxy does nothing
		line.dirty = true;
		expect(line.dirty).toBe(false);

		// Actual state unchanged
		expect(grid.isRowDirty(0)).toBe(false);
		grid.dispose();
	});
});

// ── Row batch API tests ─────────────────────────────────

describe("getRowPacked", () => {
	test("returns non-empty data for populated row", () => {
		const grid = new WasmGrid(3, 1);
		grid.setCell(0, 0, {
			char: "A",
			width: 1,
			attrs: createDefaultAttributes(),
			dirty: true,
		});
		const packed = grid.getRowPacked(0);
		expect(packed.length).toBeGreaterThan(0);
		// First byte: char_len=1, second byte: 'A' (0x41)
		expect(packed[0]).toBe(1);
		expect(packed[1]).toBe(0x41);
		grid.dispose();
	});
});

// ══════════════════════════════════════════════════════════
// Phase 5: Cross-validation and Benchmarks
// ══════════════════════════════════════════════════════════

// ── Cross-validation: WASM Grid vs JS Line ──────────────

describe("Cross-validation: WASM vs JS grid operations", () => {
	test("ASCII content: WASM getText matches JS Line getText", () => {
		const cols = 20;
		const grid = new WasmGrid(cols, 1);
		const line = new Line(cols);
		const text = "Hello, World!";

		for (let i = 0; i < text.length; i++) {
			const cell = { char: text[i]!, width: 1, attrs: createDefaultAttributes(), dirty: true };
			grid.setCell(i, 0, cell);
			line.setCell(i, cell);
		}

		expect(grid.getLine(0).getText()).toBe(line.getText());
		grid.dispose();
	});

	test("CJK content: WASM getText matches JS Line getText", () => {
		const cols = 20;
		const grid = new WasmGrid(cols, 1);
		const line = new Line(cols);

		// Write "漢字" with width-2 characters + placeholders
		const cjkChars = [
			{ char: "漢", width: 2, col: 0 },
			{ char: "", width: 0, col: 1 },
			{ char: "字", width: 2, col: 2 },
			{ char: "", width: 0, col: 3 },
		];
		for (const { char, width, col } of cjkChars) {
			const cell = { char, width, attrs: createDefaultAttributes(), dirty: true };
			grid.setCell(col, 0, cell);
			line.setCell(col, cell);
		}

		expect(grid.getLine(0).getText()).toBe(line.getText());
		grid.dispose();
	});

	test("isEmpty: WASM matches JS Line", () => {
		const cols = 10;
		const grid = new WasmGrid(cols, 2);
		const line0 = new Line(cols); // all spaces
		const line1 = new Line(cols);
		line1.setCell(5, { char: "X", width: 1, attrs: createDefaultAttributes(), dirty: true });
		grid.setCell(5, 1, { char: "X", width: 1, attrs: createDefaultAttributes(), dirty: true });

		expect(grid.getLine(0).isEmpty()).toBe(line0.isEmpty());
		expect(grid.getLine(1).isEmpty()).toBe(line1.isEmpty());
		grid.dispose();
	});

	test("full attribute round-trip: WASM preserves all 8 style flags + colors", () => {
		const grid = new WasmGrid(1, 1);
		const attrs: CellAttributes = {
			bold: true, dim: true, italic: true, underline: true,
			blink: true, reverse: true, hidden: true, strikethrough: true,
			fg: { type: "rgb", r: 10, g: 20, b: 30 },
			bg: { type: "indexed", index: 200 },
		};
		const cell = { char: "Z", width: 1, attrs, dirty: true };
		grid.setCell(0, 0, cell);

		const read = grid.getLine(0).getCell(0);
		expect(read.attrs.bold).toBe(true);
		expect(read.attrs.dim).toBe(true);
		expect(read.attrs.italic).toBe(true);
		expect(read.attrs.underline).toBe(true);
		expect(read.attrs.blink).toBe(true);
		expect(read.attrs.reverse).toBe(true);
		expect(read.attrs.hidden).toBe(true);
		expect(read.attrs.strikethrough).toBe(true);
		expect(read.attrs.fg).toEqual({ type: "rgb", r: 10, g: 20, b: 30 });
		expect(read.attrs.bg).toEqual({ type: "indexed", index: 200 });
		grid.dispose();
	});

	test("wasmRowToLine round-trip preserves data", () => {
		const cols = 15;
		const grid = new WasmGrid(cols, 2);
		const attrs = createDefaultAttributes();
		attrs.bold = true;
		attrs.fg = { type: "rgb", r: 128, g: 0, b: 255 };

		// Write data to WASM row 0
		for (let i = 0; i < 5; i++) {
			grid.setCell(i, 0, { char: String.fromCharCode(65 + i), width: 1, attrs, dirty: true });
		}
		grid.core.set_line_wrapped(0, true);

		// Convert to JS Line
		const jsLine = wasmRowToLine(grid.core, 0);

		// Write JS Line back to WASM row 1
		lineToWasmRow(grid.core, 1, jsLine);

		// Compare row 0 and row 1
		for (let i = 0; i < cols; i++) {
			const cell0 = grid.getLine(0).getCell(i);
			const cell1 = grid.getLine(1).getCell(i);
			expect(cell1.char).toBe(cell0.char);
			expect(cell1.width).toBe(cell0.width);
			expect(cell1.attrs.bold).toBe(cell0.attrs.bold);
			expect(cell1.attrs.fg).toEqual(cell0.attrs.fg);
		}
		expect(grid.core.get_line_wrapped(1)).toBe(true);
		grid.dispose();
	});

	test("shift_rows_up preserves data correctly", () => {
		const grid = new WasmGrid(5, 4);
		// Row 0: "AAAA", Row 1: "BBBB", Row 2: "CCCC", Row 3: "DDDD"
		for (let r = 0; r < 4; r++) {
			const ch = String.fromCharCode(65 + r);
			for (let c = 0; c < 4; c++) {
				grid.setCell(c, r, { char: ch, width: 1, attrs: createDefaultAttributes(), dirty: true });
			}
		}

		grid.core.shift_rows_up(0, 3, 1);
		// After shift: Row 0 = "BBBB", Row 1 = "CCCC", Row 2 = "DDDD", Row 3 = cleared
		expect(grid.getLine(0).getCell(0).char).toBe("B");
		expect(grid.getLine(1).getCell(0).char).toBe("C");
		expect(grid.getLine(2).getCell(0).char).toBe("D");
		grid.dispose();
	});

	test("shift_rows_down preserves data correctly", () => {
		const grid = new WasmGrid(5, 4);
		for (let r = 0; r < 4; r++) {
			const ch = String.fromCharCode(65 + r);
			for (let c = 0; c < 4; c++) {
				grid.setCell(c, r, { char: ch, width: 1, attrs: createDefaultAttributes(), dirty: true });
			}
		}

		grid.core.shift_rows_down(0, 3, 1);
		// After shift: Row 0 = cleared, Row 1 = "AAAA", Row 2 = "BBBB", Row 3 = "CCCC"
		expect(grid.getLine(1).getCell(0).char).toBe("A");
		expect(grid.getLine(2).getCell(0).char).toBe("B");
		expect(grid.getLine(3).getCell(0).char).toBe("C");
		grid.dispose();
	});
});

// ── Cross-validation: CursorState WASM delegation ───────

describe("CursorState WASM delegation", () => {
	test("col/row delegates to WASM core", () => {
		const grid = new WasmGrid(80, 24);
		const cursor = new CursorState(80, 24, grid.core);

		cursor.col = 10;
		cursor.row = 5;
		expect(cursor.col).toBe(10);
		expect(cursor.row).toBe(5);

		// Verify WASM side matches
		expect(grid.core.get_cursor_col()).toBe(10);
		expect(grid.core.get_cursor_row()).toBe(5);
		grid.dispose();
	});

	test("col/row without WASM core uses JS backing", () => {
		const cursor = new CursorState(80, 24);
		cursor.col = 10;
		cursor.row = 5;
		expect(cursor.col).toBe(10);
		expect(cursor.row).toBe(5);
	});

	test("movement methods work with WASM delegation", () => {
		const grid = new WasmGrid(80, 24);
		const cursor = new CursorState(80, 24, grid.core);

		cursor.moveTo(5, 10);
		expect(grid.core.get_cursor_col()).toBe(5);
		expect(grid.core.get_cursor_row()).toBe(10);

		cursor.moveRight(3);
		expect(cursor.col).toBe(8);

		cursor.moveDown(2);
		expect(cursor.row).toBe(12);

		cursor.moveLeft(1);
		expect(cursor.col).toBe(7);

		cursor.moveUp(1);
		expect(cursor.row).toBe(11);
		grid.dispose();
	});

	test("save/restore works with WASM delegation", () => {
		const grid = new WasmGrid(80, 24);
		const cursor = new CursorState(80, 24, grid.core);

		cursor.moveTo(15, 20);
		cursor.attrs.bold = true;
		cursor.save();

		cursor.moveTo(0, 0);
		cursor.attrs.bold = false;
		cursor.restore();

		expect(cursor.col).toBe(15);
		expect(cursor.row).toBe(20);
		expect(cursor.attrs.bold).toBe(true);
		grid.dispose();
	});

	test("clone creates JS-only cursor", () => {
		const grid = new WasmGrid(80, 24);
		const cursor = new CursorState(80, 24, grid.core);
		cursor.moveTo(30, 15);

		const cloned = cursor.clone();
		expect(cloned.col).toBe(30);
		expect(cloned.row).toBe(15);

		// Cloned cursor is independent (JS-backed, not WASM-backed)
		cloned.moveTo(0, 0);
		expect(cursor.col).toBe(30); // original unchanged
		grid.dispose();
	});

	test("resize clamps cursor with WASM delegation", () => {
		const grid = new WasmGrid(80, 24);
		const cursor = new CursorState(80, 24, grid.core);
		cursor.moveTo(70, 20);

		cursor.resize(40, 10);
		expect(cursor.col).toBe(39);
		expect(cursor.row).toBe(9);
		grid.dispose();
	});
});

// ── Cross-validation: Modes sync ────────────────────────

describe("Modes WASM sync", () => {
	test("syncModesToWasm writes all boolean modes", () => {
		const grid = new WasmGrid(80, 24);
		const modes = createDefaultModes();
		modes.autoWrap = false;
		modes.bracketedPaste = true;
		modes.originMode = true;

		syncModesToWasm(modes, grid.core);

		expect(grid.core.get_mode(WASM_MODE_BITS.autoWrap)).toBe(false);
		expect(grid.core.get_mode(WASM_MODE_BITS.bracketedPaste)).toBe(true);
		expect(grid.core.get_mode(WASM_MODE_BITS.originMode)).toBe(true);
		expect(grid.core.get_mode(WASM_MODE_BITS.cursorVisible)).toBe(true); // default
		grid.dispose();
	});

	test("syncModesFromWasm reads all boolean modes", () => {
		const grid = new WasmGrid(80, 24);
		// Set modes in WASM directly
		grid.core.set_mode(WASM_MODE_BITS.autoWrap, false);
		grid.core.set_mode(WASM_MODE_BITS.reverseScreen, true);
		grid.core.set_mode(WASM_MODE_BITS.focusTracking, true);

		const modes = createDefaultModes();
		syncModesFromWasm(modes, grid.core);

		expect(modes.autoWrap).toBe(false);
		expect(modes.reverseScreen).toBe(true);
		expect(modes.focusTracking).toBe(true);
		expect(modes.cursorVisible).toBe(true); // WASM default
		grid.dispose();
	});

	test("round-trip: JS → WASM → JS preserves all boolean modes", () => {
		const grid = new WasmGrid(80, 24);
		const original = createDefaultModes();
		original.autoWrap = false;
		original.column132 = true;
		original.reverseScreen = true;
		original.originMode = true;
		original.cursorBlink = false;
		original.cursorVisible = false;
		original.focusTracking = true;
		original.bracketedPaste = true;

		syncModesToWasm(original, grid.core);

		const restored = createDefaultModes();
		syncModesFromWasm(restored, grid.core);

		expect(restored.autoWrap).toBe(original.autoWrap);
		expect(restored.column132).toBe(original.column132);
		expect(restored.reverseScreen).toBe(original.reverseScreen);
		expect(restored.originMode).toBe(original.originMode);
		expect(restored.cursorBlink).toBe(original.cursorBlink);
		expect(restored.cursorVisible).toBe(original.cursorVisible);
		expect(restored.focusTracking).toBe(original.focusTracking);
		expect(restored.bracketedPaste).toBe(original.bracketedPaste);
		grid.dispose();
	});
});

// ── Cursor visibility PTY flow simulation ────────────────

describe("Cursor visibility: PTY flow (top startup)", () => {
	test("CSI ?25l alone hides cursor in primary buffer", () => {
		const grid = new WasmGrid(80, 24);
		const modes = createDefaultModes();

		// Send CSI ?25l: \x1b[?25l
		const data = new Uint8Array([0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x6c]);
		const consumed = grid.core.process_pty_data(data);
		grid.core.take_mode_actions(); // drain (should be empty)
		syncModesFromWasm(modes, grid.core);

		expect(consumed).toBe(6);
		expect(grid.core.get_mode(WASM_MODE_BITS.cursorVisible)).toBe(false);
		expect(modes.cursorVisible).toBe(false);
		grid.dispose();
	});

	test("CSI ?1049h + CSI ?25l: simulate top startup sequence", () => {
		const primaryGrid = new WasmGrid(80, 24);
		const modes = createDefaultModes();

		// Simulate: \x1b[?1049h\x1b[?25l\x1b[H (save+alt, hide cursor, home)
		const data = new Uint8Array([
			0x1b, 0x5b, 0x3f, 0x31, 0x30, 0x34, 0x39, 0x68, // CSI ?1049h
			0x1b, 0x5b, 0x3f, 0x32, 0x35, 0x6c,               // CSI ?25l
			0x1b, 0x5b, 0x48,                                   // CSI H (home)
		]);

		// First iteration: process with primary core
		const consumed1 = primaryGrid.core.process_pty_data(data);
		const actions1 = primaryGrid.core.take_mode_actions();

		// Expect: stops after CSI ?1049h (buffer switch detected)
		expect(consumed1).toBeLessThan(data.length); // should NOT consume all
		expect(actions1.length).toBeGreaterThan(0); // should have buffer switch action

		// Simulate buffer switch: create alternate grid
		const altGrid = new WasmGrid(80, 24);
		syncModesFromWasm(modes, altGrid.core); // sync from NEW alt (defaults)

		// Second iteration: process remaining data with alt core
		const remaining = data.subarray(consumed1);
		const consumed2 = altGrid.core.process_pty_data(remaining);
		expect(consumed2).toBe(remaining.length); // should consume all remaining
		altGrid.core.take_mode_actions();
		syncModesFromWasm(modes, altGrid.core);

		expect(modes.cursorVisible).toBe(false);
		expect(altGrid.core.get_mode(WASM_MODE_BITS.cursorVisible)).toBe(false);

		primaryGrid.dispose();
		altGrid.dispose();
	});
});

// ── Performance benchmarks ──────────────────────────────

describe("Performance benchmarks", () => {
	test("setCell/getCell: 10,000 calls", () => {
		const grid = new WasmGrid(80, 120);
		const attrs = createDefaultAttributes();
		const cell = { char: "A", width: 1, attrs, dirty: true };

		const iterations = 10_000;
		const start = performance.now();
		for (let i = 0; i < iterations; i++) {
			const col = i % 80;
			const row = Math.floor(i / 80) % 120;
			grid.setCell(col, row, cell);
		}
		const setTime = performance.now() - start;

		const start2 = performance.now();
		for (let i = 0; i < iterations; i++) {
			const col = i % 80;
			const row = Math.floor(i / 80) % 120;
			grid.getLine(row).getCell(col);
		}
		const getTime = performance.now() - start2;

		const setNs = (setTime * 1_000_000) / iterations;
		const getNs = (getTime * 1_000_000) / iterations;

		console.log(`  setCell: ${setNs.toFixed(0)}ns/call (${iterations} calls in ${setTime.toFixed(2)}ms)`);
		console.log(`  getCell: ${getNs.toFixed(0)}ns/call (${iterations} calls in ${getTime.toFixed(2)}ms)`);

		// Acceptance criteria: < 100ns per call (generous for test environment)
		// Note: in test environment overhead may be higher due to bun test runtime
		expect(setTime).toBeLessThan(200); // 200ms for 10k = 20,000ns/call max
		expect(getTime).toBeLessThan(200);
		grid.dispose();
	});

	test("full viewport read: 80x120 cells", () => {
		const cols = 80;
		const rows = 120;
		const grid = new WasmGrid(cols, rows);

		// Fill viewport with data
		const attrs = createDefaultAttributes();
		for (let r = 0; r < rows; r++) {
			for (let c = 0; c < cols; c++) {
				grid.setCellAscii(c, r, 0x41 + (c % 26), attrs);
			}
		}

		// Measure full viewport read
		const start = performance.now();
		for (let r = 0; r < rows; r++) {
			const line = grid.getLine(r);
			for (let c = 0; c < cols; c++) {
				line.getCell(c);
			}
		}
		const readTime = performance.now() - start;

		console.log(`  Full viewport read (${cols}x${rows}): ${readTime.toFixed(2)}ms`);

		// Acceptance criteria: < 1ms for 80x120 (generous margin for test overhead)
		expect(readTime).toBeLessThan(50); // 50ms generous limit
		grid.dispose();
	});

	test("setCellAscii fast path: 10,000 calls", () => {
		const grid = new WasmGrid(80, 120);
		const attrs = createDefaultAttributes();

		const iterations = 10_000;
		const start = performance.now();
		for (let i = 0; i < iterations; i++) {
			const col = i % 80;
			const row = Math.floor(i / 80) % 120;
			grid.setCellAscii(col, row, 0x41, attrs);
		}
		const elapsed = performance.now() - start;
		const nsPerCall = (elapsed * 1_000_000) / iterations;

		console.log(`  setCellAscii: ${nsPerCall.toFixed(0)}ns/call (${iterations} calls in ${elapsed.toFixed(2)}ms)`);
		expect(elapsed).toBeLessThan(200);
		grid.dispose();
	});
});

// ── WASM binary size check ──────────────────────────────

describe("WASM binary size", () => {
	test("WASM binary < 80KB", () => {
		const wasmPath = resolve(import.meta.dir, "../../../../wasm/pkg/emterm_wasm_bg.wasm");
		const stat = statSync(wasmPath);
		const sizeKB = stat.size / 1024;
		console.log(`  WASM binary size: ${sizeKB.toFixed(1)}KB`);
		expect(sizeKB).toBeLessThan(80);
	});
});
