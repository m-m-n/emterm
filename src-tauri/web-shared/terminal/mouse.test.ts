/**
 * Tests for mouse event handling.
 */
import { describe, expect, test } from "bun:test";
import { encodeMouseEvent, type MouseEvent } from "./mouse.ts";

describe("Mouse Event Encoding", () => {
	describe("X10 encoding (default)", () => {
		test("should encode left button press", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			// ESC [ M <button+32> <col+32> <row+32>
			expect(result![0]).toBe(0x1b); // ESC
			expect(result![1]).toBe(0x5b); // [
			expect(result![2]).toBe(0x4d); // M
			expect(result![3]).toBe(0 + 32); // button 0 + 32
			expect(result![4]).toBe(1 + 32); // col 1 + 32
			expect(result![5]).toBe(1 + 32); // row 1 + 32
		});

		test("should encode middle button press", () => {
			const event: MouseEvent = {
				button: "middle",
				col: 10,
				row: 5,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe(1 + 32); // button 1 + 32
			expect(result![4]).toBe(10 + 32); // col 10 + 32
			expect(result![5]).toBe(5 + 32); // row 5 + 32
		});

		test("should encode right button press", () => {
			const event: MouseEvent = {
				button: "right",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe(2 + 32); // button 2 + 32
		});

		test("should not encode release in x10 mode", () => {
			const event: MouseEvent = {
				button: "release",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).toBeNull();
		});

		test("should not encode motion in x10 mode", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
				motion: true,
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).toBeNull();
		});

		test("should encode wheel up", () => {
			const event: MouseEvent = {
				button: "wheelUp",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe(64 + 32); // wheel up is 64
		});

		test("should encode wheel down", () => {
			const event: MouseEvent = {
				button: "wheelDown",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe(65 + 32); // wheel down is 65
		});

		test("should encode shift modifier", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: true, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe((0 | 4) + 32); // button 0 + shift (4) + 32
		});

		test("should encode ctrl modifier", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: true },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe((0 | 16) + 32); // button 0 + ctrl (16) + 32
		});

		test("should encode meta modifier", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: true, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe((0 | 8) + 32); // button 0 + meta (8) + 32
		});

		test("should encode combined modifiers", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: true, meta: true, ctrl: true },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe((0 | 4 | 8 | 16) + 32); // button + all modifiers
		});

		test("should clamp coordinates to max 223", () => {
			const event: MouseEvent = {
				button: "left",
				col: 300,
				row: 400,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "x10", "default");
			expect(result).not.toBeNull();
			expect(result![4]).toBe(223 + 32); // clamped to 223
			expect(result![5]).toBe(223 + 32); // clamped to 223
		});
	});

	describe("SGR encoding (mode 1006)", () => {
		test("should encode left button press", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "button", "sgr");
			expect(result).not.toBeNull();
			const str = new TextDecoder().decode(result!);
			expect(str).toBe("\x1b[<0;1;1M");
		});

		test("should encode right button press", () => {
			const event: MouseEvent = {
				button: "right",
				col: 10,
				row: 5,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "button", "sgr");
			expect(result).not.toBeNull();
			const str = new TextDecoder().decode(result!);
			expect(str).toBe("\x1b[<2;10;5M");
		});

		test("should encode button release with lowercase m", () => {
			const event: MouseEvent = {
				button: "release",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "button", "sgr");
			expect(result).not.toBeNull();
			const str = new TextDecoder().decode(result!);
			expect(str).toBe("\x1b[<0;1;1m"); // lowercase m for release
		});

		test("should encode motion with button 32 added", () => {
			const event: MouseEvent = {
				button: "left",
				col: 5,
				row: 5,
				modifiers: { shift: false, meta: false, ctrl: false },
				motion: true,
			};

			const result = encodeMouseEvent(event, "any", "sgr");
			expect(result).not.toBeNull();
			const str = new TextDecoder().decode(result!);
			expect(str).toBe("\x1b[<32;5;5M"); // 0 + 32 for motion
		});

		test("should handle large coordinates", () => {
			const event: MouseEvent = {
				button: "left",
				col: 300,
				row: 200,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "button", "sgr");
			expect(result).not.toBeNull();
			const str = new TextDecoder().decode(result!);
			expect(str).toBe("\x1b[<0;300;200M"); // No clamping in SGR mode
		});
	});

	describe("UTF-8 encoding (mode 1005)", () => {
		test("should encode simple coordinates", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "button", "utf8");
			expect(result).not.toBeNull();
			expect(result![0]).toBe(0x1b); // ESC
			expect(result![1]).toBe(0x5b); // [
			expect(result![2]).toBe(0x4d); // M
			expect(result![3]).toBe(32); // button 0 + 32
			expect(result![4]).toBe(33); // col 1 + 32
			expect(result![5]).toBe(33); // row 1 + 32
		});

		test("should use UTF-8 for large coordinates", () => {
			const event: MouseEvent = {
				button: "left",
				col: 100,
				row: 100,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "button", "utf8");
			expect(result).not.toBeNull();
			// 100 + 32 = 132, which needs UTF-8 encoding (0xC2 0x84)
			expect(result!.length).toBe(8); // ESC [ M button col(2) row(2)
		});
	});

	describe("Button tracking mode", () => {
		test("should encode release events", () => {
			const event: MouseEvent = {
				button: "release",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "button", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe(3 + 32); // release is button 3
		});

		test("should encode motion events", () => {
			const event: MouseEvent = {
				button: "left",
				col: 5,
				row: 5,
				modifiers: { shift: false, meta: false, ctrl: false },
				motion: true,
			};

			const result = encodeMouseEvent(event, "button", "default");
			expect(result).not.toBeNull();
			expect(result![3]).toBe((0 | 32) + 32); // button 0 + motion (32) + 32
		});
	});

	describe("Any-event tracking mode", () => {
		test("should encode all motion events", () => {
			const event: MouseEvent = {
				button: "left",
				col: 5,
				row: 5,
				modifiers: { shift: false, meta: false, ctrl: false },
				motion: true,
			};

			const result = encodeMouseEvent(event, "any", "default");
			expect(result).not.toBeNull();
		});
	});

	describe("No tracking mode", () => {
		test("should return null when tracking is disabled", () => {
			const event: MouseEvent = {
				button: "left",
				col: 1,
				row: 1,
				modifiers: { shift: false, meta: false, ctrl: false },
			};

			const result = encodeMouseEvent(event, "none", "default");
			expect(result).toBeNull();
		});
	});
});
