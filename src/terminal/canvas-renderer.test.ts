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
	groupPackedCellsIntoSpans,
	packedAttrsEqual,
	unpackAttrsFromBinary,
	getVisibleLines,
	calculateScrollPosition,
} from "./canvas-renderer.ts";
import { attributesEqual, packColor, packStyleFlags } from "./attributes.ts";
import { TerminalState } from "./state.ts";
import { C0 } from "../types/terminal.ts";

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

		test("returns lines from scrollback when scrollOffset > 0", () => {
			const state = new TerminalState(10, 3);

			// Fill 5 lines to create scrollback
			for (let i = 0; i < 5; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + i) });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			// Should have some scrollback
			const scrollbackLength = state.getScrollbackLength();
			expect(scrollbackLength).toBeGreaterThan(0);

			// When scrollOffset = 1, should show lines from scrollback + screen
			const lines = getVisibleLines(state, 1);
			expect(lines.length).toBe(3);
		});

		test("handles scrollOffset at max scrollback", () => {
			const state = new TerminalState(10, 3);

			// Fill lines to create scrollback
			for (let i = 0; i < 5; i++) {
				for (let j = 0; j < 10; j++) {
					state.processAction({ type: "Print", value: String.fromCharCode(65 + i) });
				}
				state.processAction({ type: "Execute", value: C0.LF });
				state.processAction({ type: "Execute", value: C0.CR });
			}

			const scrollbackLength = state.getScrollbackLength();
			expect(scrollbackLength).toBeGreaterThan(0);

			// scrollOffset at max should show oldest scrollback lines
			const lines = getVisibleLines(state, scrollbackLength);
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

// ── Packed Binary Span Parser Tests (Phase 1) ─────────────

/**
 * Helper: pack a single cell into bytes matching WASM packed format.
 * Binary: char_len(1) + char_data + width(1) + fg(4) + bg(4) + flags(2 LE)
 */
function packCell(
	ch: string,
	width: number,
	attrs: CellAttributes,
): number[] {
	const bytes: number[] = [];
	const encoder = new TextEncoder();

	if (ch.length === 0) {
		bytes.push(0); // charLen = 0
	} else {
		const encoded = encoder.encode(ch);
		if (encoded.length >= 0xFF) {
			bytes.push(0xFF);
			bytes.push((encoded.length >> 8) & 0xFF);
			bytes.push(encoded.length & 0xFF);
			bytes.push(...encoded);
		} else {
			bytes.push(encoded.length);
			bytes.push(...encoded);
		}
	}

	bytes.push(width);

	const fg = packColor(attrs.fg);
	bytes.push(fg.tag, fg.r, fg.g, fg.b);

	const bg = packColor(attrs.bg);
	bytes.push(bg.tag, bg.r, bg.g, bg.b);

	const flags = packStyleFlags(attrs);
	bytes.push(flags & 0xFF, (flags >> 8) & 0xFF);

	return bytes;
}

/** Helper: build packed row from cell byte arrays. */
function buildPackedRow(...cells: number[][]): Uint8Array {
	const flat: number[] = [];
	for (const cell of cells) {
		flat.push(...cell);
	}
	return new Uint8Array(flat);
}

describe("Packed Binary Span Parser", () => {
	const defaultAttrs = createDefaultAttributes();
	const boldAttrs: CellAttributes = { ...createDefaultAttributes(), bold: true };
	const colorAttrs: CellAttributes = {
		...createDefaultAttributes(),
		fg: { type: "rgb", r: 255, g: 0, b: 0 },
		bg: { type: "rgb", r: 0, g: 255, b: 0 },
	};

	describe("packedAttrsEqual", () => {
		test("returns true for identical attribute bytes", () => {
			const packed = buildPackedRow(
				packCell("A", 1, defaultAttrs),
				packCell("B", 1, defaultAttrs),
			);
			// Attribute bytes start after char+width: A=1+1+1=3, B=1+1+1=3
			// Cell A: charLen(1) + charData(1) + width(1) = offset 3 for attrs
			// Cell B: offset 3+10 + charLen(1) + charData(1) + width(1) = offset 16
			const attrOffsetA = 3; // after "A" (charLen=1, charData=1, width=1)
			const attrOffsetB = 3 + 10 + 3; // after 10 attr bytes of A, then B's header
			expect(packedAttrsEqual(packed, attrOffsetA, attrOffsetB)).toBe(true);
		});

		test("returns false for different attribute bytes", () => {
			const packed = buildPackedRow(
				packCell("A", 1, defaultAttrs),
				packCell("B", 1, boldAttrs),
			);
			const attrOffsetA = 3;
			const attrOffsetB = 3 + 10 + 3;
			expect(packedAttrsEqual(packed, attrOffsetA, attrOffsetB)).toBe(false);
		});
	});

	describe("unpackAttrsFromBinary", () => {
		test("unpacks default attributes", () => {
			const packed = buildPackedRow(packCell("A", 1, defaultAttrs));
			const attrs = unpackAttrsFromBinary(packed, 3);
			expect(attrs.bold).toBe(false);
			expect(attrs.fg).toBe(null);
			expect(attrs.bg).toBe(null);
		});

		test("unpacks bold attribute", () => {
			const packed = buildPackedRow(packCell("A", 1, boldAttrs));
			const attrs = unpackAttrsFromBinary(packed, 3);
			expect(attrs.bold).toBe(true);
		});

		test("unpacks RGB colors", () => {
			const packed = buildPackedRow(packCell("A", 1, colorAttrs));
			const attrs = unpackAttrsFromBinary(packed, 3);
			expect(attrs.fg).toEqual({ type: "rgb", r: 255, g: 0, b: 0 });
			expect(attrs.bg).toEqual({ type: "rgb", r: 0, g: 255, b: 0 });
		});

		test("unpacks indexed color", () => {
			const indexedAttrs: CellAttributes = {
				...createDefaultAttributes(),
				fg: { type: "indexed", index: 5 },
			};
			const packed = buildPackedRow(packCell("A", 1, indexedAttrs));
			const attrs = unpackAttrsFromBinary(packed, 3);
			expect(attrs.fg).toEqual({ type: "indexed", index: 5 });
		});
	});

	describe("groupPackedCellsIntoSpans", () => {
		test("TS-07: groups consecutive cells with same attributes", () => {
			const packed = buildPackedRow(
				packCell("H", 1, defaultAttrs),
				packCell("e", 1, defaultAttrs),
				packCell("l", 1, defaultAttrs),
				packCell("l", 1, defaultAttrs),
				packCell("o", 1, defaultAttrs),
			);
			const spans = groupPackedCellsIntoSpans(packed, 5);
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("Hello");
			expect(spans[0]!.startCol).toBe(0);
			expect(spans[0]!.cellCount).toBe(5);
		});

		test("TS-08: splits spans at attribute boundaries", () => {
			const packed = buildPackedRow(
				packCell("A", 1, defaultAttrs),
				packCell("B", 1, defaultAttrs),
				packCell("C", 1, boldAttrs),
				packCell("D", 1, boldAttrs),
			);
			const spans = groupPackedCellsIntoSpans(packed, 4);
			expect(spans.length).toBe(2);
			expect(spans[0]!.text).toBe("AB");
			expect(spans[0]!.startCol).toBe(0);
			expect(spans[0]!.cellCount).toBe(2);
			expect(spans[1]!.text).toBe("CD");
			expect(spans[1]!.startCol).toBe(2);
			expect(spans[1]!.cellCount).toBe(2);
			expect(spans[1]!.attrs.bold).toBe(true);
		});

		test("TS-02: handles empty row (all space cells)", () => {
			const packed = buildPackedRow(
				packCell(" ", 1, defaultAttrs),
				packCell(" ", 1, defaultAttrs),
				packCell(" ", 1, defaultAttrs),
			);
			const spans = groupPackedCellsIntoSpans(packed, 3);
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("   ");
		});

		test("TS-03: handles wide characters", () => {
			const packed = buildPackedRow(
				packCell("A", 1, defaultAttrs),
				packCell("\u3042", 2, defaultAttrs), // あ (wide)
				packCell("", 0, defaultAttrs),       // placeholder
				packCell("B", 1, defaultAttrs),
			);
			const spans = groupPackedCellsIntoSpans(packed, 4);
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("A\u3042B");
			expect(spans[0]!.cellCount).toBe(4);
		});

		test("TS-04: handles combining marks", () => {
			const packed = buildPackedRow(
				packCell("e", 1, defaultAttrs),
				packCell("\u0301", 0, defaultAttrs), // combining acute
				packCell("x", 1, defaultAttrs),
			);
			const spans = groupPackedCellsIntoSpans(packed, 3);
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("e\u0301x");
			// cellCount: e(1) + x(1) = 2 (combining mark doesn't add cells)
			expect(spans[0]!.cellCount).toBe(2);
		});

		test("TS-05: handles overflow characters (charLen=0xFF)", () => {
			// Build overflow character manually: a 4-byte emoji
			const encoder = new TextEncoder();
			const emoji = "\u{1F600}"; // 😀
			const emojiBytes = encoder.encode(emoji);
			expect(emojiBytes.length).toBe(4);

			// Manually build packed data with overflow format
			const bytes: number[] = [];
			bytes.push(0xFF); // overflow marker
			bytes.push(0, emojiBytes.length); // 2-byte BE length
			bytes.push(...emojiBytes);
			bytes.push(2); // width=2

			// Default attrs (10 bytes: fg 4 + bg 4 + flags 2)
			bytes.push(0, 0, 0, 0); // fg=null
			bytes.push(0, 0, 0, 0); // bg=null
			bytes.push(0, 0);       // flags=0

			// Placeholder cell for wide char
			bytes.push(0); // charLen=0
			bytes.push(0); // width=0
			bytes.push(0, 0, 0, 0, 0, 0, 0, 0, 0, 0); // attrs

			const packed = new Uint8Array(bytes);
			const spans = groupPackedCellsIntoSpans(packed, 2);
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe(emoji);
			expect(spans[0]!.cellCount).toBe(2);
		});

		test("TS-06: handles truncated packed data safely", () => {
			// Only provide partial data (less than one full cell)
			const packed = new Uint8Array([1, 65]); // charLen=1, charData='A', but no width/attrs
			const spans = groupPackedCellsIntoSpans(packed, 5);
			// Should return empty (bounds check fails for minimum 12 bytes)
			expect(spans.length).toBe(0);
		});

		test("TS-01: equivalence with groupCellsIntoSpans", () => {
			// Create a Line with known cells
			const line = new Line(6);
			line.setCell(0, { char: "A", width: 1, attrs: defaultAttrs, dirty: false });
			line.setCell(1, { char: "B", width: 1, attrs: defaultAttrs, dirty: false });
			line.setCell(2, { char: "C", width: 1, attrs: boldAttrs, dirty: false });
			line.setCell(3, { char: "\u3042", width: 2, attrs: boldAttrs, dirty: false });
			line.setCell(4, { char: "", width: 0, attrs: boldAttrs, dirty: false });
			line.setCell(5, { char: "D", width: 1, attrs: boldAttrs, dirty: false });

			const existing = groupCellsIntoSpans(line);

			// Build matching packed data
			const packed = buildPackedRow(
				packCell("A", 1, defaultAttrs),
				packCell("B", 1, defaultAttrs),
				packCell("C", 1, boldAttrs),
				packCell("\u3042", 2, boldAttrs),
				packCell("", 0, boldAttrs),
				packCell("D", 1, boldAttrs),
			);
			const packed_spans = groupPackedCellsIntoSpans(packed, 6);

			// Compare spans
			expect(packed_spans.length).toBe(existing.length);
			for (let i = 0; i < existing.length; i++) {
				expect(packed_spans[i]!.text).toBe(existing[i]!.text);
				expect(packed_spans[i]!.startCol).toBe(existing[i]!.startCol);
				expect(packed_spans[i]!.cellCount).toBe(existing[i]!.cellCount);
				expect(packed_spans[i]!.cells.length).toBe(existing[i]!.cells.length);
				expect(attributesEqual(packed_spans[i]!.attrs, existing[i]!.attrs)).toBe(true);
			}
		});

		test("handles multi-byte UTF-8 inline characters", () => {
			const packed = buildPackedRow(
				packCell("é", 1, defaultAttrs), // 2-byte UTF-8
				packCell("漢", 2, defaultAttrs), // 3-byte UTF-8
				packCell("", 0, defaultAttrs),   // placeholder
			);
			const spans = groupPackedCellsIntoSpans(packed, 3);
			expect(spans.length).toBe(1);
			expect(spans[0]!.text).toBe("é漢");
		});
	});
});
