/**
 * Tests for CanvasRenderer.
 * Note: These tests require a DOM environment (happy-dom).
 */
import { beforeEach, afterEach, describe, expect, test, mock } from "bun:test";
import type { CellAttributes } from "./attributes.ts";
import { createDefaultAttributes } from "./attributes.ts";
import { Line } from "./grid.ts";
import {
	groupCellsIntoSpans,
	getVisibleLines,
	calculateScrollPosition,
} from "./canvas-renderer.ts";
import { TerminalState } from "./state.ts";

describe("CanvasRenderer", () => {
	describe("groupCellsIntoSpans", () => {
		test("groups cells with same attributes into single span", () => {
			const line = new Line(5);
			const attrs = createDefaultAttributes();

			// Set 'Hello' with same attributes
			for (let i = 0; i < 5; i++) {
				const char = "Hello"[i]!;
				line.setCell(i, { char, width: 1, attrs, dirty: false });
			}

			const spans = groupCellsIntoSpans(line);

			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("Hello");
		});

		test("creates separate spans for different attributes", () => {
			const line = new Line(4);
			const defaultAttrs = createDefaultAttributes();
			const boldAttrs: CellAttributes = { ...createDefaultAttributes(), bold: true };

			// 'AB' with default, 'CD' with bold
			line.setCell(0, { char: "A", width: 1, attrs: defaultAttrs, dirty: false });
			line.setCell(1, { char: "B", width: 1, attrs: defaultAttrs, dirty: false });
			line.setCell(2, { char: "C", width: 1, attrs: boldAttrs, dirty: false });
			line.setCell(3, { char: "D", width: 1, attrs: boldAttrs, dirty: false });

			const spans = groupCellsIntoSpans(line);

			expect(spans.length).toBe(2);
			expect(spans[0]!.text).toBe("AB");
			expect(spans[1]!.text).toBe("CD");
			expect(spans[1]!.attrs.bold).toBe(true);
		});

		test("skips wide character placeholder cells (width=0)", () => {
			const line = new Line(4);
			const attrs = createDefaultAttributes();

			// Wide character followed by placeholder
			line.setCell(0, { char: "A", width: 1, attrs, dirty: false });
			line.setCell(1, { char: "\u3042", width: 2, attrs, dirty: false }); // Japanese 'a'
			line.setCell(2, { char: "", width: 0, attrs, dirty: false }); // Placeholder for wide char
			line.setCell(3, { char: "B", width: 1, attrs, dirty: false });

			const spans = groupCellsIntoSpans(line);

			// Should only include actual characters, not placeholders
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("A\u3042B");
		});

		test("combines combining marks with previous character", () => {
			const line = new Line(3);
			const attrs = createDefaultAttributes();

			// 'e' followed by combining acute accent
			line.setCell(0, { char: "e", width: 1, attrs, dirty: false });
			line.setCell(1, { char: "\u0301", width: 0, attrs, dirty: false }); // Combining acute
			line.setCell(2, { char: "x", width: 1, attrs, dirty: false });

			const spans = groupCellsIntoSpans(line);

			// Combining mark should be merged with previous character
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("e\u0301x");
		});

		test("handles empty line", () => {
			const line = new Line(5);
			// Default empty cells have space character

			const spans = groupCellsIntoSpans(line);

			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("     "); // 5 spaces
		});
	});

	describe("getVisibleLines", () => {
		test("returns screen buffer lines when scrollOffset is 0", () => {
			const state = new TerminalState(80, 24);
			// Process some text to first cell
			state.processAction({ type: "Print", value: "H" });

			const lines = getVisibleLines(state, 0);

			expect(lines.length).toBe(24);
			expect(lines[0]!.getCell(0).char).toBe("H");
		});

		test("returns correct number of rows", () => {
			const state = new TerminalState(80, 3);

			const lines = getVisibleLines(state, 0);

			expect(lines.length).toBe(3);
		});
	});

	describe("calculateScrollPosition", () => {
		test("returns scrollbackLength - scrollOffset", () => {
			const pos = calculateScrollPosition(0, 100);
			expect(pos).toBe(100); // scrollbackLength - scrollOffset
		});

		test("returns correct position for scrollOffset > 0", () => {
			const pos = calculateScrollPosition(10, 100);
			expect(pos).toBe(90); // scrollbackLength - scrollOffset
		});

		test("handles zero scrollback", () => {
			const pos = calculateScrollPosition(0, 0);
			expect(pos).toBe(0);
		});
	});

	describe("TextSpan structure", () => {
		test("includes startCol and cellCount", () => {
			const line = new Line(5);
			const attrs = createDefaultAttributes();

			for (let i = 0; i < 5; i++) {
				line.setCell(i, { char: "A", width: 1, attrs, dirty: false });
			}

			const spans = groupCellsIntoSpans(line);

			expect(spans[0]!.startCol).toBe(0);
			expect(spans[0]!.cellCount).toBe(5);
		});

		test("tracks startCol correctly for multiple spans", () => {
			const line = new Line(6);
			const defaultAttrs = createDefaultAttributes();
			const boldAttrs: CellAttributes = { ...createDefaultAttributes(), bold: true };

			// 'ABC' with default, 'DEF' with bold
			line.setCell(0, { char: "A", width: 1, attrs: defaultAttrs, dirty: false });
			line.setCell(1, { char: "B", width: 1, attrs: defaultAttrs, dirty: false });
			line.setCell(2, { char: "C", width: 1, attrs: defaultAttrs, dirty: false });
			line.setCell(3, { char: "D", width: 1, attrs: boldAttrs, dirty: false });
			line.setCell(4, { char: "E", width: 1, attrs: boldAttrs, dirty: false });
			line.setCell(5, { char: "F", width: 1, attrs: boldAttrs, dirty: false });

			const spans = groupCellsIntoSpans(line);

			expect(spans.length).toBe(2);
			expect(spans[0]!.startCol).toBe(0);
			expect(spans[0]!.cellCount).toBe(3);
			expect(spans[1]!.startCol).toBe(3);
			expect(spans[1]!.cellCount).toBe(3);
		});

		test("counts wide characters correctly in cellCount", () => {
			const line = new Line(4);
			const attrs = createDefaultAttributes();

			// 'A' + wide char (2 cells) + 'B'
			line.setCell(0, { char: "A", width: 1, attrs, dirty: false });
			line.setCell(1, { char: "\u3042", width: 2, attrs, dirty: false });
			line.setCell(2, { char: "", width: 0, attrs, dirty: false }); // Placeholder
			line.setCell(3, { char: "B", width: 1, attrs, dirty: false });

			const spans = groupCellsIntoSpans(line);

			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("A\u3042B");
			// cellCount should be 1 + 2 + 1 = 4 (wide char counts as 2)
			expect(spans[0]!.cellCount).toBe(4);
		});
	});
});

