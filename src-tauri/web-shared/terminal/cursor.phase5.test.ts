/**
 * Tests for cursor features added in Phase 5.
 */
import { describe, expect, it } from "bun:test";
import { CursorState } from "./cursor.ts";

describe("CursorState Phase 5 features", () => {
	describe("cursor style", () => {
		it("should default to block style", () => {
			const cursor = new CursorState(80, 24);
			expect(cursor.style).toBe("block");
		});

		it("should set style", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyle("underline");
			expect(cursor.style).toBe("underline");

			cursor.setStyle("bar");
			expect(cursor.style).toBe("bar");
		});
	});

	describe("cursor blink", () => {
		it("should default to blink enabled", () => {
			const cursor = new CursorState(80, 24);
			expect(cursor.blink).toBe(true);
		});
	});

	describe("setStyleFromDECSCUSR", () => {
		it("should set blinking block for param 0", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyleFromDECSCUSR(0);
			expect(cursor.style).toBe("block");
			expect(cursor.blink).toBe(true);
		});

		it("should set blinking block for param 1", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyleFromDECSCUSR(1);
			expect(cursor.style).toBe("block");
			expect(cursor.blink).toBe(true);
		});

		it("should set steady block for param 2", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyleFromDECSCUSR(2);
			expect(cursor.style).toBe("block");
			expect(cursor.blink).toBe(false);
		});

		it("should set blinking underline for param 3", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyleFromDECSCUSR(3);
			expect(cursor.style).toBe("underline");
			expect(cursor.blink).toBe(true);
		});

		it("should set steady underline for param 4", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyleFromDECSCUSR(4);
			expect(cursor.style).toBe("underline");
			expect(cursor.blink).toBe(false);
		});

		it("should set blinking bar for param 5", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyleFromDECSCUSR(5);
			expect(cursor.style).toBe("bar");
			expect(cursor.blink).toBe(true);
		});

		it("should set steady bar for param 6", () => {
			const cursor = new CursorState(80, 24);
			cursor.setStyleFromDECSCUSR(6);
			expect(cursor.style).toBe("bar");
			expect(cursor.blink).toBe(false);
		});
	});

	describe("clone", () => {
		it("should create independent copy", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 10;
			cursor.row = 5;
			cursor.style = "underline";
			cursor.blink = false;

			const cloned = cursor.clone();

			expect(cloned.col).toBe(10);
			expect(cloned.row).toBe(5);
			expect(cloned.style).toBe("underline");
			expect(cloned.blink).toBe(false);

			// Modify original
			cursor.col = 20;
			cursor.style = "bar";

			// Clone should be unaffected
			expect(cloned.col).toBe(10);
			expect(cloned.style).toBe("underline");
		});

		it("should clone saved cursor state", () => {
			const cursor = new CursorState(80, 24);
			cursor.col = 10;
			cursor.row = 5;
			cursor.save();

			cursor.col = 20;
			cursor.row = 10;

			const cloned = cursor.clone();

			// Restore on clone
			cloned.restore();

			expect(cloned.col).toBe(10);
			expect(cloned.row).toBe(5);
		});
	});

	describe("restoreFrom", () => {
		it("should restore state from another cursor", () => {
			const cursor1 = new CursorState(80, 24);
			cursor1.col = 15;
			cursor1.row = 7;
			cursor1.style = "bar";
			cursor1.blink = false;

			const cursor2 = new CursorState(80, 24);
			cursor2.restoreFrom(cursor1);

			expect(cursor2.col).toBe(15);
			expect(cursor2.row).toBe(7);
			expect(cursor2.style).toBe("bar");
			expect(cursor2.blink).toBe(false);
		});

		it("should clamp position to terminal bounds", () => {
			const cursor1 = new CursorState(80, 24);
			cursor1.col = 100; // Out of bounds for smaller terminal
			cursor1.row = 30;

			const cursor2 = new CursorState(40, 10);
			cursor2.restoreFrom(cursor1);

			// Should be clamped to terminal bounds
			expect(cursor2.col).toBe(39); // 40 - 1
			expect(cursor2.row).toBe(9); // 10 - 1
		});
	});

	describe("reset", () => {
		it("should reset style and blink to defaults", () => {
			const cursor = new CursorState(80, 24);
			cursor.style = "bar";
			cursor.blink = false;
			cursor.col = 10;
			cursor.row = 5;

			cursor.reset();

			expect(cursor.style).toBe("block");
			expect(cursor.blink).toBe(true);
			expect(cursor.col).toBe(0);
			expect(cursor.row).toBe(0);
		});
	});
});
