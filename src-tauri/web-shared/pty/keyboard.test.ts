/**
 * Tests for keyboard input handler.
 */

import { describe, expect, it } from "bun:test";
import { keyEventToBytes, shouldHandleKey, calcModifierParam, type KeyboardOptions } from "./keyboard";

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
			// Ctrl+Arrow skips DECCKM and uses modified key handler
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x35, 0x41])); // ESC [1;5A
		});

		it("should skip application mode for arrow keys with Alt modifier", () => {
			const event = createKeyEvent("ArrowUp", { altKey: true });
			const result = keyEventToBytes(event, "application");
			// Alt+Arrow skips DECCKM and uses modified key handler
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x33, 0x41])); // ESC [1;3A
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

		it("should convert Ctrl+[ to ESC (0x1b)", () => {
			// Browser reports Ctrl+[ as key="Escape" with ctrlKey=true
			const event = createKeyEvent("Escape", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b]));
		});

		it("should convert Shift+Enter to CR (0x0d) by default (setting OFF)", () => {
			const event = createKeyEvent("Enter", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x0d]));
		});

		it("should convert Alt+Enter to ESC + CR (0x1b, 0x0d)", () => {
			const event = createKeyEvent("Enter", { altKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x0d]));
		});

		it("should convert Shift+Backspace to DEL (0x7f), same as Backspace", () => {
			const event = createKeyEvent("Backspace", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x7f]));
		});

		it("should convert Shift+Escape to ESC (0x1b), same as Escape", () => {
			const event = createKeyEvent("Escape", { shiftKey: true });
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

	describe("Ctrl+symbol keys", () => {
		it("should convert Ctrl+[ with key='[' (WebKitGTK) to ESC (0x1B)", () => {
			// WebKitGTK reports Ctrl+[ as key="[" with ctrlKey=true
			const event = createKeyEvent("[", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b]));
		});

		it("should convert Ctrl+[ with key='Escape' (Chromium) to ESC (0x1B)", () => {
			// Chromium reports Ctrl+[ as key="Escape" with ctrlKey=true
			const event = createKeyEvent("Escape", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b]));
		});

		it("should convert Ctrl+] to GS (0x1D)", () => {
			const event = createKeyEvent("]", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1d]));
		});

		it("should convert Ctrl+\\ to FS (0x1C)", () => {
			const event = createKeyEvent("\\", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1c]));
		});

		it("should convert Ctrl+^ to RS (0x1E)", () => {
			const event = createKeyEvent("^", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1e]));
		});

		it("should convert Ctrl+_ to US (0x1F)", () => {
			const event = createKeyEvent("_", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1f]));
		});

		it("should convert Ctrl+@ to NUL (0x00)", () => {
			const event = createKeyEvent("@", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x00]));
		});

		it("should convert Ctrl+Space to NUL (0x00)", () => {
			const event = createKeyEvent(" ", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x00]));
		});
	});

	describe("Shift+Tab (back-tab)", () => {
		it("should convert Shift+Tab to ESC [ Z", () => {
			const event = createKeyEvent("Tab", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x5a])); // ESC [ Z
		});
	});

	describe("modifier parameter calculator", () => {
		it("should return 0 for no modifiers", () => {
			expect(calcModifierParam(false, false, false)).toBe(0);
		});

		it("should return 2 for Shift", () => {
			expect(calcModifierParam(true, false, false)).toBe(2);
		});

		it("should return 3 for Alt", () => {
			expect(calcModifierParam(false, true, false)).toBe(3);
		});

		it("should return 5 for Ctrl", () => {
			expect(calcModifierParam(false, false, true)).toBe(5);
		});

		it("should return 6 for Ctrl+Shift", () => {
			expect(calcModifierParam(true, false, true)).toBe(6);
		});

		it("should return 4 for Shift+Alt", () => {
			expect(calcModifierParam(true, true, false)).toBe(4);
		});

		it("should return 7 for Ctrl+Alt", () => {
			expect(calcModifierParam(false, true, true)).toBe(7);
		});

		it("should return 8 for Ctrl+Alt+Shift", () => {
			expect(calcModifierParam(true, true, true)).toBe(8);
		});
	});

	describe("modified arrow keys", () => {
		it("should convert Ctrl+ArrowUp to ESC [1;5A", () => {
			const event = createKeyEvent("ArrowUp", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x35, 0x41]));
		});

		it("should convert Ctrl+ArrowRight to ESC [1;5C", () => {
			const event = createKeyEvent("ArrowRight", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x35, 0x43]));
		});

		it("should convert Shift+ArrowUp to ESC [1;2A", () => {
			const event = createKeyEvent("ArrowUp", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x32, 0x41]));
		});

		it("should convert Alt+ArrowUp to ESC [1;3A", () => {
			const event = createKeyEvent("ArrowUp", { altKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x33, 0x41]));
		});

		it("should convert Ctrl+Shift+ArrowRight to ESC [1;6C", () => {
			const event = createKeyEvent("ArrowRight", { ctrlKey: true, shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x36, 0x43]));
		});
	});

	describe("modified Home/End", () => {
		it("should convert Ctrl+Home to ESC [1;5H", () => {
			const event = createKeyEvent("Home", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x35, 0x48]));
		});

		it("should convert Ctrl+End to ESC [1;5F", () => {
			const event = createKeyEvent("End", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x35, 0x46]));
		});

		it("should convert Shift+Home to ESC [1;2H", () => {
			const event = createKeyEvent("Home", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x32, 0x48]));
		});
	});

	describe("modified Delete/Insert/PageUp/PageDown", () => {
		it("should convert Ctrl+Delete to ESC [3;5~", () => {
			const event = createKeyEvent("Delete", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x33, 0x3b, 0x35, 0x7e]));
		});

		it("should convert Ctrl+PageUp to ESC [5;5~", () => {
			const event = createKeyEvent("PageUp", { ctrlKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x35, 0x3b, 0x35, 0x7e]));
		});

		it("should convert Shift+Insert to ESC [2;2~", () => {
			const event = createKeyEvent("Insert", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x32, 0x3b, 0x32, 0x7e]));
		});
	});

	describe("modified F1-F4", () => {
		it("should convert Shift+F1 to ESC [1;2P", () => {
			const event = createKeyEvent("F1", { shiftKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x3b, 0x32, 0x50]));
		});
	});

	describe("modified F5-F12", () => {
		it("should convert Ctrl+F5 to ESC [15;5~", () => {
			const event = createKeyEvent("F5", { ctrlKey: true });
			const result = keyEventToBytes(event);
			// ESC [ 1 5 ; 5 ~
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x31, 0x35, 0x3b, 0x35, 0x7e]));
		});

		it("should convert Ctrl+F12 to ESC [24;5~", () => {
			const event = createKeyEvent("F12", { ctrlKey: true });
			const result = keyEventToBytes(event);
			// ESC [ 2 4 ; 5 ~
			expect(result).toEqual(new Uint8Array([0x1b, 0x5b, 0x32, 0x34, 0x3b, 0x35, 0x7e]));
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

	describe("Ctrl+Alt combinations", () => {
		it("should convert Ctrl+Alt+C to ESC 0x03", () => {
			const event = createKeyEvent("c", { ctrlKey: true, altKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x03]));
		});

		it("should convert Ctrl+Alt+A to ESC 0x01", () => {
			const event = createKeyEvent("a", { ctrlKey: true, altKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x01]));
		});

		it("should not affect plain Alt+letter", () => {
			const event = createKeyEvent("x", { altKey: true });
			const result = keyEventToBytes(event);
			expect(result).toEqual(new Uint8Array([0x1b, 0x78])); // ESC + 'x'
		});
	});

	describe("shiftEnterAsAltEnter option", () => {
		it("should convert Shift+Enter to ESC + CR when option is enabled", () => {
			const event = createKeyEvent("Enter", { shiftKey: true });
			const result = keyEventToBytes(event, { shiftEnterAsAltEnter: true });
			expect(result).toEqual(new Uint8Array([0x1b, 0x0d]));
		});

		it("should convert Shift+Enter to CR when option is disabled", () => {
			const event = createKeyEvent("Enter", { shiftKey: true });
			const result = keyEventToBytes(event, { shiftEnterAsAltEnter: false });
			expect(result).toEqual(new Uint8Array([0x0d]));
		});

		it("should not affect Ctrl+Shift+Enter", () => {
			const event = createKeyEvent("Enter", { shiftKey: true, ctrlKey: true });
			const result = keyEventToBytes(event, { shiftEnterAsAltEnter: true });
			// Should NOT trigger the remapping (ctrlKey is true)
			expect(result).not.toEqual(new Uint8Array([0x1b, 0x0d]));
		});

		it("should not affect plain Enter when option is enabled", () => {
			const event = createKeyEvent("Enter");
			const result = keyEventToBytes(event, { shiftEnterAsAltEnter: true });
			expect(result).toEqual(new Uint8Array([0x0d]));
		});

		it("should accept KeyboardOptions object with cursorKeysMode", () => {
			const event = createKeyEvent("ArrowUp");
			const result = keyEventToBytes(event, { cursorKeysMode: "application" });
			expect(result).toEqual(new Uint8Array([0x1b, 0x4f, 0x41])); // ESC O A
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