/**
 * Integration tests for CanvasRenderer class.
 * These tests require proper Canvas mock which is complex in happy-dom.
 * We test the public interface where possible.
 */
describe("CanvasRenderer integration", () => {
	// These tests are skipped because happy-dom doesn't fully support canvas
	// and the mock approach has issues with appendChild
	test.todo("creates renderer with container");
	test.todo("schedules render with requestAnimationFrame");
	test.todo("resizes canvas correctly");
	test.todo("measures character size correctly");
});

// Import for Phase 2 and Phase 3 tests
import { buildFontString, applyTextAttributes, normalizeSelection } from "./canvas-renderer.ts";

describe("Phase 2: Attributes and Styling", () => {
	describe("buildFontString", () => {
		test("returns default font string for no style attributes", () => {
			const attrs = createDefaultAttributes();
			const fontString = buildFontString(attrs, 13, "monospace");

			expect(fontString).toBe("13px monospace");
		});

		test("includes bold when bold attribute is set", () => {
			const attrs: CellAttributes = { ...createDefaultAttributes(), bold: true };
			const fontString = buildFontString(attrs, 13, "monospace");

			expect(fontString).toBe("bold 13px monospace");
		});

		test("includes italic when italic attribute is set", () => {
			const attrs: CellAttributes = { ...createDefaultAttributes(), italic: true };
			const fontString = buildFontString(attrs, 13, "monospace");

			expect(fontString).toBe("italic 13px monospace");
		});

		test("includes both bold and italic when both are set", () => {
			const attrs: CellAttributes = {
				...createDefaultAttributes(),
				bold: true,
				italic: true,
			};
			const fontString = buildFontString(attrs, 13, "monospace");

			expect(fontString).toBe("italic bold 13px monospace");
		});
	});

	describe("applyTextAttributes", () => {
		test("returns correct styles for default attributes", () => {
			const attrs = createDefaultAttributes();
			const styles = applyTextAttributes(attrs);

			expect(styles.globalAlpha).toBe(1);
			expect(styles.hidden).toBe(false);
		});

		test("sets globalAlpha to 0.5 for dim attribute", () => {
			const attrs: CellAttributes = { ...createDefaultAttributes(), dim: true };
			const styles = applyTextAttributes(attrs);

			expect(styles.globalAlpha).toBe(0.5);
		});

		test("sets hidden to true for hidden attribute", () => {
			const attrs: CellAttributes = { ...createDefaultAttributes(), hidden: true };
			const styles = applyTextAttributes(attrs);

			expect(styles.hidden).toBe(true);
		});

		test("returns underline flag when underline is set", () => {
			const attrs: CellAttributes = { ...createDefaultAttributes(), underline: true };
			const styles = applyTextAttributes(attrs);

			expect(styles.underline).toBe(true);
		});

		test("returns strikethrough flag when strikethrough is set", () => {
			const attrs: CellAttributes = { ...createDefaultAttributes(), strikethrough: true };
			const styles = applyTextAttributes(attrs);

			expect(styles.strikethrough).toBe(true);
		});
	});

	describe("color handling", () => {
		test("getEffectiveForeground returns default foreground for null fg", () => {
			const { getEffectiveForeground } = require("./attributes.ts");
			const attrs = createDefaultAttributes();
			const fg = getEffectiveForeground(attrs);

			// DEFAULT_FOREGROUND is { r: 0x40, g: 0xff, b: 0x40 }
			expect(fg.r).toBe(0x40);
			expect(fg.g).toBe(0xff);
			expect(fg.b).toBe(0x40);
		});

		test("getEffectiveBackground returns null for null bg (transparent)", () => {
			const { getEffectiveBackground } = require("./attributes.ts");
			const attrs = createDefaultAttributes();
			const bg = getEffectiveBackground(attrs);

			expect(bg).toBe(null);
		});

		test("reverse attribute swaps foreground and background", () => {
			const { getEffectiveForeground, getEffectiveBackground } = require("./attributes.ts");
			const attrs: CellAttributes = {
				...createDefaultAttributes(),
				reverse: true,
				fg: { type: "rgb", r: 255, g: 0, b: 0 },
				bg: { type: "rgb", r: 0, g: 255, b: 0 },
			};

			const fg = getEffectiveForeground(attrs);
			const bg = getEffectiveBackground(attrs);

			// Foreground should be the original background (green)
			expect(fg.r).toBe(0);
			expect(fg.g).toBe(255);
			expect(fg.b).toBe(0);

			// Background should be the original foreground (red)
			expect(bg!.r).toBe(255);
			expect(bg!.g).toBe(0);
			expect(bg!.b).toBe(0);
		});
	});
});

