/**
 * Tests for cursor state management.
 */
import { describe, expect, test } from "bun:test";
import { createDefaultAttributes } from "./attributes.ts";
import { CursorState } from "./cursor.ts";

describe("CursorState", () => {
	describe("constructor", () => {
		test("initializes at position (0, 0)", () => {
			const cursor = new CursorState(80, 24);
			expect(cursor.col).toBe(0);
			expect(cursor.row).toBe(0);
		});

		test("stores dimensions", () => {
			const cursor = new CursorState(80, 24);
			expect(cursor.cols).toBe(80);
			expect(cursor.rows).toBe(24);
		});

		test("initializes with default attributes", () => {
			const cursor = new CursorState(80, 24);
			const defaultAttrs = createDefaultAttributes();
			expect(cursor.attrs.bold).toBe(defaultAttrs.bold);
			expect(cursor.attrs.fg).toBeNull();
		});

		test("starts visible", () => {
			const cursor = new CursorState(80, 24);
			expect(cursor.visible).toBe(true);
		});
	});

	describe("moveRight", () => {
		test("moves cursor right by one", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveRight();
			expect(cursor.col).toBe(1);
		});

		test("moves cursor right by specified amount", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveRight(5);
			expect(cursor.col).toBe(5);
		});

		test("stops at right boundary without wrap", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 79;
			cursor.moveRight();
			expect(cursor.col).toBe(79);
			expect(cursor.row).toBe(0);
		});
	});

	describe("moveLeft", () => {
		test("moves cursor left by one", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 5;
			cursor.moveLeft();
			expect(cursor.col).toBe(4);
		});

		test("moves cursor left by specified amount", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 10;
			cursor.moveLeft(5);
			expect(cursor.col).toBe(5);
		});

		test("stops at left boundary", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveLeft();
			expect(cursor.col).toBe(0);
		});

		test("clamps to zero", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 3;
			cursor.moveLeft(10);
			expect(cursor.col).toBe(0);
		});
	});

	describe("moveDown", () => {
		test("moves cursor down by one", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveDown();
			expect(cursor.row).toBe(1);
		});

		test("moves cursor down by specified amount", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveDown(5);
			expect(cursor.row).toBe(5);
		});

		test("stops at bottom boundary", () => {
			const cursor = new CursorState(80, 24);
			cursor.row = 23;
			cursor.moveDown();
			expect(cursor.row).toBe(23);
		});
	});

	describe("moveUp", () => {
		test("moves cursor up by one", () => {
			const cursor = new CursorState(80, 24);
			cursor.row = 5;
			cursor.moveUp();
			expect(cursor.row).toBe(4);
		});

		test("moves cursor up by specified amount", () => {
			const cursor = new CursorState(80, 24);
			cursor.row = 10;
			cursor.moveUp(5);
			expect(cursor.row).toBe(5);
		});

		test("stops at top boundary", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveUp();
			expect(cursor.row).toBe(0);
		});
	});

	describe("moveTo", () => {
		test("moves cursor to absolute position", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveTo(10, 5);
			expect(cursor.col).toBe(10);
			expect(cursor.row).toBe(5);
		});

		test("clamps to valid range", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveTo(100, 50);
			expect(cursor.col).toBe(79);
			expect(cursor.row).toBe(23);
		});

		test("clamps negative values to zero", () => {
			const cursor = new CursorState(80, 24);
			cursor.moveTo(-5, -10);
			expect(cursor.col).toBe(0);
			expect(cursor.row).toBe(0);
		});
	});

	describe("carriageReturn", () => {
		test("moves cursor to column 0", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 50;
			cursor.carriageReturn();
			expect(cursor.col).toBe(0);
			expect(cursor.row).toBe(0);
		});
	});

	describe("lineFeed", () => {
		test("moves cursor down one row", () => {
			const cursor = new CursorState(80, 24);
			cursor.lineFeed();
			expect(cursor.row).toBe(1);
		});

		test("returns true when at bottom (needs scroll)", () => {
			const cursor = new CursorState(80, 24);
			cursor.row = 23;
			const needsScroll = cursor.lineFeed();
			expect(needsScroll).toBe(true);
			expect(cursor.row).toBe(23);
		});

		test("returns false when not at bottom", () => {
			const cursor = new CursorState(80, 24);
			cursor.row = 10;
			const needsScroll = cursor.lineFeed();
			expect(needsScroll).toBe(false);
			expect(cursor.row).toBe(11);
		});
	});

	describe("tab", () => {
		test("moves to next tab stop (8 columns)", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 0;
			cursor.tab();
			expect(cursor.col).toBe(8);
		});

		test("moves from column 3 to column 8", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 3;
			cursor.tab();
			expect(cursor.col).toBe(8);
		});

		test("moves from column 8 to column 16", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 8;
			cursor.tab();
			expect(cursor.col).toBe(16);
		});

		test("stops at right boundary", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 75;
			cursor.tab();
			expect(cursor.col).toBe(79);
		});
	});

	describe("backspace", () => {
		test("moves cursor left by one", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 5;
			cursor.backspace();
			expect(cursor.col).toBe(4);
		});

		test("stops at left boundary", () => {
			const cursor = new CursorState(80, 24);
			cursor.backspace();
			expect(cursor.col).toBe(0);
		});
	});

	describe("resize", () => {
		test("updates dimensions", () => {
			const cursor = new CursorState(80, 24);
			cursor.resize(120, 40);
			expect(cursor.cols).toBe(120);
			expect(cursor.rows).toBe(40);
		});

		test("clamps cursor position to new dimensions", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 70;
			cursor.row = 20;
			cursor.resize(60, 15);
			expect(cursor.col).toBe(59);
			expect(cursor.row).toBe(14);
		});
	});

	describe("save and restore", () => {
		test("saves and restores position", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 10;
			cursor.row = 5;
			cursor.save();

			cursor.col = 50;
			cursor.row = 20;
			cursor.restore();

			expect(cursor.col).toBe(10);
			expect(cursor.row).toBe(5);
		});

		test("saves and restores attributes", () => {
			const cursor = new CursorState(80, 24);
			cursor.attrs.bold = true;
			cursor.attrs.fg = { type: "indexed", index: 1 };
			cursor.save();

			cursor.attrs.bold = false;
			cursor.attrs.fg = null;
			cursor.restore();

			expect(cursor.attrs.bold).toBe(true);
			expect(cursor.attrs.fg).toEqual({ type: "indexed", index: 1 });
		});

		test("restore without save uses defaults", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 10;
			cursor.row = 5;
			cursor.restore();
			expect(cursor.col).toBe(0);
			expect(cursor.row).toBe(0);
		});
	});

	// Phase 4: New absolute positioning tests
	describe("setColumn", () => {
		test("sets cursor to absolute column", () => {
			const cursor = new CursorState(80, 24);
			cursor.setColumn(15);
			expect(cursor.col).toBe(15);
		});

		test("clamps to valid range", () => {
			const cursor = new CursorState(80, 24);
			cursor.setColumn(100);
			expect(cursor.col).toBe(79);
		});

		test("clamps negative to zero", () => {
			const cursor = new CursorState(80, 24);
			cursor.setColumn(-5);
			expect(cursor.col).toBe(0);
		});
	});

	describe("setRow", () => {
		test("sets cursor to absolute row", () => {
			const cursor = new CursorState(80, 24);
			cursor.setRow(10);
			expect(cursor.row).toBe(10);
		});

		test("clamps to valid range", () => {
			const cursor = new CursorState(80, 24);
			cursor.setRow(50);
			expect(cursor.row).toBe(23);
		});

		test("clamps negative to zero", () => {
			const cursor = new CursorState(80, 24);
			cursor.setRow(-5);
			expect(cursor.row).toBe(0);
		});
	});
});
