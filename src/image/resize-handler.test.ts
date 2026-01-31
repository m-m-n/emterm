/**
 * Resize handler tests.
 *
 * @module image/resize-handler.test
 */

import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";

// Save original globals
const savedWindow = globalThis.window;

// Mock timers
const timeoutCallbacks: Map<number, { fn: () => void; delay: number }> =
	new Map();
let nextTimeoutId = 1;

const mockWindow = {
	setTimeout: mock((fn: () => void, delay: number) => {
		const id = nextTimeoutId++;
		timeoutCallbacks.set(id, { fn, delay });
		return id;
	}),
	clearTimeout: mock((id: number) => {
		timeoutCallbacks.delete(id);
	}),
	requestAnimationFrame: mock((fn: () => void) => {
		fn();
		return 1;
	}),
} as unknown as Window & typeof globalThis;

globalThis.window = mockWindow;

// Helper to advance time and trigger callbacks
function advanceTimers(ms: number): void {
	for (const [id, { fn, delay }] of timeoutCallbacks) {
		if (delay <= ms) {
			timeoutCallbacks.delete(id);
			fn();
		}
	}
}

// Import after mocks
import {
	ResizeCallback,
	ResizeEvent,
	ResizeHandler,
} from "./resize-handler.ts";

describe("ResizeHandler", () => {
	beforeEach(() => {
		globalThis.window = mockWindow;
		timeoutCallbacks.clear();
		nextTimeoutId = 1;
	});

	afterEach(() => {
		globalThis.window = savedWindow;
	});

	describe("constructor", () => {
		test("creates handler with default debounce time", () => {
			const handler = new ResizeHandler();
			expect(handler.getDebounceTime()).toBe(100);
			handler.dispose();
		});

		test("creates handler with custom debounce time", () => {
			const handler = new ResizeHandler({ debounceMs: 200 });
			expect(handler.getDebounceTime()).toBe(200);
			handler.dispose();
		});
	});

	describe("onResize", () => {
		test("registers callback", () => {
			const handler = new ResizeHandler();
			const callback = mock(() => {});

			handler.onResize(callback);
			// Callback should be registered

			handler.dispose();
		});

		test("returns unsubscribe function", () => {
			const handler = new ResizeHandler();
			const callback = mock(() => {});

			const unsubscribe = handler.onResize(callback);
			expect(typeof unsubscribe).toBe("function");

			handler.dispose();
		});
	});

	describe("handleResize", () => {
		test("debounces rapid resize events", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);

			// Rapid resize events
			handler.handleResize({ width: 100, height: 100 });
			handler.handleResize({ width: 110, height: 110 });
			handler.handleResize({ width: 120, height: 120 });

			// Should not have called callback yet
			expect(callback).not.toHaveBeenCalled();

			// Advance time
			advanceTimers(100);

			// Should have called callback once with final dimensions
			expect(callback).toHaveBeenCalledTimes(1);
			expect(callback).toHaveBeenCalledWith({ width: 120, height: 120 });

			handler.dispose();
		});

		test("does not trigger callback if dimensions unchanged", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);

			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(100);

			expect(callback).toHaveBeenCalledTimes(1);

			// Same dimensions
			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(100);

			// Should not trigger again
			expect(callback).toHaveBeenCalledTimes(1);

			handler.dispose();
		});

		test("triggers callback on dimension change", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);

			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(100);

			handler.handleResize({ width: 200, height: 200 });
			advanceTimers(100);

			expect(callback).toHaveBeenCalledTimes(2);

			handler.dispose();
		});
	});

	describe("multiple callbacks", () => {
		test("notifies all registered callbacks", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback1 = mock(() => {});
			const callback2 = mock(() => {});

			handler.onResize(callback1);
			handler.onResize(callback2);

			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(100);

			expect(callback1).toHaveBeenCalledTimes(1);
			expect(callback2).toHaveBeenCalledTimes(1);

			handler.dispose();
		});

		test("unsubscribe removes only specific callback", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback1 = mock(() => {});
			const callback2 = mock(() => {});

			const unsubscribe1 = handler.onResize(callback1);
			handler.onResize(callback2);

			unsubscribe1();

			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(100);

			expect(callback1).not.toHaveBeenCalled();
			expect(callback2).toHaveBeenCalledTimes(1);

			handler.dispose();
		});
	});

	describe("setDebounceTime", () => {
		test("updates debounce time", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });

			handler.setDebounceTime(200);
			expect(handler.getDebounceTime()).toBe(200);

			handler.dispose();
		});

		test("applies new debounce time to next resize", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);
			handler.setDebounceTime(50);

			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(50);

			expect(callback).toHaveBeenCalledTimes(1);

			handler.dispose();
		});
	});

	describe("cancel", () => {
		test("cancels pending resize callback", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);

			handler.handleResize({ width: 100, height: 100 });
			handler.cancel();

			advanceTimers(100);

			expect(callback).not.toHaveBeenCalled();

			handler.dispose();
		});
	});

	describe("flush", () => {
		test("immediately triggers pending callback", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);

			handler.handleResize({ width: 100, height: 100 });
			handler.flush();

			expect(callback).toHaveBeenCalledTimes(1);

			handler.dispose();
		});

		test("does nothing if no pending resize", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);

			handler.flush();

			expect(callback).not.toHaveBeenCalled();

			handler.dispose();
		});
	});

	describe("getLastDimensions", () => {
		test("returns null initially", () => {
			const handler = new ResizeHandler();
			expect(handler.getLastDimensions()).toBeNull();
			handler.dispose();
		});

		test("returns last processed dimensions", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);

			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(100);

			expect(handler.getLastDimensions()).toEqual({ width: 100, height: 100 });

			handler.dispose();
		});
	});

	describe("dispose", () => {
		test("cancels pending callbacks", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);
			handler.handleResize({ width: 100, height: 100 });

			handler.dispose();

			advanceTimers(100);

			expect(callback).not.toHaveBeenCalled();
		});

		test("removes all registered callbacks", () => {
			const handler = new ResizeHandler({ debounceMs: 100 });
			const callback = mock(() => {});

			handler.onResize(callback);
			handler.dispose();

			// Should not throw when trying to handle resize after dispose
			handler.handleResize({ width: 100, height: 100 });
			advanceTimers(100);

			expect(callback).not.toHaveBeenCalled();
		});
	});
});