describe("Phase 3: Cursor and Selection", () => {
	describe("normalizeSelection", () => {
		test("returns unchanged selection when start is before end (same row)", () => {
			const selection = {
				start: { col: 5, row: 2 },
				end: { col: 10, row: 2 },
			};

			const normalized = normalizeSelection(selection);

			expect(normalized.start.col).toBe(5);
			expect(normalized.start.row).toBe(2);
			expect(normalized.end.col).toBe(10);
			expect(normalized.end.row).toBe(2);
		});

		test("returns unchanged selection when start row is before end row", () => {
			const selection = {
				start: { col: 10, row: 1 },
				end: { col: 5, row: 3 },
			};

			const normalized = normalizeSelection(selection);

			expect(normalized.start.row).toBe(1);
			expect(normalized.end.row).toBe(3);
		});

		test("swaps start and end when end is before start (same row)", () => {
			const selection = {
				start: { col: 10, row: 2 },
				end: { col: 5, row: 2 },
			};

			const normalized = normalizeSelection(selection);

			expect(normalized.start.col).toBe(5);
			expect(normalized.start.row).toBe(2);
			expect(normalized.end.col).toBe(10);
			expect(normalized.end.row).toBe(2);
		});

		test("swaps start and end when end row is before start row", () => {
			const selection = {
				start: { col: 5, row: 5 },
				end: { col: 10, row: 2 },
			};

			const normalized = normalizeSelection(selection);

			expect(normalized.start.row).toBe(2);
			expect(normalized.start.col).toBe(10);
			expect(normalized.end.row).toBe(5);
			expect(normalized.end.col).toBe(5);
		});
	});

	describe("cursor styles", () => {
		test.todo("renders block cursor as filled rectangle");
		test.todo("renders underline cursor as thin rectangle at bottom");
		test.todo("renders bar cursor as thin rectangle at left");
	});

	describe("cursor blink", () => {
		test.todo("starts cursor blink timer");
		test.todo("stops cursor blink timer on dispose");
	});

	describe("selection rendering", () => {
		test.todo("renders single line selection");
		test.todo("renders multi-line selection");
		test.todo("clears selection highlight");
	});
});
