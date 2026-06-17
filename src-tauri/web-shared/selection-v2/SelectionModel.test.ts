/**
 * Tests for SelectionModel
 */

import { describe, test, expect, mock, beforeEach } from "bun:test";
import { SelectionModel } from "./SelectionModel";
import type { SelectionEvent } from "./types";

describe("SelectionModel", () => {
	let model: SelectionModel;

	beforeEach(() => {
		model = new SelectionModel();
	});

	describe("initial state", () => {
		test("should have no selection initially", () => {
			expect(model.hasSelection()).toBe(false);
			expect(model.getNormalizedRange()).toBeNull();
			expect(model.isActivelySelecting()).toBe(false);
		});

		test("should have mode 'none' initially", () => {
			expect(model.getState().mode).toBe("none");
		});
	});

	describe("startSelection", () => {
		test("should start a character selection", () => {
			model.startSelection({ col: 5, row: 10 }, "char");

			expect(model.hasSelection()).toBe(true);
			expect(model.isActivelySelecting()).toBe(true);
			expect(model.getState().mode).toBe("char");
		});

		test("should set start and end to same position", () => {
			model.startSelection({ col: 5, row: 10 }, "char");

			const range = model.getNormalizedRange();
			expect(range).not.toBeNull();
			expect(range!.start.col).toBe(5);
			expect(range!.start.row).toBe(10);
			expect(range!.end.col).toBe(5);
			expect(range!.end.row).toBe(10);
		});

		test("should emit start event", () => {
			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.startSelection({ col: 5, row: 10 }, "char");

			expect(listener).toHaveBeenCalledTimes(1);
			const event = listener.mock.calls[0][0];
			expect(event.type).toBe("start");
			expect(event.mode).toBe("char");
		});
	});

	describe("updateSelection", () => {
		test("should update end position", () => {
			model.startSelection({ col: 5, row: 10 }, "char");
			model.updateSelection({ col: 20, row: 12 });

			const range = model.getNormalizedRange();
			expect(range!.start.col).toBe(5);
			expect(range!.start.row).toBe(10);
			expect(range!.end.col).toBe(20);
			expect(range!.end.row).toBe(12);
		});

		test("should do nothing if not selecting", () => {
			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.updateSelection({ col: 20, row: 12 });

			expect(listener).not.toHaveBeenCalled();
			expect(model.hasSelection()).toBe(false);
		});

		test("should emit update event", () => {
			model.startSelection({ col: 5, row: 10 }, "char");

			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.updateSelection({ col: 20, row: 12 });

			expect(listener).toHaveBeenCalledTimes(1);
			const event = listener.mock.calls[0][0];
			expect(event.type).toBe("update");
		});
	});

	describe("endSelection", () => {
		test("should stop active selection", () => {
			model.startSelection({ col: 5, row: 10 }, "char");
			model.updateSelection({ col: 20, row: 12 });
			model.endSelection();

			expect(model.hasSelection()).toBe(true);
			expect(model.isActivelySelecting()).toBe(false);
		});

		test("should emit end event", () => {
			model.startSelection({ col: 5, row: 10 }, "char");

			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.endSelection();

			expect(listener).toHaveBeenCalledTimes(1);
			const event = listener.mock.calls[0][0];
			expect(event.type).toBe("end");
		});
	});

	describe("clearSelection", () => {
		test("should clear the selection", () => {
			model.startSelection({ col: 5, row: 10 }, "char");
			model.updateSelection({ col: 20, row: 12 });
			model.endSelection();
			model.clearSelection();

			expect(model.hasSelection()).toBe(false);
			expect(model.getNormalizedRange()).toBeNull();
			expect(model.getState().mode).toBe("none");
		});

		test("should emit clear event", () => {
			model.startSelection({ col: 5, row: 10 }, "char");

			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.clearSelection();

			expect(listener).toHaveBeenCalledTimes(1);
			const event = listener.mock.calls[0][0];
			expect(event.type).toBe("clear");
		});

		test("should not emit if already cleared", () => {
			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.clearSelection();

			expect(listener).not.toHaveBeenCalled();
		});
	});

	describe("getNormalizedRange", () => {
		test("should normalize backwards selection", () => {
			model.startSelection({ col: 20, row: 12 }, "char");
			model.updateSelection({ col: 5, row: 10 });

			const range = model.getNormalizedRange();
			expect(range!.start.col).toBe(5);
			expect(range!.start.row).toBe(10);
			expect(range!.end.col).toBe(20);
			expect(range!.end.row).toBe(12);
		});

		test("should normalize same-row backwards selection", () => {
			model.startSelection({ col: 20, row: 10 }, "char");
			model.updateSelection({ col: 5, row: 10 });

			const range = model.getNormalizedRange();
			expect(range!.start.col).toBe(5);
			expect(range!.end.col).toBe(20);
		});
	});

	describe("setSelection", () => {
		test("should set selection directly", () => {
			model.setSelection(
				{ start: { col: 5, row: 10 }, end: { col: 20, row: 12 } },
				"word"
			);

			expect(model.hasSelection()).toBe(true);
			expect(model.isActivelySelecting()).toBe(false);
			expect(model.getState().mode).toBe("word");
		});

		test("should set isSelecting=false by default (backward compatible)", () => {
			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.setSelection(
				{ start: { col: 5, row: 10 }, end: { col: 20, row: 12 } },
				"word"
			);

			expect(model.isActivelySelecting()).toBe(false);
			// Should emit both start and end events
			expect(listener).toHaveBeenCalledTimes(2);
			expect(listener.mock.calls[0][0].type).toBe("start");
			expect(listener.mock.calls[1][0].type).toBe("end");
		});

		test("should set isSelecting=true when specified", () => {
			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.setSelection(
				{ start: { col: 5, row: 10 }, end: { col: 20, row: 12 } },
				"word",
				true
			);

			expect(model.isActivelySelecting()).toBe(true);
			// Should emit only start event (no end event)
			expect(listener).toHaveBeenCalledTimes(1);
			expect(listener.mock.calls[0][0].type).toBe("start");
		});

		test("should allow drag extension after setSelection with isSelecting=true", () => {
			model.setSelection(
				{ start: { col: 5, row: 10 }, end: { col: 10, row: 10 } },
				"word",
				true
			);

			expect(model.isActivelySelecting()).toBe(true);

			// Should be able to call endSelection later
			model.endSelection();
			expect(model.isActivelySelecting()).toBe(false);
			expect(model.hasSelection()).toBe(true);
		});
	});

	describe("updateSelectionRange", () => {
		test("should update range and emit update event", () => {
			model.setSelection(
				{ start: { col: 5, row: 10 }, end: { col: 10, row: 10 } },
				"word",
				true
			);

			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.updateSelectionRange({
				start: { col: 5, row: 10 },
				end: { col: 20, row: 12 },
			});

			expect(listener).toHaveBeenCalledTimes(1);
			expect(listener.mock.calls[0][0].type).toBe("update");
			expect(listener.mock.calls[0][0].range).toEqual({
				start: { col: 5, row: 10 },
				end: { col: 20, row: 12 },
			});
		});

		test("should preserve mode and isSelecting state", () => {
			model.setSelection(
				{ start: { col: 5, row: 10 }, end: { col: 10, row: 10 } },
				"word",
				true
			);

			model.updateSelectionRange({
				start: { col: 5, row: 10 },
				end: { col: 20, row: 12 },
			});

			expect(model.getState().mode).toBe("word");
			expect(model.isActivelySelecting()).toBe(true);
		});

		test("should do nothing if not actively selecting", () => {
			model.setSelection(
				{ start: { col: 5, row: 10 }, end: { col: 10, row: 10 } },
				"word",
				false // isSelecting=false
			);

			const listener = mock<(event: SelectionEvent) => void>(() => {});
			model.subscribe(listener);

			model.updateSelectionRange({
				start: { col: 5, row: 10 },
				end: { col: 20, row: 12 },
			});

			expect(listener).not.toHaveBeenCalled();
		});
	});

	describe("subscribe/unsubscribe", () => {
		test("should allow unsubscribing", () => {
			const listener = mock<(event: SelectionEvent) => void>(() => {});
			const unsubscribe = model.subscribe(listener);

			model.startSelection({ col: 5, row: 10 }, "char");
			expect(listener).toHaveBeenCalledTimes(1);

			unsubscribe();

			model.updateSelection({ col: 20, row: 12 });
			expect(listener).toHaveBeenCalledTimes(1); // No additional calls
		});
	});
});
