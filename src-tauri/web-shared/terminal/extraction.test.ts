import { beforeEach, describe, expect, test } from "bun:test";
import { TerminalState } from "./state";

describe("TerminalState - Text Extraction", () => {
	let state: TerminalState;

	beforeEach(() => {
		// Create a 20-column, 10-row terminal
		state = new TerminalState(20, 10);
	});

	describe("extractText", () => {
		test("extracts single character", () => {
			// Write "A" at (0,0)
			state.processAction({ type: "Print", value: "A" });
			const text = state.extractText(0, 0, 0, 0);
			expect(text).toBe("A");
		});

		test("extracts single line text", () => {
			// Write "Hello"
			for (const char of "Hello") {
				state.processAction({ type: "Print", value: char });
			}
			const text = state.extractText(0, 0, 4, 0);
			expect(text).toBe("Hello");
		});

		test("extracts partial line", () => {
			// Write "Hello, World!" and extract "World"
			for (const char of "Hello, World!") {
				state.processAction({ type: "Print", value: char });
			}
			const text = state.extractText(7, 0, 11, 0);
			expect(text).toBe("World");
		});

		test("extracts multiple lines", () => {
			// Write "Line1" on row 0
			for (const char of "Line1") {
				state.processAction({ type: "Print", value: char });
			}
			// Move to row 1
			state.processAction({ type: "Execute", value: 10 }); // LF
			state.processAction({ type: "Execute", value: 13 }); // CR
			// Write "Line2"
			for (const char of "Line2") {
				state.processAction({ type: "Print", value: char });
			}

			const text = state.extractText(0, 0, 4, 1);
			expect(text).toBe("Line1\nLine2");
		});

		test("extracts text with trailing spaces removed", () => {
			// Write "Hi" (rest of line is spaces)
			for (const char of "Hi") {
				state.processAction({ type: "Print", value: char });
			}
			const text = state.extractText(0, 0, 19, 0);
			// Should extract "Hi" without trailing spaces
			expect(text).toBe("Hi");
		});

		test("extracts empty cells as empty string", () => {
			// Don't write anything, just extract empty line
			const text = state.extractText(0, 0, 4, 0);
			expect(text).toBe("");
		});

		test("extracts middle portion of line", () => {
			// Write "ABCDEFGHIJ"
			for (const char of "ABCDEFGHIJ") {
				state.processAction({ type: "Print", value: char });
			}
			const text = state.extractText(2, 0, 5, 0);
			expect(text).toBe("CDEF");
		});

		test("handles multi-line selection from middle to middle", () => {
			// Write "Hello" on row 0
			for (const char of "Hello") {
				state.processAction({ type: "Print", value: char });
			}
			state.processAction({ type: "Execute", value: 10 });
			state.processAction({ type: "Execute", value: 13 });
			// Write "World" on row 1
			for (const char of "World") {
				state.processAction({ type: "Print", value: char });
			}
			state.processAction({ type: "Execute", value: 10 });
			state.processAction({ type: "Execute", value: 13 });
			// Write "Test" on row 2
			for (const char of "Test") {
				state.processAction({ type: "Print", value: char });
			}

			// Extract from "lo" (row 0, col 3-4) to "Wo" (row 1, col 0-1)
			const text = state.extractText(3, 0, 1, 1);
			expect(text).toBe("lo\nWo");
		});

		test("extracts three complete lines", () => {
			// Write three lines
			for (const char of "AAA") {
				state.processAction({ type: "Print", value: char });
			}
			state.processAction({ type: "Execute", value: 10 });
			state.processAction({ type: "Execute", value: 13 });
			for (const char of "BBB") {
				state.processAction({ type: "Print", value: char });
			}
			state.processAction({ type: "Execute", value: 10 });
			state.processAction({ type: "Execute", value: 13 });
			for (const char of "CCC") {
				state.processAction({ type: "Print", value: char });
			}

			const text = state.extractText(0, 0, 2, 2);
			expect(text).toBe("AAA\nBBB\nCCC");
		});

		test("handles Unicode characters", () => {
			// Write Japanese text
			for (const char of "日本語") {
				state.processAction({ type: "Print", value: char });
			}
			const text = state.extractText(0, 0, 5, 0);
			expect(text).toContain("日本語");
		});

		test("handles single-cell selection", () => {
			state.processAction({ type: "Print", value: "X" });
			const text = state.extractText(0, 0, 0, 0);
			expect(text).toBe("X");
		});

		test("handles reversed coordinates (auto-normalizes)", () => {
			// Write "Hello"
			for (const char of "Hello") {
				state.processAction({ type: "Print", value: char });
			}
			// Extract with reversed coords (should normalize)
			const text = state.extractText(4, 0, 0, 0);
			expect(text).toBe("Hello");
		});

		test("handles empty selection on empty terminal", () => {
			const text = state.extractText(0, 0, 0, 0);
			expect(text).toBe("");
		});
	});
});
