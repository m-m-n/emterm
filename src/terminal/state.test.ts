/**
 * Tests for TerminalState.
 */
import { describe, expect, test } from "bun:test";
import type { TerminalAction } from "../types/terminal.ts";
import { C0 } from "../types/terminal.ts";
import { TerminalState } from "./state.ts";

describe("TerminalState", () => {
	describe("constructor", () => {
		test("initializes with correct dimensions", () => {
			const state = new TerminalState(80, 24);
			expect(state.cols).toBe(80);
			expect(state.rows).toBe(24);
		});

		test("initializes cursor at (0, 0)", () => {
			const state = new TerminalState(80, 24);
			const buffer = state.getActiveBuffer();
			expect(state.cursorCol).toBe(0);
			expect(state.cursorRow).toBe(0);
		});
	});

	describe("processAction - Print", () => {
		test("prints character at cursor position", () => {
			const state = new TerminalState(80, 24);
			const action: TerminalAction = { type: "Print", value: "A" };
			state.processAction(action);

			const cell = state.getActiveBuffer().getCell(0, 0);
			expect(cell.char).toBe("A");
		});

		test("advances cursor after print", () => {
			const state = new TerminalState(80, 24);
			state.processAction({ type: "Print", value: "A" });

			expect(state.cursorCol).toBe(1);
			expect(state.cursorRow).toBe(0);
		});

		test("advances cursor by 2 for wide characters", () => {
			const state = new TerminalState(80, 24);
			state.processAction({ type: "Print", value: "\u4e00" }); // CJK

			expect(state.cursorCol).toBe(2);
		});

		test("wraps to next line at column boundary", () => {
			const state = new TerminalState(10, 5);
			for (let i = 0; i < 10; i++) {
				state.processAction({ type: "Print", value: "X" });
			}
			// Cursor should be at end of line
			expect(state.cursorCol).toBe(9);
			expect(state.cursorRow).toBe(0);

			// Next character should wrap
			state.processAction({ type: "Print", value: "Y" });
			expect(state.cursorCol).toBe(1);
			expect(state.cursorRow).toBe(1);
		});

		test("scrolls when printing at bottom right corner", () => {
			const state = new TerminalState(5, 3);
			// Fill first two lines
			for (let i = 0; i < 5; i++) {
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}
			for (let i = 0; i < 5; i++) {
				state.processAction({
					type: "Print",
					value: String.fromCharCode(70 + i),
				});
			}
			// Now on row 2
			for (let i = 0; i < 5; i++) {
				state.processAction({
					type: "Print",
					value: String.fromCharCode(75 + i),
				});
			}

			// At end of last row, next print should wrap and scroll
			state.processAction({ type: "Print", value: "Z" });

			// First line should now contain what was the second line
			const firstLine = state.getActiveBuffer().getLine(0).getText();
			expect(firstLine.startsWith("FGHIJ")).toBe(true);
		});
	});

	describe("processAction - Execute (C0 controls)", () => {
		test("handles CR (carriage return)", () => {
			const state = new TerminalState(80, 24);
			// Print one character at a time (matching Rust parser behavior)
			for (const char of "Hello") {
				state.processAction({ type: "Print", value: char });
			}
			expect(state.cursorCol).toBe(5);

			state.processAction({ type: "Execute", value: C0.CR });
			expect(state.cursorCol).toBe(0);
			expect(state.cursorRow).toBe(0);
		});

		test("handles LF (line feed)", () => {
			const state = new TerminalState(80, 24);
			state.processAction({ type: "Execute", value: C0.LF });

			expect(state.cursorCol).toBe(0);
			expect(state.cursorRow).toBe(1);
		});

		test("handles LF at bottom of screen (scroll)", () => {
			const state = new TerminalState(10, 3);
			state.processAction({ type: "Print", value: "A" });

			// Move to last row
			state.processAction({ type: "Execute", value: C0.LF });
			state.processAction({ type: "Execute", value: C0.LF });
			expect(state.cursorRow).toBe(2);

			// LF at bottom should scroll
			state.processAction({ type: "Execute", value: C0.LF });
			expect(state.cursorRow).toBe(2);

			// First line should be empty (scrolled out)
			const firstLine = state.getActiveBuffer().getLine(0);
			// Line 0 now contains what was line 1
		});

		test("handles BS (backspace)", () => {
			const state = new TerminalState(80, 24);
			for (const char of "Hello") {
				state.processAction({ type: "Print", value: char });
			}
			state.processAction({ type: "Execute", value: C0.BS });

			expect(state.cursorCol).toBe(4);
		});

		test("handles BS at column 0 (no movement)", () => {
			const state = new TerminalState(80, 24);
			state.processAction({ type: "Execute", value: C0.BS });

			expect(state.cursorCol).toBe(0);
		});

		test("handles HT (horizontal tab)", () => {
			const state = new TerminalState(80, 24);
			state.processAction({ type: "Execute", value: C0.HT });

			expect(state.cursorCol).toBe(8);
		});

		test("handles HT from column 3 to column 8", () => {
			const state = new TerminalState(80, 24);
			for (let i = 0; i < 3; i++) {
				state.processAction({ type: "Print", value: "X" });
			}
			state.processAction({ type: "Execute", value: C0.HT });

			expect(state.cursorCol).toBe(8);
		});

		test("handles BEL (no visible change)", () => {
			const state = new TerminalState(80, 24);
			// BEL should not change state, just not crash
			state.processAction({ type: "Execute", value: C0.BEL });
			expect(state.cursorCol).toBe(0);
		});
	});

	describe("getDirtyRows", () => {
		test("returns dirty row indices", () => {
			const state = new TerminalState(80, 24);
			state.clearDirty();

			state.processAction({ type: "Print", value: "A" });

			const dirty = state.getDirtyRows();
			expect(dirty).toContain(0);
		});
	});

	describe("clearDirty", () => {
		test("clears all dirty flags", () => {
			const state = new TerminalState(80, 24);
			state.processAction({ type: "Print", value: "A" });

			state.clearDirty();

			const dirty = state.getDirtyRows();
			expect(dirty.length).toBe(0);
		});
	});

	describe("resize", () => {
		test("updates dimensions", () => {
			const state = new TerminalState(80, 24);
			state.resize(120, 40);

			expect(state.cols).toBe(120);
			expect(state.rows).toBe(40);
		});

		test("reflows content and tracks cursor on resize", () => {
			const state = new TerminalState(80, 24);
			// Print 70 X's on an 80-column line
			for (let i = 0; i < 70; i++) {
				state.processAction({ type: "Print", value: "X" });
			}
			expect(state.cursorCol).toBe(70);

			// Resize to 50 columns: 70 chars reflow into 50+20,
			// so cursor ends up at row 1, col 20
			state.resize(50, 10);
			expect(state.cursorCol).toBe(20);
			expect(state.cursorRow).toBe(1);
		});
	});

	describe("multiple characters", () => {
		test("prints string correctly", () => {
			const state = new TerminalState(80, 24);
			const text = "Hello, World!";
			for (const char of text) {
				state.processAction({ type: "Print", value: char });
			}

			const line = state.getActiveBuffer().getLine(0).getText();
			expect(line.startsWith("Hello, World!")).toBe(true);
			expect(state.cursorCol).toBe(13);
		});

		test("handles CR LF sequence", () => {
			const state = new TerminalState(80, 24);
			for (const char of "Line1") {
				state.processAction({ type: "Print", value: char });
			}
			state.processAction({ type: "Execute", value: C0.CR });
			state.processAction({ type: "Execute", value: C0.LF });
			for (const char of "Line2") {
				state.processAction({ type: "Print", value: char });
			}

			expect(
				state.getActiveBuffer().getLine(0).getText().startsWith("Line1"),
			).toBe(true);
			expect(
				state.getActiveBuffer().getLine(1).getText().startsWith("Line2"),
			).toBe(true);
			expect(state.cursorRow).toBe(1);
			expect(state.cursorCol).toBe(5);
		});
	});

	describe("dirty row tracking", () => {
		test("marks rows dirty when printed", () => {
			const state = new TerminalState(10, 5);
			state.clearDirty();

			state.processAction({ type: "Print", value: "A" });
			expect(state.getDirtyRows()).toEqual([0]);
		});

		test("marks rows dirty when scrolled", () => {
			const state = new TerminalState(10, 3);
			// Move to bottom and trigger scroll
			state.processAction({ type: "Execute", value: C0.LF });
			state.processAction({ type: "Execute", value: C0.LF });
			state.clearDirty();

			state.processAction({ type: "Execute", value: C0.LF });
			// With differential scroll optimization (FR8), full-screen
			// scroll(1) marks only the last row dirty + emits scroll event.
			// At minimum, the last row (where new blank line appears) must be dirty.
			const dirty = state.getDirtyRows();
			expect(dirty.length).toBeGreaterThanOrEqual(1);
			expect(dirty).toContain(2); // last row (0-indexed) of 3-row terminal
		});
	});

	// Phase 4: Cursor movement CSI tests
	describe("processAction - CSI cursor movement", () => {
		test("CursorUp moves cursor up", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 10, col: 10 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "CursorUp", data: 3 },
			});

			expect(state.cursorRow).toBe(6);
		});

		test("CursorDown moves cursor down", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorDown", data: 5 },
			});

			expect(state.cursorRow).toBe(5);
		});

		test("CursorForward moves cursor right", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorForward", data: 10 },
			});

			expect(state.cursorCol).toBe(10);
		});

		test("CursorBack moves cursor left", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 1, col: 20 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "CursorBack", data: 5 },
			});

			expect(state.cursorCol).toBe(14);
		});

		test("CursorNextLine moves cursor down and to column 1", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 5, col: 20 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "CursorNextLine", data: 3 },
			});

			expect(state.cursorRow).toBe(7);
			expect(state.cursorCol).toBe(0);
		});

		test("CursorPreviousLine moves cursor up and to column 1", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 10, col: 20 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "CursorPreviousLine", data: 2 },
			});

			expect(state.cursorRow).toBe(7);
			expect(state.cursorCol).toBe(0);
		});

		test("CursorHorizontalAbsolute sets column", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 10, col: 20 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "CursorHorizontalAbsolute", data: 35 },
			});

			expect(state.cursorCol).toBe(34); // 1-indexed to 0-indexed
			expect(state.cursorRow).toBe(9); // Row unchanged
		});

		test("CursorVerticalAbsolute sets row", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 10, col: 20 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "CursorVerticalAbsolute", data: 15 },
			});

			expect(state.cursorRow).toBe(14); // 1-indexed to 0-indexed
			expect(state.cursorCol).toBe(19); // Column unchanged
		});

		test("CursorPosition sets absolute position", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 15, col: 30 } },
			});

			expect(state.cursorRow).toBe(14); // 1-indexed to 0-indexed
			expect(state.cursorCol).toBe(29);
		});
	});

	// Phase 4: Erase operations tests
	describe("processAction - CSI erase operations", () => {
		test("EraseInDisplay Below clears from cursor to end", () => {
			const state = new TerminalState(10, 5);
			// Fill screen row by row using cursor positioning
			for (let row = 0; row < 5; row++) {
				state.processAction({
					type: "Csi",
					value: { action: "CursorPosition", data: { row: row + 1, col: 1 } },
				});
				for (let i = 0; i < 10; i++) {
					state.processAction({ type: "Print", value: "X" });
				}
			}

			// Position cursor at row 2 (1-indexed: row 3), col 5 (1-indexed: col 6)
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 3, col: 6 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "EraseInDisplay", data: "Below" },
			});

			// Before cursor should be unchanged (row 2, col 4)
			expect(state.getActiveBuffer().getCell(4, 2).char).toBe("X");
			// At cursor should be cleared (row 2, col 5)
			expect(state.getActiveBuffer().getCell(5, 2).char).toBe(" ");
			// After cursor should be cleared
			expect(state.getActiveBuffer().getCell(0, 3).char).toBe(" ");
		});

		test("EraseInDisplay All clears entire screen", () => {
			const state = new TerminalState(10, 5);
			for (let i = 0; i < 10; i++) {
				state.processAction({ type: "Print", value: "X" });
			}

			state.processAction({
				type: "Csi",
				value: { action: "EraseInDisplay", data: "All" },
			});

			expect(state.getActiveBuffer().getCell(0, 0).char).toBe(" ");
			expect(state.getActiveBuffer().getCell(9, 4).char).toBe(" ");
		});

		test("EraseCharacters erases without shifting", () => {
			const state = new TerminalState(10, 5);
			for (let i = 0; i < 10; i++) {
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}

			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 1, col: 4 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "EraseCharacters", data: 3 },
			});

			expect(state.getActiveBuffer().getCell(3, 0).char).toBe(" ");
			expect(state.getActiveBuffer().getCell(5, 0).char).toBe(" ");
			expect(state.getActiveBuffer().getCell(6, 0).char).toBe("G"); // Unchanged
		});
	});

	// Phase 4: Insert/delete operations tests
	describe("processAction - CSI insert/delete", () => {
		test("InsertLines inserts blank lines", () => {
			const state = new TerminalState(10, 5);
			// Place A-E on rows 0-4 using explicit cursor positioning
			for (let i = 0; i < 5; i++) {
				state.processAction({
					type: "Csi",
					value: { action: "CursorPosition", data: { row: i + 1, col: 1 } },
				});
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}

			// Insert line at row 1 (1-indexed: row 2)
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 2, col: 1 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "InsertLines", data: 1 },
			});

			// Row 0 should be unchanged (A)
			expect(state.getActiveBuffer().getCell(0, 0).char).toBe("A");
			// Row 1 should be blank (inserted line)
			expect(state.getActiveBuffer().getCell(0, 1).char).toBe(" ");
			// Row 2 should have what was in row 1 (B)
			expect(state.getActiveBuffer().getCell(0, 2).char).toBe("B");
		});

		test("DeleteLines deletes lines", () => {
			const state = new TerminalState(10, 5);
			// Place A-E on rows 0-4 using explicit cursor positioning
			for (let i = 0; i < 5; i++) {
				state.processAction({
					type: "Csi",
					value: { action: "CursorPosition", data: { row: i + 1, col: 1 } },
				});
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}

			// Delete line at row 1 (1-indexed: row 2)
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 2, col: 1 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "DeleteLines", data: 1 },
			});

			// Row 0 should be unchanged (A)
			expect(state.getActiveBuffer().getCell(0, 0).char).toBe("A");
			// Row 1 should now have what was in row 2 (C)
			expect(state.getActiveBuffer().getCell(0, 1).char).toBe("C");
			// Last row should be blank
			expect(state.getActiveBuffer().getCell(0, 4).char).toBe(" ");
		});

		test("InsertCharacters inserts blank characters", () => {
			const state = new TerminalState(10, 5);
			for (let i = 0; i < 10; i++) {
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}

			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 1, col: 4 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "InsertCharacters", data: 2 },
			});

			expect(state.getActiveBuffer().getCell(3, 0).char).toBe(" ");
			expect(state.getActiveBuffer().getCell(4, 0).char).toBe(" ");
			expect(state.getActiveBuffer().getCell(5, 0).char).toBe("D");
		});

		test("DeleteCharacters deletes characters", () => {
			const state = new TerminalState(10, 5);
			for (let i = 0; i < 10; i++) {
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}

			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 1, col: 4 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "DeleteCharacters", data: 2 },
			});

			expect(state.getActiveBuffer().getCell(3, 0).char).toBe("F");
			expect(state.getActiveBuffer().getCell(8, 0).char).toBe(" ");
		});
	});

	// Phase 4: Scroll operations tests
	describe("processAction - CSI scroll", () => {
		test("ScrollUp scrolls buffer up", () => {
			const state = new TerminalState(10, 5);
			// Place A-E on rows 0-4 using explicit cursor positioning
			for (let i = 0; i < 5; i++) {
				state.processAction({
					type: "Csi",
					value: { action: "CursorPosition", data: { row: i + 1, col: 1 } },
				});
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}

			state.processAction({
				type: "Csi",
				value: { action: "ScrollUp", data: 1 },
			});

			// Row 0 should now have what was in row 1 (B)
			expect(state.getActiveBuffer().getCell(0, 0).char).toBe("B");
			// Last row should be blank
			expect(state.getActiveBuffer().getCell(0, 4).char).toBe(" ");
		});

		test("ScrollDown scrolls buffer down", () => {
			const state = new TerminalState(10, 5);
			// Place A-E on rows 0-4 using explicit cursor positioning
			for (let i = 0; i < 5; i++) {
				state.processAction({
					type: "Csi",
					value: { action: "CursorPosition", data: { row: i + 1, col: 1 } },
				});
				state.processAction({
					type: "Print",
					value: String.fromCharCode(65 + i),
				});
			}

			state.processAction({
				type: "Csi",
				value: { action: "ScrollDown", data: 1 },
			});

			// Row 0 should be blank
			expect(state.getActiveBuffer().getCell(0, 0).char).toBe(" ");
			// Row 1 should have what was in row 0 (A)
			expect(state.getActiveBuffer().getCell(0, 1).char).toBe("A");
		});

		test("SetScrollRegion sets scroll region and moves cursor home", () => {
			const state = new TerminalState(80, 24);
			state.processAction({
				type: "Csi",
				value: { action: "CursorPosition", data: { row: 10, col: 20 } },
			});
			state.processAction({
				type: "Csi",
				value: { action: "SetScrollRegion", data: { top: 5, bottom: 20 } },
			});

			// Cursor should move to home
			expect(state.cursorRow).toBe(0);
			expect(state.cursorCol).toBe(0);

			// Scroll region should be set
			const region = state.getActiveBuffer().getScrollRegion();
			expect(region).toEqual({ top: 4, bottom: 19 });
		});
	});

	describe("Scrollback buffer", () => {
		test("captures lines when they scroll off the top", () => {
			const state = new TerminalState(10, 3);

			// Fill screen with content: write enough lines to cause scrolling
			// Start at top, write "AAAA...", then LF, write "BBBB...", then LF, etc.
			for (let i = 0; i < 5; i++) {
				// Write a full line of characters
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + i) });
				}
				// Move to next line (this causes scrolling when at bottom)
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			// Should have captured at least some scrollback
			expect(state.getScrollbackLength()).toBeGreaterThan(0);
		});

		test("enforces maximum scrollback size", () => {
			const state = new TerminalState(10, 2, 5); // Max 5 lines in scrollback

			// Scroll many lines off the top
			for (let i = 0; i < 12; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + (i % 26)) });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			// Should enforce max scrollback (may be <= 5 due to how scrolling works)
			expect(state.getScrollbackLength()).toBeLessThanOrEqual(5);
		});

		test("does not capture scrollback in alternate buffer", () => {
			const state = new TerminalState(10, 3);

			// Switch to alternate buffer
			state.processAction({
				type: "Csi",
				value: { action: "SetMode", data: [1049] },
			});

			// Fill lines in alternate buffer to cause scrolling
			for (let i = 0; i < 6; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: "X" });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			// Scrollback should be empty (alternate buffer doesn't save scrollback)
			expect(state.getScrollbackLength()).toBe(0);
		});

		test("getScrollbackBuffer returns array of lines", () => {
			const state = new TerminalState(10, 3);

			// Fill with content and scroll
			for (let i = 0; i < 5; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + i) });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			const scrollback = state.getScrollbackBuffer();
			expect(scrollback.length).toBeGreaterThan(0);
			// Verify it's an array of Line objects
			expect(scrollback[0]).toBeDefined();
			expect(scrollback[0]!.getText).toBeDefined();
		});

		test("clears scrollback on reset", () => {
			const state = new TerminalState(10, 3);

			// Fill some lines to create scrollback
			for (let i = 0; i < 6; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: "X" });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			expect(state.getScrollbackLength()).toBeGreaterThan(0);

			// Reset terminal
			state.reset();

			// Scrollback should be cleared
			expect(state.getScrollbackLength()).toBe(0);
		});
	});

	describe("getScrollbackLine", () => {
		test("returns correct line by index", () => {
			const state = new TerminalState(10, 3);

			// Fill with known content to create scrollback
			for (let i = 0; i < 5; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + i) });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			const scrollbackLength = state.getScrollbackLength();
			expect(scrollbackLength).toBeGreaterThan(0);

			// Verify getScrollbackLine returns a line with content
			const line = state.getScrollbackLine(0);
			expect(line).toBeDefined();
			expect(line.getText).toBeDefined();
		});

		test("returns consistent content across calls", () => {
			const state = new TerminalState(10, 3);

			// Create scrollback
			for (let i = 0; i < 5; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + i) });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			const scrollbackLength = state.getScrollbackLength();
			expect(scrollbackLength).toBeGreaterThan(0);

			// Two calls should return equivalent content
			const line1 = state.getScrollbackLine(0);
			const line2 = state.getScrollbackLine(0);
			expect(line1.getText()).toBe(line2.getText());
		});

		test("matches getScrollbackBuffer content", () => {
			const state = new TerminalState(10, 3);

			// Create scrollback
			for (let i = 0; i < 5; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + i) });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			const scrollbackLength = state.getScrollbackLength();
			const scrollbackBuffer = state.getScrollbackBuffer();

			// Each line from getScrollbackLine should have the same text as the buffer
			for (let i = 0; i < scrollbackLength; i++) {
				const line = state.getScrollbackLine(i);
				expect(line.getText()).toBe(scrollbackBuffer[i]!.getText());
			}
		});
	});
});
