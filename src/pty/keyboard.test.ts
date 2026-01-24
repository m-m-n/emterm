/**
 * Tests for keyboard input handler.
 */

import { describe, expect, it } from "bun:test";
import { keyEventToBytes, shouldHandleKey } from "./keyboard";

/**
 * Helper to create a mock KeyboardEvent.
 */
function createKeyEvent(
	key: string,
	options: {
		ctrlKey?: boolean;
		altKey?: boolean;
		shiftKey?: boolean;
	} = {},
): KeyboardEvent {
	return new KeyboardEvent("keydown", {
		key,
		ctrlKey: options.ctrlKey ?? false,
		altKey: options.altKey ?? false,
		shiftKey: options.shiftKey ?? false,
	});
}

describe("keyEventToBytes", () => {
	describe("regular characters", () => {
		it("should encode regular characters as UTF-8", () => {
			const event = createKeyEvent("a");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x61])); // 'a' = 0x61
		});

		it("should encode uppercase characters correctly", () => {
			const event = createKeyEvent("A", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x41])); // 'A' = 0x41
		});

		it("should encode numbers correctly", () => {
			const event = createKeyEvent("5");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x35])); // '5' = 0x35
		});

		it("should encode special characters correctly", () => {
			const event = createKeyEvent("@");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x40])); // '@' = 0x40
		});
	});

	describe("control characters", () => {
		it("should convert Ctrl+C to ETX (0x03)", () => {
			const event = createKeyEvent("c", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x03]));
		});

		it("should convert Ctrl+D to EOT (0x04)", () => {
			const event = createKeyEvent("d", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x04]));
		});

		it("should convert Ctrl+Z to SUB (0x1a)", () => {
			const event = createKeyEvent("z", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1a]));
		});

		it("should convert Ctrl+L to FF (0x0c)", () => {
			const event = createKeyEvent("l", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x0c]));
		});

		it("should convert any Ctrl+letter to control character", () => {
			const event = createKeyEvent("g", { ctrlKey: true });
			const result = keyEventToBytes(event);
			// Ctrl+G = 0x07 (BEL)
			expect(result).toEqual(new Uint8Array([0x07]));
		});
	});

	describe("arrow keys (normal mode)", () => {
		it("should convert ArrowUp to ESC [ A", () => {
			const event = createKeyEvent("ArrowUp");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x41]));
		});

		it("should convert ArrowDown to ESC [ B", () => {
			const event = createKeyEvent("ArrowDown");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x42]));
		});

		it("should convert ArrowRight to ESC [ C", () => {
			const event = createKeyEvent("ArrowRight");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x43]));
		});

		it("should convert ArrowLeft to ESC [ D", () => {
			const event = createKeyEvent("ArrowLeft");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x44]));
		});

		it("should use normal mode by default", () => {
			const event = createKeyEvent("ArrowUp");
			// Without second argument, should default to normal mode
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x41])); // ESC [ A
		});
	});

	describe("arrow keys (application mode - DECCKM)", () => {
		it("should convert ArrowUp to ESC O A in application mode", () => {
			const event = createKeyEvent("ArrowUp");
			const result = keyEventToBytes(event, "application");
			expect(result).toEqual(new Uint8Array([0x1b, 0x4f, 0x41])); // ESC O A
		});

		it("should convert ArrowDown to ESC O B in application mode", () => {
			const event = createKeyEvent("ArrowDown");
			const result = keyEventToBytes(event, "application");
			expect(result).toEqual(new Uint8Array([0x1b, 0x4f, 0x42])); // ESC O B
		});

		it("should convert ArrowRight to ESC O C in application mode", () => {
			const event = createKeyEvent("ArrowRight");
			const result = keyEventToBytes(event, "application");
			expect(result).toEqual(new Uint8Array([0x1b, 0x4f, 0x43])); // ESC O C
		});

		it("should convert ArrowLeft to ESC O D in application mode", () => {
			const event = createKeyEvent("ArrowLeft");
			const result = keyEventToBytes(event, "application");
			expect(result).toEqual(new Uint8Array([0x1b, 0x4f, 0x44])); // ESC O D
		});

		it("should skip application mode for arrow keys with Ctrl modifier", () => {
			const event = createKeyEvent("ArrowUp", { ctrlKey: true });
			const result = keyEventToBytes(event, "application");
			// Ctrl+Arrow is not in SPECIAL_KEYS (no Ctrl variant defined)
			// Falls through and returns null as unhandled
			expect(result).toBeNull();
		});

		it("should skip application mode for arrow keys with Alt modifier", () => {
			const event = createKeyEvent("ArrowUp", { altKey: true });
			const result = keyEventToBytes(event, "application");
			// Alt+Arrow is not in SPECIAL_KEYS (no Alt variant defined)
			// Falls through and returns null as unhandled
			expect(result).toBeNull();
		});

		it("should use normal sequence for non-arrow keys in application mode", () => {
			const event = createKeyEvent("Home");
			const result = keyEventToBytes(event, "application");
			// Home key is not affected by cursor keys mode
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x48])); // ESC [ H
		});
	});

	describe("navigation keys", () => {
		it("should convert Home to ESC [ H", () => {
			const event = createKeyEvent("Home");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x48]));
		});

		it("should convert End to ESC [ F", () => {
			const event = createKeyEvent("End");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x46]));
		});

		it("should convert PageUp to ESC [ 5 ~", () => {
			const event = createKeyEvent("PageUp");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x35, 0x7e]));
		});

		it("should convert PageDown to ESC [ 6 ~", () => {
			const event = createKeyEvent("PageDown");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x36, 0x7e]));
		});

		it("should convert Delete to ESC [ 3 ~", () => {
			const event = createKeyEvent("Delete");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x33, 0x7e]));
		});
	});

	describe("special keys", () => {
		it("should convert Enter to CR (0x0d)", () => {
			const event = createKeyEvent("Enter");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x0d]));
		});

		it("should convert Tab to HT (0x09)", () => {
			const event = createKeyEvent("Tab");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x09]));
		});

		it("should convert Backspace to DEL (0x7f)", () => {
			const event = createKeyEvent("Backspace");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x7f]));
		});

		it("should convert Escape to ESC (0x1b)", () => {
			const event = createKeyEvent("Escape");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b]));
		});
	});

	describe("function keys", () => {
		it("should convert F1 to ESC O P", () => {
			const event = createKeyEvent("F1");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x4f, 0x50]));
		});

		it("should convert F5 to ESC [ 15 ~", () => {
			const event = createKeyEvent("F5");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x35, 0x7e]));
		});

		it("should convert F12 to ESC [ 24 ~", () => {
			const event = createKeyEvent("F12");
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x32, 0x34, 0x7e]));
		});
	});

	describe("Alt combinations", () => {
		it("should add ESC prefix for Alt+letter", () => {
			const event = createKeyEvent("x", { altKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x78])); // ESC + 'x'
		});

		it("should add ESC prefix for Alt+number", () => {
			const event = createKeyEvent("1", { altKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x31])); // ESC + '1'
		});
	});

	describe("ignored keys", () => {
		it("should return null for modifier keys alone", () => {
			expect(keyEventToBytes(createKeyEvent("Control"))).toBeNull();
			expect(keyEventToBytes(createKeyEvent("Alt"))).toBeNull();
			expect(keyEventToBytes(createKeyEvent("Shift"))).toBeNull();
			expect(keyEventToBytes(createKeyEvent("Meta"))).toBeNull();
		});
	});
});

describe("shouldHandleKey", () => {
	it("should return false for modifier keys", () => {
		expect(shouldHandleKey(createKeyEvent("Control"))).toBe(false);
		expect(shouldHandleKey(createKeyEvent("Alt"))).toBe(false);
		expect(shouldHandleKey(createKeyEvent("Shift"))).toBe(false);
		expect(shouldHandleKey(createKeyEvent("Meta"))).toBe(false);
	});

	it("should return true for regular characters", () => {
		expect(shouldHandleKey(createKeyEvent("a"))).toBe(true);
		expect(shouldHandleKey(createKeyEvent("1"))).toBe(true);
	});

	it("should return true for special keys", () => {
		expect(shouldHandleKey(createKeyEvent("Enter"))).toBe(true);
		expect(shouldHandleKey(createKeyEvent("ArrowUp"))).toBe(true);
		expect(shouldHandleKey(createKeyEvent("F1"))).toBe(true);
	});
});
