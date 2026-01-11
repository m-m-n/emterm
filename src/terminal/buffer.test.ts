/**
 * Tests for ScreenBuffer.
 */
import { describe, expect, test } from "bun:test";
import { ScreenBuffer } from "./buffer.ts";
import { createCell } from "./grid.ts";

describe("ScreenBuffer", () => {
	describe("constructor", () => {
		test("creates buffer with specified dimensions", () => {
			const buffer = new ScreenBuffer(80, 24);
			expect(buffer.cols).toBe(80);
			expect(buffer.rows).toBe(24);
		});

		test("initializes all lines", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let row = 0; row < 5; row++) {
				const line = buffer.getLine(row);
				expect(line.length).toBe(10);
			}
		});
	});

	describe("getLine", () => {
		test("returns line at specified row", () => {
			const buffer = new ScreenBuffer(10, 5);
			const line = buffer.getLine(2);
			expect(line.length).toBe(10);
		});

		test("throws for out of bounds row", () => {
			const buffer = new ScreenBuffer(10, 5);
			expect(() => buffer.getLine(5)).toThrow();
			expect(() => buffer.getLine(-1)).toThrow();
		});
	});

	describe("getCell", () => {
		test("returns cell at specified position", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.setCell(3, 2, createCell("X"));
			expect(buffer.getCell(3, 2).char).toBe("X");
		});

		test("throws for out of bounds", () => {
			const buffer = new ScreenBuffer(10, 5);
			expect(() => buffer.getCell(10, 0)).toThrow();
			expect(() => buffer.getCell(0, 5)).toThrow();
		});
	});

	describe("setCell", () => {
		test("sets cell at specified position", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.setCell(5, 3, createCell("A"));
			expect(buffer.getCell(5, 3).char).toBe("A");
		});

		test("marks line as dirty", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.clearAllDirty();
			buffer.setCell(0, 0, createCell("X"));
			expect(buffer.getLine(0).dirty).toBe(true);
		});
	});

	describe("scrollUp", () => {
		test("scrolls content up by one line", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.setCell(0, 0, createCell("A"));
			buffer.setCell(0, 1, createCell("B"));
			buffer.setCell(0, 2, createCell("C"));

			buffer.scrollUp();

			expect(buffer.getCell(0, 0).char).toBe("B");
			expect(buffer.getCell(0, 1).char).toBe("C");
			expect(buffer.getCell(0, 4).char).toBe(" "); // New bottom line is empty
		});

		test("scrolls content up by specified count", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 5; i++) {
				buffer.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buffer.scrollUp(2);

			expect(buffer.getCell(0, 0).char).toBe("C");
			expect(buffer.getCell(0, 1).char).toBe("D");
			expect(buffer.getCell(0, 2).char).toBe("E");
			expect(buffer.getCell(0, 3).char).toBe(" ");
			expect(buffer.getCell(0, 4).char).toBe(" ");
		});
	});

	describe("scrollDown", () => {
		test("scrolls content down by one line", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.setCell(0, 0, createCell("A"));
			buffer.setCell(0, 1, createCell("B"));

			buffer.scrollDown();

			expect(buffer.getCell(0, 0).char).toBe(" "); // New top line is empty
			expect(buffer.getCell(0, 1).char).toBe("A");
			expect(buffer.getCell(0, 2).char).toBe("B");
		});
	});

	describe("clearAll", () => {
		test("clears all lines", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.setCell(0, 0, createCell("A"));
			buffer.setCell(5, 3, createCell("B"));

			buffer.clearAll();

			for (let row = 0; row < 5; row++) {
				for (let col = 0; col < 10; col++) {
					expect(buffer.getCell(col, row).char).toBe(" ");
				}
			}
		});
	});

	describe("clearLine", () => {
		test("clears entire line", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 10; i++) {
				buffer.setCell(i, 2, createCell("X"));
			}

			buffer.clearLine(2);

			for (let i = 0; i < 10; i++) {
				expect(buffer.getCell(i, 2).char).toBe(" ");
			}
		});
	});

	describe("clearLineFromCursor", () => {
		test("clears from cursor to end of line", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 10; i++) {
				buffer.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buffer.clearLineFromCursor(0, 5);

			expect(buffer.getCell(4, 0).char).toBe("E");
			expect(buffer.getCell(5, 0).char).toBe(" ");
			expect(buffer.getCell(9, 0).char).toBe(" ");
		});
	});

	describe("clearLineToCursor", () => {
		test("clears from start of line to cursor", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 10; i++) {
				buffer.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buffer.clearLineToCursor(0, 5);

			expect(buffer.getCell(0, 0).char).toBe(" ");
			expect(buffer.getCell(5, 0).char).toBe(" ");
			expect(buffer.getCell(6, 0).char).toBe("G");
		});
	});

	describe("clearBelow", () => {
		test("clears from cursor position to end of screen", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let row = 0; row < 5; row++) {
				for (let col = 0; col < 10; col++) {
					buffer.setCell(col, row, createCell("X"));
				}
			}

			buffer.clearBelow(5, 2);

			// Row 2, cols 0-4 should still be X
			expect(buffer.getCell(4, 2).char).toBe("X");
			// Row 2, cols 5-9 should be cleared
			expect(buffer.getCell(5, 2).char).toBe(" ");
			// Row 3 and 4 should be cleared
			expect(buffer.getCell(0, 3).char).toBe(" ");
			expect(buffer.getCell(0, 4).char).toBe(" ");
		});
	});

	describe("clearAbove", () => {
		test("clears from cursor position to start of screen", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let row = 0; row < 5; row++) {
				for (let col = 0; col < 10; col++) {
					buffer.setCell(col, row, createCell("X"));
				}
			}

			buffer.clearAbove(5, 2);

			// Row 0 and 1 should be cleared
			expect(buffer.getCell(0, 0).char).toBe(" ");
			expect(buffer.getCell(0, 1).char).toBe(" ");
			// Row 2, cols 0-5 should be cleared
			expect(buffer.getCell(5, 2).char).toBe(" ");
			// Row 2, cols 6-9 should still be X
			expect(buffer.getCell(6, 2).char).toBe("X");
		});
	});

	describe("getDirtyRows", () => {
		test("returns indices of dirty rows", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.clearAllDirty();

			buffer.getLine(1).dirty = true;
			buffer.getLine(3).dirty = true;

			const dirty = buffer.getDirtyRows();
			expect(dirty).toEqual([1, 3]);
		});

		test("returns empty array when no dirty rows", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.clearAllDirty();

			const dirty = buffer.getDirtyRows();
			expect(dirty).toEqual([]);
		});
	});

	describe("clearAllDirty", () => {
		test("clears dirty flag on all lines", () => {
			const buffer = new ScreenBuffer(10, 5);

			buffer.clearAllDirty();

			for (let row = 0; row < 5; row++) {
				expect(buffer.getLine(row).dirty).toBe(false);
			}
		});
	});

	describe("resize", () => {
		test("expands buffer", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.setCell(0, 0, createCell("A"));

			buffer.resize(20, 10);

			expect(buffer.cols).toBe(20);
			expect(buffer.rows).toBe(10);
			expect(buffer.getCell(0, 0).char).toBe("A");
			expect(buffer.getCell(19, 9).char).toBe(" ");
		});

		test("shrinks buffer", () => {
			const buffer = new ScreenBuffer(10, 5);
			buffer.setCell(9, 4, createCell("Z"));

			buffer.resize(5, 3);

			expect(buffer.cols).toBe(5);
			expect(buffer.rows).toBe(3);
			expect(() => buffer.getCell(9, 4)).toThrow();
		});
	});

	// Phase 4: Scroll region tests
	describe("setScrollRegion", () => {
		test("sets scroll region", () => {
			const buffer = new ScreenBuffer(10, 10);
			buffer.setScrollRegion(2, 7);

			const region = buffer.getScrollRegion();
			expect(region).toEqual({ top: 2, bottom: 7 });
		});

		test("clears region when full screen", () => {
			const buffer = new ScreenBuffer(10, 10);
			buffer.setScrollRegion(2, 7);
			buffer.setScrollRegion(0, 9);

			expect(buffer.getScrollRegion()).toBeNull();
		});

		test("ignores invalid region (top >= bottom)", () => {
			const buffer = new ScreenBuffer(10, 10);
			buffer.setScrollRegion(5, 5);

			expect(buffer.getScrollRegion()).toBeNull();
		});
	});

	describe("scrollUp with region", () => {
		test("scrolls only within region", () => {
			const buffer = new ScreenBuffer(10, 6);
			for (let i = 0; i < 6; i++) {
				buffer.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buffer.setScrollRegion(1, 4);
			buffer.scrollUp(1);

			// Row 0 (outside region) should be unchanged
			expect(buffer.getCell(0, 0).char).toBe("A");
			// Row 1 should now have what was in row 2
			expect(buffer.getCell(0, 1).char).toBe("C");
			// Row 4 should be blank (bottom of region)
			expect(buffer.getCell(0, 4).char).toBe(" ");
			// Row 5 (outside region) should be unchanged
			expect(buffer.getCell(0, 5).char).toBe("F");
		});
	});

	describe("scrollDown with region", () => {
		test("scrolls only within region", () => {
			const buffer = new ScreenBuffer(10, 6);
			for (let i = 0; i < 6; i++) {
				buffer.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buffer.setScrollRegion(1, 4);
			buffer.scrollDown(1);

			// Row 0 (outside region) should be unchanged
			expect(buffer.getCell(0, 0).char).toBe("A");
			// Row 1 should be blank (top of region)
			expect(buffer.getCell(0, 1).char).toBe(" ");
			// Row 2 should now have what was in row 1
			expect(buffer.getCell(0, 2).char).toBe("B");
			// Row 5 (outside region) should be unchanged
			expect(buffer.getCell(0, 5).char).toBe("F");
		});
	});

	// Phase 4: Insert/delete lines tests
	describe("insertLines", () => {
		test("inserts blank lines at cursor row", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 5; i++) {
				buffer.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buffer.insertLines(2, 1);

			// Row 2 should now be blank
			expect(buffer.getCell(0, 2).char).toBe(" ");
			// Original row 2 content moved to row 3
			expect(buffer.getCell(0, 3).char).toBe("C");
			// Row 4 should have what was in row 3
			expect(buffer.getCell(0, 4).char).toBe("D");
			// Original row 4 content pushed out
		});

		test("respects scroll region", () => {
			const buffer = new ScreenBuffer(10, 6);
			for (let i = 0; i < 6; i++) {
				buffer.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buffer.setScrollRegion(1, 4);
			buffer.insertLines(2, 1);

			// Row 0 unchanged
			expect(buffer.getCell(0, 0).char).toBe("A");
			// Row 2 should be blank
			expect(buffer.getCell(0, 2).char).toBe(" ");
			// Row 5 unchanged
			expect(buffer.getCell(0, 5).char).toBe("F");
		});
	});

	describe("deleteLines", () => {
		test("deletes lines at cursor row", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 5; i++) {
				buffer.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buffer.deleteLines(1, 1);

			// Row 1 should now have what was in row 2
			expect(buffer.getCell(0, 1).char).toBe("C");
			// Last row should be blank
			expect(buffer.getCell(0, 4).char).toBe(" ");
		});
	});

	// Phase 4: Insert/delete characters tests
	describe("insertCharacters", () => {
		test("inserts blank characters at position", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 10; i++) {
				buffer.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buffer.insertCharacters(0, 3, 2);

			// Columns 3-4 should be blank
			expect(buffer.getCell(3, 0).char).toBe(" ");
			expect(buffer.getCell(4, 0).char).toBe(" ");
			// Original column 3 content moved to column 5
			expect(buffer.getCell(5, 0).char).toBe("D");
		});
	});

	describe("deleteCharacters", () => {
		test("deletes characters at position", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 10; i++) {
				buffer.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buffer.deleteCharacters(0, 3, 2);

			// Column 3 should now have what was in column 5
			expect(buffer.getCell(3, 0).char).toBe("F");
			// Last columns should be blank
			expect(buffer.getCell(8, 0).char).toBe(" ");
			expect(buffer.getCell(9, 0).char).toBe(" ");
		});
	});

	describe("eraseCharacters", () => {
		test("erases characters without shifting", () => {
			const buffer = new ScreenBuffer(10, 5);
			for (let i = 0; i < 10; i++) {
				buffer.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buffer.eraseCharacters(0, 3, 2);

			// Columns 3-4 should be blank
			expect(buffer.getCell(3, 0).char).toBe(" ");
			expect(buffer.getCell(4, 0).char).toBe(" ");
			// Column 5 should be unchanged
			expect(buffer.getCell(5, 0).char).toBe("F");
		});
	});
});
