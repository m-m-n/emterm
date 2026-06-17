/**
 * Tests for selection types utilities
 */

import { describe, test, expect } from "bun:test";
import { normalizeRange, isPositionInRange } from "./types";

describe("normalizeRange", () => {
	test("should return same range if already normalized", () => {
		const range = {
			start: { col: 5, row: 10 },
			end: { col: 20, row: 12 },
		};
		const normalized = normalizeRange(range);

		expect(normalized.start.col).toBe(5);
		expect(normalized.start.row).toBe(10);
		expect(normalized.end.col).toBe(20);
		expect(normalized.end.row).toBe(12);
	});

	test("should swap if start row > end row", () => {
		const range = {
			start: { col: 5, row: 12 },
			end: { col: 20, row: 10 },
		};
		const normalized = normalizeRange(range);

		expect(normalized.start.row).toBe(10);
		expect(normalized.end.row).toBe(12);
	});

	test("should swap if same row but start col > end col", () => {
		const range = {
			start: { col: 20, row: 10 },
			end: { col: 5, row: 10 },
		};
		const normalized = normalizeRange(range);

		expect(normalized.start.col).toBe(5);
		expect(normalized.end.col).toBe(20);
	});
});

describe("isPositionInRange", () => {
	describe("single row range", () => {
		const range = {
			start: { col: 5, row: 10 },
			end: { col: 15, row: 10 },
		};

		test("should return true for position in range", () => {
			expect(isPositionInRange({ col: 10, row: 10 }, range)).toBe(true);
		});

		test("should return true for start position", () => {
			expect(isPositionInRange({ col: 5, row: 10 }, range)).toBe(true);
		});

		test("should return true for end position", () => {
			expect(isPositionInRange({ col: 15, row: 10 }, range)).toBe(true);
		});

		test("should return false for position before range", () => {
			expect(isPositionInRange({ col: 2, row: 10 }, range)).toBe(false);
		});

		test("should return false for position after range", () => {
			expect(isPositionInRange({ col: 20, row: 10 }, range)).toBe(false);
		});

		test("should return false for different row", () => {
			expect(isPositionInRange({ col: 10, row: 5 }, range)).toBe(false);
		});
	});

	describe("multi-row range", () => {
		const range = {
			start: { col: 10, row: 5 },
			end: { col: 20, row: 8 },
		};

		test("should return true for position in first row after start col", () => {
			expect(isPositionInRange({ col: 15, row: 5 }, range)).toBe(true);
			expect(isPositionInRange({ col: 50, row: 5 }, range)).toBe(true);
		});

		test("should return false for position in first row before start col", () => {
			expect(isPositionInRange({ col: 5, row: 5 }, range)).toBe(false);
		});

		test("should return true for position in middle row", () => {
			expect(isPositionInRange({ col: 0, row: 6 }, range)).toBe(true);
			expect(isPositionInRange({ col: 50, row: 7 }, range)).toBe(true);
		});

		test("should return true for position in last row before end col", () => {
			expect(isPositionInRange({ col: 10, row: 8 }, range)).toBe(true);
			expect(isPositionInRange({ col: 0, row: 8 }, range)).toBe(true);
		});

		test("should return false for position in last row after end col", () => {
			expect(isPositionInRange({ col: 25, row: 8 }, range)).toBe(false);
		});

		test("should return false for row before range", () => {
			expect(isPositionInRange({ col: 15, row: 3 }, range)).toBe(false);
		});

		test("should return false for row after range", () => {
			expect(isPositionInRange({ col: 15, row: 10 }, range)).toBe(false);
		});
	});
});
