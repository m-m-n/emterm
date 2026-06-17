/**
 * Tests for WordBoundary
 */

import { describe, test, expect } from "bun:test";
import { WordBoundary } from "./WordBoundary";

describe("WordBoundary", () => {
	const createBoundary = (lines: string[]) => {
		const cols = Math.max(...lines.map((l) => l.length), 80);
		return new WordBoundary((row) => lines[row] || "", cols);
	};

	describe("isWordSeparator", () => {
		test("should identify spaces as separators", () => {
			const boundary = createBoundary(["test"]);
			expect(boundary.isWordSeparator(" ")).toBe(true);
			expect(boundary.isWordSeparator("\t")).toBe(true);
		});

		test("should identify punctuation as separators", () => {
			const boundary = createBoundary(["test"]);
			expect(boundary.isWordSeparator(".")).toBe(true);
			expect(boundary.isWordSeparator(",")).toBe(true);
			expect(boundary.isWordSeparator("!")).toBe(true);
			expect(boundary.isWordSeparator("/")).toBe(true);
		});

		test("should not identify letters as separators", () => {
			const boundary = createBoundary(["test"]);
			expect(boundary.isWordSeparator("a")).toBe(false);
			expect(boundary.isWordSeparator("Z")).toBe(false);
			expect(boundary.isWordSeparator("0")).toBe(false);
		});

		test("should treat empty string as separator", () => {
			const boundary = createBoundary(["test"]);
			expect(boundary.isWordSeparator("")).toBe(true);
		});
	});

	describe("getWordAt", () => {
		test("should select word at position", () => {
			const boundary = createBoundary(["hello world test"]);
			const range = boundary.getWordAt(7, 0); // 'w' in 'world'

			expect(range.start.col).toBe(6);
			expect(range.end.col).toBe(10);
			expect(range.start.row).toBe(0);
			expect(range.end.row).toBe(0);
		});

		test("should select first word", () => {
			const boundary = createBoundary(["hello world"]);
			const range = boundary.getWordAt(2, 0); // 'l' in 'hello'

			expect(range.start.col).toBe(0);
			expect(range.end.col).toBe(4);
		});

		test("should select last word", () => {
			const boundary = createBoundary(["hello world"]);
			const range = boundary.getWordAt(8, 0); // 'r' in 'world'

			expect(range.start.col).toBe(6);
			expect(range.end.col).toBe(10);
		});

		test("should select single character when on separator", () => {
			const boundary = createBoundary(["hello world"]);
			const range = boundary.getWordAt(5, 0); // space between words

			expect(range.start.col).toBe(5);
			expect(range.end.col).toBe(5);
		});

		test("should select contiguous spaces", () => {
			const boundary = createBoundary(["hello   world"]);
			const range = boundary.getWordAt(6, 0); // middle space

			expect(range.start.col).toBe(5);
			expect(range.end.col).toBe(7);
		});

		test("should handle empty line", () => {
			const boundary = createBoundary([""]);
			const range = boundary.getWordAt(0, 0);

			expect(range.start.col).toBe(0);
			expect(range.end.col).toBe(0);
		});

		test("should handle position beyond line length", () => {
			const boundary = createBoundary(["hi"]);
			const range = boundary.getWordAt(10, 0);

			expect(range.start.col).toBe(10);
			expect(range.end.col).toBe(10);
		});
	});

	describe("getLineAt", () => {
		test("should select entire line", () => {
			const boundary = createBoundary(["hello world", "second line"]);
			const range = boundary.getLineAt(0);

			expect(range.start.col).toBe(0);
			expect(range.end.col).toBe(10); // 'hello world' length - 1
			expect(range.start.row).toBe(0);
			expect(range.end.row).toBe(0);
		});

		test("should handle empty line", () => {
			const boundary = createBoundary([""]);
			const range = boundary.getLineAt(0);

			expect(range.start.col).toBe(0);
			expect(range.end.col).toBe(0);
		});
	});

	describe("expandWordSelection", () => {
		test("should expand forwards", () => {
			const boundary = createBoundary(["hello world test"]);
			const anchorWord = { start: { col: 0, row: 0 }, end: { col: 4, row: 0 } }; // 'hello'

			const expanded = boundary.expandWordSelection(anchorWord, { col: 8, row: 0 }); // in 'world'

			expect(expanded.start.col).toBe(0); // start of 'hello'
			expect(expanded.end.col).toBe(10); // end of 'world'
		});

		test("should expand backwards", () => {
			const boundary = createBoundary(["hello world test"]);
			const anchorWord = { start: { col: 12, row: 0 }, end: { col: 15, row: 0 } }; // 'test'

			const expanded = boundary.expandWordSelection(anchorWord, { col: 2, row: 0 }); // in 'hello'

			expect(expanded.start.col).toBe(0); // start of 'hello'
			expect(expanded.end.col).toBe(15); // end of 'test'
		});
	});

	describe("expandLineSelection", () => {
		test("should expand forwards", () => {
			const boundary = createBoundary([
				"line one",
				"line two",
				"line three",
			]);

			const expanded = boundary.expandLineSelection(0, 2);

			expect(expanded.start.row).toBe(0);
			expect(expanded.end.row).toBe(2);
			expect(expanded.start.col).toBe(0);
		});

		test("should expand backwards", () => {
			const boundary = createBoundary([
				"line one",
				"line two",
				"line three",
			]);

			const expanded = boundary.expandLineSelection(2, 0);

			expect(expanded.start.row).toBe(0);
			expect(expanded.end.row).toBe(2);
		});
	});
});
