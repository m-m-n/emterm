import { beforeEach, describe, expect, test } from "bun:test";
import { SelectionManager } from "./manager";

describe("SelectionManager", () => {
	let manager: SelectionManager;

	beforeEach(() => {
		manager = new SelectionManager();
	});

	describe("initialization", () => {
		test("starts with no active selection", () => {
			expect(manager.isActive()).toBe(false);
		});

		test("getSelection returns null when not active", () => {
			expect(manager.getSelection()).toBeNull();
		});
	});

	describe("startSelection", () => {
		test("activates selection with start and end at same position", () => {
			manager.startSelection(5, 10);
			expect(manager.isActive()).toBe(true);
			const selection = manager.getSelection();
			expect(selection).not.toBeNull();
			expect(selection?.start).toEqual({ col: 5, row: 10 });
			expect(selection?.end).toEqual({ col: 5, row: 10 });
		});

		test("replaces existing selection", () => {
			manager.startSelection(5, 10);
			manager.startSelection(8, 12);
			const selection = manager.getSelection();
			expect(selection?.start).toEqual({ col: 8, row: 12 });
			expect(selection?.end).toEqual({ col: 8, row: 12 });
		});

		test("handles column 0, row 0", () => {
			manager.startSelection(0, 0);
			expect(manager.isActive()).toBe(true);
			const selection = manager.getSelection();
			expect(selection?.start).toEqual({ col: 0, row: 0 });
		});
	});

	describe("updateSelection", () => {
		test("updates end position while keeping start unchanged", () => {
			manager.startSelection(5, 10);
			manager.updateSelection(15, 12);
			const selection = manager.getSelection();
			expect(selection?.start).toEqual({ col: 5, row: 10 });
			expect(selection?.end).toEqual({ col: 15, row: 12 });
		});

		test("allows backward selection (end before start)", () => {
			manager.startSelection(15, 12);
			manager.updateSelection(5, 10);
			const selection = manager.getSelection();
			expect(selection?.start).toEqual({ col: 15, row: 12 });
			expect(selection?.end).toEqual({ col: 5, row: 10 });
		});

		test("does nothing when no selection active", () => {
			manager.updateSelection(10, 10);
			expect(manager.isActive()).toBe(false);
		});

		test("handles multiple updates", () => {
			manager.startSelection(0, 0);
			manager.updateSelection(5, 5);
			manager.updateSelection(10, 10);
			manager.updateSelection(15, 15);
			const selection = manager.getSelection();
			expect(selection?.end).toEqual({ col: 15, row: 15 });
		});
	});

	describe("clearSelection", () => {
		test("deactivates selection", () => {
			manager.startSelection(5, 10);
			manager.clearSelection();
			expect(manager.isActive()).toBe(false);
			expect(manager.getSelection()).toBeNull();
		});

		test("is safe to call when no selection active", () => {
			manager.clearSelection();
			expect(manager.isActive()).toBe(false);
		});

		test("allows starting new selection after clear", () => {
			manager.startSelection(5, 10);
			manager.clearSelection();
			manager.startSelection(8, 12);
			expect(manager.isActive()).toBe(true);
			const selection = manager.getSelection();
			expect(selection?.start).toEqual({ col: 8, row: 12 });
		});
	});

	describe("normalizeSelection", () => {
		test("keeps forward selection unchanged", () => {
			manager.startSelection(5, 10);
			manager.updateSelection(15, 12);
			const normalized = manager.normalizeSelection();
			expect(normalized.start).toEqual({ col: 5, row: 10 });
			expect(normalized.end).toEqual({ col: 15, row: 12 });
		});

		test("swaps start and end for backward row selection", () => {
			manager.startSelection(15, 12);
			manager.updateSelection(5, 10);
			const normalized = manager.normalizeSelection();
			expect(normalized.start).toEqual({ col: 5, row: 10 });
			expect(normalized.end).toEqual({ col: 15, row: 12 });
		});

		test("swaps start and end for backward column selection on same row", () => {
			manager.startSelection(15, 10);
			manager.updateSelection(5, 10);
			const normalized = manager.normalizeSelection();
			expect(normalized.start).toEqual({ col: 5, row: 10 });
			expect(normalized.end).toEqual({ col: 15, row: 10 });
		});

		test("handles single-cell selection", () => {
			manager.startSelection(5, 10);
			const normalized = manager.normalizeSelection();
			expect(normalized.start).toEqual({ col: 5, row: 10 });
			expect(normalized.end).toEqual({ col: 5, row: 10 });
		});

		test("throws error when no selection active", () => {
			expect(() => manager.normalizeSelection()).toThrow();
		});

		test("handles selection at grid origin", () => {
			manager.startSelection(0, 0);
			manager.updateSelection(5, 5);
			const normalized = manager.normalizeSelection();
			expect(normalized.start).toEqual({ col: 0, row: 0 });
			expect(normalized.end).toEqual({ col: 5, row: 5 });
		});

		test("normalizes complex backward selection", () => {
			manager.startSelection(20, 20);
			manager.updateSelection(0, 0);
			const normalized = manager.normalizeSelection();
			expect(normalized.start).toEqual({ col: 0, row: 0 });
			expect(normalized.end).toEqual({ col: 20, row: 20 });
		});
	});

	describe("edge cases", () => {
		test("handles very large coordinates", () => {
			manager.startSelection(9999, 9999);
			manager.updateSelection(10000, 10000);
			expect(manager.isActive()).toBe(true);
		});

		test("handles rapid start/clear cycles", () => {
			for (let i = 0; i < 100; i++) {
				manager.startSelection(i, i);
				manager.clearSelection();
			}
			expect(manager.isActive()).toBe(false);
		});
	});
});
