import { describe, expect, test } from "bun:test";
import { coordsToGrid } from "./coords";

describe("coordsToGrid", () => {
	test("converts valid pixel coordinates to grid position", () => {
		const result = coordsToGrid(100, 50, 10, 20, 80, 40);
		expect(result).toEqual({ col: 10, row: 2 });
	});

	test("handles coordinates at grid origin", () => {
		const result = coordsToGrid(0, 0, 10, 20, 80, 40);
		expect(result).toEqual({ col: 0, row: 0 });
	});

	test("clamps column to maximum bounds", () => {
		const result = coordsToGrid(1000, 50, 10, 20, 80, 40);
		expect(result).toEqual({ col: 79, row: 2 });
	});

	test("clamps row to maximum bounds", () => {
		const result = coordsToGrid(100, 1000, 10, 20, 80, 40);
		expect(result).toEqual({ col: 10, row: 39 });
	});

	test("clamps negative column coordinates to zero", () => {
		const result = coordsToGrid(-50, 50, 10, 20, 80, 40);
		expect(result).toEqual({ col: 0, row: 2 });
	});

	test("clamps negative row coordinates to zero", () => {
		const result = coordsToGrid(100, -50, 10, 20, 80, 40);
		expect(result).toEqual({ col: 10, row: 0 });
	});

	test("handles different character dimensions", () => {
		const result = coordsToGrid(80, 60, 8, 16, 80, 40);
		expect(result).toEqual({ col: 10, row: 3 });
	});

	test("handles fractional pixel positions", () => {
		const result = coordsToGrid(105, 55, 10, 20, 80, 40);
		expect(result).toEqual({ col: 10, row: 2 });
	});

	test("clamps both coordinates when out of bounds", () => {
		const result = coordsToGrid(-10, 2000, 10, 20, 80, 40);
		expect(result).toEqual({ col: 0, row: 39 });
	});

	test("handles edge case at max column boundary", () => {
		const result = coordsToGrid(790, 50, 10, 20, 80, 40);
		expect(result).toEqual({ col: 79, row: 2 });
	});

	test("handles edge case at max row boundary", () => {
		const result = coordsToGrid(100, 780, 10, 20, 80, 40);
		expect(result).toEqual({ col: 10, row: 39 });
	});

	test("handles very small character dimensions", () => {
		const result = coordsToGrid(10, 10, 2, 4, 80, 40);
		expect(result).toEqual({ col: 5, row: 2 });
	});

	test("handles very large character dimensions", () => {
		const result = coordsToGrid(200, 200, 20, 40, 80, 40);
		expect(result).toEqual({ col: 10, row: 5 });
	});
});
