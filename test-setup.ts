/**
 * Test setup file for Bun tests.
 * Configures happy-dom for DOM environment emulation.
 */

import { Window } from "happy-dom";

const window = new Window();

// Register global DOM objects
globalThis.document = window.document as unknown as Document;
globalThis.window = window as unknown as Window & typeof globalThis;
globalThis.KeyboardEvent =
	window.KeyboardEvent as unknown as typeof KeyboardEvent;
globalThis.HTMLElement = window.HTMLElement as unknown as typeof HTMLElement;
globalThis.getComputedStyle = window.getComputedStyle.bind(
	window,
) as typeof getComputedStyle;
globalThis.ResizeObserver =
	window.ResizeObserver as unknown as typeof ResizeObserver;
globalThis.Event = window.Event as unknown as typeof Event;
// WheelEvent may not be available in happy-dom, use Event as fallback
globalThis.WheelEvent = (window.WheelEvent ??
	window.Event) as unknown as typeof WheelEvent;
// Performance API
globalThis.performance = window.performance as unknown as Performance;

// Polyfill requestAnimationFrame for tests
globalThis.requestAnimationFrame = (callback: FrameRequestCallback): number => {
	return setTimeout(() => callback(Date.now()), 0) as unknown as number;
};
globalThis.cancelAnimationFrame = (id: number): void => {
	clearTimeout(id);
};
