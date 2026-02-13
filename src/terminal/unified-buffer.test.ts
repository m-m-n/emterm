/**
 * Tests for UnifiedBuffer.
 */
import { describe, expect, test } from "bun:test";
import { UnifiedBuffer } from "./unified-buffer.ts";
import { createCell, createEmptyCell, Line } from "./grid.ts";

describe("UnifiedBuffer", () => {
	// ===== Phase 1: Ring Buffer + Core =====

	describe("constructor", () => {
		test("creates buffer with specified dimensions", () => {
			const buf = new UnifiedBuffer(80, 24, 1000);
			expect(buf.cols).toBe(80);
			expect(buf.rows).toBe(24);
		});

		test("initializes viewport with empty lines", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			for (let row = 0; row < 5; row++) {
				const line = buf.getLine(row);
				expect(line.length).toBe(10);
				expect(line.getCell(0).char).toBe(" ");
			}
		});

		test("initial scrollback is 0", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			expect(buf.scrollbackLength).toBe(0);
		});

		test("initial size equals rows", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			expect(buf.size).toBe(5);
		});
	});

	describe("ring buffer push/get", () => {
		test("push below capacity increases size", () => {
			const buf = new UnifiedBuffer(10, 3, 5);
			// capacity = 5 + 3 = 8, initial size = 3
			const line = new Line(10);
			line.setCell(0, createCell("X"));
			buf.push(line);
			expect(buf.size).toBe(4);
			expect(buf.scrollbackLength).toBe(1);
		});

		test("push at capacity evicts oldest line", () => {
			const buf = new UnifiedBuffer(10, 2, 2);
			// capacity = 2 + 2 = 4, initial size = 2

			// Push 2 scrollback lines (filling to capacity)
			const lineA = new Line(10);
			lineA.setCell(0, createCell("A"));
			buf.push(lineA);

			const lineB = new Line(10);
			lineB.setCell(0, createCell("B"));
			buf.push(lineB);

			expect(buf.size).toBe(4); // at capacity

			// Push one more - should evict the oldest
			const lineC = new Line(10);
			lineC.setCell(0, createCell("C"));
			buf.push(lineC);

			expect(buf.size).toBe(4); // still at capacity
			// The oldest line (initial empty) was evicted
			expect(buf.scrollbackLength).toBe(2);
		});

		test("push above capacity continuously evicts oldest", () => {
			const buf = new UnifiedBuffer(5, 2, 3);
			// capacity = 3 + 2 = 5, initial size = 2

			// Push 10 lines (way over capacity)
			for (let i = 0; i < 10; i++) {
				const line = new Line(5);
				line.setCell(0, createCell(String.fromCharCode(65 + i))); // A, B, C...
				buf.push(line);
			}

			expect(buf.size).toBe(5); // capped at capacity
			expect(buf.scrollbackLength).toBe(3); // capacity - rows

			// Viewport should contain the last 2 pushed lines
			expect(buf.getLine(0).getCell(0).char).toBe("I"); // 9th push (index 8)
			expect(buf.getLine(1).getCell(0).char).toBe("J"); // 10th push (index 9)
		});
	});

	describe("drain", () => {
		test("returns all lines in order", () => {
			const buf = new UnifiedBuffer(5, 3, 2);
			// Push some lines to create scrollback
			const lineA = new Line(5);
			lineA.setCell(0, createCell("A"));
			buf.push(lineA);

			const lineB = new Line(5);
			lineB.setCell(0, createCell("B"));
			buf.push(lineB);

			const drained = buf.drain();
			expect(drained.length).toBe(5); // 3 initial + 2 pushed
			// First 3 are initial empty lines
			expect(drained[0]!.getCell(0).char).toBe(" ");
			// Last 2 are pushed lines
			expect(drained[3]!.getCell(0).char).toBe("A");
			expect(drained[4]!.getCell(0).char).toBe("B");
		});

		test("resets buffer state after drain", () => {
			const buf = new UnifiedBuffer(5, 3, 2);
			buf.push(new Line(5));
			buf.drain();
			expect(buf.size).toBe(0);
		});
	});

	describe("viewport access", () => {
		test("getLine returns viewport lines", () => {
			const buf = new UnifiedBuffer(10, 3, 5);
			// Set content in viewport
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(0, 1, createCell("B"));
			buf.setCell(0, 2, createCell("C"));

			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(1).getCell(0).char).toBe("B");
			expect(buf.getLine(2).getCell(0).char).toBe("C");
		});

		test("getLine throws for out of bounds row", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			expect(() => buf.getLine(5)).toThrow();
			expect(() => buf.getLine(-1)).toThrow();
		});

		test("getCell/setCell roundtrip", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			const cell = createCell("Z");
			buf.setCell(3, 2, cell);
			expect(buf.getCell(3, 2).char).toBe("Z");
		});
	});

	describe("scrollback access", () => {
		test("getScrollbackLine returns scrollback lines", () => {
			const buf = new UnifiedBuffer(5, 2, 5);
			// Push lines to create scrollback
			for (let i = 0; i < 3; i++) {
				const line = new Line(5);
				line.setCell(0, createCell(String.fromCharCode(65 + i)));
				buf.push(line);
			}
			// Initial 2 lines + 3 pushed = 5 total, viewport = 2, scrollback = 3
			expect(buf.scrollbackLength).toBe(3);
			// Scrollback line 0 is the oldest (first initial empty line)
			expect(buf.getScrollbackLine(0).getCell(0).char).toBe(" ");
			// Scrollback line 2 is the first pushed line
			expect(buf.getScrollbackLine(2).getCell(0).char).toBe("A");
		});

		test("scrollbackLength returns correct count", () => {
			const buf = new UnifiedBuffer(5, 3, 10);
			expect(buf.scrollbackLength).toBe(0);

			buf.push(new Line(5));
			expect(buf.scrollbackLength).toBe(1);

			buf.push(new Line(5));
			expect(buf.scrollbackLength).toBe(2);
		});
	});

	describe("scroll region", () => {
		test("setScrollRegion stores region", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			buf.setScrollRegion(1, 3);
			const region = buf.getScrollRegion();
			expect(region).not.toBeNull();
			expect(region!.top).toBe(1);
			expect(region!.bottom).toBe(3);
		});

		test("setScrollRegion full screen clears region", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			buf.setScrollRegion(0, 4);
			expect(buf.getScrollRegion()).toBeNull();
		});

		test("clearScrollRegion resets to null", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			buf.setScrollRegion(1, 3);
			buf.clearScrollRegion();
			expect(buf.getScrollRegion()).toBeNull();
		});

		test("getEffectiveScrollRegion returns full screen when no region set", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			const region = buf.getEffectiveScrollRegion();
			expect(region.top).toBe(0);
			expect(region.bottom).toBe(4);
		});

		test("getEffectiveScrollRegion returns set region", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			buf.setScrollRegion(1, 3);
			const region = buf.getEffectiveScrollRegion();
			expect(region.top).toBe(1);
			expect(region.bottom).toBe(3);
		});
	});

	describe("clear operations", () => {
		test("clearAll clears all viewport lines", () => {
			const buf = new UnifiedBuffer(10, 3, 100);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(5, 1, createCell("B"));
			buf.setCell(9, 2, createCell("C"));

			buf.clearAll();

			for (let row = 0; row < 3; row++) {
				for (let col = 0; col < 10; col++) {
					expect(buf.getCell(col, row).char).toBe(" ");
				}
			}
		});

		test("clearLine clears specific row", () => {
			const buf = new UnifiedBuffer(10, 3, 100);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(0, 1, createCell("B"));
			buf.setCell(0, 2, createCell("C"));

			buf.clearLine(1);

			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(0, 1).char).toBe(" ");
			expect(buf.getCell(0, 2).char).toBe("C");
		});

		test("clearLineFromCursor clears from col to end", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}
			buf.clearLineFromCursor(0, 2);
			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(1, 0).char).toBe("B");
			expect(buf.getCell(2, 0).char).toBe(" ");
			expect(buf.getCell(3, 0).char).toBe(" ");
			expect(buf.getCell(4, 0).char).toBe(" ");
		});

		test("clearLineToCursor clears from start to col (inclusive)", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}
			buf.clearLineToCursor(0, 2);
			expect(buf.getCell(0, 0).char).toBe(" ");
			expect(buf.getCell(1, 0).char).toBe(" ");
			expect(buf.getCell(2, 0).char).toBe(" ");
			expect(buf.getCell(3, 0).char).toBe("D");
			expect(buf.getCell(4, 0).char).toBe("E");
		});

		test("clearBelow clears from cursor down", () => {
			const buf = new UnifiedBuffer(5, 3, 100);
			for (let row = 0; row < 3; row++) {
				buf.setCell(0, row, createCell(String.fromCharCode(65 + row)));
			}
			buf.clearBelow(0, 1);

			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(0, 1).char).toBe(" ");
			expect(buf.getCell(0, 2).char).toBe(" ");
		});

		test("clearAbove clears from start to cursor", () => {
			const buf = new UnifiedBuffer(5, 3, 100);
			for (let row = 0; row < 3; row++) {
				buf.setCell(0, row, createCell(String.fromCharCode(65 + row)));
			}
			buf.clearAbove(4, 1);

			expect(buf.getCell(0, 0).char).toBe(" ");
			expect(buf.getCell(0, 1).char).toBe(" ");
			expect(buf.getCell(0, 2).char).toBe("C");
		});

		test("clearScrollback retains only viewport", () => {
			const buf = new UnifiedBuffer(5, 2, 10);
			// Push lines to create scrollback
			for (let i = 0; i < 5; i++) {
				const line = new Line(5);
				line.setCell(0, createCell(String.fromCharCode(65 + i)));
				buf.push(line);
			}
			expect(buf.scrollbackLength).toBe(5);

			// Set known content in viewport
			buf.setCell(0, 0, createCell("X"));
			buf.setCell(0, 1, createCell("Y"));

			buf.clearScrollback();

			expect(buf.scrollbackLength).toBe(0);
			expect(buf.size).toBe(2); // only viewport
			expect(buf.getLine(0).getCell(0).char).toBe("X");
			expect(buf.getLine(1).getCell(0).char).toBe("Y");
		});
	});

	describe("clone", () => {
		test("creates independent copy", () => {
			const buf = new UnifiedBuffer(5, 3, 10);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(1, 1, createCell("B"));

			const cloned = buf.clone();
			expect(cloned.getCell(0, 0).char).toBe("A");
			expect(cloned.getCell(1, 1).char).toBe("B");
			expect(cloned.cols).toBe(5);
			expect(cloned.rows).toBe(3);

			// Modifying clone doesn't affect original
			cloned.setCell(0, 0, createCell("Z"));
			expect(buf.getCell(0, 0).char).toBe("A");
			expect(cloned.getCell(0, 0).char).toBe("Z");
		});

		test("preserves scrollback in clone", () => {
			const buf = new UnifiedBuffer(5, 2, 5);
			for (let i = 0; i < 3; i++) {
				const line = new Line(5);
				line.setCell(0, createCell(String.fromCharCode(65 + i)));
				buf.push(line);
			}

			const cloned = buf.clone();
			expect(cloned.scrollbackLength).toBe(buf.scrollbackLength);
			expect(cloned.size).toBe(buf.size);
		});

		test("preserves scroll region in clone", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			buf.setScrollRegion(1, 3);

			const cloned = buf.clone();
			const region = cloned.getScrollRegion();
			expect(region).not.toBeNull();
			expect(region!.top).toBe(1);
			expect(region!.bottom).toBe(3);
		});
	});

	// ===== Phase 2: Scroll Operations + Line/Character Manipulation =====

	describe("scrollUp", () => {
		test("scrolls content up by one line (full screen)", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(0, 1, createCell("B"));
			buf.setCell(0, 2, createCell("C"));

			buf.scrollUp();

			expect(buf.getCell(0, 0).char).toBe("B");
			expect(buf.getCell(0, 1).char).toBe("C");
			expect(buf.getCell(0, 4).char).toBe(" "); // New bottom line is empty
		});

		test("scrolls content up by specified count", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buf.scrollUp(2);

			expect(buf.getCell(0, 0).char).toBe("C");
			expect(buf.getCell(0, 1).char).toBe("D");
			expect(buf.getCell(0, 2).char).toBe("E");
			expect(buf.getCell(0, 3).char).toBe(" ");
			expect(buf.getCell(0, 4).char).toBe(" ");
		});

		test("line scrolled off top becomes scrollback (full screen)", () => {
			const buf = new UnifiedBuffer(5, 3, 10);
			buf.setCell(0, 0, createCell("X"));

			const prevScrollback = buf.scrollbackLength;
			buf.scrollUp();

			expect(buf.scrollbackLength).toBe(prevScrollback + 1);
			// The scrolled-off line should be accessible as scrollback
			expect(buf.getScrollbackLine(buf.scrollbackLength - 1).getCell(0).char).toBe("X");
		});

		test("scrollUp with scroll region only affects region", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}
			buf.setScrollRegion(1, 3);

			buf.scrollUp();

			// Row 0 (outside region) unchanged
			expect(buf.getCell(0, 0).char).toBe("A");
			// Region shifted up
			expect(buf.getCell(0, 1).char).toBe("C");
			expect(buf.getCell(0, 2).char).toBe("D");
			expect(buf.getCell(0, 3).char).toBe(" "); // New blank at bottom of region
			// Row 4 (outside region) unchanged
			expect(buf.getCell(0, 4).char).toBe("E");
		});

		test("scrollUp with top=0 but bottom<rows-1 uses in-place (not ring push)", () => {
			const buf = new UnifiedBuffer(5, 5, 10);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}
			buf.setScrollRegion(0, 3); // top=0, bottom=3 (not full screen)

			const prevScrollback = buf.scrollbackLength;
			buf.scrollUp();

			// This should NOT create scrollback (partial region)
			expect(buf.scrollbackLength).toBe(prevScrollback);
			// Region (rows 0-3) shifted up
			expect(buf.getCell(0, 0).char).toBe("B");
			expect(buf.getCell(0, 1).char).toBe("C");
			expect(buf.getCell(0, 2).char).toBe("D");
			expect(buf.getCell(0, 3).char).toBe(" ");
			// Row 4 (outside region) unchanged
			expect(buf.getCell(0, 4).char).toBe("E");
		});

		test("does nothing for count <= 0", () => {
			const buf = new UnifiedBuffer(5, 3, 10);
			buf.setCell(0, 0, createCell("A"));
			buf.scrollUp(0);
			expect(buf.getCell(0, 0).char).toBe("A");
		});
	});

	describe("scrollDown", () => {
		test("scrolls content down by one line", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buf.scrollDown();

			expect(buf.getCell(0, 0).char).toBe(" "); // New blank at top
			expect(buf.getCell(0, 1).char).toBe("A");
			expect(buf.getCell(0, 2).char).toBe("B");
			expect(buf.getCell(0, 3).char).toBe("C");
			expect(buf.getCell(0, 4).char).toBe("D");
		});

		test("scrollDown with scroll region only affects region", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}
			buf.setScrollRegion(1, 3);

			buf.scrollDown();

			expect(buf.getCell(0, 0).char).toBe("A"); // Above region unchanged
			expect(buf.getCell(0, 1).char).toBe(" "); // New blank at top of region
			expect(buf.getCell(0, 2).char).toBe("B");
			expect(buf.getCell(0, 3).char).toBe("C");
			expect(buf.getCell(0, 4).char).toBe("E"); // Below region unchanged
		});

		test("does nothing for count <= 0", () => {
			const buf = new UnifiedBuffer(5, 3, 10);
			buf.setCell(0, 0, createCell("A"));
			buf.scrollDown(0);
			expect(buf.getCell(0, 0).char).toBe("A");
		});
	});

	describe("insertLines", () => {
		test("inserts blank lines within scroll region", () => {
			const buf = new UnifiedBuffer(5, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buf.insertLines(1, 2);

			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(0, 1).char).toBe(" "); // Inserted blank
			expect(buf.getCell(0, 2).char).toBe(" "); // Inserted blank
			expect(buf.getCell(0, 3).char).toBe("B");
			expect(buf.getCell(0, 4).char).toBe("C");
			// D and E were pushed out
		});

		test("insertLines outside scroll region is no-op", () => {
			const buf = new UnifiedBuffer(5, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}
			buf.setScrollRegion(1, 3);

			buf.insertLines(0, 1); // Row 0 is outside the region

			// Nothing should change
			for (let i = 0; i < 5; i++) {
				expect(buf.getCell(0, i).char).toBe(String.fromCharCode(65 + i));
			}
		});
	});

	describe("deleteLines", () => {
		test("deletes lines within scroll region", () => {
			const buf = new UnifiedBuffer(5, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}

			buf.deleteLines(1, 2);

			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(0, 1).char).toBe("D");
			expect(buf.getCell(0, 2).char).toBe("E");
			expect(buf.getCell(0, 3).char).toBe(" "); // Blank at bottom
			expect(buf.getCell(0, 4).char).toBe(" "); // Blank at bottom
		});

		test("deleteLines outside scroll region is no-op", () => {
			const buf = new UnifiedBuffer(5, 5, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(0, i, createCell(String.fromCharCode(65 + i)));
			}
			buf.setScrollRegion(1, 3);

			buf.deleteLines(0, 1);

			for (let i = 0; i < 5; i++) {
				expect(buf.getCell(0, i).char).toBe(String.fromCharCode(65 + i));
			}
		});
	});

	describe("insertCharacters", () => {
		test("shifts cells right and inserts blanks", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buf.insertCharacters(0, 1, 2);

			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(1, 0).char).toBe(" ");
			expect(buf.getCell(2, 0).char).toBe(" ");
			expect(buf.getCell(3, 0).char).toBe("B");
			expect(buf.getCell(4, 0).char).toBe("C");
		});
	});

	describe("deleteCharacters", () => {
		test("shifts cells left and blanks at end", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buf.deleteCharacters(0, 1, 2);

			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(1, 0).char).toBe("D");
			expect(buf.getCell(2, 0).char).toBe("E");
			expect(buf.getCell(3, 0).char).toBe(" ");
			expect(buf.getCell(4, 0).char).toBe(" ");
		});
	});

	describe("eraseCharacters", () => {
		test("blanks cells in-place without shifting", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			buf.eraseCharacters(0, 1, 2);

			expect(buf.getCell(0, 0).char).toBe("A");
			expect(buf.getCell(1, 0).char).toBe(" ");
			expect(buf.getCell(2, 0).char).toBe(" ");
			expect(buf.getCell(3, 0).char).toBe("D");
			expect(buf.getCell(4, 0).char).toBe("E");
		});
	});

	describe("dirty tracking", () => {
		test("getDirtyRows returns dirty rows", () => {
			const buf = new UnifiedBuffer(5, 3, 100);
			buf.clearAllDirty();
			buf.setCell(0, 1, createCell("X"));

			const dirty = buf.getDirtyRows();
			expect(dirty).toContain(1);
			expect(dirty).not.toContain(0);
			expect(dirty).not.toContain(2);
		});

		test("clearAllDirty resets dirty state", () => {
			const buf = new UnifiedBuffer(5, 3, 100);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(0, 1, createCell("B"));

			buf.clearAllDirty();

			expect(buf.getDirtyRows().length).toBe(0);
		});
	});

	// ===== Phase 3: Full-Buffer Reflow with Cursor Tracking =====

	describe("resize (reflow narrowing)", () => {
		test("long line wraps at new width", () => {
			const buf = new UnifiedBuffer(10, 1, 100);
			// Write "ABCDEFGHIJ" in the single viewport line
			for (let i = 0; i < 10; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			const result = buf.resize(5, 1, 0, 0);

			// Should now have 2 physical lines (1 viewport + 1 scrollback or viewport expanded)
			// With 1 viewport row, the first line becomes scrollback
			expect(buf.getLine(0).getCell(0).char).toBe("F");
			expect(buf.getLine(0).getCell(4).char).toBe("J");
			expect(buf.getLine(0).wrapped).toBe(true);
			expect(buf.scrollbackLength).toBe(1);
			expect(buf.getScrollbackLine(0).getCell(0).char).toBe("A");
		});

		test("multiple lines wrap correctly", () => {
			const buf = new UnifiedBuffer(10, 2, 100);
			// Line 0: "ABCDEFGHIJ"
			for (let i = 0; i < 10; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}
			// Line 1: "0123456789"
			for (let i = 0; i < 10; i++) {
				buf.setCell(i, 1, createCell(String(i)));
			}

			buf.resize(5, 2, 1, 0);

			// Each 10-char line becomes 2 lines → 4 total, viewport=2
			expect(buf.scrollbackLength).toBe(2);
			// Viewport shows last 2 lines (second half of second logical line)
			expect(buf.getLine(0).getCell(0).char).toBe("0");
			expect(buf.getLine(1).getCell(0).char).toBe("5");
		});

		test("scrollback lines also reflowed", () => {
			const buf = new UnifiedBuffer(10, 2, 10);
			// Create a scrollback line
			const sbLine = new Line(10);
			for (let i = 0; i < 10; i++) {
				sbLine.setCell(i, createCell(String.fromCharCode(65 + i)));
			}
			buf.push(sbLine); // Goes into scrollback

			buf.resize(5, 2, 0, 0);

			// Before reflow: [empty(10), empty(10), "ABCDEFGHIJ"(10)]
			// After reflow at width 5: [empty(5), empty(5), "ABCDE"(5), "FGHIJ"(5,wrapped)]
			// Total 4 lines, viewport=2, scrollback=2
			expect(buf.scrollbackLength).toBe(2);
			// Scrollback should contain the reflowed content
			expect(buf.getScrollbackLine(0).getCell(0).char).toBe(" ");
			expect(buf.getScrollbackLine(1).getCell(0).char).toBe(" ");
			// Viewport should have the reflowed long line
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(1).getCell(0).char).toBe("F");
		});
	});

	describe("resize (reflow widening)", () => {
		test("wrapped lines merge", () => {
			const buf = new UnifiedBuffer(5, 2, 100);
			// Write "ABCDE" in row 0
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}
			// Write "FGHIJ" in row 1, mark as wrapped continuation
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 1, createCell(String.fromCharCode(70 + i)));
			}
			buf.getLine(1).wrapped = true;

			const result = buf.resize(10, 2, 1, 4);

			// Should merge into single line "ABCDEFGHIJ" → becomes row 0
			// Row 1 is empty (added to fill viewport)
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(0).getCell(9).char).toBe("J");
			expect(buf.getLine(0).wrapped).toBe(false);
			expect(buf.getLine(1).getCell(0).char).toBe(" ");
		});

		test("hard line breaks preserved", () => {
			const buf = new UnifiedBuffer(5, 3, 100);
			// Line 0: "ABC" (not wrapped)
			for (let i = 0; i < 3; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}
			// Line 1: "DEF" (not wrapped - hard line break)
			for (let i = 0; i < 3; i++) {
				buf.setCell(i, 1, createCell(String.fromCharCode(68 + i)));
			}
			// Line 2: empty

			buf.resize(10, 3, 0, 0);

			// Lines should remain separate (not merged) because they're not wrapped
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(1).getCell(0).char).toBe("D");
		});
	});

	describe("resize (cursor tracking)", () => {
		test("cursor tracked through narrowing", () => {
			const buf = new UnifiedBuffer(10, 1, 100);
			for (let i = 0; i < 10; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}
			// Cursor at col 7 (on "H")
			const result = buf.resize(5, 2, 0, 7);

			// "ABCDEFGHIJ" → "ABCDE" + "FGHIJ" at width 5
			// Cursor was at col 7 → logical offset 7 → row 1, col 2
			expect(result.col).toBe(2);
			expect(result.row).toBe(1);
		});

		test("cursor tracked through widening", () => {
			const buf = new UnifiedBuffer(5, 2, 100);
			// Row 0: "ABCDE"
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}
			// Row 1: "FGHIJ" (wrapped continuation)
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 1, createCell(String.fromCharCode(70 + i)));
			}
			buf.getLine(1).wrapped = true;

			// Cursor at row 1, col 2 (on "H")
			const result = buf.resize(10, 2, 1, 2);

			// Merged: "ABCDEFGHIJ" → logical offset = 5 + 2 = 7
			// At width 10: row 0, col 7
			// But viewport adjusts, so cursor row is relative to viewport
			expect(result.col).toBe(7);
		});

		test("cursor at column 0 boundary", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			const result = buf.resize(3, 2, 0, 0);

			// Cursor at col 0 should stay col 0
			expect(result.col).toBe(0);
		});

		test("cursor on last column before wrap", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			for (let i = 0; i < 5; i++) {
				buf.setCell(i, 0, createCell(String.fromCharCode(65 + i)));
			}

			// Cursor at col 4 (last column)
			const result = buf.resize(3, 2, 0, 4);

			// "ABCDE" → "ABC" + "DE" at width 3
			// Cursor at logical offset 4 → row 1, col 1
			expect(result.col).toBe(1);
			expect(result.row).toBe(1);
		});
	});

	describe("resize (empty line trimming)", () => {
		test("trailing empty lines trimmed on shrink", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			// Only write content on row 0-1, rows 2-4 are empty
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(0, 1, createCell("B"));

			// Shrink rows from 5 to 3
			buf.resize(10, 3, 0, 0);

			expect(buf.rows).toBe(3);
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(1).getCell(0).char).toBe("B");
		});

		test("non-empty lines NOT trimmed", () => {
			const buf = new UnifiedBuffer(10, 3, 100);
			for (let row = 0; row < 3; row++) {
				buf.setCell(0, row, createCell(String.fromCharCode(65 + row)));
			}

			// Shrink rows from 3 to 2 (no empty lines to trim, content pushed to scrollback)
			buf.resize(10, 2, 0, 0);

			expect(buf.rows).toBe(2);
			// Content should be preserved
			expect(buf.getLine(0).getCell(0).char).toBe("B");
			expect(buf.getLine(1).getCell(0).char).toBe("C");
			expect(buf.scrollbackLength).toBe(1);
		});
	});

	describe("resize (edge cases)", () => {
		test("resize to 1 column", () => {
			const buf = new UnifiedBuffer(5, 1, 100);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(1, 0, createCell("B"));
			buf.setCell(2, 0, createCell("C"));

			const result = buf.resize(1, 3, 0, 0);

			// "ABC" → 3 lines of 1 char each
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(1).getCell(0).char).toBe("B");
			expect(buf.getLine(2).getCell(0).char).toBe("C");
		});

		test("empty buffer resize", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			const result = buf.resize(20, 10, 0, 0);

			expect(buf.cols).toBe(20);
			expect(buf.rows).toBe(10);
			expect(result.col).toBe(0);
			expect(result.row).toBe(0);
		});

		test("same width resize (cols unchanged) adjusts rows", () => {
			const buf = new UnifiedBuffer(10, 5, 100);
			buf.setCell(0, 0, createCell("A"));

			const result = buf.resize(10, 3, 0, 0);

			expect(buf.cols).toBe(10);
			expect(buf.rows).toBe(3);
			expect(buf.getLine(0).getCell(0).char).toBe("A");
		});

		test("buffer at scrollback capacity after reflow", () => {
			const buf = new UnifiedBuffer(10, 2, 3);
			// capacity = 3 + 2 = 5
			// Fill scrollback
			for (let i = 0; i < 3; i++) {
				const line = new Line(10);
				line.setCell(0, createCell(String.fromCharCode(65 + i)));
				buf.push(line);
			}
			// Write viewport
			buf.setCell(0, 0, createCell("X"));
			buf.setCell(0, 1, createCell("Y"));

			// Narrow: doubles line count → should be capped at capacity
			buf.resize(5, 2, 0, 0);

			expect(buf.size).toBeLessThanOrEqual(buf.rows + 3); // scrollback capped
		});
	});

	describe("resizeNoReflow (alternate buffer)", () => {
		test("lines resized in-place without reflow", () => {
			const buf = new UnifiedBuffer(10, 3, 0);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(5, 1, createCell("B"));

			buf.resizeNoReflow(20, 3);

			expect(buf.cols).toBe(20);
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(0).length).toBe(20);
			expect(buf.getLine(1).getCell(5).char).toBe("B");
		});

		test("row increase adds blank lines at bottom", () => {
			const buf = new UnifiedBuffer(10, 3, 0);
			buf.setCell(0, 0, createCell("A"));

			buf.resizeNoReflow(10, 5);

			expect(buf.rows).toBe(5);
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			expect(buf.getLine(3).getCell(0).char).toBe(" ");
			expect(buf.getLine(4).getCell(0).char).toBe(" ");
		});

		test("row decrease removes from bottom", () => {
			const buf = new UnifiedBuffer(10, 5, 0);
			buf.setCell(0, 0, createCell("A"));
			buf.setCell(0, 4, createCell("E"));

			buf.resizeNoReflow(10, 3);

			expect(buf.rows).toBe(3);
			expect(buf.getLine(0).getCell(0).char).toBe("A");
			// Rows 3,4 should have been removed
		});
	});

	// ===== Phase 5: Performance Validation =====

	describe("performance", () => {
		test("reflow with 10000 scrollback lines completes in under 500ms", () => {
			const cols = 80;
			const rows = 24;
			const scrollbackLines = 10000;
			const buf = new UnifiedBuffer(cols, rows, scrollbackLines);

			// Fill the buffer to capacity with content
			for (let i = 0; i < scrollbackLines + rows; i++) {
				const line = new Line(cols);
				// Write some content to make reflow meaningful
				const text = `Line ${i}: ${"x".repeat(60)}`;
				for (let j = 0; j < Math.min(text.length, cols); j++) {
					line.setCell(j, createCell(text[j]!));
				}
				buf.push(line);
			}

			expect(buf.scrollbackLength).toBe(scrollbackLines);

			// Measure reflow time (narrow)
			const startNarrow = performance.now();
			buf.resize(40, rows, 0, 0);
			const narrowTime = performance.now() - startNarrow;

			// Measure reflow time (widen back)
			const startWiden = performance.now();
			buf.resize(80, rows, 0, 0);
			const widenTime = performance.now() - startWiden;

			// Both should complete well within budget
			expect(narrowTime).toBeLessThan(500);
			expect(widenTime).toBeLessThan(500);
		});

		test("getLine access is O(1)", () => {
			const buf = new UnifiedBuffer(80, 24, 1000);
			for (let i = 0; i < 1024; i++) {
				buf.push(new Line(80));
			}

			const iterations = 100000;
			const start = performance.now();
			for (let i = 0; i < iterations; i++) {
				buf.getLine(i % 24);
			}
			const elapsed = performance.now() - start;

			// 100k accesses should be near instant (well under 50ms)
			expect(elapsed).toBeLessThan(50);
		});
	});
});
